//! QuestMaster: auto-complete Discord Quests (owner's userplugin `QuestMaster`).
//!
//! Ported from the owner's userplugin `QuestMaster`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/QuestMaster, GPL-3.0-or-later,
//! Copyright (c) 2026 Nandak070), modified for PhoeCord 2026-11-25.
//!
//! **Risk, stated plainly.** This plugin fakes quest progress: it tells Discord you watched a
//! video, played a game, streamed or used an activity when you did not. That is inauthentic
//! engagement under Discord's Community Guidelines and Terms. Discord can void the quest, take
//! the reward back, or act against the whole account. The script ships no safeguard against
//! this and neither does the port. Use it at your own risk.
//!
//! Switching it on: `[plugins.quest_master] enabled = true` (off by default; it is one of the
//! owner's userplugins, not a default plugin). Controls (the dashboard is UI, Phase 2):
//! - `/phoecord quests list`: the quest picker (incomplete quests, type and reward),
//! - `/phoecord quests start` (picker's START with every quest checked) or
//!   `/phoecord quests start <quest id>` (only that quest),
//! - `/phoecord quests stop` (STOP), `/phoecord quests retry` (Retry: stop, reopen after 1.1 s),
//! - `/phoecord quests status` (the task cards), `/phoecord quests claim <quest id>` (CLAIM REWARD),
//! - `/phoecord quests consent <app id>` / `consent-always <app id>` / `deny`: the bypass's
//!   "Authorize application?" dialog (Authorize, "don't ask again this session", Cancel; 60 s).
//!
//! Progress and the script's log lines go to the PhoeCord log; with `fileLogging` every line,
//! debug included, is also appended to `data/plugins/quest_master/questmaster.log` (the script's
//! `Documents/QuestMaster/questmaster.log`). A completed quest raises a desktop notification, as
//! the script's `Notification` does; its toasts are notifications too.
//!
//! What is ported 1:1, as a timer-driven state machine (one step per timer, the script's delays):
//! - quest selection: incomplete, not expired, not the blacklisted quest, not skipped; task type
//!   detection (`detectType`, exact keys then fuzzy, then GAME by application id);
//! - the main loop: `Starting loop #n`, per-cycle queues, `concurrentGames` / `concurrentVideos`
//!   runners (`runConcurrent`: 1.5–4 s between launches, wait for a free slot), 4–6 s when nothing
//!   was queued, 2.5–4.5 s between cycles or 1–30 min with `randomDelay`;
//! - the request queue (`Traffic`): every quest POST goes through one FIFO, 1.2–1.8 s apart;
//!   429/5xx/408 are retried up to 3 times after `retry_after` (or 2^n s) plus 200–800 ms jitter,
//!   in place for a global limit, re-queued later otherwise. A local PhoeCord limiter refusal
//!   (`retry in X.Xs`) is handled as a 429 with `retry_after` X. Calls the script makes outside
//!   the queue (`GET /quests/@me`, `GET /applications/public`, `POST .../claim-reward`) wait
//!   `retry_after` and try again on a 429, as Discord's own API client does for the script;
//! - enrolling (`POST /quests/{id}/enroll` `{location: 11, is_targeted: false}`, 0.8–1.5 s pause),
//!   skipping on 403/404/410;
//! - VIDEO: `POST /quests/{id}/video-progress` `{timestamp}`, up to 7 s per step, never more than
//!   10 s ahead of the time since enrolment, 1 s ticks, re-enrol on 403/404, 5 failures, 25 min cap;
//! - GAME: `GET /applications/public`, then what the client puts on the wire while the script's
//!   fake game runs: a gateway op 3 presence with the game's activity (`application_id`, `name`,
//!   `type: 0`, `timestamps.start`; real "Playing" entries replaced, others kept; skipped with
//!   `hideActivity`), the client's later presences rewritten the same way, the last real presence
//!   sent again when the last fake game goes (`restore()`), and desktop heartbeats
//!   `POST /quests/{id}/heartbeat` `{application_id, terminal: false}` every 15–20 s;
//! - STREAM: heartbeats `{stream_key, terminal: false}` with your own stream's key in your current
//!   voice channel (`guild:{guild}:{channel}:{you}`, or `call:{channel}:{you}` in a DM call), from
//!   your op 4 sends and your `VOICE_STATE_UPDATE`s; the script's "join a VC with ≥1 person and
//!   stream any window" warning is logged;
//! - GAME / STREAM fallback: when the desktop shape is refused (400/403/404) or you are not in
//!   voice, the script's soft heartbeats `{stream_key: "call:{quest}:1" | "game:{app}"}`, rotating
//!   on 400/404; the closing `terminal: true` beat in the shape in use; 25 min cap; progress is
//!   also read from the client's own heartbeat answers (`QUESTS_SEND_HEARTBEAT_SUCCESS`);
//! - ACTIVITY and ACHIEVEMENT: heartbeats with `call:{channel}:{1000–9999}` every 19–22 s
//!   (channel: the most recent DM, else the first voice channel of the first guild that has one);
//! - the ACHIEVEMENT bypass (`bypassAchievement`, native.ts): numeric app id check,
//!   `GET /oauth2/tokens` snapshot, `GET /applications/public`, the consent prompt (notification
//!   plus the `consent` command, no answer in 60 s = no), `POST /oauth2/authorize?response_type=
//!   code&client_id={app}&scope=identify%20applications.commands%20applications.entitlements`
//!   with the script's body, `POST /applications/{app}/proxy-tickets` `{}`, then through
//!   `Host::fetch_with` (`fetch = ["*.discordsays.com"]`) `POST https://{app}.discordsays.com/
//!   .proxy/acf/authorize` `{code}` and `/.proxy/acf/quest/progress` `{progress: target}` with
//!   `Content-Type`, `X-Auth-Token`, `X-Discord-Quest-ID` and `Referer` exactly as native.ts sends
//!   them, and finally the grants the run created revoked (`DELETE /oauth2/tokens/{id}`), also
//!   when stopped mid-way;
//! - completion: notification, `autoClaim` after 2.5–6 s (`POST /quests/{id}/claim-reward`),
//!   captcha detection, the claim command for the rest;
//! - settings with the script's names and defaults: `fileLogging` (true), `concurrentGames` (1),
//!   `concurrentVideos` (3), `autoEnroll` (true), `autoClaim` (false), `playSound` (false),
//!   `randomDelay` (false), `hideActivity` (false), `autoStart` (true: open the picker, here a
//!   logged quest list, once the client is connected; nothing runs until `start`).
//!
//! Not ported: the completion sound (`playSound`), the dashboard, picker filters and the cosmetic
//! 1 s video ticker (UI, Phase 2); the bypass's local relay (127.0.0.1:43210) and DiscordNative
//! fallbacks (PhoeCord sends the native.ts request itself).
//!
//! Unverified against a live account (shapes taken from the script and the client's known
//! formats, checked only with synthetic payloads): the desktop heartbeat body
//! `{application_id, terminal}` and its 15–20 s cadence (the client's own cadence may differ);
//! whether Discord counts a STREAM heartbeat without a real Go Live stream; whether the server
//! needs the presence at all; the discordsays answers. Discord has been reported to suspend
//! quest access for automated completion.
//!   Reading the client's `QuestStore` becomes `GET /quests/@me` when QuestMaster opens and at the
//!   start of every cycle; between cycles the copy also learns from the client's own quest answers.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::Shared;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, TIMER_EVENT, Val, Verdict};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

mod bypass;
mod data;
mod spoof;
#[cfg(test)]
mod tests;

use data::*;
use spoof::FakeGame;

const PLUGIN_ID: &str = "quest_master";
const PLUGIN_VERSION: &str = "v2.0.0";
/// Known broken quest id from upstream QuestMaster.
const BLACKLISTED_QUEST_ID: &str = "1412491570820812933";
const MAX_TIME_MS: u64 = 25 * 60 * 1000;
const MAX_TASK_FAILURES: u32 = 5;
const MAX_RETRIES: u32 = 3;
const LOG_FILE: &str = "questmaster.log";
const LOG_PATH: &str = "data/plugins/quest_master/questmaster.log";
/// Timer that runs what a `/phoecord quests` command asked for, in this plugin's own scope.
const CMD_TAG: &str = "cmd";
const RETRYABLE: [u16; 6] = [429, 500, 502, 503, 504, 408];
const CLIENT_ERRORS: [u16; 5] = [400, 403, 404, 409, 410];
/// VIDEO: seconds per step, seconds ahead of real time allowed, pause between steps.
const VIDEO_SPEED: f64 = 7.0;
const VIDEO_MAX_FUTURE: i64 = 10;
const TICK_MS: u64 = 1000;
/// Dispatches that feed the channel lookup and `autoStart`.
const DISPATCHES: &[&str] = &[
    "READY",
    "GUILD_CREATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_DELETE",
    "VOICE_STATE_UPDATE",
];

/* ── errors (the script's ErrorHandler) ────────────────────────── */

/// A failed Discord call: an HTTP status with its body, or a local error.
#[derive(Clone, Debug)]
struct ApiErr {
    status: Option<u16>,
    body: Option<Val>,
    message: String,
}

impl ApiErr {
    fn retryable(&self) -> bool {
        self.status.is_some_and(|s| RETRYABLE.contains(&s))
    }
    fn client(&self) -> bool {
        self.status.is_some_and(|s| CLIENT_ERRORS.contains(&s))
    }
    /// `ErrorHandler.isSkippableQuest`.
    fn skippable(&self) -> bool {
        matches!(self.status, Some(403 | 404 | 410))
    }
    fn status_text(&self) -> String {
        self.status.map_or("?".into(), |s| s.to_string())
    }
    /// `classify(e).message`.
    fn msg(&self) -> String {
        let http = || {
            format!(
                "HTTP {}",
                self.status.map_or("UNKNOWN".into(), |s| s.to_string())
            )
        };
        let raw = self
            .body
            .as_ref()
            .and_then(|b| get(b, "message"))
            .and_then(text)
            .or_else(|| (!self.message.is_empty()).then(|| self.message.clone()))
            .unwrap_or_else(http);
        let s = sanitize(&raw, 180);
        if s.is_empty() { http() } else { s }
    }
    fn body_field(&self, key: &str) -> Option<&Val> {
        self.body.as_ref().and_then(|b| get(b, key))
    }
    /// A 429 (from Discord or the local limiter): how long to wait, in ms.
    fn rate_limited(&self) -> Option<u64> {
        (self.status == Some(429)).then(|| {
            self.body_field("retry_after")
                .and_then(num)
                .map_or(1000, |s| (s * 1000.0).max(0.0) as u64)
        })
    }
}

