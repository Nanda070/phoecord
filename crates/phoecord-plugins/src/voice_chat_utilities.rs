//! VoiceChatUtilities: act on everyone in a voice channel at once (Vencord userplugin: VoiceChatUtilities).
//!
//! Ported from the owner's userplugin `VoiceChatUtilities`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/VoiceChatUtilities,
//! GPL-3.0-or-later, Copyright (c) 2024 Vendicated and contributors), modified for PhoeCord
//! 2026-11-25.
//!
//! What the script does: the channel context menu "Voice Tools" patches every member who is in
//! the voice or stage channel with `PATCH /guilds/{guild}/members/{user}`:
//! - Disconnect all: `{"channel_id": null}`; Mute all / Unmute all: `{"mute": true|false}`;
//!   Deafen all / Undeafen all: `{"deaf": true|false}`; Move all: `{"channel_id": "<target>"}`
//!   with the target one of the guild's other voice channels.
//! - Everyone but you is patched, except Move all, which moves you too (the script's `bypass`).
//! - Between the calls it waits: after the call at index `i` when `i % waitAfter == 0` it waits
//!   `waitSeconds` before going on.
//!
//! Here the members come from the voice state table kept from `READY`, `GUILD_CREATE` and
//! `VOICE_STATE_UPDATE`. A job takes its member list when it starts (sorted by user id), sends one
//! PATCH per step and schedules the next step with the script's wait (50 ms, the timer minimum,
//! when the script does not wait). A call refused for the rate limit (local `rate limited ... retry
//! in X.Xs`, or a Discord 429 with `retry_after`) is tried again after that time; any other failure
//! is logged and the job goes on with the next member, as the script's other requests had already
//! gone out.
//!
//! Switching it on: `[plugins.voice_chat_utilities] enabled = true`. The context menu is UI
//! (Phase 2); until then it is `/phoecord vcu <action> <channel> [target]`:
//! - `action`: `disconnect`, `mute`, `unmute`, `deafen`, `undeafen`, `move` (needs `target`, a
//!   voice channel of the same guild), `stop` (cancel every running job), `status`.
//! - `channel`, `target`: a channel id or a `<#id>` mention.
//!
//! Settings (`[plugins.voice_chat_utilities]`, the script's names and defaults): `waitAfter`
//! (`5`, actions before waiting), `waitSeconds` (`2`, seconds to wait).
//!
//! How the command reaches the job: a `/phoecord` subcommand runs inside the command plugin, whose
//! timers and REST calls would belong to it. So this plugin reads the `POST /interactions` of its
//! own `vcu` subcommand (it must sit before the command plugin in the chain), does the work as
//! itself, and the subcommand only hands back the text.
//!
//! Not ported: the context menu itself (UI, Phase 2). The script sends all its requests at once
//! and only awaits them one after another, so its waits never delayed a request; here each wait
//! delays the next PATCH, as asked for the port. The member order is by user id, not the order
//! the client's store happened to hold them.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Guilds, Shared, VoiceStates, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, TIMER_EVENT, Val, Verdict};

const PLUGIN_ID: &str = "voice_chat_utilities";
/// Shortest timer the host accepts.
const MIN_MS: u64 = 50;
/// Discord channel types the script's menu is shown for: voice and stage.
const VOICE_KINDS: [i64; 2] = [2, 13];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Action {
    Disconnect,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    Move,
}

