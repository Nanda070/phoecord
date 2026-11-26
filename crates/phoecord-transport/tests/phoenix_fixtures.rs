//! The Phoenix gateway fixtures (`tests/fixtures/phoenix/`, task 2.1.4) load, have the shape the
//! recon notes describe, and pass through the engine with the plugins that read these events.

use phoecord_core::Val;
use phoecord_plugin_runtime::Chain;
use phoecord_transport::{Engine, config::Config, host::Shared};
use std::path::PathBuf;

fn fixture(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/phoenix")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}.json is not JSON: {e}"))
}

fn engine() -> Engine {
    let dir = std::env::temp_dir().join(format!(
        "phoecord-test-phoenix-fixtures-{}",
        std::process::id()
    ));
    let chain = Chain::new(vec![
        Box::new(phoecord_plugins::message_logger::MessageLogger::new()),
        Box::new(phoecord_plugins::platform_indicators::PlatformIndicators::new()),
    ]);
    Engine::new(chain, Shared::new(dir), Config::default())
}

#[test]
fn hello_carries_the_heartbeat() {
    let v = fixture("HELLO");
    assert_eq!(v["op"], 10);
    assert!(v["d"]["heartbeat_interval"].is_u64());
}

#[test]
fn ready_is_partial_on_phoenix() {
    let v = fixture("READY");
    assert_eq!(
        (v["op"].as_i64(), v["t"].as_str()),
        (Some(0), Some("READY"))
    );
    let d = &v["d"];
    assert_eq!(d["v"], 9);
    assert!(d["session_id"].is_string() && d["user"]["id"].is_string());
    let guilds = d["guilds"].as_array().expect("guilds");
    assert!(!guilds.is_empty());
    // Phoenix asks for a partial READY: changes against its cache, no channel or role lists.
    for g in guilds {
        assert_eq!(g["data_mode"], "partial");
        assert!(g.get("channels").is_none() && g.get("roles").is_none());
        assert!(g["channel_updates"].is_object() || g["channel_updates"].is_array());
    }
    assert!(d.get("user_settings_proto").is_none());
}

#[test]
fn message_create_has_the_fields_plugins_read() {
    let v = fixture("MESSAGE_CREATE");
    assert_eq!(v["t"], "MESSAGE_CREATE");
    let d = &v["d"];
    for key in ["id", "channel_id", "content", "timestamp"] {
        assert!(d[key].is_string(), "{key}");
    }
    assert!(d["author"]["id"].is_string());
}

#[test]
fn the_engine_takes_every_fixture() {
    let e = engine();
    for name in ["HELLO", "READY", "MESSAGE_CREATE"] {
        let r = e.on_dispatch(Val::from_json(fixture(name)));
        assert!(!r.drop, "{name} was dropped");
    }
}
