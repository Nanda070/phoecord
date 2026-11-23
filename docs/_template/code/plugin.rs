//! <PluginName>: <one line about what it does> (Vencord: <VencordName>, or "none").
//!
//! Template for a native built-in plugin. Copy to `crates/phoecord-plugins/src/<id>.rs`, add
//! `pub mod <id>;` and an entry in `builtin()` in `crates/phoecord-plugins/src/lib.rs`, then a row
//! in `docs/PLUGINS.md`.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Verdict};

pub struct MyPlugin {
    manifest: Manifest,
}

impl MyPlugin {
    pub fn new() -> MyPlugin {
        MyPlugin {
            manifest: Manifest {
                id: "my_plugin".into(),
                name: "MyPlugin".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "What it does, in one English sentence".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: None,
                layers: vec![Layer::Network],
                api: API_VERSION,
            },
        }
    }
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Keep the matching rule in one function so `wants` and `on_event` cannot disagree.
fn matches(method: &str, route: &str) -> bool {
    method == "POST" && route.ends_with("/typing")
}

impl Plugin for MyPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if matches(method, route))
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest { method, route, .. } if matches(method, route) => Verdict::Reply {
                status: 204,
                body: None,
            },
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    #[test]
    fn matches_only_its_routes() {
        assert!(matches("POST", "/channels/1/typing"));
        assert!(!matches("GET", "/channels/1/typing"));
        assert!(!matches("POST", "/channels/1/messages"));
    }

    #[test]
    fn replies_locally() {
        let chain = Chain::new(vec![Box::new(MyPlugin::new())]);
        let mut host = NullHost::default();
        let out = chain.rest_request(&mut host, "POST", "/channels/1/typing", None);
        assert!(matches!(out, Outcome::Replied { status: 204, .. }));
    }
}
