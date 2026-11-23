//! Plugin runtime: runs the plugin chain for every event, whatever a plugin is made of.
//!
//! Today it drives native plugins (`phoecord-plugins`). The WASM backend (wasmtime + a WIT world that
//! mirrors `phoecord_core::plugin`) plugs in behind the same [`Chain`] (docs/ROADMAP.md, milestone 1.3).

pub mod package;
pub mod wasm;

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::RwLock;

/// Plugins in the order they run. Order matters: the first `Drop` or `Reply` wins.
///
/// Only *enabled* plugins run (all of them until [`Chain::set_enabled`] is called). A plugin that
/// panics is disabled for the rest of the session and the chain carries on without it.
#[derive(Default)]
pub struct Chain {
    plugins: Vec<Box<dyn Plugin>>,
    /// `None` = every plugin is enabled.
    enabled: RwLock<Option<HashSet<String>>>,
    crashed: RwLock<HashSet<String>>,
}

/// What the chain decided, folded over all plugins.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Untouched,
    Changed,
    Dropped {
        by: String,
    },
    Replied {
        by: String,
        status: u16,
        body: Option<Val>,
    },
}

impl Chain {
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Chain {
        Chain {
            plugins,
            ..Chain::default()
        }
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|p| p.manifest().id.as_str())
    }

    pub fn manifests(&self) -> impl Iterator<Item = &Manifest> {
        self.plugins.iter().map(|p| p.manifest())
    }

    /// Replace the set of enabled plugin ids (called on every config reload).
    pub fn set_enabled(&self, ids: impl IntoIterator<Item = String>) {
        *self.enabled.write().unwrap() = Some(ids.into_iter().collect());
    }

    /// Ids that currently run, in chain order.
    pub fn enabled_ids(&self) -> Vec<String> {
        self.active().map(|p| p.manifest().id.clone()).collect()
    }

    /// Ids of plugins disabled for the session after a panic.
    pub fn crashed_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.crashed.read().unwrap().iter().cloned().collect();
        v.sort();
        v
    }

    fn active(&self) -> impl Iterator<Item = &dyn Plugin> {
        let enabled = self.enabled.read().unwrap().clone();
        let crashed = self.crashed.read().unwrap().clone();
        self.plugins.iter().map(|p| p.as_ref()).filter(move |p| {
            let id = &p.manifest().id;
            !crashed.contains(id) && enabled.as_ref().is_none_or(|e| e.contains(id))
        })
    }

    /// Whether any enabled plugin wants this kind of event (lets the proxy skip decoding).
    pub fn wants(&self, kind: &EventKind) -> bool {
        self.active().any(|p| p.wants(kind))
    }

    /// Whether any enabled plugin may want gateway events at all.
    pub fn wants_gateway(&self) -> bool {
        self.active().any(|p| p.wants_gateway())
    }

    /// Whether the plugin with this id may want gateway events (enabled or not).
    pub fn uses_gateway(&self, id: &str) -> bool {
        self.plugins
            .iter()
            .any(|p| p.manifest().id == id && p.wants_gateway())
    }

    /// Run one event through every interested plugin. `call` builds the [`Event`] for each plugin
    /// (re-borrowing the possibly already changed payload) and hands it to `Plugin::on_event`.
    pub fn run(
        &self,
        host: &mut dyn Host,
        kind: &EventKind,
        mut call: impl FnMut(&dyn Plugin, &mut dyn Host) -> Verdict,
    ) -> Outcome {
        let mut changed = false;
        for p in self.active().filter(|p| p.wants(kind)) {
            host.begin_plugin(&p.manifest().id);
            let verdict = match catch_unwind(AssertUnwindSafe(|| call(p, host))) {
                Ok(v) => v,
                Err(_) => {
                    let id = p.manifest().id.clone();
                    host.log(&format!(
                        "plugin {id} panicked and is disabled for this session"
                    ));
                    self.crashed.write().unwrap().insert(id);
                    continue;
                }
            };
            match verdict {
                Verdict::Pass => {}
                Verdict::Changed => changed = true,
                Verdict::Drop => {
                    return Outcome::Dropped {
                        by: p.manifest().id.clone(),
                    };
                }
                Verdict::Reply { status, body } => {
                    return Outcome::Replied {
                        by: p.manifest().id.clone(),
                        status,
                        body,
                    };
                }
            }
        }
        if changed {
            Outcome::Changed
        } else {
            Outcome::Untouched
        }
    }

    /// A REST request through the chain.
    pub fn rest_request(
        &self,
        host: &mut dyn Host,
        method: &str,
        route: &str,
        mut body: Option<&mut Val>,
    ) -> Outcome {
        self.run(host, &EventKind::RestRequest { method, route }, |p, h| {
            p.on_event(
                h,
                Event::RestRequest {
                    method,
                    route,
                    body: body.as_deref_mut(),
                },
            )
        })
    }

    /// A REST answer through the chain (plugins may rewrite the body).
    pub fn rest_response(
        &self,
        host: &mut dyn Host,
        method: &str,
        route: &str,
        status: u16,
        body: &mut Val,
    ) -> Outcome {
        self.run(host, &EventKind::RestResponse { method, route }, |p, h| {
            p.on_event(
                h,
                Event::RestResponse {
                    method,
                    route,
                    status,
                    body: &mut *body,
                },
            )
        })
    }

    /// A client → server gateway payload through the chain.
    pub fn send(&self, host: &mut dyn Host, op: i64, payload: &mut Val) -> Outcome {
        self.run(host, &EventKind::Send { op }, |p, h| {
            p.on_event(h, Event::Send { op, payload })
        })
    }

    /// A gateway dispatch through the chain. Payloads plugins add end up in `extra`.
    pub fn dispatch(
        &self,
        host: &mut dyn Host,
        name: &str,
        payload: &mut Val,
        extra: &mut Vec<Val>,
    ) -> Outcome {
        self.run(host, &EventKind::Dispatch { name }, |p, h| {
            p.on_event(
                h,
                Event::Dispatch {
                    name,
                    payload,
                    extra,
                },
            )
        })
    }
}

