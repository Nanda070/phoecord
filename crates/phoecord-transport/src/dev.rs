//! The dev console's side of the transport (task 1.3.9): a bus that fans out plugin log lines and
//! per-plugin run records to whoever is attached, and the last event of each kind so it can be
//! replayed into one plugin.
//!
//! Nothing here knows about sockets. A transport subscribes a sink and carries the entries to the
//! console; with no subscriber every call costs one atomic load, and no payload is copied.

use crate::Engine;
use crate::config::Config;
use crate::host::ProxyHost;
use phoecord_core::{Event, EventKind, Val};
use phoecord_plugin_runtime::Outcome;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many distinct event kinds are remembered for replay.
const MAX_REMEMBERED: usize = 64;

/// One line for the console.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DevEntry {
    /// A plugin's `Host::log` line.
    Log {
        ts: u64,
        plugin: String,
        message: String,
    },
    /// One plugin ran one event.
    Run {
        ts: u64,
        /// `rest_request`, `rest_response`, `dispatch` or `send`.
        kind: String,
        /// `POST /channels/1/messages`, `MESSAGE_CREATE`, `op 3`.
        event: String,
        plugin: String,
        /// `pass`, `changed`, `drop`, `reply 204` or `panic`.
        verdict: String,
        micros: u64,
    },
}

/// A sink gets every entry; it returns `false` when its end is gone, and is dropped.
pub type Sink = Box<dyn Fn(&DevEntry) -> bool + Send + Sync>;

/// An event kept so it can be run again (`replay`).
#[derive(Clone, Debug)]
pub enum Replay {
    RestRequest {
        method: String,
        route: String,
        body: Option<Val>,
    },
    RestResponse {
        method: String,
        route: String,
        status: u16,
        body: Val,
    },
    Dispatch {
        name: String,
        payload: Val,
    },
    Send {
        op: i64,
        payload: Val,
    },
}

#[derive(Default)]
struct Remembered {
    seq: u64,
    /// `<kind> <label>` → (sequence number, label, event)
    by_key: HashMap<String, (u64, String, Replay)>,
}

#[derive(Default)]
pub struct DevBus {
    sinks: Mutex<Vec<Sink>>,
    attached: AtomicUsize,
    dropped: AtomicU64,
    last: Mutex<Remembered>,
}

/// `(kind, event)` of an [`EventKind`], as the console shows them.
pub fn describe(kind: &EventKind) -> (&'static str, String) {
    match kind {
        EventKind::RestRequest { method, route } => ("rest_request", format!("{method} {route}")),
        EventKind::RestResponse { method, route } => ("rest_response", format!("{method} {route}")),
        EventKind::Dispatch { name } => ("dispatch", (*name).to_string()),
        EventKind::Send { op } => ("send", format!("op {op}")),
        EventKind::Ui { name } => ("ui", (*name).to_string()),
        other => ("other", format!("{other:?}")),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl DevBus {
    /// Whether anything is attached. Callers copy payloads only when this is true.
    pub fn active(&self) -> bool {
        self.attached.load(Ordering::Relaxed) > 0
    }

    /// How many entries sinks reported as lost (a console that reads too slowly).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn count_dropped(&self, n: u64) {
        self.dropped.fetch_add(n, Ordering::Relaxed);
    }

    pub fn subscribe(&self, sink: Sink) {
        let mut sinks = self.sinks.lock().unwrap();
        sinks.push(sink);
        self.attached.store(sinks.len(), Ordering::Relaxed);
    }

    fn publish(&self, entry: &DevEntry) {
        let mut sinks = self.sinks.lock().unwrap();
        sinks.retain(|s| s(entry));
        self.attached.store(sinks.len(), Ordering::Relaxed);
    }

    /// A plugin's log line.
    pub fn log(&self, plugin: &str, message: &str) {
        if self.active() {
            self.publish(&DevEntry::Log {
                ts: now_ms(),
                plugin: plugin.to_string(),
                message: message.to_string(),
            });
        }
    }

    /// One plugin's run of one event.
    pub fn run(&self, kind: &EventKind, plugin: &str, verdict: &str, elapsed: Duration) {
        if self.active() {
            let (kind, event) = describe(kind);
            self.publish(&DevEntry::Run {
                ts: now_ms(),
                kind: kind.to_string(),
                event,
                plugin: plugin.to_string(),
                verdict: verdict.to_string(),
                micros: elapsed.as_micros().min(u128::from(u64::MAX)) as u64,
            });
        }
    }

    /// Keep `event` as the last of its kind (only while somebody is attached; `event` is not
    /// called otherwise, so no payload is copied).
    pub fn remember(&self, kind: &'static str, label: String, event: impl FnOnce() -> Replay) {
        if !self.active() {
            return;
        }
        let mut last = self.last.lock().unwrap();
        last.seq += 1;
        let seq = last.seq;
        let key = format!("{kind} {label}");
        last.by_key.insert(key, (seq, label, event()));
        if last.by_key.len() > MAX_REMEMBERED
            && let Some(oldest) = last
                .by_key
                .iter()
                .min_by_key(|(_, (seq, _, _))| *seq)
                .map(|(k, _)| k.clone())
        {
            last.by_key.remove(&oldest);
        }
    }

    /// The newest remembered event whose label is `query` (case does not matter), else the newest
    /// whose label or `<kind> <label>` contains it. Returns its label too.
    pub fn recall(&self, query: &str) -> Option<(String, Replay)> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return None;
        }
        let last = self.last.lock().unwrap();
        let pick = |exact: bool| {
            last.by_key
                .iter()
                .filter(|(key, (_, label, _))| {
                    let (key, label) = (key.to_ascii_lowercase(), label.to_ascii_lowercase());
                    if exact {
                        label == q || key == q
                    } else {
                        key.contains(&q)
                    }
                })
                .max_by_key(|(_, (seq, _, _))| *seq)
                .map(|(_, (_, label, ev))| (label.clone(), ev.clone()))
        };
        pick(true).or_else(|| pick(false))
    }

    /// `<kind> <label>` of what is remembered, newest first.
    pub fn remembered(&self) -> Vec<String> {
        let last = self.last.lock().unwrap();
        let mut v: Vec<(u64, String)> = last
            .by_key
            .iter()
            .map(|(k, (seq, _, _))| (*seq, k.clone()))
            .collect();
        v.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
        v.into_iter().map(|(_, k)| k).collect()
    }
}

