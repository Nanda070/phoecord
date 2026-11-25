//! The proxy's end of the loader ↔ proxy IPC (protocol: [`phoecord_loader::ipc`]).
//!
//! `phoecord run` / `launch` call [`spawn`], which owns the per-user named pipe on Windows and does
//! nothing elsewhere (the loader is Windows-only). Every connection is served on its own thread:
//! the proxy logs one `loader connected` line, answers `settings.get` / `settings.set` from
//! `config.toml` (the `[loader]` section only) and logs `ui.event` messages. Garbage, an
//! oversized line or a wrong protocol version drops the connection.
//!
//! The serving logic works on any `BufRead` / `Write`, so the tests run on every OS; only the
//! pipe itself is Windows code, and its `unsafe` lives in `phoecord_loader::pipe`.

use crate::config::Config;
use phoecord_loader::ipc::{self, Msg, Read1};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where loader settings live in `config.toml`: `[loader]`.
pub const SECTION: &str = "loader";

/// Longest setting key.
const MAX_KEY: usize = 64;

/// Most loader connections served at once; one per client process is expected.
#[cfg(windows)]
const MAX_CONNECTIONS: usize = 8;

/// Where the proxy writes what the loader tells it (production: the `tracing` log).
pub type Sink = Arc<dyn Fn(String) + Send + Sync>;

/// The loader's settings, stored in the `[loader]` table of `config.toml`.
pub struct ConfigSettings {
    path: PathBuf,
    write: Mutex<()>,
}

impl ConfigSettings {
    pub fn new(path: PathBuf) -> ConfigSettings {
        ConfigSettings {
            path,
            write: Mutex::new(()),
        }
    }

    fn check_key(key: &str) -> Result<(), String> {
        let ok = !key.is_empty()
            && key.len() <= MAX_KEY
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if ok {
            Ok(())
        } else {
            Err(format!(
                "invalid key {key:?}: 1-{MAX_KEY} characters of letters, digits, _ and -"
            ))
        }
    }

