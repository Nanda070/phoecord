//! GameActivityToggle: your status does not show the game you are playing
//! (Vencord: GameActivityToggle). The proxy re-sends your last presence whenever the set of
//! enabled plugins changes, so `phoecord toggle game_activity_toggle` takes effect at once.
//!
//! Ported from the v0 implementation; the Vencord chat bar button is Phase 2 UI.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

/// Gateway opcode of a presence update.
const OP_PRESENCE: i64 = 3;
/// Activity type 0 = "Playing".
const ACTIVITY_PLAYING: i64 = 0;

pub struct GameActivityToggle {
    manifest: Manifest,
}

impl GameActivityToggle {
    pub fn new() -> GameActivityToggle {
        GameActivityToggle {
            manifest: {
                let mut m = Manifest::new(
                    "game_activity_toggle",
                    "GameActivityToggle",
                    env!("CARGO_PKG_VERSION"),
                    "While on, your status doesn't show the game you're playing",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("GameActivityToggle".into());
                m
            },
        }
    }
}

impl Default for GameActivityToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// The matching rule: client presence updates (op 3) and nothing else.
fn matches(op: i64) -> bool {
    op == OP_PRESENCE
}

/// Remove "Playing" activities from a presence frame (`{"op": 3, "d": {"activities": [...]}}`).
/// Returns whether anything was removed.
fn strip_games(frame: &mut Val) -> bool {
    let Some(list) = frame
        .get_mut("d")
        .and_then(|d| d.get_mut("activities"))
        .and_then(Val::as_list_mut)
    else {
        return false;
    };
    let before = list.len();
    list.retain(|a| a.get("type").and_then(Val::as_i64) != Some(ACTIVITY_PLAYING));
    list.len() != before
}

impl Plugin for GameActivityToggle {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Send { op } if matches(*op))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if matches(op) => {
                if strip_games(payload) {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn json(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    const PRESENCE: &str = r#"{"op":3,"d":{"status":"online","activities":[{"type":0,"name":"CS2"},{"type":2,"name":"Spotify"}]}}"#;

    #[test]
    fn matches_only_presence_updates() {
        assert!(matches(3));
        assert!(!matches(1));
        assert!(!matches(4));
    }

    #[test]
    fn strips_playing_only() {
        let mut p = json(PRESENCE);
        assert!(strip_games(&mut p));
        let list = p.at(&["d", "activities"]).unwrap().as_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].str_at(&["name"]), Some("Spotify"));
        assert!(!strip_games(&mut p), "nothing left to strip");
        let mut hb = json(r#"{"op":1,"d":5}"#);
        assert!(!strip_games(&mut hb));
    }

    #[test]
    fn chain_removes_the_game() {
        let chain = Chain::new(vec![Box::new(GameActivityToggle::new())]);
        let mut host = NullHost::default();
        let mut p = json(PRESENCE);
        assert_eq!(chain.send(&mut host, 3, &mut p), Outcome::Changed);
        assert_eq!(p.at(&["d", "activities"]).unwrap().as_list().len(), 1);
        // Other opcodes and presences without games are left alone.
        let mut hb = json(r#"{"op":1,"d":5}"#);
        assert_eq!(chain.send(&mut host, 1, &mut hb), Outcome::Untouched);
        let mut clean = json(r#"{"op":3,"d":{"status":"idle","activities":[]}}"#);
        assert_eq!(chain.send(&mut host, 3, &mut clean), Outcome::Untouched);
    }
}