/// `Mods.API.<method>(...)`: a 2xx body, or the error the RestAPI would throw.
fn call(host: &mut dyn Host, method: &str, route: &str, body: Option<&Val>) -> Result<Val, ApiErr> {
    match host.rest(method, route, body) {
        Ok(r) if (200..300).contains(&r.status) => Ok(r.body.unwrap_or(Val::Null)),
        Ok(r) => Err(ApiErr {
            status: Some(r.status),
            message: r.text.unwrap_or_default(),
            body: r.body,
        }),
        Err(e) => match retry_in_ms(&e) {
            Some(ms) => Err(ApiErr {
                status: Some(429),
                body: Some(obj(&[
                    ("retry_after", Val::Float(ms as f64 / 1000.0)),
                    ("global", Val::Bool(false)),
                ])),
                message: e,
            }),
            None => Err(ApiErr {
                status: None,
                body: None,
                message: e,
            }),
        },
    }
}

/* ── engine state ──────────────────────────────────────────────── */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Not started (`engineActive` false).
    Idle,
    /// Reading the quests before the picker.
    Loading,
    /// The picker is open: waiting for `/phoecord quests start`.
    Picker,
    /// The main loop runs.
    Running,
    /// `main()` returned; the dashboard stays until stop or retry.
    Finished,
}