/// One row of the console's plugin list.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    /// `builtin` (part of the binary) or `package` (a WASM package that can be reloaded).
    pub source: &'static str,
    pub enabled: bool,
    /// Disabled for the session by a panic.
    pub crashed: bool,
}

/// What a replay did. Nothing a plugin queued (sends, local payloads) is delivered.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplayReport {
    pub event: String,
    pub plugin: String,
    pub verdict: String,
    pub dropped_sends: usize,
    pub dropped_locals: usize,
}

fn verdict_text(o: &Outcome) -> String {
    match o {
        Outcome::Untouched => "pass".into(),
        Outcome::Changed => "changed".into(),
        Outcome::Dropped { .. } => "drop".into(),
        Outcome::Replied { status, .. } => format!("reply {status}"),
    }
}

/// What the dev console can ask of the engine.
impl Engine {
    fn require_plugin(&self, id: &str) -> Result<(), String> {
        if self.chain.ids().iter().any(|p| p == id) {
            Ok(())
        } else {
            Err(format!("no plugin {id}"))
        }
    }

    /// Every plugin in chain order, with its state.
    pub fn plugin_list(&self) -> Vec<PluginInfo> {
        let cfg = self.cfg();
        self.chain
            .manifests()
            .into_iter()
            .map(|m| PluginInfo {
                enabled: cfg.is_enabled(&m.id),
                crashed: self.chain.is_crashed(&m.id),
                source: if self.chain.is_dynamic(&m.id) {
                    "package"
                } else {
                    "builtin"
                },
                id: m.id,
                name: m.name,
                version: m.version,
            })
            .collect()
    }

    /// Flip a plugin in `config.toml` and apply it at once. Returns the new state.
    pub fn toggle_plugin(&self, id: &str) -> Result<bool, String> {
        self.require_plugin(id)?;
        let on = !self.cfg().is_enabled(id);
        self.shared.persist_enabled(id, on)?;
        let cfg = Config::load(&self.shared.config_path).map_err(|e| format!("{e:#}"))?;
        self.apply(cfg);
        Ok(on)
    }

    /// Reload a WASM package from its folder now. The old version keeps running if the new one
    /// does not load. Compiling takes a while: call this off an async executor's threads.
    pub fn reload_plugin(&self, id: &str) -> Result<(), String> {
        self.require_plugin(id)?;
        if !self.chain.is_dynamic(id) {
            return Err(format!(
                "{id} is built in: only WASM packages can be reloaded"
            ));
        }
        let mut packages = self.packages.lock().unwrap();
        let set = packages
            .as_mut()
            .ok_or("this proxy does not watch a plugins folder")?;
        set.reload(&self.chain, id)?;
        tracing::info!("plugin {id} reloaded from the dev console");
        Ok(())
    }

    /// The plugin's section of `config.toml` as JSON (`null` when it has none).
    pub fn plugin_settings(&self, id: &str) -> Result<serde_json::Value, String> {
        self.require_plugin(id)?;
        let cfg = self.cfg();
        Ok(cfg
            .plugins
            .get(id)
            .and_then(|t| serde_json::to_value(t).ok())
            .unwrap_or(serde_json::Value::Null))
    }

    /// The plugin's `kv.json`.
    pub fn plugin_kv(&self, id: &str) -> Result<serde_json::Value, String> {
        self.require_plugin(id)?;
        Ok(self.shared.kv_dump(id))
    }

