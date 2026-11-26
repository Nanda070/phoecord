//! rmcord: delete your own messages in bulk, with filters, a schedule and adaptive delays
//! (Vencord userplugin: rmcord).
//!
//! Ported from the owner's userplugin `rmcord`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/rmcord, GPL-3.0-or-later,
//! Copyright (c) 2024 Vendicated and contributors), modified for PhoeCord 2026-11-25. The
//! script's README and LEGAL.md (GPL-3.0 licence text included there) travel with this port in
//! spirit: the warning below is shown by `/phoecord rmcord` (`action: help`, and on start).
//!
//! WARNING (from the script's README and LEGAL.md): automating actions on a user account can
//! violate Discord's Terms of Service. Your account can be rate limited, suspended or banned,
//! and deleted messages are gone for good. The authors have no affiliation with Discord Inc., give
//! no warranty and accept no liability. You use this entirely at your own risk and are
//! responsible for following Discord's current Terms of Service and the law where you live.
//! Discord rate limits every API action; the engine backs off on its own, and a 10 s pause
//! between search pages is intentional (the search index needs time to catch up after deletions).
//!
//! Switching it on: `[plugins.rmcord] enabled = true` (off by default). The script has no
//! settings. The panel's controls are the `/phoecord rmcord` command (`action` picks the control):
//! - `start`: delete in `channels` (ids, comma or space separated) with the filters `from` and `to`
//!   (a date, `YYYY-MM-DDTHH:MM` in your local time or `YYYY-MM-DD`), `types` (a list of text,
//!   image, video, link, embed, sound, file, sticker; default all), `regex` and `mode` (`include`
//!   or `exclude`, default include).
//! - `schedule`: the same options plus `at` (a date and time): starts then.
//! - `cancel` (drop the schedule), `pause`, `resume`, `stop`, `status` (counts, current channel,
//!   ETA, schedule), `log` (the latest progress lines), `help`.
//!
//! The engine is the script's `utils.tsx` 1:1: search through `GET /guilds/{id}/messages/search`
//! (`GET /channels/{id}/messages/search` in DMs) with `author_id`, `sort_by=timestamp`,
//! `sort_order=desc`, `offset`, `channel_id` (guilds), `min_id` / `max_id` from the date filters;
//! only your own hits of type 0 and 6 to 21 are deleted, one by one with `DELETE
//! /channels/{id}/messages/{id}` (the bulk endpoint is for bots). `SEARCH_DELAY` 10000 ms and
//! `DELETE_DELAY` 600 ms adapt: a 429 raises the search delay by `retry_after` or the delete delay
//! to it, then waits `retry_after * 2`; every 5 loops without a throttle both fall back by 30 %
//! (never below the base). A 400 with code 50083 (archived thread) skips the message; other
//! delete errors are retried up to 3 times; search answers 202 (not indexed) wait 5 s; an empty
//! page with results pending waits one search delay; filtered-out hits move the offset on.
//! Every `sleep` of the script is one timer step (`host.schedule`), nothing blocks.
//!
//! A local refusal of the host's REST limiter ("rate limited: ... retry in X.Xs") is handled as a
//! Discord 429 with `retry_after = X`, the same way (delays rise, wait `X * 2`).
//!
//! How commands reach the engine: a `/phoecord` subcommand runs inside the command plugin, whose
//! timers and REST calls belong to it. So this plugin reads the `POST /interactions` of its own
//! subcommand (it sits before the command plugin in the chain), does the work as itself, and the
//! subcommand only hands back the text.
//!
//! Not ported (on-screen, Phase 2): the floating panel, the toolbar button and its progress bar,
//! the guild, DM and "import dump" pickers (give channel ids instead; the picker only makes a
//! selection), streamer mode, the log export, the date and type widgets. Regexes use Rust's
//! `regex` syntax (no look-around or back-references); one it cannot compile is ignored, as the
//! script ignores an invalid one. Pausing, like the script's, takes effect at the next loop check.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Guilds, Shared, Users, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, TIMER_EVENT, Val, Verdict};
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const PLUGIN_ID: &str = "rmcord";
const STEP: &str = "step";
const SCHED: &str = "sched";
const SEARCH_DELAY: f64 = 10_000.0;
const DELETE_DELAY: f64 = 600.0;
const MAX_RETRY: u32 = 3;
const DISCORD_EPOCH: i128 = 1_420_070_400_000;
const COOLDOWN_ITERATIONS: u32 = 5;
const DAY_MS: i64 = 86_400_000;
/// Progress lines kept for `log`.
const LOG_LINES: usize = 5000;
/// Lines `log` shows.
const LOG_SHOWN: usize = 20;

const TYPES: [&str; 8] = [
    "text", "image", "video", "link", "embed", "sound", "file", "sticker",
];

/// Dispatches that tell which channels and DMs exist.
const EVENTS: &[&str] = &[
    "READY",
    "GUILD_CREATE",
    "GUILD_UPDATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
];

// ---------------------------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Filters {
    min_id: Option<String>,
    max_id: Option<String>,
    types: [bool; 8],
    regex: Option<Regex>,
    include: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Filters {
            min_id: None,
            max_id: None,
            types: [true; 8],
            regex: None,
            include: true,
        }
    }
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://\S+").expect("static regex"))
}

fn date_to_snowflake(ms: i64) -> String {
    ((i128::from(ms) - DISCORD_EPOCH) << 22).to_string()
}

fn content_type(a: &Val) -> &str {
    a.str_at(&["content_type"]).unwrap_or("")
}

fn matches_types(msg: &Val, types: &[bool; 8]) -> bool {
    if types.iter().all(|t| *t) {
        return true;
    }
    let has = |name: &str| {
        TYPES
            .iter()
            .position(|t| *t == name)
            .is_some_and(|i| types[i])
    };
    let content = msg.str_at(&["content"]).unwrap_or("");
    let attachments = msg.get("attachments").map(Val::as_list).unwrap_or(&[]);
    let any = |prefix: &str| {
        attachments
            .iter()
            .any(|a| content_type(a).starts_with(prefix) && !content_type(a).is_empty())
    };
    if has("text") && !content.trim().is_empty() {
        return true;
    }
    if has("image") && any("image/") {
        return true;
    }
    if has("video") && any("video/") {
        return true;
    }
    if has("sound") && any("audio/") {
        return true;
    }
    if has("file")
        && attachments.iter().any(|a| {
            let c = content_type(a);
            !c.is_empty()
                && !c.starts_with("image/")
                && !c.starts_with("video/")
                && !c.starts_with("audio/")
        })
    {
        return true;
    }
    if has("link") && link_re().is_match(content) {
        return true;
    }
    if has("embed")
        && !msg
            .get("embeds")
            .map(Val::as_list)
            .unwrap_or(&[])
            .is_empty()
    {
        return true;
    }
    if has("sticker")
        && !msg
            .get("sticker_items")
            .map(Val::as_list)
            .unwrap_or(&[])
            .is_empty()
    {
        return true;
    }
    false
}

