//! OverrideForumDefaults: your own default forum layout and sort order.
//!
//! Ported from Vencord `src/plugins/overrideForumDefaults/index.tsx` (GPL-3.0-or-later, Copyright
//! (c) 2024 Vendicated and contributors; author Inbestigator). Vencord replaces the client's
//! `getDefaultLayout()` and `getDefaultSortOrder()`; per-channel choices you make in the client
//! still win. Here the same default is written into the channel objects the client receives:
//! `default_forum_layout` and `default_sort_order` of forum channels (type 15), and the sort order
//! of media channels (type 16, which have no layout choice). It covers `READY` and `GUILD_CREATE`
//! channel lists, `CHANNEL_CREATE` / `CHANNEL_UPDATE`, `GET /guilds/{id}/channels` and
//! `GET /channels/{id}`. Nothing is written to Discord.
//!
//! Settings (`[plugins.override_forum_defaults]`), names and values as in Vencord:
//! `defaultLayout` (1 list, the default; 2 gallery) and `defaultSortOrder` (0 recently active, the
//! default; 1 date posted).
//!
//! Switching it on: `[plugins.override_forum_defaults] enabled = true`, or
//! `/phoecord toggle override_forum_defaults`.

use crate::state::{dispatch_data_mut, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const FORUM: i64 = 15;
const MEDIA: i64 = 16;
const EVENTS: &[&str] = &["READY", "GUILD_CREATE", "CHANNEL_CREATE", "CHANNEL_UPDATE"];

pub struct OverrideForumDefaults {
    manifest: Manifest,
}

impl OverrideForumDefaults {
    pub fn new() -> OverrideForumDefaults {
        OverrideForumDefaults {
            manifest: {
                let mut m = Manifest::new(
                    "override_forum_defaults",
                    "OverrideForumDefaults",
                    env!("CARGO_PKG_VERSION"),
                    "Your own default forum layout and sort order",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("OverrideForumDefaults".into());
                m
            },
        }
    }
}

impl Default for OverrideForumDefaults {
    fn default() -> Self {
        Self::new()
    }
}

/// `GET /guilds/{id}/channels` or `GET /channels/{id}`.
fn is_channels(method: &str, route: &str) -> bool {
    let path = route.split('?').next().unwrap_or("");
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["guilds", _, "channels"] | ["channels", _])
}

fn choice(host: &dyn Host, key: &str, allowed: &[i64], default: i64) -> i64 {
    host.setting(key)
        .and_then(|v| v.as_i64())
        .filter(|n| allowed.contains(n))
        .unwrap_or(default)
}

/// Rewrites one channel object. `true` if it changed.
fn fix_channel(c: &mut Val, layout: i64, sort: i64) -> bool {
    let kind = c.get("type").and_then(Val::as_i64);
    let mut changed = false;
    if kind == Some(FORUM) && c.get("default_forum_layout").and_then(Val::as_i64) != Some(layout) {
        c.set("default_forum_layout", Val::Int(layout));
        changed = true;
    }
    if matches!(kind, Some(FORUM | MEDIA))
        && c.get("default_sort_order").and_then(Val::as_i64) != Some(sort)
    {
        c.set("default_sort_order", Val::Int(sort));
        changed = true;
    }
    changed
}

fn fix_list(list: &mut Val, layout: i64, sort: i64) -> bool {
    let mut changed = false;
    if let Some(items) = list.as_list_mut() {
        for c in items {
            changed |= fix_channel(c, layout, sort);
        }
    }
    changed
}

impl Plugin for OverrideForumDefaults {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => is_one_of(name, EVENTS),
            EventKind::RestResponse { method, route } => is_channels(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let layout = choice(host, "defaultLayout", &[1, 2], 1);
        let sort = choice(host, "defaultSortOrder", &[0, 1], 0);
        let changed = match event {
            Event::Dispatch { name, payload, .. } => {
                let payload = dispatch_data_mut(payload);
                match name {
                    "READY" => {
                        let mut ch = false;
                        if let Some(gs) = payload.get_mut("guilds").and_then(Val::as_list_mut) {
                            for g in gs {
                                if let Some(chs) = g.get_mut("channels") {
                                    ch |= fix_list(chs, layout, sort);
                                }
                            }
                        }
                        ch
                    }
                    "GUILD_CREATE" => payload
                        .get_mut("channels")
                        .is_some_and(|chs| fix_list(chs, layout, sort)),
                    "CHANNEL_CREATE" | "CHANNEL_UPDATE" => fix_channel(payload, layout, sort),
                    _ => false,
                }
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } if is_channels(method, route) => {
                fix_list(body, layout, sort) | fix_channel(body, layout, sort)
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
    use crate::testutil::{Cfg, j};

    fn dispatch(p: &OverrideForumDefaults, h: &mut Cfg, name: &str, payload: &mut Val) -> Verdict {
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name,
                payload,
                extra: &mut extra,
            },
        )
    }

    #[test]
    fn forum_channels_get_your_defaults() {
        let p = OverrideForumDefaults::new();
        let mut h = Cfg::default()
            .with("defaultLayout", Val::Int(2))
            .with("defaultSortOrder", Val::Int(1));
        let mut g = j(
            r#"{"id":"1","channels":[{"id":"2","type":15,"default_forum_layout":0,"default_sort_order":null},
            {"id":"3","type":0},{"id":"4","type":16}]}"#,
        );
        assert_eq!(
            dispatch(&p, &mut h, "GUILD_CREATE", &mut g),
            Verdict::Changed
        );
        let chs = g.get("channels").unwrap().as_list();
        assert_eq!(chs[0].get("default_forum_layout"), Some(&Val::Int(2)));
        assert_eq!(chs[0].get("default_sort_order"), Some(&Val::Int(1)));
        assert!(
            chs[1].get("default_sort_order").is_none(),
            "text channel untouched"
        );
        assert!(
            chs[2].get("default_forum_layout").is_none(),
            "media has no layout"
        );
        assert_eq!(chs[2].get("default_sort_order"), Some(&Val::Int(1)));
        // Already right: no change.
        assert_eq!(dispatch(&p, &mut h, "GUILD_CREATE", &mut g), Verdict::Pass);
    }

    #[test]
    fn forum_channels_get_your_defaults_in_a_real_gateway_frame() {
        use phoecord_plugin_runtime::{Chain, Outcome};
        let chain = Chain::new(vec![Box::new(OverrideForumDefaults::new())]);
        let mut h = Cfg::default()
            .with("defaultLayout", Val::Int(2))
            .with("defaultSortOrder", Val::Int(1));
        let mut frame = j(
            r#"{"op":0,"t":"GUILD_CREATE","s":1,"d":{"id":"1","channels":[
            {"id":"2","type":15,"default_forum_layout":0,"default_sort_order":null}]}}"#,
        );
        let out = chain.dispatch(&mut h, "GUILD_CREATE", &mut frame, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        let ch = &frame.at(&["d", "channels"]).unwrap().as_list()[0];
        assert_eq!(ch.get("default_forum_layout"), Some(&Val::Int(2)));
        assert_eq!(ch.get("default_sort_order"), Some(&Val::Int(1)));
        let mut update = j(r#"{"op":0,"t":"CHANNEL_UPDATE","s":2,"d":{"type":15}}"#);
        chain.dispatch(&mut h, "CHANNEL_UPDATE", &mut update, &mut Vec::new());
        assert_eq!(
            update.at(&["d", "default_forum_layout"]),
            Some(&Val::Int(2))
        );
    }

    #[test]
    fn defaults_apply_without_settings_and_to_rest_answers() {
        let p = OverrideForumDefaults::new();
        let mut h = Cfg::default();
        let mut one = j(r#"{"id":"2","type":15,"default_forum_layout":2,"default_sort_order":1}"#);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/channels/2",
                status: 200,
                body: &mut one,
            },
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(one.get("default_forum_layout"), Some(&Val::Int(1)));
        assert_eq!(one.get("default_sort_order"), Some(&Val::Int(0)));
        let mut list = j(r#"[{"type":15},{"type":2}]"#);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/guilds/7/channels",
                status: 200,
                body: &mut list,
            },
        );
        assert_eq!(v, Verdict::Changed);
    }

    #[test]
    fn junk_and_bad_settings_do_not_panic() {
        let p = OverrideForumDefaults::new();
        let mut h = Cfg::default().with("defaultLayout", Val::Int(99));
        for json in [
            r#"null"#,
            r#""x""#,
            r#"{"channels":5}"#,
            r#"{"guilds":[1,{"channels":"x"}]}"#,
        ] {
            let mut v = j(json);
            assert_eq!(
                dispatch(&p, &mut h, "READY", &mut v),
                Verdict::Pass,
                "{json}"
            );
        }
        let mut c = j(r#"{"type":15}"#);
        dispatch(&p, &mut h, "CHANNEL_UPDATE", &mut c);
        assert_eq!(
            c.get("default_forum_layout"),
            Some(&Val::Int(1)),
            "bad value -> default"
        );
    }
}
