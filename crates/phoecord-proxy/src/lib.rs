//! PhoeCord network layer: the local MITM proxy between the client and Discord.
//!
//! [`Proxy`] is one transport of the plugin chain: it owns the TLS setup and the sockets and hands
//! every event to the transport-neutral [`Engine`] (`phoecord-transport`), which owns the config,
//! the account state and the plugin [`phoecord_plugin_runtime::Chain`]. [`server`] accepts
//! `CONNECT`, [`rest`] and [`gateway`] run Discord's traffic through the engine.

pub mod ca;
pub mod codec;
pub mod egress;
pub mod gateway;
pub mod hostnet;
pub mod ipc;
pub mod live;
pub mod process;
pub mod rest;
pub mod server;
pub mod timers;
pub mod trust;

pub use phoecord_transport::{DispatchResult, Effects, Engine, RequestOutcome, SendResult};
pub use phoecord_transport::{account, config, host, queued_frame};

use anyhow::Result;
use ca::Authority;
use config::{Config, Paths};
use host::Shared;
use hostnet::Net;
use live::LiveGateways;
use phoecord_core::Val;
use phoecord_plugin_runtime::Chain;
use phoecord_plugin_runtime::package::PackageSet;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, watch};

#[derive(Default)]
pub struct Stats {
    pub decoded: AtomicU64,
    pub undecodable: AtomicU64,
}

pub struct Proxy {
    pub paths: Paths,
    /// The transport boundary: config, account state, host state and the plugin chain. The proxy
    /// reaches all of it through this; `Proxy` derefs to it.
    pub engine: Engine,
    /// The client's Discord session and HTTPS for `Host::rest` / `Host::fetch`: what this
    /// transport supplies to the engine.
    pub net: Arc<Net>,
    pub ca: Arc<Authority>,
    pub server_tls: Arc<rustls::ServerConfig>,
    pub upstream_tls: Arc<rustls::ClientConfig>,
    /// Bumped on every config reload.
    pub reloads: watch::Sender<u64>,
    /// Server → client payloads made locally (`Host::emit_dispatch` from a REST event); every
    /// gateway connection sends what it receives to its client.
    pub locals: broadcast::Sender<Val>,
    /// Bridged gateway connections, for payloads plugins queue outside a gateway frame.
    pub live: LiveGateways,
    /// Whether the timer driver runs (it is started once, by [`Proxy::spawn_timers`]).
    timers_started: AtomicBool,
    /// Decode (and re-encode) the gateway even when no plugin needs it (self-test).
    pub force_decode: bool,
    pub stats: Stats,
}

impl Deref for Proxy {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

pub fn upstream_tls(
    extra_root: Option<rustls::pki_types::CertificateDer<'static>>,
) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(r) = extra_root {
        let _ = roots.add(r);
    }
    let mut c = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(c)
}

impl Proxy {
    pub fn new(paths: Paths, cfg: Config, chain: Chain, force_decode: bool) -> Result<Arc<Proxy>> {
        let ca = Arc::new(Authority::load_or_create(&paths.data)?);
        let server_tls = ca.server_config();
        let net = Arc::new(Net::new());
        let mut shared = Shared::with_network(paths.data.join("plugins"), net.clone());
        shared.config_path = paths.config.clone();
        Ok(Arc::new(Proxy {
            paths,
            engine: Engine::new(chain, shared, cfg),
            net,
            ca,
            server_tls,
            upstream_tls: upstream_tls(None),
            reloads: watch::channel(0).0,
            locals: broadcast::channel(64).0,
            live: LiveGateways::default(),
            timers_started: AtomicBool::new(false),
            force_decode,
            stats: Stats::default(),
        }))
    }

    /// The `upstream` proxy from the live config; `None` means connect directly.
    pub fn upstream_route(&self) -> anyhow::Result<Option<egress::Upstream>> {
        self.cfg()
            .upstream
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(egress::Upstream::parse)
            .transpose()
    }

    /// Follow `data/plugins/` with `packages`: load what is new now and on every later
    /// [`Proxy::sync_packages`].
    pub fn watch_packages(&self, packages: PackageSet) {
        if self.engine.watch_packages(packages) {
            self.reloads.send_modify(|n| *n += 1);
        }
    }

    /// Load new packages and unload removed ones, then switch the chain to what the config
    /// enables. Compiling a WASM package takes a while, so call this off the async executor's
    /// threads.
    pub fn sync_packages(&self) {
        if self.engine.sync_packages() {
            // Gateway tasks replay their state through the chain when this changes.
            self.reloads.send_modify(|n| *n += 1);
        }
    }

