//! UI hooks inside Phoenix (Phase 2, stage 2.3): the pure parts, tested on plain data.
//!
//! `context_menu` (2.3.5, 2.3.11) is wired into a live hook (`hooks::context_menu`).
//! `message_logger` (2.3.4) still waits on the address of `MessageRow::render`, which the private
//! recon notes do not have yet.
//! `settings` (2.3.3) is the PhoeCord settings tab as plain data: the page, its controls and the
//! edits written to `config.toml`; the native page that draws it waits on its hook.

pub mod context_menu;
pub mod message_logger;
#[cfg(feature = "dll")]
pub mod settings;
#[cfg(feature = "dll")]
pub mod voice;