    /// Run the last event matching `query` into `plugin` alone, as a dry run: the plugin sees the
    /// event as it came in and its verdict is reported, but sends and local payloads it queues are
    /// discarded. What it does through the host itself (log, KV, `rest`, `fetch`, notify) is real.
    /// Only events seen while a console was attached can be replayed.
    pub fn replay(&self, query: &str, plugin: &str) -> Result<ReplayReport, String> {
        self.require_plugin(plugin)?;
        let (label, event) = self.shared.dev.recall(query).ok_or_else(|| {
            format!("no event matching {query:?} was seen while the console was attached")
        })?;
        let cfg = self.cfg();
        let mut host = ProxyHost::new(&self.shared, &cfg);
        let outcome = match event {
            Replay::RestRequest {
                method,
                route,
                mut body,
            } => self.chain.run_to(
                plugin,
                &mut host,
                &EventKind::RestRequest {
                    method: &method,
                    route: &route,
                },
                |p, h| {
                    p.on_event(
                        h,
                        Event::RestRequest {
                            method: &method,
                            route: &route,
                            body: body.as_mut(),
                        },
                    )
                },
            ),
            Replay::RestResponse {
                method,
                route,
                status,
                mut body,
            } => self.chain.run_to(
                plugin,
                &mut host,
                &EventKind::RestResponse {
                    method: &method,
                    route: &route,
                },
                |p, h| {
                    p.on_event(
                        h,
                        Event::RestResponse {
                            method: &method,
                            route: &route,
                            status,
                            body: &mut body,
                        },
                    )
                },
            ),
            Replay::Dispatch { name, mut payload } => {
                let mut extra = Vec::new();
                let outcome =
                    self.chain
                        .dispatch_to(&mut host, plugin, &name, &mut payload, &mut extra);
                host.locals.append(&mut extra);
                outcome
            }
            Replay::Send { op, mut payload } => {
                self.chain
                    .run_to(plugin, &mut host, &EventKind::Send { op }, |p, h| {
                        p.on_event(
                            h,
                            Event::Send {
                                op,
                                payload: &mut payload,
                            },
                        )
                    })
            }
        };
        let outcome = outcome.ok_or_else(|| {
            format!("{plugin} did not run: it is off, crashed or does not want {label}")
        })?;
        Ok(ReplayReport {
            event: label,
            plugin: plugin.to_string(),
            verdict: verdict_text(&outcome),
            dropped_sends: host.sends.len(),
            dropped_locals: host.locals.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn collector(bus: &DevBus) -> Arc<Mutex<Vec<DevEntry>>> {
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        bus.subscribe(Box::new(move |e| {
            g.lock().unwrap().push(e.clone());
            true
        }));
        got
    }

    #[test]
    fn nothing_is_built_or_kept_while_nobody_is_attached() {
        let bus = DevBus::default();
        bus.log("p", "hello");
        bus.remember("dispatch", "X".into(), || panic!("copied a payload"));
        assert!(!bus.active());
        assert!(bus.recall("X").is_none());
    }

    #[test]
    fn entries_reach_every_sink_and_a_closed_sink_is_dropped() {
        let bus = DevBus::default();
        let got = collector(&bus);
        bus.subscribe(Box::new(|_| false));
        assert!(bus.active());
        bus.log("p", "one");
        assert!(bus.active(), "the live sink stays");
        bus.run(
            &EventKind::Dispatch {
                name: "MESSAGE_CREATE",
            },
            "p",
            "pass",
            Duration::from_micros(7),
        );
        let got = got.lock().unwrap();
        assert_eq!(got.len(), 2);
        assert!(
            matches!(&got[0], DevEntry::Log { plugin, message, .. } if plugin == "p" && message == "one")
        );
        assert!(
            matches!(&got[1], DevEntry::Run { event, micros: 7, .. } if event == "MESSAGE_CREATE")
        );
    }

    #[test]
    fn the_last_event_of_a_kind_is_found_by_label_or_part_of_it() {
        let bus = DevBus::default();
        let _got = collector(&bus);
        for id in [1, 2] {
            let mut p = Val::map();
            p.set("id", Val::Int(id));
            bus.remember("dispatch", "MESSAGE_CREATE".into(), || Replay::Dispatch {
                name: "MESSAGE_CREATE".into(),
                payload: p,
            });
        }
        bus.remember("rest_request", "POST /channels/1/messages".into(), || {
            Replay::RestRequest {
                method: "POST".into(),
                route: "/channels/1/messages".into(),
                body: None,
            }
        });
        let Some((label, Replay::Dispatch { payload, .. })) = bus.recall("message_create") else {
            panic!("not found");
        };
        assert_eq!(label, "MESSAGE_CREATE");
        assert_eq!(
            payload.get("id").and_then(Val::as_i64),
            Some(2),
            "the newest"
        );
        assert!(matches!(
            bus.recall("channels/1"),
            Some((_, Replay::RestRequest { .. }))
        ));
        assert!(bus.recall("nothing").is_none());
    }

    #[test]
    fn only_the_newest_kinds_are_kept() {
        let bus = DevBus::default();
        let _got = collector(&bus);
        for i in 0..MAX_REMEMBERED + 5 {
            bus.remember("dispatch", format!("E{i}"), || Replay::Dispatch {
                name: format!("E{i}"),
                payload: Val::map(),
            });
        }
        assert_eq!(bus.remembered().len(), MAX_REMEMBERED);
        assert!(bus.recall("E0").is_none());
        assert!(bus.recall("E68").is_some());
    }
}
