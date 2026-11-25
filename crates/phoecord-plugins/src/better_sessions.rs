//! BetterSessions: exact timestamps and names for sessions, and new-session alerts (Vencord:
//! BetterSessions).
//!
//! Ported from Vencord `src/plugins/betterSessions/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; author amia). Vencord reads `GET /auth/sessions`, remembers
//! the session ids it has seen and shows a notification for one it has not. This is the same data
//! part: the call goes through `Host::rest`, the known session ids live in the plugin's kv store
//! (`known_sessions`), and an unknown session raises `Host::notify`.
//!
//! Switching it on: `[plugins.better_sessions] enabled = true`, or
//! `/phoecord toggle better_sessions`.
//!
//! When it checks. The Plugin API has no timer, so there is no real background loop:
//! - on `READY` (Vencord also checks once when the plugin starts), always;
//! - on `/phoecord sessions`, which also lists every session with its exact last-used time and
//!   marks the ones not seen before with `NEW` (those are not notified again);
//! - when the Vencord settings `backgroundCheck = true` and `checkInterval` (minutes, default 20)
//!   are set: on the next `MESSAGE_CREATE` after the interval has passed. That is an approximation:
//!   a quiet account is checked later than the interval.
//!
//! The first check with an empty kv store only records the sessions (Vencord alerts about every
//! session then; a burst of alerts on first start is not useful here).
//!
//! Not ported: the rename button and custom session names, the "NEW" badge and the platform
//! icons in the devices settings page (Phase 2 UI hooks).

use crate::commands::{FnCmd, Subcommand};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PLUGIN_ID: &str = "better_sessions";
const KV_KNOWN: &str = "known_sessions";
/// Vencord's default `checkInterval`, minutes.
const DEFAULT_INTERVAL_MIN: u64 = 20;

pub struct BetterSessions {
    manifest: Manifest,
    /// Unix milliseconds of the last check, 0 for never.
    last_check: Arc<AtomicU64>,
}