#[derive(Clone, Debug, PartialEq)]
enum Sel {
    All,
    One(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    Queue,
    Pending,
    Running,
    Completed,
    Claimed,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClaimState {
    None,
    Waiting,
    Failed,
}

/// One task card (`Logger.tasks`).
#[derive(Clone, Debug)]
struct Card {
    name: String,
    kind: Kind,
    cur: f64,
    max: f64,
    status: Status,
    claimable: bool,
    claim: ClaimState,
    /// `actionRequired: "ENROLL"`.
    enroll_required: bool,
    /// Insertion order, for a stable status listing.
    order: u64,
}

#[derive(Clone, Debug)]
struct Info {
    id: String,
    app_id: String,
    name: String,
    target: f64,
    kind: Kind,
    key: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lane {
    Game,
    Video,
}

impl Lane {
    fn idx(self) -> usize {
        match self {
            Lane::Game => 0,
            Lane::Video => 1,
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Lane::Game => "game",
            Lane::Video => "video",
        }
    }
}

/// What a task does when its timer is due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wake {
    /// After the pre-run enrol pause: re-read the quest and run its handler.
    Dispatch,
    /// VIDEO: after `ensureEnrolled`'s pause.
    VideoEnsured,
    /// VIDEO: after the re-enrol pause on a 403/404.
    VideoReenrolled,
    VideoIter,
    GenericFetch,
    GenericIter,
    AchIter,
    ActIter,
    /// ACHIEVEMENT bypass: a rate-limited step again, or the consent prompt timed out.
    Bypass,
}

/// What a queued request is for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Purpose {
    PreEnroll,
    VideoEnsure,
    VideoReenroll,
    VideoProgress(f64),
    VideoFinal,
    GenBeat,
    GenFinal,
    AchBeat,
    AchFinal,
    ActBeat,
    ActFinal,
}

/// A running task.
struct Run {
    info: Info,
    lane: Lane,
    /// `questRef` / `quest` of the script.
    quest: Val,
    /// The `userStatus` handed to the handler (`s`).
    status: Option<Val>,
    cur: f64,
    fails: u32,
    start: u64,
    enrolled_at: u64,
    /// `enrolledAt` synthesized when the store lags after an enrol.
    synth_enrolled: Option<u64>,
    completed: bool,
    /// Heartbeat `stream_key` (ACTIVITY / ACHIEVEMENT).
    key: String,
    soft_keys: Vec<String>,
    key_idx: usize,
    wake: Wake,
    /// ACHIEVEMENT returns `finish()`'s promise: the slot frees after the auto-claim.
    awaits_finish: bool,
    /// The ACHIEVEMENT bypass in progress.
    bypass: Option<bypass::Bypass>,
    /// GAME / STREAM: the client-shaped heartbeat was refused; soft heartbeats from now on.
    primary_rejected: bool,
    /// The heartbeat in flight is the client-shaped one.
    beat_primary: bool,
}

struct Queued {
    info: Info,
    quest: Val,
}

#[derive(Default)]
struct Runner {
    queue: VecDeque<Queued>,
    executing: BTreeSet<String>,
    limit: usize,
    /// Waiting in `Promise.race` for a free slot.
    waiting: bool,
    /// The launch loop ran out of tasks.
    loop_done: bool,
    finished: bool,
}

struct Req {
    id: u64,
    qid: String,
    purpose: Purpose,
    route: String,
    body: Val,
    attempts: u32,
}

/// The script's `Traffic` queue.
#[derive(Default)]
struct Traffic {
    queue: VecDeque<Req>,
    processing: bool,
    /// Requests waiting for their non-global retry timer.
    parked: HashMap<u64, Req>,
    next_id: u64,
}

/// A claim waiting for its timer: automatic after `finish()`, or the CLAIM REWARD button.
struct ClaimJob {
    name: String,
    manual: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    Start(Sel),
    Stop,
    Retry,
    Claim(String),
    Refresh,
    /// The consent prompt's Authorize (`remember`: "don't ask again this session") or Cancel.
    Consent {
        app: Option<String>,
        yes: bool,
        remember: bool,
    },
}

/// Everything QuestMaster knows, shared with its `/phoecord quests` command.
struct Engine {
    rng: Rng,
    /// Test clock; `None` = the system clock.
    clock: Option<u64>,
    session: u64,
    phase: Phase,
    /// `RUNTIME.running`.
    running: bool,
    auto_enroll: bool,
    auto_claim: bool,
    random_delay: bool,
    /// The QuestStore copy, in Discord's order.
    quests: Vec<Val>,
    /// Picker ids (in order) and the selection.
    picker: Vec<String>,
    selection: Option<Sel>,
    selected: BTreeSet<String>,
    skipped: HashSet<String>,
    cards: BTreeMap<String, Card>,
    card_seq: u64,
    loop_n: u64,
    runners: [Runner; 2],
    runs: HashMap<String, Run>,
    traffic: Traffic,
    claims: HashMap<String, ClaimJob>,
    pending: Vec<Cmd>,
    /// Session timers still pending, so `stop` can cancel them.
    tags: HashSet<String>,
    autostarted: bool,
    /// DMs as `(channel, last_message_id)`; guilds with their voice channels `(position, id)`.
    dms: Vec<(u64, u64)>,
    vocal: Vec<(u64, Vec<(i64, u64)>)>,
    /// The client's last presence (`d` of its op 3), restored when the fake games go.
    presence: Option<Val>,
    /// `Patcher.games`.
    fake_games: Vec<FakeGame>,
    /// Your voice channel: `(guild, channel)`.
    voice: Option<(Option<u64>, u64)>,
    /// `Consent._granted`: apps authorized without asking again (kept across stop/start).
    granted: HashSet<String>,
    /// The consent prompt waiting for an answer: `(quest, app)`.
    consent_wait: Option<(String, String)>,
}

impl Default for Engine {
    fn default() -> Engine {
        Engine {
            rng: Rng::seeded(),
            clock: None,
            session: 0,
            phase: Phase::Idle,
            running: false,
            auto_enroll: true,
            auto_claim: false,
            random_delay: false,
            quests: Vec::new(),
            picker: Vec::new(),
            selection: None,
            selected: BTreeSet::new(),
            skipped: HashSet::new(),
            cards: BTreeMap::new(),
            card_seq: 0,
            loop_n: 0,
            runners: [Runner::default(), Runner::default()],
            runs: HashMap::new(),
            traffic: Traffic::default(),
            claims: HashMap::new(),
            pending: Vec::new(),
            tags: HashSet::new(),
            autostarted: false,
            dms: Vec::new(),
            vocal: Vec::new(),
            presence: None,
            fake_games: Vec::new(),
            voice: None,
            granted: HashSet::new(),
            consent_wait: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lvl {
    Info,
    Success,
    Warn,
    Err,
    Debug,
}

impl Lvl {
    fn name(self) -> &'static str {
        match self {
            Lvl::Info => "INFO",
            Lvl::Success => "SUCCESS",
            Lvl::Warn => "WARN",
            Lvl::Err => "ERR",
            Lvl::Debug => "DEBUG",
        }
    }
}

impl Engine {
    fn now(&self) -> u64 {
        self.clock.unwrap_or_else(system_ms)
    }

    fn active(&self) -> bool {
        self.phase != Phase::Idle
    }

    /* ── logging (Logger.log + writeFileLog) ── */

    fn say(&mut self, host: &mut dyn Host, lvl: Lvl, key: &str, args: &[(&str, &str)]) {
        let msg = host.t(key, args);
        if lvl != Lvl::Debug {
            host.log(&msg);
        }
        self.file_log(host, lvl, &msg);
    }

    fn file_log(&self, host: &mut dyn Host, lvl: Lvl, msg: &str) {
        if !flag(host, "fileLogging", true) {
            return;
        }
        let mut safe = sanitize(msg, 500);
        if safe.is_empty() {
            safe = msg.chars().take(500).collect();
        }
        let line = format!("[{}] [{}] {}", fmt_iso(self.now()), lvl.name(), safe);
        // The script only reports a failed write to the console.
        let _ = host.append_data(LOG_FILE, &line);
    }

    /* ── timers ── */

    fn after(&mut self, host: &mut dyn Host, ms: u64, kind: &str, arg: &str) {
        let tag = format!("{}|{kind}|{arg}", self.session);
        match host.schedule(ms, &tag) {
            Ok(()) => {
                self.tags.insert(tag);
            }
            Err(e) => self.say(host, Lvl::Err, "qm-timer-failed", &[("error", &e)]),
        }
    }

    fn on_timer(&mut self, host: &mut dyn Host, tag: &str) {
        if tag == CMD_TAG {
            self.run_cmds(host);
            return;
        }
        self.tags.remove(tag);
        let mut parts = tag.splitn(3, '|');
        let (Some(session), Some(kind), Some(arg)) = (parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        if session.parse::<u64>().ok() != Some(self.session) {
            return;
        }
        let arg = arg.to_string();
        match kind {
            "load" => self.load(host),
            "open" => {
                self.open(host, None);
            }
            "cyc" => match arg.as_str() {
                "fetch" => self.cycle_fetch(host),
                "post" => self.cycle_post(host),
                _ => {
                    self.loop_n += 1;
                    self.cycle_begin(host);
                }
            },
            "run" => {
                let lane = if arg == "video" {
                    Lane::Video
                } else {
                    Lane::Game
                };
                self.runner_wake(host, lane);
            }
            "task" => self.task_wake(host, &arg),
            "traffic" => self.traffic_next(host),
            "rt" => self.traffic_retry(host, &arg),
            "claim" => self.claim_wake(host, &arg),
            "refresh" => self.refresh(host),
            "rm" => {
                self.cards.remove(&arg);
            }
            _ => {}
        }
    }

    /* ── commands ── */

    fn run_cmds(&mut self, host: &mut dyn Host) {
        for cmd in std::mem::take(&mut self.pending) {
            match cmd {
                Cmd::Start(sel) => match self.phase {
                    Phase::Idle => {
                        self.open(host, Some(sel));
                    }
                    Phase::Picker => self.choose(host, sel),
                    // Loading: the selection waits for the picker.
                    Phase::Loading => self.selection = Some(sel),
                    _ => {}
                },
                Cmd::Stop => self.stop(host),
                Cmd::Retry => {
                    // restartQuestMaster: stop, start again after 1.1 s.
                    self.stop(host);
                    self.after(host, 1100, "open", "");
                }
                Cmd::Claim(id) => {
                    self.claims.insert(
                        id.clone(),
                        ClaimJob {
                            name: self
                                .cards
                                .get(&id)
                                .map(|c| c.name.clone())
                                .unwrap_or_default(),
                            manual: true,
                        },
                    );
                    self.claim_wake_now(host, &id);
                }
                Cmd::Refresh => self.refresh(host),
                Cmd::Consent { app, yes, remember } => self.consent_cmd(host, app, yes, remember),
            }
        }
    }

    /// `GET /quests/@me` for `list`, outside a run.
    fn refresh(&mut self, host: &mut dyn Host) {
        match call(host, "GET", "/quests/@me", None) {
            Ok(body) => self.set_quests(&body),
            Err(e) => {
                if let Some(ms) = e.rate_limited() {
                    self.after(host, ms, "refresh", "");
                }
            }
        }
    }

    /* ── quest store copy ── */

    fn set_quests(&mut self, body: &Val) {
        let list = match get(body, "quests") {
            Some(l) => l.as_list(),
            None => body.as_list(),
        };
        self.quests = list.iter().filter(|q| q_id(q).is_some()).cloned().collect();
    }

    fn cached(&self, id: &str) -> Option<&Val> {
        self.quests.iter().find(|q| q_id(q).as_deref() == Some(id))
    }

    /// The client's own quest answer (enrol, heartbeat, video progress) updates the store.
    fn learn_status(&mut self, id: &str, status: &Val) {
        if !matches!(status, Val::Map(_)) {
            return;
        }
        if let Some(q) = self
            .quests
            .iter_mut()
            .find(|q| q_id(q).as_deref() == Some(id))
        {
            let key = if q.get("userStatus").is_some() {
                "userStatus"
            } else {
                "user_status"
            };
            q.set(key, status.clone());
        }
    }

    /// The quests `main()` starts from: incomplete, not expired, not blacklisted, not skipped.
    fn open_quests(&self) -> Vec<Val> {
        let now = self.now();
        self.quests
            .iter()
            .filter(|q| {
                let id = q_id(q).unwrap_or_default();
                !q_completed(q)
                    && not_expired(q, now)
                    && id != BLACKLISTED_QUEST_ID
                    && !self.skipped.contains(&id)
            })
            .cloned()
            .collect()
    }

    /// The picker's items: open quests with a known task type.
    fn picker_items(&self) -> Vec<(String, String, Kind, &'static str, String)> {
        self.open_quests()
            .iter()
            .filter_map(|q| {
                let cfg = q_task_cfg(q)?;
                get(cfg, "tasks")?;
                let (kind, _, _) = detect_type(cfg, q_app_id(q).as_deref())?;
                let (_, label, reward) = q_reward(q);
                Some((q_id(q)?, q_name(q), kind, label, reward))
            })
            .collect()
    }

    /* ── start / stop (startQuestMaster, main, stopQuestMaster) ── */

    fn open(&mut self, host: &mut dyn Host, sel: Option<Sel>) -> bool {
        if self.active() {
            return false;
        }
        self.session += 1;
        self.phase = Phase::Loading;
        self.running = true;
        self.auto_enroll = flag(host, "autoEnroll", true);
        self.auto_claim = flag(host, "autoClaim", false);
        self.random_delay = flag(host, "randomDelay", false);
        self.skipped.clear();
        self.reset_traffic();
        self.selection = sel;
        if flag(host, "fileLogging", true) {
            self.say(host, Lvl::Info, "qm-file-path", &[("path", LOG_PATH)]);
            let line = host.t("qm-session-start", &[("version", PLUGIN_VERSION)]);
            self.file_log(host, Lvl::Info, &line);
        }
        self.load(host);
        true
    }

    /// Read the quests and open the picker.
    fn load(&mut self, host: &mut dyn Host) {
        if self.phase != Phase::Loading {
            return;
        }
        match call(host, "GET", "/quests/@me", None) {
            Ok(body) => self.set_quests(&body),
            Err(e) => {
                if let Some(ms) = e.rate_limited() {
                    return self.after(host, ms, "load", "");
                }
                // Not signed in yet: Discord's modules are not ready (`loadModules`); wait.
                if e.status.is_none() && e.message.starts_with("client not signed in") {
                    return self.after(host, 1000, "load", "");
                }
                let msg = e.msg();
                self.say(host, Lvl::Err, "qm-fatal", &[("error", &msg)]);
                let (title, body) = (
                    host.t("qm-notify-title", &[]),
                    host.t("qm-notify-stopped", &[("error", &msg)]),
                );
                host.notify(&title, &body);
                return self.stop(host);
            }
        }
        if self.open_quests().is_empty() {
            self.say(host, Lvl::Success, "qm-all-completed", &[]);
            let (title, body) = (
                host.t("qm-notify-title", &[]),
                host.t("qm-notify-none", &[]),
            );
            host.notify(&title, &body);
            self.phase = Phase::Finished;
            return;
        }
        let items = self.picker_items();
        if items.is_empty() {
            // The picker closes at once with nothing selected.
            self.say(host, Lvl::Info, "qm-no-selection", &[]);
            return self.stop(host);
        }
        self.picker = items.iter().map(|i| i.0.clone()).collect();
        self.phase = Phase::Picker;
        let count = items.len().to_string();
        self.say(host, Lvl::Info, "qm-picker", &[("count", &count)]);
        for (id, name, kind, label, reward) in &items {
            let args = [
                ("id", id.as_str()),
                ("name", name.as_str()),
                ("type", kind.label()),
                ("label", *label),
                ("reward", reward.as_str()),
            ];
            self.say(host, Lvl::Info, "qm-picker-item", &args);
        }
        if let Some(sel) = self.selection.take() {
            self.choose(host, sel);
        }
    }

    /// The picker's START.
    fn choose(&mut self, host: &mut dyn Host, sel: Sel) {
        self.selected = match sel {
            Sel::All => self.picker.iter().cloned().collect(),
            Sel::One(id) => self.picker.iter().filter(|p| **p == id).cloned().collect(),
        };
        if self.selected.is_empty() {
            self.say(host, Lvl::Info, "qm-no-selection", &[]);
            return self.stop(host);
        }
        self.phase = Phase::Running;
        self.loop_n = 1;
        self.cycle_begin(host);
    }

    fn reset_traffic(&mut self) {
        self.traffic.queue.clear();
        self.traffic.parked.clear();
        self.traffic.processing = false;
    }

    fn stop(&mut self, host: &mut dyn Host) {
        if !self.running && !self.active() {
            return;
        }
        for tag in std::mem::take(&mut self.tags) {
            host.cancel_schedule(&tag);
        }
        self.session += 1;
        self.running = false;
        self.reset_traffic();
        self.say(host, Lvl::Warn, "qm-stopping", &[]);
        self.bypass_abort(host);
        self.runs.clear();
        self.fake_games_clear(host);
        self.runners = [Runner::default(), Runner::default()];
        self.claims.clear();
        self.cards.clear();
        self.picker.clear();
        self.selected.clear();
        self.selection = None;
        self.phase = Phase::Idle;
    }

    /* ── the main loop ── */

    fn cycle_begin(&mut self, host: &mut dyn Host) {
        if !self.running {
            return;
        }
        let n = self.loop_n.to_string();
        self.say(host, Lvl::Info, "qm-loop-start", &[("n", &n)]);
        self.cycle_fetch(host);
    }

    fn cycle_fetch(&mut self, host: &mut dyn Host) {
        if !self.running {
            return;
        }
        match call(host, "GET", "/quests/@me", None) {
            Ok(body) => self.set_quests(&body),
            Err(e) => {
                if let Some(ms) = e.rate_limited() {
                    return self.after(host, ms, "cyc", "fetch");
                }
                // The cycle's catch: log, wait 3 s, next loop.
                let (n, msg) = (self.loop_n.to_string(), e.msg());
                self.say(
                    host,
                    Lvl::Err,
                    "qm-loop-error",
                    &[("n", &n), ("error", &msg)],
                );
                return self.after(host, 3000, "cyc", "next");
            }
        }
        let now = self.now();
        let active: Vec<Val> = self
            .quests
            .iter()
            .filter(|q| {
                let id = q_id(q).unwrap_or_default();
                self.selected.contains(&id)
                    && !q_completed(q)
                    && not_expired(q, now)
                    && id != BLACKLISTED_QUEST_ID
                    && !self.skipped.contains(&id)
            })
            .cloned()
            .collect();
        if active.is_empty() {
            let still = self.quests.iter().any(|q| {
                let id = q_id(q).unwrap_or_default();
                self.selected.contains(&id)
                    && !q_completed(q)
                    && not_expired(q, now)
                    && id != BLACKLISTED_QUEST_ID
            });
            if still && !self.skipped.is_empty() {
                self.say(host, Lvl::Warn, "qm-failed-skipped", &[]);
                let (title, body) = (
                    host.t("qm-notify-title", &[]),
                    host.t("qm-notify-failed", &[]),
                );
                host.notify(&title, &body);
                self.phase = Phase::Finished;
                return;
            }
            self.say(host, Lvl::Success, "qm-all-completed", &[]);
            // Sound.play("done"): no audio in the proxy.
            return self.after_loop(host);
        }
        let mut lanes: [VecDeque<Queued>; 2] = [VecDeque::new(), VecDeque::new()];
        for q in active {
            let id = q_id(&q).unwrap_or_default();
            let Some(cfg) = q_task_cfg(&q).filter(|c| matches!(get(c, "tasks"), Some(Val::Map(_))))
            else {
                self.say(host, Lvl::Warn, "qm-invalid-config", &[("id", &id)]);
                continue;
            };
            let app = q_app_id(&q);
            let Some((kind, key, target)) = detect_type(cfg, app.as_deref()) else {
                let name = q_config(&q)
                    .and_then(|c| get(c, "messages"))
                    .and_then(|m| pick(m, "questName", "quest_name"))
                    .and_then(text)
                    .unwrap_or_else(|| id.clone());
                self.say(host, Lvl::Warn, "qm-unknown-type", &[("name", &name)]);
                continue;
            };
            if target <= 0.0 {
                let t = n(target);
                self.say(
                    host,
                    Lvl::Warn,
                    "qm-invalid-target",
                    &[("target", &t), ("id", &id)],
                );
                continue;
            }
            let info = Info {
                id: id.clone(),
                app_id: app.unwrap_or_else(|| "0".into()),
                name: q_name(&q),
                target,
                kind,
                key,
            };
            if q_enrolled(&q).is_none() && !self.auto_enroll {
                self.card(&info, Some(0.0), Status::Pending);
                if let Some(c) = self.cards.get_mut(&id) {
                    c.enroll_required = true;
                }
                continue;
            }
            if self
                .cards
                .get(&id)
                .is_some_and(|c| c.status == Status::Running)
            {
                continue;
            }
            self.card(&info, Some(0.0), Status::Queue);
            if let Some(c) = self.cards.get_mut(&id) {
                c.enroll_required = false;
            }
            let lane = if kind == Kind::Video {
                Lane::Video
            } else {
                Lane::Game
            };
            lanes[lane.idx()].push_back(Queued { info, quest: q });
        }
        let (games, videos) = (lanes[0].len(), lanes[1].len());
        if games + videos == 0 {
            let ms = self.rng.rnd(4000, 6000);
            return self.after(host, ms, "cyc", "post");
        }
        let game_limit = setting_num(host, "concurrentGames", 1.0).max(1.0) as usize;
        let video_limit = setting_num(host, "concurrentVideos", 3.0).max(1.0) as usize;
        let args = [
            ("videos", videos.to_string()),
            ("vlimit", video_limit.to_string()),
            ("games", games.to_string()),
            ("glimit", game_limit.to_string()),
        ];
        let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.say(host, Lvl::Info, "qm-processing", &args);
        let [game_q, video_q] = lanes;
        for (lane, queue, limit) in [
            (Lane::Game, game_q, game_limit),
            (Lane::Video, video_q, video_limit),
        ] {
            let finished = queue.is_empty();
            self.runners[lane.idx()] = Runner {
                queue,
                limit,
                finished,
                loop_done: finished,
                ..Runner::default()
            };
        }
        for lane in [Lane::Game, Lane::Video] {
            if !self.runners[lane.idx()].finished {
                self.runner_step(host, lane);
            }
        }
    }

    /// After a cycle: the pause before the next rescan.
    fn cycle_post(&mut self, host: &mut dyn Host) {
        if !self.running {
            return;
        }
        let n = self.loop_n.to_string();
        if self.random_delay {
            let ms = self.rng.rnd(60_000, 1_800_000);
            let min = ((ms as f64) / 60_000.0).round().to_string();
            self.say(
                host,
                Lvl::Info,
                "qm-loop-random",
                &[("n", &n), ("min", &min)],
            );
            self.after(host, ms, "cyc", "next");
        } else {
            self.say(host, Lvl::Info, "qm-loop-wait", &[("n", &n)]);
            let ms = self.rng.rnd(2500, 4500);
            self.after(host, ms, "cyc", "next");
        }
    }

    /// The end of `main()` after the loop.
    fn after_loop(&mut self, host: &mut dyn Host) {
        self.phase = Phase::Finished;
        if self.cards.values().any(|c| c.claimable) {
            self.say(host, Lvl::Info, "qm-claim-above", &[]);
        } else {
            self.say(host, Lvl::Info, "qm-cycle-finished", &[]);
        }
    }

    /* ── runConcurrent ── */

    fn runner_step(&mut self, host: &mut dyn Host, lane: Lane) {
        if !self.running {
            return;
        }
        let Some(task) = self.runners[lane.idx()].queue.pop_front() else {
            self.runners[lane.idx()].loop_done = true;
            return self.runner_check(host, lane);
        };
        self.runners[lane.idx()]
            .executing
            .insert(task.info.id.clone());
        self.task_start(host, task, lane);
        let ms = self.rng.rnd(1500, 4000);
        self.after(host, ms, "run", lane.tag());
    }

    fn runner_wake(&mut self, host: &mut dyn Host, lane: Lane) {
        let r = &mut self.runners[lane.idx()];
        if r.executing.len() >= r.limit {
            r.waiting = true;
        } else {
            self.runner_step(host, lane);
        }
    }

    fn runner_check(&mut self, host: &mut dyn Host, lane: Lane) {
        let r = &mut self.runners[lane.idx()];
        if !r.loop_done || !r.executing.is_empty() || r.finished {
            return;
        }
        r.finished = true;
        if self.runners.iter().all(|r| r.finished) && self.phase == Phase::Running {
            self.cycle_post(host);
        }
    }

    /* ── tasks ── */

    /// `Logger.updateTask` with the fields the script passes.
    fn card(&mut self, info: &Info, cur: Option<f64>, status: Status) {
        let seq = self.card_seq;
        let c = self.cards.entry(info.id.clone()).or_insert_with(|| Card {
            name: String::new(),
            kind: info.kind,
            cur: 0.0,
            max: 0.0,
            status,
            claimable: false,
            claim: ClaimState::None,
            enroll_required: false,
            order: seq,
        });
        c.name = info.name.clone();
        c.kind = info.kind;
        if let Some(cur) = cur {
            c.cur = cur;
        }
        c.max = info.target;
        c.status = status;
        self.card_seq += 1;
    }

    fn task_start(&mut self, host: &mut dyn Host, task: Queued, lane: Lane) {
        let Queued { info, quest } = task;
        let mut run = Run {
            info,
            lane,
            quest,
            status: None,
            cur: 0.0,
            fails: 0,
            start: self.now(),
            enrolled_at: 0,
            synth_enrolled: None,
            completed: false,
            key: String::new(),
            soft_keys: Vec::new(),
            key_idx: 0,
            wake: Wake::Dispatch,
            awaits_finish: false,
            bypass: None,
            primary_rejected: false,
            beat_primary: false,
        };
        if q_enrolled(&run.quest).is_none() {
            let name = run.info.name.clone();
            self.say(host, Lvl::Info, "qm-enroll-accept", &[("name", &name)]);
            let route = format!("/quests/{}/enroll", run.info.id);
            return self.send(host, run, Purpose::PreEnroll, route, enroll_body());
        }
        run.wake = Wake::Dispatch;
        self.dispatch_type(host, run);
    }

    /// The handler for the task's type.
    fn dispatch_type(&mut self, host: &mut dyn Host, mut r: Run) {
        r.status = q_status(&r.quest).cloned();
        match r.info.kind {
            Kind::Video => self.video_begin(host, r),
            Kind::Achievement => self.ach_begin(host, r),
            Kind::Activity => self.act_begin(host, r),
            Kind::Game | Kind::Stream => self.generic_begin(host, r),
        }
    }

    /// Keep the task and wake it later.
    fn sleep(&mut self, host: &mut dyn Host, mut r: Run, ms: u64, wake: Wake) {
        r.wake = wake;
        let id = r.info.id.clone();
        self.runs.insert(id.clone(), r);
        self.after(host, ms, "task", &id);
    }

    fn task_wake(&mut self, host: &mut dyn Host, id: &str) {
        let Some(mut r) = self.runs.remove(id) else {
            return;
        };
        match r.wake {
            Wake::Dispatch => {
                if let Some(q) = self.cached(id) {
                    r.quest = q.clone();
                }
                self.dispatch_type(host, r);
            }
            Wake::VideoEnsured => {
                self.reread_enrolled(&mut r);
                self.video_after_ensure(host, r);
            }
            Wake::VideoReenrolled => {
                self.reread_enrolled(&mut r);
                self.say(host, Lvl::Info, "qm-video-reenrolled", &[]);
                let ms = self.rng.rnd(2000, 4000);
                self.sleep(host, r, ms, Wake::VideoIter);
            }
            Wake::VideoIter => self.video_iter(host, r),
            Wake::GenericFetch => self.generic_begin(host, r),
            Wake::GenericIter => self.generic_iter(host, r),
            Wake::AchIter => self.ach_iter(host, r),
            Wake::ActIter => self.act_iter(host, r),
            Wake::Bypass => {
                if r.bypass.as_ref().map(|b| b.step) == Some(bypass::Step::Consent) {
                    // `Consent.TIMEOUT`: no answer in 60 s is a no.
                    self.consent_wait = None;
                    self.consent_answer(host, &mut r, false);
                }
                self.bypass_run(host, r);
            }
        }
    }

    /// The task is over: free its slot.
    fn task_end(&mut self, host: &mut dyn Host, id: &str, lane: Lane) {
        self.runs.remove(id);
        // The generic task's `finally { cleanup() }`: its fake game goes.
        self.fake_game_remove(host, id);
        let r = &mut self.runners[lane.idx()];
        if !r.executing.remove(id) {
            return;
        }
        if r.waiting {
            r.waiting = false;
            self.runner_step(host, lane);
        } else {
            self.runner_check(host, lane);
        }
    }

    /// `Tasks.failTask`.
    fn fail(&mut self, host: &mut dyn Host, r: Run, reason_key: &str, args: &[(&str, &str)]) {
        let cur = self.cards.get(&r.info.id).map_or(0.0, |c| c.cur);
        self.card(&r.info, Some(cur), Status::Failed);
        let reason = host.t(reason_key, args);
        let name = r.info.name.clone();
        self.say(
            host,
            Lvl::Err,
            "qm-task-aborted",
            &[("name", &name), ("reason", &reason)],
        );
        self.skipped.insert(r.info.id.clone());
        self.after(host, 2000, "rm", &r.info.id);
        self.task_end(host, &r.info.id, r.lane);
    }

    /// `Tasks.finish`: the card, the log, the notification and `autoClaim`.
    fn finish(&mut self, host: &mut dyn Host, mut r: Run) {
        let target = r.info.target;
        self.card(&r.info, Some(target), Status::Completed);
        let name = r.info.name.clone();
        self.say(host, Lvl::Success, "qm-completed", &[("name", &name)]);
        // Sound.play("tick"): no audio in the proxy.
        let title = host.t("qm-notify-completed", &[]);
        host.notify(&title, &name);
        let id = r.info.id.clone();
        if self.auto_claim {
            self.claims.insert(
                id.clone(),
                ClaimJob {
                    name,
                    manual: false,
                },
            );
            let ms = self.rng.rnd(2500, 6000);
            self.after(host, ms, "claim", &id);
            if r.awaits_finish {
                // The slot frees once the claim is done.
                r.wake = Wake::Dispatch;
                self.runs.insert(id, r);
                return;
            }
        } else if let Some(c) = self.cards.get_mut(&id) {
            c.claimable = true;
        }
        self.task_end(host, &id, r.lane);
    }

    fn claim_wake(&mut self, host: &mut dyn Host, id: &str) {
        if !self.claims.get(id).is_some_and(|j| j.manual) && !self.running {
            return;
        }
        self.claim_wake_now(host, id);
    }

    /// `Tasks.claimReward`: automatic (after `finish`) or the CLAIM REWARD button.
    fn claim_wake_now(&mut self, host: &mut dyn Host, id: &str) {
        let Some(job) = self.claims.remove(id) else {
            return;
        };
        let route = format!("/quests/{id}/claim-reward");
        let res = call(host, "POST", &route, Some(&claim_body()));
        if let Err(e) = &res
            && let Some(ms) = e.rate_limited()
        {
            self.claims.insert(id.to_string(), job);
            return self.after(host, ms, "claim", id);
        }
        let name = job.name.clone();
        if job.manual {
            match res {
                Ok(_) => {
                    self.say(host, Lvl::Success, "qm-claim-ok", &[("name", &name)]);
                    if let Some(c) = self.cards.get_mut(id) {
                        c.status = Status::Claimed;
                        c.claimable = false;
                        c.claim = ClaimState::None;
                    }
                    self.after(host, 2000, "rm", id);
                }
                Err(_) => {
                    self.say(host, Lvl::Warn, "qm-claim-action", &[("name", &name)]);
                    if let Some(c) = self.cards.get_mut(id) {
                        c.claim = ClaimState::Failed;
                    }
                }
            }
            return;
        }
        match res {
            Ok(body) if get(&body, "claimed_at").is_some() => {
                self.say(host, Lvl::Success, "qm-claim-auto", &[("name", &name)]);
                if let Some(c) = self.cards.get_mut(id) {
                    c.status = Status::Claimed;
                }
                self.after(host, 2000, "rm", id);
                return self.end_awaiting(host, id);
            }
            Ok(_) => {}
            Err(e) => {
                if e.body_field("captcha_key").is_some()
                    || e.body_field("captcha_sitekey").is_some()
                {
                    self.say(
                        host,
                        Lvl::Warn,
                        "qm-claim-captcha",
                        &[("name", &name), ("id", id)],
                    );
                } else {
                    let msg = e
                        .body_field("message")
                        .and_then(text)
                        .unwrap_or_else(|| e.message.clone());
                    self.say(
                        host,
                        Lvl::Err,
                        "qm-claim-auto-failed",
                        &[("name", &name), ("error", &msg)],
                    );
                }
            }
        }
        if let Some(c) = self.cards.get_mut(id) {
            c.status = Status::Completed;
            c.claimable = true;
        }
        self.end_awaiting(host, id);
    }

    /// An ACHIEVEMENT task that waited for its claim ends now.
    fn end_awaiting(&mut self, host: &mut dyn Host, id: &str) {
        if let Some(lane) = self
            .runs
            .get(id)
            .filter(|r| r.awaits_finish)
            .map(|r| r.lane)
        {
            self.task_end(host, id, lane);
        }
    }

    /* ── the request queue (Traffic) ── */

    /// Park the task, then queue its request (the answer may come back at once).
    fn send(&mut self, host: &mut dyn Host, r: Run, purpose: Purpose, route: String, body: Val) {
        let id = r.info.id.clone();
        self.runs.insert(id.clone(), r);
        if !self.running {
            return;
        }
        self.traffic.next_id += 1;
        let req = Req {
            id: self.traffic.next_id,
            qid: id,
            purpose,
            route,
            body,
            attempts: 0,
        };
        self.traffic.queue.push_back(req);
        if !self.traffic.processing {
            self.traffic.processing = true;
            self.traffic_next(host);
        }
    }

    /// One turn of `Traffic.process`: send the head of the queue, then wait 1.2–1.8 s.
    fn traffic_next(&mut self, host: &mut dyn Host) {
        if !self.running {
            return self.reset_traffic();
        }
        let Some(mut req) = self.traffic.queue.pop_front() else {
            self.traffic.processing = false;
            return;
        };
        let mut extra = 0;
        match call(host, "POST", &req.route, Some(&req.body)) {
            Ok(body) => self.deliver(host, &req.qid, req.purpose, Ok(body)),
            Err(e) if e.retryable() && req.attempts < MAX_RETRIES => {
                req.attempts += 1;
                let secs = e
                    .body_field("retry_after")
                    .and_then(num)
                    .unwrap_or(2f64.powi(req.attempts as i32));
                let delay = (secs * 1000.0).max(0.0) as u64;
                let global = e.body_field("global").and_then(Val::as_bool) == Some(true);
                let args = [
                    ("n", req.attempts.to_string()),
                    ("max", MAX_RETRIES.to_string()),
                    ("secs", format!("{:.1}", delay as f64 / 1000.0)),
                    ("status", e.status_text()),
                ];
                let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                self.say(host, Lvl::Warn, "qm-net-retry", &args);
                let jitter = self.rng.rnd(200, 800);
                if global {
                    self.traffic.queue.push_front(req);
                    extra = delay + jitter;
                } else {
                    let rid = req.id.to_string();
                    self.traffic.parked.insert(req.id, req);
                    self.after(host, delay + jitter, "rt", &rid);
                }
            }
            Err(e) => {
                if e.client() {
                    let s = e.status_text();
                    self.say(
                        host,
                        Lvl::Debug,
                        "qm-net-client",
                        &[("status", &s), ("url", &req.route)],
                    );
                } else {
                    let m = e.msg();
                    self.say(
                        host,
                        Lvl::Err,
                        "qm-net-failed",
                        &[("url", &req.route), ("error", &m)],
                    );
                }
                self.deliver(host, &req.qid, req.purpose, Err(e));
            }
        }
        if !self.running {
            return;
        }
        let ms = extra + self.rng.rnd(1200, 1800);
        self.after(host, ms, "traffic", "");
    }

    /// A non-global retry is due: back to the end of the queue.
    fn traffic_retry(&mut self, host: &mut dyn Host, rid: &str) {
        let Some(req) = rid
            .parse()
            .ok()
            .and_then(|id| self.traffic.parked.remove(&id))
        else {
            return;
        };
        if !self.running {
            return;
        }
        self.traffic.queue.push_back(req);
        if !self.traffic.processing {
            self.traffic.processing = true;
            self.traffic_next(host);
        }
    }

    /// A request's answer goes back to the task that sent it.
    fn deliver(
        &mut self,
        host: &mut dyn Host,
        id: &str,
        purpose: Purpose,
        res: Result<Val, ApiErr>,
    ) {
        let Some(r) = self.runs.remove(id) else {
            return;
        };
        match purpose {
            Purpose::PreEnroll => self.on_pre_enroll(host, r, res),
            Purpose::VideoEnsure => match res {
                Ok(_) => {
                    let ms = self.rng.rnd(800, 1500);
                    self.sleep(host, r, ms, Wake::VideoEnsured);
                }
                Err(e) => {
                    let (s, m) = (e.status_text(), e.msg());
                    self.say(
                        host,
                        Lvl::Warn,
                        "qm-video-enroll-failed",
                        &[("status", &s), ("error", &m)],
                    );
                    self.video_after_ensure(host, r);
                }
            },
            Purpose::VideoReenroll => match res {
                Ok(_) => {
                    let ms = self.rng.rnd(800, 1500);
                    self.sleep(host, r, ms, Wake::VideoReenrolled);
                }
                Err(_) => {
                    let ms = self.rng.rnd(2000, 4000);
                    self.sleep(host, r, ms, Wake::VideoIter);
                }
            },
            Purpose::VideoProgress(ts) => self.on_video_progress(host, r, ts, res),
            Purpose::VideoFinal => self.finish(host, r),
            Purpose::GenBeat => self.on_generic_beat(host, r, res),
            Purpose::GenFinal => {
                self.finish(host, r);
            }
            Purpose::AchBeat => self.on_ach_beat(host, r, res),
            Purpose::AchFinal => self.ach_after_loop(host, r),
            Purpose::ActBeat => self.on_act_beat(host, r, res),
            Purpose::ActFinal => {
                if let Err(e) = res {
                    let m = e.msg();
                    self.say(
                        host,
                        Lvl::Debug,
                        "qm-activity-final-failed",
                        &[("error", &m)],
                    );
                }
                self.act_after_loop(host, r);
            }
        }
    }

    /// The enrol before a task runs (`taskFunc`).
    fn on_pre_enroll(&mut self, host: &mut dyn Host, r: Run, res: Result<Val, ApiErr>) {
        match res {
            Ok(_) => {
                let ms = self.rng.rnd(800, 1500);
                self.sleep(host, r, ms, Wake::Dispatch);
            }
            Err(e) => {
                let name = r.info.name.clone();
                if e.skippable() {
                    self.skipped.insert(r.info.id.clone());
                    let s = e.status_text();
                    self.say(
                        host,
                        Lvl::Warn,
                        "qm-enroll-unavailable",
                        &[("name", &name), ("status", &s)],
                    );
                } else {
                    let m = e.msg();
                    self.say(
                        host,
                        Lvl::Err,
                        "qm-enroll-failed",
                        &[("name", &name), ("error", &m)],
                    );
                }
                self.fail(host, r, "qm-reason-enroll", &[]);
            }
        }
    }

    /* ── VIDEO ── */

    /// `ensureEnrolled`: `true` when the store already has the enrolment (no request).
    fn ensure(&mut self, host: &mut dyn Host, mut r: Run, purpose: Purpose) -> Option<Run> {
        if let Some(q) = self.cached(&r.info.id) {
            r.quest = q.clone();
        }
        if q_enrolled(&r.quest).is_some() {
            return Some(r);
        }
        let id = r.info.id.clone();
        self.say(host, Lvl::Info, "qm-enroll-accept-id", &[("id", &id)]);
        let route = format!("/quests/{id}/enroll");
        self.send(host, r, purpose, route, enroll_body());
        None
    }

    /// After an enrol: read the store again; it may lag, so `enrolledAt` becomes now.
    fn reread_enrolled(&self, r: &mut Run) {
        if let Some(q) = self.cached(&r.info.id) {
            r.quest = q.clone();
        }
        r.synth_enrolled = if q_enrolled(&r.quest).is_none() {
            Some(self.now())
        } else {
            None
        };
    }

    fn video_begin(&mut self, host: &mut dyn Host, r: Run) {
        if let Some(r) = self.ensure(host, r, Purpose::VideoEnsure) {
            self.video_after_ensure(host, r);
        }
    }

    fn video_after_ensure(&mut self, host: &mut dyn Host, mut r: Run) {
        let now = self.now();
        r.enrolled_at = q_enrolled(&r.quest)
            .and_then(|v| match v {
                Val::Str(s) => parse_iso(s),
                v => num(v).map(|f| f as i64),
            })
            .map(|t| t.max(0) as u64)
            .or(r.synth_enrolled)
            .unwrap_or(now);
        let key = r.info.key.clone();
        let s = r.status.as_ref();
        let quest_status = q_status(&r.quest);
        r.cur = prog(s, &key)
            .or_else(|| prog(quest_status, &key))
            .or_else(|| prog(s, "WATCH_VIDEO"))
            .or_else(|| prog(s, "WATCH_VIDEO_ON_MOBILE"))
            .unwrap_or(0.0)
            .floor();
        r.fails = 0;
        r.completed = false;
        self.card(&r.info, Some(r.cur), Status::Running);
        let (name, cur, target) = (r.info.name.clone(), n(r.cur), n(r.info.target));
        self.say(
            host,
            Lvl::Info,
            "qm-video-start",
            &[("name", &name), ("cur", &cur), ("target", &target)],
        );
        r.start = now;
        self.video_iter(host, r);
    }

    fn video_iter(&mut self, host: &mut dyn Host, r: Run) {
        if !(r.cur < r.info.target && self.running) {
            return self.video_after_loop(host, r);
        }
        let now = self.now();
        if now.saturating_sub(r.start) > MAX_TIME_MS {
            return self.fail(host, r, "qm-reason-timeout", &[]);
        }
        let max_allowed = (now as i64 - r.enrolled_at as i64).div_euclid(1000) + VIDEO_MAX_FUTURE;
        let room = max_allowed as f64 - r.cur;
        if room < 1.0 {
            return self.sleep(host, r, TICK_MS, Wake::VideoIter);
        }
        let step = VIDEO_SPEED.min(room).min(r.info.target - r.cur);
        let ts = r
            .info
            .target
            .min((r.cur + step + self.rng.random()).floor());
        let route = format!("/quests/{}/video-progress", r.info.id);
        let body = obj(&[("timestamp", ts_val(ts))]);
        self.send(host, r, Purpose::VideoProgress(ts), route, body);
    }

    fn on_video_progress(
        &mut self,
        host: &mut dyn Host,
        mut r: Run,
        ts: f64,
        res: Result<Val, ApiErr>,
    ) {
        match res {
            Ok(body) => {
                r.completed = get(&body, "completed_at").is_some();
                let key = r.info.key.clone();
                let b = Some(&body);
                let server = prog(b, &key)
                    .or_else(|| prog(b, "WATCH_VIDEO"))
                    .or_else(|| prog(b, "WATCH_VIDEO_ON_MOBILE"));
                r.cur = match server {
                    Some(v) => r.cur.max(v.floor()),
                    None => r.cur.max(ts),
                };
                r.fails = 0;
                self.card(&r.info, Some(r.cur), Status::Running);
                let (name, cur, target) = (r.info.name.clone(), n(r.cur), n(r.info.target));
                self.say(
                    host,
                    Lvl::Info,
                    "qm-video-progress",
                    &[("name", &name), ("cur", &cur), ("target", &target)],
                );
                if r.completed || r.cur >= r.info.target {
                    return self.video_after_loop(host, r);
                }
                self.sleep(host, r, TICK_MS, Wake::VideoIter);
            }
            Err(e) => {
                r.fails += 1;
                let (s, m) = (e.status_text(), e.msg());
                self.say(
                    host,
                    Lvl::Warn,
                    "qm-video-http",
                    &[("status", &s), ("error", &m)],
                );
                if matches!(e.status, Some(403 | 404)) {
                    // 404 often = not enrolled yet / store lag: re-enrol and keep trying.
                    if let Some(r) = self.ensure(host, r, Purpose::VideoReenroll) {
                        self.say(host, Lvl::Info, "qm-video-reenrolled", &[]);
                        let ms = self.rng.rnd(2000, 4000);
                        self.sleep(host, r, ms, Wake::VideoIter);
                    }
                } else if e.client() && e.status != Some(429) {
                    if r.fails >= MAX_TASK_FAILURES {
                        let s = e.status_text();
                        return self.fail(host, r, "qm-reason-client", &[("status", &s)]);
                    }
                    self.video_iter(host, r);
                } else if r.fails >= MAX_TASK_FAILURES {
                    self.fail(host, r, "qm-reason-network", &[]);
                } else {
                    let ms = self.rng.rnd(1500, 3000);
                    self.sleep(host, r, ms, Wake::VideoIter);
                }
            }
        }
    }

    fn video_after_loop(&mut self, host: &mut dyn Host, r: Run) {
        if !(self.running && (r.completed || r.cur >= r.info.target)) {
            let (id, lane) = (r.info.id.clone(), r.lane);
            return self.task_end(host, &id, lane);
        }
        if r.completed {
            return self.finish(host, r);
        }
        let route = format!("/quests/{}/video-progress", r.info.id);
        let body = obj(&[("timestamp", ts_val(r.info.target.floor()))]);
        self.send(host, r, Purpose::VideoFinal, route, body);
    }

    /* ── GAME / STREAM (Tasks.generic) ── */

    fn generic_begin(&mut self, host: &mut dyn Host, mut r: Run) {
        if !self.running {
            let (id, lane) = (r.info.id.clone(), r.lane);
            return self.task_end(host, &id, lane);
        }
        // fetchGameData: the app's public data (the script builds its fake process from it).
        let route = format!("/applications/public?application_ids={}", r.info.app_id);
        let mut game_name = r.info.name.clone();
        match call(host, "GET", &route, None) {
            Ok(body) => {
                if let Some(n) = body
                    .as_list()
                    .first()
                    .and_then(|a| get(a, "name"))
                    .and_then(text)
                    && !n.is_empty()
                {
                    game_name = n;
                }
            }
            Err(e) => {
                if let Some(ms) = e.rate_limited() {
                    return self.sleep(host, r, ms, Wake::GenericFetch);
                }
                let (name, s, m) = (r.info.name.clone(), e.status_text(), e.msg());
                self.say(
                    host,
                    Lvl::Debug,
                    "qm-fetch-game-fallback",
                    &[("name", &name), ("status", &s), ("error", &m)],
                );
            }
        }
        let key = r.info.key.clone();
        let s = r.status.as_ref();
        r.cur = prog(s, &key)
            .or_else(|| prog(s, "PLAY_ON_DESKTOP"))
            .or_else(|| prog(s, "PLAY_ON_DESKTOP_V2"))
            .or_else(|| prog(s, "STREAM_ON_DESKTOP"))
            .unwrap_or(0.0);
        if r.info.kind == Kind::Stream {
            self.say(
                host,
                Lvl::Warn,
                "qm-stream-spoofed",
                &[("name", &game_name)],
            );
        } else {
            let game = FakeGame {
                quest: r.info.id.clone(),
                app_id: r.info.app_id.clone(),
                name: game_name.clone(),
                start: self.now(),
            };
            self.fake_game_add(host, game);
            self.say(host, Lvl::Info, "qm-game-spoofed", &[("name", &game_name)]);
        }
        self.card(&r.info, Some(r.cur), Status::Running);
        r.soft_keys = vec![format!("call:{}:1", r.info.id)];
        if !r.info.app_id.is_empty() {
            r.soft_keys.push(format!("game:{}", r.info.app_id));
        }
        r.key_idx = 0;
        r.start = self.now();
        self.generic_iter(host, r);
    }

    fn generic_iter(&mut self, host: &mut dyn Host, mut r: Run) {
        if r.cur < r.info.target && self.running {
            // Prefer live store progress (what Discord's UI shows).
            let store = self.read_store_progress(&r.info.id, &r.info.key, r.cur);
            if store > r.cur {
                r.cur = store;
                self.card(&r.info, Some(r.cur), Status::Running);
            }
        }
        if !(r.cur < r.info.target && self.running) {
            return self.generic_after_loop(host, r);
        }
        let body = self.generic_beat_body(host, &mut r, false);
        let route = format!("/quests/{}/heartbeat", r.info.id);
        self.send(host, r, Purpose::GenBeat, route, body);
    }

    /// `Tasks.readQuestProgress`.
    fn read_store_progress(&self, id: &str, key: &str, fallback: f64) -> f64 {
        let s = self.cached(id).and_then(q_status);
        prog(s, key)
            .or_else(|| prog(s, "PLAY_ON_DESKTOP"))
            .or_else(|| prog(s, "PLAY_ON_DESKTOP_V2"))
            .or_else(|| prog(s, "STREAM_ON_DESKTOP"))
            .or_else(|| stream_secs(s))
            .unwrap_or(fallback)
    }

    fn on_generic_beat(&mut self, host: &mut dyn Host, mut r: Run, res: Result<Val, ApiErr>) {
        match res {
            Ok(body) => {
                let key = r.info.key.clone();
                let b = Some(&body);
                let v = prog(b, &key)
                    .or_else(|| prog(b, "PLAY_ON_DESKTOP"))
                    .or_else(|| prog(b, "PLAY_ON_DESKTOP_V2"))
                    .or_else(|| prog(b, "STREAM_ON_DESKTOP"))
                    .or_else(|| stream_secs(b));
                if let Some(v) = v.filter(|v| *v > r.cur) {
                    r.cur = v;
                    self.card(&r.info, Some(r.cur), Status::Running);
                    let (name, cur, target) =
                        (r.info.name.clone(), n(r.cur.floor()), n(r.info.target));
                    self.say(
                        host,
                        Lvl::Info,
                        "qm-progress-api",
                        &[("name", &name), ("cur", &cur), ("target", &target)],
                    );
                }
            }
            Err(e) if r.beat_primary => {
                // The client-shaped beat was refused: the script's soft heartbeats take over.
                if matches!(e.status, Some(400 | 403 | 404)) {
                    r.primary_rejected = true;
                }
                let (name, s) = (r.info.name.clone(), e.status_text());
                self.say(
                    host,
                    Lvl::Warn,
                    "qm-desktop-beat-refused",
                    &[("name", &name), ("status", &s)],
                );
            }
            Err(e) => {
                let sk = r.soft_keys[r.key_idx % r.soft_keys.len()].clone();
                // Rotate the stream key on rejection; never fail the task from a soft heartbeat.
                if matches!(e.status, Some(400 | 404)) {
                    r.key_idx += 1;
                }
                let s = e.status.map_or("err".into(), |s| s.to_string());
                self.say(
                    host,
                    Lvl::Debug,
                    "qm-soft-heartbeat",
                    &[("key", &sk), ("status", &s)],
                );
            }
        }
        if self.now().saturating_sub(r.start) > MAX_TIME_MS {
            return self.fail(host, r, "qm-reason-timeout-25", &[]);
        }
        let ms = self.rng.rnd(15_000, 20_000);
        self.sleep(host, r, ms, Wake::GenericIter);
    }

    fn generic_after_loop(&mut self, host: &mut dyn Host, mut r: Run) {
        if self.running && r.cur >= r.info.target {
            let route = format!("/quests/{}/heartbeat", r.info.id);
            let body = self.generic_beat_body(host, &mut r, true);
            return self.send(host, r, Purpose::GenFinal, route, body);
        }
        let (id, lane) = (r.info.id.clone(), r.lane);
        self.task_end(host, &id, lane);
    }

    /// The `QUESTS_SEND_HEARTBEAT_SUCCESS` subscription: the client's own heartbeat answer.
    fn heartbeat_seen(&mut self, host: &mut dyn Host, id: &str, status: &Val) {
        if !self.running {
            return;
        }
        let Some(r) = self.runs.get_mut(id) else {
            return;
        };
        if !matches!(r.info.kind, Kind::Game | Kind::Stream) || r.soft_keys.is_empty() {
            return;
        }
        let s = Some(status);
        let p = prog(s, &r.info.key)
            .or_else(|| prog(s, "PLAY_ON_DESKTOP"))
            .or_else(|| prog(s, "PLAY_ON_DESKTOP_V2"))
            .or_else(|| prog(s, "STREAM_ON_DESKTOP"))
            .map(f64::floor)
            .or_else(|| stream_secs(s))
            .unwrap_or(0.0);
        if p > r.cur {
            r.cur = p;
            let info = r.info.clone();
            self.card(&info, Some(p), Status::Running);
            let (name, cur, target) = (info.name, n(p.floor()), n(info.target));
            self.say(
                host,
                Lvl::Info,
                "qm-progress",
                &[("name", &name), ("cur", &cur), ("target", &target)],
            );
        }
    }

    /* ── ACHIEVEMENT ── */

    /// The most recent DM, else the first voice channel of the first guild that has one.
    fn heartbeat_channel(&self) -> Option<u64> {
        let dm = self
            .dms
            .iter()
            .enumerate()
            .max_by(|(ia, a), (ib, b)| a.1.cmp(&b.1).then(ib.cmp(ia)))
            .map(|(_, d)| d.0);
        dm.or_else(|| {
            self.vocal
                .iter()
                .find(|(_, v)| !v.is_empty())
                .and_then(|(_, v)| {
                    v.iter()
                        .min_by_key(|(pos, id)| (*pos, *id))
                        .map(|(_, id)| *id)
                })
        })
    }

    fn ach_begin(&mut self, host: &mut dyn Host, mut r: Run) {
        r.awaits_finish = true;
        self.card(&r.info, Some(0.0), Status::Running);
        r.cur = 0.0;
        match self.heartbeat_channel() {
            Some(chan) => {
                let name = r.info.name.clone();
                self.say(host, Lvl::Info, "qm-achievement-try", &[("name", &name)]);
                r.key = format!("call:{chan}:{}", self.rng.rnd(1000, 9999));
                r.fails = 0;
                self.ach_iter(host, r);
            }
            None => self.ach_bypass(host, r),
        }
    }

    fn ach_iter(&mut self, host: &mut dyn Host, r: Run) {
        if !(r.cur < r.info.target && self.running) {
            return self.ach_after_loop(host, r);
        }
        let route = format!("/quests/{}/heartbeat", r.info.id);
        let body = obj(&[
            ("stream_key", Val::Str(r.key.clone())),
            ("terminal", Val::Bool(false)),
        ]);
        self.send(host, r, Purpose::AchBeat, route, body);
    }

    fn on_ach_beat(&mut self, host: &mut dyn Host, mut r: Run, res: Result<Val, ApiErr>) {
        match res {
            Ok(body) => {
                let b = Some(&body);
                r.cur = prog(b, &r.info.key)
                    .or_else(|| prog(b, "ACHIEVEMENT_IN_ACTIVITY"))
                    .unwrap_or(r.cur);
                self.card(&r.info, Some(r.cur), Status::Running);
                r.fails = 0;
                if r.cur >= r.info.target {
                    let route = format!("/quests/{}/heartbeat", r.info.id);
                    let body = obj(&[
                        ("stream_key", Val::Str(r.key.clone())),
                        ("terminal", Val::Bool(true)),
                    ]);
                    return self.send(host, r, Purpose::AchFinal, route, body);
                }
            }
            Err(e) => {
                r.fails += 1;
                if e.client() {
                    let s = e.status_text();
                    self.say(
                        host,
                        Lvl::Warn,
                        "qm-achievement-rejected",
                        &[("status", &s)],
                    );
                    return self.ach_after_loop(host, r);
                }
                if r.fails >= MAX_TASK_FAILURES {
                    self.say(host, Lvl::Warn, "qm-achievement-failures", &[]);
                    return self.ach_after_loop(host, r);
                }
            }
        }
        let ms = self.rng.rnd(19_000, 22_000);
        self.sleep(host, r, ms, Wake::AchIter);
    }

    fn ach_after_loop(&mut self, host: &mut dyn Host, r: Run) {
        if r.cur >= r.info.target && self.running {
            return self.finish(host, r);
        }
        self.ach_bypass(host, r);
    }

    /* ── ACTIVITY ── */

    fn act_begin(&mut self, host: &mut dyn Host, mut r: Run) {
        let Some(chan) = self.heartbeat_channel() else {
            return self.fail(host, r, "qm-reason-no-voice", &[]);
        };
        r.key = format!("call:{chan}:{}", self.rng.rnd(1000, 9999));
        r.cur = 0.0;
        r.fails = 0;
        self.card(&r.info, Some(0.0), Status::Running);
        r.start = self.now();
        self.act_iter(host, r);
    }

    fn act_iter(&mut self, host: &mut dyn Host, r: Run) {
        if !(r.cur < r.info.target && self.running) {
            return self.act_after_loop(host, r);
        }
        let route = format!("/quests/{}/heartbeat", r.info.id);
        let body = obj(&[
            ("stream_key", Val::Str(r.key.clone())),
            ("terminal", Val::Bool(false)),
        ]);
        self.send(host, r, Purpose::ActBeat, route, body);
    }

    fn on_act_beat(&mut self, host: &mut dyn Host, mut r: Run, res: Result<Val, ApiErr>) {
        match res {
            Ok(body) => {
                let b = Some(&body);
                r.cur = prog(b, &r.info.key)
                    .or_else(|| prog(b, "PLAY_ACTIVITY"))
                    .unwrap_or(r.cur + 20.0);
                self.card(&r.info, Some(r.cur), Status::Running);
                r.fails = 0;
                if r.cur >= r.info.target {
                    let route = format!("/quests/{}/heartbeat", r.info.id);
                    let body = obj(&[
                        ("stream_key", Val::Str(r.key.clone())),
                        ("terminal", Val::Bool(true)),
                    ]);
                    return self.send(host, r, Purpose::ActFinal, route, body);
                }
            }
            Err(e) => {
                r.fails += 1;
                if e.client() {
                    let s = e.status_text();
                    self.say(
                        host,
                        Lvl::Warn,
                        "qm-activity-unavailable",
                        &[("status", &s)],
                    );
                    return self.fail(host, r, "qm-reason-client", &[("status", &s)]);
                }
                if r.fails >= MAX_TASK_FAILURES {
                    return self.fail(host, r, "qm-reason-network", &[]);
                }
                let args = [
                    ("n", r.fails.to_string()),
                    ("max", MAX_TASK_FAILURES.to_string()),
                    ("error", e.msg()),
                ];
                let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
                self.say(host, Lvl::Debug, "qm-activity-failed", &args);
            }
        }
        if self.now().saturating_sub(r.start) > MAX_TIME_MS {
            return self.fail(host, r, "qm-reason-timeout", &[]);
        }
        let ms = self.rng.rnd(19_000, 22_000);
        self.sleep(host, r, ms, Wake::ActIter);
    }

    fn act_after_loop(&mut self, host: &mut dyn Host, r: Run) {
        if self.running && r.cur >= r.info.target {
            return self.finish(host, r);
        }
        let (id, lane) = (r.info.id.clone(), r.lane);
        self.task_end(host, &id, lane);
    }

    /* ── channel lookup (ChannelStore / GuildChannelStore) ── */

    fn observe(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                self.dms.clear();
                self.vocal.clear();
                for c in d.get("private_channels").map(Val::as_list).unwrap_or(&[]) {
                    self.add_dm(c);
                }
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.add_guild(g);
                }
            }
            "GUILD_CREATE" => self.add_guild(d),
            "GUILD_DELETE" => {
                if d.get("unavailable").and_then(Val::as_bool) != Some(true)
                    && let Some(id) = d.id_at(&["id"])
                {
                    self.vocal.retain(|(g, _)| *g != id);
                }
            }
            "CHANNEL_CREATE" => match d.id_at(&["guild_id"]) {
                Some(g) => {
                    if let Some(v) = vocal_entry(d) {
                        match self.vocal.iter_mut().find(|(id, _)| *id == g) {
                            Some((_, list)) => list.push(v),
                            None => self.vocal.push((g, vec![v])),
                        }
                    }
                }
                None => self.add_dm(d),
            },
            "CHANNEL_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.dms.retain(|(c, _)| *c != id);
                    for (_, list) in &mut self.vocal {
                        list.retain(|(_, c)| *c != id);
                    }
                }
            }
            _ => {}
        }
    }

    fn add_dm(&mut self, c: &Val) {
        if !matches!(c.get("type").and_then(Val::as_i64), Some(1 | 3)) {
            return;
        }
        if let Some(id) = c.id_at(&["id"]) {
            let last = c.id_at(&["last_message_id"]).unwrap_or(0);
            self.dms.retain(|(x, _)| *x != id);
            self.dms.push((id, last));
        }
    }

    fn add_guild(&mut self, g: &Val) {
        let Some(id) = g.id_at(&["id"]) else {
            return;
        };
        let list: Vec<(i64, u64)> = g
            .get("channels")
            .map(Val::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(vocal_entry)
            .collect();
        match self.vocal.iter_mut().find(|(x, _)| *x == id) {
            Some((_, l)) => *l = list,
            None => self.vocal.push((id, list)),
        }
    }
}

