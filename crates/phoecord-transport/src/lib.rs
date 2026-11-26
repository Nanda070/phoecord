//! The transport boundary: one entry between whatever carries Discord's traffic and the plugin
//! chain.
//!
//! [`Engine`] owns the plugin [`Chain`], the host state ([`Shared`]: account facts, KV store,
//! rate limits, timers, strings) and the live [`Config`]. A transport (the local proxy today, the
//! in-process hooks of the loader on Phoenix) feeds it the four events and acts on what it gives
//! back:
//!
//! | Event | Method | Gives back |
//! |:---|:---|:---|
//! | REST request | [`Engine::on_request`] | [`RequestOutcome`] (forward, changed, or a local reply) + [`Effects`] |
//! | REST response | [`Engine::on_response`] | whether the body was rewritten + [`Effects`] |
//! | Gateway dispatch (server → client) | [`Engine::on_dispatch`] | [`DispatchResult`]: payload, extra payloads, verdict, queued sends |
//! | Gateway send (client → server) | [`Engine::on_send`] | [`SendResult`]: verdict, queued sends, local payloads |
//!
//! Plus [`Engine::chain_queued`] (run queued sends through the Send chain), [`Engine::fire_timer`]
//! with [`timers::Timers::next_due`] / [`timers::Timers::take_due`] (timer ticks), and
//! [`Engine::apply`] / [`Engine::sync_packages`] (config and packages).
//!
//! What a transport supplies: the [`host::Network`] behind `Host::rest` and `Host::fetch` (the
//! client's session headers, TLS, an upstream proxy), a clock that calls [`Engine::fire_timer`],
//! and delivery of [`Effects`]: `locals` go to the client as server → client payloads, `frames`
//! go to the server as client → server frames. Nothing here knows about sockets, TLS or HTTP.

pub mod account;
pub mod config;
pub mod dev;
pub mod full_ready;
pub mod host;
pub mod timers;

use config::Config;
use host::{ProxyHost, Shared};
use phoecord_core::{EventKind, TIMER_EVENT, Val};
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::{Chain, Outcome};
use std::sync::{Arc, Mutex, RwLock};

/// What the chain decided about a REST request.
#[derive(Debug, PartialEq)]
pub enum RequestOutcome {
    /// Forward it; `changed` says the body must be re-serialized.
    Forward { changed: bool },
    /// Answer locally, the request never reaches Discord.
    Reply { status: u16, body: Option<Val> },
}

/// What the chain decided about a server → client gateway payload.
#[derive(Debug)]
pub struct DispatchResult {
    pub payload: Val,
    pub extra: Vec<Val>,
    pub changed: bool,
    pub drop: bool,
    /// Client → server payloads plugins queued with `Host::emit_send`.
    pub sends: Vec<(i64, Val)>,
}

/// What the chain decided about a client → server gateway payload.
#[derive(Debug)]
pub struct SendResult {
    pub changed: bool,
    pub drop: bool,
    pub sends: Vec<(i64, Val)>,
    /// Server → client payloads plugins queued with `Host::emit_dispatch`.
    pub locals: Vec<Val>,
}

/// What plugins queued outside a gateway frame (a REST event, a timer): the transport delivers it
/// at once.
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    /// Server → client payloads (`Host::emit_dispatch`): to the client's gateway connection.
    pub locals: Vec<Val>,
    /// Client → server frames (`{"op":…,"d":…}`), already through the Send chain: to the newest
    /// live gateway connection (none: dropped).
    pub frames: Vec<Val>,
}

pub struct Engine {
    pub shared: Shared,
    pub chain: Chain,
    config: RwLock<Arc<Config>>,
    /// Keeps the chain's dynamic segment in step with `data/plugins/` (WASM packages).
    pub(crate) packages: Mutex<Option<PackageSet>>,
}

impl Engine {
    /// An engine for `chain` with `shared` host state, running `cfg`.
    pub fn new(chain: Chain, shared: Shared, cfg: Config) -> Engine {
        let engine = Engine {
            shared,
            chain,
            config: RwLock::new(Arc::new(Config::default())),
            packages: Mutex::new(None),
        };
        engine.apply(cfg);
        // The dev console (1.3.9) hears about every plugin run; with nobody attached this is a
        // timestamp and an atomic load.
        let bus = engine.shared.dev.clone();
        engine.chain.set_observer(Some(Arc::new(move |r| {
            bus.run(r.kind, r.plugin, &r.verdict, r.elapsed);
        })));
        engine
    }

