//! FriendInvites: create, list and revoke friend invite links with commands (Vencord:
//! FriendInvites).
//!
//! Ported from Vencord `src/plugins/friendInvites/index.ts` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; authors afn, Dziurwa). Vencord calls the client's own
//! `createFriendInvite`, `getAllFriendInvites` and `revokeFriendInvites`; here the same calls go
//! through `Host::rest`:
//!
//! - `/phoecord create-friend-invite`: `POST /users/@me/invites` with `{}`;
//! - `/phoecord view-friend-invites`: `GET /users/@me/invites`;
//! - `/phoecord revoke-friend-invites`: `DELETE /users/@me/invites`.
//!
//! The answers are Vencord's: `discord.gg/<code> · Expires: <t:…:R> · Max uses: N`, shown as a
//! message only the user sees. Unverified against a live account: the request body the client
//! sends with `POST /users/@me/invites` (Vencord does not show it; `{}` is assumed).

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer};
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "friend_invites";
const CREATE: &str = "create-friend-invite";
const VIEW: &str = "view-friend-invites";
const REVOKE: &str = "revoke-friend-invites";
const ROUTE: &str = "/users/@me/invites";

pub struct FriendInvites {
    manifest: Manifest,
    kept: Kept,
}

impl FriendInvites {
    pub fn new() -> FriendInvites {
        FriendInvites {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "FriendInvites",
                    env!("CARGO_PKG_VERSION"),
                    "Create, list and revoke friend invite links with commands",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("FriendInvites".into());
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord create-friend-invite`, `view-friend-invites` and `revoke-friend-invites`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        vec![
            kept_answer(
                PLUGIN_ID,
                CREATE,
                "Generate a friend invite link",
                &self.kept,
            )
            .boxed(),
            kept_answer(PLUGIN_ID, VIEW, "List your friend invite links", &self.kept).boxed(),
            kept_answer(
                PLUGIN_ID,
                REVOKE,
                "Revoke all your friend invite links",
                &self.kept,
            )
            .boxed(),
        ]
    }

    fn run(&self, host: &mut dyn Host, sub: &str) -> String {
        match sub {
            CREATE => match call(host, "POST", Some(&Val::map())) {
                Ok(invite) => line(host, &invite, true),
                Err(e) => host.t("friend_invites-failed", &[("error", &e)]),
            },
            VIEW => match call(host, "GET", None) {
                Ok(list) => {
                    let lines: Vec<String> = list
                        .as_list()
                        .iter()
                        .map(|i| line(host, i, false))
                        .collect();
                    if lines.is_empty() {
                        host.t("friend_invites-none", &[])
                    } else {
                        lines.join("\n")
                    }
                }
                Err(e) => host.t("friend_invites-failed", &[("error", &e)]),
            },
            _ => match call(host, "DELETE", None) {
                Ok(_) => host.t("friend_invites-revoked", &[]),
                Err(e) => host.t("friend_invites-failed", &[("error", &e)]),
            },
        }
    }
}

impl Default for FriendInvites {
    fn default() -> Self {
        Self::new()
    }
}

/// One call to `/users/@me/invites`; the JSON answer, or why it failed.
fn call(host: &mut dyn Host, method: &str, body: Option<&Val>) -> Result<Val, String> {
    match host.rest(method, ROUTE, body)? {
        HostResponse {
            status: 200..=299,
            body,
            ..
        } => Ok(body.unwrap_or(Val::Null)),
        HostResponse { status, .. } => Err(format!("HTTP {status}")),
    }
}

/// One invite as Vencord prints it: just made (max uses) or listed (uses / max uses).
fn line(host: &dyn Host, invite: &Val, created: bool) -> String {
    let code = invite.str_at(&["code"]).unwrap_or("?");
    let expires = invite
        .str_at(&["expires_at"])
        .and_then(iso_unix)
        .map_or_else(
            || host.t("friend_invites-never", &[]),
            |t| format!("<t:{t}:R>"),
        );
    let num = |k: &str| {
        invite
            .get(k)
            .and_then(Val::as_i64)
            .map_or("?".to_string(), |n| n.to_string())
    };
    if created {
        host.t(
            "friend_invites-created",
            &[
                ("code", code),
                ("expires", &expires),
                ("max", &num("max_uses")),
            ],
        )
    } else {
        host.t(
            "friend_invites-line",
            &[
                ("code", code),
                ("expires", &expires),
                ("uses", &num("uses")),
                ("max", &num("max_uses")),
            ],
        )
    }
}

/// Unix seconds of an ISO 8601 time such as `2026-11-25T17:00:00.123+00:00` or `…Z`.
pub(crate) fn iso_unix(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, offset) = match time.find(['+', '-', 'Z']) {
        Some(i) => time.split_at(i),
        None => (time, ""),
    };
    let mut c = clock.split(':');
    let h: i64 = c.next()?.parse().ok()?;
    let min: i64 = c.next()?.parse().ok()?;
    let sec: i64 = c.next().map_or(Some(0.0), |s| s.parse::<f64>().ok())? as i64;
    let off = match offset.as_bytes().first() {
        Some(b'+' | b'-') => {
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let (oh, om) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
            sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60)
        }
        _ => 0,
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + sec - off)
}

impl Plugin for FriendInvites {
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
            && [CREATE, VIEW, REVOKE].contains(&inv.sub.as_str())
        {
            let answer = self.run(host, &inv.sub);
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

    fn run(p: &FriendInvites, h: &mut Rec, sub: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"{sub}"}}]}}}}"#
        ));
        let v = p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        assert_eq!(v, Verdict::Pass);
        let cmds = p.commands();
        let c = cmds.iter().find(|c| c.name() == sub).unwrap();
        c.run(h, &Args { options: vec![] })
    }

    #[test]
    fn creates_an_invite_and_prints_it_like_vencord() {
        let p = FriendInvites::new();
        let mut h = Rec::default().answer(
            "POST",
            ROUTE,
            200,
            r#"{"code":"abc","expires_at":"2026-11-26T00:00:00+00:00","max_uses":5,"uses":0}"#,
        );
        let out = run(&p, &mut h, CREATE);
        assert_eq!(
            out,
            "discord.gg/abc · Expires: <t:1795651200:R> · Max uses: `5`"
        );
        assert_eq!(h.rest_calls[0].0, "POST /users/@me/invites");
        assert_eq!(h.rest_calls[0].1, Some(Val::map()));
    }

    #[test]
    fn lists_invites_or_says_there_are_none() {
        let p = FriendInvites::new();
        let mut h = Rec::default().answer(
            "GET",
            ROUTE,
            200,
            r#"[{"code":"a","expires_at":"1970-01-01T00:01:00Z","max_uses":3,"uses":1},
                {"code":"b","expires_at":null,"max_uses":0,"uses":2}]"#,
        );
        assert_eq!(
            run(&p, &mut h, VIEW),
            "_discord.gg/a_ · Expires: <t:60:R> · Times used: `1/3`\n\
             _discord.gg/b_ · Expires: never · Times used: `2/0`"
        );
        let mut empty = Rec::default().answer("GET", ROUTE, 200, "[]");
        assert_eq!(
            run(&p, &mut empty, VIEW),
            "You have no active friend invites!"
        );
    }

    #[test]
    fn revokes_and_reports_failures() {
        let p = FriendInvites::new();
        let mut h = Rec::default().answer("DELETE", ROUTE, 200, "[]");
        assert_eq!(
            run(&p, &mut h, REVOKE),
            "All friend invites have been revoked."
        );
        let mut down = Rec::default();
        assert_eq!(
            run(&p, &mut down, REVOKE),
            "Could not reach Discord: HTTP 404"
        );
    }

    #[test]
    fn ignores_other_commands_and_reads_iso_times() {
        let p = FriendInvites::new();
        let mut h = Rec::default();
        let mut body =
            j(r#"{"type":2,"data":{"name":"phoecord","options":[{"type":1,"name":"plugins"}]}}"#);
        p.on_event(
            &mut h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        assert!(h.rest_calls.is_empty());
        assert!(p.kept.lock().is_none());
        assert_eq!(iso_unix("2000-03-01T00:00:00.999+01:00"), Some(951_865_200));
        assert_eq!(iso_unix("bad"), None);
    }
}