/// A guild voice or stage channel as `(position, id)` (`VOCAL` of GuildChannelStore).
fn vocal_entry(c: &Val) -> Option<(i64, u64)> {
    if !matches!(c.get("type").and_then(Val::as_i64), Some(2 | 13)) {
        return None;
    }
    let pos = c.get("position").and_then(Val::as_i64).unwrap_or(0);
    Some((pos, c.id_at(&["id"])?))
}

fn enroll_body() -> Val {
    obj(&[
        ("location", Val::Int(11)),
        ("is_targeted", Val::Bool(false)),
    ])
}

fn claim_body() -> Val {
    obj(&[
        ("platform", Val::Int(0)),
        ("location", Val::Int(11)),
        ("is_targeted", Val::Bool(false)),
        ("metadata_raw", Val::Null),
        ("metadata_sealed", Val::Null),
        ("traffic_metadata_raw", Val::Null),
        ("traffic_metadata_sealed", Val::Null),
    ])
}

/// An integer timestamp as JSON sends it.
fn ts_val(f: f64) -> Val {
    if f.fract() == 0.0 && f.abs() < 9e15 {
        Val::Int(f as i64)
    } else {
        Val::Float(f)
    }
}

/// `POST /quests/{id}/<what>` -> `(id, what)`.
fn quest_route(route: &str) -> Option<(&str, &str)> {
    let route = route.split('?').next().unwrap_or(route);
    match route
        .trim_matches('/')
        .split('/')
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["quests", id, what] if *id != "@me" => Some((id, what)),
        _ => None,
    }
}