impl Action {
    fn parse(s: &str) -> Option<Action> {
        Some(match s {
            "disconnect" | "disconnect-all" => Action::Disconnect,
            "mute" | "mute-all" => Action::Mute,
            "unmute" | "unmute-all" => Action::Unmute,
            "deafen" | "deafen-all" => Action::Deafen,
            "undeafen" | "undeafen-all" => Action::Undeafen,
            "move" | "move-all" => Action::Move,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Action::Disconnect => "disconnect",
            Action::Mute => "mute",
            Action::Unmute => "unmute",
            Action::Deafen => "deafen",
            Action::Undeafen => "undeafen",
            Action::Move => "move",
        }
    }

    /// The script's PATCH body (`target` is the channel of Move all).
    fn body(self, target: Option<u64>) -> Val {
        let mut b = Val::map();
        match self {
            Action::Disconnect => b.set("channel_id", Val::Null),
            Action::Mute => b.set("mute", Val::Bool(true)),
            Action::Unmute => b.set("mute", Val::Bool(false)),
            Action::Deafen => b.set("deaf", Val::Bool(true)),
            Action::Undeafen => b.set("deaf", Val::Bool(false)),
            Action::Move => b.set(
                "channel_id",
                target.map(|t| Val::Str(t.to_string())).unwrap_or(Val::Null),
            ),
        }
        b
    }
}

struct Job {
    id: u64,
    action: Action,
    channel: u64,
    guild: u64,
    body: Val,
    users: Vec<u64>,
    /// Index of the next member to patch.
    next: usize,
    failed: usize,
}

#[derive(Default)]
struct State {
    voice: VoiceStates,
    guilds: Guilds,
    jobs: Vec<Job>,
    next_id: u64,
    /// The text for the subcommand to show, set when the interaction is read.
    reply: Option<String>,
}

pub struct VoiceChatUtilities {
    manifest: Manifest,
    state: Shared<State>,
}

/// A channel id from `123` or `<#123>`.
fn parse_channel(s: &str) -> Option<u64> {
    s.trim()
        .trim_start_matches("<#")
        .trim_end_matches('>')
        .parse()
        .ok()
}

fn tag(id: u64) -> String {
    format!("vcu:{id}")
}

fn number(v: Option<Val>) -> Option<f64> {
    match v? {
        Val::Int(n) => Some(n as f64),
        Val::Big(n) => Some(n as f64),
        Val::Float(f) => Some(f),
        _ => None,
    }
}

/// The script's `waitAfter`, at least 1.
fn wait_after(host: &dyn Host) -> u64 {
    number(host.setting("waitAfter"))
        .filter(|n| n.is_finite() && *n >= 1.0)
        .unwrap_or(5.0) as u64
}

/// The script's `waitSeconds` in milliseconds.
fn wait_ms(host: &dyn Host) -> u64 {
    let s = number(host.setting("waitSeconds"))
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(2.0);
    (s * 1000.0).round() as u64
}

/// X of a local refusal `rate limited: ..., retry in X.Xs`, in milliseconds.
fn retry_from_error(e: &str) -> Option<u64> {
    if !e.contains("rate limited") {
        return None;
    }
    let rest = e.split("retry in ").nth(1)?;
    let secs: f64 = rest
        .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .trim_end_matches('s')
        .parse()
        .ok()?;
    Some((secs * 1000.0).ceil() as u64)
}

/// `retry_after` (seconds) of a Discord 429 answer, in milliseconds.
fn retry_from_429(body: Option<&Val>) -> u64 {
    number(body.and_then(|b| b.get("retry_after")).cloned())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|s| (s * 1000.0).ceil() as u64)
        .unwrap_or(1000)
}

impl State {
    fn guild_of(&self, host: &dyn Host, channel: u64) -> Option<u64> {
        host.channel_guild(channel)
            .or_else(|| self.guilds.channel(channel).map(|c| c.guild_id))
    }

    /// `Err(key)` if the channel is known and is not a voice or stage channel.
    fn check_voice(&self, channel: u64) -> bool {
        self.guilds
            .channel(channel)
            .is_none_or(|c| VOICE_KINDS.contains(&c.kind))
    }

    /// One step of a job: PATCH the next member and schedule the one after it.
    fn step(&mut self, host: &mut dyn Host, id: u64) {
        let Some(pos) = self.jobs.iter().position(|j| j.id == id) else {
            return;
        };
        let (user, guild, body, i) = {
            let j = &self.jobs[pos];
            (j.users[j.next], j.guild, j.body.clone(), j.next)
        };
        let route = format!("/guilds/{guild}/members/{user}");
        let retry = match host.rest("PATCH", &route, Some(&body)) {
            Err(e) => match retry_from_error(&e) {
                Some(ms) => Some(ms),
                None => {
                    self.failed(host, pos, user, &e);
                    None
                }
            },
            Ok(r) if r.status == 429 => Some(retry_from_429(r.body.as_ref())),
            Ok(r) if r.status >= 400 => {
                self.failed(host, pos, user, &format!("status {}", r.status));
                None
            }
            Ok(_) => None,
        };
        if let Some(ms) = retry {
            self.schedule(host, pos, ms.max(MIN_MS));
            return;
        }
        let after = wait_after(&*host);
        let wait = if (i as u64).is_multiple_of(after) {
            wait_ms(&*host)
        } else {
            0
        };
        let job = &mut self.jobs[pos];
        job.next += 1;
        if job.next >= job.users.len() {
            let (action, channel, n, bad) = (job.action, job.channel, job.users.len(), job.failed);
            self.jobs.remove(pos);
            host.log(&host.t(
                "voice_chat_utilities-done",
                &[
                    ("action", action.name()),
                    ("channel", &channel.to_string()),
                    ("count", &n.to_string()),
                    ("failed", &bad.to_string()),
                ],
            ));
            return;
        }
        self.schedule(host, pos, wait.max(MIN_MS));
    }

