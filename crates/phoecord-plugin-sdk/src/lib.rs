//! PhoeCord plugin SDK: the crate third-party plugin authors depend on.
//!
//! A plugin is a `wasm32-wasip2` component built against `wit/phoecord.wit`, which mirrors
//! `phoecord_core::plugin`. With this crate it is one type, one `impl` and a `plugin.toml`:
//!
//! ```ignore
//! // Cargo.toml: phoecord = { package = "phoecord-plugin-sdk", path = "..." }
//! use phoecord::{Event, Manifest, Outcome, Plugin};
//!
//! struct Hello;
//!
//! #[phoecord::plugin]
//! impl Plugin for Hello {
//!     fn new() -> Self { Hello }
//!     fn manifest(&mut self) -> Manifest { phoecord::manifest("hello", "Hello", "Says hello") }
//!     fn on_event(&mut self, _event: Event) -> Outcome { Outcome::pass() }
//! }
//! ```
//!
//! The full example is `examples/hello-plugin`. The guest only reaches the outside world through
//! [`host`]; what it may call is set by the `rights` of its `plugin.toml`.
//!
//! It deliberately does not depend on `phoecord-core`: guests must build without the host's deps.
//!
//! Plugin API v1 is frozen: a plugin built with this SDK keeps loading in later 1.x hosts. Later
//! UI events arrive as new names in [`Event::Ui`] (a [`UiEvent`] with a `name` and a `payload`
//! tree), never as new [`Event`] cases, so match `Event::Ui` by name and ignore names you do not
//! know. Today there is one: `menu_action` (`{"entry", "channel_id", "message_id"}`, ids as
//! strings), sent when the user clicks one of the plugin's `[[menu]]` entries of `plugin.toml`.

/// The bindings generated from `wit/phoecord.wit`, for anything the re-exports below leave out.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "export_plugin",
    });
}

pub use bindings::phoecord::plugin::host;
pub use bindings::phoecord::plugin::types::{
    Dispatch, Event, EventKind, FilePart, HostResponse, Layer, Manifest, Node, Outcome, Reply,
    RestKind, RestRequest, RestResponse, Send, Tree, UiEvent, Verdict,
};
pub use phoecord_plugin_sdk_macros::plugin;

/// Plugin API version this SDK targets. Equals `phoecord_core::plugin::API_VERSION` (checked by
/// `crates/phoecord-plugin-runtime/tests/frozen_wit.rs`).
pub const API_VERSION: u32 = 1;

/// What a plugin implements. There is one instance per loaded component; it is made on first use
/// and keeps its fields between events (until the package is reloaded).
pub trait Plugin: 'static {
    fn new() -> Self;
    fn manifest(&mut self) -> Manifest;
    /// Cheap pre-filter over the events the `events` list of `plugin.toml` lets through.
    fn wants(&mut self, _kind: &EventKind) -> bool {
        true
    }
    fn on_event(&mut self, event: Event) -> Outcome;
}

/// A network plugin's manifest with this SDK's API version; change the fields you need.
pub fn manifest(id: &str, name: &str, description: &str) -> Manifest {
    Manifest {
        id: id.into(),
        name: name.into(),
        version: "0.1.0".into(),
        description: description.into(),
        authors: Vec::new(),
        vencord: None,
        layers: vec![Layer::Network],
        api: API_VERSION,
    }
}

impl Outcome {
    fn of(verdict: Verdict, payload: Option<Tree>) -> Outcome {
        Outcome {
            verdict,
            payload,
            extra: Vec::new(),
        }
    }
    /// Not interested or nothing changed.
    pub fn pass() -> Outcome {
        Outcome::of(Verdict::Pass, None)
    }
    /// `payload` replaces the event's payload.
    pub fn changed(payload: Tree) -> Outcome {
        Outcome::of(Verdict::Changed, Some(payload))
    }
    /// Swallow the event.
    pub fn swallow() -> Outcome {
        Outcome::of(Verdict::Drop, None)
    }
    /// Answer a REST request locally.
    pub fn reply(status: u16, body: Option<Tree>) -> Outcome {
        Outcome::of(Verdict::Reply(Reply { status, body }), None)
    }
}

