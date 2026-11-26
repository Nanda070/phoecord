//! ServerInfo: facts about a server (Vencord: ServerInfo), data part.
//!
//! Ported from Vencord `src/plugins/serverInfo/` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; authors Ven, Nuckyz). Vencord adds "Server Info" to the server
//! menu and opens a modal with the icon, banner, name, description and these fields: Server
//! Owner, Created At, Joined At, Vanity Link, Preferred Locale, Verification Level, Server Boosts,
//! Channels, Roles. The menu and the modal wait for Phase 2; the same fields are answered by
//! `/phoecord serverinfo [guild:<id>]` (default: the server the command was typed in), from
//! `Host::rest`:
//!
//! - `GET /guilds/{id}?with_counts=true`: name, description, owner, vanity code, locale,
//!   verification level, boosts, roles, approximate member and online counts (an addition);
//! - `GET /guilds/{id}/channels`: channels, without categories (Vencord subtracts the null
//!   category of its store);
//! - `GET /users/@me/guilds/{id}/member`: when you joined.
//!
//! The Friends, Blocked Users and Ignored Users tabs (task 2.4.2 b): your relationships come from
//! `READY` and the `RELATIONSHIP_*` events (type 1 friend, type 2 blocked, `user_ignored`); the
//! command sends **one** gateway op 8 (Request Guild Members) for the server with those user ids
//! (at most [`MAX_IDS`], Discord's limit) and a nonce of its own, and the `GUILD_MEMBERS_CHUNK`s
//! with that nonce are collected and dropped (the client never asked for them). When the last
//! chunk is in, the three lists come as a local message only you see, in the channel the command
//! was typed in. A REST member lookup per user would hit the per-plugin rate limit.

use crate::commands::{Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer};
use crate::state::dispatch_data;
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

const PLUGIN_ID: &str = "server_info";
const SUB: &str = "serverinfo";
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
/// Channel type of a category.
const CATEGORY: i64 = 4;
/// Discord's limit on `user_ids` in one op 8.
const MAX_IDS: usize = 100;
/// Lookups waiting for their chunks, at most (the oldest is forgotten).
const MAX_PENDING: usize = 8;
/// Relationship types.
const FRIEND: i64 = 1;
const BLOCKED: i64 = 2;
/// Our nonces start with this, so the client's own op 8 answers are left alone.
const NONCE_PREFIX: &str = "phoecord-serverinfo-";

#[derive(Default)]
struct Relations {
    friends: HashSet<u64>,
    blocked: HashSet<u64>,
    ignored: HashSet<u64>,
}

struct Pending {
    channel: u64,
    asked: Vec<u64>,
    found: HashSet<u64>,
    chunks_seen: i64,
    order: u64,
}

#[derive(Default)]
struct State {
    rel: Relations,
    pending: HashMap<String, Pending>,
    counter: u64,
}

fn relationship_user(r: &Val) -> Option<u64> {
    r.id_at(&["id"])
        .or_else(|| r.id_at(&["user_id"]))
        .or_else(|| r.id_at(&["user", "id"]))
}

impl Relations {
    fn set(&mut self, r: &Val) {
        let Some(id) = relationship_user(r) else {
            return;
        };
        let kind = r.at(&["type"]).and_then(Val::as_i64);
        let ignored = r.at(&["user_ignored"]).and_then(Val::as_bool) == Some(true);
        for (set, on) in [
            (&mut self.friends, kind == Some(FRIEND)),
            (&mut self.blocked, kind == Some(BLOCKED)),
            (&mut self.ignored, ignored),
        ] {
            if on {
                set.insert(id);
            } else {
                set.remove(&id);
            }
        }
    }

    fn track(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                *self = Relations::default();
                for r in d.get("relationships").map(Val::as_list).unwrap_or_default() {
                    self.set(r);
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => self.set(d),
            "RELATIONSHIP_REMOVE" => {
                if let Some(id) = relationship_user(d) {
                    self.friends.remove(&id);
                    self.blocked.remove(&id);
                    self.ignored.remove(&id);
                }
            }
            _ => {}
        }
    }