fn is_quest_list(method: &str, route: &str) -> bool {
    method == "GET"
        && route
            .split('?')
            .next()
            .unwrap_or(route)
            .trim_end_matches('/')
            == "/quests/@me"
}

fn is_quest_answer(method: &str, route: &str) -> bool {
    method == "POST"
        && quest_route(route)
            .is_some_and(|(_, w)| matches!(w, "heartbeat" | "video-progress" | "enroll"))
}

fn is_interaction(method: &str, route: &str) -> bool {
    method == "POST" && route.trim_end_matches('/') == "/interactions"
}

/// `/phoecord quests ...`.
fn is_our_command(body: &Val) -> bool {
    body.str_at(&["data", "name"]) == Some(crate::commands::COMMAND_NAME)
        && body.at(&["data", "options"]).is_some_and(|o| {
            o.as_list()
                .iter()
                .any(|s| s.str_at(&["name"]) == Some("quests"))
        })
}

/* ── the plugin ────────────────────────────────────────────────── */

pub struct QuestMaster {
    manifest: Manifest,
    state: Shared<Engine>,
}

impl Default for QuestMaster {
    fn default() -> Self {
        Self::new()
    }
}

impl QuestMaster {
    pub fn new() -> QuestMaster {
        QuestMaster {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "QuestMaster",
                    env!("CARGO_PKG_VERSION"),
                    "Auto-complete Discord Quests (video, game, stream, activity, \
                              achievements). Fakes quest progress: Discord may void rewards or \
                              act against the account",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("QuestMaster".into());
                m.fetch = vec!["*.discordsays.com".into()];
                m
            },
            state: Shared::default(),
        }
    }

    /// `/phoecord quests <action> [quest]`: the dashboard's buttons.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "quests",
            "QuestMaster: list, start, stop, status, claim, retry, consent, deny",
            move |host, args: &Args| answer(&mut state.lock(), host, args),
        )
        .option(
            "action",
            "list, start, stop, status, claim, retry, consent, consent-always or deny",
            true,
        )
        .option(
            "quest",
            "A quest id (start, claim) or an app id (consent)",
            false,
        )
        .boxed()
    }
}

