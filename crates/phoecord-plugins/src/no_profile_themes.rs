//! NoProfileThemes: other people's Nitro profile themes are removed (Vencord: NoProfileThemes).
//!
//! Ported from Vencord `src/plugins/noProfileThemes/index.ts` (GPL-3.0-or-later, Copyright (c)
//! Vendicated, TheKodeToad and contributors). Vencord makes `canEditThemes()` false for everyone but
//! the current user, so no theme shows on anyone else's profile. On the wire a theme is
//! `theme_colors` in the `user_profile` and `guild_member_profile` of `GET /users/{id}/profile`;
//! this plugin sets it to `null` for every user except the signed-in one.
//!
//! Used together with `fake_profile_themes`, nobody else's theme shows, fake ones included, which
//! is what Vencord does when both are on. No settings.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

pub struct NoProfileThemes {
    manifest: Manifest,
}

impl NoProfileThemes {
    pub fn new() -> NoProfileThemes {
        NoProfileThemes {
            manifest: Manifest {
                id: "no_profile_themes".into(),
                name: "NoProfileThemes".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Removes Nitro profile themes from everyone but you".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("NoProfileThemes".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
        }
    }
}

impl Default for NoProfileThemes {
    fn default() -> Self {
        Self::new()
    }
}

/// `GET /users/{id}/profile`: the id of the profile's owner.
pub(crate) fn profile_route(method: &str, route: &str) -> Option<u64> {
    let path = route.split('?').next().unwrap_or(route);
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    match seg.as_slice() {
        ["users", id, "profile"] if method == "GET" => id.parse().ok(),
        _ => None,
    }
}

/// The two places a profile response keeps `theme_colors`.
pub(crate) const PROFILES: [&str; 2] = ["user_profile", "guild_member_profile"];

/// Set `theme_colors` to null wherever it holds colours; true if anything changed.
fn strip(body: &mut Val) -> bool {
    let mut changed = false;
    for key in PROFILES {
        if let Some(profile) = body.get_mut(key)
            && profile
                .get("theme_colors")
                .is_some_and(|c| !matches!(c, Val::Null))
        {
            profile.set("theme_colors", Val::Null);
            changed = true;
        }
    }
    changed
}

impl Plugin for NoProfileThemes {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestResponse { method, route }
            if profile_route(method, route).is_some())
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestResponse {
            method,
            route,
            status: 200,
            body,
        } = event
        else {
            return Verdict::Pass;
        };
        let Some(owner) = profile_route(method, route) else {
            return Verdict::Pass;
        };
        // Until the gateway says who "me" is, change nothing rather than strip your own theme.
        if host.self_id().is_none_or(|me| me == owner) {
            return Verdict::Pass;
        }
        if strip(body) {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    const PROFILE: &str = r#"{"user":{"id":"7"},
        "user_profile":{"bio":"hi","theme_colors":[1,2]},
        "guild_member_profile":{"theme_colors":[3,4]}}"#;

    #[test]
    fn matching_rule() {
        assert_eq!(profile_route("GET", "/users/7/profile"), Some(7));
        assert_eq!(
            profile_route("GET", "/users/7/profile?with_mutual_guilds=true"),
            Some(7)
        );
        assert_eq!(profile_route("POST", "/users/7/profile"), None);
        assert_eq!(profile_route("GET", "/users/@me/profile"), None);
        assert_eq!(profile_route("GET", "/users/7"), None);
        let p = NoProfileThemes::new();
        assert!(p.wants(&EventKind::RestResponse {
            method: "GET",
            route: "/users/7/profile"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/users/7/profile"
        }));
        assert!(!p.wants(&EventKind::Dispatch {
            name: "USER_UPDATE"
        }));
    }

    #[test]
    fn strips_other_users_but_not_mine() {
        let chain = Chain::new(vec![Box::new(NoProfileThemes::new())]);
        let mut host = Cfg {
            me: Some(1),
            ..Cfg::default()
        };

        let mut body = j(PROFILE);
        let out = chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(body.at(&["user_profile", "theme_colors"]), Some(&Val::Null));
        assert_eq!(
            body.at(&["guild_member_profile", "theme_colors"]),
            Some(&Val::Null)
        );
        assert_eq!(body.str_at(&["user_profile", "bio"]), Some("hi"));

        let mut mine = j(PROFILE);
        let out = chain.rest_response(&mut host, "GET", "/users/1/profile", 200, &mut mine);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(mine, j(PROFILE));
    }

    #[test]
    fn leaves_everything_alone_when_me_is_unknown_or_nothing_to_strip() {
        let chain = Chain::new(vec![Box::new(NoProfileThemes::new())]);
        let mut host = Cfg::default();
        let mut body = j(PROFILE);
        let out = chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body);
        assert_eq!(out, Outcome::Untouched);

        let mut host = Cfg {
            me: Some(1),
            ..Cfg::default()
        };
        let mut plain = j(r#"{"user_profile":{"bio":"x","theme_colors":null}}"#);
        let out = chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut plain);
        assert_eq!(out, Outcome::Untouched);
        let mut err = j(PROFILE);
        let out = chain.rest_response(&mut host, "GET", "/users/7/profile", 404, &mut err);
        assert_eq!(out, Outcome::Untouched);
    }
}
