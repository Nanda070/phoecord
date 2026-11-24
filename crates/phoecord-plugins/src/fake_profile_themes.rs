//! FakeProfileThemes: profile theme colours hidden in a bio (Vencord: FakeProfileThemes).
//!
//! Ported from Vencord `src/plugins/fakeProfileThemes/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! Vendicated and contributors; itself a port of Alyxia's Vendetta plugin). A user without Nitro
//! writes `[#rrggbb,#rrggbb]` into their bio as invisible "tag" characters (each ASCII character
//! shifted by `0xE0000`, see [`encode`]). This plugin finds that text in the bio of
//! `GET /users/{id}/profile` and fills `theme_colors` with `[primary, accent]`, so the client
//! shows the theme.
//!
//! Settings (`[plugins.fake_profile_themes]`), as Vencord's:
//! - `nitro_first` (default `true`): keep the real Nitro colours when a profile has both; `false`
//!   prefers the colours hidden in the bio.
//!
//! Not ported: the colour pickers and the "Copy 3y3" button of Vencord's settings page (UI, Phase 2).
//! [`encode`] is here so the future settings tab and tests share one implementation.
//! Only `user_profile.bio` is read; a per-server bio does not carry a fake theme.
//! Order matters when `no_profile_themes` is also on: it runs after this plugin and removes
//! everyone else's colours, fake ones included, as Vencord does.

use crate::no_profile_themes::{PROFILES, profile_route};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

pub struct FakeProfileThemes {
    manifest: Manifest,
}