/// The command's answer; the work runs in the plugin's own scope on its `cmd` timer.
fn answer(st: &mut Engine, host: &mut dyn Host, args: &Args) -> String {
    let action = args.get("action").unwrap_or("").trim().to_ascii_lowercase();
    let quest = args.get("quest").map(str::trim).filter(|q| !q.is_empty());
    match action.as_str() {
        "list" => {
            st.pending.push(Cmd::Refresh);
            let items = st.picker_items();
            if items.is_empty() {
                return host.t("quest_master-list-empty", &[]);
            }
            let count = items.len().to_string();
            let mut out = vec![host.t("quest_master-list-header", &[("count", &count)])];
            for (id, name, kind, label, reward) in &items {
                out.push(host.t(
                    "quest_master-list-line",
                    &[
                        ("id", id),
                        ("name", name),
                        ("type", kind.label()),
                        ("label", label),
                        ("reward", reward),
                    ],
                ));
            }
            out.join("\n")
        }
        "start" => {
            if !matches!(st.phase, Phase::Idle | Phase::Picker | Phase::Loading) {
                return host.t("quest_master-already", &[]);
            }
            let sel = quest.map_or(Sel::All, |q| Sel::One(q.to_string()));
            st.pending.push(Cmd::Start(sel));
            host.t("quest_master-starting", &[])
        }
        "stop" => {
            if !st.running && !st.active() {
                return host.t("quest_master-not-running", &[]);
            }
            st.pending.push(Cmd::Stop);
            host.t("quest_master-stopping", &[])
        }
        "retry" => {
            st.pending.push(Cmd::Retry);
            host.t("quest_master-retry", &[])
        }
        "claim" => {
            let Some(id) = quest else {
                return host.t("quest_master-claim-missing", &[]);
            };
            match st.cards.get_mut(id) {
                Some(c) if c.claimable && c.claim == ClaimState::None => {
                    c.claim = ClaimState::Waiting;
                    st.pending.push(Cmd::Claim(id.to_string()));
                    host.t("quest_master-claiming", &[("id", id)])
                }
                _ => host.t("quest_master-claim-none", &[("id", id)]),
            }
        }
        "consent" | "consent-always" | "deny" => {
            let Some((_, app)) = st.consent_wait.clone() else {
                return host.t("quest_master-consent-none", &[]);
            };
            let yes = action != "deny";
            if yes && quest.is_some_and(|q| q != app) {
                return host.t("quest_master-consent-other", &[("id", &app)]);
            }
            st.pending.push(Cmd::Consent {
                app: Some(app.clone()),
                yes,
                remember: action == "consent-always",
            });
            if yes {
                host.t("quest_master-consent-yes", &[("id", &app)])
            } else {
                host.t("quest_master-consent-no", &[("id", &app)])
            }
        }
        "status" | "" => status_text(st, host),
        other => host.t("quest_master-unknown-action", &[("action", other)]),
    }
}

