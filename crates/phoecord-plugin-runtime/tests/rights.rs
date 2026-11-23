mod common;

use common::{TempDir, write_package};
use phoecord_core::{Event, Plugin, Val, Verdict};
use phoecord_plugin_runtime::package::PackagePlugin;
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

fn load(extra: &str) -> (TempDir, PackagePlugin) {
    let root = TempDir::new();
    let dir = write_package(&root.0, "gated", 1, extra);
    let rt = common::runtime();
    let plugin = PackagePlugin::load(&rt, &dir).unwrap();
    (root, plugin)
}

#[test]
fn a_guest_without_the_notify_right_cannot_notify() {
    let (_root, plugin) = load("events = [\"dispatch\"]\nrights = [\"kv\"]");
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = NullHost::default();
    // The guest sees the error and answers 403 with it; nothing reached the real host.
    for _ in 0..2 {
        let out = chain.dispatch(&mut host, "DENIED", &mut Val::map(), &mut vec![]);
        let Outcome::Replied { status, body, .. } = out else {
            panic!("expected a reply, got {out:?}");
        };
        assert_eq!(status, 403);
        assert!(
            body.unwrap()
                .as_str()
                .unwrap()
                .contains("no `notify` right")
        );
    }
    assert!(
        !host.logs.iter().any(|l| l.starts_with("notify:")),
        "{:?}",
        host.logs
    );
    // Logged once, not once per call.
    let denials = host
        .logs
        .iter()
        .filter(|l| l.contains("no `notify` right"))
        .count();
    assert_eq!(denials, 1, "{:?}", host.logs);
}

#[test]
fn a_granted_right_goes_through() {
    let (_root, plugin) = load("events = [\"dispatch\"]\nrights = [\"notify\"]");
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = NullHost::default();
    let out = chain.dispatch(&mut host, "DENIED", &mut Val::map(), &mut vec![]);
    assert_eq!(out, Outcome::Untouched, "{:?}", host.logs);
    assert_eq!(host.logs, vec!["notify: title: body".to_string()]);
}

/// Dispatch `event` to a package with `rights`; the reply status (200 worked, 403 denied) and body.
fn call(rights: &str, event: &str, host: &mut NullHost) -> (u16, Option<Val>) {
    let (_root, plugin) = load(&format!("events = [\"dispatch\"]\nrights = [{rights}]"));
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    match chain.dispatch(host, event, &mut Val::map(), &mut vec![]) {
        Outcome::Replied { status, body, .. } => (status, body),
        other => panic!("expected a reply, got {other:?}"),
    }
}

fn denial(body: Option<Val>) -> String {
    body.and_then(|b| b.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn count(host: &NullHost, needle: &str) -> usize {
    host.logs.iter().filter(|l| l.contains(needle)).count()
}

#[test]
fn emit_dispatch_needs_its_right() {
    let mut host = NullHost::default();
    let (status, body) = call("\"kv\"", "DISPATCH", &mut host);
    assert_eq!(status, 403);
    assert!(denial(body).contains("no `emit_dispatch` right"));
    assert!(host.dispatched.is_empty());
    assert_eq!(count(&host, "no `emit_dispatch` right"), 1);

    let (status, _) = call("\"emit_dispatch\"", "DISPATCH", &mut host);
    assert_eq!(status, 200);
    assert_eq!(host.dispatched.len(), 1);
}

#[test]
fn toggling_plugins_needs_the_plugins_right() {
    let mut host = NullHost::default();
    host.plugins.insert("other".into(), false);
    for event in ["TOGGLE", "ENABLED"] {
        let (status, body) = call("\"kv\", \"data\"", event, &mut host);
        assert_eq!(status, 403);
        assert!(denial(body).contains("no `plugins` right"));
    }
    assert!(!host.plugins["other"]);

    let (status, _) = call("\"plugins\"", "TOGGLE", &mut host);
    assert_eq!(status, 200);
    assert!(host.plugins["other"]);
    let (status, _) = call("\"plugins\"", "ENABLED", &mut host);
    assert_eq!(status, 200);
    // The host's own error (unknown id) reaches the guest as an error.
    host.plugins.clear();
    let (status, body) = call("\"plugins\"", "TOGGLE", &mut host);
    assert_eq!(status, 403);
    assert!(denial(body).contains("unknown plugin"));
}

#[test]
fn append_data_needs_the_data_right() {
    let mut host = NullHost::default();
    let (status, body) = call("\"plugins\"", "DATA", &mut host);
    assert_eq!(status, 403);
    assert!(denial(body).contains("no `data` right"));
    assert!(host.data.is_empty());

    let (status, _) = call("\"data\"", "DATA", &mut host);
    assert_eq!(status, 200);
    assert_eq!(host.data, vec![("log.jsonl".to_string(), "{}".to_string())]);
}

#[test]
fn unknown_rights_are_refused() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "greedy", 1, "rights = [\"root\"]");
    let rt = common::runtime();
    let err = PackagePlugin::load(&rt, &dir).err().unwrap();
    assert!(err.contains("unknown right"), "{err}");
}

#[test]
fn a_rest_plugin_never_sees_gateway_events() {
    let (_root, plugin) = load("events = [\"rest_request\"]");
    assert!(!plugin.wants_gateway());
    // Even if a caller skips `wants` and hands over a gateway event, it is not delivered.
    let mut host = NullHost::default();
    let mut payload = Val::map();
    let verdict = plugin.on_event(
        &mut host,
        Event::Dispatch {
            name: "MESSAGE_CREATE",
            payload: &mut payload,
            extra: &mut vec![],
        },
    );
    assert_eq!(verdict, Verdict::Pass);
    assert_eq!(payload, Val::map());
    assert!(host.logs.is_empty(), "the guest ran: {:?}", host.logs);
    let verdict = plugin.on_event(
        &mut host,
        Event::Send {
            op: 3,
            payload: &mut payload,
        },
    );
    assert_eq!(verdict, Verdict::Pass);
    assert!(host.logs.is_empty());
    // A subscribed REST event still works.
    let mut body = Val::map();
    let verdict = plugin.on_event(
        &mut host,
        Event::RestRequest {
            method: "POST",
            route: "/x",
            body: Some(&mut body),
        },
    );
    assert_eq!(verdict, Verdict::Changed);
}
