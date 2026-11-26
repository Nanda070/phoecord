//! ShowHiddenChannels: show channels you cannot open (Vencord: ShowHiddenChannels).
//!
//! Ported from Vencord `src/plugins/showHiddenChannels/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors BigDuck, AverageReactEnjoyer, D3SOX, Ven, Nuckyz,
//! Nickyux, Rini). Vencord patches the client's permission check so channels without `VIEW_CHANNEL`
//! stay in the channel list (with a lock icon) and open a "this channel is hidden" page. Discord
//! still sends those channels to the client; the client only filters them out.
//!
//! What this port does (data part):
//! - It watches `READY`, `GUILD_CREATE`, `GUILD_UPDATE`, `CHANNEL_*`, `GUILD_ROLE_*` and
//!   `GUILD_MEMBER_UPDATE`, works out with Discord's permission algorithm (guild roles, your roles,
//!   channel overwrites for `@everyone`, your roles and you) which channels deny you
//!   `VIEW_CHANNEL`, and remembers id, name, type and topic of each (IDs and names only, at most
//!   [`MAX_HIDDEN`] channels).
//! - `/phoecord hidden [guild]` lists them, grouped by server. `guild` is an id or part of a name.
//!
//! Limits of the calculation: it needs the guild's roles and your member roles in the payload
//! (`READY.merged_members` or a `members` entry of `GUILD_CREATE`). A guild where they are missing
//! is skipped and counted as "unknown" in the command. Role or member changes are applied to a
//! channel the next time its guild or channel payload arrives. Timeouts, thread permissions and
//! channels the server never sends cannot be seen. The owner and administrators see everything,
//! so nothing is listed for them.
//!
//! Switching it on:
//! - `[plugins.show_hidden_channels] enabled = true` (off by default, like every plugin).
//! - RISK, second switch, also off by default: `rewrite_payload = true` rewrites the gateway
//!   payload so the client lists hidden channels (Vencord's approach, done on the wire). For each
//!   hidden channel it adds or edits YOUR member overwrite so it allows `VIEW_CHANNEL`. The
//!   client then shows the channel and tries to load it; Discord answers 403 for every request
//!   it makes there, and the client may report those errors. A client sending requests for
//!   channels it should not see is exactly the pattern that gets accounts flagged. Rewriting is
//!   the most ban-prone thing a plugin here can do: leave it off unless you accept that.
//!
//! Not ported: the lock icons, the "hidden channel" page with the allowed users and roles, the
//! `showMode`, `hideUnreads` and `defaultAllowedUsersAndRolesDropdownState` settings (Phase 2).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{BTreeMap, HashMap};

const PLUGIN_ID: &str = "show_hidden_channels";
const VIEW_CHANNEL: u64 = 1 << 10;
const ADMINISTRATOR: u64 = 1 << 3;
/// Most hidden channels kept in memory.
pub const MAX_HIDDEN: usize = 5000;
const TOPIC_LEN: usize = 200;
const LIST_LIMIT: usize = 40;

const EVENTS: &[&str] = &[
    "READY",
    "GUILD_CREATE",
    "GUILD_UPDATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
    "GUILD_ROLE_CREATE",
    "GUILD_ROLE_UPDATE",
    "GUILD_ROLE_DELETE",
    "GUILD_MEMBER_UPDATE",
];

/// One channel that denies you `VIEW_CHANNEL`.
#[derive(Clone, Debug, PartialEq)]
pub struct HiddenChannel {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    pub kind: i64,
    pub topic: Option<String>,
}

struct Overwrite {
    id: u64,
    member: bool,
    allow: u64,
    deny: u64,
}

#[derive(Default)]
struct GuildCtx {
    name: String,
    owner: Option<u64>,
    /// role id -> permissions (the `@everyone` role has the guild's id).
    roles: HashMap<u64, u64>,
    /// Your role ids; `None` until a payload tells us.
    my_roles: Option<Vec<u64>>,
}

#[derive(Default)]
struct State {
    me: Option<u64>,
    guilds: HashMap<u64, GuildCtx>,
    hidden: BTreeMap<u64, HiddenChannel>,
    /// Guilds whose permissions could not be worked out.
    unknown: usize,
}

/// Permission bits from a JSON string or an ETF integer.
fn perms(v: Option<&Val>) -> u64 {
    match v {
        Some(Val::Str(s)) => s.parse().unwrap_or(0),
        Some(v) => v.snowflake().unwrap_or(0),
        None => 0,
    }
}

