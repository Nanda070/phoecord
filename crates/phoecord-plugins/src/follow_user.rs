//! FollowUser: follow a user from voice channel to voice channel (Vencord userplugin: FollowUser).
//!
//! Ported from the owner's userplugin `FollowUser`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/FollowUser, GPL-3.0-or-later,
//! Copyright (c) 2024 Vendicated and contributors), modified for PhoeCord 2026-11-25.
//!
//! What the script does: it watches voice state changes. When the followed user joins or moves to
//! a channel you are not in, it joins that channel; when they leave voice and `followLeave` is on,
//! it disconnects; with `autoMoveBack` it follows again after you were moved; with `channelFull`
//! it tries again when a full channel of the followed user gets a free slot. Here the voice
//! channel table is kept from `READY`, `GUILD_CREATE` and `VOICE_STATE_UPDATE`, and joining is a
//! queued gateway op 4 (`guild_id`, `channel_id`, your last `self_mute` and `self_deaf`); leaving
//! is the same with `channel_id: null`. The script's toasts are written to the log.
//!
//! Switching it on: `[plugins.follow_user] enabled = true`. Picking the user (the context menu
//! "Follow User" and the header icon are UI, Phase 2):
//! - setting `followUserId` (a user id), or
//! - `/phoecord follow <user>` (an id or a mention; sets the target, and joins them at once when
//!   `executeOnFollow` is on), `/phoecord follow` with no user (the header icon's click: join the
//!   followed user's channel now), `/phoecord unfollow` (the right-click). The command's choice
//!   lives in memory and wins over the setting until the proxy restarts.
//!
//! Settings (`[plugins.follow_user]`, the script's names and defaults): `executeOnFollow` (`true`),
//! `onlyManualTrigger` (`false`), `followLeave` (`false`), `autoMoveBack` (`false`),
//! `followUserId` (`""`), `channelFull` (`true`).
//!
//! Not ported: the context menu item, the header icon and tooltip, toasts as on-screen toasts
//! (UI, Phase 2); the CONNECT and MOVE_MEMBERS permission checks (the host exposes no
//! permissions: CONNECT is assumed, MOVE_MEMBERS is assumed missing); the choice made with the
//! command is not written back to `followUserId`. The commands
//! queue their op 4 themselves and it goes out at once.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Shared, VoiceChange, VoiceStates, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;

const PLUGIN_ID: &str = "follow_user";
/// Gateway op 4: voice state update.
const VOICE_STATE_UPDATE: i64 = 4;
/// Dispatches that carry a channel's `user_limit`.
const CHANNEL_EVENTS: &[&str] = &["CHANNEL_CREATE", "CHANNEL_UPDATE", "CHANNEL_DELETE"];

#[derive(Default)]
struct State {
    voice: VoiceStates,
    /// channel -> `user_limit` (only channels with a limit).
    limits: HashMap<u64, i64>,
    /// Your last `self_mute` and `self_deaf` (the client's own op 4).
    mute: bool,
    deaf: bool,
    /// The target chosen with a command; `Some("")` means unfollowed. `None` = use the setting.
    target: Option<String>,
}

/// A user id from `123` or `<@123>` / `<@!123>`.
fn parse_user(s: &str) -> Option<u64> {
    s.trim()
        .trim_start_matches("<@")
        .trim_start_matches('!')
        .trim_end_matches('>')
        .parse()
        .ok()
}

