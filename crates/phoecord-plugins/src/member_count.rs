//! MemberCount: online, total and voice member counts (Vencord: MemberCount).
//!
//! Ported from Vencord `src/plugins/memberCount/index.tsx` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; authors Ven, Commandtechno, Apexo). Vencord shows "online / total
//! members" above the member list and a voice count above the voice channel list. Here the numbers
//! are collected per server and shown by a command:
//! - total: `member_count` of `GUILD_CREATE`, kept current by `GUILD_MEMBER_ADD` / `REMOVE`;
//! - online: `approximate_presence_count` when the server sends it, otherwise the number of
//!   members seen online in `GUILD_CREATE` `presences` and `PRESENCE_UPDATE`. Large servers send
//!   no full presence list, so that number can be too low: the reply says "seen";
//! - voice: people seen in voice channels of the server (`VoiceStates::in_guild`).
//!
//! Traffic is never changed.
//!
//! Switching it on: `[plugins.member_count] enabled = true`, or `/phoecord toggle member_count`.
//! Then `/phoecord members [server]` lists every server, or one (id or part of its name).
//!
//! Not ported: the counters in the member list and voice list (Phase 2, on-screen part) and the
//! settings `toolTip`, `memberList`, `voiceActivity`, which only change how they look.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Guilds, Shared, VoiceStates, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, HashSet};

const PLUGIN_ID: &str = "member_count";
const EXTRA_EVENTS: &[&str] = &["PRESENCE_UPDATE"];
/// Online users kept per guild at most.
const ONLINE_CAP: usize = 50_000;

#[derive(Default)]
struct State {
    guilds: Guilds,
    voice: VoiceStates,
    /// guild -> members seen online.
    online: HashMap<u64, HashSet<u64>>,
    /// guild -> `approximate_presence_count`.
    approx_online: HashMap<u64, i64>,
}

impl State {
    fn set_online(&mut self, guild: u64, user: u64, status: &str) {
        let set = self.online.entry(guild).or_default();
        if status == "offline" || status == "invisible" {
            set.remove(&user);
        } else if set.len() < ONLINE_CAP || set.contains(&user) {
            set.insert(user);
        }
    }

    fn guild_presences(&mut self, gid: u64, d: &Val) {
        for p in d.get("presences").map(Val::as_list).unwrap_or(&[]) {
            if let Some(u) = p.id_at(&["user", "id"]).or_else(|| p.id_at(&["user_id"])) {
                let status = p.str_at(&["status"]).unwrap_or("online");
                self.set_online(gid, u, status);
            }
        }
        if let Some(n) = d.get("approximate_presence_count").and_then(Val::as_i64) {
            self.approx_online.insert(gid, n);
        }
    }

