//! ShowHiddenThings: show hidden and moderator-only things (Vencord: ShowHiddenThings).
//!
//! Ported from Vencord `src/plugins/showHiddenThings/index.ts` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors Dolfies). Vencord patches the client so that things
//! Discord shows only to moderators or only in some places become visible to everyone: member
//! timeouts (`showTimeouts`), the "invites paused" state of a server (`showInvitesPaused`), the
//! moderator view of a member (`showModView`) and the discovery search filters
//! (`disableDiscoveryFilters`, `disableDisallowedDiscoveryFilters`).
//!
//! Most of that data already reaches the client; the client just hides it. This port collects it
//! from the gateway and shows it with a command, without changing any traffic:
//! - `communication_disabled_until` of members (timeouts) from `GUILD_MEMBER_ADD/UPDATE`,
//!   `GUILD_MEMBERS_CHUNK`, `GUILD_CREATE.members` and your own entry of `READY.merged_members`;
//! - per server: paused invites and DMs (`incidents_data.invites_disabled_until`,
//!   `dms_disabled_until`, the `INVITES_DISABLED` feature) and community/discovery features.
//!
//! `/phoecord hiddenthings [timeouts|servers]` prints what was seen; expired timeouts and
//! incidents are left out. It keeps ids, names and timestamps only, bounded ([`MAX_TIMEOUTS`]
//! members).
//!
//! Switching it on: `[plugins.show_hidden_things] enabled = true` (off by default). It never
//! rewrites a payload, so there is no extra switch and no added ban risk. The settings of
//! Vencord (`showTimeouts`, `showInvitesPaused`, `showModView`, `disableDiscoveryFilters`,
//! `disableDisallowedDiscoveryFilters`) are accepted by `import-vencord` but only mean something
//! once the on-screen part exists.
//!
//! Not ported (Phase 2, needs UI hooks): the timeout icon next to a member, the "invites paused"
//! banner in the invite dialog, the mod view side panel (it also needs the member's moderation
//! data, which Discord only sends to people with the permission), and the discovery filter
//! changes (they are client-side checks on the search request, not data).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, Users, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

const PLUGIN_ID: &str = "show_hidden_things";
/// Most timed-out members kept.
pub const MAX_TIMEOUTS: usize = 2000;
const LIST_LIMIT: usize = 40;
/// Guild features worth showing.
const FEATURES: &[&str] = &[
    "INVITES_DISABLED",
    "DISCOVERABLE",
    "COMMUNITY",
    "PREVIEW_ENABLED",
    "MEMBER_VERIFICATION_GATE_ENABLED",
    "RAID_ALERTS_DISABLED",
];
const EVENTS: &[&str] = &[
    "READY",
    "GUILD_CREATE",
    "GUILD_UPDATE",
    "GUILD_DELETE",
    "GUILD_MEMBER_ADD",
    "GUILD_MEMBER_UPDATE",
    "GUILD_MEMBER_REMOVE",
    "GUILD_MEMBERS_CHUNK",
];

#[derive(Default)]
struct GuildInfo {
    name: String,
    features: BTreeSet<String>,
    /// ISO timestamps from `incidents_data`.
    invites_until: Option<String>,
    dms_until: Option<String>,
}

struct State {
    guilds: HashMap<u64, GuildInfo>,
    /// (guild, user) -> ISO time the timeout ends.
    timeouts: BTreeMap<(u64, u64), String>,
    users: Users,
}

impl Default for State {
    fn default() -> State {
        State {
            guilds: HashMap::new(),
            timeouts: BTreeMap::new(),
            users: Users::with_capacity(2000),
        }
    }
}

