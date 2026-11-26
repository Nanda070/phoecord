//! SilentDelete: "silently" delete a message by replacing it with a placeholder first, so message
//! loggers keep the placeholder instead of the text (Vencord userplugin: SilentDelete).
//!
//! Ported from the owner's userplugin `SilentDelete`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/SilentDelete, GPL-3.0-or-later,
//! Copyright (c) 2026 Vendicated and contributors), modified for PhoeCord 2026-11-25.
//!
//! What the script does, step by step (`silentDeleteMessage`): it sends a NEW message
//! `POST /channels/{id}/messages` with `content` = `replacementText`, `flags` 4096 (suppress
//! notifications) or 0, `mobile_network_type: "unknown"`, `nonce` = the id of the message being
//! deleted and `tts: false`; the nonce makes the client show the placeholder in the original's
//! place. It waits `deleteDelay` ms, deletes the placeholder (`DELETE /channels/{id}/messages/{new
//! id}`), and when `deleteOriginal` (the parameter and the setting) allows it waits 100 ms and
//! deletes the original (`DELETE /channels/{id}/messages/{id}`). Any error ends it as a failure
//! (the script logs it and returns false). Every wait is a timer step (`host.schedule`).
//!
//! Switching it on: `[plugins.silent_delete] enabled = true` (off by default). Commands, both with
//! `channel` defaulting to the channel the command is run in:
//! - `/phoecord silentdelete <message> [channel] [history]`: the message action. `history: true`
//!   is the context menu's "Silent Delete History" (the original is NOT deleted, only the
//!   placeholder round trip is done); without it, the popover button's "Silent Delete".
//! - `/phoecord silentpurge <count> [channel]`: the script's `/silentpurge`. It pages through
//!   `GET /channels/{id}/messages?limit=100[&before=..]` (100 ms between pages) until it has
//!   `count` of your own messages, then silently deletes them one by one with `purgeInterval` ms
//!   (`|| 500`) between them, and shows "Successfully silently deleted N message(s)." as a local
//!   message. The script's "(1-100)" is only its option description; there is no cap.
//!
//! Settings (`[plugins.silent_delete]`, the script's names and defaults): `replacementText`
//! (`"** **"`), `deleteDelay` (`200`), `suppressNotifications` (`true`), `deleteOriginal` (`true`),
//! `silentDeleteOnEmptyEdit` (`false`), `purgeInterval` (`500`), `accentColor` (`"#ed4245"`).
//!
//! How commands reach the engine: a `/phoecord` subcommand runs inside the command plugin, whose
//! timers and REST calls belong to it. So this plugin reads the `POST /interactions` of its own
//! subcommands (it sits before the command plugin in the chain), does the work as itself, and
//! the subcommand only hands back the text.
//!
//! The host's REST limiter allows 5 calls per 5 s per plugin and a silent delete is three calls,
//! so a purge is slower here than in the browser. A local refusal ("rate limited: ... retry in
//! X.Xs") is waited out and the same step is tried again; the script has no such wait (it has no
//! limiter), and it still treats a Discord 429 as a plain failure.
//!
//! Not ported: the message popover button, the context menu item and the icon, `accentColor`
//! (only colours that icon, kept so the config maps 1:1), and `silentDeleteOnEmptyEdit`, which
//! hooks the client's own edit box before it sends anything: the client never sends an empty edit
//! to Discord, so the network layer has nothing to catch (kept in the config, no effect).

use crate::commands::{APP_ID, Args, FnCmd, Subcommand};
use crate::state::Shared;
use phoecord_core::{
    Event, EventKind, Host, Manifest, MenuEntry, Plugin, TIMER_EVENT, UiEvent, Val, Verdict,
};

/// Message context menu entries (Phoenix, task 2.4.2 d): the script's "Silent Delete" and
/// "Silent Delete History" (`/phoecord silentdelete history:true`).
const MENU_DELETE: &str = "silent_delete";
const MENU_HISTORY: &str = "silent_delete_history";
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const PLUGIN_ID: &str = "silent_delete";
const SUPPRESS_NOTIFICATIONS: i64 = 1 << 12;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Post,
    DelReply,
    DelOrig,
}

struct Job {
    channel: String,
    message: String,
    /// The `deleteOriginal` parameter (false for "Silent Delete History").
    delete_original: bool,
    /// Settings as read when the job started (the script destructures them once per call).
    delete_delay: f64,
    delete_setting: bool,
    reply_id: Option<String>,
    stage: Stage,
    purge: Option<u64>,
}

