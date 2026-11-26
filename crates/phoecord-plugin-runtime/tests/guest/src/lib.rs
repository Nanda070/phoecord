//! A guest plugin whose behaviour is picked by the event it receives, so one build serves every test.

wit_bindgen::generate!({
    path: "../../../../wit",
    world: "plugin",
});

use phoecord::plugin::host;
use phoecord::plugin::types::{
    Event, EventKind, Layer, Manifest, Node, Outcome, Reply, Tree, Verdict,
};

struct TestGuest;

fn outcome(verdict: Verdict) -> Outcome {
    Outcome {
        verdict,
        payload: None,
        extra: Vec::new(),
    }
}

/// `{"touched": true}`
fn touched() -> Tree {
    Tree {
        nodes: vec![
            Node::Map(vec![(1, 2)]),
            Node::String("touched".into()),
            Node::Bool(true),
        ],
    }
}

fn denied(e: String) -> Outcome {
    outcome(Verdict::Reply(Reply {
        status: 403,
        body: Some(Tree {
            nodes: vec![Node::String(e)],
        }),
    }))
}

/// 200 when the host call worked, 403 with the error otherwise.
fn answer(r: Result<(), String>) -> Outcome {
    match r {
        Ok(()) => outcome(Verdict::Reply(Reply {
            status: 200,
            body: None,
        })),
        Err(e) => denied(e),
    }
}

fn dispatch(name: &str) -> Outcome {
    match name {
        // No host call, no change: measures the cost of the WASM path alone.
        "NOOP" => outcome(Verdict::Pass),
        "PANIC" => panic!("guest panic"),
        "LOOP" => {
            let mut n = 0u64;
            loop {
                n = std::hint::black_box(n.wrapping_add(1));
            }
        }
        "HOG" => {
            // 128 MiB, far past the 32 MiB limit.
            let mut v: Vec<u8> = Vec::new();
            v.resize(128 << 20, 1);
            std::hint::black_box(&v);
            outcome(Verdict::Pass)
        }
        "DENIED" => match host::notify("title", "body") {
            Ok(()) => outcome(Verdict::Pass),
            Err(e) => outcome(Verdict::Reply(Reply {
                status: 403,
                body: Some(Tree {
                    nodes: vec![Node::String(e)],
                }),
            })),
        },
        // The plugin's own strings: one key in the user's language, one only English has.
        "STRINGS" => {
            let hello = host::t("greeting", &[("who".to_string(), "Мир".to_string())]);
            let fallback = host::t("only-en", &[]);
            outcome(Verdict::Reply(Reply {
                status: 200,
                body: Some(Tree {
                    nodes: vec![Node::String(format!("{hello}|{fallback}"))],
                }),
            }))
        }
        "NOTIFY" => {
            let _ = host::notify("title", "body");
            outcome(Verdict::Pass)
        }
        "DISPATCH" => answer(host::emit_dispatch(&touched())),
        "TOGGLE" => answer(host::set_plugin_enabled("other", true)),
        "ENABLED" => match host::plugin_enabled("other") {
            Ok(on) => outcome(Verdict::Reply(Reply {
                status: if on == Some(true) { 200 } else { 404 },
                body: None,
            })),
            Err(e) => denied(e),
        },
        "DATA" => answer(host::append_data("log.jsonl", "{}")),
        // `fetch-with`: 200 with the answer's status in the body, 403 with the error.
        "FETCH_WITH" => match host::fetch_with(
            "POST",
            "https://1.example.com/x",
            &[("X-Test".to_string(), "1".to_string())],
            None,
        ) {
            Ok(r) => outcome(Verdict::Reply(Reply {
                status: 200,
                body: Some(Tree {
                    nodes: vec![Node::Int(i64::from(r.status))],
                }),
            })),
            Err(e) => denied(e),
        },
        // Two timers and a cancelled third; a timer answers with a log line naming its tag.
        "SCHEDULE" => {
            let r = host::schedule(1000, "tick").and_then(|()| host::schedule(10, "drop"));
            host::cancel_schedule("drop");
            answer(r.and_then(|()| host::schedule(200, "tock")))
        }
        "PHOECORD_TIMER" => {
            host::log("timer fired");
            outcome(Verdict::Pass)
        }
        "KV" => {
            let _ = host::kv_set("seen", &touched());
            let got = host::kv_get("seen");
            let status = if matches!(got, Ok(Some(_))) { 200 } else { 500 };
            outcome(Verdict::Reply(Reply { status, body: None }))
        }
        _ => {
            host::log(&format!("saw {name}"));
            Outcome {
                verdict: Verdict::Changed,
                payload: Some(touched()),
                extra: vec![touched()],
            }
        }
    }
}

impl Guest for TestGuest {
    fn manifest() -> Manifest {
        Manifest {
            id: "test_guest".into(),
            name: "Test guest".into(),
            version: "0.0.0".into(),
            description: String::new(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network],
            api: 1,
        }
    }

    fn wants(kind: EventKind) -> bool {
        !matches!(&kind, EventKind::Dispatch(n) if n == "UNWANTED")
    }

    fn on_event(event: Event) -> Outcome {
        match event {
            Event::Dispatch(d) => dispatch(&d.name),
            Event::RestRequest(r) if r.route == "/drop" => outcome(Verdict::Drop),
            // The call's query string, or 404 without one.
            Event::RestRequest(r) if r.route == "/query" => match host::request_query() {
                Some(q) => outcome(Verdict::Reply(Reply {
                    status: 200,
                    body: Some(Tree {
                        nodes: vec![Node::String(q)],
                    }),
                })),
                None => outcome(Verdict::Reply(Reply {
                    status: 404,
                    body: None,
                })),
            },
            Event::RestRequest(r) => Outcome {
                verdict: Verdict::Changed,
                payload: Some(touched()),
                extra: vec![],
            }
            .with_route(&r.route),
            Event::RestResponse(_) | Event::Send(_) => outcome(Verdict::Pass),
            // Logs the UI event's name and its `entry` field.
            Event::Ui(u) => {
                let entry = u.payload.text_of("entry").unwrap_or_default();
                host::log(&format!("ui {} {entry}", u.name));
                outcome(Verdict::Pass)
            }
        }
    }
}

trait TextOf {
    fn text_of(&self, key: &str) -> Option<String>;
}

impl TextOf for Tree {
    /// The string under `key` of the root map.
    fn text_of(&self, key: &str) -> Option<String> {
        let Some(Node::Map(pairs)) = self.nodes.first() else {
            return None;
        };
        pairs.iter().find_map(|(k, v)| match (self.nodes.get(*k as usize)?, self.nodes.get(*v as usize)?) {
            (Node::String(k), Node::String(v)) if k == key => Some(v.clone()),
            _ => None,
        })
    }
}

trait WithRoute {
    fn with_route(self, route: &str) -> Self;
}

impl WithRoute for Outcome {
    fn with_route(self, route: &str) -> Self {
        host::log(&format!("rest {route}"));
        self
    }
}

export!(TestGuest);
