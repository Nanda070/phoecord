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

mod live {
    use super::*;
    use phoecord_plugin_runtime::package::PackageSet;

    fn ids(chain: &Chain) -> Vec<String> {
        chain.ids()
    }

    #[test]
    fn a_package_added_after_start_is_loaded_and_a_removed_one_is_gone() {
        let root = TempDir::new();
        write_package(&root.0, "first", 1, "events = [\"dispatch\"]");
        let chain = Chain::default();
        let mut set = PackageSet::new(common::runtime(), root.0.clone());

        let logs = set.sync(&chain);
        assert_eq!(ids(&chain), vec!["first"], "{logs:?}");
        assert!(logs[0].contains("first loaded"), "{logs:?}");
        // Nothing changed: nothing logged, the same instance keeps running.
        let before = chain.dynamic_plugins();
        assert!(set.sync(&chain).is_empty());
        assert!(std::sync::Arc::ptr_eq(
            &before[0],
            &chain.dynamic_plugins()[0]
        ));

        // A new folder is seen on the next scan, after the running one.
        write_package(&root.0, "second", 1, "events = [\"dispatch\"]");
        let logs = set.sync(&chain);
        assert_eq!(ids(&chain), vec!["first", "second"], "{logs:?}");
        assert!(std::sync::Arc::ptr_eq(
            &before[0],
            &chain.dynamic_plugins()[0]
        ));
        let mut host = NullHost::default();
        assert_eq!(
            chain.dispatch(&mut host, "MESSAGE_CREATE", &mut Val::map(), &mut vec![]),
            Outcome::Changed
        );

        // A removed folder is unloaded.
        std::fs::remove_dir_all(root.0.join("first")).unwrap();
        let logs = set.sync(&chain);
        assert_eq!(ids(&chain), vec!["second"]);
        assert!(
            logs.iter().any(|l| l.contains("first unloaded")),
            "{logs:?}"
        );
    }

    #[test]
    fn a_broken_new_package_is_reported_once_and_does_not_break_the_chain() {
        let root = TempDir::new();
        write_package(&root.0, "good", 1, "events = [\"dispatch\"]");
        let chain = Chain::default();
        let mut set = PackageSet::new(common::runtime(), root.0.clone());
        set.sync(&chain);

        // Wrong API version, and a package whose wasm is not there yet (still being copied).
        write_package(&root.0, "bad", 2, "");
        write_package(&root.0, "partial", 1, "events = [\"dispatch\"]");
        std::fs::remove_file(root.0.join("partial/plugin.wasm")).unwrap();
        let logs = set.sync(&chain);
        assert_eq!(ids(&chain), vec!["good"]);
        assert_eq!(logs.len(), 2, "{logs:?}");
        assert!(logs.iter().any(|l| l.contains("API v2")), "{logs:?}");
        // Not retried (and not logged again) until its files change.
        assert!(set.sync(&chain).is_empty());
        let mut host = NullHost::default();
        assert_eq!(
            chain.dispatch(&mut host, "MESSAGE_CREATE", &mut Val::map(), &mut vec![]),
            Outcome::Changed
        );

        // The copy finishes: the package loads on the next scan.
        std::fs::copy(
            root.0.join("good/plugin.wasm"),
            root.0.join("partial/plugin.wasm"),
        )
        .unwrap();
        set.sync(&chain);
        assert_eq!(ids(&chain), vec!["good", "partial"]);
    }

    #[test]
    fn built_in_plugins_come_first_and_keep_their_ids() {
        use phoecord_core::{EventKind, Host, Manifest, Verdict};
        struct Builtin(Manifest);
        impl Plugin for Builtin {
            fn manifest(&self) -> &Manifest {
                &self.0
            }
            fn wants(&self, _: &EventKind) -> bool {
                true
            }
            fn on_event(&self, _: &mut dyn Host, _: phoecord_core::Event<'_>) -> Verdict {
                Verdict::Pass
            }
        }
        let root = TempDir::new();
        write_package(&root.0, "taken", 1, "events = [\"dispatch\"]");
        write_package(&root.0, "free", 1, "events = [\"dispatch\"]");
        let manifest = |id: &str| Manifest::new(id, id, "0", String::new());
        let chain = Chain::new(vec![Box::new(Builtin(manifest("taken"))) as Box<dyn Plugin>]);
        let mut set = PackageSet::new(common::runtime(), root.0.clone());
        let logs = set.sync(&chain);
        assert_eq!(ids(&chain), vec!["taken", "free"]);
        assert!(
            logs.iter().any(|l| l.contains("taken not loaded")),
            "{logs:?}"
        );
        // Enabling still works across both segments.
        chain.set_enabled(["free".to_string()]);
        assert_eq!(chain.enabled_ids(), vec!["free"]);
    }
}
