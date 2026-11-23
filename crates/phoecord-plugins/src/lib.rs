//! Built-in plugins, native Rust, on the unified Plugin API.
//!
//! Porting the v0 plugins (docs/ROADMAP.md, milestone 1.2): `no_track`, `silent_typing`,
//! `message_logger`, `fake_nitro` (emoji), `relationship_notifier`, `game_activity_toggle` and the
//! new `clear_urls` are done; `unindent` follows. Once the WASM runtime is stable, each one moves
//! to its own WASM package and this crate shrinks to the few plugins that must stay native.

use phoecord_core::Plugin;

pub mod clear_urls;
pub mod commands;
pub mod fake_nitro;
pub mod game_activity_toggle;
pub mod i_love_spam;
pub mod message_logger;
pub mod no_track;
pub mod relationship_notifier;
pub mod silent_typing;

/// Every built-in plugin, in default run order.
pub fn builtin() -> Vec<Box<dyn Plugin>> {
    let mut plugins: Vec<Box<dyn Plugin>> = vec![
        Box::new(no_track::NoTrack::new()),
        Box::new(silent_typing::SilentTyping::new()),
        Box::new(clear_urls::ClearUrls::new()),
        Box::new(game_activity_toggle::GameActivityToggle::new()),
        Box::new(relationship_notifier::RelationshipNotifier::new()),
        Box::new(fake_nitro::FakeNitro::new()),
        Box::new(message_logger::MessageLogger::new()),
        Box::new(i_love_spam::ILoveSpam::new()),
    ];
    // `/phoecord plugins` lists everything above and the commands plugin itself. Register more
    // `/phoecord` subcommands here with `Commands::with`.
    let commands = commands::Commands::empty();
    let mut infos: Vec<_> = plugins
        .iter()
        .map(|p| commands::PluginInfo::of(p.as_ref()))
        .collect();
    infos.push(commands::PluginInfo::of(&commands));
    plugins.push(Box::new(commands::Commands::new(infos)));
    plugins
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