    pub fn cfg(&self) -> Arc<Config> {
        self.config.read().unwrap().clone()
    }

    /// Make `cfg` the live config and switch the chain to the plugins it enables.
    pub fn apply(&self, cfg: Config) {
        self.set_enabled_from(&cfg);
        *self.config.write().unwrap() = Arc::new(cfg);
    }

    fn set_enabled_from(&self, cfg: &Config) {
        let enabled: Vec<String> = self
            .chain
            .ids()
            .into_iter()
            .filter(|id| cfg.is_enabled(id))
            .collect();
        self.chain.set_enabled(enabled);
    }

    /// Follow `data/plugins/` with `packages`: load what is new now and on every later
    /// [`Engine::sync_packages`]. Returns whether anything changed now.
    pub fn watch_packages(&self, packages: PackageSet) -> bool {
        *self.packages.lock().unwrap() = Some(packages);
        self.sync_packages()
    }

    /// Load new packages and unload removed ones, then switch the chain to what the config
    /// enables. Returns whether anything changed (a transport that replays state through the chain
    /// on a change does so then). Does nothing when no packages are watched or nothing changed.
    /// Compiling a WASM package takes a while, so call this off an async executor's threads.
    pub fn sync_packages(&self) -> bool {
        let logs = match self.packages.lock().unwrap().as_mut() {
            Some(set) => set.sync(&self.chain),
            None => return false,
        };
        if logs.is_empty() {
            return false;
        }
        for line in &logs {
            tracing::info!("{line}");
        }
        self.set_enabled_from(&self.cfg());
        true
    }

    /// Whether any enabled plugin wants gateway payloads.
    pub fn wants_gateway(&self) -> bool {
        self.chain.wants_gateway()
    }

    pub fn wants_request(&self, method: &str, route: &str) -> bool {
        self.chain.wants(&EventKind::RestRequest { method, route })
    }

    pub fn wants_response(&self, method: &str, route: &str) -> bool {
        self.chain.wants(&EventKind::RestResponse { method, route })
    }

    /// A REST request: plugins may rewrite `body`, answer it themselves or let it through.
    pub fn on_request(
        &self,
        method: &str,
        route: &str,
        body: &mut Option<Val>,
    ) -> (RequestOutcome, Effects) {
        self.on_request_with_query(method, route, None, body)
    }

    /// [`Engine::on_request`] for a call with a query string (`q=cat`, no leading `?`), which
    /// plugins read with `Host::request_query`. `route` stays the path alone.
    pub fn on_request_with_query(
        &self,
        method: &str,
        route: &str,
        query: Option<&str>,
        body: &mut Option<Val>,
    ) -> (RequestOutcome, Effects) {
        let cfg = self.cfg();
        self.shared
            .dev
            .remember("rest_request", format!("{method} {route}"), || {
                dev::Replay::RestRequest {
                    method: method.to_string(),
                    route: route.to_string(),
                    body: body.clone(),
                }
            });
        self.shared
            .account
            .lock()
            .unwrap()
            .record_request(method, route, body.as_ref());
        let mut host = ProxyHost::new(&self.shared, &cfg).with_query(query);
        let outcome = self
            .chain
            .rest_request(&mut host, method, route, body.as_mut());
        let effects = self.finish(host);
        let outcome = match outcome {
            Outcome::Untouched => RequestOutcome::Forward { changed: false },
            Outcome::Changed => RequestOutcome::Forward { changed: true },
            Outcome::Replied { by, status, body } => {
                tracing::debug!("{by}: answered {method} {route} locally ({status})");
                RequestOutcome::Reply { status, body }
            }
            Outcome::Dropped { by } => {
                tracing::debug!("{by}: dropped {method} {route}");
                RequestOutcome::Reply {
                    status: 204,
                    body: None,
                }
            }
        };
        (outcome, effects)
    }

