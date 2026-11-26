//! PlatformIndicators: which devices a user is online on (Vencord: PlatformIndicators).
//!
//! Ported from Vencord `src/plugins/platformIndicators/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; authors kemo, TheSun, Nuckyz, Ven). Vencord draws a desktop,
//! mobile or web icon next to a name from the presence's `client_status`. Here the same field is
//! collected from `READY` (`presences`, `merged_presences`), `GUILD_CREATE` (`presences`) and
//! `PRESENCE_UPDATE`, and shown by a command. Traffic is never changed.
//!
//! Switching it on: `[plugins.platform_indicators] enabled = true`, or
//! `/phoecord toggle platform_indicators`. Then `/phoecord platforms [user]` lists who is online
//! on which platform (`user` is a mention, an id or part of a name; without it, the first 25).
//!
//! Not ported: the icons in the member list, user popout, profile and message headers, and the
//! `list`, `profiles`, `messages`, `colorMobileIndicator` settings (all Phase 2, on-screen part).
//! At most 10 000 users are kept; the oldest are forgotten first.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Shared, Users, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, VecDeque};

const PLUGIN_ID: &str = "platform_indicators";
const EVENTS: &[&str] = &["READY", "GUILD_CREATE", "PRESENCE_UPDATE"];
const CAP: usize = 10_000;
const LIST_LIMIT: usize = 25;
const PLATFORMS: [&str; 3] = ["desktop", "mobile", "web"];

/// Status (`online`, `idle`, `dnd`) per platform; `None` when not on that platform.
#[derive(Clone, Debug, Default, PartialEq)]
struct Platforms([Option<String>; 3]);

impl Platforms {
    fn from_presence(p: &Val) -> Platforms {
        let mut out = Platforms::default();
        for (slot, key) in out.0.iter_mut().zip(PLATFORMS) {
            *slot = p
                .at(&["client_status", key])
                .and_then(Val::as_str)
                .map(str::to_string);
        }
        out
    }
    fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }
    fn describe(&self) -> String {
        let parts: Vec<String> = PLATFORMS
            .iter()
            .zip(&self.0)
            .filter_map(|(name, st)| st.as_ref().map(|s| format!("{name} ({s})")))
            .collect();
        if parts.is_empty() {
            "offline".into()
        } else {
            parts.join(", ")
        }
    }
}

#[derive(Default)]
struct State {
    online: HashMap<u64, Platforms>,
    order: VecDeque<u64>,
    users: Users,
}

impl State {
    fn set(&mut self, user: u64, p: Platforms) {
        if p.is_empty() {
            self.online.remove(&user);
            return;
        }
        if self.online.insert(user, p).is_none() {
            self.order.push_back(user);
            while self.online.len() > CAP {
                match self.order.pop_front() {
                    Some(old) => {
                        self.online.remove(&old);
                    }
                    None => break,
                }
            }
        }
        // Ids removed earlier stay in `order` until they reach the front; keep it bounded too.
        if self.order.len() > CAP * 2 {
            let online = &self.online;
            self.order.retain(|u| online.contains_key(u));
        }
    }

    fn presence(&mut self, p: &Val) {
        if let Some(u) = p.get("user") {
            self.users.learn(u);
        }
        if let Some(id) = p.id_at(&["user", "id"]).or_else(|| p.id_at(&["user_id"])) {
            self.set(id, Platforms::from_presence(p));
        }
    }