fn flag(host: &dyn Host, key: &str, default: bool) -> bool {
    host.setting(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

fn id_val(id: Option<u64>) -> Val {
    id.map(|i| Val::Str(i.to_string())).unwrap_or(Val::Null)
}

impl State {
    /// The followed user: the command's choice, else `followUserId`.
    fn followed(&self, host: &dyn Host) -> Option<u64> {
        match &self.target {
            Some(t) => t.parse().ok(),
            None => host
                .setting("followUserId")
                .and_then(|v| v.as_str().and_then(|s| s.trim().parse().ok())),
        }
    }

    fn learn_limits(&mut self, channels: &[Val]) {
        for c in channels {
            let Some(id) = c.id_at(&["id"]) else { continue };
            match c.get("user_limit").and_then(Val::as_i64) {
                Some(n) if n > 0 => self.limits.insert(id, n),
                _ => self.limits.remove(&id),
            };
        }
    }

    fn observe(&mut self, name: &str, d: &Val) -> Option<VoiceChange> {
        match name {
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.learn_limits(g.get("channels").map(Val::as_list).unwrap_or(&[]));
                }
            }
            "GUILD_CREATE" => {
                self.learn_limits(d.get("channels").map(Val::as_list).unwrap_or(&[]));
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => self.learn_limits(std::slice::from_ref(d)),
            "CHANNEL_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.limits.remove(&id);
                }
            }
            _ => {}
        }
        if is_one_of(name, VoiceStates::EVENTS) {
            return self.voice.observe(name, d);
        }
        None
    }

    fn my_channel(&self, host: &dyn Host) -> Option<u64> {
        host.self_id().and_then(|m| self.voice.channel_of(m))
    }

    /// The script's `triggerFollow(userChannelId)`; `None` means "the followed user is in no channel".
    fn trigger(&mut self, host: &mut dyn Host, channel: Option<u64>) {
        let mine = self.my_channel(&*host);
        match (channel, mine) {
            (Some(ch), mine) if mine != Some(ch) => {
                let limit = self.limits.get(&ch).copied().unwrap_or(0);
                let count = self.voice.in_channel(ch).len() as i64;
                // MOVE_MEMBERS is not known here: a full channel always stops the join.
                if limit != 0 && count >= limit {
                    let text = host.t("follow-full", &[]);
                    host.log(&text);
                    return;
                }
                let mut d = Val::map();
                d.set("guild_id", id_val(host.channel_guild(ch)));
                d.set("channel_id", id_val(Some(ch)));
                d.set("self_mute", Val::Bool(self.mute));
                d.set("self_deaf", Val::Bool(self.deaf));
                host.emit_send(VOICE_STATE_UPDATE, d);
                let text = host.t("follow-joined", &[]);
                host.log(&text);
            }
            (Some(_), _) => {
                let text = host.t("follow-same", &[]);
                host.log(&text);
            }
            (None, Some(mine)) => {
                if flag(&*host, "followLeave", false) {
                    let mut d = Val::map();
                    d.set("guild_id", id_val(host.channel_guild(mine)));
                    d.set("channel_id", Val::Null);
                    d.set("self_mute", Val::Bool(self.mute));
                    d.set("self_deaf", Val::Bool(self.deaf));
                    host.emit_send(VOICE_STATE_UPDATE, d);
                    let text = host.t("follow-left", &[]);
                    host.log(&text);
                } else {
                    let text = host.t("follow-left-stay", &[]);
                    host.log(&text);
                }
            }
            (None, None) => {
                let text = host.t("follow-not-in-voice", &[]);
                host.log(&text);
            }
        }
    }

    /// `triggerFollow()` with its default: the followed user's current channel.
    fn trigger_now(&mut self, host: &mut dyn Host) {
        if let Some(f) = self.followed(&*host) {
            let ch = self.voice.channel_of(f);
            self.trigger(host, ch);
        }
    }

    /// The script's `VOICE_STATE_UPDATES` handler for one change.
    fn voice_change(&mut self, host: &mut dyn Host, change: VoiceChange) {
        if flag(&*host, "onlyManualTrigger", false) {
            return;
        }
        let Some(followed) = self.followed(&*host) else {
            return;
        };
        let (user, new, old) = match change {
            VoiceChange::Joined { user, channel } => (user, Some(channel), None),
            VoiceChange::Left { user, channel } => (user, None, Some(channel)),
            VoiceChange::Moved { user, from, to } => (user, Some(to), Some(from)),
        };
        let is_me = host.self_id() == Some(user);
        // Moved: go back to the followed user.
        if flag(&*host, "autoMoveBack", false) && is_me && new.is_some() && old.is_some() {
            self.trigger_now(host);
            return;
        }
        // Someone left a full channel the followed user sits in: try to get in.
        if flag(&*host, "channelFull", true)
            && !is_me
            && new.is_none()
            && let Some(old) = old
            && Some(old) != self.my_channel(&*host)
        {
            let limit = self.limits.get(&old).copied().unwrap_or(0);
            let users = self.voice.in_channel(old);
            // MOVE_MEMBERS is not known here: it is taken as missing.
            if limit != 0 && users.len() as i64 == limit - 1 && users.contains(&followed) {
                self.trigger(host, Some(old));
                return;
            }
        }
        if user != followed {
            return;
        }
        match (new, old) {
            // moved or joined a channel -> also join
            (Some(ch), _) => self.trigger(host, Some(ch)),
            // left -> disconnect
            (None, Some(_)) => self.trigger(host, None),
            (None, None) => {}
        }
    }
}

pub struct FollowUser {
    manifest: Manifest,
    state: Shared<State>,
}

