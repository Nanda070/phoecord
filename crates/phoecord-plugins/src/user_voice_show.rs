//! UserVoiceShow: show who is in voice (Vencord: UserVoiceShow).
//!
//! Ported from Vencord `src/plugins/userVoiceShow/index.tsx` (GPL-3.0-or-later, Copyright (c) 2024
//! Vendicated and contributors; authors Nuckyz, LordElias). Vencord adds a "in voice channel" badge
//! to the member list, user popout and profile. Here voice states are collected from `READY`,
//! `GUILD_CREATE` and `VOICE_STATE_UPDATE` and shown by a command. Traffic is never changed.
//!
//! Switching it on: `[plugins.user_voice_show] enabled = true`, or
//! `/phoecord toggle user_voice_show`. Then `/phoecord voice [user]` lists every voice channel
//! with people in it, or where one user is (`user` is a mention, an id or part of a name).
//!
//! Not ported: the badge and the profile section, and the settings `showInUserProfile`,
//! `showVoiceChannelSectionHeader` (Phase 2, on-screen part). Voice states of servers the proxy
//! has not seen a `GUILD_CREATE` for are not listed.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Guilds, Shared, Users, VoiceStates, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::BTreeMap;

const PLUGIN_ID: &str = "user_voice_show";

#[derive(Default)]
struct State {
    guilds: Guilds,
    voice: VoiceStates,
    users: Users,
}

impl State {
    fn observe(&mut self, name: &str, d: &Val) {
        if is_one_of(name, Guilds::EVENTS) {
            self.guilds.observe(name, d);
        }
        if is_one_of(name, Users::EVENTS) {
            self.users.observe(name, d);
        }
        if is_one_of(name, VoiceStates::EVENTS) {
            self.voice.observe(name, d);
        }
    }

    /// Everyone in voice in known guilds and DM calls, grouped by channel.
    fn by_channel(&self) -> BTreeMap<u64, Vec<u64>> {
        let mut map: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let mut scopes: Vec<u64> = self.guilds.guilds().iter().map(|g| g.id).collect();
        scopes.push(0);
        for scope in scopes {
            for user in self.voice.in_guild(scope) {
                if let Some(ch) = self.voice.channel_of(user) {
                    map.entry(ch).or_default().push(user);
                }
            }
        }
        map
    }

    fn channel_label(&self, ch: u64) -> String {
        match self.guilds.channel(ch) {
            Some(c) => match self.guilds.guild(c.guild_id) {
                Some(g) if !g.name.is_empty() => format!("{} / #{}", g.name, c.name),
                _ => format!("#{}", c.name),
            },
            None => format!("<#{ch}> (call)"),
        }
    }

    fn matches(&self, user: u64, query: &str) -> bool {
        let q = query.trim().trim_start_matches("<@").trim_end_matches('>');
        let q = q.trim_start_matches('!');
        if let Ok(id) = q.parse::<u64>() {
            return id == user;
        }
        self.users
            .name(user)
            .is_some_and(|n| n.to_lowercase().contains(&q.to_lowercase()))
    }

    fn report(&self, query: Option<&str>) -> String {
        let channels = self.by_channel();
        let mut rows = Vec::new();
        for (ch, users) in &channels {
            let shown: Vec<u64> = users
                .iter()
                .copied()
                .filter(|u| query.is_none_or(|q| self.matches(*u, q)))
                .collect();
            if shown.is_empty() {
                continue;
            }
            let names: Vec<String> = shown.iter().map(|u| self.users.display(*u)).collect();
            rows.push(match query {
                Some(_) => format!("{} is in {}", names.join(", "), self.channel_label(*ch)),
                None => format!(
                    "{} ({}): {}",
                    self.channel_label(*ch),
                    users.len(),
                    names.join(", ")
                ),
            });
        }
        rows.sort();
        if rows.is_empty() {
            return match query {
                Some(q) => format!("`{q}` is not in a voice channel."),
                None => "Nobody is in a voice channel.".into(),
            };
        }
        rows.join("\n")
    }
}