impl Tree {
    /// The node under a path of map keys, e.g. `tree.at(&["d", "author", "id"])`. Keys match in
    /// both gateway encodings (JSON strings, ETF atoms and binaries).
    pub fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self.nodes.first()?, |node, key| {
            let Node::Map(pairs) = node else {
                return None;
            };
            let (_, value) = pairs
                .iter()
                .find(|(k, _)| self.nodes.get(*k as usize).and_then(text) == Some(*key))?;
            self.nodes.get(*value as usize)
        })
    }

    /// The text at a path.
    pub fn str_at(&self, path: &[&str]) -> Option<&str> {
        self.at(path).and_then(text)
    }

    /// A Discord id at a path: a string in JSON, an integer in ETF.
    pub fn id_at(&self, path: &[&str]) -> Option<u64> {
        match self.at(path)? {
            Node::Int(n) => u64::try_from(*n).ok(),
            Node::Big((0, low)) => Some(*low),
            node => text(node)?.parse().ok(),
        }
    }
}

/// The text of a string, atom or binary node.
fn text(node: &Node) -> Option<&str> {
    match node {
        Node::String(s) | Node::Atom(s) => Some(s),
        Node::Bin(b) | Node::Charlist(b) => std::str::from_utf8(b).ok(),
        _ => None,
    }
}

/// What `#[phoecord::plugin]` expands to. Not public API.
#[doc(hidden)]
#[macro_export]
macro_rules! __export {
    ($plugin:ident) => {
        const _: () = {
            ::std::thread_local!(
                static INSTANCE: ::std::cell::RefCell<::std::option::Option<$plugin>> =
                    const { ::std::cell::RefCell::new(::std::option::Option::None) }
            );

            fn with<R>(f: impl ::std::ops::FnOnce(&mut $plugin) -> R) -> R {
                INSTANCE.with(|slot| {
                    f(slot
                        .borrow_mut()
                        .get_or_insert_with(<$plugin as $crate::Plugin>::new))
                })
            }

            impl $crate::bindings::Guest for $plugin {
                fn manifest() -> $crate::Manifest {
                    with(|p| $crate::Plugin::manifest(p))
                }
                fn wants(kind: $crate::EventKind) -> bool {
                    with(|p| $crate::Plugin::wants(p, &kind))
                }
                fn on_event(event: $crate::Event) -> $crate::Outcome {
                    with(|p| $crate::Plugin::on_event(p, event))
                }
            }

    $crate::bindings::export_plugin!($plugin with_types_in $crate::bindings);
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(nodes: Vec<Node>) -> Tree {
        Tree { nodes }
    }

    #[test]
    fn paths_work_in_both_encodings() {
        // {"d": {"id": "7", n: 8}}
        let t = tree(vec![
            Node::Map(vec![(1, 2)]),
            Node::String("d".into()),
            Node::Map(vec![(3, 4), (5, 6)]),
            Node::String("id".into()),
            Node::String("7".into()),
            Node::Atom("n".into()),
            Node::Int(8),
        ]);
        assert_eq!(t.str_at(&["d", "id"]), Some("7"));
        assert_eq!(t.id_at(&["d", "id"]), Some(7));
        assert_eq!(t.id_at(&["d", "n"]), Some(8));
        assert!(t.at(&["d", "missing"]).is_none());
        assert!(t.at(&["d", "id", "deeper"]).is_none());
        assert!(tree(Vec::new()).at(&[]).is_none());
        // A child index outside the arena is not a panic.
        assert!(tree(vec![Node::Map(vec![(9, 9)])]).at(&["a"]).is_none());
    }

    #[test]
    fn outcomes_and_manifest() {
        assert!(matches!(Outcome::pass().verdict, Verdict::Pass));
        assert!(matches!(Outcome::swallow().verdict, Verdict::Drop));
        let r = Outcome::reply(204, None);
        assert!(matches!(
            r.verdict,
            Verdict::Reply(Reply { status: 204, .. })
        ));
        assert!(Outcome::changed(tree(vec![Node::Null])).payload.is_some());
        assert_eq!(manifest("a", "A", "d").api, API_VERSION);
    }
}
