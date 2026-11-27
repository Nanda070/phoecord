//! Gateway bridge: what a registry handler detour hands to `Engine::on_dispatch`.
//!
//! The registry gives the already-decoded event, so there is no wire format here: the event name
//! and its `d` as JSON bytes go in, a verdict comes out. Whether the detour then calls the original
//! handler (`Pass`, `Replace`) or skips it (`Drop`) is the detour's job.

use phoecord_core::Val;
use phoecord_transport::Engine;

#[derive(Debug, PartialEq)]
pub enum GatewayAction {
    /// Call the original handler with the original data.
    Pass,
    /// Call it with this `d` instead.
    Replace(Vec<u8>),
    /// Deliver it as another event: this name and this `d`. Only from [`dispatch`], when a plugin
    /// changed `t` (MessageLogger shows a deleted message by turning its `MESSAGE_DELETE` into a
    /// `MESSAGE_UPDATE`).
    Rename { name: String, data: Vec<u8> },
    /// Do not call it.
    Drop,
}

#[derive(Debug)]
pub struct GatewayOutcome {
    pub action: GatewayAction,
    /// Extra server → client payloads plugins queued; delivery belongs to the transport (2.2.7).
    pub extra: Vec<Val>,
    /// Client → server payloads plugins queued (`Host::emit_send`).
    pub sends: Vec<(i64, Val)>,
}

/// One dispatch event: `name` (`MESSAGE_CREATE`) and its `d` as JSON.
pub fn dispatch(engine: &Engine, name: &str, data: &[u8]) -> Result<GatewayOutcome, String> {
    if !engine.wants_gateway() {
        return Ok(GatewayOutcome {
            action: GatewayAction::Pass,
            extra: Vec::new(),
            sends: Vec::new(),
        });
    }
    let d: serde_json::Value =
        serde_json::from_slice(data).map_err(|e| format!("{name}: data is not JSON ({e})"))?;
    let mut payload = Val::map();
    payload.set("op", Val::Int(0));
    payload.set("t", Val::Str(name.to_string()));
    payload.set("d", Val::from_json(d));
    let r = engine.on_dispatch(payload);
    let action = if r.drop {
        GatewayAction::Drop
    } else if r.changed {
        let d = r.payload.get("d").unwrap_or(&Val::Null);
        let bytes = serde_json::to_vec(&d.to_json())
            .map_err(|e| format!("{name}: cannot encode data: {e}"))?;
        match r.payload.get("t").and_then(Val::as_str) {
            Some(t) if t != name => GatewayAction::Rename {
                name: t.to_string(),
                data: bytes,
            },
            _ => GatewayAction::Replace(bytes),
        }
    } else {
        GatewayAction::Pass
    };
    Ok(GatewayOutcome {
        action,
        extra: r.extra,
        sends: r.sends,
    })
}

/// One outgoing frame (2.2.4): the JSON text `{"op": …, "d": …}` the client's encoder produced.
/// `Replace` carries the whole new frame text; `Drop` means a plugin wants it not sent.
pub fn send(engine: &Engine, frame: &[u8]) -> Result<GatewayOutcome, String> {
    let json: serde_json::Value =
        serde_json::from_slice(frame).map_err(|e| format!("outgoing frame is not JSON ({e})"))?;
    let mut payload = Val::from_json(json);
    let op = payload.get("op").and_then(Val::as_i64);
    let r = engine.on_send(&mut payload);
    let action = if r.drop {
        GatewayAction::Drop
    } else if r.changed {
        let bytes = serde_json::to_vec(&payload.to_json())
            .map_err(|e| format!("op {op:?}: cannot encode the frame: {e}"))?;
        GatewayAction::Replace(bytes)
    } else {
        GatewayAction::Pass
    };
    Ok(GatewayOutcome {
        action,
        extra: r.locals,
        sends: r.sends,
    })
}

/// An incoming frame's parts when it is a dispatch (`op` 0): event name, sequence number and `d`
/// as JSON text. `None` for every other opcode and for text that is not a frame.
pub fn split_dispatch(frame: &[u8]) -> Option<(String, Option<i64>, Vec<u8>)> {
    let v: serde_json::Value = serde_json::from_slice(frame).ok()?;
    if v.get("op")?.as_i64()? != 0 {
        return None;
    }
    let name = v.get("t")?.as_str()?.to_string();
    let s = v.get("s").and_then(serde_json::Value::as_i64);
    let d = serde_json::to_vec(v.get("d").unwrap_or(&serde_json::Value::Null)).ok()?;
    Some((name, s, d))
}

/// The frame text of a dispatch with a new `d`.
pub fn rebuild(name: &str, s: Option<i64>, d: &[u8]) -> Result<Vec<u8>, String> {
    let d: serde_json::Value =
        serde_json::from_slice(d).map_err(|e| format!("{name}: new data is not JSON ({e})"))?;
    serde_json::to_vec(&serde_json::json!({ "t": name, "s": s, "op": 0, "d": d }))
        .map_err(|e| format!("{name}: cannot encode the frame: {e}"))
}