    /// Everyone in any of the three lists, friends first, sorted within each.
    fn ids(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for set in [&self.friends, &self.blocked, &self.ignored] {
            let mut v: Vec<u64> = set.iter().copied().filter(|i| !out.contains(i)).collect();
            v.sort_unstable();
            out.extend(v);
        }
        out
    }
}

/// Vencord's names for the verification levels.
const VERIFICATION: &[&str] = &["None", "Low", "Medium", "High", "Highest"];

pub struct ServerInfo {
    manifest: Manifest,
    kept: Kept,
    state: Mutex<State>,
}

impl ServerInfo {
    pub fn new() -> ServerInfo {
        ServerInfo {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ServerInfo",
                    env!("CARGO_PKG_VERSION"),
                    "Show facts about a server: owner, age, boosts, channels, roles",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ServerInfo".into());
                m
            },
            kept: Kept::default(),
            state: Mutex::new(State::default()),
        }
    }

    /// Send the op 8 for `guild` and remember where to answer; the line to add to the answer.
    fn ask_members(&self, host: &mut dyn Host, guild: u64, channel: Option<u64>) -> String {
        let mut st = self.state.lock().unwrap();
        let mut ids = st.rel.ids();
        if ids.is_empty() {
            return String::new();
        }
        let Some(channel) = channel else {
            return String::new();
        };
        let cut = ids.len() > MAX_IDS;
        ids.truncate(MAX_IDS);
        st.counter += 1;
        let nonce = format!("{NONCE_PREFIX}{}", st.counter);
        let mut d = Val::map();
        d.set("guild_id", Val::Str(guild.to_string()));
        d.set(
            "user_ids",
            Val::List(ids.iter().map(|i| Val::Str(i.to_string())).collect()),
        );
        d.set("presences", Val::Bool(false));
        d.set("nonce", Val::Str(nonce.clone()));
        let mut frame = Val::map();
        frame.set("op", Val::Int(8));
        frame.set("d", d);
        host.emit_send(8, frame);
        if st.pending.len() >= MAX_PENDING
            && let Some(oldest) = st
                .pending
                .iter()
                .min_by_key(|(_, p)| p.order)
                .map(|(k, _)| k.clone())
        {
            st.pending.remove(&oldest);
        }
        let order = st.counter;
        st.pending.insert(
            nonce,
            Pending {
                channel,
                asked: ids,
                found: HashSet::new(),
                chunks_seen: 0,
                order,
            },
        );
        let key = if cut {
            "server_info-relations-asked-cut"
        } else {
            "server_info-relations-asked"
        };
        host.t(key, &[("max", &MAX_IDS.to_string())])
    }

    /// A `GUILD_MEMBERS_CHUNK` with one of our nonces: collect it, answer after the last one.
    /// `true` when it was ours (and is dropped).
    fn chunk(&self, host: &mut dyn Host, d: &Val) -> bool {
        let Some(nonce) = d.str_at(&["nonce"]).filter(|n| n.starts_with(NONCE_PREFIX)) else {
            return false;
        };
        let mut st = self.state.lock().unwrap();
        let Some(p) = st.pending.get_mut(nonce) else {
            return true;
        };
        for m in d.get("members").map(Val::as_list).unwrap_or_default() {
            if let Some(id) = m.id_at(&["user", "id"]) {
                p.found.insert(id);
            }
        }
        p.chunks_seen += 1;
        let count = d.at(&["chunk_count"]).and_then(Val::as_i64).unwrap_or(1);
        if p.chunks_seen < count {
            return true;
        }
        let p = st.pending.remove(nonce).expect("present above");
        let list = |set: &HashSet<u64>| {
            let ids: Vec<String> = p
                .asked
                .iter()
                .filter(|i| set.contains(i) && p.found.contains(i))
                .map(|i| format!("<@{i}>"))
                .collect();
            if ids.is_empty() {
                "-".to_string()
            } else {
                ids.join(", ")
            }
        };
        let text = [
            host.t("server_info-friends", &[("value", &list(&st.rel.friends))]),
            host.t("server_info-blocked", &[("value", &list(&st.rel.blocked))]),
            host.t("server_info-ignored", &[("value", &list(&st.rel.ignored))]),
        ]
        .join("\n");
        host.emit_dispatch(crate::commands::local_message(p.channel, &text));
        true
    }

    /// `/phoecord serverinfo`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(PLUGIN_ID, SUB, "Show facts about a server", &self.kept)
            .option("guild", "Server id (default: this server)", false)
            .boxed()
    }
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self::new()
    }
}

