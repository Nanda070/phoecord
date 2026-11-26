//! A full `READY` for every client (owner's ruling, 2026-11-26).
//!
//! Desktop Phoenix keeps its own cache and tells the gateway in `IDENTIFY` which version of each
//! guild it already holds (`d.client_state.guild_versions`). The gateway then sends every such
//! guild in `READY` as `data_mode: "partial"`: no `channels`, `roles`, `emojis`, `stickers` or
//! `properties`, only changes against the cache (2.1.4). Plugins that build their picture from
//! `READY` see nothing there. With an empty `guild_versions` the gateway sends every guild in
//! full, as on a first sign-in, which the client handles as well: it only costs a bigger `READY`.

use phoecord_core::Val;

/// Empties `d.client_state.guild_versions` of an `IDENTIFY` (op 2) so the gateway answers with a
/// full `READY`. Returns how many cached guild versions were dropped, `None` when `payload` is not
/// an `IDENTIFY` or names none (nothing changed then).
pub fn ask_full_ready(payload: &mut Val) -> Option<usize> {
    if payload.get("op").and_then(Val::as_i64) != Some(2) {
        return None;
    }
    let versions = payload
        .get_mut("d")?
        .get_mut("client_state")?
        .get_mut("guild_versions")?;
    let n = match versions {
        Val::Map(m) if !m.is_empty() => m.len(),
        _ => return None,
    };
    *versions = Val::map();
    Some(n)
}

/// For a `READY` payload's `d`: how many guilds arrived partial, out of how many.
pub fn partial_guilds(d: &Val) -> (usize, usize) {
    let Some(Val::List(guilds)) = d.get("guilds") else {
        return (0, 0);
    };
    let partial = guilds
        .iter()
        .filter(|g| g.str_at(&["data_mode"]) == Some("partial"))
        .count();
    (partial, guilds.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    #[test]
    fn an_identify_with_cached_guilds_asks_for_everything() {
        let mut p = j(
            r#"{"op":2,"d":{"token":"t","client_state":{"guild_versions":{"1":5,"2":9},"highest_last_message_id":"0"}}}"#,
        );
        assert_eq!(ask_full_ready(&mut p), Some(2));
        assert_eq!(
            p.at(&["d", "client_state", "guild_versions"]),
            Some(&Val::map())
        );
        // Everything else is left as the client sent it.
        assert_eq!(p.str_at(&["d", "token"]), Some("t"));
        assert_eq!(
            p.str_at(&["d", "client_state", "highest_last_message_id"]),
            Some("0")
        );
        // A second pass has nothing left to drop.
        assert_eq!(ask_full_ready(&mut p), None);
    }

    #[test]
    fn other_frames_and_identifies_without_a_cache_are_untouched() {
        for s in [
            r#"{"op":1,"d":42}"#,
            r#"{"op":3,"d":{"status":"online"}}"#,
            r#"{"op":2,"d":{"token":"t"}}"#,
            r#"{"op":2,"d":{"client_state":{"guild_versions":{}}}}"#,
        ] {
            let mut p = j(s);
            let before = p.clone();
            assert_eq!(ask_full_ready(&mut p), None, "{s}");
            assert_eq!(p, before, "{s}");
        }
    }

    #[test]
    fn partial_guilds_are_counted_in_ready() {
        let d = j(
            r#"{"guilds":[{"id":"1","data_mode":"partial"},{"id":"2","data_mode":"full"},{"id":"3"}]}"#,
        );
        assert_eq!(partial_guilds(&d), (1, 3));
        assert_eq!(partial_guilds(&j(r#"{"user":{}}"#)), (0, 0));
    }

    #[test]
    fn the_phoenix_ready_fixture_is_partial() {
        // Recorded on Phoenix 0.1.0.2208 before this change (2.1.4): every guild partial.
        let text = include_str!("../../../tests/fixtures/phoenix/READY.json");
        let v = j(text);
        let d = v.get("d").unwrap_or(&v);
        let (partial, total) = partial_guilds(d);
        assert!(total > 0 && partial == total, "{partial} of {total}");
    }
}