impl FakeProfileThemes {
    pub fn new() -> FakeProfileThemes {
        FakeProfileThemes {
            manifest: Manifest {
                id: "fake_profile_themes".into(),
                name: "FakeProfileThemes".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Shows profile theme colours that people hide in their bio".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("FakeProfileThemes".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
        }
    }
}

impl Default for FakeProfileThemes {
    fn default() -> Self {
        Self::new()
    }
}

/// Invisible text for a bio: a space, then `[#primary,#accent]` as tag characters.
#[allow(dead_code)] // used by tests now and by the settings tab in Phase 2
pub fn encode(primary: u32, accent: u32) -> String {
    let message = format!("[#{primary:06x},#{accent:06x}]");
    let encoded: String = message
        .chars()
        .filter(|c| (0x20..=0x7f).contains(&(*c as u32)))
        .filter_map(|c| char::from_u32(c as u32 + 0xE0000))
        .collect();
    format!(" {encoded}")
}

/// The colours hidden in a bio, if any: `[#1-6 hex digits,#1-6 hex digits]` written in tag characters.
pub fn decode(bio: &str) -> Option<[i64; 2]> {
    let ascii: String = bio
        .chars()
        .filter(|c| (0xE0000..=0xE007F).contains(&(*c as u32)))
        .filter_map(|c| char::from_u32(c as u32 - 0xE0000))
        .collect();
    // `[#` hex `,#` hex `]`, the first occurrence (Vencord's DecodeRegex).
    let mut rest = ascii.as_str();
    while let Some(start) = rest.find("[#") {
        rest = &rest[start + 2..];
        if let Some(colors) = parse_pair(rest) {
            return Some(colors);
        }
    }
    None
}

/// `rrggbb,#rrggbb]…` → both numbers.
fn parse_pair(s: &str) -> Option<[i64; 2]> {
    let (first, rest) = hex_run(s)?;
    let rest = rest.strip_prefix(",#")?;
    let (second, rest) = hex_run(rest)?;
    rest.starts_with(']').then_some([first, second])
}

/// 1–6 hex digits at the start of `s`, and what follows them.
fn hex_run(s: &str) -> Option<(i64, &str)> {
    let n = s.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if !(1..=6).contains(&n) {
        return None;
    }
    Some((i64::from_str_radix(&s[..n], 16).ok()?, &s[n..]))
}

impl Plugin for FakeProfileThemes {
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
        if profile_route(method, route).is_none() {
            return Verdict::Pass;
        }
        let nitro_first = host
            .setting("nitro_first")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let Some(profile) = body.get_mut(PROFILES[0]) else {
            return Verdict::Pass;
        };
        let has_nitro_colors = profile
            .get("theme_colors")
            .is_some_and(|c| !matches!(c, Val::Null));
        if nitro_first && has_nitro_colors {
            return Verdict::Pass;
        }
        let Some([primary, accent]) = profile.str_at(&["bio"]).and_then(decode) else {
            return Verdict::Pass;
        };
        profile.set(
            "theme_colors",
            Val::List(vec![Val::Int(primary), Val::Int(accent)]),
        );
        Verdict::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::no_profile_themes::NoProfileThemes;
    use crate::testutil::{Cfg, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn profile(bio: &str, colors: &str) -> Val {
        let mut v = j(&format!(
            r#"{{"user":{{"id":"7"}},"user_profile":{{"theme_colors":{colors}}}}}"#
        ));
        v.get_mut("user_profile")
            .unwrap()
            .set("bio", Val::Str(bio.into()));
        v
    }

    #[test]
    fn encode_and_decode_round_trip() {
        let bio = format!("hello{}", encode(0xff8800, 0x0a0b0c));
        assert!(bio.chars().skip(5).skip(1).all(|c| (c as u32) >= 0xE0000));
        assert_eq!(decode(&bio), Some([0xff8800, 0x0a0b0c]));
        assert_eq!(decode(&encode(0, 0)), Some([0, 0]));
    }

    #[test]
    fn decode_follows_vencords_regex() {
        let tag = |s: &str| -> String {
            s.chars()
                .map(|c| char::from_u32(c as u32 + 0xE0000).unwrap())
                .collect()
        };
        assert_eq!(decode(&tag("[#fff,#A")), None);
        assert_eq!(decode(&tag("[#fff,#abc]")), Some([0xfff, 0xabc]));
        assert_eq!(decode(&tag("x[#12,#34]y[#56,#78]")), Some([0x12, 0x34]));
        assert_eq!(decode(&tag("[#1234567,#34]")), None);
        assert_eq!(decode(&tag("[#zz,#34]")), None);
        // The brackets must be tag characters; plain text does not count.
        assert_eq!(decode("[#ffffff,#000000]"), None);
        assert_eq!(decode(""), None);
        // A broken first match does not hide a good second one.
        assert_eq!(decode(&tag("[#gg,#1][#1,#2]")), Some([1, 2]));
    }

    #[test]
    fn matching_rule() {
        let p = FakeProfileThemes::new();
        assert!(p.wants(&EventKind::RestResponse {
            method: "GET",
            route: "/users/7/profile"
        }));
        assert!(!p.wants(&EventKind::RestResponse {
            method: "GET",
            route: "/users/7"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/users/7/profile"
        }));
    }

    #[test]
    fn fills_theme_colors_from_the_bio() {
        let chain = Chain::new(vec![Box::new(FakeProfileThemes::new())]);
        let mut host = Cfg::default();
        let mut body = profile(&format!("hi{}", encode(0x112233, 0x445566)), "null");
        let out = chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(
            body.at(&["user_profile", "theme_colors"]),
            Some(&Val::List(vec![Val::Int(0x112233), Val::Int(0x445566)]))
        );
    }

    #[test]
    fn nitro_first_decides_a_conflict() {
        let chain = Chain::new(vec![Box::new(FakeProfileThemes::new())]);
        let bio = encode(1, 2);

        let mut host = Cfg::default(); // default: Nitro colours win
        let mut body = profile(&bio, "[9,9]");
        assert_eq!(
            chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body),
            Outcome::Untouched
        );

        let mut host = Cfg::default().with("nitro_first", Val::Bool(false));
        let mut body = profile(&bio, "[9,9]");
        assert_eq!(
            chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body),
            Outcome::Changed
        );
        assert_eq!(
            body.at(&["user_profile", "theme_colors"]),
            Some(&Val::List(vec![Val::Int(1), Val::Int(2)]))
        );
    }

    #[test]
    fn plain_bios_and_errors_are_untouched() {
        let chain = Chain::new(vec![Box::new(FakeProfileThemes::new())]);
        let mut host = Cfg::default();
        let mut body = profile("just text", "null");
        assert_eq!(
            chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body),
            Outcome::Untouched
        );
        let mut body = profile(&encode(1, 2), "null");
        assert_eq!(
            chain.rest_response(&mut host, "GET", "/users/7/profile", 403, &mut body),
            Outcome::Untouched
        );
    }

    #[test]
    fn with_no_profile_themes_nobody_elses_fake_theme_shows() {
        let chain = Chain::new(vec![
            Box::new(FakeProfileThemes::new()),
            Box::new(NoProfileThemes::new()),
        ]);
        let mut host = Cfg {
            me: Some(1),
            ..Cfg::default()
        };
        let mut body = profile(&encode(1, 2), "null");
        chain.rest_response(&mut host, "GET", "/users/7/profile", 200, &mut body);
        assert_eq!(body.at(&["user_profile", "theme_colors"]), Some(&Val::Null));
    }
}
