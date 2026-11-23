mod common;

use common::guest_wasm;
use phoecord_core::{Plugin, Val};
use phoecord_plugin_runtime::wasm::{WasmPlugin, WasmRuntime};
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

fn chain_of(plugin: WasmPlugin) -> Chain {
    Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>])
}

fn guest() -> WasmPlugin {
    let rt = common::runtime();
    WasmPlugin::from_bytes(&rt, guest_wasm()).unwrap()
}

fn touched() -> Val {
    Val::Map(vec![(Val::Str("touched".into()), Val::Bool(true))])
}

#[test]
fn well_behaved_guest_works_in_the_chain_like_a_native_plugin() {
    let plugin = guest();
    assert_eq!(plugin.manifest().id, "test_guest");
    let chain = chain_of(plugin);
    let mut host = NullHost::default();

    // Dispatch: payload replaced, one extra payload, a host log reached the real host.
    let mut payload = Val::map();
    let mut extra = vec![];
    let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut payload, &mut extra);
    assert_eq!(out, Outcome::Changed);
    assert_eq!(payload, touched());
    assert_eq!(extra, vec![touched()]);
    assert_eq!(host.logs, vec!["saw MESSAGE_CREATE".to_string()]);

    // REST request: a body is replaced, `/drop` is swallowed.
    let mut body = Val::map();
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body)),
        Outcome::Changed
    );
    assert_eq!(body, touched());
    assert_eq!(
        chain.rest_request(&mut host, "GET", "/drop", None),
        Outcome::Dropped {
            by: "test_guest".into()
        }
    );

    // The guest's own `wants` can say no.
    let mut payload = Val::map();
    assert_eq!(
        chain.dispatch(&mut host, "UNWANTED", &mut payload, &mut vec![]),
        Outcome::Untouched
    );
    assert_eq!(payload, Val::map());

    // Send and a REST answer reach the guest and pass.
    assert_eq!(
        chain.send(&mut host, 3, &mut Val::map()),
        Outcome::Untouched
    );
    assert_eq!(
        chain.rest_response(&mut host, "GET", "/x", 200, &mut Val::Null),
        Outcome::Untouched
    );
}

mod limits {
    use super::*;
    use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Verdict};
    use phoecord_plugin_runtime::wasm::WasmLimits;
    use std::time::{Duration, Instant};

    /// A native plugin that always answers `Changed`, to prove the chain carries on.
    struct Native(Manifest);

    impl Plugin for Native {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
            Verdict::Changed
        }
    }

    fn native() -> Box<dyn Plugin> {
        Box::new(Native(Manifest {
            id: "native".into(),
            name: "native".into(),
            version: "0".into(),
            description: String::new(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network],
            api: 1,
        }))
    }

    /// The guest faults on `event` (default limits: 5 ms, 32 MiB). It is switched off, logged once
    /// and the event passes; the native plugin behind it still runs, now and on later events.
    fn assert_disabled_and_chain_alive(event: &str) {
        let rt = common::runtime();
        let guest = WasmPlugin::from_bytes(&rt, guest_wasm()).unwrap();
        let chain = Chain::new(vec![Box::new(guest) as Box<dyn Plugin>, native()]);
        let mut host = NullHost::default();

        let started = Instant::now();
        let out = chain.dispatch(&mut host, event, &mut Val::map(), &mut vec![]);
        assert_eq!(
            out,
            Outcome::Changed,
            "{event}: only the native plugin changed"
        );
        assert!(started.elapsed() < Duration::from_secs(5), "{event} hung");
        let faults = host
            .logs
            .iter()
            .filter(|l| l.contains("test_guest is disabled until restart"))
            .count();
        assert_eq!(faults, 1, "{event}: {:?}", host.logs);

        // Later events skip the disabled plugin without calling it again.
        host.logs.clear();
        assert_eq!(
            chain.dispatch(&mut host, "MESSAGE_CREATE", &mut Val::map(), &mut vec![]),
            Outcome::Changed
        );
        assert!(host.logs.is_empty(), "{:?}", host.logs);
    }

    #[test]
    fn a_panicking_guest_is_disabled() {
        assert_disabled_and_chain_alive("PANIC");
    }

    #[test]
    fn an_endless_loop_is_interrupted() {
        assert_disabled_and_chain_alive("LOOP");
    }

    #[test]
    fn a_memory_hog_is_stopped_at_the_limit() {
        assert_disabled_and_chain_alive("HOG");
    }

    #[test]
    fn the_timeout_is_configurable() {
        let rt = WasmRuntime::with_limits(WasmLimits {
            event_timeout: Duration::from_millis(100),
            ..WasmLimits::default()
        })
        .unwrap();
        let chain = chain_of(WasmPlugin::from_bytes(&rt, guest_wasm()).unwrap());
        let t = Instant::now();
        chain.dispatch(
            &mut NullHost::default(),
            "LOOP",
            &mut Val::map(),
            &mut vec![],
        );
        let took = t.elapsed();
        assert!(took >= Duration::from_millis(90), "{took:?}");
        assert!(took < Duration::from_secs(3), "{took:?}");
    }
}