    /// A REST answer: plugins may rewrite `body`; returns whether they did.
    pub fn on_response(
        &self,
        method: &str,
        route: &str,
        status: u16,
        body: &mut Val,
    ) -> (bool, Effects) {
        self.on_response_with_query(method, route, None, status, body)
    }

    /// [`Engine::on_response`] for a call with a query string, as [`Engine::on_request_with_query`].
    pub fn on_response_with_query(
        &self,
        method: &str,
        route: &str,
        query: Option<&str>,
        status: u16,
        body: &mut Val,
    ) -> (bool, Effects) {
        let cfg = self.cfg();
        self.shared
            .dev
            .remember("rest_response", format!("{method} {route}"), || {
                dev::Replay::RestResponse {
                    method: method.to_string(),
                    route: route.to_string(),
                    status,
                    body: body.clone(),
                }
            });
        let mut host = ProxyHost::new(&self.shared, &cfg).with_query(query);
        let outcome = self
            .chain
            .rest_response(&mut host, method, route, status, body);
        let effects = self.finish(host);
        (outcome == Outcome::Changed, effects)
    }

    /// A server → client payload: learn from it, then let plugins change it.
    pub fn on_dispatch(&self, mut payload: Val) -> DispatchResult {
        let cfg = self.cfg();
        let mut extra = Vec::new();
        let name = if payload.get("op").and_then(Val::as_i64) == Some(0) {
            payload.str_at(&["t"]).map(str::to_string)
        } else {
            None
        };
        let Some(name) = name else {
            return DispatchResult {
                payload,
                extra,
                changed: false,
                drop: false,
                sends: Vec::new(),
            };
        };
        self.shared
            .dev
            .remember("dispatch", name.clone(), || dev::Replay::Dispatch {
                name: name.clone(),
                payload: payload.clone(),
            });
        if let Some(d) = payload.get("d") {
            self.shared.account.lock().unwrap().track(&name, d);
            if name == "READY" {
                let (partial, total) = full_ready::partial_guilds(d);
                if partial > 0 {
                    tracing::info!("READY: {partial} of {total} guilds arrived partial");
                } else {
                    tracing::debug!("READY: {total} guilds, all full");
                }
            }
        }
        let mut host = ProxyHost::new(&self.shared, &cfg);
        let outcome = self
            .chain
            .dispatch(&mut host, &name, &mut payload, &mut extra);
        let (changed, drop) = match outcome {
            Outcome::Untouched => (false, false),
            Outcome::Changed => (true, false),
            Outcome::Dropped { .. } | Outcome::Replied { .. } => (false, true),
        };
        extra.append(&mut host.locals);
        DispatchResult {
            payload,
            extra,
            changed,
            drop,
            sends: host.sends,
        }
    }

    /// A client → server payload.
    pub fn on_send(&self, payload: &mut Val) -> SendResult {
        let cfg = self.cfg();
        let Some(op) = payload.get("op").and_then(Val::as_i64) else {
            return SendResult {
                changed: false,
                drop: false,
                sends: Vec::new(),
                locals: Vec::new(),
            };
        };
        self.shared
            .dev
            .remember("send", format!("op {op}"), || dev::Replay::Send {
                op,
                payload: payload.clone(),
            });
        // Before the plugins, so they see the IDENTIFY that is actually sent.
        let full = cfg.phoenix.full_ready && {
            let dropped = full_ready::ask_full_ready(payload);
            if let Some(n) = dropped {
                tracing::info!(
                    "IDENTIFY: asked for a full READY ({n} cached guild versions dropped)"
                );
            }
            dropped.is_some()
        };
        let mut host = ProxyHost::new(&self.shared, &cfg);
        let outcome = self.chain.send(&mut host, op, payload);
        SendResult {
            changed: full || outcome == Outcome::Changed,
            drop: matches!(outcome, Outcome::Dropped { .. } | Outcome::Replied { .. }),
            sends: host.sends,
            locals: host.locals,
        }
    }

