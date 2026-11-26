//! ReviewDB: read the reviews people left on a user's profile (Vencord: ReviewDB), data part.
//!
//! Ported from Vencord `src/plugins/reviewDB/` (GPL-3.0-or-later, Copyright (c) 2022 Vendicated
//! and contributors). Vencord shows a reviews section on profiles, read from
//! `https://manti.vendicated.dev/api/reviewdb/users/{id}/reviews` (50 per page, `offset`, the
//! `flags` bit 2 hides the "be respectful" notice when `showWarning` is off), and lets you write,
//! vote on, report and delete reviews after authorizing with ReviewDB.
//!
//! Here: `/phoecord reviews user:<id or mention> [page]` reads the same list through
//! `Host::fetch_with` and shows it as a message only you see: who wrote it, when (unless
//! `hideTimestamps`), and the text; reviews by people you blocked on Discord are left out with
//! `hideBlockedUsers` (default on; your blocked list comes from `GET /users/@me/relationships`).
//! An answer longer than Discord's 2000 characters is cut.
//!
//! Not ported: writing, voting, reporting and deleting, and `notifyReviews`. They need the
//! ReviewDB token in an `Authorization` header, which `Host::fetch_with` refuses. The profile
//! section waits for Phase 2.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer, snowflake_arg};
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "review_db";
const SUB: &str = "reviews";
const HOST: &str = "manti.vendicated.dev";
const PER_PAGE: u64 = 50;
/// Vencord's `WarningFlag`: leave out the "be respectful" notice.
const WARNING_FLAG: i64 = 0b10;
/// Review type of ReviewDB's own notices.
const SYSTEM: i64 = 3;
/// Relationship type "blocked".
const BLOCKED: i64 = 2;
const MAX_LEN: usize = 2000;

pub struct ReviewDb {
    manifest: Manifest,
    kept: Kept,
}