/// A [`Host`] that remembers nothing, for tests and the self-test.
#[derive(Default)]
pub struct NullHost {
    pub logs: Vec<String>,
}

impl Host for NullHost {
    fn setting(&self, _key: &str) -> Option<Val> {
        None
    }
    fn kv_get(&self, _key: &str) -> Option<Val> {
        None
    }
    fn kv_set(&mut self, _key: &str, _value: Val) {}
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, title: &str, body: &str) {
        self.logs.push(format!("notify: {title}: {body}"));
    }
    fn log(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }
    fn emit_send(&mut self, _op: i64, _payload: Val) {}
    fn guild_emojis(&self, _guild_id: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _channel_id: u64) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_core::Layer;

    struct Fixed(Manifest, Verdict);

    fn manifest(id: &str) -> Manifest {
        Manifest {
            id: id.into(),
            name: id.into(),
            version: "0".into(),
            description: String::new(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network],
            api: phoecord_core::plugin::API_VERSION,
        }
    }

    fn fixed(id: &str, v: Verdict) -> Box<dyn Plugin> {
        Box::new(Fixed(manifest(id), v))
    }

    impl Plugin for Fixed {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
            self.1.clone()
        }
    }

    struct Panics(Manifest);

    impl Plugin for Panics {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
            panic!("boom")
        }
    }

    #[test]
    fn first_stop_wins_and_changes_fold() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            fixed("a", Verdict::Changed),
            fixed("b", Verdict::Drop),
            fixed(
                "c",
                Verdict::Reply {
                    status: 204,
                    body: None,
                },
            ),
        ]);
        let mut p = Val::map();
        let mut extra = vec![];
        assert_eq!(
            chain.dispatch(&mut h, "MESSAGE_CREATE", &mut p, &mut extra),
            Outcome::Dropped { by: "b".into() }
        );
        let chain = Chain::new(vec![
            fixed("a", Verdict::Changed),
            fixed("b", Verdict::Pass),
        ]);
        assert_eq!(
            chain.rest_request(&mut h, "GET", "/x", None),
            Outcome::Changed
        );
    }

    #[test]
    fn disabled_and_panicking_plugins_do_not_run() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            Box::new(Panics(manifest("bad"))),
            fixed("a", Verdict::Changed),
            fixed("off", Verdict::Drop),
        ]);
        chain.set_enabled(["bad".to_string(), "a".to_string()]);
        let mut p = Val::map();
        assert_eq!(chain.send(&mut h, 3, &mut p), Outcome::Changed);
        assert_eq!(chain.crashed_ids(), vec!["bad".to_string()]);
        assert_eq!(chain.enabled_ids(), vec!["a".to_string()]);
        // The crashed plugin stays out for the rest of the session.
        assert_eq!(
            chain.rest_response(&mut h, "GET", "/x", 200, &mut Val::Null),
            Outcome::Changed
        );
        assert_eq!(chain.crashed_ids().len(), 1);
    }
}