/// Seconds since the epoch of an ISO 8601 UTC time (`2026-11-25T10:20:30.5+00:00`); the offset
/// is ignored because Discord always sends UTC.
fn iso_secs(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

fn active(until: &str, now: i64) -> bool {
    iso_secs(until).is_some_and(|t| t > now)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl State {
    fn guild(&mut self, g: &Val) {
        let Some(id) = g.id_at(&["id"]) else { return };
        if g.get("unavailable").and_then(Val::as_bool) == Some(true) && g.get("name").is_none() {
            return;
        }
        let info = self.guilds.entry(id).or_default();
        if let Some(n) = g
            .str_at(&["properties", "name"])
            .or_else(|| g.str_at(&["name"]))
        {
            info.name = n.to_string();
        }
        let feats = g
            .at(&["properties", "features"])
            .or_else(|| g.get("features"));
        if let Some(f) = feats {
            info.features = f
                .as_list()
                .iter()
                .filter_map(Val::as_str)
                .filter(|s| FEATURES.contains(s))
                .map(str::to_string)
                .collect();
        }
        let inc = g
            .get("incidents_data")
            .or_else(|| g.at(&["properties", "incidents_data"]));
        if let Some(inc) = inc {
            let get = |k: &str| inc.str_at(&[k]).map(str::to_string);
            info.invites_until = get("invites_disabled_until");
            info.dms_until = get("dms_disabled_until");
        }
        for m in g.get("members").map(Val::as_list).unwrap_or(&[]) {
            self.member(id, m);
        }
    }

    /// A member object (`user` inside, or `user_id` for merged members).
    fn member(&mut self, guild: u64, m: &Val) {
        let Some(user) = m.id_at(&["user", "id"]).or_else(|| m.id_at(&["user_id"])) else {
            return;
        };
        if let Some(v) = m.get("communication_disabled_until") {
            match v.as_str() {
                Some(until) => {
                    let known = self.timeouts.contains_key(&(guild, user));
                    if !known && self.timeouts.len() >= MAX_TIMEOUTS {
                        let now = now_secs();
                        self.timeouts.retain(|_, u| active(u, now));
                    }
                    if known || self.timeouts.len() < MAX_TIMEOUTS {
                        self.timeouts.insert((guild, user), until.to_string());
                    }
                }
                None => {
                    self.timeouts.remove(&(guild, user));
                }
            }
        }
    }

    fn observe(&mut self, name: &str, d: &Val) {
        self.users.observe(name, d);
        match name {
            "READY" => {
                let guilds = d.get("guilds").map(Val::as_list).unwrap_or(&[]);
                for g in guilds {
                    self.guild(g);
                }
                let merged = d.get("merged_members").map(Val::as_list).unwrap_or(&[]);
                for (g, list) in guilds.iter().zip(merged) {
                    if let Some(gid) = g.id_at(&["id"]) {
                        for m in list.as_list() {
                            self.member(gid, m);
                        }
                    }
                }
            }
            "GUILD_CREATE" | "GUILD_UPDATE" => self.guild(d),
            "GUILD_DELETE" => {
                if d.get("unavailable").and_then(Val::as_bool) != Some(true)
                    && let Some(id) = d.id_at(&["id"])
                {
                    self.guilds.remove(&id);
                    self.timeouts.retain(|(g, _), _| *g != id);
                }
            }
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
                if let Some(gid) = d.id_at(&["guild_id"]) {
                    self.member(gid, d);
                }
            }
            "GUILD_MEMBER_REMOVE" => {
                if let (Some(gid), Some(u)) = (d.id_at(&["guild_id"]), d.id_at(&["user", "id"])) {
                    self.timeouts.remove(&(gid, u));
                }
            }
            "GUILD_MEMBERS_CHUNK" => {
                if let Some(gid) = d.id_at(&["guild_id"]) {
                    for m in d.get("members").map(Val::as_list).unwrap_or(&[]) {
                        self.member(gid, m);
                    }
                }
            }
            _ => {}
        }
    }

    fn guild_name(&self, id: u64) -> String {
        self.guilds
            .get(&id)
            .map(|g| g.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("server {id}"))
    }

    fn render(&self, what: &str, now: i64) -> String {
        let mut sections = Vec::new();
        if what != "timeouts" {
            let mut lines = Vec::new();
            for (id, g) in &self.guilds {
                let mut tags = Vec::new();
                if g.invites_until.as_deref().is_some_and(|u| active(u, now))
                    || g.features.contains("INVITES_DISABLED")
                {
                    tags.push("invites paused");
                }
                if g.dms_until.as_deref().is_some_and(|u| active(u, now)) {
                    tags.push("DMs paused");
                }
                if g.features.contains("DISCOVERABLE") {
                    tags.push("discoverable");
                }
                if !tags.is_empty() {
                    lines.push(format!("- {}: {}", self.guild_name(*id), tags.join(", ")));
                }
            }
            lines.sort();
            if !lines.is_empty() {
                sections.push(format!("Servers:\n{}", lines.join("\n")));
            }
        }
        if what != "servers" {
            let live: Vec<_> = self
                .timeouts
                .iter()
                .filter(|(_, u)| active(u, now))
                .collect();
            if !live.is_empty() {
                let mut text = format!("Timed-out members ({}):", live.len());
                for ((g, u), until) in live.iter().take(LIST_LIMIT) {
                    text.push_str(&format!(
                        "\n- {} in {} until {}",
                        self.users.display(*u),
                        self.guild_name(*g),
                        until.get(..19).unwrap_or(until)
                    ));
                }
                if live.len() > LIST_LIMIT {
                    text.push_str(&format!("\n... and {} more", live.len() - LIST_LIMIT));
                }
                sections.push(text);
            }
        }
        if sections.is_empty() {
            "Nothing hidden seen yet.".into()
        } else {
            sections.join("\n")
        }
    }
}

pub struct ShowHiddenThings {
    manifest: Manifest,
    state: Shared<State>,
}

impl ShowHiddenThings {
    pub fn new() -> ShowHiddenThings {
        ShowHiddenThings {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ShowHiddenThings",
                    env!("CARGO_PKG_VERSION"),
                    "Show hidden and moderator-only things",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ShowHiddenThings".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord hiddenthings` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let st = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "hiddenthings",
            "Show timeouts, paused invites and other hidden server info",
            move |_, args| {
                let what = args.get("what").map(|s| s.trim().to_lowercase());
                st.lock()
                    .render(what.as_deref().unwrap_or("all"), now_secs())
            },
        )
        .option("what", "timeouts or servers; empty for both", false)
        .boxed()
    }
}

impl Default for ShowHiddenThings {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ShowHiddenThings {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if is_one_of(name, EVENTS))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event
            && is_one_of(name, EVENTS)
            && let Some(d) = payload.get("d")
        {
            self.state.lock().observe(name, d);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn feed(chain: &Chain, name: &str, json: &str) -> Outcome {
        let mut p = j(json);
        chain.dispatch(&mut Rec::default(), name, &mut p, &mut Vec::new())
    }

    fn pair() -> (Chain, ShowHiddenThings) {
        let p = ShowHiddenThings::new();
        let probe = ShowHiddenThings {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        (Chain::new(vec![Box::new(p)]), probe)
    }

    #[test]
    fn iso_parsing() {
        assert_eq!(iso_secs("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(iso_secs("2000-03-01T00:00:01.5+00:00"), Some(951_868_801));
        assert_eq!(iso_secs("nope"), None);
        assert_eq!(iso_secs("2026-13-01T00:00:00"), None);
        assert!(active("2999-01-01T00:00:00+00:00", now_secs()));
        assert!(!active("2001-01-01T00:00:00+00:00", now_secs()));
    }

    #[test]
    fn collects_timeouts_and_paused_invites() {
        let (chain, probe) = pair();
        let t = |s: &str| iso_secs(s).unwrap();
        let o = feed(
            &chain,
            "READY",
            r#"{"d":{"guilds":[{"id":"1","properties":{"name":"Alpha","features":["INVITES_DISABLED","FOO","DISCOVERABLE"]},
              "incidents_data":{"dms_disabled_until":"2999-01-01T00:00:00+00:00","invites_disabled_until":null}}],
              "merged_members":[[{"user_id":"5","roles":[],"communication_disabled_until":"2999-02-02T10:00:00.000000+00:00"}]]}}"#,
        );
        assert_eq!(o, Outcome::Untouched);
        feed(
            &chain,
            "GUILD_MEMBER_UPDATE",
            r#"{"d":{"guild_id":"1","user":{"id":"6","username":"bob"},"communication_disabled_until":"2999-03-03T00:00:00+00:00"}}"#,
        );
        feed(
            &chain,
            "GUILD_MEMBER_ADD",
            r#"{"d":{"guild_id":"1","user":{"id":"7","username":"old"},"communication_disabled_until":"2001-01-01T00:00:00+00:00"}}"#,
        );
        let st = probe.state.lock();
        let now = t("2026-11-25T00:00:00");
        let all = st.render("all", now);
        assert!(
            all.contains("Alpha: invites paused, DMs paused, discoverable"),
            "{all}"
        );
        assert!(all.contains("Timed-out members (2)"), "{all}");
        assert!(all.contains("bob in Alpha until 2999-03-03T00:00:00"));
        assert!(!all.contains("old"));
        assert!(!st.render("timeouts", now).contains("Servers:"));
        assert!(!st.render("servers", now).contains("Timed-out"));
        assert_eq!(st.guilds[&1].features.len(), 2);
    }

    #[test]
    fn timeout_cleared_and_member_removed() {
        let (chain, probe) = pair();
        let up = |until: &str| {
            format!(
                r#"{{"d":{{"guild_id":"1","user":{{"id":"6"}},"communication_disabled_until":{until}}}}}"#
            )
        };
        feed(
            &chain,
            "GUILD_MEMBER_UPDATE",
            &up(r#""2999-01-01T00:00:00+00:00""#),
        );
        assert_eq!(probe.state.lock().timeouts.len(), 1);
        feed(&chain, "GUILD_MEMBER_UPDATE", &up("null"));
        assert!(probe.state.lock().timeouts.is_empty());
        feed(
            &chain,
            "GUILD_MEMBERS_CHUNK",
            r#"{"d":{"guild_id":"1","members":[{"user":{"id":"8"},"communication_disabled_until":"2999-01-01T00:00:00+00:00"}]}}"#,
        );
        assert_eq!(probe.state.lock().timeouts.len(), 1);
        feed(
            &chain,
            "GUILD_MEMBER_REMOVE",
            r#"{"d":{"guild_id":"1","user":{"id":"8"}}}"#,
        );
        assert!(probe.state.lock().timeouts.is_empty());
    }

    #[test]
    fn malformed_payloads_and_off_plugin() {
        let (chain, probe) = pair();
        for (n, b) in [
            ("READY", "{}"),
            ("READY", r#"{"d":{"guilds":[1,2],"merged_members":"x"}}"#),
            ("GUILD_CREATE", r#"{"d":{"id":"1","members":[{"user":5}]}}"#),
            ("GUILD_MEMBER_UPDATE", r#"{"d":{}}"#),
            (
                "GUILD_MEMBERS_CHUNK",
                r#"{"d":{"guild_id":"1","members":"x"}}"#,
            ),
            ("GUILD_DELETE", r#"{"d":{"id":"zz"}}"#),
        ] {
            assert_eq!(feed(&chain, n, b), Outcome::Untouched);
        }
        let mut h = Rec::default();
        let cmd = probe.command();
        let args = Args {
            options: Vec::new(),
        };
        assert_eq!(cmd.run(&mut h, &args), "Nothing hidden seen yet.");
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args).contains("is off"));
        assert!(!probe.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
    }
}