/// What a dropped dispatch becomes: an event name the client does not know, with the same
/// sequence number, so the session's `s` still advances (a resume would otherwise replay it).
pub const DROPPED_EVENT: &str = "PHOECORD_DROPPED";

pub fn dropped(s: Option<i64>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "t": DROPPED_EVENT, "s": s, "op": 0, "d": {} }))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::Chain;
    use phoecord_transport::{config::Config, host::Shared};

    fn engine(cfg: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!("phoecord-test-gwsend-{}", std::process::id()));
        let cfg: Config = toml::from_str(cfg).unwrap();
        Engine::new(
            Chain::new(phoecord_plugins::builtin()),
            Shared::new(dir),
            cfg,
        )
    }

    #[test]
    fn a_frame_no_plugin_touches_passes() {
        let e = engine("");
        let out = send(
            &e,
            br#"{"op":3,"d":{"status":"online","since":0,"activities":[],"afk":false}}"#,
        )
        .unwrap();
        assert_eq!(out.action, GatewayAction::Pass);
    }

    #[test]
    fn a_voice_state_frame_reaches_the_chain_and_any_change_is_a_whole_frame() {
        // FakeDeafen looks at op 4; whether it changes it depends on its toggle.
        let e = engine("[plugins.fake_deafen]\nenabled = true\n");
        let frame = br#"{"op":4,"d":{"guild_id":"1","channel_id":"2","self_mute":false,"self_deaf":false}}"#;
        let out = send(&e, frame).unwrap();
        if let GatewayAction::Replace(b) = out.action {
            let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
            assert_eq!(v["op"], 4);
            assert!(v["d"].is_object());
        }
    }

    #[test]
    fn a_frame_that_is_not_json_is_an_error() {
        let e = engine("");
        assert!(send(&e, b"not json").is_err());
    }

    #[test]
    fn dispatch_frames_split_and_rebuild() {
        let f = br#"{"t":"MESSAGE_CREATE","s":42,"op":0,"d":{"content":"hi"}}"#;
        let (name, s, d) = split_dispatch(f).unwrap();
        assert_eq!((name.as_str(), s), ("MESSAGE_CREATE", Some(42)));
        assert_eq!(d, br#"{"content":"hi"}"#);
        let back: serde_json::Value =
            serde_json::from_slice(&rebuild(&name, s, br#"{"content":"bye"}"#).unwrap()).unwrap();
        assert_eq!(back["t"], "MESSAGE_CREATE");
        assert_eq!(back["s"], 42);
        assert_eq!(back["op"], 0);
        assert_eq!(back["d"]["content"], "bye");
    }

    #[test]
    fn only_dispatches_split() {
        assert!(split_dispatch(br#"{"t":null,"s":null,"op":11,"d":null}"#).is_none());
        assert!(split_dispatch(b"not json").is_none());
    }

    #[test]
    fn a_dropped_dispatch_keeps_its_sequence_number() {
        let v: serde_json::Value = serde_json::from_slice(&dropped(Some(7))).unwrap();
        assert_eq!(
            (v["t"].as_str(), v["s"].as_i64(), v["op"].as_i64()),
            (Some(DROPPED_EVENT), Some(7), Some(0))
        );
    }

    #[test]
    fn a_renamed_event_keeps_its_new_name() {
        // MessageLogger shows a deleted message by turning the MESSAGE_DELETE into a
        // MESSAGE_UPDATE; the inflate hook must rebuild the frame under the new name, or the
        // client gets a MESSAGE_DELETE and removes the message anyway.
        let e = engine("[plugins.message_logger]\nenabled = true\nwrite_log = false\n");
        let created = br#"{"id":"5","channel_id":"2","author":{"id":"9"},"content":"hi"}"#;
        assert_eq!(
            dispatch(&e, "MESSAGE_CREATE", created).unwrap().action,
            GatewayAction::Pass
        );
        let out = dispatch(&e, "MESSAGE_DELETE", br#"{"id":"5","channel_id":"2"}"#).unwrap();
        let GatewayAction::Rename { name, data } = out.action else {
            panic!("expected a rename, got {:?}", out.action);
        };
        assert_eq!(name, "MESSAGE_UPDATE");
        let frame: serde_json::Value =
            serde_json::from_slice(&rebuild(&name, Some(8), &data).unwrap()).unwrap();
        assert_eq!(frame["t"], "MESSAGE_UPDATE");
        assert_eq!(frame["s"], 8);
        assert_eq!(frame["d"]["id"], "5");
        assert!(frame["d"]["content"].as_str().unwrap().contains("hi"));
    }

    #[test]
    fn a_change_that_keeps_the_name_is_a_replace() {
        // FakeNitro edits READY in place: same event, new `d`.
        let e = engine("[plugins.fake_nitro]\nenabled = true\n");
        let out = dispatch(&e, "READY", br#"{"user":{"id":"1","premium_type":0}}"#).unwrap();
        let GatewayAction::Replace(d) = out.action else {
            panic!("expected a replace, got {:?}", out.action);
        };
        let d: serde_json::Value = serde_json::from_slice(&d).unwrap();
        assert_eq!(d["user"]["premium_type"], 2);
    }
}