fn get(host: &mut dyn Host, route: &str) -> Result<Val, String> {
    match host.rest("GET", route, None)? {
        HostResponse {
            status: 200..=299,
            body,
            ..
        } => Ok(body.unwrap_or(Val::Null)),
        HostResponse { status, .. } => Err(format!("HTTP {status}")),
    }
}

/// Unix seconds a snowflake was made at.
fn created_at(id: u64) -> u64 {
    ((id >> 22) + DISCORD_EPOCH_MS) / 1000
}

/// The answer, and the server when its facts were found (then the relationship lookup follows).
fn run(host: &mut dyn Host, inv: &Invocation) -> (String, Option<u64>) {
    let guild = match inv.get("guild") {
        Some(g) => match crate::commands::snowflake_arg(g) {
            Some(id) => id,
            None => return (host.t("server_info-bad-guild", &[]), None),
        },
        None => match inv.guild_id {
            Some(id) => id,
            None => return (host.t("server_info-bad-guild", &[]), None),
        },
    };
    let g = match get(host, &format!("/guilds/{guild}?with_counts=true")) {
        Ok(g) => g,
        Err(e) => return (host.t("server_info-failed", &[("error", &e)]), None),
    };
    // Both are extras: a refusal only leaves the field unknown.
    let channels = get(host, &format!("/guilds/{guild}/channels")).ok();
    let member = get(host, &format!("/users/@me/guilds/{guild}/member")).ok();

    let dash = "-".to_string();
    let text = |k: &str| g.str_at(&[k]).filter(|s| !s.is_empty()).map(str::to_string);
    let int = |k: &str| g.get(k).and_then(Val::as_i64);
    let owner = g
        .id_at(&["owner_id"])
        .map_or_else(|| "?".to_string(), |id| format!("<@{id}>"));
    let joined = member
        .as_ref()
        .and_then(|m| m.str_at(&["joined_at"]))
        .and_then(crate::friend_invites::iso_unix)
        .map_or_else(|| dash.clone(), |t| format!("<t:{t}:F>"));
    let vanity =
        text("vanity_url_code").map_or_else(|| dash.clone(), |c| format!("discord.gg/{c}"));
    let verification = int("verification_level")
        .and_then(|l| usize::try_from(l).ok())
        .and_then(|l| VERIFICATION.get(l))
        .map_or("?", |s| s);
    let channel_count = channels.map_or_else(
        || "?".to_string(),
        |c| {
            c.as_list()
                .iter()
                .filter(|c| c.get("type").and_then(Val::as_i64) != Some(CATEGORY))
                .count()
                .to_string()
        },
    );
    let roles = g.get("roles").map_or_else(
        || "?".to_string(),
        |r| r.as_list().len().saturating_sub(1).to_string(),
    );
    let count = |k: &str| int(k).map_or_else(|| "?".to_string(), |n| n.to_string());
    let boosts = count("premium_subscription_count");
    let tier = count("premium_tier");

    let mut out = format!("**{}**", text("name").unwrap_or_else(|| guild.to_string()));
    if let Some(d) = text("description") {
        out.push('\n');
        out.push_str(&d);
    }
    let created = format!("<t:{}:F>", created_at(guild));
    let members = count("approximate_member_count");
    let online = count("approximate_presence_count");
    let locale = text("preferred_locale").unwrap_or(dash);
    let rows: [(&str, &[(&str, &str)]); 10] = [
        ("server_info-owner", &[("value", &owner)]),
        ("server_info-created", &[("value", &created)]),
        ("server_info-joined", &[("value", &joined)]),
        ("server_info-vanity", &[("value", &vanity)]),
        ("server_info-locale", &[("value", &locale)]),
        ("server_info-verification", &[("value", verification)]),
        ("server_info-boosts", &[("count", &boosts), ("tier", &tier)]),
        (
            "server_info-members",
            &[("count", &members), ("online", &online)],
        ),
        ("server_info-channels", &[("value", &channel_count)]),
        ("server_info-roles", &[("value", &roles)]),
    ];
    for (key, args) in rows {
        out.push('\n');
        out.push_str(&host.t(key, args));
    }
    (out, Some(guild))
}