    /// A due timer: a [`TIMER_EVENT`] dispatch for `plugin` alone (skipped when it is off or does
    /// not want it). What it queues is returned for the transport to deliver at once.
    pub fn fire_timer(&self, plugin: &str, tag: &str) -> Effects {
        let cfg = self.cfg();
        let mut payload = timers::timer_payload(tag);
        let mut extra = Vec::new();
        let mut host = ProxyHost::new(&self.shared, &cfg);
        let ran = self
            .chain
            .dispatch_to(&mut host, plugin, TIMER_EVENT, &mut payload, &mut extra);
        if ran.is_none() {
            tracing::debug!(
                plugin,
                tag,
                "timer skipped: the plugin is off or not listening"
            );
        }
        extra.append(&mut host.locals);
        host.locals = extra;
        self.finish(host)
    }

    /// Run sends plugins queued with `Host::emit_send` through the Send chain once and return
    /// the frames (`{"op":…,"d":…}`) to send, in order, with the local payloads plugins queued on
    /// the way. A send the chain drops is left out; sends plugins queue while this runs follow the
    /// frame as they are (never chained again, so a plugin cannot loop).
    pub fn chain_queued(&self, sends: Vec<(i64, Val)>) -> Effects {
        let mut out = Effects::default();
        if sends.is_empty() {
            return out;
        }
        let cfg = self.cfg();
        for (op, payload) in sends {
            let mut frame = queued_frame(op, payload);
            let mut host = ProxyHost::new(&self.shared, &cfg);
            let outcome = self.chain.send(&mut host, op, &mut frame);
            out.locals.append(&mut host.locals);
            if !matches!(outcome, Outcome::Dropped { .. } | Outcome::Replied { .. }) {
                out.frames.push(frame);
            }
            out.frames
                .extend(host.sends.into_iter().map(|(op, p)| queued_frame(op, p)));
        }
        out
    }

    /// The effects of one event outside a gateway frame: its local payloads, and its queued
    /// sends through the Send chain.
    fn finish(&self, mut host: ProxyHost<'_>) -> Effects {
        let mut effects = Effects {
            locals: std::mem::take(&mut host.locals),
            frames: Vec::new(),
        };
        let mut chained = self.chain_queued(host.sends);
        effects.locals.append(&mut chained.locals);
        effects.frames = chained.frames;
        effects
    }
}

/// A payload a plugin queued with `Host::emit_send`, as a gateway frame: `{"op": op, "d": payload}`.
pub fn queued_frame(op: i64, payload: Val) -> Val {
    let mut v = Val::map();
    v.set("op", Val::Int(op));
    v.set("d", payload);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let dir = std::env::temp_dir().join(format!("phoecord-test-engine-{}", std::process::id()));
        let chain = Chain::new(vec![Box::new(phoecord_plugins::no_track::NoTrack::new())]);
        Engine::new(chain, Shared::new(dir), Config::default())
    }

    #[test]
    fn the_engine_runs_the_four_events_without_any_transport() {
        let e = engine();
        assert!(e.wants_request("POST", "/science"));
        let mut body = None;
        let (outcome, effects) = e.on_request("POST", "/science", &mut body);
        assert_eq!(
            outcome,
            RequestOutcome::Reply {
                status: 204,
                body: None
            }
        );
        assert_eq!(effects, Effects::default());
        let (outcome, _) = e.on_request("GET", "/users/@me", &mut body);
        assert_eq!(outcome, RequestOutcome::Forward { changed: false });
        let mut resp = Val::map();
        assert!(!e.on_response("GET", "/users/@me", 200, &mut resp).0);
        let r = e.on_dispatch(Val::map());
        assert!(!r.changed && !r.drop);
        let mut frame = queued_frame(3, Val::map());
        assert!(!e.on_send(&mut frame).changed);
        assert_eq!(
            e.chain_queued(vec![(3, Val::map())]).frames,
            vec![queued_frame(3, Val::map())]
        );
    }

    #[test]
    fn plugins_have_no_network_unless_the_transport_supplies_one() {
        use phoecord_core::{ChainHost, Host};
        let e = engine();
        let cfg = e.cfg();
        let mut h = ProxyHost::new(&e.shared, &cfg);
        h.begin_plugin("p");
        assert!(h.rest("GET", "/users/@me", None).is_err());
    }
}