    /// The value of `[loader] <key>`; `None` when it is not set.
    pub fn get(&self, key: &str) -> Result<Option<Value>, String> {
        Self::check_key(key)?;
        if !self.path.exists() {
            return Ok(None);
        }
        let cfg = Config::load(&self.path).map_err(|e| format!("{e:#}"))?;
        let Some(toml::Value::Table(t)) = cfg.extra.get(SECTION) else {
            return Ok(None);
        };
        match t.get(key) {
            Some(v) => serde_json::to_value(v).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    /// Sets `[loader] <key>`; only booleans, numbers and strings are stored.
    pub fn set(&self, key: &str, value: Value) -> Result<(), String> {
        Self::check_key(key)?;
        let v: toml::Value =
            serde_json::from_value(value).map_err(|_| "value must be a scalar".to_string())?;
        if matches!(
            v,
            toml::Value::Array(_) | toml::Value::Table(_) | toml::Value::Datetime(_)
        ) {
            return Err("value must be a boolean, a number or a string".into());
        }
        let _guard = self.write.lock().unwrap();
        let mut cfg = Config::load_or_init(&self.path).map_err(|e| format!("{e:#}"))?;
        let entry = cfg
            .extra
            .entry(SECTION)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(t) = entry else {
            return Err(format!("`{SECTION}` in the config is not a table"));
        };
        t.insert(key.to_string(), v);
        cfg.save(&self.path).map_err(|e| format!("{e:#}"))
    }
}

/// Serves one connection until the loader closes it or sends something invalid. Returns why it
/// ended, for the log.
pub fn serve_connection<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    settings: &ConfigSettings,
    sink: &Sink,
) -> String {
    let mut hello: Option<Msg> = None;
    let mut status: Option<Msg> = None;
    let mut announced = false;
    let announce = |hello: &Option<Msg>, status: &Option<Msg>, announced: &mut bool| {
        if !*announced
            && let Some(h) = hello
            && let Some(line) = ipc::describe_connection(h, status.as_ref())
        {
            *announced = true;
            sink(line);
        }
    };
    let end = loop {
        let line = match ipc::read_line(&mut reader) {
            Err(e) => break format!("read failed: {e}"),
            Ok(Err(e)) => break format!("dropped: {e}"),
            Ok(Ok(Read1::Eof)) => break "closed".to_string(),
            Ok(Ok(Read1::Line(l))) => l,
        };
        let msg = match ipc::decode_line(&line) {
            Ok(m) => m,
            Err(e) => break format!("dropped: {e}"),
        };
        match msg {
            Msg::Hello { .. } => hello = Some(msg),
            Msg::Status { .. } => {
                status = Some(msg);
                announce(&hello, &status, &mut announced);
            }
            Msg::SettingsGet { id, key } => {
                let reply = match settings.get(&key) {
                    Ok(value) => Msg::SettingsValue {
                        id,
                        key,
                        value,
                        error: None,
                    },
                    Err(error) => Msg::SettingsValue {
                        id,
                        key,
                        value: None,
                        error: Some(error),
                    },
                };
                if let Err(e) = reply_to(&mut writer, &reply) {
                    break format!("write failed: {e}");
                }
            }
            Msg::SettingsSet { id, key, value } => {
                let reply = match settings.set(&key, value.clone()) {
                    Ok(()) => Msg::SettingsValue {
                        id,
                        key,
                        value: Some(value),
                        error: None,
                    },
                    Err(error) => Msg::SettingsValue {
                        id,
                        key,
                        value: None,
                        error: Some(error),
                    },
                };
                if let Err(e) = reply_to(&mut writer, &reply) {
                    break format!("write failed: {e}");
                }
            }
            Msg::SettingsValue { .. } => break "dropped: a reply sent to the proxy".to_string(),
            Msg::UiEvent { name, payload } => {
                let payload = payload.to_string();
                let shown: String = payload.chars().take(200).collect();
                tracing::debug!("loader ui.event {name}: {shown}");
            }
        }
    };
    announce(&hello, &status, &mut announced);
    end
}

fn reply_to<W: Write>(w: &mut W, m: &Msg) -> std::io::Result<()> {
    let line = ipc::encode_line(m).map_err(std::io::Error::other)?;
    w.write_all(&line)?;
    w.flush()
}

/// Starts the pipe server for the current user on a background thread. On Windows a failure to
/// create the pipe (another `phoecord run` owns it) is logged and the proxy goes on without IPC;
/// elsewhere it does nothing.
pub fn spawn(config: PathBuf) {
    #[cfg(windows)]
    {
        let sink: Sink = Arc::new(|line| tracing::info!("{line}"));
        match phoecord_loader::pipe::current_pipe_name() {
            Ok(name) => spawn_on(&name, config, sink),
            Err(e) => tracing::warn!("loader IPC is off: cannot name the pipe: {e}"),
        }
    }
    #[cfg(not(windows))]
    let _ = config;
}

/// [`spawn`] on a named pipe of your choosing, with your own sink (the tests).
#[cfg(windows)]
pub fn spawn_on(pipe_name: &str, config: PathBuf, sink: Sink) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut server = match phoecord_loader::pipe::PipeServer::bind(pipe_name) {
        Ok(s) => s,
        Err(e) => {
            return tracing::warn!(
                "loader IPC is off: cannot create {pipe_name}: {e} (is another phoecord running?)"
            );
        }
    };
    tracing::debug!("loader IPC listens on {pipe_name}");
    let settings = Arc::new(ConfigSettings::new(config));
    let live = Arc::new(AtomicUsize::new(0));
    let spawned = std::thread::Builder::new()
        .name("phoecord-ipc".into())
        .spawn(move || {
            loop {
                let conn = match server.accept() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!("loader IPC accept: {e}");
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }
                };
                if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    live.fetch_sub(1, Ordering::SeqCst);
                    tracing::warn!("loader IPC: too many connections, one refused");
                    continue;
                }
                let (settings, sink, mine) = (settings.clone(), sink.clone(), live.clone());
                let spawned = std::thread::Builder::new()
                    .name("phoecord-ipc-conn".into())
                    .spawn(move || {
                        let reader = std::io::BufReader::new(&conn);
                        let end = serve_connection(reader, &conn, &settings, &sink);
                        tracing::debug!("loader connection {end}");
                        mine.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    // The closure (and the connection) was dropped with the failed spawn.
                    live.fetch_sub(1, Ordering::SeqCst);
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("loader IPC is off: cannot start its thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("phoecord-test-ipc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("config.toml")
    }

    fn script(msgs: &[Msg]) -> Vec<u8> {
        msgs.iter()
            .flat_map(|m| ipc::encode_line(m).unwrap())
            .collect()
    }

    fn run(input: Vec<u8>, settings: &ConfigSettings) -> (String, Vec<String>, Vec<Msg>) {
        let logged = Arc::new(Mutex::new(Vec::new()));
        let l2 = logged.clone();
        let sink: Sink = Arc::new(move |s| l2.lock().unwrap().push(s));
        let mut out = Vec::new();
        let end = serve_connection(Cursor::new(input), &mut out, settings, &sink);
        let replies = out
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| ipc::decode_line(l).unwrap())
            .collect();
        let logged = logged.lock().unwrap().clone();
        (end, logged, replies)
    }

    fn hello() -> Msg {
        Msg::Hello {
            loader_version: "0.5.0".into(),
            client_version: Some("1.0.1355".into()),
            text_hash: "ab12".into(),
            pid: 7,
            detect_ms: 12,
        }
    }

    #[test]
    fn hello_and_status_make_one_log_line() {
        let s = ConfigSettings::new(temp("log"));
        let (end, logged, _) = run(
            script(&[
                hello(),
                Msg::Status {
                    ui: false,
                    reason: Some("no signatures".into()),
                },
            ]),
            &s,
        );
        assert_eq!(end, "closed");
        assert_eq!(logged.len(), 1);
        assert!(
            logged[0].starts_with(
                "loader connected: build 1.0.1355, hash ab12, UI layer off (no signatures)"
            ),
            "{logged:?}"
        );
    }

    #[test]
    fn a_hello_without_status_is_still_logged_once_at_the_end() {
        let s = ConfigSettings::new(temp("nostatus"));
        let (_, logged, _) = run(script(&[hello()]), &s);
        assert_eq!(logged.len(), 1);
        assert!(logged[0].contains("UI layer status not reported"));
        // Nothing but status: nothing to say.
        let (_, logged, _) = run(
            script(&[Msg::Status {
                ui: true,
                reason: None,
            }]),
            &s,
        );
        assert!(logged.is_empty());
    }

    #[test]
    fn settings_are_answered_from_config_toml_and_only_in_the_loader_section() {
        let path = temp("settings");
        std::fs::write(&path, "language = \"ru\"\n[loader]\nui = false\n").unwrap();
        let s = ConfigSettings::new(path.clone());
        let (_, _, replies) = run(
            script(&[
                Msg::SettingsGet {
                    id: 1,
                    key: "ui".into(),
                },
                Msg::SettingsGet {
                    id: 2,
                    key: "missing".into(),
                },
                Msg::SettingsSet {
                    id: 3,
                    key: "theme".into(),
                    value: json!("dark"),
                },
                Msg::SettingsGet {
                    id: 4,
                    key: "theme".into(),
                },
                Msg::SettingsGet {
                    id: 5,
                    key: "plugins.no_track".into(),
                },
                Msg::SettingsSet {
                    id: 6,
                    key: "x".into(),
                    value: json!({"a": 1}),
                },
                Msg::SettingsSet {
                    id: 7,
                    key: "x".into(),
                    value: json!(null),
                },
            ]),
            &s,
        );
        let val = |m: &Msg| match m {
            Msg::SettingsValue {
                id, value, error, ..
            } => (*id, value.clone(), error.is_some()),
            _ => panic!("not a reply: {m:?}"),
        };
        assert_eq!(val(&replies[0]), (1, Some(json!(false)), false));
        assert_eq!(val(&replies[1]), (2, None, false));
        assert_eq!(val(&replies[2]), (3, Some(json!("dark")), false));
        assert_eq!(val(&replies[3]), (4, Some(json!("dark")), false));
        assert_eq!(val(&replies[4]), (5, None, true), "a dotted key is refused");
        assert_eq!(val(&replies[5]), (6, None, true), "objects are refused");
        assert_eq!(val(&replies[6]), (7, None, true), "null is refused");
        // The write kept everything else in the file.
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.language.as_deref(), Some("ru"));
        assert!(cfg.is_enabled("no_track"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[loader]") && text.contains("theme = \"dark\""),
            "{text}"
        );
    }

    #[test]
    fn garbage_oversize_and_wrong_version_drop_the_connection() {
        let s = ConfigSettings::new(temp("garbage"));
        let (end, logged, _) = run(b"not json\n".to_vec(), &s);
        assert!(end.starts_with("dropped:"), "{end}");
        assert!(logged.is_empty());
        let mut big = vec![b'x'; ipc::MAX_LINE * 2];
        big.push(b'\n');
        let (end, ..) = run(big, &s);
        assert!(end.contains("longer than"), "{end}");
        let (end, ..) = run(br#"{"v":9,"type":"status","ui":true}"#.to_vec(), &s);
        assert!(end.contains("protocol version 9"), "{end}");
        // Messages after the garbage are never read.
        let mut input = b"junk\n".to_vec();
        input.extend(script(&[hello()]));
        let (_, logged, _) = run(input, &s);
        assert!(logged.is_empty());
        // A reply sent to the proxy is a protocol error.
        let (end, ..) = run(
            script(&[Msg::SettingsValue {
                id: 1,
                key: "k".into(),
                value: None,
                error: None,
            }]),
            &s,
        );
        assert!(end.starts_with("dropped:"));
    }
}
