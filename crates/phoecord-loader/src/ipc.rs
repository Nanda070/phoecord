//! Loader ↔ proxy IPC: the message types, the line codec and the retry schedule.
//!
//! The loader (inside the Discord client) is the *client* and `phoecord run` / `launch` is the
//! *server* of a per-user Windows named pipe ([`crate::pipe`]). This module has no OS code, so
//! both sides use it (the proxy depends on this crate with the `dll` feature off) and the tests run
//! on every OS. It is the one place that defines the protocol.
//!
//! Wire format: JSON lines, one message per line, UTF-8, `\n` terminated. Every message is an
//! object with a protocol version `v` and a `type`:
//!
//! | `type` | direction | fields |
//! |:---|:---|:---|
//! | `hello` | loader → proxy | `loader_version`, `client_version?`, `text_hash`, `pid`, `detect_ms` |
//! | `status` | loader → proxy | `ui` (bool), `reason?` (why it is off) |
//! | `settings.get` | loader → proxy | `id`, `key` |
//! | `settings.set` | loader → proxy | `id`, `key`, `value` (a JSON scalar) |
//! | `settings.value` | proxy → loader | `id`, `key`, `value?` (absent: unset), `error?` |
//! | `ui.event` | loader → proxy | `name`, `payload` (any JSON) |
//!
//! A line longer than [`MAX_LINE`] bytes, invalid JSON, an unknown `type` or another `v` is an
//! error, and the receiver drops the connection.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Read};

/// Version of the protocol, sent as `v` in every message.
pub const PROTOCOL_VERSION: u32 = 1;

/// The longest line (without the newline) either side accepts or sends.
pub const MAX_LINE: usize = 64 * 1024;

/// A message without its version; [`Envelope`] adds `v`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Msg {
    /// First message of a connection: who the loader is and which client build it sits in.
    #[serde(rename = "hello")]
    Hello {
        loader_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_version: Option<String>,
        /// SHA-256 of the client's `.text` section (lowercase hex).
        text_hash: String,
        pid: u32,
        /// How long build detection took.
        detect_ms: u64,
    },
    /// Whether the UI layer is on and, when it is off, why ("no signatures").
    #[serde(rename = "status")]
    Status {
        ui: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Ask for a setting of the loader's own section.
    #[serde(rename = "settings.get")]
    SettingsGet { id: u64, key: String },
    /// Store a setting of the loader's own section.
    #[serde(rename = "settings.set")]
    SettingsSet { id: u64, key: String, value: Value },
    /// The proxy's answer to `settings.get` and `settings.set`.
    #[serde(rename = "settings.value")]
    SettingsValue {
        id: u64,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Something happened in the client UI. Logged and ignored for now (Phase 2 uses it).
    #[serde(rename = "ui.event")]
    UiEvent { name: String, payload: Value },
}

/// A message on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub msg: Msg,
}

/// Why a line was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum LineError {
    TooLong,
    NotUtf8OrJson(String),
    Version(u32),
}

impl std::fmt::Display for LineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineError::TooLong => write!(f, "line longer than {MAX_LINE} bytes"),
            LineError::NotUtf8OrJson(e) => write!(f, "not a valid message: {e}"),
            LineError::Version(v) => write!(f, "protocol version {v}, expected {PROTOCOL_VERSION}"),
        }
    }
}

impl std::error::Error for LineError {}

/// `msg` as one line including the `\n`. Fails when the line would exceed [`MAX_LINE`].
pub fn encode_line(msg: &Msg) -> Result<Vec<u8>, LineError> {
    let env = Envelope {
        v: PROTOCOL_VERSION,
        msg: msg.clone(),
    };
    let mut out = serde_json::to_vec(&env).map_err(|e| LineError::NotUtf8OrJson(e.to_string()))?;
    if out.len() > MAX_LINE {
        return Err(LineError::TooLong);
    }
    out.push(b'\n');
    Ok(out)
}

/// One received line (with or without its `\r\n`) as a message.
pub fn decode_line(line: &[u8]) -> Result<Msg, LineError> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.len() > MAX_LINE {
        return Err(LineError::TooLong);
    }
    let env: Envelope =
        serde_json::from_slice(line).map_err(|e| LineError::NotUtf8OrJson(e.to_string()))?;
    if env.v != PROTOCOL_VERSION {
        return Err(LineError::Version(env.v));
    }
    Ok(env.msg)
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Read1 {
    /// The peer closed the connection before any byte of a new line.
    Eof,
    /// A line (newline included when it was there).
    Line(Vec<u8>),
}

/// Reads one line, never buffering more than [`MAX_LINE`] bytes plus the newline: a peer that
/// sends endless data without a newline gets [`LineError::TooLong`] after 64 KiB, not memory.
pub fn read_line<R: BufRead>(r: &mut R) -> io::Result<Result<Read1, LineError>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE as u64 + 2)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Ok(Read1::Eof));
    }
    let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    if body.len() > MAX_LINE {
        return Ok(Err(LineError::TooLong));
    }
    Ok(Ok(Read1::Line(buf)))
}

/// `\\.\pipe\phoecord-<user SID>`: one pipe per user, so two users on one machine never meet.
pub fn pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\phoecord-{user_sid}")
}

/// Waits (ms) before each connection attempt of the loader; after the last one it gives up. The
/// proxy usually runs already (the client is started by `phoecord launch`), and when it starts a
/// moment later the later attempts catch it.
pub const CONNECT_DELAYS_MS: [u64; 5] = [0, 500, 2_000, 5_000, 15_000];