fn overwrites(ch: &Val) -> Vec<Overwrite> {
    ch.get("permission_overwrites")
        .map(Val::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|o| {
            let member = match o.get("type") {
                Some(Val::Str(s)) => s == "1" || s == "member",
                Some(Val::Atom(s)) => s == "member",
                Some(t) => t.as_i64() == Some(1),
                None => false,
            };
            Some(Overwrite {
                id: o.id_at(&["id"])?,
                member,
                allow: perms(o.get("allow")),
                deny: perms(o.get("deny")),
            })
        })
        .collect()
}

fn role_ids(member: &Val) -> Option<Vec<u64>> {
    member
        .get("roles")
        .map(|r| r.as_list().iter().filter_map(Val::snowflake).collect())
}

fn trim_topic(t: &str) -> String {
    t.chars().take(TOPIC_LEN).collect()
}

impl GuildCtx {
    /// `Some(true)` you can see the channel, `Some(false)` you cannot, `None` unknown.
    fn can_view(&self, guild_id: u64, me: u64, ows: &[Overwrite]) -> Option<bool> {
        if self.owner == Some(me) {
            return Some(true);
        }
        let mine = self.my_roles.as_ref()?;
        let everyone = *self.roles.get(&guild_id)?;
        let mut p = everyone;
        for r in mine {
            p |= self.roles.get(r).copied().unwrap_or(0);
        }
        if p & ADMINISTRATOR != 0 {
            return Some(true);
        }
        if let Some(o) = ows.iter().find(|o| !o.member && o.id == guild_id) {
            p = (p & !o.deny) | o.allow;
        }
        let (mut allow, mut deny) = (0, 0);
        for o in ows.iter().filter(|o| !o.member && mine.contains(&o.id)) {
            allow |= o.allow;
            deny |= o.deny;
        }
        p = (p & !deny) | allow;
        if let Some(o) = ows.iter().find(|o| o.member && o.id == me) {
            p = (p & !o.deny) | o.allow;
        }
        Some(p & VIEW_CHANNEL != 0)
    }
}

/// An id or permission value shaped like `sample`: a string if `sample` is one (JSON), else an
/// integer (ETF).
fn like(sample: Option<&Val>, n: u64) -> Val {
    match sample {
        Some(Val::Str(_)) | None => Val::Str(n.to_string()),
        _ => Val::Int(n as i64),
    }
}

/// Make YOUR member overwrite on this channel allow `VIEW_CHANNEL` (the rewrite).
fn grant_view(ch: &mut Val, me: u64) {
    let id_sample = ch.get("id").cloned();
    let atoms = matches!(&*ch, Val::Map(m) if matches!(m.first(), Some((Val::Atom(_), _))));
    if ch.get("permission_overwrites").is_none() {
        ch.set("permission_overwrites", Val::List(Vec::new()));
    }
    let Some(list) = ch
        .get_mut("permission_overwrites")
        .and_then(Val::as_list_mut)
    else {
        return;
    };
    let existing = list.iter_mut().find(|o| {
        o.id_at(&["id"]) == Some(me)
            && match o.get("type") {
                Some(Val::Str(s)) => s == "1" || s == "member",
                Some(Val::Atom(s)) => s == "member",
                Some(t) => t.as_i64() == Some(1),
                None => false,
            }
    });
    if let Some(o) = existing {
        let allow = perms(o.get("allow")) | VIEW_CHANNEL;
        let deny = perms(o.get("deny")) & !VIEW_CHANNEL;
        let (a, d) = (like(o.get("allow"), allow), like(o.get("deny"), deny));
        o.set("allow", a);
        o.set("deny", d);
        return;
    }
    // New entry, with atom keys when the channel map uses them (ETF).
    let mut o = if atoms {
        Val::Map(vec![(Val::Atom("id".into()), Val::Null)])
    } else {
        Val::map()
    };
    o.set("id", like(id_sample.as_ref(), me));
    o.set("type", Val::Int(1));
    o.set("allow", like(id_sample.as_ref(), VIEW_CHANNEL));
    o.set("deny", like(id_sample.as_ref(), 0));
    list.push(o);
}