impl Plugin for ServerInfo {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Dispatch { name } => matches!(
                *name,
                "READY"
                    | "RELATIONSHIP_ADD"
                    | "RELATIONSHIP_UPDATE"
                    | "RELATIONSHIP_REMOVE"
                    | "GUILD_MEMBERS_CHUNK"
            ),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event {
            let d = dispatch_data(payload);
            if name == "GUILD_MEMBERS_CHUNK" {
                return if self.chunk(host, d) {
                    Verdict::Drop
                } else {
                    Verdict::Pass
                };
            }
            self.state.lock().unwrap().rel.track(name, d);
            return Verdict::Pass;
        }
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && inv.sub == SUB
        {
            let (mut answer, found) = run(host, &inv);
            if let Some(guild) = found {
                let line = self.ask_members(host, guild, inv.channel_id);
                if !line.is_empty() {
                    answer.push('\n');
                    answer.push_str(&line);
                }
            }
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    /// Made 2015-01-01T00:00:01Z (1000 ms after the Discord epoch).
    const GUILD: u64 = 1000 << 22;

    fn run(p: &ServerInfo, h: &mut Rec, guild_opt: Option<&str>, here: Option<u64>) -> String {
        let opts = guild_opt.map_or(String::new(), |g| {
            format!(r#"{{"type":3,"name":"guild","value":"{g}"}}"#)
        });
        let here = here.map_or(String::new(), |g| format!(r#""guild_id":"{g}","#));
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9",{here}"data":{{"name":"phoecord","options":[{{"type":1,"name":"serverinfo","options":[{opts}]}}]}}}}"#
        ));
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        p.command().run(h, &Args { options: vec![] })
    }

    #[test]
    fn lists_vencords_fields_for_this_server() {
        let p = ServerInfo::new();
        let g = GUILD.to_string();
        let mut h = Rec::default()
            .answer(
                "GET",
                &format!("/guilds/{g}?with_counts=true"),
                200,
                r#"{"name":"Test","description":"A place","owner_id":"5","vanity_url_code":"test",
                    "preferred_locale":"en-US","verification_level":2,"premium_subscription_count":7,
                    "premium_tier":2,"roles":[{"id":"1"},{"id":"2"},{"id":"3"}],
                    "approximate_member_count":40,"approximate_presence_count":12}"#,
            )
            .answer(
                "GET",
                &format!("/guilds/{g}/channels"),
                200,
                r#"[{"type":4},{"type":0},{"type":2}]"#,
            )
            .answer(
                "GET",
                &format!("/users/@me/guilds/{g}/member"),
                200,
                r#"{"joined_at":"2020-01-01T00:00:00+00:00"}"#,
            );
        let out = run(&p, &mut h, None, Some(GUILD));
        assert_eq!(
            out,
            "**Test**\nA place\nServer Owner: <@5>\nCreated At: <t:1420070401:F>\n\
             Joined At: <t:1577836800:F>\nVanity Link: discord.gg/test\nPreferred Locale: en-US\n\
             Verification Level: Medium\nServer Boosts: 7 (Level 2)\nMembers: 40 (12 online)\n\
             Channels: 2\nRoles: 2"
        );
    }

    #[test]
    fn missing_extras_leave_fields_unknown() {
        let p = ServerInfo::new();
        let mut h = Rec::default().answer("GET", "/guilds/77?with_counts=true", 200, "{}");
        let out = run(&p, &mut h, Some("77"), None);
        assert!(out.starts_with("**77**\nServer Owner: ?\n"), "{out}");
        assert!(
            out.contains("Joined At: -") && out.contains("Channels: ?"),
            "{out}"
        );
        assert!(out.contains("Verification Level: ?"), "{out}");
    }

    #[test]
    fn needs_a_server_and_reports_refusals() {
        let p = ServerInfo::new();
        let mut h = Rec::default();
        assert!(run(&p, &mut h, None, None).contains("Run it in a server"));
        assert!(run(&p, &mut h, Some("x"), None).contains("Run it in a server"));
        assert!(run(&p, &mut h, Some("77"), None).contains("HTTP 404"));
    }

    fn dispatch(p: &ServerInfo, h: &mut Rec, frame: &str) -> Verdict {
        let mut f = j(frame);
        let name = f.str_at(&["t"]).unwrap().to_string();
        p.on_event(
            h,
            Event::Dispatch {
                name: &name,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        )
    }

    #[test]
    fn friends_blocked_and_ignored_come_from_one_op_8() {
        let p = ServerInfo::new();
        dispatch(
            &p,
            &mut Rec::default(),
            r#"{"op":0,"t":"READY","d":{"relationships":[
                {"id":"11","type":1},{"id":"12","type":1},{"id":"21","type":2},
                {"id":"31","type":1,"user_ignored":true},{"id":"41","type":3}]}}"#,
        );
        let mut h = Rec::default().answer("GET", "/guilds/77?with_counts=true", 200, "{}");
        let out = run(&p, &mut h, Some("77"), None);
        assert!(out.ends_with("follow in a moment."), "{out}");
        assert_eq!(h.sent.len(), 1);
        let (op, frame) = &h.sent[0];
        assert_eq!(*op, 8);
        assert_eq!(frame.str_at(&["d", "guild_id"]), Some("77"));
        assert_eq!(
            frame.at(&["d", "user_ids"]),
            Some(&j(r#"["11","12","31","21"]"#))
        );
        let nonce = frame.str_at(&["d", "nonce"]).unwrap().to_string();
        // The client's own chunks pass untouched.
        assert!(matches!(
            dispatch(
                &p,
                &mut h,
                r#"{"op":0,"t":"GUILD_MEMBERS_CHUNK","d":{"nonce":"x","chunk_count":1,"members":[]}}"#
            ),
            Verdict::Pass
        ));
        let first = format!(
            r#"{{"op":0,"t":"GUILD_MEMBERS_CHUNK","d":{{"nonce":"{nonce}","chunk_index":0,"chunk_count":2,"members":[{{"user":{{"id":"11"}}}}]}}}}"#
        );
        assert!(matches!(dispatch(&p, &mut h, &first), Verdict::Drop));
        assert!(h.dispatched.is_empty(), "waits for the last chunk");
        let last = format!(
            r#"{{"op":0,"t":"GUILD_MEMBERS_CHUNK","d":{{"nonce":"{nonce}","chunk_index":1,"chunk_count":2,"members":[{{"user":{{"id":"31"}}}}],"not_found":["12","21"]}}}}"#
        );
        assert!(matches!(dispatch(&p, &mut h, &last), Verdict::Drop));
        let text = h.dispatched[0]
            .str_at(&["d", "content"])
            .unwrap()
            .to_string();
        assert_eq!(
            text,
            "Friends: <@11>, <@31>
Blocked Users: -
Ignored Users: <@31>"
        );
    }

    #[test]
    fn without_relationships_no_lookup_is_sent() {
        let p = ServerInfo::new();
        let mut h = Rec::default().answer("GET", "/guilds/77?with_counts=true", 200, "{}");
        run(&p, &mut h, Some("77"), None);
        assert!(h.sent.is_empty());
    }
}