    fn observe(&mut self, name: &str, d: &Val) {
        if is_one_of(name, Guilds::EVENTS) {
            self.guilds.observe(name, d);
        }
        if is_one_of(name, VoiceStates::EVENTS) {
            self.voice.observe(name, d);
        }
        match name {
            "GUILD_CREATE" => {
                if let Some(gid) = d.id_at(&["id"]) {
                    self.online.remove(&gid);
                    self.guild_presences(gid, d);
                }
            }
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    if let Some(gid) = g.id_at(&["id"]) {
                        self.guild_presences(gid, g);
                    }
                }
            }
            "PRESENCE_UPDATE" => {
                if let (Some(gid), Some(u)) = (d.id_at(&["guild_id"]), d.id_at(&["user", "id"])) {
                    let status = d.str_at(&["status"]).unwrap_or("online");
                    self.set_online(gid, u, status);
                }
            }
            "GUILD_DELETE" => {
                if d.get("unavailable").and_then(Val::as_bool) != Some(true)
                    && let Some(gid) = d.id_at(&["id"])
                {
                    self.online.remove(&gid);
                    self.approx_online.remove(&gid);
                }
            }
            _ => {}
        }
    }

    fn line(&self, id: u64) -> String {
        let Some(g) = self.guilds.guild(id) else {
            return format!("{id}: unknown server");
        };
        let name = if g.name.is_empty() { "?" } else { &g.name };
        let total = g
            .member_count
            .map_or_else(|| "total unknown".to_string(), |n| format!("{n} total"));
        let seen = self.online.get(&id).map_or(0, |s| s.len() as i64);
        let online = match self.approx_online.get(&id) {
            Some(n) => format!("{n} online"),
            None => format!("{seen} online (seen)"),
        };
        let voice = self.voice.in_guild(id).len();
        format!("{name}: {online}, {total}, {voice} in voice")
    }

    fn report(&self, query: Option<&str>) -> String {
        let guilds = self.guilds.guilds();
        let picked: Vec<u64> = match query {
            None => guilds.iter().map(|g| g.id).collect(),
            Some(q) => {
                let q = q.trim();
                let lower = q.to_lowercase();
                guilds
                    .iter()
                    .filter(|g| {
                        q.parse::<u64>() == Ok(g.id) || g.name.to_lowercase().contains(&lower)
                    })
                    .map(|g| g.id)
                    .collect()
            }
        };
        if picked.is_empty() {
            return match query {
                Some(q) => format!("No server matches `{q}`."),
                None => "No servers seen yet.".into(),
            };
        }
        picked
            .iter()
            .map(|id| self.line(*id))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct MemberCount {
    manifest: Manifest,
    state: Shared<State>,
}

impl MemberCount {
    pub fn new() -> MemberCount {
        MemberCount {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "MemberCount",
                    env!("CARGO_PKG_VERSION"),
                    "Online, total and voice member counts",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("MemberCount".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord members` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "members",
            "Show online, total and voice member counts",
            move |_host: &mut dyn Host, args: &Args| {
                let q = args.get("server").map(str::trim).filter(|s| !s.is_empty());
                state.lock().report(q)
            },
        )
        .option("server", "Server id or part of its name", false)
        .boxed()
    }
}

impl Default for MemberCount {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, EXTRA_EVENTS)
        || is_one_of(name, Guilds::EVENTS)
        || is_one_of(name, VoiceStates::EVENTS)
}

impl Plugin for MemberCount {
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

    fn args(server: Option<&str>) -> Args {
        Args {
            options: server
                .map(|s| vec![("server".to_string(), s.to_string())])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn counts_total_online_and_voice() {
        let plugin = MemberCount::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        assert!(cmd.run(&mut h, &args(None)).contains("No servers"));
        let out = feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"1","name":"Alpha","member_count":100,
               "presences":[{"user":{"id":"5"},"status":"online"},{"user":{"id":"6"},"status":"idle"}],
               "voice_states":[{"user_id":"5","channel_id":"12"}]}"#,
        );
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(
            cmd.run(&mut h, &args(None)),
            "Alpha: 2 online (seen), 100 total, 1 in voice"
        );
        feed(
            &chain,
            &mut h,
            "PRESENCE_UPDATE",
            r#"{"guild_id":"1","user":{"id":"6"},"status":"offline"}"#,
        );
        feed(
            &chain,
            &mut h,
            "GUILD_MEMBER_ADD",
            r#"{"guild_id":"1","user":{"id":"9"}}"#,
        );
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            r#"{"guild_id":"1","user_id":"5","channel_id":null}"#,
        );
        assert_eq!(
            cmd.run(&mut h, &args(Some("alp"))),
            "Alpha: 1 online (seen), 101 total, 0 in voice"
        );
        assert!(
            cmd.run(&mut h, &args(Some("zzz")))
                .contains("No server matches")
        );
    }

    #[test]
    fn counts_from_a_real_gateway_frame() {
        let plugin = MemberCount::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        let mut frame = j(
            r#"{"op":0,"t":"GUILD_CREATE","s":1,"d":{"id":"1","name":"Alpha","member_count":100,
               "presences":[{"user":{"id":"5"},"status":"online"}],
               "voice_states":[{"user_id":"5","channel_id":"12"}]}}"#,
        );
        let out = chain.dispatch(&mut h, "GUILD_CREATE", &mut frame, &mut Vec::new());
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(
            cmd.run(&mut h, &args(None)),
            "Alpha: 1 online (seen), 100 total, 1 in voice"
        );
    }

    #[test]
    fn approximate_presence_count_wins() {
        let plugin = MemberCount::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"1","name":"Big","member_count":5000,"approximate_presence_count":321}"#,
        );
        assert_eq!(
            cmd.run(&mut h, &args(Some("1"))),
            "Big: 321 online, 5000 total, 0 in voice"
        );
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let chain = Chain::new(vec![Box::new(MemberCount::new())]);
        let mut h = Cfg::default();
        for (n, d) in [
            ("PRESENCE_UPDATE", "{}"),
            ("GUILD_CREATE", r#"{"presences":[1,2],"member_count":"x"}"#),
            ("GUILD_DELETE", "null"),
            ("READY", r#"{"guilds":[{"presences":5}]}"#),
        ] {
            assert_eq!(feed(&chain, &mut h, n, d), Outcome::Untouched);
        }
    }

    #[test]
    fn command_refuses_while_the_plugin_is_off() {
        let cmd = MemberCount::new().command();
        let mut h = Cfg::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args(None)).contains("is off"));
    }
}