    fn failed(&mut self, host: &mut dyn Host, pos: usize, user: u64, why: &str) {
        self.jobs[pos].failed += 1;
        host.log(&host.t(
            "voice_chat_utilities-failed",
            &[("user", &user.to_string()), ("error", why)],
        ));
    }

    fn schedule(&mut self, host: &mut dyn Host, pos: usize, ms: u64) {
        let id = self.jobs[pos].id;
        if let Err(e) = host.schedule(ms, &tag(id)) {
            host.log(&host.t(
                "voice_chat_utilities-failed",
                &[("user", "-"), ("error", &e)],
            ));
            self.jobs.remove(pos);
        }
    }

    fn start(&mut self, host: &mut dyn Host, args: &Args, action: Action) -> String {
        let Some(channel) = args.get("channel").and_then(parse_channel) else {
            return host.t("voice_chat_utilities-need-channel", &[]);
        };
        let Some(guild) = self.guild_of(&*host, channel) else {
            return host.t(
                "voice_chat_utilities-unknown-channel",
                &[("channel", &channel.to_string())],
            );
        };
        if !self.check_voice(channel) {
            return host.t(
                "voice_chat_utilities-not-voice",
                &[("channel", &channel.to_string())],
            );
        }
        let mut target = None;
        if action == Action::Move {
            let Some(t) = args.get("target").and_then(parse_channel) else {
                return host.t("voice_chat_utilities-need-target", &[]);
            };
            if t == channel || self.guild_of(&*host, t) != Some(guild) || !self.check_voice(t) {
                return host.t(
                    "voice_chat_utilities-bad-target",
                    &[("channel", &t.to_string())],
                );
            }
            target = Some(t);
        }
        let all = self.voice.in_channel(channel);
        if all.is_empty() {
            return host.t(
                "voice_chat_utilities-empty",
                &[("channel", &channel.to_string())],
            );
        }
        // Everyone but you, except Move all (the script's `bypass`).
        let me = host.self_id();
        let users: Vec<u64> = all
            .into_iter()
            .filter(|u| action == Action::Move || Some(*u) != me)
            .collect();
        if users.is_empty() {
            return host.t(
                "voice_chat_utilities-nobody",
                &[("channel", &channel.to_string())],
            );
        }
        self.next_id += 1;
        let id = self.next_id;
        let count = users.len();
        self.jobs.push(Job {
            id,
            action,
            channel,
            guild,
            body: action.body(target),
            users,
            next: 0,
            failed: 0,
        });
        self.step(host, id);
        host.t(
            "voice_chat_utilities-started",
            &[
                ("action", action.name()),
                ("channel", &channel.to_string()),
                ("count", &count.to_string()),
            ],
        )
    }

    fn stop(&mut self, host: &mut dyn Host) -> String {
        let n = self.jobs.len();
        for j in self.jobs.drain(..) {
            host.cancel_schedule(&tag(j.id));
        }
        host.t("voice_chat_utilities-stopped", &[("count", &n.to_string())])
    }

