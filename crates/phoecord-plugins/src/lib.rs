//! Built-in plugins, native Rust, on the unified Plugin API.
//!
//! Porting the v0 plugins (docs/ROADMAP.md, milestone 1.2): `no_track` is done; `silent_typing`,
//! `message_logger`, `fake_nitro`, `relationship_notifier`, `game_activity_toggle` and `unindent`
//! follow. Once the WASM runtime is stable, each one moves to its own WASM package and this crate
//! shrinks to the few plugins that must stay native.

use phoecord_core::Plugin;

pub mod no_track;

/// Every built-in plugin, in default run order.
pub fn builtin() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(no_track::NoTrack::new())]
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = super::builtin()
            .iter()
            .map(|p| p.manifest().id.clone())
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
