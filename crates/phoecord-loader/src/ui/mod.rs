//! UI hooks inside Phoenix (Phase 2, stage 2.3).
//!
//! Nothing here is wired into a real Phoenix hook yet. The private `phoecord-recon` notes map
//! the app's own init and render chain (`docs/re/gpui-lifecycle.md`: `Application::run`,
//! `DiscordApp::render`, the `MenuView` / `Region` views) and name injection points for a global
//! overlay and a context-menu patch — but not `MessageRow::render` (task 2.3.4) or the settings
//! page list (task 2.3.3, moved to ken). Every module here is pure logic a future detour would
//! call once that anchor exists, tested on its own against plain data, never a live client.

pub mod message_logger;