impl BetterSessions {
    pub fn new() -> BetterSessions {
        BetterSessions {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "BetterSessions".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Exact timestamps for sessions and alerts about new ones".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("BetterSessions".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            last_check: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The `/phoecord sessions` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let last = self.last_check.clone();
        FnCmd::new(
            PLUGIN_ID,
            "sessions",
            "List your logged-in sessions with exact times and flag new ones",
            move |host, _| {
                last.store(now_ms(), Ordering::Relaxed);
                match check(host) {
                    Ok(report) => render(host, &report),
                    Err(e) => host.t("sessions-error", &[("error", &e)]),
                }
            },
        )
        .boxed()
    }
}

impl Default for BetterSessions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Session {
    id: String,
    os: String,
    platform: String,
    location: String,
    /// `approx_last_used_time` as Discord sent it.
    last_used: String,
}

struct Report {
    sessions: Vec<Session>,
    /// Ids of the sessions not seen before. Empty on the very first check.
    new: HashSet<String>,
    first_run: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Is a background check due? `last` 0 means never checked.
fn due(last_ms: u64, now_ms: u64, interval_min: u64) -> bool {
    now_ms.saturating_sub(last_ms) >= interval_min.max(1) * 60_000
}

fn number(v: Option<Val>) -> Option<u64> {
    match v? {
        Val::Int(n) => u64::try_from(n).ok(),
        Val::Big(n) => u64::try_from(n).ok(),
        Val::Float(f) if f >= 0.0 => Some(f as u64),
        _ => None,
    }
}

fn parse_sessions(body: &Val) -> Vec<Session> {
    let text = |s: &Val, k: &str| {
        s.str_at(&["client_info", k])
            .unwrap_or("unknown")
            .to_string()
    };
    body.get("user_sessions")
        .map(Val::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|s| {
            Some(Session {
                id: s.str_at(&["id_hash"])?.to_string(),
                os: text(s, "os"),
                platform: text(s, "platform"),
                location: text(s, "location"),
                last_used: s
                    .str_at(&["approx_last_used_time"])
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

fn load_known(host: &dyn Host) -> Option<HashSet<String>> {
    match host.kv_get(KV_KNOWN)? {
        Val::List(l) => Some(
            l.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        ),
        _ => None,
    }
}

/// Fetch the sessions, compare them with the remembered ones and remember the current set.
fn check(host: &mut dyn Host) -> Result<Report, String> {
    let r = host.rest("GET", "/auth/sessions", None)?;
    if r.status != 200 {
        return Err(format!("Discord answered {}", r.status));
    }
    let body = r.body.ok_or("empty answer")?;
    let sessions = parse_sessions(&body);
    let known = load_known(host);
    let first_run = known.is_none();
    let new = match &known {
        Some(k) => sessions
            .iter()
            .filter(|s| !k.contains(&s.id))
            .map(|s| s.id.clone())
            .collect(),
        None => HashSet::new(),
    };
    // Like Vencord: an empty list never wipes the memory.
    if !sessions.is_empty() {
        host.kv_set(
            KV_KNOWN,
            Val::List(sessions.iter().map(|s| Val::Str(s.id.clone())).collect()),
        );
    }
    Ok(Report {
        sessions,
        new,
        first_run,
    })
}

/// `2026-11-24T10:00:00.123456+00:00` -> `2026-11-24 10:00:00 UTC`; anything else is kept.
fn exact_time(raw: &str) -> String {
    let Some(head) = raw.get(..19).filter(|h| h.as_bytes()[10] == b'T') else {
        return if raw.is_empty() {
            "unknown".into()
        } else {
            raw.into()
        };
    };
    let mut tail = &raw[19..];
    if let Some(t) = tail.strip_prefix('.') {
        tail = t.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let zone = match tail {
        "" | "Z" | "+00:00" | "-00:00" => "UTC",
        z => z,
    };
    format!("{} {zone}", head.replace('T', " "))
}

fn describe(s: &Session) -> String {
    format!("{} \u{b7} {} \u{b7} {}", s.os, s.platform, s.location)
}

fn render(host: &dyn Host, r: &Report) -> String {
    if r.sessions.is_empty() {
        return host.t("sessions-none", &[]);
    }
    let count = r.sessions.len().to_string();
    let mut out = format!("**{}**", host.t("sessions-header", &[("count", &count)]));
    if r.first_run {
        out.push_str(&format!(" ({})", host.t("sessions-first-run", &[])));
    }
    for s in &r.sessions {
        let tag = if r.new.contains(&s.id) {
            format!(" **{}**", host.t("sessions-new-tag", &[]))
        } else {
            String::new()
        };
        let line = host.t(
            "sessions-line",
            &[
                ("session", &describe(s)),
                ("time", &exact_time(&s.last_used)),
            ],
        );
        out.push_str(&format!("\n- {line}{tag}"));
    }
    out
}

fn notify_new(host: &mut dyn Host, r: &Report) {
    for s in r.sessions.iter().filter(|s| r.new.contains(&s.id)) {
        let body = format!("{}\n{}", host.t("new-session", &[]), describe(s));
        host.notify("BetterSessions", &body);
    }
}

impl Plugin for BetterSessions {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if matches!(*name, "READY" | "MESSAGE_CREATE"))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch { name, .. } = event else {
            return Verdict::Pass;
        };
        let run = match name {
            "READY" => true,
            "MESSAGE_CREATE" => {
                host.setting("backgroundCheck").and_then(|v| v.as_bool()) == Some(true)
                    && due(
                        self.last_check.load(Ordering::Relaxed),
                        now_ms(),
                        number(host.setting("checkInterval")).unwrap_or(DEFAULT_INTERVAL_MIN),
                    )
            }
            _ => false,
        };
        if run {
            self.last_check.store(now_ms(), Ordering::Relaxed);
            match check(host) {
                Ok(report) => notify_new(host, &report),
                Err(e) => host.log(&format!("session check failed: {e}")),
            }
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    const TWO: &str = r#"{"user_sessions":[
        {"id_hash":"a","approx_last_used_time":"2026-11-24T10:00:00.123456+00:00",
         "client_info":{"os":"Windows","platform":"Discord Client","location":"Almaty, Kazakhstan"}},
        {"id_hash":"b","approx_last_used_time":"2026-11-20T08:30:00+00:00",
         "client_info":{"os":"Android","platform":"Discord Android","location":"Astana"}}]}"#;

    fn dispatch(p: &BetterSessions, h: &mut Rec, name: &str) -> Verdict {
        let mut payload = j(r#"{"d":{}}"#);
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name,
                payload: &mut payload,
                extra: &mut extra,
            },
        )
    }

    #[test]
    fn first_ready_records_then_new_session_notifies() {
        let p = BetterSessions::new();
        let mut h = Rec::default().answer("GET", "/auth/sessions", 200, TWO);
        assert_eq!(dispatch(&p, &mut h, "READY"), Verdict::Pass);
        assert!(h.notes.is_empty(), "first run only records");
        assert!(h.kv.contains_key(KV_KNOWN));

        h.rest_answers.clear();
        let three = TWO.replace(
            r#"{"id_hash":"b""#,
            r#"{"id_hash":"c","approx_last_used_time":"x","client_info":{"os":"Linux","platform":"Firefox","location":"Oral"}},{"id_hash":"b""#,
        );
        let mut h2 = Rec::default().answer("GET", "/auth/sessions", 200, &three);
        h2.kv = h.kv.clone();
        dispatch(&p, &mut h2, "READY");
        assert_eq!(h2.notes.len(), 1);
        assert!(h2.notes[0].1.contains("Linux \u{b7} Firefox \u{b7} Oral"));
        // Seen now: no second alert.
        let mut h3 = Rec::default().answer("GET", "/auth/sessions", 200, &three);
        h3.kv = h2.kv.clone();
        dispatch(&p, &mut h3, "READY");
        assert!(h3.notes.is_empty());
    }

    #[test]
    fn command_lists_exact_times_and_marks_new() {
        let p = BetterSessions::new();
        let cmd = p.command();
        let mut h = Rec::default().answer("GET", "/auth/sessions", 200, TWO);
        h.kv.insert(KV_KNOWN.into(), Val::List(vec![Val::Str("b".into())]));
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(out.contains("Windows \u{b7} Discord Client \u{b7} Almaty, Kazakhstan, last used 2026-11-24 10:00:00 UTC **NEW**"), "{out}");
        assert!(out.contains("2026-11-20 08:30:00 UTC"));
        assert!(!out.contains("Astana, last used 2026-11-20 08:30:00 UTC **NEW**"));
    }

    #[test]
    fn disabled_plugin_command_says_off() {
        let p = BetterSessions::new();
        let cmd = p.command();
        let mut h = Rec::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(out.contains("is off"));
        assert!(h.rest_calls.is_empty());
    }

    #[test]
    fn bad_answers_do_not_panic() {
        let p = BetterSessions::new();
        for (status, body) in [
            (200, "{}"),
            (200, r#"{"user_sessions":[{"x":1},5,null]}"#),
            (500, "{}"),
        ] {
            let mut h = Rec::default().answer("GET", "/auth/sessions", status, body);
            assert_eq!(dispatch(&p, &mut h, "READY"), Verdict::Pass);
            assert!(h.notes.is_empty());
        }
        // No rest answer at all (404).
        let mut h = Rec::default();
        assert_eq!(dispatch(&p, &mut h, "READY"), Verdict::Pass);
        assert!(!h.logs.is_empty());
    }

    #[test]
    fn background_check_follows_settings_and_interval() {
        let p = BetterSessions::new();
        // Setting off: MESSAGE_CREATE never checks.
        let mut h = Rec::default().answer("GET", "/auth/sessions", 200, TWO);
        dispatch(&p, &mut h, "MESSAGE_CREATE");
        assert!(h.rest_calls.is_empty());
        // On and never checked: due.
        let mut h = Rec::default()
            .with("backgroundCheck", Val::Bool(true))
            .answer("GET", "/auth/sessions", 200, TWO);
        dispatch(&p, &mut h, "MESSAGE_CREATE");
        assert_eq!(h.rest_calls.len(), 1);
        // Just checked: not due again for 20 minutes.
        dispatch(&p, &mut h, "MESSAGE_CREATE");
        assert_eq!(h.rest_calls.len(), 1);
        assert!(due(0, 20 * 60_000, 20));
        assert!(!due(1, 20 * 60_000, 20));
        assert!(due(0, 60_000, 0), "interval has a floor of one minute");
    }

    #[test]
    fn exact_time_formats() {
        assert_eq!(
            exact_time("2026-11-24T10:00:00Z"),
            "2026-11-24 10:00:00 UTC"
        );
        assert_eq!(
            exact_time("2026-11-24T10:00:00.5+05:00"),
            "2026-11-24 10:00:00 +05:00"
        );
        assert_eq!(exact_time(""), "unknown");
        assert_eq!(exact_time("garbage"), "garbage");
    }
}