enum Phase {
    Collect,
    /// Waiting to start message `i`.
    Delete,
}

struct Purge {
    channel: String,
    user: String,
    count: f64,
    collected: Vec<String>,
    before: Option<String>,
    phase: Phase,
    i: usize,
    success: u64,
}

#[derive(Default)]
struct State {
    next_id: u64,
    jobs: HashMap<u64, Job>,
    purges: HashMap<u64, Purge>,
    counter: u64,
    /// What the command should show for the interaction just handled.
    reply: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn text_of(v: &Val) -> Option<String> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

/// `(subcommand, options, channel the command was run in)` of a `/phoecord <sub>` interaction.
type Parsed = (String, Vec<(String, String)>, Option<String>);

fn parse_interaction(body: &Val) -> Option<Parsed> {
    let data = body.get("data")?;
    if data.get("name").and_then(text_of).as_deref() != Some("phoecord") {
        return None;
    }
    let sub = data
        .get("options")?
        .as_list()
        .iter()
        .find(|o| o.get("type").and_then(Val::as_i64) == Some(1))?;
    let name = sub.get("name").and_then(text_of)?;
    let options = sub
        .get("options")
        .map(Val::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|o| {
            let v = match o.get("value")? {
                Val::Int(n) => n.to_string(),
                v => text_of(v)?,
            };
            Some((text_of(o.get("name")?)?, v))
        })
        .collect();
    let channel = body.get("channel_id").and_then(|c| match c {
        Val::Int(n) => Some(n.to_string()),
        c => text_of(c),
    });
    Some((name, options, channel))
}

fn is_interaction(method: &str, route: &str) -> bool {
    method == "POST" && route.trim_end_matches('/') == "/interactions"
}

fn opt<'a>(options: &'a [(String, String)], name: &str) -> Option<&'a str> {
    options
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// A snowflake as digits only (these go into URLs).
fn snowflake(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("<#").trim_end_matches('>');
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.to_string())
}

fn num(v: &Val) -> Option<f64> {
    match v {
        Val::Float(f) => Some(*f),
        v => v.as_i64().map(|n| n as f64),
    }
}

fn setting_num(host: &dyn Host, key: &str, default: f64) -> f64 {
    host.setting(key).as_ref().and_then(num).unwrap_or(default)
}