impl State {
    fn handle(&mut self, name: &str, d: &mut Val, me: Option<u64>, rewrite: bool) -> bool {
        if me.is_some() {
            self.me = me;
        }
        match name {
            "READY" => {
                if let Some(u) = d.id_at(&["user", "id"]) {
                    self.me = Some(u);
                }
                // merged_members[i] belongs to guilds[i]; it holds your own member.
                let mine: Vec<Option<Vec<u64>>> = d
                    .get("merged_members")
                    .map(Val::as_list)
                    .unwrap_or(&[])
                    .iter()
                    .map(|l| l.as_list().first().and_then(role_ids))
                    .collect();
                let mut changed = false;
                if let Some(gs) = d.get_mut("guilds").and_then(Val::as_list_mut) {
                    for (i, g) in gs.iter_mut().enumerate() {
                        changed |= self.guild(g, mine.get(i).cloned().flatten(), rewrite);
                    }
                }
                changed
            }
            "GUILD_CREATE" => self.guild(d, None, rewrite),
            "GUILD_UPDATE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    let ctx = self.guilds.entry(id).or_default();
                    if let Some(n) = d.str_at(&["name"]) {
                        ctx.name = n.to_string();
                    }
                    if let Some(o) = d.id_at(&["owner_id"]) {
                        ctx.owner = Some(o);
                    }
                }
                false
            }
            "GUILD_DELETE" => {
                if d.get("unavailable").and_then(Val::as_bool) != Some(true)
                    && let Some(id) = d.id_at(&["id"])
                {
                    self.guilds.remove(&id);
                    self.hidden.retain(|_, c| c.guild_id != id);
                }
                false
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => match d.id_at(&["guild_id"]) {
                Some(gid) => self.channel(gid, d, rewrite),
                None => false,
            },
            "CHANNEL_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.hidden.remove(&id);
                }
                false
            }
            "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
                if let (Some(gid), Some(r)) = (d.id_at(&["guild_id"]), d.get("role"))
                    && let Some(rid) = r.id_at(&["id"])
                {
                    let p = perms(r.get("permissions"));
                    self.guilds.entry(gid).or_default().roles.insert(rid, p);
                }
                false
            }
            "GUILD_ROLE_DELETE" => {
                if let (Some(gid), Some(rid)) = (d.id_at(&["guild_id"]), d.id_at(&["role_id"])) {
                    self.guilds.entry(gid).or_default().roles.remove(&rid);
                }
                false
            }
            "GUILD_MEMBER_UPDATE" => {
                if let (Some(gid), Some(u), Some(r)) = (
                    d.id_at(&["guild_id"]),
                    d.id_at(&["user", "id"]),
                    role_ids(d),
                ) && Some(u) == self.me
                {
                    self.guilds.entry(gid).or_default().my_roles = Some(r);
                }
                false
            }
            _ => false,
        }
    }

    /// A full guild object (READY entry or GUILD_CREATE).
    fn guild(&mut self, g: &mut Val, mine: Option<Vec<u64>>, rewrite: bool) -> bool {
        let Some(gid) = g.id_at(&["id"]) else {
            return false;
        };
        if g.get("channels").is_none() && g.get("roles").is_none() {
            return false; // an unavailable stub
        }
        let me = self.me;
        let found = mine.or_else(|| {
            let listed = g.get("members").map(Val::as_list).unwrap_or(&[]);
            listed
                .iter()
                .find(|m| me.is_some() && m.id_at(&["user", "id"]).or(m.id_at(&["user_id"])) == me)
                .or_else(|| g.get("member"))
                .and_then(role_ids)
        });
        let name = g
            .str_at(&["properties", "name"])
            .or_else(|| g.str_at(&["name"]))
            .map(str::to_string);
        let owner = g
            .id_at(&["properties", "owner_id"])
            .or_else(|| g.id_at(&["owner_id"]));
        let roles: Option<HashMap<u64, u64>> = g.get("roles").map(|l| {
            l.as_list()
                .iter()
                .filter_map(|r| Some((r.id_at(&["id"])?, perms(r.get("permissions")))))
                .collect()
        });
        let ctx = self.guilds.entry(gid).or_default();
        if let Some(n) = name {
            ctx.name = n;
        }
        if owner.is_some() {
            ctx.owner = owner;
        }
        if let Some(r) = roles {
            ctx.roles = r;
        }
        if found.is_some() {
            ctx.my_roles = found;
        }
        self.hidden.retain(|_, c| c.guild_id != gid);
        let mut changed = false;
        let mut unknown = false;
        if let Some(chs) = g.get_mut("channels").and_then(Val::as_list_mut) {
            for ch in chs {
                match self.eval(gid, ch, rewrite) {
                    Some(c) => changed |= c,
                    None => unknown = true,
                }
            }
        }
        if unknown {
            self.unknown += 1;
        }
        changed
    }

    fn channel(&mut self, gid: u64, ch: &mut Val, rewrite: bool) -> bool {
        if let Some(id) = ch.id_at(&["id"]) {
            self.hidden.remove(&id);
        }
        self.eval(gid, ch, rewrite).unwrap_or(false)
    }

    /// Record the channel if hidden; `Some(rewritten)`, or `None` if the guild is unknown.
    fn eval(&mut self, gid: u64, ch: &mut Val, rewrite: bool) -> Option<bool> {
        let id = ch.id_at(&["id"])?;
        let me = self.me?;
        let visible = self.guilds.get(&gid)?.can_view(gid, me, &overwrites(ch))?;
        if visible {
            return Some(false);
        }
        if self.hidden.len() < MAX_HIDDEN {
            self.hidden.insert(
                id,
                HiddenChannel {
                    id,
                    guild_id: gid,
                    name: ch.str_at(&["name"]).unwrap_or_default().to_string(),
                    kind: ch.get("type").and_then(Val::as_i64).unwrap_or(0),
                    topic: ch
                        .str_at(&["topic"])
                        .filter(|t| !t.is_empty())
                        .map(trim_topic),
                },
            );
        }
        if rewrite {
            grant_view(ch, me);
        }
        Some(rewrite)
    }
}