impl ReviewDb {
    pub fn new() -> ReviewDb {
        ReviewDb {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ReviewDB",
                    env!("CARGO_PKG_VERSION"),
                    "Read the reviews people left on a user with /phoecord reviews",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ReviewDB".into());
                m.fetch = vec![HOST.into()];
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord reviews`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            SUB,
            "Read the ReviewDB reviews of a user",
            &self.kept,
        )
        .option("user", "User id or mention", true)
        .option("page", "Page, 50 reviews each (default 1)", false)
        .boxed()
    }
}

impl Default for ReviewDb {
    fn default() -> Self {
        Self::new()
    }
}

fn flag(host: &dyn Host, key: &str, default: bool) -> bool {
    host.setting(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// Ids of the users you blocked on Discord; empty when the list cannot be read.
fn blocked(host: &mut dyn Host) -> Vec<String> {
    match host.rest("GET", "/users/@me/relationships", None) {
        Ok(HostResponse {
            status: 200..=299,
            body: Some(list),
            ..
        }) => list
            .as_list()
            .iter()
            .filter(|r| r.get("type").and_then(Val::as_i64) == Some(BLOCKED))
            .filter_map(|r| crate::presence::text(r, "id"))
            .collect(),
        _ => Vec::new(),
    }
}

fn line(host: &dyn Host, review: &Val, timestamps: bool) -> String {
    let comment = review.str_at(&["comment"]).unwrap_or("").replace('\n', " ");
    if review.get("type").and_then(Val::as_i64) == Some(SYSTEM) {
        return host.t("review_db-notice", &[("text", &comment)]);
    }
    let who = review
        .get("sender")
        .and_then(|s| crate::presence::text(s, "discordID"))
        .filter(|id| !id.is_empty() && id != "0")
        .map(|id| format!("<@{id}>"))
        .or_else(|| review.str_at(&["sender", "username"]).map(str::to_string))
        .unwrap_or_else(|| "?".into());
    let when = review
        .get("timestamp")
        .and_then(Val::as_i64)
        .filter(|t| timestamps && *t > 0)
        .map(|t| format!(" <t:{t}:R>"))
        .unwrap_or_default();
    host.t(
        "review_db-line",
        &[("who", &who), ("when", &when), ("text", &comment)],
    )
}

fn run(host: &mut dyn Host, user: u64, page: u64) -> String {
    let mut url = format!("https://{HOST}/api/reviewdb/users/{user}/reviews?limit={PER_PAGE}");
    if !flag(host, "showWarning", true) {
        url.push_str(&format!("&flags={WARNING_FLAG}"));
    }
    if page > 1 {
        url.push_str(&format!("&offset={}", (page - 1) * PER_PAGE));
    }
    let data = match host.fetch_with("GET", &url, &[("Accept", "application/json")], None) {
        Ok(HostResponse {
            status: 200..=299,
            body: Some(b),
            ..
        }) => b,
        // Vencord's own words for these two.
        Ok(HostResponse { status: 429, .. }) => return host.t("review_db-too-fast", &[]),
        Ok(_) | Err(_) => return host.t("review_db-failed", &[]),
    };
    if data.get("hasOptedOut").and_then(Val::as_bool) == Some(true) {
        return host.t("review_db-opted-out", &[]);
    }
    let hide: Vec<String> = if flag(host, "hideBlockedUsers", true) {
        blocked(host)
    } else {
        Vec::new()
    };
    let timestamps = !flag(host, "hideTimestamps", false);
    let reviews: Vec<String> = data
        .get("reviews")
        .map(Val::as_list)
        .unwrap_or_default()
        .iter()
        .filter(|r| {
            r.get("sender")
                .and_then(|s| crate::presence::text(s, "discordID"))
                .is_none_or(|id| !hide.contains(&id))
        })
        .map(|r| line(host, r, timestamps))
        .collect();
    let count = data
        .get("reviewCount")
        .and_then(Val::as_i64)
        .unwrap_or(0)
        .to_string();
    let mut out = host.t(
        "review_db-header",
        &[("user", &format!("<@{user}>")), ("count", &count)],
    );
    if reviews.is_empty() {
        out.push('\n');
        out.push_str(&host.t("review_db-none", &[]));
    }
    let more = (data.get("hasNextPage").and_then(Val::as_bool) == Some(true)).then(|| {
        host.t(
            "review_db-next",
            &[
                ("user", &user.to_string()),
                ("page", &(page + 1).to_string()),
            ],
        )
    });
    let cut = host.t("review_db-cut", &[]);
    let tail = more.as_deref().unwrap_or("");
    for r in reviews {
        if out.chars().count() + r.chars().count() + tail.chars().count() + cut.chars().count() + 3
            > MAX_LEN
        {
            out.push('\n');
            out.push_str(&cut);
            break;
        }
        out.push('\n');
        out.push_str(&r);
    }
    if let Some(m) = more {
        out.push('\n');
        out.push_str(&m);
    }
    out
}

impl Plugin for ReviewDb {
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
            let page = inv.get("page").and_then(|p| p.parse::<u64>().ok());
            let answer = match (
                inv.get("user").and_then(snowflake_arg),
                inv.get("page"),
                page,
            ) {
                (None, _, _) => host.t("review_db-bad-user", &[]),
                (Some(_), Some(_), None | Some(0)) => host.t("review_db-bad-page", &[]),
                (Some(user), _, page) => run(host, user, page.unwrap_or(1)),
            };
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

    const URL: &str = "https://manti.vendicated.dev/api/reviewdb/users/5/reviews?limit=50";
    const PAGE: &str = r#"{"reviewCount":3,"hasNextPage":true,"hasOptedOut":false,"reviews":[
        {"id":0,"type":3,"comment":"Be respectful.","timestamp":0,"sender":{"discordID":"1134864775000629298","username":"ReviewDB"}},
        {"id":1,"type":0,"comment":"nice\nperson","timestamp":1700000000,"sender":{"discordID":"7","username":"ann"}},
        {"id":2,"type":0,"comment":"spam","timestamp":1700000001,"sender":{"discordID":"8","username":"bob"}}]}"#;

    fn run(p: &ReviewDb, h: &mut Rec, opts: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"reviews","options":[{opts}]}}]}}}}"#
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
    fn lists_reviews_without_blocked_authors() {
        let p = ReviewDb::new();
        let mut h = Rec::default().answer_fetch("GET", URL, 200, PAGE).answer(
            "GET",
            "/users/@me/relationships",
            200,
            r#"[{"id":"8","type":2},{"id":"7","type":1}]"#,
        );
        let out = run(&p, &mut h, r#"{"type":3,"name":"user","value":"<@5>"}"#);
        assert_eq!(
            out,
            "**Reviews of <@5>: 3**\n-# Be respectful.\n<@7> <t:1700000000:R>: nice person\n\
             More: `/phoecord reviews user:5 page:2`"
        );
        assert_eq!(
            h.fetch_calls[0].1,
            vec![("Accept".to_string(), "application/json".to_string())]
        );
    }

    #[test]
    fn settings_pages_and_opt_out() {
        let p = ReviewDb::new();
        let mut h = Rec::default()
            .with("showWarning", Val::Bool(false))
            .with("hideTimestamps", Val::Bool(true))
            .with("hideBlockedUsers", Val::Bool(false))
            .answer_fetch(
                "GET",
                &format!("{URL}&flags=2&offset=50"),
                200,
                r#"{"reviewCount":1,"reviews":[{"id":1,"comment":"ok","timestamp":5,"sender":{"discordID":"7"}}]}"#,
            );
        let out = run(
            &p,
            &mut h,
            r#"{"type":3,"name":"user","value":"5"},{"type":3,"name":"page","value":"2"}"#,
        );
        assert_eq!(out, "**Reviews of <@5>: 1**\n<@7>: ok");
        assert!(h.rest_calls.is_empty(), "no blocked list needed");
        let mut h = Rec::default().answer_fetch("GET", URL, 200, r#"{"hasOptedOut":true}"#);
        assert!(run(&p, &mut h, r#"{"type":3,"name":"user","value":"5"}"#).contains("opted out"));
    }

    #[test]
    fn errors_and_long_lists() {
        let p = ReviewDb::new();
        let mut h = Rec::default().answer_fetch("GET", URL, 429, "{}");
        assert!(run(&p, &mut h, r#"{"type":3,"name":"user","value":"5"}"#).contains("too fast"));
        assert!(run(&p, &mut h, r#"{"type":3,"name":"user","value":"bob"}"#).contains("user id"));
        assert!(
            run(
                &p,
                &mut h,
                r#"{"type":3,"name":"user","value":"5"},{"type":3,"name":"page","value":"0"}"#
            )
            .contains("page")
        );
        let many: Vec<String> = (0..60)
            .map(|i| {
                format!(
                    r#"{{"id":{i},"comment":"{}","timestamp":0,"sender":{{"discordID":"7"}}}}"#,
                    "x".repeat(60)
                )
            })
            .collect();
        let mut h = Rec::default().answer_fetch(
            "GET",
            URL,
            200,
            &format!(
                r#"{{"reviewCount":60,"hasNextPage":true,"reviews":[{}]}}"#,
                many.join(",")
            ),
        );
        let out = run(&p, &mut h, r#"{"type":3,"name":"user","value":"5"}"#);
        assert!(out.chars().count() <= MAX_LEN, "{}", out.len());
        assert!(out.contains("(cut") && out.ends_with("page:2`"), "{out}");
    }
}