fn status_text(st: &Engine, host: &mut dyn Host) -> String {
    let phase = match st.phase {
        Phase::Idle => "quest_master-phase-idle",
        Phase::Loading => "quest_master-phase-loading",
        Phase::Picker => "quest_master-phase-picker",
        Phase::Running => "quest_master-phase-running",
        Phase::Finished => "quest_master-phase-finished",
    };
    let phase = host.t(phase, &[]);
    let loop_n = st.loop_n.to_string();
    let mut out = vec![host.t(
        "quest_master-status-header",
        &[("phase", &phase), ("loop", &loop_n)],
    )];
    // The dashboard's order: running by progress, then queued, failed, done.
    let mut cards: Vec<(&String, &Card)> = st.cards.iter().collect();
    let rank = |c: &Card| match c.status {
        Status::Completed | Status::Claimed => 3,
        Status::Failed => 2,
        Status::Queue | Status::Pending => 1,
        Status::Running => 0,
    };
    cards.sort_by(|a, b| {
        let pct = |c: &Card| if c.max > 0.0 { c.cur / c.max } else { 0.0 };
        rank(a.1)
            .cmp(&rank(b.1))
            .then(
                pct(b.1)
                    .partial_cmp(&pct(a.1))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.1.order.cmp(&b.1.order))
    });
    if cards.is_empty() {
        out.push(host.t("quest_master-status-empty", &[]));
    }
    for (id, c) in cards {
        let state = if c.enroll_required {
            "quest_master-st-enroll"
        } else if c.claimable && c.claim == ClaimState::Failed {
            "quest_master-st-claim-failed"
        } else if c.claimable && c.claim == ClaimState::Waiting {
            "quest_master-st-claim-waiting"
        } else if c.claimable {
            "quest_master-st-claimable"
        } else {
            match c.status {
                Status::Queue => "quest_master-st-queue",
                Status::Pending => "quest_master-st-pending",
                Status::Running => "quest_master-st-running",
                Status::Completed => "quest_master-st-completed",
                Status::Claimed => "quest_master-st-claimed",
                Status::Failed => "quest_master-st-failed",
            }
        };
        let state = host.t(state, &[("id", id)]);
        let (cur, max) = (n(c.cur.min(c.max).floor()), n(c.max));
        out.push(host.t(
            "quest_master-status-line",
            &[
                ("id", id),
                ("name", &c.name),
                ("type", c.kind.label()),
                ("state", &state),
                ("cur", &cur),
                ("max", &max),
            ],
        ));
    }
    out.join("\n")
}

