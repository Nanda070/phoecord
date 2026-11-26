//! TypingIndicator: who is typing, per channel (Vencord: TypingIndicator).
//!
//! Ported from Vencord `src/plugins/typingIndicator/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors Nuckyz, fawn, Sqaaakoi). Vencord shows a dots and
//! avatars indicator on channels and servers that have someone typing. Here the gateway's
//! `TYPING_START` events are tracked per channel and shown by a command. A user counts as typing
//! for 10 seconds after the event (as the client does), or until their message arrives. Your own
//! typing is ignored. Traffic is never changed.
//!
//! Switching it on: `[plugins.typing_indicator] enabled = true`, or
//! `/phoecord toggle typing_indicator`. Then `/phoecord typing` lists who is typing where.
//!
//! Not ported: the dots and avatars on the channel list, server list and chat header, and the
//! settings `includeMutedChannels`, `includeCurrentChannel`, `indicatorMode`, which only matter
//! on screen (Phase 2). Blocked users are not filtered (the proxy does not track relationships
//! here). At most 2000 typing entries are kept.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Guilds, Shared, Users, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PLUGIN_ID: &str = "typing_indicator";
/// How long a `TYPING_START` counts, as in the client.
const TTL: Duration = Duration::from_secs(10);
const CAP: usize = 2000;
const EXTRA_EVENTS: &[&str] = &["TYPING_START", "MESSAGE_CREATE"];

#[derive(Default)]
struct State {
    /// (channel, user) -> when typing started.
    typing: HashMap<(u64, u64), Instant>,
    guilds: Guilds,
    users: Users,
}

impl State {
    fn prune(&mut self, now: Instant) {
        self.typing.retain(|_, t| now.duration_since(*t) < TTL);
    }

    fn observe(&mut self, name: &str, d: &Val, me: Option<u64>, now: Instant) {
        if is_one_of(name, Guilds::EVENTS) {
            self.guilds.observe(name, d);
        }
        if is_one_of(name, Users::EVENTS) {
            self.users.observe(name, d);
        }
        match name {
            "TYPING_START" => {
                let (Some(ch), Some(user)) = (
                    d.id_at(&["channel_id"]),
                    d.id_at(&["user_id"])
                        .or_else(|| d.id_at(&["member", "user", "id"])),
                ) else {
                    return;
                };
                if Some(user) == me {
                    return;
                }
                self.prune(now);
                if self.typing.len() >= CAP && !self.typing.contains_key(&(ch, user)) {
                    // Full: drop the entry that started typing first.
                    if let Some(oldest) =
                        self.typing.iter().min_by_key(|(_, t)| **t).map(|(k, _)| *k)
                    {
                        self.typing.remove(&oldest);
                    }
                }
                self.typing.insert((ch, user), now);
            }
            "MESSAGE_CREATE" => {
                if let (Some(ch), Some(user)) =
                    (d.id_at(&["channel_id"]), d.id_at(&["author", "id"]))
                {
                    self.typing.remove(&(ch, user));
                }
            }
            _ => {}
        }
    }

    fn channel_label(&self, ch: u64) -> String {
        match self.guilds.channel(ch) {
            Some(c) => {
                let guild = self
                    .guilds
                    .guild(c.guild_id)
                    .map(|g| g.name.as_str())
                    .filter(|n| !n.is_empty());
                match guild {
                    Some(g) => format!("#{} ({g})", c.name),
                    None => format!("#{}", c.name),
                }
            }
            None => format!("<#{ch}>"),
        }
    }

