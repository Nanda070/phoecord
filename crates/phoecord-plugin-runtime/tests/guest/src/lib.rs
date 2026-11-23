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
            Event::RestRequest(r) => Outcome {
                verdict: Verdict::Changed,
                payload: Some(touched()),
                extra: vec![],
            }
            .with_route(&r.route),
            Event::RestResponse(_) | Event::Send(_) => outcome(Verdict::Pass),
        }
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
