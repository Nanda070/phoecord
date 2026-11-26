//! VcNarrator: announce voice joins and leaves (Vencord: VcNarrator).
//!
//! Ported from Vencord `src/plugins/vcNarrator/index.tsx` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; authors Ven). Vencord speaks "X joined", "X left", "X moved to Y"
//! (and mute/deafen changes) for the voice channel you are in, using the system's text to speech.
//! The proxy has no text to speech: here each message is sent as a desktop notification
//! (`Host::notify`) instead, so nothing is spoken. Traffic is never changed.
//!
//! Switching it on: `[plugins.vc_narrator] enabled = true`, or `/phoecord toggle vc_narrator`.
//! Only changes in the voice channel you are currently in are announced, plus your own moves
//! when `say_own_name` is on.
//!
//! Settings (`[plugins.vc_narrator]`, Vencord's names in snake_case): `say_own_name` (default
//! `false`: ignore yourself), `join_message` (`{{USER}} joined`), `leave_message`
//! (`{{USER}} left`), `move_message` (`{{USER}} moved to {{CHANNEL}}`), `mute_message`
//! (`{{USER}} muted`), `unmute_message` (`{{USER}} unmuted`), `deafen_message`
//! (`{{USER}} deafened`), `undeafen_message` (`{{USER}} undeafened`). An empty message turns that
//! announcement off. `{{USER}}` and `{{CHANNEL}}` are replaced by the name and the channel.
//!
//! Not ported: speech itself and its settings `voice`, `volume`, `rate`, `latinOnly`, and the
//! "test" button (Phase 2 or never; the proxy cannot speak).

use crate::state::{Guilds, Shared, Users, VoiceChange, VoiceStates, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;

const PLUGIN_ID: &str = "vc_narrator";
const EXTRA_EVENTS: &[&str] = &["VOICE_STATE_UPDATE"];
/// Users whose mute and deafen flags are remembered, at most.
const FLAGS_CAP: usize = 5000;

#[derive(Default)]
struct State {
    guilds: Guilds,
    voice: VoiceStates,
    users: Users,
    /// user -> (muted, deafened), from `VOICE_STATE_UPDATE`.
    flags: HashMap<u64, (bool, bool)>,
}

fn setting_text(host: &dyn Host, key: &str, default: &str) -> String {
    match host.setting(key) {
        Some(v) => v.as_str().unwrap_or(default).to_string(),
        None => default.to_string(),
    }
}

fn render(template: &str, user: &str, channel: &str) -> String {
    template
        .replace("{{USER}}", user)
        .replace("{{CHANNEL}}", channel)
        .trim()
        .to_string()
}

/// `self_mute || mute`, `self_deaf || deaf`.
fn flags_of(d: &Val) -> (bool, bool) {
    let b = |k: &str| d.get(k).and_then(Val::as_bool).unwrap_or(false);
    (b("self_mute") || b("mute"), b("self_deaf") || b("deaf"))
}

impl State {
    fn channel_name(&self, ch: u64) -> String {
        self.guilds
            .channel(ch)
            .map(|c| c.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "another channel".into())
    }

    /// The messages to announce for one `VOICE_STATE_UPDATE`.
    fn voice_update(&mut self, host: &dyn Host, d: &Val) -> Vec<String> {
        let Some(user) = d.id_at(&["user_id"]) else {
            return Vec::new();
        };
        self.users.observe("VOICE_STATE_UPDATE", d);
        let me = host.self_id();
        let mine_before = me.and_then(|m| self.voice.channel_of(m));
        let change = self.voice.observe("VOICE_STATE_UPDATE", d);
        let mine_after = me.and_then(|m| self.voice.channel_of(m));
        let new_flags = flags_of(d);
        let old_flags = if d.id_at(&["channel_id"]).is_some() {
            self.flags.insert(user, new_flags)
        } else {
            self.flags.remove(&user)
        };
        if self.flags.len() > FLAGS_CAP {
            self.flags.clear();
        }
        let own = Some(user) == me;
        if own && host.setting("say_own_name").and_then(|v| v.as_bool()) != Some(true) {
            return Vec::new();
        }
        let name = self.users.display(user);
        let name = self.users.name(user).map_or(name, str::to_string);
        let say = |key: &str, default: &str, channel: &str| {
            render(&setting_text(host, key, default), &name, channel)
        };
        let mut out = Vec::new();
        match change {
            Some(VoiceChange::Joined { channel, .. }) if Some(channel) == mine_after => {
                out.push(say(
                    "join_message",
                    "{{USER}} joined",
                    &self.channel_name(channel),
                ));
            }
            Some(VoiceChange::Left { channel, .. }) if Some(channel) == mine_before => {
                out.push(say(
                    "leave_message",
                    "{{USER}} left",
                    &self.channel_name(channel),
                ));
            }
            Some(VoiceChange::Moved { from, to, .. }) => {
                if Some(to) == mine_after {
                    out.push(say(
                        "join_message",
                        "{{USER}} joined",
                        &self.channel_name(to),
                    ));
                } else if Some(from) == mine_after || Some(from) == mine_before {
                    out.push(say(
                        "move_message",
                        "{{USER}} moved to {{CHANNEL}}",
                        &self.channel_name(to),
                    ));
                }
            }
            Some(_) => {}
            None => {
                // Same channel as before: a mute or deafen change, in my channel only.
                let ch = d.id_at(&["channel_id"]);
                if ch.is_some()
                    && ch == mine_after
                    && let Some((om, od)) = old_flags
                {
                    let (nm, nd) = new_flags;
                    let ch_name = self.channel_name(ch.unwrap_or(0));
                    if om != nm {
                        out.push(if nm {
                            say("mute_message", "{{USER}} muted", &ch_name)
                        } else {
                            say("unmute_message", "{{USER}} unmuted", &ch_name)
                        });
                    }
                    if od != nd {
                        out.push(if nd {
                            say("deafen_message", "{{USER}} deafened", &ch_name)
                        } else {
                            say("undeafen_message", "{{USER}} undeafened", &ch_name)
                        });
                    }
                }
            }
        }
        out.retain(|m| !m.is_empty());
        out
    }
}

pub struct VcNarrator {
    manifest: Manifest,
    state: Shared<State>,
}

impl VcNarrator {
    pub fn new() -> VcNarrator {
        VcNarrator {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "VcNarrator",
                    env!("CARGO_PKG_VERSION"),
                    "Announce voice joins and leaves as notifications (no speech)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("VcNarrator".into());
                m
            },
            state: Shared::default(),
        }
    }
}