fn kind_name(kind: i64) -> &'static str {
    match kind {
        0 => "text",
        2 => "voice",
        4 => "category",
        5 => "announcement",
        13 => "stage",
        14 => "directory",
        15 => "forum",
        16 => "media",
        _ => "other",
    }
}

pub struct ShowHiddenChannels {
    manifest: Manifest,
    state: Shared<State>,
}

impl ShowHiddenChannels {
    pub fn new() -> ShowHiddenChannels {
        ShowHiddenChannels {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ShowHiddenChannels",
                    env!("CARGO_PKG_VERSION"),
                    "Show channels you cannot open",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ShowHiddenChannels".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord hidden` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let st = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "hidden",
            "List channels you cannot open",
            move |host, args| {
                let st = st.lock();
                let filter = args.get("guild").map(|s| s.trim().to_lowercase());
                let filter = filter.filter(|f| !f.is_empty());
                let mut by_guild: BTreeMap<(String, u64), Vec<&HiddenChannel>> = BTreeMap::new();
                for c in st.hidden.values() {
                    let gname = st
                        .guilds
                        .get(&c.guild_id)
                        .map(|g| g.name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| format!("server {}", c.guild_id));
                    if let Some(f) = &filter
                        && !gname.to_lowercase().contains(f)
                        && c.guild_id.to_string() != *f
                    {
                        continue;
                    }
                    by_guild.entry((gname, c.guild_id)).or_default().push(c);
                }
                let rewrite =
                    host.setting("rewrite_payload").and_then(|v| v.as_bool()) == Some(true);
                let mut out = String::new();
                let total: usize = by_guild.values().map(Vec::len).sum();
                if total == 0 {
                    out.push_str("No hidden channels seen.");
                } else {
                    out.push_str(&format!("Hidden channels ({total}):"));
                }
                let mut shown = 0;
                'outer: for ((gname, _), chs) in &by_guild {
                    out.push_str(&format!("\n**{gname}**"));
                    for c in chs {
                        if shown >= LIST_LIMIT {
                            out.push_str(&format!("\n... and {} more", total - shown));
                            break 'outer;
                        }
                        shown += 1;
                        out.push_str(&format!("\n- {} ({}, {})", c.name, kind_name(c.kind), c.id));
                        if let Some(t) = &c.topic {
                            out.push_str(&format!(": {t}"));
                        }
                    }
                }
                if st.unknown > 0 {
                    out.push_str(&format!(
                        "\n{} server(s) could not be checked (roles not in the payload).",
                        st.unknown
                    ));
                }
                out.push_str(if rewrite {
                    "\nRewrite is **on**: the client is shown these channels."
                } else {
                    "\nRewrite is off (`rewrite_payload`): the client list is unchanged."
                });
                out
            },
        )
        .option("guild", "Server id or part of its name", false)
        .boxed()
    }
}

