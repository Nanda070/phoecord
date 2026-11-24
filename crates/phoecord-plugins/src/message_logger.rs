//! MessageLogger: keep deleted messages on screen and show what edited messages used to say.
//!
//! Adapted from the v0 implementation and from Vencord's MessageLogger
//! (Copyright (c) Vendicated and contributors, GPL-3.0-or-later).
//!
//! The plugin remembers recent messages. When Discord says one was deleted, the client gets an edit
//! instead, with a "deleted" line on top; edits get their earlier versions underneath. Like Vencord,
//! this only lasts until the client reloads the channel. With `write_log` every deletion and edit is
//! also appended to `data/plugins/message_logger/messages.jsonl` (read it with `phoecord log`).
//!
//! Settings (`[plugins.message_logger]`): `cache_size` (5000), `ignore_self` (true), `ignore_bots`
//! (false), `show_edits` (true), `write_log` (true).

use phoecord_core::plugin::API_VERSION;
use phoecord_core::route::message_route_channel;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

/// File name inside the plugin's data folder.
pub const LOG_FILE: &str = "messages.jsonl";
const DELETED: &str = "-# 🗑 deleted";
const MAX_HISTORY: usize = 5;
const DEFAULT_CACHE: usize = 5000;
const MIN_CACHE: usize = 100;

struct Entry {
    msg: Val,
    history: Vec<String>,
    tick: u64,
}

/// A least-recently-used map of messages by id.
#[derive(Default)]
struct Lru {
    cap: usize,
    tick: u64,
    items: HashMap<u64, Entry>,
    order: BTreeMap<u64, u64>,
}

impl Lru {
    fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.evict();
    }

    fn evict(&mut self) {
        while self.items.len() > self.cap.max(1) {
            let Some((_, id)) = self.order.pop_first() else {
                break;
            };
            self.items.remove(&id);
        }
    }

    fn touch(&mut self, id: u64) {
        self.tick += 1;
        if let Some(e) = self.items.get_mut(&id) {
            self.order.remove(&e.tick);
            e.tick = self.tick;
            self.order.insert(self.tick, id);
        }
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Entry> {
        self.touch(id);
        self.items.get_mut(&id)
    }

    fn put(&mut self, id: u64, msg: Val) {
        if let Some(e) = self.get_mut(id) {
            e.msg = msg;
            return;
        }
        self.tick += 1;
        self.order.insert(self.tick, id);
        self.items.insert(
            id,
            Entry {
                msg,
                history: Vec::new(),
                tick: self.tick,
            },
        );
        self.evict();
    }

    fn pop(&mut self, id: u64) -> Option<Entry> {
        let e = self.items.remove(&id)?;
        self.order.remove(&e.tick);
        Some(e)
    }
}

pub struct MessageLogger {
    manifest: Manifest,
    cache: Mutex<Lru>,
}