impl Plugin for QuestMaster {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => *name == TIMER_EVENT || DISPATCHES.contains(name),
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::RestResponse { method, route } => {
                is_quest_list(method, route) || is_quest_answer(method, route)
            }
            EventKind::Send { op } => {
                *op == spoof::PRESENCE_UPDATE || *op == spoof::VOICE_STATE_UPDATE
            }
            _ => false,
        }
    }

    /// READY and the guild dispatches feed the heartbeat channel lookup; the presence and voice
    /// sends feed the GAME and STREAM paths; timers send the fake game's presence.
    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let mut st = self.state.lock();
        match event {
            Event::Dispatch { name, payload, .. } if name == TIMER_EVENT => {
                let tag = payload
                    .get("d")
                    .unwrap_or(payload)
                    .str_at(&["tag"])
                    .map(str::to_string);
                if let Some(tag) = tag {
                    st.on_timer(host, &tag);
                }
            }
            Event::Dispatch { name, payload, .. } if DISPATCHES.contains(&name) => {
                let d = payload.get("d").unwrap_or(payload);
                if name == "VOICE_STATE_UPDATE" {
                    if host.self_id().is_some() && d.id_at(&["user_id"]) == host.self_id() {
                        st.voice_seen(d);
                    }
                } else {
                    st.observe(name, d);
                }
                if !st.pending.is_empty() {
                    st.run_cmds(host);
                }
                // autoStart: open the dashboard (the picker) when the plugin starts.
                if name == "READY" && !st.autostarted && !st.active() {
                    st.autostarted = true;
                    if flag(host, "autoStart", true) {
                        st.open(host, None);
                    }
                }
            }
            Event::Send { op, payload } if op == spoof::VOICE_STATE_UPDATE => {
                st.voice_seen(payload.get("d").unwrap_or(payload));
            }
            Event::Send { op, payload } if op == spoof::PRESENCE_UPDATE => {
                if st.presence_sent(&*host, payload) {
                    return Verdict::Changed;
                }
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if is_our_command(body) {
                    // The `/phoecord` command runs after this plugin; act on it in our own scope.
                    let _ = host.schedule(50, CMD_TAG);
                } else if !st.pending.is_empty() {
                    st.run_cmds(host);
                }
            }
            Event::RestResponse {
                method,
                route,
                status,
                body,
            } if (200..300).contains(&status) => {
                if is_quest_list(method, route) {
                    if get(body, "quests").is_some() || matches!(body, Val::List(_)) {
                        st.set_quests(body);
                    }
                } else if let Some((id, what)) = quest_route(route) {
                    st.learn_status(id, body);
                    if what == "heartbeat" {
                        st.heartbeat_seen(host, id, body);
                    }
                }
            }
            _ => {}
        }
        Verdict::Pass
    }
}
