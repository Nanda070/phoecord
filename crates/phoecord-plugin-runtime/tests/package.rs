mod common;

use common::{TempDir, write_package};
use phoecord_core::{EventKind, Plugin, Val};
use phoecord_plugin_runtime::package::{PackagePlugin, load_dir};
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::time::Duration;

const MESSAGE_CREATE: EventKind = EventKind::Dispatch {
    name: "MESSAGE_CREATE",
};
const TYPING_START: EventKind = EventKind::Dispatch {
    name: "TYPING_START",
};

#[test]
fn packages_load_validate_and_filter_events() {
    let root = TempDir::new();
    write_package(
        &root.0,
        "good",
        1,
        "events = [\"dispatch:MESSAGE_*\", \"rest_request:POST /channels/*\"]",
    );
    write_package(&root.0, "wrong_api", 2, "events = [\"dispatch\"]");
    // A package whose folder name differs from its id is refused.
    let moved = write_package(&root.0, "moved", 1, "");
    std::fs::rename(&moved, root.0.join("elsewhere")).unwrap();

    let rt = common::runtime();
    let loaded = load_dir(&rt, &root.0);
    assert_eq!(loaded.plugins.len(), 1);
    assert_eq!(loaded.errors.len(), 2, "{:?}", loaded.errors);
    let reasons: String = loaded.errors.iter().map(|(_, e)| e.as_str()).collect();
    assert!(reasons.contains("API v2"), "{reasons}");
    assert!(reasons.contains("folder name"), "{reasons}");

    let plugin = loaded.plugins.into_iter().next().unwrap();
    assert_eq!(plugin.manifest().id, "good");
    // `events` globs become `wants`.
    assert!(plugin.wants(&MESSAGE_CREATE));
    assert!(!plugin.wants(&TYPING_START));
    assert!(plugin.wants(&EventKind::RestRequest {
        method: "POST",
        route: "/channels/1/messages"
    }));
    assert!(!plugin.wants(&EventKind::RestRequest {
        method: "GET",
        route: "/users/@me"
    }));
    assert!(!plugin.wants(&EventKind::Send { op: 3 }));
    assert!(plugin.wants_gateway());

    // Through the chain: a subscribed event is handled, an unsubscribed one never reaches it.
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = NullHost::default();
    assert_eq!(
        chain.dispatch(&mut host, "MESSAGE_CREATE", &mut Val::map(), &mut vec![]),
        Outcome::Changed
    );
    assert_eq!(
        chain.dispatch(&mut host, "TYPING_START", &mut Val::map(), &mut vec![]),
        Outcome::Untouched
    );
    assert!(!chain.wants(&TYPING_START));
}

#[test]
fn a_changed_package_is_reloaded_between_events() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "hot", 1, "events = [\"dispatch:MESSAGE_*\"]");
    let rt = common::runtime();
    let plugin = PackagePlugin::load(&rt, &dir).unwrap();
    plugin.set_check_interval(Duration::ZERO);
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = NullHost::default();
    assert!(chain.wants(&MESSAGE_CREATE));
    assert!(!chain.wants(&TYPING_START));

    // Subscribe to something else: the next question already sees it.
    write_package(&root.0, "hot", 1, "events = [\"dispatch:TYPING_*\"]");
    assert!(!chain.wants(&MESSAGE_CREATE));
    assert!(chain.wants(&TYPING_START));
    assert_eq!(
        chain.dispatch(&mut host, "TYPING_START", &mut Val::map(), &mut vec![]),
        Outcome::Changed
    );
    assert!(
        host.logs.iter().any(|l| l.contains("hot reloaded")),
        "{:?}",
        host.logs
    );

    // A broken new version leaves the old one running, and says why.
    write_package(&root.0, "hot", 1, "events = [\"nonsense\"]");
    host.logs.clear();
    assert_eq!(
        chain.dispatch(&mut host, "TYPING_START", &mut Val::map(), &mut vec![]),
        Outcome::Changed
    );
    assert!(
        host.logs
            .iter()
            .any(|l| l.contains("not reloaded") && l.contains("nonsense")),
        "{:?}",
        host.logs
    );
}

#[test]
fn a_reload_brings_back_a_faulted_plugin() {
    let root = TempDir::new();
    let dir = write_package(&root.0, "flaky", 1, "events = [\"dispatch\"]");
    let rt = common::runtime();
    let plugin = PackagePlugin::load(&rt, &dir).unwrap();
    plugin.set_check_interval(Duration::ZERO);
    let mut host = NullHost::default();
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    chain.dispatch(&mut host, "PANIC", &mut Val::map(), &mut vec![]);
    assert!(!chain.wants(&MESSAGE_CREATE), "disabled after the fault");

    write_package(&root.0, "flaky", 1, "events = [\"dispatch\"] # fixed");
    assert!(chain.wants(&MESSAGE_CREATE), "reload enables it again");
}