impl Default for VcNarrator {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, EXTRA_EVENTS)
        || is_one_of(name, Guilds::EVENTS)
        || is_one_of(name, Users::EVENTS)
        || is_one_of(name, VoiceStates::EVENTS)
}

impl Plugin for VcNarrator {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if wanted(name))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch { name, payload, .. } = event else {
            return Verdict::Pass;
        };
        if !wanted(name) {
            return Verdict::Pass;
        }
        let payload = dispatch_data(payload);
        let messages = {
            let mut st = self.state.lock();
            if name == "VOICE_STATE_UPDATE" {
                st.voice_update(&*host, payload)
            } else {
                if is_one_of(name, Guilds::EVENTS) {
                    st.guilds.observe(name, payload);
                }
                if is_one_of(name, Users::EVENTS) {
                    st.users.observe(name, payload);
                }
                if is_one_of(name, VoiceStates::EVENTS) {
                    st.voice.observe(name, payload);
                }
                if name == "READY" || name == "GUILD_CREATE" {
                    st.sync_flags(payload);
                }
                Vec::new()
            }
        };
        if !messages.is_empty() {
            let title = host.t("title", &[]);
            for m in messages {
                host.notify(&title, &m);
            }
        }
        Verdict::Pass
    }
}

impl State {
    /// Initial mute and deafen flags from `READY` / `GUILD_CREATE` voice states.
    fn sync_flags(&mut self, d: &Val) {
        let mut load = |g: &Val| {
            for vs in g.get("voice_states").map(Val::as_list).unwrap_or(&[]) {
                if let Some(u) = vs.id_at(&["user_id"]) {
                    if self.flags.len() >= FLAGS_CAP {
                        return;
                    }
                    self.flags.insert(u, flags_of(vs));
                }
            }
        };
        load(d);
        for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
            load(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn feed(chain: &Chain, h: &mut Rec, name: &str, json: &str) -> Outcome {
        let mut d = j(json);
        chain.dispatch(h, name, &mut d, &mut Vec::new())
    }

    fn setup() -> (Chain, Rec) {
        let chain = Chain::new(vec![Box::new(VcNarrator::new())]);
        let mut h = Rec {
            self_id: Some(1),
            ..Rec::default()
        };
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"10","name":"Alpha","channels":[{"id":"20","name":"Lounge","type":2},
               {"id":"21","name":"Games","type":2}],
               "voice_states":[{"user_id":"1","channel_id":"20"}]}"#,
        );
        (chain, h)
    }