fn setting_bool(host: &dyn Host, key: &str, default: bool) -> bool {
    host.setting(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// X of "rate limited: ... retry in X.Xs".
fn retry_in(err: &str) -> Option<f64> {
    if !err.contains("rate limited") {
        return None;
    }
    err.split("retry in ")
        .nth(1)?
        .trim_end_matches('s')
        .split('s')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn ms(n: f64) -> u64 {
    if n.is_finite() && n > 0.0 {
        n.ceil() as u64
    } else {
        0
    }
}

fn obj(fields: Vec<(&str, Val)>) -> Val {
    Val::Map(
        fields
            .into_iter()
            .map(|(k, v)| (Val::Str(k.into()), v))
            .collect(),
    )
}

fn schedule(host: &mut dyn Host, after_ms: u64, tag: &str) -> bool {
    match host.schedule(after_ms, tag) {
        Ok(()) => true,
        Err(e) => {
            host.log(&format!("SilentDelete: could not schedule a step: {e}"));
            false
        }
    }
}

enum Rest {
    Done(u16, Option<Val>),
    /// The host's limiter said to come back in this many ms.
    Wait(u64),
    Failed(String),
}

fn rest(host: &mut dyn Host, method: &str, route: &str, body: Option<&Val>) -> Rest {
    match host.rest(method, route, body) {
        Ok(r) => Rest::Done(r.status, r.body),
        Err(e) => match retry_in(&e) {
            Some(x) => Rest::Wait(ms(x * 1000.0)),
            None => Rest::Failed(e),
        },
    }
}

fn ok(status: u16) -> bool {
    (200..300).contains(&status)
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
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
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000000+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The script's `sendBotMessage`: a message only this client sees.
fn bot_message(st: &mut State, channel: &str, text: &str) -> Val {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    st.counter += 1;
    let id = (ms.saturating_sub(DISCORD_EPOCH_MS) << 22) | (st.counter & 0xfff);
    let msg = obj(vec![
        ("id", Val::Str(id.to_string())),
        ("channel_id", Val::Str(channel.into())),
        ("type", Val::Int(0)),
        ("content", Val::Str(text.into())),
        ("flags", Val::Int(64)),
        ("tts", Val::Bool(false)),
        ("pinned", Val::Bool(false)),
        ("mention_everyone", Val::Bool(false)),
        ("mentions", Val::List(Vec::new())),
        ("mention_roles", Val::List(Vec::new())),
        ("attachments", Val::List(Vec::new())),
        ("embeds", Val::List(Vec::new())),
        ("components", Val::List(Vec::new())),
        ("timestamp", Val::Str(timestamp())),
        ("edited_timestamp", Val::Null),
        (
            "author",
            obj(vec![
                ("id", Val::Str(APP_ID.into())),
                ("username", Val::Str("PhoeCord".into())),
                ("discriminator", Val::Str("0000".into())),
                ("avatar", Val::Null),
                ("bot", Val::Bool(true)),
            ]),
        ),
    ]);
    obj(vec![
        ("op", Val::Int(0)),
        ("s", Val::Null),
        ("t", Val::Str("MESSAGE_CREATE".into())),
        ("d", msg),
    ])
}

// ---------------------------------------------------------------------------------------------
// One silent delete
// ---------------------------------------------------------------------------------------------

fn new_job(
    st: &mut State,
    channel: String,
    message: String,
    delete_original: bool,
    purge: Option<u64>,
) -> u64 {
    st.next_id += 1;
    let id = st.next_id;
    st.jobs.insert(
        id,
        Job {
            channel,
            message,
            delete_original,
            delete_delay: 200.0,
            delete_setting: true,
            reply_id: None,
            stage: Stage::Post,
            purge,
        },
    );
    id
}

/// A job ended (`ok` is the script's return value); a purge goes on to its next message.
fn job_done(host: &mut dyn Host, st: &mut State, purge: Option<u64>, ok: bool) {
    let Some(pid) = purge else { return };
    let Some(mut p) = st.purges.remove(&pid) else {
        return;
    };
    if ok {
        p.success += 1;
    }
    if p.i + 1 < p.collected.len() {
        p.i += 1;
        let interval = setting_num(host, "purgeInterval", 500.0);
        // `settings.store.purgeInterval || 500`
        let interval = if interval == 0.0 || interval.is_nan() {
            500.0
        } else {
            interval
        };
        if schedule(host, ms(interval), &format!("purge:{pid}")) {
            st.purges.insert(pid, p);
        }
        return;
    }
    let text = host.t("silent_delete-purged", &[("count", &p.success.to_string())]);
    let frame = bot_message(st, &p.channel, &text);
    host.emit_dispatch(frame);
}

fn job_step(host: &mut dyn Host, st: &mut State, id: u64) {
    let Some(mut job) = st.jobs.remove(&id) else {
        return;
    };
    let tag = format!("job:{id}");
    match job.stage {
        Stage::Post => {
            let text = host
                .setting("replacementText")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "** **".into());
            job.delete_delay = setting_num(host, "deleteDelay", 200.0);
            job.delete_setting = setting_bool(host, "deleteOriginal", true);
            let suppress = setting_bool(host, "suppressNotifications", true);
            let body = obj(vec![
                ("content", Val::Str(text)),
                (
                    "flags",
                    Val::Int(if suppress { SUPPRESS_NOTIFICATIONS } else { 0 }),
                ),
                ("mobile_network_type", Val::Str("unknown".into())),
                ("nonce", Val::Str(job.message.clone())),
                ("tts", Val::Bool(false)),
            ]);
            match rest(
                host,
                "POST",
                &format!("/channels/{}/messages", job.channel),
                Some(&body),
            ) {
                Rest::Wait(w) => {
                    if schedule(host, w, &tag) {
                        st.jobs.insert(id, job);
                    }
                }
                Rest::Done(status, body) if ok(status) => {
                    let reply = body
                        .as_ref()
                        .and_then(|b| b.get("id"))
                        .and_then(|v| match v {
                            Val::Int(n) => Some(n.to_string()),
                            v => text_of(v),
                        })
                        .and_then(|s| snowflake(&s));
                    match reply {
                        Some(rid) => {
                            job.reply_id = Some(rid);
                            job.stage = Stage::DelReply;
                            if schedule(host, ms(job.delete_delay), &tag) {
                                st.jobs.insert(id, job);
                            }
                        }
                        None => fail(host, st, job, "the placeholder has no id"),
                    }
                }
                Rest::Done(status, _) => fail(host, st, job, &format!("HTTP {status}")),
                Rest::Failed(e) => fail(host, st, job, &e),
            }
        }
        Stage::DelReply | Stage::DelOrig => {
            let target = if job.stage == Stage::DelReply {
                job.reply_id.clone().unwrap_or_default()
            } else {
                job.message.clone()
            };
            match rest(
                host,
                "DELETE",
                &format!("/channels/{}/messages/{target}", job.channel),
                None,
            ) {
                Rest::Wait(w) => {
                    if schedule(host, w, &tag) {
                        st.jobs.insert(id, job);
                    }
                }
                Rest::Done(status, _) if ok(status) => {
                    if job.stage == Stage::DelReply && job.delete_original && job.delete_setting {
                        job.stage = Stage::DelOrig;
                        if schedule(host, 100, &tag) {
                            st.jobs.insert(id, job);
                        }
                    } else {
                        let purge = job.purge;
                        job_done(host, st, purge, true);
                    }
                }
                Rest::Done(status, _) => fail(host, st, job, &format!("HTTP {status}")),
                Rest::Failed(e) => fail(host, st, job, &e),
            }
        }
    }
}

fn fail(host: &mut dyn Host, st: &mut State, job: Job, why: &str) {
    host.log(&format!("SilentDelete: error: {why}"));
    job_done(host, st, job.purge, false);
}

// ---------------------------------------------------------------------------------------------
// /silentpurge
// ---------------------------------------------------------------------------------------------

fn purge_step(host: &mut dyn Host, st: &mut State, pid: u64) {
    let Some(mut p) = st.purges.remove(&pid) else {
        return;
    };
    let tag = format!("purge:{pid}");
    if matches!(p.phase, Phase::Delete) {
        let (channel, message) = (p.channel.clone(), p.collected[p.i].clone());
        st.purges.insert(pid, p);
        let jid = new_job(st, channel, message, true, Some(pid));
        job_step(host, st, jid);
        return;
    }
    if (p.collected.len() as f64) < p.count {
        let route = match &p.before {
            Some(b) => format!("/channels/{}/messages?limit=100&before={b}", p.channel),
            None => format!("/channels/{}/messages?limit=100", p.channel),
        };
        match rest(host, "GET", &route, None) {
            Rest::Wait(w) => {
                if schedule(host, w, &tag) {
                    st.purges.insert(pid, p);
                }
                return;
            }
            Rest::Done(status, Some(Val::List(messages))) if ok(status) => {
                if !messages.is_empty() {
                    for m in &messages {
                        if m.str_at(&["author", "id"]) == Some(p.user.as_str())
                            && let Some(id) = m.str_at(&["id"])
                        {
                            p.collected.push(id.to_string());
                            if p.collected.len() as f64 >= p.count {
                                break;
                            }
                        }
                    }
                    p.before = messages
                        .last()
                        .and_then(|m| m.str_at(&["id"]))
                        .and_then(snowflake);
                    if messages.len() >= 100 {
                        // `await sleep(100)`, then the loop condition decides.
                        if schedule(host, 100, &tag) {
                            st.purges.insert(pid, p);
                        }
                        return;
                    }
                }
            }
            Rest::Done(status, _) if ok(status) => {}
            Rest::Done(status, _) => {
                host.log(&format!(
                    "SilentDelete: error during silent purge: HTTP {status}"
                ));
                return;
            }
            Rest::Failed(e) => {
                host.log(&format!("SilentDelete: error during silent purge: {e}"));
                return;
            }
        }
    }
    // The collecting loop is over.
    if p.collected.is_empty() {
        return;
    }
    p.phase = Phase::Delete;
    p.i = 0;
    let (channel, message) = (p.channel.clone(), p.collected[0].clone());
    st.purges.insert(pid, p);
    let jid = new_job(st, channel, message, true, Some(pid));
    job_step(host, st, jid);
}

// ---------------------------------------------------------------------------------------------
// The commands
// ---------------------------------------------------------------------------------------------

fn handle(
    host: &mut dyn Host,
    st: &mut State,
    sub: &str,
    options: &[(String, String)],
    here: Option<String>,
) -> Option<String> {
    let channel = match opt(options, "channel") {
        Some(c) => snowflake(c),
        None => here.as_deref().and_then(snowflake),
    };
    match sub {
        "silentdelete" => {
            let Some(channel) = channel else {
                return Some(host.t("silent_delete-bad-channel", &[]));
            };
            let Some(message) = opt(options, "message").and_then(snowflake) else {
                return Some(host.t("silent_delete-bad-message", &[]));
            };
            let history = matches!(
                opt(options, "history").map(str::to_lowercase).as_deref(),
                Some("true" | "yes" | "on" | "1")
            );
            let id = new_job(st, channel, message, !history, None);
            if !schedule(host, 50, &format!("job:{id}")) {
                st.jobs.remove(&id);
                return Some(host.t("silent_delete-no-timer", &[]));
            }
            Some(host.t("silent_delete-started", &[]))
        }
        "silentpurge" => {
            // `Number(value)`; `!count || count < 1` ends the command without doing anything.
            let count = opt(options, "count")
                .and_then(|c| c.parse::<f64>().ok())
                .filter(|c| *c >= 1.0);
            let Some(count) = count else {
                return Some(host.t("silent_delete-bad-count", &[]));
            };
            let Some(channel) = channel else {
                return Some(host.t("silent_delete-bad-channel", &[]));
            };
            let Some(user) = host.self_id().map(|u| u.to_string()) else {
                return Some(host.t("silent_delete-not-signed-in", &[]));
            };
            st.next_id += 1;
            let pid = st.next_id;
            if !schedule(host, 50, &format!("purge:{pid}")) {
                return Some(host.t("silent_delete-no-timer", &[]));
            }
            st.purges.insert(
                pid,
                Purge {
                    channel,
                    user,
                    count,
                    collected: Vec::new(),
                    before: None,
                    phase: Phase::Collect,
                    i: 0,
                    success: 0,
                },
            );
            Some(host.t("silent_delete-purge-started", &[("count", &js_num(count))]))
        }
        _ => None,
    }
}

fn js_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

// ---------------------------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------------------------

pub struct SilentDelete {
    manifest: Manifest,
    state: Shared<State>,
}

impl SilentDelete {
    pub fn new() -> SilentDelete {
        SilentDelete {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "SilentDelete",
                    env!("CARGO_PKG_VERSION"),
                    "\"Silently\" delete a message: replace it with a placeholder first",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("SilentDelete".into());
                m.menu = vec![
                    MenuEntry::new(MENU_DELETE, "menu-silent-delete"),
                    MenuEntry::new(MENU_HISTORY, "menu-silent-delete-history"),
                ];
                m
            },
            state: Shared::new(State::default()),
        }
    }

    /// `/phoecord silentdelete` and `/phoecord silentpurge`. Register both with `Commands::with`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        let reply = |state: &Shared<State>| {
            let state = state.clone();
            move |host: &mut dyn Host, _: &Args| {
                state
                    .lock()
                    .reply
                    .take()
                    .unwrap_or_else(|| host.t("silent_delete-no-reply", &[]))
            }
        };
        vec![
            FnCmd::new(
                PLUGIN_ID,
                "silentdelete",
                "Silently delete one of your messages: replace it with a placeholder, then delete",
                reply(&self.state),
            )
            .option("message", "Id of your message to silently delete", true)
            .option("channel", "Channel id (default: this channel)", false)
            .option(
                "history",
                "true: only the placeholder round trip, keep the original (Silent Delete History)",
                false,
            )
            .boxed(),
            FnCmd::new(
                PLUGIN_ID,
                "silentpurge",
                "Silently delete your recent messages in this channel",
                reply(&self.state),
            )
            .option(
                "count",
                "Number of your messages to silently delete (1-100)",
                true,
            )
            .option("channel", "Channel id (default: this channel)", false)
            .boxed(),
        ]
    }
}