fn matches_regex(msg: &Val, f: &Filters) -> bool {
    let Some(re) = &f.regex else { return true };
    let hit = re.is_match(msg.str_at(&["content"]).unwrap_or(""));
    if f.include { hit } else { !hit }
}

fn matches_filters(msg: &Val, f: &Filters) -> bool {
    matches_types(msg, &f.types) && matches_regex(msg, f)
}

// ---------------------------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------------------------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// The local UTC offset in seconds at this unix time (0 when the system will not say).
fn local_offset(unix_secs: i64) -> i64 {
    time::OffsetDateTime::from_unix_timestamp(unix_secs)
        .ok()
        .and_then(|t| time::UtcOffset::local_offset_at(t).ok())
        .map_or(0, |o| i64::from(o.whole_seconds()))
}

/// What JavaScript's `new Date(text).getTime()` gives for the shapes the panel's date inputs make:
/// `YYYY-MM-DD` (UTC), `YYYY-MM-DDTHH:MM[:SS[.fff]]` (local time), and the same with `Z` or
/// `+HH:MM` (that zone). `None` for anything else.
fn parse_date(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = match text.split_once(['T', ' ']) {
        Some((d, r)) => (d, Some(r)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let day_ms = days_from_civil(y, m, d) * DAY_MS;
    let Some(rest) = rest else {
        return Some(day_ms);
    };
    let (clock, zone) = if let Some(c) = rest.strip_suffix(['Z', 'z']) {
        (c, Some(0))
    } else if let Some(i) = rest.rfind(['+', '-']).filter(|i| *i > 0) {
        let (c, z) = rest.split_at(i);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let (zh, zm) = z[1..].split_once(':')?;
        let secs =
            i64::from(zh.parse::<u8>().ok()?) * 3600 + i64::from(zm.parse::<u8>().ok()?) * 60;
        (c, Some(sign * secs))
    } else {
        (rest, None)
    };
    let mut hms = clock.split(':');
    let hh: i64 = hms.next()?.parse().ok()?;
    let mm: i64 = hms.next()?.parse().ok()?;
    let (ss, frac_ms) = match hms.next() {
        None => (0, 0),
        Some(s) => {
            let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
            let ms = format!("{frac:0<3}");
            (
                whole.parse::<i64>().ok()?,
                ms.get(..3)?.parse::<i64>().ok()?,
            )
        }
    };
    if hms.next().is_some()
        || !(0..24).contains(&hh)
        || !(0..60).contains(&mm)
        || !(0..60).contains(&ss)
    {
        return None;
    }
    let naive = day_ms + ((hh * 60 + mm) * 60 + ss) * 1000 + frac_ms;
    let offset = zone.unwrap_or_else(|| local_offset(naive / 1000));
    Some(naive - offset * 1000)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// `HH:MM:SS` in local time, like the script's `toLocaleTimeString`.
fn clock(ms: i64) -> String {
    let secs = ms.div_euclid(1000) + local_offset(ms.div_euclid(1000));
    let day = secs.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", day / 3600, day % 3600 / 60, day % 60)
}

fn format_eta(ms: f64) -> String {
    let seconds = (ms / 1000.0).ceil() as i64;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, secs) = (seconds / 60, seconds % 60);
    if minutes < 60 {
        return format!("{minutes}m {secs}s");
    }
    format!("{}h {}m", minutes / 60, minutes % 60)
}

/// A number as JavaScript prints it in a template string.
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

// ---------------------------------------------------------------------------------------------
// REST
// ---------------------------------------------------------------------------------------------

struct Resp {
    /// `None`: the call never got an answer (the script's "unknown" status).
    status: Option<u16>,
    body: Option<Val>,
    /// `retry_after` in seconds.
    retry_after: Option<f64>,
}

fn num(v: &Val) -> Option<f64> {
    match v {
        Val::Float(f) => Some(*f),
        v => v.as_i64().map(|n| n as f64),
    }
}

/// X of "rate limited: ... retry in X.Xs".
fn retry_in(err: &str) -> Option<f64> {
    if !err.contains("rate limited") {
        return None;
    }
    let tail = err.split("retry in ").nth(1)?;
    tail.trim_end_matches('s')
        .split('s')
        .next()?
        .trim()
        .parse()
        .ok()
}

fn call(host: &mut dyn Host, method: &str, route: &str) -> Resp {
    match host.rest(method, route, None) {
        Ok(r) => {
            let retry_after = r
                .body
                .as_ref()
                .and_then(|b| b.get("retry_after"))
                .and_then(num)
                .or_else(|| {
                    r.headers
                        .iter()
                        .find(|(k, _)| k == "retry-after")
                        .and_then(|(_, v)| v.trim().parse().ok())
                });
            Resp {
                status: Some(r.status),
                body: r.body,
                retry_after,
            }
        }
        // A local refusal is a 429 for the engine, as the owner ruled.
        Err(e) => match retry_in(&e) {
            Some(x) => Resp {
                status: Some(429),
                body: None,
                retry_after: Some(x),
            },
            None => Resp {
                status: None,
                body: None,
                retry_after: None,
            },
        },
    }
}

fn status_text(s: Option<u16>, host: &dyn Host) -> String {
    s.map_or_else(|| host.t("rmcord-unknown", &[]), |s| s.to_string())
}

/// `(retryAfter || 2) * 1000`.
fn wait_ms(r: &Resp) -> f64 {
    r.retry_after.filter(|x| *x != 0.0).unwrap_or(2.0) * 1000.0
}

// ---------------------------------------------------------------------------------------------
// Job
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Next {
    /// The top of `runDeletion`'s channel loop.
    Channel,
    /// The top of `deleteChannelMessages`' search loop.
    Search,
    /// The `for` over the page's messages (and what follows it).
    Delete,
}

struct Delays {
    search: f64,
    delete: f64,
    since_throttle: u32,
}

#[derive(Default)]
struct Progress {
    deleted: u64,
    failed: u64,
    total: i64,
    current: Option<String>,
    eta: Option<String>,
    paused: bool,
    stopped: bool,
    logs: VecDeque<String>,
}

struct ChanRun {
    id: u64,
    guild: Option<u64>,
    label: String,
    offset: i64,
    total_counted: bool,
    total: i64,
    deleted: i64,
    start: Instant,
    queue: VecDeque<Val>,
    /// The message being deleted and how many attempts it has had.
    cur: Option<(Val, u32)>,
}

struct Job {
    user: String,
    filters: Filters,
    channels: Vec<u64>,
    ci: usize,
    delays: Delays,
    paused: bool,
    stopped: bool,
    finished: bool,
    next: Next,
    ch: Option<ChanRun>,
    progress: Progress,
}

impl Job {
    fn new(user: String, channels: Vec<u64>, filters: Filters) -> Job {
        Job {
            user,
            filters,
            channels,
            ci: 0,
            delays: Delays {
                search: SEARCH_DELAY,
                delete: DELETE_DELAY,
                since_throttle: 0,
            },
            paused: false,
            stopped: false,
            finished: false,
            next: Next::Channel,
            ch: None,
            progress: Progress::default(),
        }
    }

    fn running(&self) -> bool {
        !self.finished
    }
}

struct Sched {
    at: i64,
    channels: Vec<u64>,
    filters: Filters,
}

#[derive(Default)]
struct State {
    guilds: Guilds,
    users: Users,
    /// DM and group DM channels: id -> first recipient.
    dms: HashMap<u64, Option<u64>>,
    job: Option<Job>,
    sched: Option<Sched>,
    /// What the `/phoecord rmcord` command should show for the interaction just handled.
    reply: Option<String>,
}

impl State {
    fn observe(&mut self, name: &str, d: &Val) {
        self.guilds.observe(name, d);
        match name {
            "READY" => {
                self.users.observe(name, d);
                self.dms.clear();
                for c in d.get("private_channels").map(Val::as_list).unwrap_or(&[]) {
                    self.put_dm(c);
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => self.put_dm(d),
            "CHANNEL_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.dms.remove(&id);
                }
            }
            _ => {}
        }
    }

    fn put_dm(&mut self, c: &Val) {
        if c.get("guild_id").is_some_and(|g| !matches!(g, Val::Null)) {
            return;
        }
        let Some(id) = c.id_at(&["id"]) else { return };
        if !matches!(c.get("type").and_then(Val::as_i64), Some(1 | 3)) {
            return;
        }
        let mut first = c
            .get("recipient_ids")
            .and_then(|l| l.as_list().first())
            .and_then(Val::snowflake);
        for r in c.get("recipients").map(Val::as_list).unwrap_or(&[]) {
            if matches!(r, Val::Map(_)) {
                self.users.learn(r);
                first = first.or_else(|| r.id_at(&["id"]));
            } else {
                first = first.or_else(|| r.snowflake());
            }
        }
        self.dms.insert(id, first);
    }
}

fn t(host: &dyn Host, key: &str, args: &[(&str, &str)]) -> String {
    host.t(key, args)
}

/// Add a progress line (the script's `infoLog` / `messageLog`) and write it to the log.
fn note(host: &mut dyn Host, job: &mut Job, text: String) {
    let line = format!("{} {text}", clock(now_ms()));
    host.log(&format!("rmcord: {text}"));
    job.progress.logs.push_back(line);
    while job.progress.logs.len() > LOG_LINES {
        job.progress.logs.pop_front();
    }
}

/// Wait `ms` (a timer step) and run the next step then.
fn again(host: &mut dyn Host, job: &mut Job, ms: f64) {
    if let Err(e) = host.schedule(ms.max(0.0).ceil() as u64, STEP) {
        let text = format!("could not schedule the next step: {e}");
        note(host, job, text);
        job.finished = true;
        job.progress.stopped = true;
    }
}

fn finish(host: &mut dyn Host, job: &mut Job) {
    if job.stopped {
        let text = t(host, "rmcord-log-stopped", &[]);
        note(host, job, text);
    } else {
        let text = t(host, "rmcord-log-done", &[]);
        note(host, job, text);
        job.progress.stopped = true;
    }
    job.progress.current = None;
    job.progress.eta = None;
    job.finished = true;
}

fn end_channel(job: &mut Job) {
    job.ch = None;
    job.progress.current = None;
    job.progress.eta = None;
}

/// The script's `maybeRecoverDelays`.
fn maybe_recover(d: &mut Delays) {
    d.since_throttle += 1;
    if d.since_throttle >= COOLDOWN_ITERATIONS {
        if d.search > SEARCH_DELAY {
            d.search = SEARCH_DELAY.max((d.search * 0.7).floor());
        }
        if d.delete > DELETE_DELAY {
            d.delete = DELETE_DELAY.max((d.delete * 0.7).floor());
        }
        d.since_throttle = 0;
    }
}

/// `(guild id, label)` of a channel the client knows, as `runDeletion` labels it.
fn lookup(
    host: &dyn Host,
    st: (&Guilds, &Users, &HashMap<u64, Option<u64>>),
    id: u64,
) -> Option<(Option<u64>, String)> {
    let (guilds, users, dms) = st;
    let guild_label = |gid: u64, name: &str| {
        let g = guilds
            .guild(gid)
            .map(|g| g.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("???");
        format!("{g} > #{name}")
    };
    if let Some(c) = guilds.channel(id) {
        return Some((Some(c.guild_id), guild_label(c.guild_id, &c.name)));
    }
    if let Some(recipient) = dms.get(&id) {
        let name = recipient
            .and_then(|u| users.name(u))
            .unwrap_or("Unknown")
            .to_string();
        return Some((None, format!("DM > {name}")));
    }
    host.channel_guild(id)
        .map(|g| (Some(g), guild_label(g, "")))
}

fn search_route(ch: &ChanRun, user: &str, f: &Filters) -> String {
    let mut q = format!(
        "author_id={user}&sort_by=timestamp&sort_order=desc&offset={}",
        ch.offset
    );
    if ch.guild.is_some() {
        q.push_str(&format!("&channel_id={}", ch.id));
    }
    if let Some(m) = &f.min_id {
        q.push_str(&format!("&min_id={m}"));
    }
    if let Some(m) = &f.max_id {
        q.push_str(&format!("&max_id={m}"));
    }
    match ch.guild {
        Some(g) => format!("/guilds/{g}/messages/search?{q}"),
        None => format!("/channels/{}/messages/search?{q}", ch.id),
    }
}

fn is_ok(r: &Resp) -> bool {
    r.status.is_some_and(|s| (200..300).contains(&s))
}

fn message_log(host: &mut dyn Host, job: &mut Job, ok: bool, msg: &Val, channel: u64) {
    let content = match msg.str_at(&["content"]).filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None if !msg
            .get("attachments")
            .map(Val::as_list)
            .unwrap_or(&[])
            .is_empty() =>
        {
            "[attachment]".to_string()
        }
        None => String::new(),
    };
    let label = job
        .ch
        .as_ref()
        .map_or_else(|| channel.to_string(), |c| c.label.clone());
    let when = msg.str_at(&["timestamp"]).unwrap_or("");
    let key = if ok {
        "rmcord-log-deleted"
    } else {
        "rmcord-log-failed"
    };
    let text = t(
        host,
        key,
        &[("channel", &label), ("when", when), ("content", &content)],
    );
    if ok {
        job.progress.deleted += 1;
    } else {
        job.progress.failed += 1;
    }
    note(host, job, text);
}

/// One step of the engine: the script's loops from where the last timer left off until the next
/// `sleep` (which becomes a timer) or the end.
fn step(host: &mut dyn Host, st: &mut State) {
    let State {
        guilds,
        users,
        dms,
        job,
        ..
    } = st;
    let Some(job) = job.as_mut() else { return };
    if job.finished {
        return;
    }
    loop {
        match job.next {
            Next::Channel => {
                if job.stopped || job.ci >= job.channels.len() {
                    finish(host, job);
                    return;
                }
                if job.paused {
                    again(host, job, 500.0);
                    return;
                }
                let id = job.channels[job.ci];
                job.ci += 1;
                let Some((guild, label)) = lookup(host, (guilds, users, dms), id) else {
                    let text = t(host, "rmcord-log-not-found", &[("id", &id.to_string())]);
                    note(host, job, text);
                    continue;
                };
                job.progress.current = Some(label.clone());
                job.progress.eta = None;
                let text = t(host, "rmcord-log-searching", &[("label", &label)]);
                note(host, job, text);
                job.ch = Some(ChanRun {
                    id,
                    guild,
                    label,
                    offset: 0,
                    total_counted: false,
                    total: 0,
                    deleted: 0,
                    start: Instant::now(),
                    queue: VecDeque::new(),
                    cur: None,
                });
                job.next = Next::Search;
            }
            Next::Search => {
                if job.stopped {
                    end_channel(job);
                    job.next = Next::Channel;
                    continue;
                }
                if job.paused {
                    again(host, job, 500.0);
                    return;
                }
                let Some(ch) = job.ch.as_ref() else {
                    job.next = Next::Channel;
                    continue;
                };
                let label = ch.label.clone();
                let route = search_route(ch, &job.user, &job.filters);
                let r = call(host, "GET", &route);
                if r.status == Some(429) {
                    let wait = wait_ms(&r);
                    job.delays.search += wait;
                    job.delays.since_throttle = 0;
                    let text = t(
                        host,
                        "rmcord-log-search-limited",
                        &[
                            ("wait", &format!("{:.1}", wait / 1000.0)),
                            ("delay", &js_num(job.delays.search)),
                        ],
                    );
                    note(host, job, text);
                    again(host, job, wait * 2.0);
                    return;
                }
                if r.status == Some(202) {
                    let text = t(host, "rmcord-log-not-indexed", &[]);
                    note(host, job, text);
                    again(host, job, 5000.0);
                    return;
                }
                if !is_ok(&r) {
                    let status = status_text(r.status, host);
                    let text = t(
                        host,
                        "rmcord-log-search-error",
                        &[("status", &status), ("label", &label)],
                    );
                    note(host, job, text);
                    end_channel(job);
                    job.next = Next::Channel;
                    continue;
                }
                let data = r.body.unwrap_or(Val::Null);
                let total = data.get("total_results").and_then(Val::as_i64).unwrap_or(0);
                let groups = data.get("messages").map(Val::as_list).unwrap_or(&[]);
                if groups.is_empty() {
                    if total > 0 {
                        let text = t(
                            host,
                            "rmcord-log-index-updating",
                            &[("total", &total.to_string())],
                        );
                        note(host, job, text);
                        let delay = job.delays.search;
                        again(host, job, delay);
                        return;
                    }
                    let text = t(host, "rmcord-log-no-more", &[("label", &label)]);
                    note(host, job, text);
                    end_channel(job);
                    job.next = Next::Channel;
                    continue;
                }
                if let Some(ch) = job.ch.as_mut()
                    && !ch.total_counted
                {
                    ch.total = total;
                    job.progress.total += total;
                    ch.total_counted = true;
                }
                let hits: Vec<&Val> = groups
                    .iter()
                    .filter_map(|g| {
                        g.as_list()
                            .iter()
                            .find(|m| m.get("hit").and_then(Val::as_bool) == Some(true))
                    })
                    .collect();
                let filtered: Vec<Val> = hits
                    .iter()
                    .filter(|m| {
                        let ty = m.get("type").and_then(Val::as_i64).unwrap_or(-1);
                        ty == 0 || (6..=21).contains(&ty)
                    })
                    .filter(|m| matches_filters(m, &job.filters))
                    .map(|m| (*m).clone())
                    .collect();
                let skipped = (hits.len() - filtered.len()) as i64;
                if skipped > 0 {
                    if let Some(ch) = job.ch.as_mut() {
                        ch.offset += skipped;
                        ch.total -= skipped;
                    }
                    job.progress.total = (job.progress.total - skipped).max(0);
                }
                if filtered.is_empty() {
                    if skipped > 0 {
                        let text = t(
                            host,
                            "rmcord-log-skipped",
                            &[("count", &skipped.to_string())],
                        );
                        note(host, job, text);
                        let delay = job.delays.search;
                        again(host, job, delay);
                        return;
                    }
                    end_channel(job);
                    job.next = Next::Channel;
                    continue;
                }
                if let Some(ch) = job.ch.as_mut() {
                    ch.queue = filtered.into();
                }
                job.next = Next::Delete;
            }
            Next::Delete => {
                let Some(ch) = job.ch.as_mut() else {
                    job.next = Next::Channel;
                    continue;
                };
                if ch.cur.is_none() {
                    if job.stopped {
                        ch.queue.clear();
                    }
                    if ch.queue.is_empty() {
                        maybe_recover(&mut job.delays);
                        job.next = Next::Search;
                        let delay = job.delays.search;
                        again(host, job, delay);
                        return;
                    }
                    if job.paused {
                        again(host, job, 500.0);
                        return;
                    }
                    ch.cur = ch.queue.pop_front().map(|m| (m, 0));
                }
                let Some((msg, attempt)) = ch.cur.clone() else {
                    continue;
                };
                if attempt >= MAX_RETRY {
                    delete_done(host, job, &msg, false, false);
                    return;
                }
                let channel = msg.id_at(&["channel_id"]).unwrap_or(ch.id);
                let Some(mid) = msg.str_at(&["id"]) else {
                    // A hit without an id cannot be deleted; the script would send "undefined".
                    delete_done(host, job, &msg, false, false);
                    return;
                };
                let route = format!("/channels/{channel}/messages/{mid}");
                let r = call(host, "DELETE", &route);
                if is_ok(&r) {
                    delete_done(host, job, &msg, true, false);
                    return;
                }
                let code = r
                    .body
                    .as_ref()
                    .and_then(|b| b.get("code"))
                    .and_then(Val::as_i64);
                if r.status == Some(400) && code == Some(50083) {
                    let text = t(host, "rmcord-log-archived", &[]);
                    note(host, job, text);
                    delete_done(host, job, &msg, false, true);
                    return;
                }
                if let Some(c) = job.ch.as_mut().and_then(|c| c.cur.as_mut()) {
                    c.1 += 1;
                }
                if r.status == Some(429) {
                    let wait = wait_ms(&r);
                    job.delays.delete = job.delays.delete.max(wait);
                    job.delays.since_throttle = 0;
                    let text = t(
                        host,
                        "rmcord-log-delete-limited",
                        &[
                            ("wait", &format!("{:.1}", wait / 1000.0)),
                            ("delay", &js_num(job.delays.delete)),
                        ],
                    );
                    note(host, job, text);
                    again(host, job, wait * 2.0);
                    return;
                }
                if attempt < MAX_RETRY - 1 {
                    let status = status_text(r.status, host);
                    let text = t(
                        host,
                        "rmcord-log-delete-retry",
                        &[
                            ("status", &status),
                            ("n", &(attempt + 1).to_string()),
                            ("max", &MAX_RETRY.to_string()),
                        ],
                    );
                    note(host, job, text);
                    let delay = job.delays.delete;
                    again(host, job, delay);
                    return;
                }
                delete_done(host, job, &msg, false, false);
                return;
            }
        }
    }
}

/// The end of `tryDeleteMessage` and what `deleteChannelMessages` does with its result, then the
/// `sleep(deleteDelay)`.
fn delete_done(host: &mut dyn Host, job: &mut Job, msg: &Val, ok: bool, archived: bool) {
    let channel = job.ch.as_ref().map_or(0, |c| c.id);
    let channel = msg.id_at(&["channel_id"]).unwrap_or(channel);
    if let Some(ch) = job.ch.as_mut() {
        ch.cur = None;
    }
    if archived {
        if let Some(ch) = job.ch.as_mut() {
            ch.offset += 1;
            ch.total -= 1;
        }
        job.progress.total = (job.progress.total - 1).max(0);
    } else {
        if ok && let Some(ch) = job.ch.as_mut() {
            ch.deleted += 1;
        }
        message_log(host, job, ok, msg, channel);
        if let Some(ch) = job.ch.as_ref()
            && ch.deleted > 0
        {
            let elapsed = ch.start.elapsed().as_millis() as f64;
            let remaining = (ch.total - ch.deleted).max(0) as f64;
            let eta = elapsed / ch.deleted as f64 * remaining;
            job.progress.eta = Some(format_eta(eta));
        }
    }
    let delay = job.delays.delete;
    again(host, job, delay);
}

// ---------------------------------------------------------------------------------------------
// The scheduled run
// ---------------------------------------------------------------------------------------------

fn start_job(st: &mut State, user: String, channels: Vec<u64>, filters: Filters) {
    st.job = Some(Job::new(user, channels, filters));
}

fn sched_tick(host: &mut dyn Host, st: &mut State) {
    let Some(s) = st.sched.as_ref() else { return };
    let left = s.at - now_ms();
    if left > 0 {
        // Timers last 24 h at most; the rest of a longer wait is another timer.
        if let Err(e) = host.schedule(left.min(DAY_MS) as u64, SCHED) {
            host.log(&format!("rmcord: could not keep the schedule: {e}"));
            st.sched = None;
        }
        return;
    }
    let Some(s) = st.sched.take() else { return };
    if st.job.as_ref().is_some_and(Job::running) {
        host.log(&t(host, "rmcord-sched-busy", &[]));
        return;
    }
    let Some(user) = host.self_id().map(|u| u.to_string()) else {
        host.log(&t(host, "rmcord-not-signed-in", &[]));
        return;
    };
    start_job(st, user, s.channels, s.filters);
    step(host, st);
}

// ---------------------------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------------------------

fn text_of(v: &Val) -> Option<String> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

/// `(subcommand, options)` of a `/phoecord <sub>` interaction.
fn parse_interaction(body: &Val) -> Option<(String, Vec<(String, String)>)> {
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
    Some((name, options))
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

fn parse_channels(text: &str) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for tok in text.split(|c: char| c == ',' || c.is_whitespace()) {
        let tok = tok.trim_start_matches("<#").trim_end_matches('>');
        if let Ok(id) = tok.parse::<u64>()
            && !out.contains(&id)
        {
            out.push(id);
        }
    }
    out
}

/// The panel's filters from the command's options; `Err` is the text to show.
fn parse_filters(host: &dyn Host, options: &[(String, String)]) -> Result<(Filters, bool), String> {
    let mut f = Filters::default();
    for (name, slot) in [("from", 0), ("to", 1)] {
        if let Some(v) = opt(options, name) {
            let Some(ms) = parse_date(v) else {
                return Err(t(host, "rmcord-bad-date", &[("value", v)]));
            };
            if slot == 0 {
                f.min_id = Some(date_to_snowflake(ms));
            } else {
                f.max_id = Some(date_to_snowflake(ms));
            }
        }
    }
    if let Some(list) = opt(options, "types") {
        f.types = [false; 8];
        for tok in list.split(|c: char| c == ',' || c.is_whitespace()) {
            let tok = tok.trim().to_lowercase();
            if tok.is_empty() {
                continue;
            }
            let Some(i) = TYPES.iter().position(|t| *t == tok) else {
                return Err(t(host, "rmcord-bad-type", &[("type", &tok)]));
            };
            f.types[i] = true;
        }
        if f.types.iter().all(|t| !*t) {
            f.types = [true; 8];
        }
    }
    match opt(options, "mode").map(str::to_lowercase).as_deref() {
        None | Some("include") => {}
        Some("exclude") => f.include = false,
        Some(other) => return Err(t(host, "rmcord-bad-mode", &[("mode", other)])),
    }
    let mut ignored = false;
    if let Some(src) = opt(options, "regex") {
        match Regex::new(src) {
            Ok(re) => f.regex = Some(re),
            Err(_) => ignored = true,
        }
    }
    Ok((f, ignored))
}

fn status_reply(host: &dyn Host, st: &State) -> String {
    let mut out = Vec::new();
    match &st.job {
        None => out.push(t(host, "rmcord-status-idle", &[])),
        Some(j) => {
            let p = &j.progress;
            let state = if j.finished {
                if j.stopped {
                    "rmcord-state-stopped"
                } else {
                    "rmcord-state-done"
                }
            } else if j.paused {
                "rmcord-state-paused"
            } else {
                "rmcord-state-running"
            };
            out.push(t(
                host,
                "rmcord-status",
                &[
                    ("state", &t(host, state, &[])),
                    ("deleted", &p.deleted.to_string()),
                    ("total", &p.total.to_string()),
                    ("failed", &p.failed.to_string()),
                    ("search", &js_num(j.delays.search)),
                    ("delete", &js_num(j.delays.delete)),
                ],
            ));
            if let Some(c) = &p.current {
                out.push(t(host, "rmcord-status-channel", &[("channel", c)]));
            }
            if let Some(e) = &p.eta {
                out.push(t(host, "rmcord-status-eta", &[("eta", e)]));
            }
        }
    }
    if let Some(s) = &st.sched {
        out.push(t(
            host,
            "rmcord-status-scheduled",
            &[
                ("when", &utc_text(s.at)),
                ("count", &s.channels.len().to_string()),
            ],
        ));
    }
    out.join("\n")
}

/// `YYYY-MM-DD HH:MM UTC`.
fn utc_text(ms: i64) -> String {
    let days = ms.div_euclid(DAY_MS);
    let rem = ms.rem_euclid(DAY_MS) / 1000;
    // civil_from_days
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
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}

fn handle(host: &mut dyn Host, st: &mut State, options: &[(String, String)]) -> String {
    let action = opt(options, "action")
        .map(str::to_lowercase)
        .unwrap_or_else(|| "help".into());
    match action.as_str() {
        "start" | "schedule" => {
            let Some(user) = host.self_id().map(|u| u.to_string()) else {
                return t(host, "rmcord-not-signed-in", &[]);
            };
            let channels = opt(options, "channels")
                .map(parse_channels)
                .unwrap_or_default();
            if channels.is_empty() {
                return t(host, "rmcord-no-channels", &[]);
            }
            let (filters, regex_ignored) = match parse_filters(host, options) {
                Ok(f) => f,
                Err(e) => return e,
            };
            let mut out = if action == "start" {
                if st.job.as_ref().is_some_and(Job::running) {
                    return t(host, "rmcord-already-running", &[]);
                }
                start_job(st, user, channels.clone(), filters);
                // The first step: the least a timer can wait.
                if let Err(e) = host.schedule(50, STEP) {
                    st.job = None;
                    return t(host, "rmcord-schedule-failed", &[("error", &e)]);
                }
                t(
                    host,
                    "rmcord-started",
                    &[("count", &channels.len().to_string())],
                )
            } else {
                let Some(at) = opt(options, "at") else {
                    return t(host, "rmcord-need-at", &[]);
                };
                let Some(at) = parse_date(at) else {
                    return t(host, "rmcord-bad-date", &[("value", at)]);
                };
                let left = at - now_ms();
                if left <= 0 {
                    return t(host, "rmcord-schedule-past", &[]);
                }
                host.cancel_schedule(SCHED);
                if let Err(e) = host.schedule(left.min(DAY_MS) as u64, SCHED) {
                    return t(host, "rmcord-schedule-failed", &[("error", &e)]);
                }
                st.sched = Some(Sched {
                    at,
                    channels: channels.clone(),
                    filters,
                });
                t(host, "rmcord-scheduled", &[("when", &utc_text(at))])
            };
            if regex_ignored {
                out.push('\n');
                out.push_str(&t(host, "rmcord-regex-ignored", &[]));
            }
            out.push('\n');
            out.push_str(&t(host, "rmcord-warning", &[]));
            out
        }
        "cancel" => {
            if st.sched.take().is_some() {
                host.cancel_schedule(SCHED);
                t(host, "rmcord-schedule-cancelled", &[])
            } else {
                t(host, "rmcord-nothing-scheduled", &[])
            }
        }
        "pause" | "resume" | "stop" => {
            let Some(job) = st.job.as_mut().filter(|j| j.running()) else {
                return t(host, "rmcord-nothing-running", &[]);
            };
            match action.as_str() {
                "pause" => {
                    job.paused = true;
                    job.progress.paused = true;
                    let text = t(host, "rmcord-log-paused", &[]);
                    note(host, job, text);
                    t(host, "rmcord-paused", &[])
                }
                "resume" => {
                    job.paused = false;
                    job.progress.paused = false;
                    let text = t(host, "rmcord-log-resumed", &[]);
                    note(host, job, text);
                    t(host, "rmcord-resumed", &[])
                }
                _ => {
                    job.stopped = true;
                    job.paused = false;
                    job.progress.stopped = true;
                    job.progress.paused = false;
                    job.progress.eta = None;
                    job.progress.current = None;
                    t(host, "rmcord-stopped", &[])
                }
            }
        }
        "status" => status_reply(host, st),
        "log" => match st.job.as_ref().filter(|j| !j.progress.logs.is_empty()) {
            None => t(host, "rmcord-log-empty", &[]),
            Some(j) => {
                let logs = &j.progress.logs;
                let skip = logs.len().saturating_sub(LOG_SHOWN);
                logs.iter()
                    .skip(skip)
                    .map(|l| {
                        let mut line: String = l.chars().take(160).collect();
                        if l.chars().count() > 160 {
                            line.push('…');
                        }
                        line
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        },
        "help" => format!(
            "{}\n{}",
            t(host, "rmcord-help", &[]),
            t(host, "rmcord-warning", &[])
        ),
        other => t(host, "rmcord-bad-action", &[("action", other)]),
    }
}

// ---------------------------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------------------------

pub struct Rmcord {
    manifest: Manifest,
    state: Shared<State>,
}

impl Rmcord {
    pub fn new() -> Rmcord {
        Rmcord {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "rmcord",
                    env!("CARGO_PKG_VERSION"),
                    "Bulk delete your own Discord messages (search, filters, schedule)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("rmcord".into());
                m
            },
            state: Shared::new(State::default()),
        }
    }

    /// `/phoecord rmcord <action> ...`. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "rmcord",
            "Delete your own messages in bulk. Risk: can get your account banned. See action help",
            move |host: &mut dyn Host, _: &Args| {
                state
                    .lock()
                    .reply
                    .take()
                    .unwrap_or_else(|| host.t("rmcord-no-reply", &[]))
            },
        )
        .option(
            "action",
            "start, schedule, cancel, pause, resume, stop, status, log or help",
            true,
        )
        .option(
            "channels",
            "start and schedule: channel ids, comma or space separated",
            false,
        )
        .option(
            "from",
            "Only messages after this date: YYYY-MM-DDTHH:MM (local time) or YYYY-MM-DD",
            false,
        )
        .option(
            "to",
            "Only messages before this date: YYYY-MM-DDTHH:MM (local time) or YYYY-MM-DD",
            false,
        )
        .option(
            "types",
            "Message types: text, image, video, link, embed, sound, file, sticker (default all)",
            false,
        )
        .option(
            "regex",
            "Only messages whose text matches this regular expression",
            false,
        )
        .option(
            "mode",
            "include (default): delete matches of regex; exclude: delete the rest",
            false,
        )
        .option(
            "at",
            "schedule: when to start, YYYY-MM-DDTHH:MM (local time)",
            false,
        )
        .boxed()
    }
}

impl Default for Rmcord {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Rmcord {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Dispatch { name } => *name == TIMER_EVENT || is_one_of(name, EVENTS),
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
                if let Some((sub, options)) = parse_interaction(body)
                    && sub == "rmcord"
                {
                    let mut st = self.state.lock();
                    let reply = handle(host, &mut st, &options);
                    st.reply = Some(reply);
                }
                Verdict::Pass
            }
            Event::Dispatch { name, payload, .. } => {
                if name == TIMER_EVENT {
                    let tag = payload
                        .str_at(&["d", "tag"])
                        .or_else(|| payload.str_at(&["tag"]))
                        .unwrap_or("")
                        .to_string();
                    let mut st = self.state.lock();
                    match tag.as_str() {
                        STEP => step(host, &mut st),
                        SCHED => sched_tick(host, &mut st),
                        _ => {}
                    }
                } else {
                    let d = payload.get("d").unwrap_or(&*payload);
                    self.state.lock().observe(name, d);
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

    const READY: &str = r#"{"op":0,"t":"READY","d":{"user":{"id":"5","username":"me"},
        "users":[{"id":"8","username":"bob","global_name":"Bob"}],
        "guilds":[{"id":"7","properties":{"name":"Home"},"channels":[{"id":"9","name":"general","type":0}]}],
        "private_channels":[{"id":"20","type":1,"recipient_ids":["8"]}]}}"#;

    fn plugin() -> Rmcord {
        let p = Rmcord::new();
        let mut h = Rec::default();
        let mut f = j(READY);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "READY",
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
        p
    }

    fn host() -> Rec {
        Rec {
            self_id: Some(5),
            ..Rec::default()
        }
    }

    /// Run `/phoecord rmcord` with these options and return the reply text.
    fn run(p: &Rmcord, h: &mut Rec, opts: &[(&str, &str)]) -> String {
        let options: Vec<String> = opts
            .iter()
            .map(|(k, v)| format!(r#"{{"type":3,"name":"{k}","value":"{v}"}}"#))
            .collect();
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"rmcord","options":[{}]}}]}}}}"#,
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
        p.command().run(
            h,
            &Args {
                options: Vec::new(),
            },
        )
    }

    fn tick(p: &Rmcord, h: &mut Rec, tag: &str) {
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

    const SEARCH: &str = "GET /guilds/7/messages/search?author_id=5&sort_by=timestamp&sort_order=desc&offset=0&channel_id=9";

    fn page(msgs: &str, total: i64) -> String {
        format!(r#"{{"total_results":{total},"messages":[{msgs}]}}"#)
    }

    fn hit(id: &str, content: &str) -> String {
        format!(
            r#"[{{"id":"{id}","channel_id":"9","type":0,"hit":true,"content":"{content}","attachments":[],"embeds":[]}}]"#
        )
    }

    fn answer(h: Rec, key: &str, status: u16, body: &str) -> Rec {
        let (m, r) = key.split_once(' ').unwrap();
        h.answer(m, r, status, body)
    }

    #[test]
    fn start_searches_deletes_one_by_one_and_ends() {
        let p = plugin();
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer("DELETE", "/channels/9/messages/100", 204, "null");
        let r = run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        assert!(
            r.contains("Started deleting") && r.contains("Warning"),
            "{r}"
        );
        assert_eq!(h.scheduled, vec![(50, "step".to_string())]);
        tick(&p, &mut h, "step");
        let calls: Vec<_> = h.rest_calls.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(calls, vec![SEARCH, "DELETE /channels/9/messages/100"]);
        assert_eq!(h.scheduled.last(), Some(&(600, "step".to_string())));
        // The loop's second half: recover delays, wait one search delay.
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.last(), Some(&(10_000, "step".to_string())));
        // The next search finds nothing: the channel, then the job, is done.
        h.rest_answers.clear();
        let calls_before = h.rest_calls.len();
        let h2 = answer(
            std::mem::take(&mut h),
            "GET /guilds/7/messages/search?author_id=5&sort_by=timestamp&sort_order=desc&offset=0&channel_id=9",
            200,
            r#"{"total_results":0,"messages":[]}"#,
        );
        h = h2;
        h.rest_calls.truncate(calls_before);
        let n = h.scheduled.len();
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.len(), n, "nothing more to wait for");
        let st = status(&p, &mut h);
        assert!(
            st.contains("is done") && st.contains("deleted 1 of 1"),
            "{st}"
        );
        assert!(h.logs.iter().any(|l| l.contains("rmcord:")));
    }

    fn status(p: &Rmcord, h: &mut Rec) -> String {
        run(p, h, &[("action", "status")])
    }

    #[test]
    fn dm_search_uses_the_channel_route_and_date_filters() {
        let p = plugin();
        let mut h = host();
        run(
            &p,
            &mut h,
            &[
                ("action", "start"),
                ("channels", "20"),
                ("from", "2024-01-01"),
                ("to", "2024-02-01"),
            ],
        );
        tick(&p, &mut h, "step");
        let min = date_to_snowflake(1_704_067_200_000);
        let max = date_to_snowflake(1_706_745_600_000);
        assert_eq!(
            h.rest_calls[0].0,
            format!(
                "GET /channels/20/messages/search?author_id=5&sort_by=timestamp&sort_order=desc&offset=0&min_id={min}&max_id={max}"
            )
        );
        assert_eq!(min, "1191168914227200000");
        assert_eq!(max, "1202402938060800000");
    }

    #[test]
    fn rate_limits_raise_the_delays_and_wait_double() {
        let p = plugin();
        let mut h = answer(host(), SEARCH, 429, r#"{"retry_after":1.5}"#);
        run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.last(), Some(&(3000, "step".to_string())));
        // A delete 429 raises the delete delay to retry_after and waits twice that.
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer(
            "DELETE",
            "/channels/9/messages/100",
            429,
            r#"{"retry_after":2}"#,
        );
        let p = plugin();
        run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.last(), Some(&(4000, "step".to_string())));
        assert!(status(&p, &mut h).contains("2000"));
        // A local refusal of the host limiter is handled the same way.
        struct Limited(Rec);
        impl Host for Limited {
            fn setting(&self, k: &str) -> Option<Val> {
                self.0.setting(k)
            }
            fn kv_get(&self, k: &str) -> Option<Val> {
                self.0.kv_get(k)
            }
            fn kv_set(&mut self, k: &str, v: Val) {
                self.0.kv_set(k, v)
            }
            fn self_id(&self) -> Option<u64> {
                Some(5)
            }
            fn notify(&mut self, a: &str, b: &str) {
                self.0.notify(a, b)
            }
            fn log(&mut self, m: &str) {
                self.0.log(m)
            }
            fn emit_send(&mut self, o: i64, p: Val) {
                self.0.emit_send(o, p)
            }
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
                    "rate limited: at most 5 Discord API calls per 5 s per plugin, retry in 2.5s"
                        .into(),
                )
            }
        }

        impl phoecord_core::ChainHost for Limited {}
        let p = plugin();
        let mut l = Limited(host());
        run(&p, &mut l.0, &[("action", "start"), ("channels", "9")]);
        let mut f = Rec::timer_frame("step");
        p.on_event(
            &mut l,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
        assert_eq!(l.0.scheduled.last(), Some(&(5000, "step".to_string())));
    }

    #[test]
    fn filters_skip_hits_and_move_the_offset() {
        let p = plugin();
        let msgs = format!("{},{}", hit("100", "hello"), hit("101", "see https://x.y"));
        let mut h = answer(host(), SEARCH, 200, &page(&msgs, 2));
        h = h.answer("DELETE", "/channels/9/messages/101", 204, "null");
        run(
            &p,
            &mut h,
            &[("action", "start"), ("channels", "9"), ("types", "link")],
        );
        tick(&p, &mut h, "step");
        let calls: Vec<_> = h.rest_calls.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(calls, vec![SEARCH, "DELETE /channels/9/messages/101"]);
        assert!(
            status(&p, &mut h).contains("deleted 1 of 1"),
            "one hit was skipped, one deleted"
        );
        // Only filtered hits on the page: skip, wait one search delay, search from the new offset.
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hello"), 1));
        let p = plugin();
        run(
            &p,
            &mut h,
            &[("action", "start"), ("channels", "9"), ("regex", "zzz")],
        );
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.last(), Some(&(10_000, "step".to_string())));
        tick(&p, &mut h, "step");
        assert!(h.rest_calls.last().unwrap().0.contains("offset=1"));
        // exclude mode keeps what does not match.
        let f = Filters {
            regex: Regex::new("zzz").ok(),
            include: false,
            ..Filters::default()
        };
        assert!(matches_filters(&j(r#"{"content":"hello"}"#), &f));
    }

    #[test]
    fn archived_threads_skip_and_other_errors_retry_three_times() {
        let p = plugin();
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer(
            "DELETE",
            "/channels/9/messages/100",
            400,
            r#"{"code":50083}"#,
        );
        run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        tick(&p, &mut h, "step");
        assert_eq!(h.scheduled.last(), Some(&(600, "step".to_string())));
        assert!(status(&p, &mut h).contains("deleted 0 of 0, failed 0"));
        let p = plugin();
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer("DELETE", "/channels/9/messages/100", 500, "null");
        run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        tick(&p, &mut h, "step");
        tick(&p, &mut h, "step");
        tick(&p, &mut h, "step");
        let deletes = h
            .rest_calls
            .iter()
            .filter(|c| c.0.starts_with("DELETE"))
            .count();
        assert_eq!(deletes, 3);
        assert_eq!(h.scheduled.last(), Some(&(600, "step".to_string())));
    }

    #[test]
    fn delays_recover_after_five_loops() {
        let mut d = Delays {
            search: 20_000.0,
            delete: 1000.0,
            since_throttle: 0,
        };
        for _ in 0..4 {
            maybe_recover(&mut d);
        }
        assert_eq!((d.search, d.delete), (20_000.0, 1000.0));
        maybe_recover(&mut d);
        assert_eq!((d.search, d.delete), (14_000.0, 700.0));
        for _ in 0..5 {
            maybe_recover(&mut d);
        }
        assert_eq!((d.search, d.delete), (10_000.0, 600.0));
    }

    #[test]
    fn pause_polls_and_stop_ends_the_job() {
        let p = plugin();
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer("DELETE", "/channels/9/messages/100", 204, "null");
        run(&p, &mut h, &[("action", "start"), ("channels", "9")]);
        run(&p, &mut h, &[("action", "pause")]);
        tick(&p, &mut h, "step");
        assert!(h.rest_calls.is_empty());
        assert_eq!(h.scheduled.last(), Some(&(500, "step".to_string())));
        run(&p, &mut h, &[("action", "resume")]);
        run(&p, &mut h, &[("action", "stop")]);
        tick(&p, &mut h, "step");
        assert!(h.rest_calls.is_empty());
        assert!(h.logs.iter().any(|l| l.contains("Stopped by user.")));
        assert!(run(&p, &mut h, &[("action", "stop")]).contains("No deletion is running"));
    }

    #[test]
    fn schedule_waits_in_day_steps_and_can_be_cancelled() {
        let p = plugin();
        let mut h = host();
        let r = run(
            &p,
            &mut h,
            &[
                ("action", "schedule"),
                ("channels", "9"),
                ("at", "2999-01-01T00:00Z"),
            ],
        );
        assert!(!r.is_empty());
        assert_eq!(h.scheduled, vec![(86_400_000, "sched".to_string())]);
        tick(&p, &mut h, "sched");
        assert_eq!(h.scheduled.last(), Some(&(86_400_000, "sched".to_string())));
        run(&p, &mut h, &[("action", "cancel")]);
        assert_eq!(h.cancelled.last(), Some(&"sched".to_string()));
        let n = h.scheduled.len();
        tick(&p, &mut h, "sched");
        assert_eq!(h.scheduled.len(), n);
        // A time in the past is refused.
        run(
            &p,
            &mut h,
            &[
                ("action", "schedule"),
                ("channels", "9"),
                ("at", "2001-01-01T00:00Z"),
            ],
        );
        assert_eq!(h.scheduled.len(), n);
    }

    #[test]
    fn a_due_schedule_starts_the_job() {
        let p = plugin();
        let mut h = answer(host(), SEARCH, 200, &page(&hit("100", "hi"), 1));
        h = h.answer("DELETE", "/channels/9/messages/100", 204, "null");
        {
            let mut st = p.state.lock();
            st.sched = Some(Sched {
                at: now_ms() - 1,
                channels: vec![9],
                filters: Filters::default(),
            });
        }
        tick(&p, &mut h, "sched");
        assert_eq!(h.rest_calls.len(), 2);
        assert_eq!(h.scheduled.last(), Some(&(600, "step".to_string())));
    }

    #[test]
    fn bad_input_is_refused_and_nothing_panics() {
        let p = plugin();
        let mut h = host();
        assert!(!run(&p, &mut h, &[("action", "start")]).is_empty());
        assert!(
            !run(
                &p,
                &mut h,
                &[("action", "start"), ("channels", "9"), ("from", "soon")]
            )
            .is_empty()
        );
        assert!(
            !run(
                &p,
                &mut h,
                &[("action", "start"), ("channels", "9"), ("types", "gif")]
            )
            .is_empty()
        );
        assert!(!run(&p, &mut h, &[("action", "nope")]).is_empty());
        assert!(!run(&p, &mut h, &[("action", "log")]).is_empty());
        assert!(h.scheduled.is_empty());
        // Unknown channel: skipped, the job ends without any REST call.
        run(&p, &mut h, &[("action", "start"), ("channels", "12345")]);
        tick(&p, &mut h, "step");
        assert!(h.rest_calls.is_empty());
        // Garbage frames and a timer with no job.
        let mut f = j(r#"{"op":0,"t":"PHOECORD_TIMER"}"#);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
        let mut no_user = Rec::default();
        assert!(!run(&p, &mut no_user, &[("action", "start"), ("channels", "9")]).is_empty());
    }

    #[test]
    fn dates_parse_like_the_script_and_types_match() {
        assert_eq!(parse_date("2024-01-01"), Some(1_704_067_200_000));
        assert_eq!(parse_date("2024-01-01T00:00:00Z"), Some(1_704_067_200_000));
        assert_eq!(
            parse_date("2024-01-01T02:00+02:00"),
            Some(1_704_067_200_000)
        );
        assert_eq!(parse_date("2024-02-30"), None);
        assert_eq!(parse_date("nope"), None);
        let all = [true; 8];
        assert!(matches_types(&j(r#"{}"#), &all));
        let mut only = [false; 8];
        only[1] = true; // image
        assert!(matches_types(
            &j(r#"{"attachments":[{"content_type":"image/png"}]}"#),
            &only
        ));
        assert!(!matches_types(&j(r#"{"content":"x"}"#), &only));
        assert_eq!(
            retry_in("rate limited: at most 5, retry in 1.5s"),
            Some(1.5)
        );
        assert_eq!(retry_in("boom"), None);
        assert_eq!(format_eta(61_000.0), "1m 1s");
    }

    #[test]
    fn the_manifest_and_wants_match_the_plan() {
        let p = Rmcord::new();
        assert_eq!(p.manifest().id, "rmcord");
        assert_eq!(p.manifest().vencord.as_deref(), Some("rmcord"));
        assert!(p.wants(&EventKind::Dispatch { name: TIMER_EVENT }));
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/interactions"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/users/@me"
        }));
        assert!(p.wants_gateway());
    }
}
