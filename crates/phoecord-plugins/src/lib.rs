//! Built-in plugins, native Rust, on the unified Plugin API.
//!
//! Porting the v0 plugins (docs/ROADMAP.md, milestone 1.2): `no_track`, `silent_typing`,
//! `message_logger`, `fake_nitro` (emoji), `relationship_notifier` and `game_activity_toggle` are
//! done; `unindent` follows. Once the WASM runtime is stable, each one moves to its own WASM package and
//! this crate shrinks to the few plugins that must stay native.

use phoecord_core::Plugin;

pub mod fake_nitro;
pub mod game_activity_toggle;
pub mod message_logger;
pub mod no_track;
pub mod relationship_notifier;
pub mod silent_typing;

/// Every built-in plugin, in default run order.
pub fn builtin() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(no_track::NoTrack::new()),
        Box::new(silent_typing::SilentTyping::new()),
        Box::new(game_activity_toggle::GameActivityToggle::new()),
        Box::new(relationship_notifier::RelationshipNotifier::new()),
        Box::new(fake_nitro::FakeNitro::new()),
        Box::new(message_logger::MessageLogger::new()),
    ]
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