impl Default for SilentDelete {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for SilentDelete {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Dispatch { name } => *name == TIMER_EVENT,
            EventKind::Ui { name } => *name == "menu_action",
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if let Some((sub, options, here)) = parse_interaction(body) {
                    let mut st = self.state.lock();
                    if let Some(reply) = handle(host, &mut st, &sub, &options, here) {
                        st.reply = Some(reply);
                    }
                }
                Verdict::Pass
            }
            Event::Ui(UiEvent::MenuAction {
                entry,
                channel_id,
                message_id,
            }) if entry == MENU_DELETE || entry == MENU_HISTORY => {
                let options = vec![
                    ("message".to_string(), message_id.to_string()),
                    ("channel".to_string(), channel_id.to_string()),
                    ("history".to_string(), (entry == MENU_HISTORY).to_string()),
                ];
                let reply = handle(host, &mut self.state.lock(), "silentdelete", &options, None);
                if let Some(text) = reply {
                    host.emit_dispatch(crate::commands::local_message(channel_id, &text));
                }
                Verdict::Pass
            }
            Event::Dispatch { name, payload, .. } if name == TIMER_EVENT => {
                let tag = payload
                    .str_at(&["d", "tag"])
                    .or_else(|| payload.str_at(&["tag"]))
                    .unwrap_or("")
                    .to_string();
                let mut st = self.state.lock();
                if let Some((kind, id)) = tag.split_once(':')
                    && let Ok(id) = id.parse::<u64>()
                {
                    match kind {
                        "job" => job_step(host, &mut st, id),
                        "purge" => purge_step(host, &mut st, id),
                        _ => {}
                    }
                }
                Verdict::Pass
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_core::UiEvent;

    fn host() -> Rec {
        Rec {
            self_id: Some(5),
            ..Rec::default()
        }
    }

    fn command(p: &SilentDelete, name: &str, h: &mut Rec, opts: &[(&str, &str)]) -> String {
        let options: Vec<String> = opts
            .iter()
            .map(|(k, v)| format!(r#"{{"type":3,"name":"{k}","value":"{v}"}}"#))
            .collect();
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"{name}","options":[{}]}}]}}}}"#,
            options.join(",")
        ));
        let v = p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        assert_eq!(v, Verdict::Pass);
        let cmds = p.commands();
        let c = cmds.iter().find(|c| c.name() == name).unwrap();
        c.run(
            h,
            &Args {
                options: Vec::new(),
            },
        )
    }

    fn tick(p: &SilentDelete, h: &mut Rec, tag: &str) {
        let mut f = Rec::timer_frame(tag);
        p.on_event(
            h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
    }

    #[test]
    fn a_silent_delete_posts_waits_then_deletes_both() {
        let p = SilentDelete::new();
        let mut h = host()
            .answer("POST", "/channels/9/messages", 200, r#"{"id":"500"}"#)
            .answer("DELETE", "/channels/9/messages/500", 204, "null")
            .answer("DELETE", "/channels/9/messages/42", 204, "null");
        let r = command(&p, "silentdelete", &mut h, &[("message", "42")]);
        assert!(r.contains("Silent delete started"), "{r}");
        assert_eq!(h.scheduled, vec![(50, "job:1".to_string())]);
        tick(&p, &mut h, "job:1");
        let (key, body) = &h.rest_calls[0];
        assert_eq!(key, "POST /channels/9/messages");
        let body = body.as_ref().unwrap();
        assert_eq!(body.str_at(&["content"]), Some("** **"));
        assert_eq!(body.get("flags").and_then(Val::as_i64), Some(4096));
        assert_eq!(body.str_at(&["nonce"]), Some("42"));
        assert_eq!(body.str_at(&["mobile_network_type"]), Some("unknown"));
        assert_eq!(body.get("tts").and_then(Val::as_bool), Some(false));
        assert_eq!(h.scheduled.last(), Some(&(200, "job:1".to_string())));
        tick(&p, &mut h, "job:1");
        assert_eq!(h.rest_calls[1].0, "DELETE /channels/9/messages/500");
        assert_eq!(h.scheduled.last(), Some(&(100, "job:1".to_string())));
        tick(&p, &mut h, "job:1");
        assert_eq!(h.rest_calls[2].0, "DELETE /channels/9/messages/42");
        assert_eq!(h.scheduled.len(), 3, "nothing after the last delete");
    }

    #[test]
    fn settings_change_the_placeholder_the_delay_and_the_original() {
        let p = SilentDelete::new();
        let mut h = host()
            .with("replacementText", Val::Str("x".into()))
            .with("deleteDelay", Val::Int(350))
            .with("suppressNotifications", Val::Bool(false))
            .with("deleteOriginal", Val::Bool(false))
            .answer("POST", "/channels/9/messages", 200, r#"{"id":"500"}"#)
            .answer("DELETE", "/channels/9/messages/500", 204, "null");
        command(&p, "silentdelete", &mut h, &[("message", "42")]);
        tick(&p, &mut h, "job:1");
        let body = h.rest_calls[0].1.clone().unwrap();
        assert_eq!(body.str_at(&["content"]), Some("x"));
        assert_eq!(body.get("flags").and_then(Val::as_i64), Some(0));
        assert_eq!(h.scheduled.last(), Some(&(350, "job:1".to_string())));
        tick(&p, &mut h, "job:1");
        // deleteOriginal off: the original stays and no 100 ms wait follows.
        assert_eq!(h.rest_calls.len(), 2);
        assert_eq!(h.scheduled.len(), 2);
    }

    #[test]
    fn history_keeps_the_original_and_errors_end_the_job() {
        let p = SilentDelete::new();
        let mut h = host()
            .answer("POST", "/channels/9/messages", 200, r#"{"id":"500"}"#)
            .answer("DELETE", "/channels/9/messages/500", 204, "null");
        command(
            &p,
            "silentdelete",
            &mut h,
            &[("message", "42"), ("history", "true")],
        );
        tick(&p, &mut h, "job:1");
        tick(&p, &mut h, "job:1");
        assert_eq!(h.rest_calls.len(), 2);
        // A refused post (403) is a failure: nothing else is called.
        let p = SilentDelete::new();
        let mut h = host().answer("POST", "/channels/9/messages", 403, "null");
        command(&p, "silentdelete", &mut h, &[("message", "42")]);
        tick(&p, &mut h, "job:1");
        assert_eq!(h.rest_calls.len(), 1);
        assert_eq!(h.scheduled.len(), 1);
        assert!(h.logs.iter().any(|l| l.contains("error")));
        // A post answer without an id cannot be deleted.
        let p = SilentDelete::new();
        let mut h = host().answer("POST", "/channels/9/messages", 200, "{}");
        command(&p, "silentdelete", &mut h, &[("message", "42")]);
        tick(&p, &mut h, "job:1");
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn purge_collects_pages_then_deletes_with_the_interval() {
        let p = SilentDelete::new();
        let mine = |id: u32| format!(r#"{{"id":"{id}","author":{{"id":"5"}}}}"#);
        let other = |id: u32| format!(r#"{{"id":"{id}","author":{{"id":"6"}}}}"#);
        let mut page1: Vec<String> = (1000..1099).map(other).collect();
        page1.insert(0, mine(2000));
        let page1 = format!("[{}]", page1.join(","));
        let page2 = format!("[{},{}]", mine(3000), mine(3001));
        let mut h = host()
            .answer("GET", "/channels/9/messages?limit=100", 200, &page1)
            .answer(
                "GET",
                "/channels/9/messages?limit=100&before=1098",
                200,
                &page2,
            )
            .with("purgeInterval", Val::Int(700));
        for id in ["2000", "3000", "3001"] {
            h = h
                .answer("POST", "/channels/9/messages", 200, r#"{"id":"900"}"#)
                .answer("DELETE", "/channels/9/messages/900", 204, "null")
                .answer("DELETE", &format!("/channels/9/messages/{id}"), 204, "null");
        }
        let r = command(&p, "silentpurge", &mut h, &[("count", "3")]);
        assert!(r.contains("3"), "{r}");
        tick(&p, &mut h, "purge:1");
        assert_eq!(h.scheduled.last(), Some(&(100, "purge:1".to_string())));
        tick(&p, &mut h, "purge:1");
        // Collected 3: the first message's silent delete started at once.
        assert_eq!(
            h.rest_calls[1].0,
            "GET /channels/9/messages?limit=100&before=1098"
        );
        assert_eq!(h.rest_calls[2].0, "POST /channels/9/messages");
        let jid = |h: &Rec| h.scheduled.last().unwrap().1.clone();
        let tag = jid(&h);
        tick(&p, &mut h, &tag);
        tick(&p, &mut h, &tag);
        // After the last delete of message 1: wait purgeInterval, then message 2.
        assert_eq!(h.scheduled.last(), Some(&(700, "purge:1".to_string())));
        for _ in 0..2 {
            tick(&p, &mut h, "purge:1");
            let tag = jid(&h);
            tick(&p, &mut h, &tag);
            tick(&p, &mut h, &tag);
        }
        let done = h.dispatched.last().expect("the bot message");
        assert_eq!(done.str_at(&["t"]), Some("MESSAGE_CREATE"));
        assert!(
            done.str_at(&["d", "content"])
                .unwrap()
                .contains("3 message")
        );
        assert_eq!(done.str_at(&["d", "channel_id"]), Some("9"));
    }

    #[test]
    fn purge_with_nothing_to_delete_or_bad_input_says_nothing_more() {
        let p = SilentDelete::new();
        let mut h = host().answer("GET", "/channels/9/messages?limit=100", 200, "[]");
        assert!(!command(&p, "silentpurge", &mut h, &[("count", "0")]).is_empty());
        assert!(!command(&p, "silentpurge", &mut h, &[("count", "abc")]).is_empty());
        assert!(h.scheduled.is_empty());
        command(&p, "silentpurge", &mut h, &[("count", "2")]);
        tick(&p, &mut h, "purge:1");
        assert_eq!(h.scheduled.len(), 1, "no page, no messages: it just ends");
        assert!(h.dispatched.is_empty());
        // Unknown tags and ids do nothing; no sign-in is reported.
        tick(&p, &mut h, "job:99");
        tick(&p, &mut h, "weird");
        let mut anon = Rec::default();
        assert!(!command(&p, "silentpurge", &mut anon, &[("count", "1")]).is_empty());
        assert!(!command(&p, "silentdelete", &mut h, &[("message", "x/y")]).is_empty());
    }

    #[test]
    fn a_local_rate_limit_is_waited_out_not_failed() {
        struct Limited(Rec);
        impl Host for Limited {
            fn setting(&self, k: &str) -> Option<Val> {
                self.0.setting(k)
            }
            fn kv_get(&self, _: &str) -> Option<Val> {
                None
            }
            fn kv_set(&mut self, _: &str, _: Val) {}
            fn self_id(&self) -> Option<u64> {
                Some(5)
            }
            fn notify(&mut self, _: &str, _: &str) {}
            fn log(&mut self, m: &str) {
                self.0.log(m)
            }
            fn emit_send(&mut self, _: i64, _: Val) {}
            fn guild_emojis(&self, _: u64) -> Vec<u64> {
                Vec::new()
            }
            fn channel_guild(&self, _: u64) -> Option<u64> {
                None
            }
            fn t(&self, k: &str, a: &[(&str, &str)]) -> String {
                self.0.t(k, a)
            }
            fn schedule(&mut self, ms: u64, tag: &str) -> Result<(), String> {
                self.0.schedule(ms, tag)
            }
            fn rest(
                &mut self,
                _: &str,
                _: &str,
                _: Option<&Val>,
            ) -> Result<phoecord_core::plugin::HostResponse, String> {
                Err(
                    "rate limited: at most 5 Discord API calls per 5 s per plugin, retry in 1.5s"
                        .into(),
                )
            }
        }

        impl phoecord_core::ChainHost for Limited {}
        let p = SilentDelete::new();
        let mut l = Limited(host());
        command(&p, "silentdelete", &mut l.0, &[("message", "42")]);
        let mut f = Rec::timer_frame("job:1");
        p.on_event(
            &mut l,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
        assert_eq!(l.0.scheduled.last(), Some(&(1500, "job:1".to_string())));
    }

    #[test]
    fn manifest_wants_and_garbage_frames() {
        let p = SilentDelete::new();
        assert_eq!(p.manifest().id, "silent_delete");
        assert_eq!(p.manifest().vencord.as_deref(), Some("SilentDelete"));
        assert!(p.wants(&EventKind::Dispatch { name: TIMER_EVENT }));
        assert!(p.wants_gateway());
        let mut h = host();
        let mut body = j(r#"{"type":2,"data":{"name":"phoecord"}}"#);
        p.on_event(
            &mut h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        let mut f = j(r#"{"op":0,"t":"PHOECORD_TIMER"}"#);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
        assert!(h.rest_calls.is_empty());
    }

    fn click_menu(p: &dyn Plugin, h: &mut Rec, entry: &str) {
        assert!(p.wants(&EventKind::Ui {
            name: "menu_action"
        }));
        p.on_event(
            h,
            Event::Ui(UiEvent::MenuAction {
                entry,
                channel_id: 9,
                message_id: 5,
            }),
        );
    }

    fn local_text(h: &Rec) -> String {
        let f = h.dispatched.last().expect("a local message");
        assert_eq!(f.str_at(&["t"]), Some("MESSAGE_CREATE"));
        assert_eq!(f.str_at(&["d", "channel_id"]), Some("9"));
        assert_eq!(f.at(&["d", "flags"]), Some(&Val::Int(64)));
        f.str_at(&["d", "content"]).unwrap().to_string()
    }

    #[test]
    fn menu_entries_start_a_silent_delete() {
        let p = SilentDelete::new();
        let ids: Vec<&str> = p.manifest().menu.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, [MENU_DELETE, MENU_HISTORY]);
        let mut h = Rec::default();
        click_menu(&p, &mut h, MENU_DELETE);
        assert_eq!(h.scheduled.len(), 1, "{:?}", h.scheduled);
        assert!(h.scheduled[0].1.starts_with("job:"));
        assert!(!local_text(&h).is_empty());
        let job = p
            .state
            .lock()
            .jobs
            .values()
            .next()
            .map(|j| j.delete_setting);
        assert!(job.is_some());
        click_menu(&p, &mut h, MENU_HISTORY);
        assert_eq!(h.scheduled.len(), 2);
    }
}
