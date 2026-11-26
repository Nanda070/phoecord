//! A plugin built against the frozen v1 WIT (`wit/frozen/`), standing in for every third-party
//! plugin already built. It touches each part of the contract a plugin can see: the manifest,
//! `wants`, every event kind including `ui`, the verdicts and a few host calls.

wit_bindgen::generate!({
    path: "../../../../wit/frozen",
    world: "plugin",
});

use phoecord::plugin::host;
use phoecord::plugin::types::{Event, EventKind, Layer, Manifest, Node, Outcome, Reply, Tree, Verdict};

struct CompatGuest;

fn outcome(verdict: Verdict) -> Outcome {
    Outcome {
        verdict,
        payload: None,
        extra: Vec::new(),
    }
}

/// The string under `key` of the root map.
fn text_of(tree: &Tree, key: &str) -> Option<String> {
    let Some(Node::Map(pairs)) = tree.nodes.first() else {
        return None;
    };
    pairs.iter().find_map(|(k, v)| {
        match (tree.nodes.get(*k as usize)?, tree.nodes.get(*v as usize)?) {
            (Node::String(k), Node::String(v)) if k == key => Some(v.clone()),
            _ => None,
        }
    })
}

impl Guest for CompatGuest {
    fn manifest() -> Manifest {
        Manifest {
            id: "compat_guest".into(),
            name: "Compat guest".into(),
            version: "1.0.0".into(),
            description: "Built against the frozen v1 WIT".into(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network, Layer::Ui],
            api: 1,
        }
    }

    fn wants(kind: EventKind) -> bool {
        match kind {
            EventKind::RestRequest(_) | EventKind::RestResponse(_) | EventKind::Send(_) => true,
            EventKind::Dispatch(name) => name != "UNWANTED",
            EventKind::Ui(name) => name == "menu_action",
        }
    }

    fn on_event(event: Event) -> Outcome {
        match event {
            Event::Dispatch(d) => {
                host::log(&format!("dispatch {}", d.name));
                Outcome {
                    verdict: Verdict::Changed,
                    payload: Some(Tree {
                        nodes: vec![Node::String("seen".into())],
                    }),
                    extra: vec![],
                }
            }
            Event::RestRequest(r) => outcome(Verdict::Reply(Reply {
                status: 200,
                body: Some(Tree {
                    nodes: vec![Node::String(host::request_query().unwrap_or(r.route))],
                }),
            })),
            Event::RestResponse(_) => outcome(Verdict::Pass),
            Event::Send(_) => outcome(Verdict::Drop),
            Event::Ui(u) => {
                let entry = text_of(&u.payload, "entry").unwrap_or_default();
                let message = text_of(&u.payload, "message_id").unwrap_or_default();
                host::log(&format!("ui {} {entry} {message}", u.name));
                outcome(Verdict::Pass)
            }
        }
    }
}

export!(CompatGuest);