impl FollowUser {
    pub fn new() -> FollowUser {
        FollowUser {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "FollowUser",
                    env!("CARGO_PKG_VERSION"),
                    "Follow a user from voice channel to voice channel",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("FollowUser".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord follow` and `/phoecord unfollow` commands, sharing this plugin's state.
    /// Register both with `Commands::with`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        let on = self.state.clone();
        let follow = FnCmd::new(
            PLUGIN_ID,
            "follow",
            "Follow a user into voice channels (no user: join them now)",
            move |host, args: &Args| {
                let mut st = on.lock();
                match args.get("user").map(str::trim).filter(|s| !s.is_empty()) {
                    None => {
                        st.trigger_now(host);
                        host.t("follow_user-trigger", &[])
                    }
                    Some(raw) => match parse_user(raw) {
                        Some(id) => {
                            st.target = Some(id.to_string());
                            if flag(&*host, "executeOnFollow", true) {
                                st.trigger_now(host);
                            }
                            host.t("follow_user-following", &[("id", &id.to_string())])
                        }
                        None => host.t("follow_user-bad-user", &[("user", raw)]),
                    },
                }
            },
        )
        .option(
            "user",
            "User id or mention; leave empty to join them now",
            false,
        )
        .boxed();
        let off = self.state.clone();
        let unfollow = FnCmd::new(
            PLUGIN_ID,
            "unfollow",
            "Stop following the user",
            move |host, _: &Args| {
                let mut st = off.lock();
                st.target = Some(String::new());
                host.t("follow_user-unfollowed", &[])
            },
        )
        .boxed();
        vec![follow, unfollow]
    }
}

impl Default for FollowUser {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, VoiceStates::EVENTS) || is_one_of(name, CHANNEL_EVENTS)
}

impl Plugin for FollowUser {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => wanted(name),
            EventKind::Send { op } => *op == VOICE_STATE_UPDATE,
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if wanted(name) => {
                // A gateway frame is `{"op","t","d"}`; accept a bare `d` too.
                let d = payload.get("d").unwrap_or(&*payload);
                let mut st = self.state.lock();
                let change = st.observe(name, d);
                if let Some(change) = change {
                    st.voice_change(host, change);
                }
            }
            Event::Send {
                op: VOICE_STATE_UPDATE,
                payload,
            } => {
                let mut st = self.state.lock();
                if let Some(d) = payload.get("d").filter(|d| matches!(d, Val::Map(_))) {
                    st.mute = d.get("self_mute").and_then(Val::as_bool) == Some(true);
                    st.deaf = d.get("self_deaf").and_then(Val::as_bool) == Some(true);
                }
            }
            _ => {}
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
    const BOB: u64 = 7;

    fn setup() -> (Chain, Rec, Vec<Box<dyn Subcommand>>) {
        let p = FollowUser::new();
        let cmds = p.commands();
        let mut h = Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
        .with("followUserId", Val::Str(BOB.to_string()));
        let chain = Chain::new(vec![Box::new(p)]);
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"10","channels":[{"id":"20","type":2,"user_limit":0},{"id":"21","type":2,"user_limit":2},
               {"id":"22","type":2,"user_limit":0}],
               "voice_states":[{"user_id":"1","channel_id":"22"}]}"#,
        );
        (chain, h, cmds)
    }

    fn feed(chain: &Chain, h: &mut Rec, name: &str, d: &str) {
        let mut f = j(&format!(r#"{{"op":0,"t":"{name}","d":{d}}}"#));
        chain.dispatch(h, name, &mut f, &mut Vec::new());
    }

    fn voice(chain: &Chain, h: &mut Rec, user: u64, ch: Option<u64>) {
        let c = ch.map_or("null".to_string(), |c| format!("\"{c}\""));
        feed(
            chain,
            h,
            "VOICE_STATE_UPDATE",
            &format!(r#"{{"guild_id":"10","user_id":"{user}","channel_id":{c}}}"#),
        );
    }

    fn joined(h: &Rec, ch: &str) -> bool {
        h.sent
            .iter()
            .any(|(op, d)| *op == 4 && d.str_at(&["channel_id"]) == Some(ch))
    }

    #[test]
    fn follows_the_user_into_a_new_channel_with_the_script_fields() {
        let (chain, mut h, _) = setup();
        chain.send(
            &mut h,
            4,
            &mut j(r#"{"op":4,"d":{"channel_id":"22","self_mute":true,"self_deaf":false}}"#),
        );
        voice(&chain, &mut h, BOB, Some(20));
        assert_eq!(h.sent.len(), 1);
        let (op, d) = &h.sent[0];
        assert_eq!(*op, 4);
        assert_eq!(d.str_at(&["channel_id"]), Some("20"));
        assert_eq!(d.at(&["self_mute"]), Some(&Val::Bool(true)));
        assert_eq!(d.at(&["self_deaf"]), Some(&Val::Bool(false)));
        // Same channel as me: nothing more to do.
        h.sent.clear();
        voice(&chain, &mut h, ME, Some(20));
        voice(&chain, &mut h, BOB, Some(20));
        assert!(h.sent.is_empty());
    }

    #[test]
    fn leaving_disconnects_only_with_follow_leave() {
        let (chain, mut h, _) = setup();
        voice(&chain, &mut h, BOB, Some(22));
        h.sent.clear();
        voice(&chain, &mut h, BOB, None);
        assert!(h.sent.is_empty());
        let mut on = Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
        .with("followUserId", Val::Str(BOB.to_string()))
        .with("followLeave", Val::Bool(true));
        voice(&chain, &mut on, BOB, Some(22));
        voice(&chain, &mut on, BOB, None);
        assert_eq!(on.sent.len(), 1);
        assert_eq!(on.sent[0].1.at(&["channel_id"]), Some(&Val::Null));
    }

    #[test]
    fn only_manual_trigger_and_other_users_are_ignored() {
        let (chain, mut h, _) = setup();
        voice(&chain, &mut h, 99, Some(20));
        assert!(h.sent.is_empty());
        let mut manual = Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
        .with("followUserId", Val::Str(BOB.to_string()))
        .with("onlyManualTrigger", Val::Bool(true));
        voice(&chain, &mut manual, BOB, Some(20));
        assert!(manual.sent.is_empty());
    }

    #[test]
    fn a_full_channel_stops_the_join_and_retries_when_a_slot_frees() {
        let (chain, mut h, _) = setup();
        voice(&chain, &mut h, 50, Some(21));
        voice(&chain, &mut h, BOB, Some(21));
        assert!(h.sent.is_empty(), "limit 2 reached: not joined");
        // 50 leaves: 1 of 2 in the channel, the followed user inside -> join.
        voice(&chain, &mut h, 50, None);
        assert!(joined(&h, "21"));
    }

    #[test]
    fn auto_move_back_follows_again_after_being_moved() {
        let (chain, mut h, _) = setup();
        voice(&chain, &mut h, BOB, Some(20));
        voice(&chain, &mut h, ME, Some(20));
        h.sent.clear();
        let mut cfg = Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
        .with("followUserId", Val::Str(BOB.to_string()))
        .with("autoMoveBack", Val::Bool(true));
        voice(&chain, &mut cfg, ME, Some(22));
        assert!(joined(&cfg, "20"));
    }

    #[test]
    fn commands_pick_clear_and_trigger_the_target() {
        let (chain, mut h, cmds) = setup();
        let args = |u: Option<&str>| Args {
            options: u
                .map(|u| vec![("user".into(), u.into())])
                .unwrap_or_default(),
        };
        assert!(cmds[0].run(&mut h, &args(Some("nope"))).contains("nope"));
        let mut h2 = Rec {
            self_id: Some(ME),
            ..Rec::default()
        };
        voice(&chain, &mut h2, 8, Some(20));
        cmds[0].run(&mut h2, &args(Some("<@!8>")));
        assert!(joined(&h2, "20"), "the command queues its op 4 itself");
        assert_eq!(h2.sent.len(), 1);
        // Cleared: the target no longer counts, even though the setting names one.
        cmds[1].run(&mut h, &args(None));
        voice(&chain, &mut h, BOB, Some(20));
        assert!(h.sent.is_empty());
        // Manual trigger with no user, nothing followed: nothing happens.
        cmds[0].run(&mut h, &args(None));
        assert!(h.sent.is_empty());
        // Manual trigger with a target in voice joins at once, even with executeOnFollow off.
        let mut off = Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
        .with("executeOnFollow", Val::Bool(false));
        voice(&chain, &mut off, 8, Some(20));
        cmds[0].run(&mut off, &args(Some("8")));
        assert!(
            off.sent.is_empty(),
            "executeOnFollow off: following does not join"
        );
        cmds[0].run(&mut off, &args(None));
        assert!(joined(&off, "20"));
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let (chain, mut h, _) = setup();
        for d in [r#"null"#, r#"{}"#, r#"{"user_id":5}"#, r#"[]"#] {
            feed(&chain, &mut h, "VOICE_STATE_UPDATE", d);
            feed(&chain, &mut h, "CHANNEL_UPDATE", d);
        }
        chain.send(&mut h, 4, &mut j(r#"{"op":4,"d":7}"#));
        assert!(h.sent.is_empty());
    }
}