    fn observe(&mut self, name: &str, d: &Val) {
        self.users.observe(name, d);
        match name {
            "PRESENCE_UPDATE" => self.presence(d),
            "GUILD_CREATE" => {
                for p in d.get("presences").map(Val::as_list).unwrap_or(&[]) {
                    self.presence(p);
                }
            }
            "READY" => {
                for p in d.get("presences").map(Val::as_list).unwrap_or(&[]) {
                    self.presence(p);
                }
                if let Some(m) = d.get("merged_presences") {
                    for p in m.get("friends").map(Val::as_list).unwrap_or(&[]) {
                        self.presence(p);
                    }
                    for g in m.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                        for p in g.as_list() {
                            self.presence(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Ids matching a mention, an id or part of a name.
    fn find(&self, query: &str) -> Vec<u64> {
        let q = query.trim().trim_start_matches("<@").trim_end_matches('>');
        let q = q.trim_start_matches('!');
        if let Ok(id) = q.parse::<u64>() {
            return vec![id];
        }
        let q = q.to_lowercase();
        let mut v: Vec<u64> = self
            .online
            .keys()
            .copied()
            .filter(|id| {
                self.users
                    .name(*id)
                    .is_some_and(|n| n.to_lowercase().contains(&q))
            })
            .collect();
        v.sort_unstable();
        v
    }
}

pub struct PlatformIndicators {
    manifest: Manifest,
    state: Shared<State>,
}

impl PlatformIndicators {
    pub fn new() -> PlatformIndicators {
        PlatformIndicators {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "PlatformIndicators",
                    env!("CARGO_PKG_VERSION"),
                    "Desktop, mobile and web indicators (collects client status)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("PlatformIndicators".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord platforms` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "platforms",
            "Show which platforms users are online on",
            move |_host: &mut dyn Host, args: &Args| {
                let st = state.lock();
                match args.get("user").map(str::trim).filter(|s| !s.is_empty()) {
                    Some(q) => {
                        let ids = st.find(q);
                        if ids.is_empty() {
                            return format!("No online user matches `{q}`.");
                        }
                        ids.iter()
                            .take(LIST_LIMIT)
                            .map(|id| {
                                let p = st.online.get(id).cloned().unwrap_or_default();
                                format!("{}: {}", st.users.display(*id), p.describe())
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                    None => {
                        if st.online.is_empty() {
                            return "No presence seen yet.".into();
                        }
                        let mut ids: Vec<u64> = st.online.keys().copied().collect();
                        ids.sort_unstable();
                        let mut out = format!("{} users online:", ids.len());
                        for id in ids.iter().take(LIST_LIMIT) {
                            out.push_str(&format!(
                                "\n{}: {}",
                                st.users.display(*id),
                                st.online[id].describe()
                            ));
                        }
                        if ids.len() > LIST_LIMIT {
                            out.push_str(&format!("\n... and {} more", ids.len() - LIST_LIMIT));
                        }
                        out
                    }
                }
            },
        )
        .option("user", "Mention, id or part of a name", false)
        .boxed()
    }
}

impl Default for PlatformIndicators {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for PlatformIndicators {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if is_one_of(name, EVENTS))
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event
            && is_one_of(name, EVENTS)
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

    fn args(user: Option<&str>) -> Args {
        Args {
            options: user
                .map(|s| vec![("user".to_string(), s.to_string())])
                .unwrap_or_default(),
        }
    }

    fn feed(chain: &Chain, h: &mut Cfg, name: &str, json: &str) -> Outcome {
        let mut d = j(json);
        chain.dispatch(h, name, &mut d, &mut Vec::new())
    }

    #[test]
    fn collects_presence_and_never_changes_traffic() {
        let plugin = PlatformIndicators::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        let out = feed(
            &chain,
            &mut h,
            "PRESENCE_UPDATE",
            r#"{"user":{"id":"5","username":"ann"},"status":"online",
               "client_status":{"desktop":"online","mobile":"idle"}}"#,
        );
        assert_eq!(out, Outcome::Untouched);
        let all = cmd.run(&mut h, &args(None));
        assert!(
            all.contains("ann: desktop (online), mobile (idle)"),
            "{all}"
        );
        assert!(cmd.run(&mut h, &args(Some("<@5>"))).contains("ann:"));
        assert!(cmd.run(&mut h, &args(Some("AN"))).contains("ann:"));
        // Going offline sends an empty client_status.
        feed(
            &chain,
            &mut h,
            "PRESENCE_UPDATE",
            r#"{"user":{"id":"5"},"status":"offline","client_status":{}}"#,
        );
        assert!(cmd.run(&mut h, &args(None)).contains("No presence"));
    }

    #[test]
    fn collects_presence_from_a_real_gateway_frame() {
        let plugin = PlatformIndicators::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        let out = feed(
            &chain,
            &mut h,
            "PRESENCE_UPDATE",
            r#"{"op":0,"t":"PRESENCE_UPDATE","s":1,"d":{"user":{"id":"5","username":"ann"},
               "status":"online","client_status":{"desktop":"online","mobile":"idle"}}}"#,
        );
        assert_eq!(out, Outcome::Untouched);
        assert!(
            cmd.run(&mut h, &args(None))
                .contains("ann: desktop (online), mobile (idle)")
        );
    }

    #[test]
    fn reads_ready_and_guild_create_presences() {
        let plugin = PlatformIndicators::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        feed(
            &chain,
            &mut h,
            "READY",
            r#"{"users":[{"id":"1","username":"bob"}],
               "merged_presences":{"friends":[{"user_id":"1","client_status":{"web":"dnd"}}],
               "guilds":[[{"user_id":"2","client_status":{"mobile":"online"}}]]}}"#,
        );
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"9","presences":[{"user":{"id":"3","username":"cy"},"client_status":{"desktop":"idle"}}]}"#,
        );
        let all = cmd.run(&mut h, &args(None));
        assert!(all.starts_with("3 users online"), "{all}");
        assert!(all.contains("bob: web (dnd)"));
        assert!(all.contains("<@2>: mobile (online)"));
        assert!(all.contains("cy: desktop (idle)"));
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let chain = Chain::new(vec![Box::new(PlatformIndicators::new())]);
        let mut h = Cfg::default();
        for (n, d) in [
            ("PRESENCE_UPDATE", "{}"),
            (
                "PRESENCE_UPDATE",
                r#"{"user":{"id":"x"},"client_status":5}"#,
            ),
            ("GUILD_CREATE", r#"{"presences":"no"}"#),
            ("READY", "null"),
        ] {
            assert_eq!(feed(&chain, &mut h, n, d), Outcome::Untouched);
        }
    }

    #[test]
    fn command_refuses_while_the_plugin_is_off() {
        let cmd = PlatformIndicators::new().command();
        let mut h = Cfg::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args(None)).contains("is off"));
    }

    #[test]
    fn store_is_bounded() {
        let mut st = State::default();
        for id in 1..=(CAP as u64 + 10) {
            st.set(
                id,
                Platforms::from_presence(&j(r#"{"client_status":{"web":"online"}}"#)),
            );
        }
        assert_eq!(st.online.len(), CAP);
        assert!(!st.online.contains_key(&1));
    }
}