impl Default for ShowHiddenChannels {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ShowHiddenChannels {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if is_one_of(name, EVENTS))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch { name, payload, .. } = event else {
            return Verdict::Pass;
        };
        if !is_one_of(name, EVENTS) {
            return Verdict::Pass;
        }
        let Some(d) = payload.get_mut("d") else {
            return Verdict::Pass;
        };
        let rewrite = host.setting("rewrite_payload").and_then(|v| v.as_bool()) == Some(true);
        let me = host.self_id();
        if self.state.lock().handle(name, d, me, rewrite) {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    const ME: u64 = 5;

    /// Guild 1: roles 1 (@everyone, view) and 20 (mine). Channels: 11 open, 12 denied to
    /// @everyone, 13 denied to @everyone but allowed to role 20, 14 denied to me by name.
    fn guild_json() -> String {
        r#"{"op":0,"t":"GUILD_CREATE","d":{"id":"1","name":"Alpha","owner_id":"99",
        "roles":[{"id":"1","permissions":"1024"},{"id":"20","permissions":"0"}],
        "members":[{"user":{"id":"5"},"roles":["20"]}],
        "channels":[
          {"id":"11","name":"general","type":0,"permission_overwrites":[]},
          {"id":"12","name":"staff","type":0,"topic":"mods only","permission_overwrites":[
             {"id":"1","type":0,"allow":"0","deny":"1024"}]},
          {"id":"13","name":"vip","type":2,"permission_overwrites":[
             {"id":"1","type":0,"allow":"0","deny":"1024"},
             {"id":"20","type":0,"allow":"1024","deny":"0"}]},
          {"id":"14","name":"secret","type":0,"permission_overwrites":[
             {"id":"5","type":1,"allow":"0","deny":"1024"}]}
        ]}}"#
            .to_string()
    }

    fn host() -> Rec {
        Rec {
            self_id: Some(ME),
            ..Rec::default()
        }
    }

    fn run(chain: &Chain, h: &mut Rec, name: &str, json: &str) -> (Outcome, Val) {
        let mut p = j(json);
        let out = chain.dispatch(h, name, &mut p, &mut Vec::new());
        (out, p)
    }

    fn cmd_text(p: &ShowHiddenChannels, h: &mut Rec, guild: Option<&str>) -> String {
        let args = Args {
            options: guild
                .map(|g| vec![("guild".to_string(), g.to_string())])
                .unwrap_or_default(),
        };
        p.command().run(h, &args)
    }

    #[test]
    fn collects_channels_that_deny_view() {
        let p = ShowHiddenChannels::new();
        let cmd_plugin = ShowHiddenChannels {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = host();
        let (out, _) = run(&chain, &mut h, "GUILD_CREATE", &guild_json());
        assert_eq!(out, Outcome::Untouched);
        let text = cmd_text(&cmd_plugin, &mut h, None);
        assert!(text.contains("Hidden channels (2)"), "{text}");
        assert!(text.contains("staff (text, 12): mods only"));
        assert!(text.contains("secret (text, 14)"));
        assert!(!text.contains("general"));
        assert!(!text.contains("vip"));
        assert!(text.contains("Rewrite is off"));
        assert!(cmd_text(&cmd_plugin, &mut h, Some("nomatch")).contains("No hidden"));
        assert!(cmd_text(&cmd_plugin, &mut h, Some("alp")).contains("staff"));
    }

    #[test]
    fn owner_and_admin_see_everything_and_deleted_channels_vanish() {
        let p = ShowHiddenChannels::new();
        let probe = ShowHiddenChannels {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = host();
        let owner = guild_json().replace(r#""owner_id":"99""#, r#""owner_id":"5""#);
        run(&chain, &mut h, "GUILD_CREATE", &owner);
        assert!(probe.state.lock().hidden.is_empty());
        let admin = guild_json().replace(r#""permissions":"0""#, r#""permissions":"8""#);
        run(&chain, &mut h, "GUILD_CREATE", &admin);
        assert!(probe.state.lock().hidden.is_empty());
        run(&chain, &mut h, "GUILD_CREATE", &guild_json());
        assert_eq!(probe.state.lock().hidden.len(), 2);
        run(
            &chain,
            &mut h,
            "CHANNEL_DELETE",
            r#"{"d":{"id":"12","guild_id":"1"}}"#,
        );
        assert_eq!(probe.state.lock().hidden.len(), 1);
        // A channel update that opens the channel up removes it.
        run(
            &chain,
            &mut h,
            "CHANNEL_UPDATE",
            r#"{"d":{"id":"14","guild_id":"1","name":"secret","type":0,"permission_overwrites":[]}}"#,
        );
        assert!(probe.state.lock().hidden.is_empty());
    }

    #[test]
    fn rewrite_off_leaves_payload_alone_and_on_grants_view() {
        let chain = Chain::new(vec![Box::new(ShowHiddenChannels::new())]);
        let mut h = host().with("rewrite_payload", Val::Bool(true));
        let (out, p) = run(&chain, &mut h, "GUILD_CREATE", &guild_json());
        assert_eq!(out, Outcome::Changed);
        let chs = p.at(&["d", "channels"]).unwrap().as_list().to_vec();
        // Channel 12 had no overwrite for me: one was added.
        let ows = chs[1].get("permission_overwrites").unwrap().as_list();
        let mine = ows.iter().find(|o| o.id_at(&["id"]) == Some(ME)).unwrap();
        assert_eq!(mine.str_at(&["allow"]), Some("1024"));
        assert_eq!(mine.get("type").and_then(Val::as_i64), Some(1));
        // Channel 14 had my deny: now allowed, deny cleared.
        let ows = chs[3].get("permission_overwrites").unwrap().as_list();
        assert_eq!(ows[0].str_at(&["deny"]), Some("0"));
        assert_eq!(ows[0].str_at(&["allow"]), Some("1024"));
        // Visible channels untouched.
        assert!(
            chs[0]
                .get("permission_overwrites")
                .unwrap()
                .as_list()
                .is_empty()
        );
        // Same payload with the setting off is untouched.
        let mut off = host();
        let (out, _) = run(&chain, &mut off, "GUILD_CREATE", &guild_json());
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn ready_uses_merged_members_and_malformed_payloads_do_not_panic() {
        let p = ShowHiddenChannels::new();
        let probe = ShowHiddenChannels {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default(); // no self_id: READY supplies it
        let ready = r#"{"d":{"user":{"id":"5"},"merged_members":[[{"user_id":"5","roles":["20"]}]],
            "guilds":[{"id":"1","properties":{"name":"Alpha","owner_id":"99"},
            "roles":[{"id":"1","permissions":"1024"},{"id":"20","permissions":"0"}],
            "channels":[{"id":"12","name":"staff","type":0,"permission_overwrites":[
             {"id":"1","type":0,"allow":"0","deny":"1024"}]}]}]}}"#;
        run(&chain, &mut h, "READY", ready);
        assert_eq!(probe.state.lock().hidden.len(), 1);
        for (name, body) in [
            ("READY", "{}"),
            ("READY", r#"{"d":{"guilds":"x"}}"#),
            ("GUILD_CREATE", r#"{"d":{"id":"7"}}"#),
            (
                "GUILD_CREATE",
                r#"{"d":{"id":"7","channels":[{"name":"x"}]}}"#,
            ),
            ("CHANNEL_UPDATE", r#"{"d":{"id":"3"}}"#),
            ("GUILD_ROLE_UPDATE", r#"{"d":{"guild_id":"1","role":{}}}"#),
            ("GUILD_MEMBER_UPDATE", r#"{"d":{"guild_id":"1"}}"#),
            ("CHANNEL_DELETE", r#"{"d":{}}"#),
            ("GUILD_DELETE", r#"{"d":{"id":"1"}}"#),
        ] {
            run(&chain, &mut h, name, body);
        }
        assert!(probe.state.lock().hidden.is_empty());
    }

    #[test]
    fn unknown_roles_are_reported_and_off_plugin_answers_off() {
        let p = ShowHiddenChannels::new();
        let probe = ShowHiddenChannels {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = host();
        // No roles and no members in the payload: cannot be computed.
        run(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"d":{"id":"2","name":"Beta","channels":[{"id":"21","name":"x","type":0}]}}"#,
        );
        assert!(probe.state.lock().hidden.is_empty());
        let text = cmd_text(&probe, &mut h, None);
        assert!(text.contains("1 server(s) could not be checked"), "{text}");
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd_text(&probe, &mut h, None).contains("is off"));
        assert!(probe.wants(&EventKind::Dispatch { name: "READY" }));
        assert!(!probe.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
    }
}
