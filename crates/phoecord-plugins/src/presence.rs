//! Your own presence as the client sends it, for the plugins of stage 1.2.13.
//!
//! The client says who you are to others in two gateway frames: identify (`op 2`, the presence is
//! `d.presence`) and presence update (`op 3`, the presence is `d`). Both hold `status` (`online`,
//! `idle`, `dnd`, `invisible`), `since` (ms, when you went idle, else 0), `afk` and `activities`
//! (games, Spotify, Rich Presence, the custom status). A plugin rewrites them in place.
//!
//! Plugins that add an activity of their own (CustomRPC, MusicRichPresence) keep it in an
//! [`Owned`]: on every presence they remove the activity they added last time, remember the rest
//! as the client's presence and add their current one. To change it between client frames they
//! send that remembered presence again with `Host::emit_send`, which passes the plugin chain too,
//! so every plugin adds and filters as usual.

use phoecord_core::Val;

/// Gateway opcode of identify.
pub const IDENTIFY: i64 = 2;
/// Gateway opcode of a presence update.
pub const PRESENCE: i64 = 3;

/// Activity types.
pub const PLAYING: i64 = 0;
pub const STREAMING: i64 = 1;
pub const LISTENING: i64 = 2;
pub const WATCHING: i64 = 3;
pub const CUSTOM_STATUS: i64 = 4;
pub const COMPETING: i64 = 5;

/// Whether a send opcode carries a presence.
pub fn carries_presence(op: i64) -> bool {
    op == IDENTIFY || op == PRESENCE
}

/// The presence inside a whole send frame (`d.presence` for identify, `d` for a presence update).
pub fn presence_mut(frame: &mut Val) -> Option<&mut Val> {
    let op = frame.get("op").and_then(Val::as_i64)?;
    let d = frame.get_mut("d")?;
    match op {
        IDENTIFY => d.get_mut("presence"),
        PRESENCE => Some(d),
        _ => None,
    }
}

pub fn presence(frame: &Val) -> Option<&Val> {
    let op = frame.get("op").and_then(Val::as_i64)?;
    let d = frame.get("d")?;
    match op {
        IDENTIFY => d.get("presence"),
        PRESENCE => Some(d),
        _ => None,
    }
}

/// The activities of a presence, created empty when missing.
pub fn activities_mut(presence: &mut Val) -> Option<&mut Vec<Val>> {
    if !matches!(presence.get("activities"), Some(Val::List(_))) {
        presence.set("activities", Val::List(Vec::new()));
    }
    presence.get_mut("activities").and_then(Val::as_list_mut)
}

pub fn activity_type(a: &Val) -> Option<i64> {
    a.get("type").and_then(Val::as_i64)
}

/// A string field of an activity; snowflakes may come as numbers in ETF.
pub fn text(a: &Val, key: &str) -> Option<String> {
    match a.get(key)? {
        Val::Int(n) => Some(n.to_string()),
        Val::Big(n) => Some(n.to_string()),
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

/// The presence update frame (`op 3`) for a presence.
pub fn update_frame(presence: Val) -> Val {
    let mut frame = Val::map();
    frame.set("op", Val::Int(PRESENCE));
    frame.set("d", presence);
    frame
}

/// An activity a plugin adds to every presence, and the client's presence without it.
#[derive(Default)]
pub struct Owned {
    /// What the plugin added last time (removed from the next presence before adding again).
    pub added: Option<Val>,
    /// The activity to add from now on.
    pub current: Option<Val>,
    /// The last presence that passed, without this plugin's activity: what to send again.
    pub base: Option<Val>,
}

impl Owned {
    /// Rewrite one presence: drop the old activity, remember the rest, add the current one.
    /// Whether the presence changed.
    pub fn apply(&mut self, presence: &mut Val) -> bool {
        let Some(list) = activities_mut(presence) else {
            return false;
        };
        let before = list.len();
        if let Some(old) = &self.added {
            list.retain(|a| a != old);
        }
        let removed = list.len() != before;
        self.base = Some(presence.clone());
        let list = activities_mut(presence).expect("created above");
        match &self.current {
            Some(a) => {
                list.push(a.clone());
                self.added = Some(a.clone());
                true
            }
            None => {
                self.added = None;
                removed
            }
        }
    }

    /// The presence update to send now that `current` changed, or `None` before the client has
    /// sent a presence.
    pub fn resend(&self) -> Option<Val> {
        self.base.clone().map(update_frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::j;

    #[test]
    fn finds_the_presence_in_identify_and_updates() {
        let mut id = j(r#"{"op":2,"d":{"token":"x","presence":{"status":"dnd","activities":[]}}}"#);
        assert_eq!(
            presence_mut(&mut id).unwrap().str_at(&["status"]),
            Some("dnd")
        );
        let up = j(r#"{"op":3,"d":{"status":"idle"}}"#);
        assert_eq!(presence(&up).unwrap().str_at(&["status"]), Some("idle"));
        assert!(presence(&j(r#"{"op":1,"d":5}"#)).is_none());
        assert!(carries_presence(2) && carries_presence(3) && !carries_presence(4));
    }

    #[test]
    fn owned_activity_is_replaced_not_stacked() {
        let mut own = Owned {
            current: Some(j(r#"{"type":0,"name":"Mine"}"#)),
            ..Owned::default()
        };
        let mut p = j(r#"{"status":"online","activities":[{"type":4,"name":"Custom Status"}]}"#);
        assert!(own.apply(&mut p));
        assert_eq!(p.at(&["activities"]).unwrap().as_list().len(), 2);
        // The resent presence passes again: still one copy, base without it.
        let mut again = own.resend().unwrap();
        assert!(own.apply(presence_mut(&mut again).unwrap()));
        assert_eq!(again.at(&["d", "activities"]).unwrap().as_list().len(), 2);
        assert_eq!(
            own.base
                .as_ref()
                .unwrap()
                .at(&["activities"])
                .unwrap()
                .as_list()
                .len(),
            1
        );
        // Switched off: the old copy is removed once, then nothing changes.
        own.current = None;
        let mut with_old = again.get("d").unwrap().clone();
        assert!(own.apply(&mut with_old));
        assert_eq!(with_old.at(&["activities"]).unwrap().as_list().len(), 1);
        let mut clean = with_old.clone();
        assert!(!own.apply(&mut clean));
        let mut none = j(r#"{"status":"online"}"#);
        assert!(!own.apply(&mut none));
        assert_eq!(none.at(&["activities"]).unwrap().as_list().len(), 0);
    }
}
