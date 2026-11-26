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

#[test]
fn ui_events_reach_a_guest_as_a_name_and_a_payload() {
    let chain = chain_of(guest());
    let mut host = NullHost::default();
    let click = phoecord_core::UiEvent::MenuAction {
        entry: "copy_raw",
        channel_id: 1,
        message_id: 2,
    };
    assert_eq!(chain.ui(&mut host, click), Outcome::Untouched);
    assert_eq!(host.logs, vec!["ui menu_action copy_raw".to_string()]);
}

#[test]
fn a_guest_can_schedule_and_cancel_timers() {
    let chain = chain_of(guest());
    let mut host = NullHost::default();
    let out = chain.dispatch(&mut host, "SCHEDULE", &mut Val::map(), &mut vec![]);
    assert_eq!(
        out,
        Outcome::Replied {
            by: "test_guest".into(),
            status: 200,
            body: None
        }
    );
    assert_eq!(
        host.scheduled,
        vec![(1000, "tick".to_string()), (200, "tock".to_string())]
    );
    // The due timer comes back to this plugin alone as a dispatch.
    let out = chain.dispatch_to(
        &mut host,
        "test_guest",
        phoecord_core::TIMER_EVENT,
        &mut Val::map(),
        &mut vec![],
    );
    assert_eq!(out, Some(Outcome::Untouched));
    assert_eq!(host.logs, vec!["timer fired".to_string()]);
}

mod limits {
    use super::*;
    use phoecord_core::{Event, EventKind, Host, Manifest, Verdict};
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
        Box::new(Native(Manifest::new(
            "native",
            "native",
            "0",
            String::new(),
        )))
    }

    /// The guest faults on `event` (hard ceiling 200 ms, 32 MiB). It is switched off, logged once
    /// and the event passes; the native plugin behind it still runs, now and on later events.
    fn assert_disabled_and_chain_alive(event: &str) {
        let rt = common::quick_runtime();
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
    fn the_hard_ceiling_is_configurable() {
        let rt = WasmRuntime::with_limits(WasmLimits {
            hard_timeout: Duration::from_millis(300),
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
        assert!(took >= Duration::from_millis(290), "{took:?}");
        assert!(took < Duration::from_secs(3), "{took:?}");
    }

    /// Every event overruns a zero soft limit, so these tests do not depend on how fast the
    /// machine is.
    fn overrunning(window: Duration) -> (WasmPlugin, NullHost) {
        let rt = WasmRuntime::with_limits(WasmLimits {
            event_timeout: Duration::ZERO,
            overrun_window: window,
            ..WasmLimits::default()
        })
        .unwrap();
        (
            WasmPlugin::from_bytes(&rt, guest_wasm()).unwrap(),
            NullHost::default(),
        )
    }

    fn noop(plugin: &WasmPlugin, host: &mut NullHost) {
        let mut payload = Val::map();
        plugin.on_event(
            host,
            Event::Dispatch {
                name: "NOOP",
                payload: &mut payload,
                extra: &mut vec![],
            },
        );
    }

    #[test]
    fn a_single_slow_event_is_logged_but_not_fatal() {
        let (plugin, mut host) = overrunning(Duration::from_secs(60));
        noop(&plugin, &mut host);
        assert!(!plugin.is_disabled());
        assert_eq!(host.logs.len(), 1, "{:?}", host.logs);
        assert!(host.logs[0].contains("overrun 1 of 3"), "{:?}", host.logs);
        assert!(host.logs[0].contains("took"), "{:?}", host.logs);
        // Healthy again: it still answers.
        let chain = chain_of(plugin);
        let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut Val::map(), &mut vec![]);
        assert_eq!(out, Outcome::Changed);
    }

    #[test]
    fn repeated_overruns_disable_the_plugin() {
        let (plugin, mut host) = overrunning(Duration::from_secs(60));
        noop(&plugin, &mut host);
        noop(&plugin, &mut host);
        assert!(!plugin.is_disabled());
        noop(&plugin, &mut host);
        assert!(plugin.is_disabled(), "{:?}", host.logs);
        assert!(
            host.logs
                .iter()
                .any(|l| l.contains("test_guest is disabled until restart")),
            "{:?}",
            host.logs
        );
    }

    #[test]
    fn overruns_spread_over_time_do_not_add_up() {
        let (plugin, mut host) = overrunning(Duration::from_millis(40));
        for _ in 0..6 {
            noop(&plugin, &mut host);
            std::thread::sleep(Duration::from_millis(80));
        }
        assert!(!plugin.is_disabled(), "{:?}", host.logs);
    }
}

/// A host that knows the query string of the REST call being handled.
struct QueryHost(Option<String>);

impl phoecord_core::Host for QueryHost {
    fn setting(&self, _: &str) -> Option<Val> {
        None
    }
    fn kv_get(&self, _: &str) -> Option<Val> {
        None
    }
    fn kv_set(&mut self, _: &str, _: Val) {}
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, _: &str, _: &str) {}
    fn log(&mut self, _: &str) {}
    fn emit_send(&mut self, _: i64, _: Val) {}
    fn guild_emojis(&self, _: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _: u64) -> Option<u64> {
        None
    }
    fn request_query(&self) -> Option<String> {
        self.0.clone()
    }
}

impl phoecord_core::ChainHost for QueryHost {}

#[test]
fn a_guest_reads_the_query_of_the_request_it_handles() {
    let chain = chain_of(guest());
    let mut host = QueryHost(Some("q=cat%20gif&limit=5".into()));
    assert_eq!(
        chain.rest_request(&mut host, "GET", "/query", None),
        Outcome::Replied {
            by: "test_guest".into(),
            status: 200,
            body: Some(Val::Str("q=cat%20gif&limit=5".into())),
        }
    );
    // A host without a query (or one that predates `request_query`) answers `none`.
    assert_eq!(
        chain.rest_request(&mut NullHost::default(), "GET", "/query", None),
        Outcome::Replied {
            by: "test_guest".into(),
            status: 404,
            body: None,
        }
    );
}
