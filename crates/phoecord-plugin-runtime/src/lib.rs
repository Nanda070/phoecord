//! Plugin runtime: runs the plugin chain for every event, whatever a plugin is made of.
//!
//! Today it drives native plugins (`phoecord-plugins`). The WASM backend (wasmtime + a WIT world that
//! mirrors `phoecord_core::plugin`) plugs in behind the same [`Chain`] (docs/ROADMAP.md, milestone 1.3).

use phoecord_core::{Event, EventKind, Host, Plugin, Val, Verdict};

/// Plugins in the order they run. Order matters: the first `Drop` or `Reply` wins.
#[derive(Default)]
pub struct Chain {
    plugins: Vec<Box<dyn Plugin>>,
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
        Chain { plugins }
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|p| p.manifest().id.as_str())
    }

    /// Whether any plugin wants this kind of event (lets the proxy skip decoding).
    pub fn wants(&self, kind: &EventKind) -> bool {
        self.plugins.iter().any(|p| p.wants(kind))
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
        for p in self.plugins.iter().filter(|p| p.wants(kind)) {
            match call(p.as_ref(), host) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_core::{Layer, Manifest};

    struct Fixed(Manifest, Verdict);

    fn fixed(id: &str, v: Verdict) -> Box<dyn Plugin> {
        Box::new(Fixed(
            Manifest {
                id: id.into(),
                name: id.into(),
                version: "0".into(),
                description: String::new(),
                authors: vec![],
                vencord: None,
                layers: vec![Layer::Network],
                api: phoecord_core::plugin::API_VERSION,
            },
            v,
        ))
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
}