/// What the proxy logs for a connected loader.
pub fn describe_connection(hello: &Msg, status: Option<&Msg>) -> Option<String> {
    let Msg::Hello {
        loader_version,
        client_version,
        text_hash,
        pid,
        detect_ms,
    } = hello
    else {
        return None;
    };
    let ui = match status {
        Some(Msg::Status { ui: true, .. }) => "UI layer on".to_string(),
        Some(Msg::Status {
            ui: false,
            reason: Some(r),
        }) => format!("UI layer off ({r})"),
        Some(Msg::Status {
            ui: false,
            reason: None,
        }) => "UI layer off".to_string(),
        _ => "UI layer status not reported".to_string(),
    };
    Some(format!(
        "loader connected: build {}, hash {text_hash}, {ui} (loader {loader_version}, pid {pid}, detected in {detect_ms} ms)",
        client_version.as_deref().unwrap_or("unknown"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn samples() -> Vec<Msg> {
        vec![
            Msg::Hello {
                loader_version: "0.5.0".into(),
                client_version: Some("1.0.1355".into()),
                text_hash: "ab12".into(),
                pid: 4242,
                detect_ms: 9,
            },
            Msg::Hello {
                loader_version: "0.5.0".into(),
                client_version: None,
                text_hash: "ab12".into(),
                pid: 1,
                detect_ms: 0,
            },
            Msg::Status {
                ui: false,
                reason: Some("no signatures".into()),
            },
            Msg::Status {
                ui: true,
                reason: None,
            },
            Msg::SettingsGet {
                id: 1,
                key: "ui".into(),
            },
            Msg::SettingsSet {
                id: 2,
                key: "ui".into(),
                value: json!(true),
            },
            Msg::SettingsValue {
                id: 2,
                key: "ui".into(),
                value: Some(json!("x")),
                error: None,
            },
            Msg::UiEvent {
                name: "settings.opened".into(),
                payload: json!({"tab": 3, "list": [1, 2]}),
            },
        ]
    }

    #[test]
    fn every_message_round_trips_as_one_line() {
        for m in samples() {
            let line = encode_line(&m).unwrap();
            assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
            assert!(line.ends_with(b"\n"));
            assert_eq!(decode_line(&line).unwrap(), m);
        }
    }

    #[test]
    fn the_wire_format_is_the_documented_one() {
        let line = encode_line(&Msg::Status {
            ui: false,
            reason: Some("no signatures".into()),
        })
        .unwrap();
        let v: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(
            v,
            json!({"v": 1, "type": "status", "ui": false, "reason": "no signatures"})
        );
        let get = decode_line(br#"{"v":1,"type":"settings.get","id":7,"key":"k"}"#).unwrap();
        assert_eq!(
            get,
            Msg::SettingsGet {
                id: 7,
                key: "k".into()
            }
        );
        // CRLF is accepted.
        assert!(decode_line(b"{\"v\":1,\"type\":\"status\",\"ui\":true}\r\n").is_ok());
    }

    #[test]
    fn garbage_is_refused() {
        assert!(decode_line(b"hello").is_err());
        assert!(decode_line(b"").is_err());
        assert!(decode_line(&[0xff, 0xfe, b'\n']).is_err());
        assert!(decode_line(br#"{"v":1,"type":"nope"}"#).is_err());
        assert!(decode_line(br#"{"type":"status","ui":true}"#).is_err());
        assert_eq!(
            decode_line(br#"{"v":2,"type":"status","ui":true}"#),
            Err(LineError::Version(2))
        );
    }

    #[test]
    fn a_line_over_the_cap_is_refused_on_both_sides() {
        let big = Msg::UiEvent {
            name: "x".into(),
            payload: json!("a".repeat(MAX_LINE)),
        };
        assert_eq!(encode_line(&big), Err(LineError::TooLong));
        let mut long = vec![b'a'; MAX_LINE + 1];
        assert_eq!(decode_line(&long), Err(LineError::TooLong));
        // A line of exactly the cap passes the length check (and fails as JSON).
        long.truncate(MAX_LINE);
        assert!(matches!(
            decode_line(&long),
            Err(LineError::NotUtf8OrJson(_))
        ));
    }

    #[test]
    fn the_reader_stops_at_the_cap_without_buffering_the_rest() {
        let mut endless = Cursor::new(vec![b'x'; MAX_LINE * 4]);
        assert_eq!(read_line(&mut endless).unwrap(), Err(LineError::TooLong));
        assert!(
            endless.position() <= MAX_LINE as u64 + 2,
            "read {} bytes",
            endless.position()
        );

        let ok = format!("{}\n", "a".repeat(MAX_LINE));
        let mut c = Cursor::new(ok.into_bytes());
        assert!(
            matches!(read_line(&mut c).unwrap(), Ok(Read1::Line(l)) if l.len() == MAX_LINE + 1)
        );

        let mut two = Cursor::new(b"one\ntwo".to_vec());
        assert_eq!(
            read_line(&mut two).unwrap(),
            Ok(Read1::Line(b"one\n".to_vec()))
        );
        assert_eq!(
            read_line(&mut two).unwrap(),
            Ok(Read1::Line(b"two".to_vec()))
        );
        assert_eq!(read_line(&mut two).unwrap(), Ok(Read1::Eof));
    }

    #[test]
    fn the_pipe_name_has_the_sid_and_the_log_line_says_what_the_task_asks() {
        assert_eq!(pipe_name("S-1-5-21-1"), r"\\.\pipe\phoecord-S-1-5-21-1");
        let all = samples();
        let (hello, status) = (&all[0], &all[2]);
        assert_eq!(
            describe_connection(hello, Some(status)).unwrap(),
            "loader connected: build 1.0.1355, hash ab12, UI layer off (no signatures) (loader 0.5.0, pid 4242, detected in 9 ms)"
        );
        assert!(
            describe_connection(hello, None)
                .unwrap()
                .contains("UI layer status not reported")
        );
        assert_eq!(describe_connection(status, None), None);
    }
}
