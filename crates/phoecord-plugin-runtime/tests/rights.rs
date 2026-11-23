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
