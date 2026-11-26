//! localDevBadges: client-side profile badges for the current user only (Vencord: LocalDevBadges).
//!
//! Ported from the owner's userplugin `LocalDevBadges` (https://github.com/Nanda070/dsdevbadge,
//! Copyright (c) 2026 Nandak070, used under GPL-3.0-or-later with the author's permission of
//! 2026-11-25), modified for PhoeCord 2026-11-25.
//!
//! The script adds three profile badges to your own profile in your own client; nothing is sent
//! anywhere. Here the same happens on the way in, in the proxy:
//! - `GET /users/{you}/profile` (or `@me`, any query string): the enabled badges are put at the
//!   start of the response's `badges` list, with the script's description, icon and link. A badge
//!   with the same `id` that is already there is not added twice.
//! - The matching `public_flags` / `flags` bits are set on your own user object: gateway `READY`
//!   (`d.user`), the `GET /users/@me` answer and the `user` object of your profile answer. Other
//!   users are never touched.
//!
//! Outbound requests and gateway sends are never changed.
//!
//! Switching it on: `[plugins.local_dev_badges] enabled = true`, then set any of
//! `discordStaff`, `earlyVerifiedBotDeveloper`, `activeDeveloper` to `true` (all default `false`).
//!
//! Not ported: nothing of the script's logic is left out; it only adds the badge objects, which
//! the client draws itself.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

/// One badge of the script: setting key, Discord's badge `id`, description, icon hash, link, flag.
struct Badge {
    setting: &'static str,
    id: &'static str,
    description: &'static str,
    icon: &'static str,
    link: &'static str,
    flag: i64,
}

/// In the order the script lists them (`ALL_BADGES`).
const BADGES: [Badge; 3] = [
    Badge {
        setting: "discordStaff",
        id: "staff",
        description: "Discord Staff",
        icon: "5e74e9b61934fc1f67c65515d1f7e60d",
        link: "https://discord.com/company",
        flag: 1 << 0,
    },
    Badge {
        setting: "earlyVerifiedBotDeveloper",
        id: "verified_developer",
        description: "Early Verified Bot Developer",
        icon: "6df5892e0f35b051f8b61eace34f4967",
        link: "https://support-dev.discord.com/hc/en-us/articles/10113997751447",
        flag: 1 << 17,
    },
    Badge {
        setting: "activeDeveloper",
        id: "active_developer",
        description: "Active Developer",
        icon: "6bdc42827a38498929a4920da12695d9",
        link: "https://support-dev.discord.com/hc/en-us/articles/10113997751447",
        flag: 1 << 22,
    },
];

pub struct LocalDevBadges {
    manifest: Manifest,
}

impl LocalDevBadges {
    pub fn new() -> LocalDevBadges {
        LocalDevBadges {
            manifest: {
                let mut m = Manifest::new(
                    "local_dev_badges",
                    "localDevBadges",
                    env!("CARGO_PKG_VERSION"),
                    "Show staff and developer badges on your own profile, locally only",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("LocalDevBadges".into());
                m
            },
        }
    }
}

impl Default for LocalDevBadges {
    fn default() -> Self {
        Self::new()
    }
}

/// The badges whose setting is on.
fn enabled(host: &dyn Host) -> Vec<&'static Badge> {
    BADGES
        .iter()
        .filter(|b| host.setting(b.setting).and_then(|v| v.as_bool()) == Some(true))
        .collect()
}

/// What `GET <route>` is: `Some(None)` for `/users/@me`, `Some(Some(id))` for a profile of `id`.
enum Route<'a> {
    Me,
    Profile(&'a str),
}

fn parse_route<'a>(method: &str, route: &'a str) -> Option<Route<'a>> {
    if method != "GET" {
        return None;
    }
    let path = route.split('?').next().unwrap_or(route);
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    match seg.as_slice() {
        ["users", "@me"] => Some(Route::Me),
        ["users", id, "profile"] => Some(Route::Profile(id)),
        _ => None,
    }
}

/// ORs the enabled flags into `public_flags` and `flags` of a user object.
fn set_flags(user: &mut Val, on: &[&Badge]) -> bool {
    if !matches!(user, Val::Map(_)) {
        return false;
    }
    let bits = on.iter().fold(0, |acc, b| acc | b.flag);
    let mut changed = false;
    for key in ["public_flags", "flags"] {
        let now = user.get(key).and_then(Val::as_i64).unwrap_or(0);
        if now | bits != now || user.get(key).is_none() {
            user.set(key, Val::Int(now | bits));
            changed = true;
        }
    }
    changed
}

fn badge_val(b: &Badge) -> Val {
    let s = |v: &str| Val::Str(v.into());
    Val::Map(vec![
        (s("id"), s(b.id)),
        (s("description"), s(b.description)),
        (s("icon"), s(b.icon)),
        (s("link"), s(b.link)),
    ])
}