    fn report(&mut self, now: Instant) -> String {
        self.prune(now);
        let mut by_channel: HashMap<u64, Vec<u64>> = HashMap::new();
        for (ch, user) in self.typing.keys() {
            by_channel.entry(*ch).or_default().push(*user);
        }
        if by_channel.is_empty() {
            return "Nobody is typing right now.".into();
        }
        let mut rows: Vec<(String, Vec<u64>)> = by_channel
            .into_iter()
            .map(|(ch, mut users)| {
                users.sort_unstable();
                (self.channel_label(ch), users)
            })
            .collect();
        rows.sort();
        rows.iter()
            .map(|(label, users)| {
                let names: Vec<String> = users.iter().map(|u| self.users.display(*u)).collect();
                format!("{label}: {} typing", names.join(", "))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct TypingIndicator {
    manifest: Manifest,
    state: Shared<State>,
}

impl TypingIndicator {
    pub fn new() -> TypingIndicator {
        TypingIndicator {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "TypingIndicator",
                    env!("CARGO_PKG_VERSION"),
                    "Who is typing, per channel (collects typing events)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("TypingIndicator".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord typing` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "typing",
            "Show who is typing, per channel",
            move |_host: &mut dyn Host, _args: &Args| state.lock().report(Instant::now()),
        )
        .boxed()
    }
}

impl Default for TypingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, EXTRA_EVENTS)
        || is_one_of(name, Guilds::EVENTS)
        || is_one_of(name, Users::EVENTS)
}

impl Plugin for TypingIndicator {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if wanted(name))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event
            && wanted(name)
        {
            let me = host.self_id();
            self.state
                .lock()
                .observe(name, dispatch_data(payload), me, Instant::now());
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn feed(
        chain: &Chain,
        h: &mut dyn phoecord_core::ChainHost,
        name: &str,
        json: &str,
    ) -> Outcome {
        let mut d = j(json);
        chain.dispatch(h, name, &mut d, &mut Vec::new())
    }

    #[test]
    fn shows_who_types_where_and_clears_on_message() {
        let plugin = TypingIndicator::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Rec {
            self_id: Some(1),
            ..Rec::default()
        };
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"10","name":"Alpha","channels":[{"id":"20","name":"general","type":0}]}"#,
        );
        let out = feed(
            &chain,
            &mut h,
            "TYPING_START",
            r#"{"channel_id":"20","user_id":"5","member":{"user":{"id":"5","username":"ann"}}}"#,
        );
        assert_eq!(out, Outcome::Untouched);
        feed(
            &chain,
            &mut h,
            "TYPING_START",
            r#"{"channel_id":"20","user_id":"1"}"#,
        );
        feed(
            &chain,
            &mut h,
            "TYPING_START",
            r#"{"channel_id":"30","user_id":"6"}"#,
        );
        let text = cmd.run(&mut h, &Args { options: vec![] });
        assert!(text.contains("#general (Alpha): ann typing"), "{text}");
        assert!(text.contains("<#30>: <@6> typing"), "{text}");
        assert!(!text.contains("<@1>"), "own typing is ignored");
        feed(
            &chain,
            &mut h,
            "MESSAGE_CREATE",
            r#"{"channel_id":"20","author":{"id":"5","username":"ann"}}"#,
        );
        let text = cmd.run(&mut h, &Args { options: vec![] });
        assert!(!text.contains("ann"), "{text}");
    }

    #[test]
    fn shows_who_types_from_real_gateway_frames() {
        let plugin = TypingIndicator::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Rec {
            self_id: Some(1),
            ..Rec::default()
        };
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"op":0,"t":"GUILD_CREATE","s":1,"d":{"id":"10","name":"Alpha",
               "channels":[{"id":"20","name":"general","type":0}]}}"#,
        );
        feed(
            &chain,
            &mut h,
            "TYPING_START",
            r#"{"op":0,"t":"TYPING_START","s":2,"d":{"channel_id":"20","user_id":"5",
               "member":{"user":{"id":"5","username":"ann"}}}}"#,
        );
        let text = cmd.run(&mut h, &Args { options: vec![] });
        assert!(text.contains("#general (Alpha): ann typing"), "{text}");
    }

    #[test]
    fn entries_expire_after_ten_seconds() {
        let mut st = State::default();
        let t0 = Instant::now();
        st.observe(
            "TYPING_START",
            &j(r#"{"channel_id":"2","user_id":"3"}"#),
            None,
            t0,
        );
        assert!(st.report(t0 + Duration::from_secs(5)).contains("typing"));
        assert!(
            st.report(t0 + Duration::from_secs(11))
                .contains("Nobody is typing")
        );
    }

    #[test]
    fn malformed_payloads_and_a_full_table_are_fine() {
        let chain = Chain::new(vec![Box::new(TypingIndicator::new())]);
        let mut h = Cfg::default();
        for d in ["{}", r#"{"channel_id":"x"}"#, "null", r#"{"user_id":"1"}"#] {
            assert_eq!(feed(&chain, &mut h, "TYPING_START", d), Outcome::Untouched);
        }
        let mut st = State::default();
        let t0 = Instant::now();
        for u in 0..(CAP as u64 + 5) {
            st.observe(
                "TYPING_START",
                &j(&format!(r#"{{"channel_id":"2","user_id":"{}"}}"#, u + 100)),
                None,
                t0,
            );
        }
        assert_eq!(st.typing.len(), CAP);
    }

    #[test]
    fn command_refuses_while_the_plugin_is_off() {
        let cmd = TypingIndicator::new().command();
        let mut h = Cfg::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("is off")
        );
    }
}