impl MessageLogger {
    pub fn new() -> MessageLogger {
        MessageLogger {
            manifest: Manifest {
                id: "message_logger".into(),
                name: "MessageLogger".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Keep deleted messages visible and show edit history".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("MessageLogger".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            cache: Mutex::new(Lru::default()),
        }
    }
}

impl Default for MessageLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Gateway events the plugin reacts to.
fn wants_dispatch(name: &str) -> bool {
    matches!(
        name,
        "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "MESSAGE_DELETE_BULK"
    )
}

/// `GET /channels/{id}/messages`: history the client loaded, which we learn from.
fn wants_history(method: &str, route: &str) -> bool {
    method == "GET"
        && route.trim_end_matches('/').ends_with("/messages")
        && message_route_channel(route).is_some()
}

struct Settings {
    cache_size: usize,
    ignore_self: bool,
    ignore_bots: bool,
    show_edits: bool,
    write_log: bool,
}

impl Settings {
    fn read(host: &dyn Host) -> Settings {
        let flag = |k: &str, d: bool| host.setting(k).and_then(|v| v.as_bool()).unwrap_or(d);
        Settings {
            cache_size: host
                .setting("cache_size")
                .and_then(|v| v.as_i64())
                .map_or(DEFAULT_CACHE, |n| (n.max(0) as usize).max(MIN_CACHE)),
            ignore_self: flag("ignore_self", true),
            ignore_bots: flag("ignore_bots", false),
            show_edits: flag("show_edits", true),
            write_log: flag("write_log", true),
        }
    }
}

/// A message's text: JSON strings and ETF binaries both.
fn content_of(msg: &Val) -> Option<String> {
    match msg.get("content")? {
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

/// `text` in the same shape as `like` (a binary stays a binary).
fn like(like: &Val, text: &str) -> Val {
    match like {
        Val::Bin(_) => Val::Bin(text.as_bytes().to_vec()),
        v => v.str_like(text),
    }
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn annotate(content: &str, history: &[String]) -> String {
    let mut out = content.to_string();
    for old in history.iter().rev() {
        out.push_str(&format!("\n-# ✏ was: {}", one_line(old, 300)));
    }
    out
}

/// `2026-11-23T12:00:00Z` from unix seconds (civil-from-days, no time crate needed).
fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Replace the event name and data of a gateway frame, keeping everything else (`s`, `op`).
fn rewrite(payload: &mut Val, name: &str, data: Val) {
    let t = payload
        .get("t")
        .map(|v| v.str_like(name))
        .unwrap_or_else(|| Val::Str(name.into()));
    payload.set("t", t);
    payload.set("d", data);
}

fn frame_like(template: &Val, name: &str, data: Val) -> Val {
    let mut p = template.clone();
    rewrite(&mut p, name, data);
    p
}

impl MessageLogger {
    fn skip(&self, host: &dyn Host, s: &Settings, msg: &Val) -> bool {
        let mine = host.self_id().is_some() && msg.id_at(&["author", "id"]) == host.self_id();
        (s.ignore_self && mine)
            || (s.ignore_bots && msg.at(&["author", "bot"]).and_then(Val::as_bool) == Some(true))
    }

    fn with_cache<R>(&self, s: &Settings, f: impl FnOnce(&mut Lru) -> R) -> R {
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.cap != s.cache_size {
            c.set_cap(s.cache_size);
        }
        f(&mut c)
    }

    fn remember(&self, s: &Settings, msg: &Val) {
        if let Some(id) = msg.id_at(&["id"]) {
            self.with_cache(s, |c| c.put(id, msg.clone()));
        }
    }

    fn write(&self, host: &mut dyn Host, s: &Settings, kind: &str, msg: &Val, was: Option<&str>) {
        if !s.write_log {
            return;
        }
        let line = serde_json_line(kind, msg, was);
        if let Err(e) = host.append_data(LOG_FILE, &line) {
            host.log(&format!("could not write {LOG_FILE}: {e}"));
        }
    }

    /// The message to show instead of its deletion, or `None` to let the deletion through.
    fn deleted(&self, host: &mut dyn Host, s: &Settings, id: u64) -> Option<Val> {
        let e = self.with_cache(s, |c| c.pop(id))?;
        let (msg, history) = (e.msg, e.history);
        if self.skip(host, s, &msg) {
            return None;
        }
        self.write(host, s, "deleted", &msg, None);
        let text = content_of(&msg).unwrap_or_default();
        let body = if text.is_empty() {
            DELETED.to_string()
        } else {
            format!("{DELETED}\n{text}")
        };
        let body = annotate(&body, &history);
        let mut shown = msg.clone();
        let content = msg
            .get("content")
            .map(|c| like(c, &body))
            .unwrap_or(Val::Str(body));
        shown.set("content", content);
        Some(shown)
    }

    fn on_update(&self, host: &mut dyn Host, s: &Settings, payload: &mut Val) -> Verdict {
        let Some(m) = payload.get("d").cloned() else {
            return Verdict::Pass;
        };
        let (Some(id), Some(new)) = (m.id_at(&["id"]), content_of(&m)) else {
            return Verdict::Pass;
        };
        let skip = self.skip(host, s, &m);
        let edit = self.with_cache(s, |c| {
            let e = c.get_mut(id)?;
            let old = content_of(&e.msg).unwrap_or_default();
            e.msg = m.clone();
            if old == new || skip {
                return None;
            }
            e.history.push(old.clone());
            if e.history.len() > MAX_HISTORY {
                e.history.remove(0);
            }
            Some((old, e.history.clone()))
        });
        let Some((old, history)) = edit else {
            return Verdict::Pass;
        };
        self.write(host, s, "edited", &m, Some(&old));
        if !s.show_edits {
            return Verdict::Pass;
        }
        let shown = annotate(&new, &history);
        let Some(data) = payload.get_mut("d") else {
            return Verdict::Pass;
        };
        let v = data
            .get("content")
            .map(|c| like(c, &shown))
            .unwrap_or(Val::Str(shown));
        data.set("content", v);
        Verdict::Changed
    }

    fn on_bulk(
        &self,
        host: &mut dyn Host,
        s: &Settings,
        payload: &mut Val,
        extra: &mut Vec<Val>,
    ) -> Verdict {
        let ids: Vec<Val> = payload
            .at(&["d", "ids"])
            .map(|l| l.as_list().to_vec())
            .unwrap_or_default();
        let template = payload.clone();
        let mut shown = Vec::new();
        let mut gone = Vec::new();
        for v in ids {
            match v.snowflake().and_then(|id| self.deleted(host, s, id)) {
                Some(m) => shown.push(m),
                None => gone.push(v),
            }
        }
        let mut shown = shown.into_iter();
        let Some(first) = shown.next() else {
            return Verdict::Pass;
        };
        rewrite(payload, "MESSAGE_UPDATE", first);
        extra.extend(shown.map(|m| frame_like(&template, "MESSAGE_UPDATE", m)));
        // Messages we did not have (or that you deleted yourself) are still deleted.
        if !gone.is_empty() {
            let mut data = template.get("d").cloned().unwrap_or_else(Val::map);
            data.set("ids", Val::List(gone));
            extra.push(frame_like(&template, "MESSAGE_DELETE_BULK", data));
        }
        Verdict::Changed
    }
}

fn serde_json_line(kind: &str, msg: &Val, was: Option<&str>) -> String {
    use phoecord_core::Val as V;
    let attachments: Vec<V> = msg
        .get("attachments")
        .map(V::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a.str_at(&["url"]))
        .map(|u| V::Str(u.into()))
        .collect();
    let s = |v: Option<&str>| v.map_or(V::Null, |x| V::Str(x.into()));
    let id = |v: Option<u64>| v.map_or(V::Null, |x| V::Str(x.to_string()));
    let line = V::Map(vec![
        (V::Str("time".into()), V::Str(rfc3339(now_secs()))),
        (V::Str("kind".into()), V::Str(kind.into())),
        (V::Str("id".into()), id(msg.id_at(&["id"]))),
        (V::Str("channel".into()), id(msg.id_at(&["channel_id"]))),
        (V::Str("guild".into()), id(msg.id_at(&["guild_id"]))),
        (
            V::Str("author".into()),
            s(msg
                .str_at(&["author", "global_name"])
                .or_else(|| msg.str_at(&["author", "username"]))),
        ),
        (V::Str("content".into()), s(content_of(msg).as_deref())),
        (V::Str("was".into()), s(was)),
        (V::Str("attachments".into()), V::List(attachments)),
    ]);
    line.to_json().to_string()
}

impl Plugin for MessageLogger {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => wants_dispatch(name),
            EventKind::RestResponse { method, route } => wants_history(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestResponse {
                method,
                route,
                body,
                ..
            } if wants_history(method, route) => {
                let s = Settings::read(host);
                for m in body.as_list() {
                    self.remember(&s, m);
                }
                Verdict::Pass
            }
            Event::Dispatch {
                name,
                payload,
                extra,
            } if wants_dispatch(name) => {
                let s = Settings::read(host);
                match name {
                    "MESSAGE_CREATE" => {
                        if let Some(m) = payload.get("d") {
                            self.remember(&s, m);
                        }
                        Verdict::Pass
                    }
                    "MESSAGE_UPDATE" => self.on_update(host, &s, payload),
                    "MESSAGE_DELETE" => {
                        let Some(id) = payload.id_at(&["d", "id"]) else {
                            return Verdict::Pass;
                        };
                        match self.deleted(host, &s, id) {
                            Some(shown) => {
                                rewrite(payload, "MESSAGE_UPDATE", shown);
                                Verdict::Changed
                            }
                            None => Verdict::Pass,
                        }
                    }
                    _ => self.on_bulk(host, &s, payload, extra),
                }
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, Outcome};

    /// A host with settings, an own user id and a captured log file.
    #[derive(Default)]
    struct TestHost {
        settings: HashMap<String, Val>,
        me: Option<u64>,
        lines: Vec<String>,
    }

    impl Host for TestHost {
        fn setting(&self, key: &str) -> Option<Val> {
            self.settings.get(key).cloned()
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            self.me
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
        fn append_data(&mut self, file: &str, line: &str) -> Result<(), String> {
            assert_eq!(file, LOG_FILE);
            self.lines.push(line.to_string());
            Ok(())
        }
    }

    fn host() -> TestHost {
        let mut h = TestHost {
            me: Some(1),
            ..TestHost::default()
        };
        h.settings.insert("write_log".into(), Val::Bool(false));
        h
    }

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    /// Run one gateway frame through the plugin; returns the (possibly rewritten) frame and extras.
    fn dispatch(p: &MessageLogger, h: &mut TestHost, json: &str) -> (Verdict, Val, Vec<Val>) {
        let mut payload = j(json);
        let name = payload.str_at(&["t"]).unwrap().to_string();
        let mut extra = Vec::new();
        let v = p.on_event(
            h,
            Event::Dispatch {
                name: &name,
                payload: &mut payload,
                extra: &mut extra,
            },
        );
        (v, payload, extra)
    }

    #[test]
    fn matching_rules() {
        assert!(wants_dispatch("MESSAGE_DELETE"));
        assert!(wants_dispatch("MESSAGE_DELETE_BULK"));
        assert!(!wants_dispatch("TYPING_START"));
        assert!(wants_history("GET", "/channels/9/messages"));
        assert!(!wants_history("POST", "/channels/9/messages"));
        assert!(!wants_history("GET", "/channels/9/pins"));
        let p = MessageLogger::new();
        assert!(p.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
        assert!(!p.wants(&EventKind::Send { op: 3 }));
    }

    #[test]
    fn delete_becomes_marked_edit() {
        let p = MessageLogger::new();
        let mut h = host();
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"s":5,"t":"MESSAGE_CREATE","d":{"id":"50","channel_id":"9","content":"hej","author":{"id":"2"}}}"#,
        );
        let (v, d, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"s":6,"t":"MESSAGE_DELETE","d":{"id":"50","channel_id":"9"}}"#,
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(d.str_at(&["t"]), Some("MESSAGE_UPDATE"));
        assert_eq!(d.str_at(&["d", "content"]), Some("-# 🗑 deleted\nhej"));
        assert_eq!(d.get("s").and_then(Val::as_i64), Some(6));
        // Unknown messages are left alone.
        let (v, d, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"51","channel_id":"9"}}"#,
        );
        assert_eq!(v, Verdict::Pass);
        assert_eq!(d.str_at(&["t"]), Some("MESSAGE_DELETE"));
    }

    #[test]
    fn works_with_snowflake_ints_and_atoms() {
        let p = MessageLogger::new();
        let mut h = host();
        let key = |s: &str| Val::Atom(s.into());
        let msg = Val::Map(vec![
            (key("id"), Val::Int(50)),
            (key("channel_id"), Val::Int(9)),
            (key("content"), Val::Bin(b"hej".to_vec())),
            (key("author"), Val::Map(vec![(key("id"), Val::Int(2))])),
        ]);
        let frame = |t: &str, d: Val| {
            Val::Map(vec![
                (key("op"), Val::Int(0)),
                (key("t"), Val::Atom(t.into())),
                (key("d"), d),
            ])
        };
        let mut create = frame("MESSAGE_CREATE", msg);
        let mut extra = vec![];
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "MESSAGE_CREATE",
                payload: &mut create,
                extra: &mut extra,
            },
        );
        let mut del = frame(
            "MESSAGE_DELETE",
            Val::Map(vec![
                (key("id"), Val::Int(50)),
                (key("channel_id"), Val::Int(9)),
            ]),
        );
        let v = p.on_event(
            &mut h,
            Event::Dispatch {
                name: "MESSAGE_DELETE",
                payload: &mut del,
                extra: &mut extra,
            },
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(del.get("t"), Some(&Val::Atom("MESSAGE_UPDATE".into())));
        // The content stays a binary, the way the client sent it.
        assert_eq!(
            del.at(&["d", "content"]),
            Some(&Val::Bin(
                "-# 🗑 deleted
hej"
                .as_bytes()
                .to_vec()
            ))
        );
    }

    #[test]
    fn edits_show_history_and_own_messages_are_ignored() {
        let p = MessageLogger::new();
        let mut h = host();
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"60","channel_id":"9","content":"v1","author":{"id":"2"}}}"#,
        );
        let (v, d, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_UPDATE","d":{"id":"60","channel_id":"9","content":"v2","author":{"id":"2"}}}"#,
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(d.str_at(&["d", "content"]), Some("v2\n-# ✏ was: v1"));
        // Embed-only updates (no content) do not count as edits.
        let (v, _, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_UPDATE","d":{"id":"60","channel_id":"9","embeds":[]}}"#,
        );
        assert_eq!(v, Verdict::Pass);

        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"61","channel_id":"9","content":"mine","author":{"id":"1"}}}"#,
        );
        let (v, _, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"61","channel_id":"9"}}"#,
        );
        assert_eq!(
            v,
            Verdict::Pass,
            "own deletions stay deleted with ignore_self"
        );

        h.settings.insert("ignore_self".into(), Val::Bool(false));
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"62","channel_id":"9","content":"mine","author":{"id":"1"}}}"#,
        );
        let (v, _, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"62","channel_id":"9"}}"#,
        );
        assert_eq!(v, Verdict::Changed);
    }

    #[test]
    fn show_edits_off_keeps_the_edit_plain_but_remembers_it() {
        let p = MessageLogger::new();
        let mut h = host();
        h.settings.insert("show_edits".into(), Val::Bool(false));
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"63","channel_id":"9","content":"v1","author":{"id":"2"}}}"#,
        );
        let (v, d, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_UPDATE","d":{"id":"63","channel_id":"9","content":"v2","author":{"id":"2"}}}"#,
        );
        assert_eq!(v, Verdict::Pass);
        assert_eq!(d.str_at(&["d", "content"]), Some("v2"));
        // The deletion still shows the history.
        let (_, d, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"63","channel_id":"9"}}"#,
        );
        assert_eq!(
            d.str_at(&["d", "content"]),
            Some("-# 🗑 deleted\nv2\n-# ✏ was: v1")
        );
    }

    #[test]
    fn bulk_delete_keeps_every_known_message() {
        let p = MessageLogger::new();
        let mut h = host();
        for id in ["70", "71"] {
            dispatch(
                &p,
                &mut h,
                &format!(
                    r#"{{"op":0,"t":"MESSAGE_CREATE","d":{{"id":"{id}","channel_id":"9","content":"m{id}","author":{{"id":"2"}}}}}}"#
                ),
            );
        }
        let (v, d, extra) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE_BULK","d":{"ids":["70","71","72"],"channel_id":"9"}}"#,
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(d.str_at(&["t"]), Some("MESSAGE_UPDATE"));
        assert_eq!(d.str_at(&["d", "content"]), Some("-# 🗑 deleted\nm70"));
        assert_eq!(extra.len(), 2);
        assert_eq!(
            extra[0].str_at(&["d", "content"]),
            Some("-# 🗑 deleted\nm71")
        );
        assert_eq!(extra[1].str_at(&["t"]), Some("MESSAGE_DELETE_BULK"));
        assert_eq!(extra[1].at(&["d", "ids"]).unwrap().as_list().len(), 1);
    }

    #[test]
    fn learns_from_fetched_history() {
        let p = MessageLogger::new();
        let mut h = host();
        let body = j(r#"[{"id":"80","channel_id":"9","content":"old","author":{"id":"2"}}]"#);
        let mut body_mut = body;
        p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/channels/9/messages",
                status: 200,
                body: &mut body_mut,
            },
        );
        let (v, _, _) = dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"80","channel_id":"9"}}"#,
        );
        assert_eq!(v, Verdict::Changed);
    }

    #[test]
    fn cache_evicts_the_least_recently_used() {
        let p = MessageLogger::new();
        let mut h = host();
        h.settings.insert("cache_size".into(), Val::Int(100));
        for id in 1000..1101 {
            dispatch(
                &p,
                &mut h,
                &format!(
                    r#"{{"op":0,"t":"MESSAGE_CREATE","d":{{"id":"{id}","channel_id":"9","content":"x","author":{{"id":"2"}}}}}}"#
                ),
            );
        }
        let del = |id: u32| {
            format!(r#"{{"op":0,"t":"MESSAGE_DELETE","d":{{"id":"{id}","channel_id":"9"}}}}"#)
        };
        let (v, _, _) = dispatch(&p, &mut h, &del(1000));
        assert_eq!(v, Verdict::Pass, "the oldest message was evicted");
        let (v, _, _) = dispatch(&p, &mut h, &del(1100));
        assert_eq!(v, Verdict::Changed);
    }

    #[test]
    fn write_log_appends_json_lines() {
        let p = MessageLogger::new();
        let mut h = host();
        h.settings.insert("write_log".into(), Val::Bool(true));
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"90","channel_id":"9","guild_id":"7","content":"a","author":{"id":"2","username":"bob"}}}"#,
        );
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_UPDATE","d":{"id":"90","channel_id":"9","guild_id":"7","content":"b","author":{"id":"2","username":"bob"}}}"#,
        );
        dispatch(
            &p,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"90","channel_id":"9"}}"#,
        );
        assert_eq!(h.lines.len(), 2);
        let edit: serde_json::Value = serde_json::from_str(&h.lines[0]).unwrap();
        assert_eq!(edit["kind"], "edited");
        assert_eq!(edit["was"], "a");
        assert_eq!(edit["content"], "b");
        assert_eq!(edit["author"], "bob");
        assert_eq!(edit["guild"], "7");
        let del: serde_json::Value = serde_json::from_str(&h.lines[1]).unwrap();
        assert_eq!(del["kind"], "deleted");
        assert_eq!(del["channel"], "9");
    }

    #[test]
    fn runs_in_a_chain_with_null_host() {
        let chain = Chain::new(vec![Box::new(MessageLogger::new())]);
        let mut h = phoecord_plugin_runtime::NullHost::default();
        let mut create = j(
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"5","channel_id":"9","content":"hi","author":{"id":"2"}}}"#,
        );
        let mut extra = vec![];
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut create, &mut extra);
        assert_eq!(out, Outcome::Untouched);
        let mut del = j(r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"5","channel_id":"9"}}"#);
        let out = chain.dispatch(&mut h, "MESSAGE_DELETE", &mut del, &mut extra);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(del.str_at(&["d", "content"]), Some("-# 🗑 deleted\nhi"));
    }

    #[test]
    fn timestamps_are_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