/// Puts the missing badges at the start of `badges`; `false` if nothing was added.
fn add_badges(body: &mut Val, on: &[&Badge]) -> bool {
    if !matches!(body, Val::Map(_)) {
        return false;
    }
    if body.get("badges").is_none() {
        body.set("badges", Val::List(Vec::new()));
    }
    let Some(list) = body.get_mut("badges").and_then(Val::as_list_mut) else {
        return false;
    };
    let missing: Vec<Val> = on
        .iter()
        .filter(|b| !list.iter().any(|x| x.str_at(&["id"]) == Some(b.id)))
        .map(|b| badge_val(b))
        .collect();
    if missing.is_empty() {
        return false;
    }
    list.splice(0..0, missing);
    true
}

impl Plugin for LocalDevBadges {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => matches!(*name, "READY" | "USER_UPDATE"),
            EventKind::RestResponse { method, route } => parse_route(method, route).is_some(),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let changed = match event {
            Event::Dispatch {
                name: "READY",
                payload,
                ..
            } => {
                let on = enabled(host);
                !on.is_empty()
                    && payload
                        .get_mut("d")
                        .and_then(|d| d.get_mut("user"))
                        .is_some_and(|u| set_flags(u, &on))
            }
            // `USER_UPDATE` carries the current user after a profile change; without this the
            // client's copy loses the bits until the next READY.
            Event::Dispatch {
                name: "USER_UPDATE",
                payload,
                ..
            } => {
                let on = enabled(host);
                let me = host.self_id();
                !on.is_empty()
                    && payload.get_mut("d").is_some_and(|d| {
                        let id = d.str_at(&["id"]).and_then(|s| s.parse::<u64>().ok());
                        me.is_some() && id == me && set_flags(d, &on)
                    })
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } => {
                let on = enabled(host);
                if on.is_empty() {
                    return Verdict::Pass;
                }
                match parse_route(method, route) {
                    Some(Route::Me) => set_flags(body, &on),
                    Some(Route::Profile(id)) => {
                        let me = host.self_id();
                        let mine = id == "@me" || me.is_some_and(|m| id == m.to_string());
                        if !mine {
                            return Verdict::Pass;
                        }
                        let a = add_badges(body, &on);
                        let b = body.get_mut("user").is_some_and(|u| set_flags(u, &on));
                        a || b
                    }
                    None => false,
                }
            }
            _ => false,
        };
        if changed {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn chain() -> Chain {
        Chain::new(vec![Box::new(LocalDevBadges::new())])
    }

    fn host(staff: bool, dev: bool, active: bool) -> Rec {
        let mut h = Rec::default()
            .with("discordStaff", Val::Bool(staff))
            .with("earlyVerifiedBotDeveloper", Val::Bool(dev))
            .with("activeDeveloper", Val::Bool(active));
        h.self_id = Some(10);
        h
    }

    fn profile(h: &mut Rec, route: &str, body: &str) -> (Outcome, Val) {
        let mut b = j(body);
        let out = chain().rest_response(h, "GET", route, 200, &mut b);
        (out, b)
    }

    fn ids(v: &Val) -> Vec<String> {
        v.get("badges")
            .map(Val::as_list)
            .unwrap_or(&[])
            .iter()
            .map(|b| b.str_at(&["id"]).unwrap_or("").to_string())
            .collect()
    }

    const PROFILE: &str = r#"{"user":{"id":"10","public_flags":64},"badges":[{"id":"premium","description":"Nitro","icon":"x"}]}"#;

    #[test]
    fn own_profile_gets_one_badge_per_setting() {
        let (out, b) = profile(&mut host(true, false, false), "/users/10/profile", PROFILE);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(ids(&b), ["staff", "premium"]);
        let staff = &b.get("badges").unwrap().as_list()[0];
        assert_eq!(staff.str_at(&["description"]), Some("Discord Staff"));
        assert_eq!(
            staff.str_at(&["icon"]),
            Some("5e74e9b61934fc1f67c65515d1f7e60d")
        );
        assert_eq!(staff.str_at(&["link"]), Some("https://discord.com/company"));

        let (_, b) = profile(&mut host(false, true, false), "/users/10/profile", PROFILE);
        assert_eq!(ids(&b), ["verified_developer", "premium"]);
        let (_, b) = profile(&mut host(false, false, true), "/users/10/profile", PROFILE);
        assert_eq!(ids(&b), ["active_developer", "premium"]);
    }

    #[test]
    fn all_three_in_script_order_and_flags_on_the_user() {
        let (out, b) = profile(
            &mut host(true, true, true),
            "/users/@me/profile?with_mutual_guilds=false",
            PROFILE,
        );
        assert_eq!(out, Outcome::Changed);
        assert_eq!(
            ids(&b),
            ["staff", "verified_developer", "active_developer", "premium"]
        );
        let want = 64 | 1 | (1 << 17) | (1 << 22);
        assert_eq!(
            b.at(&["user", "public_flags"]).and_then(Val::as_i64),
            Some(want)
        );
        assert_eq!(
            b.at(&["user", "flags"]).and_then(Val::as_i64),
            Some(1 | (1 << 17) | (1 << 22))
        );
    }

    #[test]
    fn nothing_when_all_off_or_other_user_or_plugin_off() {
        let (out, b) = profile(&mut host(false, false, false), "/users/10/profile", PROFILE);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(b, j(PROFILE));
        let (out, b) = profile(&mut host(true, true, true), "/users/11/profile", PROFILE);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(b, j(PROFILE));
        // Plugin off: the chain skips it.
        let c = chain();
        c.set_enabled(Vec::new());
        let mut bb = j(PROFILE);
        let mut all = host(true, true, true);
        assert_eq!(
            c.rest_response(&mut all, "GET", "/users/10/profile", 200, &mut bb),
            Outcome::Untouched
        );
        assert_eq!(bb, j(PROFILE));
    }

    #[test]
    fn existing_badge_is_not_duplicated() {
        let body = r#"{"user":{"id":"10","public_flags":1,"flags":1},"badges":[{"id":"staff","description":"Discord Staff","icon":"x"}]}"#;
        let (out, b) = profile(&mut host(true, false, false), "/users/10/profile", body);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(ids(&b), ["staff"]);
        let (out, b) = profile(&mut host(true, true, false), "/users/10/profile", body);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(ids(&b), ["verified_developer", "staff"]);
    }

    #[test]
    fn ready_flags_set_for_self_only() {
        let mut h = host(true, false, true);
        let mut f = j(
            r#"{"op":0,"t":"READY","d":{"user":{"id":"10","public_flags":0},"users":[{"id":"11","public_flags":0}]}}"#,
        );
        let out = chain().dispatch(&mut h, "READY", &mut f, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        let want = 1 | (1 << 22);
        assert_eq!(
            f.at(&["d", "user", "public_flags"]).and_then(Val::as_i64),
            Some(want)
        );
        assert_eq!(
            f.at(&["d", "user", "flags"]).and_then(Val::as_i64),
            Some(want)
        );
        assert_eq!(
            f.at(&["d", "users"]).unwrap().as_list()[0]
                .at(&["public_flags"])
                .and_then(Val::as_i64),
            Some(0)
        );
        // Already set: untouched.
        let out = chain().dispatch(&mut h, "READY", &mut f, &mut Vec::new());
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn users_me_flags() {
        let mut h = host(false, true, false);
        let mut b = j(r#"{"id":"10","username":"a","flags":4}"#);
        let out = chain().rest_response(&mut h, "GET", "/users/@me", 200, &mut b);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(b.at(&["flags"]).and_then(Val::as_i64), Some(4 | (1 << 17)));
        assert_eq!(b.at(&["public_flags"]).and_then(Val::as_i64), Some(1 << 17));
        // Not a GET, not a 200, other routes.
        let mut b = j(r#"{"id":"10"}"#);
        assert_eq!(
            chain().rest_response(&mut h, "PATCH", "/users/@me", 200, &mut b),
            Outcome::Untouched
        );
        assert_eq!(
            chain().rest_response(&mut h, "GET", "/users/@me", 403, &mut b),
            Outcome::Untouched
        );
        assert_eq!(
            chain().rest_response(&mut h, "GET", "/users/@me/guilds", 200, &mut b),
            Outcome::Untouched
        );
    }

    #[test]
    fn outbound_traffic_is_never_changed() {
        let mut h = host(true, true, true);
        let mut body = j(r#"{"content":"hi"}"#);
        let c = chain();
        assert_eq!(
            c.rest_request(&mut h, "GET", "/users/10/profile", Some(&mut body)),
            Outcome::Untouched
        );
        let mut p = j(r#"{"d":{"user":{"id":"10"}}}"#);
        assert_eq!(c.send(&mut h, 3, &mut p), Outcome::Untouched);
    }

    #[test]
    fn user_update_for_self_keeps_the_flags() {
        let mut h = host(true, false, false);
        let mut f = j(r#"{"op":0,"t":"USER_UPDATE","d":{"id":"10","public_flags":0}}"#);
        let out = chain().dispatch(&mut h, "USER_UPDATE", &mut f, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        assert_eq!(f.at(&["d", "public_flags"]).and_then(Val::as_i64), Some(1));
        let mut other = j(r#"{"op":0,"t":"USER_UPDATE","d":{"id":"11","public_flags":0}}"#);
        let out = chain().dispatch(&mut h, "USER_UPDATE", &mut other, &mut Vec::new());
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let mut h = host(true, true, true);
        let c = chain();
        for body in [
            "null",
            "5",
            "[]",
            r#"{"badges":"x"}"#,
            r#"{"badges":[1,null,{}],"user":7}"#,
            r#"{"user":{"public_flags":"x"}}"#,
        ] {
            for route in ["/users/10/profile", "/users/@me"] {
                let mut b = j(body);
                let _ = c.rest_response(&mut h, "GET", route, 200, &mut b);
            }
        }
        for frame in [
            "{}",
            r#"{"d":null}"#,
            r#"{"d":{"user":5}}"#,
            r#"{"d":{"user":[]}}"#,
        ] {
            let mut f = j(frame);
            let _ = c.dispatch(&mut h, "READY", &mut f, &mut Vec::new());
        }
    }
}
