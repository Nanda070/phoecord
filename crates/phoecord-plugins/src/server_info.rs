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
//! Not ported: the Friends, Blocked Users and Ignored Users tabs. They need a member lookup per
//! user, which the per-plugin rate limit (5 calls per 5 s) makes too slow for a command.

use crate::commands::{Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer};
use phoecord_core::plugin::{API_VERSION, HostResponse};
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "server_info";
const SUB: &str = "serverinfo";
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
/// Channel type of a category.
const CATEGORY: i64 = 4;
/// Vencord's names for the verification levels.
const VERIFICATION: &[&str] = &["None", "Low", "Medium", "High", "Highest"];

pub struct ServerInfo {
    manifest: Manifest,
    kept: Kept,
}

impl ServerInfo {
    pub fn new() -> ServerInfo {
        ServerInfo {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "ServerInfo".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Show facts about a server: owner, age, boosts, channels, roles"
                    .into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("ServerInfo".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            kept: Kept::default(),
        }
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

fn run(host: &mut dyn Host, inv: &Invocation) -> String {
    let guild = match inv.get("guild") {
        Some(g) => match crate::commands::snowflake_arg(g) {
            Some(id) => id,
            None => return host.t("server_info-bad-guild", &[]),
        },
        None => match inv.guild_id {
            Some(id) => id,
            None => return host.t("server_info-bad-guild", &[]),
        },
    };
    let g = match get(host, &format!("/guilds/{guild}?with_counts=true")) {
        Ok(g) => g,
        Err(e) => return host.t("server_info-failed", &[("error", &e)]),
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
    out
}

impl Plugin for ServerInfo {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_interaction(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && inv.sub == SUB
        {
            let answer = run(host, &inv);
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
}
