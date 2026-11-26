//! AutoDNDWhilePlaying: show "Do Not Disturb" (or another status) while you play a game (Vencord:
//! AutoDNDWhilePlaying).
//!
//! Ported from Vencord `src/plugins/autoDndWhilePlaying.discordDesktop/index.ts`
//! (GPL-3.0-or-later, Copyright (c) 2024 Vendicated and contributors; author thororen; the plugin
//! now lives on in Vencord forks). Vencord watches `RUNNING_GAMES_CHANGE`: when a game starts and
//! your status is neither the one to set nor `invisible`, it saves your status and sets
//! `statusToSet` (default `dnd`); when the last game stops it puts the saved status back.
//!
//! The proxy does not see the client's game detection, only the presence it sends (identify,
//! `op 2`, and presence updates, `op 3`). So: while that presence has a "Playing" activity
//! (type 0), its `status` is rewritten to `statusToSet`, unless it is `invisible`. Nothing has to
//! be put back: once the game is gone, the client's next presence carries your own status again.
//!
//! Difference from Vencord: your status setting itself is not changed, only what others see. The
//! client keeps its own status, so its notifications are not silenced while you play. This plugin
//! sits before GameActivityToggle and IgnoreActivities in the chain, so a game they hide from
//! others still counts.
//!
//! Setting (`[plugins.auto_dnd_while_playing]`): `statusToSet` = `online`, `idle`, `dnd`
//! (default) or `invisible`.

use crate::presence::{self, PLAYING};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "auto_dnd_while_playing";
const STATUSES: &[&str] = &["online", "idle", "dnd", "invisible"];

pub struct AutoDndWhilePlaying {
    manifest: Manifest,
}

impl AutoDndWhilePlaying {
    pub fn new() -> AutoDndWhilePlaying {
        AutoDndWhilePlaying {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "AutoDNDWhilePlaying",
                    env!("CARGO_PKG_VERSION"),
                    "Show Do Not Disturb (or another status) while you play a game",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("AutoDNDWhilePlaying".into());
                m
            },
        }
    }
}

impl Default for AutoDndWhilePlaying {
    fn default() -> Self {
        Self::new()
    }
}

/// `statusToSet`, `dnd` when missing or not a status.
fn target(host: &dyn Host) -> String {
    host.setting("statusToSet")
        .and_then(|v| v.as_str().map(str::to_ascii_lowercase))
        .filter(|s| STATUSES.contains(&s.as_str()))
        .unwrap_or_else(|| "dnd".into())
}

/// Set the status of one presence; whether it changed.
pub fn rewrite(presence: &mut Val, target: &str) -> bool {
    let playing = presence
        .get("activities")
        .map(Val::as_list)
        .unwrap_or_default()
        .iter()
        .any(|a| presence::activity_type(a) == Some(PLAYING));
    let status = presence.str_at(&["status"]).unwrap_or("online");
    if !playing || status == target || status == "invisible" {
        return false;
    }
    let new = presence.str_at(&["status"]).map_or_else(
        || Val::Str(target.to_string()),
        |_| presence.get("status").expect("checked").str_like(target),
    );
    presence.set("status", new);
    true
}

impl Plugin for AutoDndWhilePlaying {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Send { op } if presence::carries_presence(*op))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Send { op, payload } = event else {
            return Verdict::Pass;
        };
        if !presence::carries_presence(op) {
            return Verdict::Pass;
        }
        let target = target(host);
        let changed = presence::presence_mut(payload).is_some_and(|p| rewrite(p, &target));
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

    fn send(p: &AutoDndWhilePlaying, h: &mut Cfg, frame: &str) -> (Verdict, Val) {
        let mut v = j(frame);
        let op = v.get("op").and_then(Val::as_i64).unwrap();
        let out = p.on_event(
            h,
            Event::Send {
                op,
                payload: &mut v,
            },
        );
        (out, v)
    }

    #[test]
    fn playing_shows_dnd_and_stopping_shows_your_status() {
        let p = AutoDndWhilePlaying::new();
        let mut h = Cfg::default();
        let (v, f) = send(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","since":0,"afk":false,"activities":[{"type":0,"name":"Game"}]}}"#,
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(f.str_at(&["d", "status"]), Some("dnd"));
        let (v, f) = send(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        assert_eq!(v, Verdict::Pass);
        assert_eq!(f.str_at(&["d", "status"]), Some("online"));
    }

    #[test]
    fn identify_invisible_and_other_targets() {
        let p = AutoDndWhilePlaying::new();
        let mut h = Cfg::default().with("statusToSet", Val::Str("idle".into()));
        let (v, f) = send(
            &p,
            &mut h,
            r#"{"op":2,"d":{"token":"t","presence":{"status":"dnd","activities":[{"type":0,"name":"G"}]}}}"#,
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(f.str_at(&["d", "presence", "status"]), Some("idle"));
        let (v, _) = send(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"invisible","activities":[{"type":0,"name":"G"}]}}"#,
        );
        assert_eq!(v, Verdict::Pass, "invisible is never changed");
        let (v, _) = send(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[{"type":2,"name":"Spotify"}]}}"#,
        );
        assert_eq!(v, Verdict::Pass, "music is not a game");
        let mut bad = Cfg::default().with("statusToSet", Val::Str("busy".into()));
        assert_eq!(target(&bad), "dnd");
        bad.settings.clear();
        assert!(p.wants(&EventKind::Send { op: 3 }) && !p.wants(&EventKind::Send { op: 4 }));
    }
}