    fn status(&self, host: &dyn Host) -> String {
        if self.jobs.is_empty() {
            return host.t("voice_chat_utilities-idle", &[]);
        }
        self.jobs
            .iter()
            .map(|j| {
                host.t(
                    "voice_chat_utilities-status-line",
                    &[
                        ("action", j.action.name()),
                        ("channel", &j.channel.to_string()),
                        ("done", &j.next.to_string()),
                        ("count", &j.users.len().to_string()),
                    ],
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl VoiceChatUtilities {
    pub fn new() -> VoiceChatUtilities {
        VoiceChatUtilities {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "VoiceChatUtilities",
                    env!("CARGO_PKG_VERSION"),
                    "Move, mute, deafen or disconnect everyone in a voice channel",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("VoiceChatUtilities".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord vcu` command, sharing this plugin's jobs. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "vcu",
            "Act on everyone in a voice channel (disconnect, mute, unmute, deafen, undeafen, move)",
            move |host, _: &Args| {
                state
                    .lock()
                    .reply
                    .take()
                    .unwrap_or_else(|| host.t("voice_chat_utilities-no-reply", &[]))
            },
        )
        .option(
            "action",
            "disconnect, mute, unmute, deafen, undeafen, move, stop or status",
            true,
        )
        .option("channel", "Voice channel id or mention", false)
        .option(
            "target",
            "Move only: the channel to move everyone to",
            false,
        )
        .boxed()
    }
}

/// `POST /interactions`.
fn is_interaction(method: &str, route: &str) -> bool {
    method == "POST" && route.trim_end_matches('/') == "/interactions"
}

fn text_of(v: &Val) -> Option<String> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

/// `(subcommand, options)` of a `/phoecord <sub>` interaction.
fn parse_interaction(body: &Val) -> Option<(String, Args)> {
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
    Some((name, Args { options }))
}

impl State {
    fn handle(&mut self, host: &mut dyn Host, args: &Args) -> String {
        let raw = args.get("action").unwrap_or("").trim().to_ascii_lowercase();
        match raw.as_str() {
            "stop" => self.stop(host),
            "status" => self.status(&*host),
            other => match Action::parse(other) {
                Some(a) => self.start(host, args, a),
                None => host.t("voice_chat_utilities-bad-action", &[("action", other)]),
            },
        }
    }
}

impl Default for VoiceChatUtilities {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    name == TIMER_EVENT || is_one_of(name, VoiceStates::EVENTS) || is_one_of(name, Guilds::EVENTS)
}

impl Plugin for VoiceChatUtilities {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Dispatch { name } => wanted(name),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = &event
            && is_interaction(method, route)
        {
            if let Some((sub, args)) = parse_interaction(body)
                && sub == "vcu"
                && host.plugin_enabled(PLUGIN_ID) != Some(false)
            {
                let mut st = self.state.lock();
                let reply = st.handle(host, &args);
                st.reply = Some(reply);
            }
            return Verdict::Pass;
        }
        if let Event::Dispatch { name, payload, .. } = event {
            // A gateway frame is `{"op","t","d"}`; accept a bare `d` too.
            let d = payload.get("d").unwrap_or(&*payload);
            let mut st = self.state.lock();
            if name == TIMER_EVENT {
                if let Some(id) = d
                    .str_at(&["tag"])
                    .and_then(|t| t.strip_prefix("vcu:"))
                    .and_then(|n| n.parse().ok())
                {
                    st.step(host, id);
                }
            } else {
                if is_one_of(name, Guilds::EVENTS) {
                    st.guilds.observe(name, d);
                }
                if is_one_of(name, VoiceStates::EVENTS) {
                    st.voice.observe(name, d);
                }
            }
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::Chain;

    const ME: u64 = 1;
    const SRC: u64 = 100;
    const DST: u64 = 200;
    const GUILD: u64 = 9;

    fn setup() -> (Chain, Rec, Box<dyn Subcommand>) {
        let p = VoiceChatUtilities::new();
        let cmd = p.command();
        let mut h = Rec {
            self_id: Some(ME),
            ..Rec::default()
        };
        let chain = Chain::new(vec![Box::new(p)]);
        let mut ready = j(&format!(
            r#"{{"op":0,"t":"GUILD_CREATE","d":{{"id":"{GUILD}","name":"G",
            "channels":[{{"id":"{SRC}","type":2,"name":"a"}},{{"id":"{DST}","type":13,"name":"b"}},{{"id":"300","type":0,"name":"t"}}],
            "voice_states":[{{"user_id":"1","channel_id":"{SRC}"}},{{"user_id":"7","channel_id":"{SRC}"}},{{"user_id":"8","channel_id":"{SRC}"}}]}}}}"#
        ));
        chain.dispatch(&mut h, "GUILD_CREATE", &mut ready, &mut Vec::new());
        (chain, h, cmd)
    }

    fn args(pairs: &[(&str, &str)]) -> Args {
        Args {
            options: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// Run `/phoecord vcu` as the client does: the interaction goes through the chain, then the
    /// subcommand hands back the text.
    fn go(chain: &Chain, h: &mut Rec, cmd: &dyn Subcommand, a: &Args) -> String {
        let opts: Vec<String> = a
            .options
            .iter()
            .map(|(k, v)| format!(r#"{{"type":3,"name":"{k}","value":"{v}"}}"#))
            .collect();
        let mut body = j(&format!(
            r#"{{"type":2,"data":{{"name":"phoecord","options":[{{"type":1,"name":"vcu","options":[{}]}}]}}}}"#,
            opts.join(",")
        ));
        chain.rest_request(h, "POST", "/interactions", Some(&mut body));
        cmd.run(h, a)
    }

    fn tick(chain: &Chain, h: &mut Rec, tag: &str) {
        let mut f = Rec::timer_frame(tag);
        chain.dispatch(h, TIMER_EVENT, &mut f, &mut Vec::new());
    }

    #[test]
    fn mute_patches_everyone_but_me_one_step_at_a_time() {
        let (chain, mut h, cmd) = setup();
        let out = go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "mute"), ("channel", "100")]),
        );
        assert!(out.contains("mute"));
        // First call at once, body as the script's.
        assert_eq!(h.rest_calls.len(), 1);
        assert_eq!(h.rest_calls[0].0, "PATCH /guilds/9/members/7");
        assert_eq!(h.rest_calls[0].1, Some(j(r#"{"mute":true}"#)));
        // i = 0: 0 % 5 == 0, so the script's wait (2 s) comes before the next call.
        assert_eq!(h.scheduled, vec![(2000, "vcu:1".to_string())]);
        tick(&chain, &mut h, "vcu:1");
        assert_eq!(h.rest_calls[1].0, "PATCH /guilds/9/members/8");
        // Last member done: no more timers, job gone.
        assert_eq!(h.scheduled.len(), 1);
        assert!(go(&chain, &mut h, cmd.as_ref(), &args(&[("action", "status")])).contains("No"));
    }

    #[test]
    fn move_includes_me_and_sends_the_target() {
        let (chain, mut h, cmd) = setup();
        h.settings.insert("waitSeconds".into(), Val::Float(0.5));
        h.settings.insert("waitAfter".into(), Val::Int(2));
        go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "move"), ("channel", "<#100>"), ("target", "200")]),
        );
        assert_eq!(h.rest_calls[0].0, "PATCH /guilds/9/members/1");
        assert_eq!(h.rest_calls[0].1, Some(j(r#"{"channel_id":"200"}"#)));
        assert_eq!(h.scheduled[0].0, 500);
        tick(&chain, &mut h, "vcu:1");
        // i = 1: 1 % 2 != 0, no wait: the timer minimum.
        assert_eq!(h.scheduled[1].0, MIN_MS);
        tick(&chain, &mut h, "vcu:1");
        assert_eq!(h.rest_calls.len(), 3);
    }

    #[test]
    fn disconnect_deafen_undeafen_unmute_bodies() {
        for (a, body) in [
            ("disconnect", r#"{"channel_id":null}"#),
            ("deafen", r#"{"deaf":true}"#),
            ("undeafen", r#"{"deaf":false}"#),
            ("unmute", r#"{"mute":false}"#),
        ] {
            let (chain, mut h, cmd) = setup();
            go(
                &chain,
                &mut h,
                cmd.as_ref(),
                &args(&[("action", a), ("channel", "100")]),
            );
            assert_eq!(h.rest_calls[0].1, Some(j(body)), "{a}");
        }
    }

    #[test]
    fn rate_limits_retry_the_same_member() {
        let (chain, mut h, cmd) = setup();
        h = h.answer(
            "PATCH",
            "/guilds/9/members/7",
            429,
            r#"{"retry_after":1.5}"#,
        );
        go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "mute"), ("channel", "100")]),
        );
        assert_eq!(h.scheduled, vec![(1500, "vcu:1".to_string())]);
        tick(&chain, &mut h, "vcu:1");
        // Still member 7: it was not skipped.
        assert_eq!(h.rest_calls[1].0, "PATCH /guilds/9/members/7");
        assert_eq!(
            retry_from_error(
                "rate limited: at most 5 Discord API calls per 5 s per plugin, retry in 2.3s"
            ),
            Some(2300)
        );
        assert_eq!(retry_from_error("network down"), None);
    }

    #[test]
    fn failures_are_logged_and_the_job_goes_on() {
        let (chain, mut h, cmd) = setup();
        h = h.answer("PATCH", "/guilds/9/members/7", 403, "{}");
        go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "mute"), ("channel", "100")]),
        );
        assert!(h.logs.iter().any(|l| l.contains('7')));
        tick(&chain, &mut h, "vcu:1");
        assert_eq!(h.rest_calls.len(), 2);
    }

    #[test]
    fn stop_status_and_bad_input_do_not_panic() {
        let (chain, mut h, cmd) = setup();
        go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "mute"), ("channel", "100")]),
        );
        assert!(go(&chain, &mut h, cmd.as_ref(), &args(&[("action", "status")])).contains("100"));
        go(&chain, &mut h, cmd.as_ref(), &args(&[("action", "stop")]));
        assert_eq!(h.cancelled, vec!["vcu:1".to_string()]);
        tick(&chain, &mut h, "vcu:1");
        assert_eq!(h.rest_calls.len(), 1);
        for a in [
            args(&[("action", "dance"), ("channel", "100")]),
            args(&[("action", "mute")]),
            args(&[("action", "mute"), ("channel", "abc")]),
            args(&[("action", "mute"), ("channel", "555")]),
            args(&[("action", "mute"), ("channel", "300")]),
            args(&[("action", "move"), ("channel", "100")]),
            args(&[("action", "move"), ("channel", "100"), ("target", "100")]),
            args(&[("action", "move"), ("channel", "100"), ("target", "300")]),
            args(&[("action", "mute"), ("channel", "200")]),
            args(&[]),
        ] {
            go(&chain, &mut h, cmd.as_ref(), &a);
        }
        assert_eq!(h.rest_calls.len(), 1);
        for f in [
            r#"{"op":0,"t":"PHOECORD_TIMER","d":null}"#,
            r#"{"op":0,"t":"PHOECORD_TIMER","d":{"tag":"vcu:x"}}"#,
        ] {
            chain.dispatch(&mut h, TIMER_EVENT, &mut j(f), &mut Vec::new());
        }
    }

    #[test]
    fn voice_states_follow_the_gateway() {
        let (chain, mut h, cmd) = setup();
        let mut left = j(&format!(
            r#"{{"op":0,"t":"VOICE_STATE_UPDATE","d":{{"user_id":"7","guild_id":"{GUILD}","channel_id":null}}}}"#
        ));
        chain.dispatch(&mut h, "VOICE_STATE_UPDATE", &mut left, &mut Vec::new());
        go(
            &chain,
            &mut h,
            cmd.as_ref(),
            &args(&[("action", "mute"), ("channel", "100")]),
        );
        assert_eq!(h.rest_calls[0].0, "PATCH /guilds/9/members/8");
    }

    #[test]
    fn the_job_runs_as_vcu_even_with_the_command_plugin_in_the_chain() {
        use crate::commands::{Commands, PluginInfo};
        let p = VoiceChatUtilities::new();
        let info = PluginInfo::of(&p);
        let cmd = p.command();
        let commands = Commands::new(vec![info]).with(cmd);
        // VCU before the command plugin, as builtin() must order them.
        let chain = Chain::new(vec![Box::new(p), Box::new(commands)]);
        let mut h = Rec {
            self_id: Some(ME),
            ..Rec::default()
        };
        let mut g = j(&format!(
            r#"{{"op":0,"t":"GUILD_CREATE","d":{{"id":"{GUILD}","name":"G","channels":[{{"id":"{SRC}","type":2,"name":"a"}}],
            "voice_states":[{{"user_id":"7","channel_id":"{SRC}"}},{{"user_id":"8","channel_id":"{SRC}"}}]}}}}"#
        ));
        chain.dispatch(&mut h, "GUILD_CREATE", &mut g, &mut Vec::new());
        let mut body = j(
            r#"{"type":2,"channel_id":"5","data":{"name":"phoecord","options":[{"type":1,"name":"vcu","options":[
            {"type":3,"name":"action","value":"mute"},{"type":3,"name":"channel","value":"100"}]}]}}"#,
        );
        chain.rest_request(&mut h, "POST", "/interactions", Some(&mut body));
        // The PATCH and the next-step timer were issued by VCU's own event, once.
        assert_eq!(h.rest_calls.len(), 1);
        assert_eq!(h.rest_calls[0].0, "PATCH /guilds/9/members/7");
        assert_eq!(h.scheduled, vec![(2000, "vcu:1".to_string())]);
        // The commands plugin showed VCU's stored text.
        let shown = format!("{:?}", h.dispatched);
        assert!(shown.contains("Started mute"), "{shown}");
        tick(&chain, &mut h, "vcu:1");
        assert_eq!(h.rest_calls.len(), 2);
    }
}