    /// Re-read config.toml whenever it changes (checked every 2 s, together with the `plugins/` folder). A broken file keeps the old config.
    pub fn spawn_reloader(self: &Arc<Self>) {
        let proxy = self.clone();
        tokio::spawn(async move {
            let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let mut last: Option<SystemTime> = mtime(&proxy.paths.config);
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let p = proxy.clone();
                let _ = tokio::task::spawn_blocking(move || p.sync_packages()).await;
                let now = mtime(&proxy.paths.config);
                if now == last {
                    continue;
                }
                last = now;
                match Config::load(&proxy.paths.config) {
                    Ok(c) => {
                        proxy.apply(c);
                        proxy.reloads.send_modify(|n| *n += 1);
                        tracing::info!("config reloaded");
                    }
                    Err(e) => tracing::warn!("config not reloaded: {e:#}"),
                }
            }
        });
    }

    /// Start the task that fires plugin timers (`Host::schedule`). Only the first call starts
    /// it; [`server::serve`] calls this. The task ends when the proxy is dropped.
    pub fn spawn_timers(self: &Arc<Self>) {
        if self.timers_started.swap(true, Ordering::SeqCst) {
            return;
        }
        tokio::spawn(timers::drive(
            Arc::downgrade(self),
            self.shared.timers.clone(),
        ));
    }

    /// A due timer: what the plugin queues goes out at once (see [`Engine::fire_timer`]).
    pub fn fire_timer(&self, plugin: &str, tag: &str) {
        let effects = self.engine.fire_timer(plugin, tag);
        self.deliver(effects);
    }

    /// Run sends plugins queued with `Host::emit_send` through the Send chain once and return
    /// the frames to send, in order (see [`Engine::chain_queued`]).
    pub fn chain_queued(&self, sends: Vec<(i64, Val)>) -> Vec<Val> {
        let effects = self.engine.chain_queued(sends);
        self.publish(effects.locals);
        effects.frames
    }

    /// Hand what plugins queued outside a gateway frame to the connections: payloads to the
    /// client, frames to the newest live gateway connection (none: dropped with a log line).
    fn deliver(&self, effects: Effects) {
        self.publish(effects.locals);
        self.live.push(effects.frames);
    }

    pub fn gateway_active(&self) -> bool {
        self.force_decode || self.engine.wants_gateway()
    }

    pub fn on_request(&self, method: &str, route: &str, body: &mut Option<Val>) -> RequestOutcome {
        let (outcome, effects) = self.engine.on_request(method, route, body);
        self.deliver(effects);
        outcome
    }

    /// A REST answer: plugins may rewrite `body`; returns whether they did.
    pub fn on_response(&self, method: &str, route: &str, status: u16, body: &mut Val) -> bool {
        let (changed, effects) = self.engine.on_response(method, route, status, body);
        self.deliver(effects);
        changed
    }

    /// Hand local server → client payloads to the gateway connections (none open: dropped).
    fn publish(&self, locals: Vec<Val>) {
        for v in locals {
            let _ = self.locals.send(v);
        }
    }

    /// A server → client payload: learn from it, then let plugins change it.
    pub fn on_dispatch(&self, payload: Val) -> DispatchResult {
        self.engine.on_dispatch(payload)
    }

    /// A client → server payload.
    pub fn on_send(&self, payload: &mut Val) -> SendResult {
        let mut result = self.engine.on_send(payload);
        self.publish(std::mem::take(&mut result.locals));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host::ProxyHost;
    use phoecord_core::plugin::API_VERSION;
    use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, TIMER_EVENT, Verdict};

    /// Rewrites MESSAGE_CREATE content and queues a presence re-send.
    struct Rewrite(Manifest);

    impl Plugin for Rewrite {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, e: &EventKind) -> bool {
            matches!(
                e,
                EventKind::Dispatch {
                    name: "MESSAGE_CREATE"
                }
            )
        }
        fn wants_gateway(&self) -> bool {
            true
        }
        fn on_event(&self, host: &mut dyn Host, e: Event<'_>) -> Verdict {
            if let Event::Dispatch { payload, .. } = e
                && let Some(d) = payload.get_mut("d")
            {
                d.set("content", Val::Str("rewritten".into()));
                host.emit_send(3, Val::map());
                return Verdict::Changed;
            }
            Verdict::Pass
        }
    }

    fn test_proxy(on: bool) -> Arc<Proxy> {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("phoecord-test-proxy-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config: dir.join("config.toml"),
            data: dir.join("data"),
        };
        let chain = Chain::new(vec![
            Box::new(phoecord_plugins::no_track::NoTrack::new()),
            Box::new(Rewrite(Manifest {
                id: "rewrite".into(),
                name: "Rewrite".into(),
                version: "0".into(),
                description: String::new(),
                authors: vec![],
                vencord: None,
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            })),
        ]);
        let mut cfg = Config::default();
        cfg.set_enabled("rewrite", on).unwrap();
        Proxy::new(paths, cfg, chain, false).unwrap()
    }

    fn msg() -> Val {
        Val::from_json(
            serde_json::from_str(
                r#"{"op":0,"t":"MESSAGE_CREATE","s":1,"d":{"content":"hi","channel_id":"5"}}"#,
            )
            .unwrap(),
        )
    }

    fn manifest(id: &str) -> Manifest {
        Manifest {
            id: id.into(),
            name: id.into(),
            version: "0".into(),
            description: String::new(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network],
            api: API_VERSION,
            fetch: Vec::new(),
        }
    }

    /// Wants its timer and `POST /ping`; both queue a send (op 3 from a timer, op 4 from REST),
    /// a timer also answers the client. Counts its timer events.
    struct Ticker(Manifest, AtomicU64);

    impl Plugin for Ticker {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, e: &EventKind) -> bool {
            matches!(e, EventKind::Dispatch { name } if *name == TIMER_EVENT)
                || matches!(e, EventKind::RestRequest { route: "/ping", .. })
        }
        fn on_event(&self, host: &mut dyn Host, e: Event<'_>) -> Verdict {
            match e {
                Event::Dispatch { payload, .. } => {
                    self.1.fetch_add(1, Ordering::Relaxed);
                    let tag = payload.str_at(&["d", "tag"]).unwrap_or("").to_string();
                    let mut d = Val::map();
                    d.set("tag", Val::Str(tag));
                    host.emit_send(3, d);
                    host.emit_dispatch(queued_frame(0, Val::Str("local".into())));
                }
                Event::RestRequest { .. } => host.emit_send(4, Val::map()),
                _ => {}
            }
            Verdict::Pass
        }
    }

    /// Stamps every client → server payload with `d.n += 1`, queues op 5 behind each op 3 and
    /// drops op 9.
    struct Stamp(Manifest);

    impl Plugin for Stamp {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, e: &EventKind) -> bool {
            matches!(e, EventKind::Send { .. })
        }
        fn on_event(&self, host: &mut dyn Host, e: Event<'_>) -> Verdict {
            let Event::Send { op, payload } = e else {
                return Verdict::Pass;
            };
            if op == 9 {
                return Verdict::Drop;
            }
            if let Some(d) = payload.get_mut("d") {
                let n = d.get("n").and_then(Val::as_i64).unwrap_or(0);
                d.set("n", Val::Int(n + 1));
            }
            if op == 3 {
                host.emit_send(5, Val::map());
            }
            Verdict::Changed
        }
    }

    fn timer_proxy(enabled: &[&str]) -> (Arc<Proxy>, Arc<Ticker>, Arc<Ticker>) {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("phoecord-test-timers-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let paths = Paths {
            config: dir.join("config.toml"),
            data: dir.join("data"),
        };
        let ticker = Arc::new(Ticker(manifest("ticker"), AtomicU64::new(0)));
        let other = Arc::new(Ticker(manifest("other"), AtomicU64::new(0)));
        let chain = Chain::new(vec![Box::new(Stamp(manifest("stamp")))]);
        chain.replace_dynamic(vec![ticker.clone(), other.clone()]);
        let mut cfg = Config::default();
        for id in ["ticker", "other", "stamp"] {
            cfg.set_enabled(id, enabled.contains(&id)).unwrap();
        }
        let proxy = Proxy::new(paths, cfg, chain, false).unwrap();
        (proxy, ticker, other)
    }

    fn schedule_for(p: &Proxy, plugin: &str, after_ms: u64, tag: &str) {
        let cfg = p.cfg();
        let mut h = ProxyHost::new(&p.shared, &cfg);
        h.begin_plugin(plugin);
        h.schedule(after_ms, tag).unwrap();
    }

    async fn next(rx: &mut live::LiveRx) -> Val {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a frame within 5 s")
            .expect("the channel is open")
    }

    #[tokio::test]
    async fn a_due_timer_fires_into_its_plugin_and_its_send_reaches_the_live_gateway() {
        let (p, ticker, other) = timer_proxy(&["ticker", "other", "stamp"]);
        let (_guard, mut rx) = p.live.register();
        let mut client = p.locals.subscribe();
        p.spawn_timers();
        p.spawn_timers(); // a second call starts nothing
        schedule_for(&p, "ticker", 0, "hb");
        schedule_for(&p, "ticker", 60_000, "later");
        let sent = next(&mut rx).await;
        assert_eq!(sent.get("op").and_then(Val::as_i64), Some(3));
        assert_eq!(sent.str_at(&["d", "tag"]), Some("hb"));
        assert_eq!(
            sent.get("d").and_then(|d| d.get("n")).and_then(Val::as_i64),
            Some(1),
            "the queued send went through the Send chain once"
        );
        let behind = next(&mut rx).await;
        assert_eq!(behind.get("op").and_then(Val::as_i64), Some(5));
        assert_eq!(
            behind.get("d").and_then(|d| d.get("n")),
            None,
            "a send queued while chaining is not chained again"
        );
        let local = tokio::time::timeout(Duration::from_secs(5), client.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(local.get("d"), Some(&Val::Str("local".into())));
        assert_eq!(ticker.1.load(Ordering::Relaxed), 1);
        assert_eq!(
            other.1.load(Ordering::Relaxed),
            0,
            "only the scheduler gets it"
        );
        assert_eq!(p.shared.timers.pending("ticker"), 1, "the later one waits");
        p.shared.timers.cancel("ticker", "later");
        assert_eq!(p.shared.timers.pending("ticker"), 0);
    }

    #[tokio::test]
    async fn a_timer_of_a_disabled_plugin_is_skipped() {
        let (p, ticker, _) = timer_proxy(&["stamp"]);
        let (_guard, mut rx) = p.live.register();
        p.fire_timer("ticker", "hb");
        assert_eq!(ticker.1.load(Ordering::Relaxed), 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_rest_path_send_reaches_the_live_gateway_at_once() {
        let (p, _, _) = timer_proxy(&["ticker", "stamp"]);
        let mut body = None;
        // No connection yet: dropped, nothing breaks.
        p.on_request("POST", "/ping", &mut body);
        let (_guard, mut rx) = p.live.register();
        assert_eq!(
            p.on_request("POST", "/ping", &mut body),
            RequestOutcome::Forward { changed: false }
        );
        let sent = rx.try_recv().expect("sent before on_request returned");
        assert_eq!(sent.get("op").and_then(Val::as_i64), Some(4));
        assert_eq!(
            sent.get("d").and_then(|d| d.get("n")).and_then(Val::as_i64),
            Some(1)
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn queued_sends_pass_the_send_chain_once_and_a_drop_holds() {
        let (p, _, _) = timer_proxy(&["stamp"]);
        let out = p.chain_queued(vec![(3, Val::map()), (8, Val::map())]);
        let ops: Vec<_> = out
            .iter()
            .map(|v| v.get("op").and_then(Val::as_i64).unwrap())
            .collect();
        assert_eq!(ops, vec![3, 5, 8]);
        let n = |v: &Val| v.get("d").and_then(|d| d.get("n")).and_then(Val::as_i64);
        assert_eq!(n(&out[0]), Some(1));
        assert_eq!(n(&out[1]), None);
        assert_eq!(n(&out[2]), Some(1));
        assert!(p.chain_queued(vec![(9, Val::map())]).is_empty(), "dropped");
        // With the Send plugin off the frames pass untouched.
        let (p, _, _) = timer_proxy(&[]);
        let out = p.chain_queued(vec![(3, Val::map())]);
        assert_eq!(out, vec![queued_frame(3, Val::map())]);
    }

    #[test]
    fn no_track_answers_science_locally() {
        let p = test_proxy(false);
        assert!(p.wants_request("POST", "/science"));
        assert!(!p.wants_request("GET", "/users/@me"));
        let mut body = None;
        assert_eq!(
            p.on_request("POST", "/science", &mut body),
            RequestOutcome::Reply {
                status: 204,
                body: None
            }
        );
        assert_eq!(
            p.on_request("GET", "/users/@me", &mut body),
            RequestOutcome::Forward { changed: false }
        );
    }

    #[test]
    fn dispatch_goes_through_the_chain_only_when_enabled() {
        let off = test_proxy(false);
        assert!(!off.gateway_active());
        let r = off.on_dispatch(msg());
        assert!(!r.changed && r.sends.is_empty());

        let on = test_proxy(true);
        assert!(on.gateway_active());
        let r = on.on_dispatch(msg());
        assert!(r.changed && !r.drop);
        assert_eq!(r.payload.str_at(&["d", "content"]), Some("rewritten"));
        assert_eq!(r.sends.len(), 1);
        // Account learned the channel's guild-less message without choking.
        assert!(on.shared.account.lock().unwrap().user_id.is_none());
    }

    #[test]
    fn reload_switches_plugins_and_keeps_the_old_config_on_error() {
        let p = test_proxy(false);
        assert_eq!(p.chain.enabled_ids(), vec!["no_track".to_string()]);
        let mut cfg = (*p.cfg()).clone();
        cfg.set_enabled("rewrite", true).unwrap();
        p.apply(cfg);
        assert_eq!(p.chain.enabled_ids().len(), 2);
        std::fs::create_dir_all(p.paths.config.parent().unwrap()).unwrap();
        std::fs::write(&p.paths.config, "listen = [").unwrap();
        assert!(Config::load(&p.paths.config).is_err());
        assert_eq!(
            p.chain.enabled_ids().len(),
            2,
            "a broken file changes nothing"
        );
    }
}