    fn vs(user: &str, ch: &str, extra: &str) -> String {
        format!(
            r#"{{"guild_id":"10","user_id":"{user}","channel_id":{ch}{extra},
               "member":{{"user":{{"id":"{user}","username":"u{user}"}}}}}}"#
        )
    }

    #[test]
    fn announces_join_move_leave_in_my_channel_only() {
        let (chain, mut h) = setup();
        let out = feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, ""),
        );
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(h.notes, [("Voice".to_string(), "u5 joined".to_string())]);
        // Someone elsewhere is not announced.
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("6", r#""21""#, ""),
        );
        assert_eq!(h.notes.len(), 1);
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""21""#, ""),
        );
        assert_eq!(h.notes[1].1, "u5 moved to Games");
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, ""),
        );
        assert_eq!(h.notes[2].1, "u5 joined");
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs("5", "null", ""));
        assert_eq!(h.notes[3].1, "u5 left");
    }

    #[test]
    fn announces_from_real_gateway_frames() {
        let chain = Chain::new(vec![Box::new(VcNarrator::new())]);
        let mut h = Rec {
            self_id: Some(1),
            ..Rec::default()
        };
        let frame = |t: &str, s: u32, d: &str| format!(r#"{{"op":0,"t":"{t}","s":{s},"d":{d}}}"#);
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            &frame(
                "GUILD_CREATE",
                1,
                r#"{"id":"10","name":"Alpha","channels":[{"id":"20","name":"Lounge","type":2}],
                   "voice_states":[{"user_id":"1","channel_id":"20"}]}"#,
            ),
        );
        let out = feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &frame("VOICE_STATE_UPDATE", 2, &vs("5", r#""20""#, "")),
        );
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(h.notes, [("Voice".to_string(), "u5 joined".to_string())]);
    }

    #[test]
    fn ignores_yourself_unless_asked_and_honours_templates() {
        let (chain, mut h) = setup();
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("1", r#""21""#, ""),
        );
        assert!(h.notes.is_empty());
        let mut h = h
            .with("say_own_name", Val::Bool(true))
            .with("move_message", Val::Str("{{USER}} -> {{CHANNEL}}!".into()))
            .with("leave_message", Val::Str(String::new()));
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("1", r#""20""#, ""),
        );
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("1", r#""21""#, ""),
        );
        assert_eq!(h.notes.last().unwrap().1, "u1 joined");
        // I am in Games (21) now.
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("7", r#""21""#, ""),
        );
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("7", r#""20""#, ""),
        );
        assert_eq!(h.notes.last().unwrap().1, "u7 -> Lounge!");
        let n = h.notes.len();
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("8", r#""21""#, ""),
        );
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs("8", "null", ""));
        assert_eq!(h.notes.len(), n + 1, "the empty leave template is silent");
    }

    #[test]
    fn announces_mute_and_deafen_changes() {
        let (chain, mut h) = setup();
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, ""),
        );
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, r#","self_mute":true"#),
        );
        assert_eq!(h.notes.last().unwrap().1, "u5 muted");
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, ""),
        );
        assert_eq!(h.notes.last().unwrap().1, "u5 unmuted");
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            &vs("5", r#""20""#, r#","self_deaf":true"#),
        );
        assert_eq!(h.notes.last().unwrap().1, "u5 deafened");
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let (chain, mut h) = setup();
        for d in [
            "{}",
            "null",
            r#"{"user_id":"x"}"#,
            r#"{"user_id":"4","channel_id":{}}"#,
        ] {
            assert_eq!(
                feed(&chain, &mut h, "VOICE_STATE_UPDATE", d),
                Outcome::Untouched
            );
        }
        assert!(h.notes.is_empty());
    }
}