pub struct UserVoiceShow {
    manifest: Manifest,
    state: Shared<State>,
}

impl UserVoiceShow {
    pub fn new() -> UserVoiceShow {
        UserVoiceShow {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "UserVoiceShow",
                    env!("CARGO_PKG_VERSION"),
                    "Show who is in voice (collects voice states)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("UserVoiceShow".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord voice` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "voice",
            "Show who is in voice channels",
            move |_host: &mut dyn Host, args: &Args| {
                let q = args.get("user").map(str::trim).filter(|s| !s.is_empty());
                state.lock().report(q)
            },
        )
        .option("user", "Mention, id or part of a name", false)
        .boxed()
    }
}

impl Default for UserVoiceShow {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, Guilds::EVENTS)
        || is_one_of(name, Users::EVENTS)
        || is_one_of(name, VoiceStates::EVENTS)
}

impl Plugin for UserVoiceShow {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if wanted(name))
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event
            && wanted(name)
        {
            self.state.lock().observe(name, dispatch_data(payload));
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn feed(chain: &Chain, h: &mut Cfg, name: &str, json: &str) -> Outcome {
        let mut d = j(json);
        chain.dispatch(h, name, &mut d, &mut Vec::new())
    }

    fn args(user: Option<&str>) -> Args {
        Args {
            options: user
                .map(|s| vec![("user".to_string(), s.to_string())])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn lists_voice_channels_and_one_user() {
        let plugin = UserVoiceShow::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        assert!(cmd.run(&mut h, &args(None)).contains("Nobody"));
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"1","name":"Alpha","channels":[{"id":"12","name":"Lounge","type":2}],
               "voice_states":[{"user_id":"7","channel_id":"12"}]}"#,
        );
        let out = feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            r#"{"guild_id":"1","user_id":"8","channel_id":"12","member":{"user":{"id":"8","username":"bob"}}}"#,
        );
        assert_eq!(out, Outcome::Untouched);
        let all = cmd.run(&mut h, &args(None));
        assert!(all.contains("Alpha / #Lounge (2): <@7>, bob"), "{all}");
        assert_eq!(
            cmd.run(&mut h, &args(Some("BO"))),
            "bob is in Alpha / #Lounge"
        );
        assert!(cmd.run(&mut h, &args(Some("<@7>"))).contains("<@7> is in"));
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            r#"{"guild_id":"1","user_id":"8","channel_id":null}"#,
        );
        assert!(
            cmd.run(&mut h, &args(Some("bob")))
                .contains("not in a voice")
        );
    }

    #[test]
    fn lists_voice_from_real_gateway_frames() {
        let plugin = UserVoiceShow::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"op":0,"t":"GUILD_CREATE","s":1,"d":{"id":"1","name":"Alpha",
               "channels":[{"id":"12","name":"Lounge","type":2}],
               "voice_states":[{"user_id":"7","channel_id":"12"}]}}"#,
        );
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            r#"{"op":0,"t":"VOICE_STATE_UPDATE","s":2,"d":{"guild_id":"1","user_id":"8","channel_id":"12",
               "member":{"user":{"id":"8","username":"bob"}}}}"#,
        );
        let all = cmd.run(&mut h, &args(None));
        assert!(all.contains("Alpha / #Lounge (2): <@7>, bob"), "{all}");
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let chain = Chain::new(vec![Box::new(UserVoiceShow::new())]);
        let mut h = Cfg::default();
        for (n, d) in [
            ("VOICE_STATE_UPDATE", "{}"),
            ("VOICE_STATE_UPDATE", r#"{"user_id":"x","channel_id":5}"#),
            ("GUILD_CREATE", r#"{"voice_states":7}"#),
            ("READY", "[]"),
        ] {
            assert_eq!(feed(&chain, &mut h, n, d), Outcome::Untouched);
        }
    }

    #[test]
    fn command_refuses_while_the_plugin_is_off() {
        let cmd = UserVoiceShow::new().command();
        let mut h = Cfg::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args(None)).contains("is off"));
    }
}
