//! PhoeCord native loader (UI layer). Windows first.
//!
//! Built as a `cdylib` named like a system DLL the client loads from its own folder (`version.dll`
//! or `winhttp.dll`). Every export is forwarded to the system copy ([`proxy_dll`]); on a worker
//! thread ([`entry`]) the loader identifies the client build ([`version`]), loads the signature
//! tables ([`signatures`]), finds client functions by AOB signature ([`aob`]) and installs hooks
//! ([`hook`]). Nothing here is needed for network plugins: if a signature misses after a Discord
//! update, the loader turns the UI layer off and the proxy keeps working (docs/ARCHITECTURE.md,
//! "Degradation").
//!
//! Everything that touches Win32 is `#[cfg(windows)]`; the parsing and resolution logic is
//! platform-independent and tested on every OS.
//!
//! Status: stage 1.5.1–1.5.5 (prototype): export forwarding, `DllMain` + worker, version detection,
//! signatures, detours, and the IPC to the proxy ([`ipc`], [`pipe`]). Survival of client updates
//! is 1.5.6. Phase 2: the `theme.toml` format and its live reload ([`theme`], 2.3.1, the hook
//! that applies it comes after the Phoenix reconnaissance), the degradation plan ([`plan`]) and the
//! in-process transport that applies it (`transport`; no detour calls it yet). Getting into
//! Phoenix itself ([`inject`], 2.1.7) is launcher injection, not sideload: live-confirmed on a
//! real Phoenix process (via `phoecord dev inject`, `crates/phoecord-cli/src/dev/inject.rs`) —
//! this crate's own `DllMain` and worker ran inside it, detected the build and degraded cleanly
//! with no signature table for it yet, no crash.
//!
//! The `dll` feature (on by default) is the DLL itself. The proxy uses this crate without it, for
//! the IPC protocol and the pipe server: that is the only way the `unsafe` of the pipe stays in
//! this crate.

pub mod aob;
pub mod check;
pub mod choice;
/// The typed locators of the in-process hook targets (`hooks::found` is this module). Built without
/// the `dll` feature too: `phoecord launch` runs the same locators on `Discord.exe` on disk to pick
/// Phoenix's transport (2.2.8).
#[path = "hooks/found.rs"]
pub mod found;
#[cfg(windows)]
pub mod inject;
pub mod ipc;
pub mod locators;
pub mod log;
pub mod pe;
pub mod plan;
pub mod signatures;
pub mod theme;
pub mod ui;
pub mod version;

#[cfg(windows)]
pub mod pipe;

/// In-process hooks: locators, panic guards and the bridge to `Engine` (tasks 2.2.2, 2.2.3).
#[cfg(feature = "dll")]
pub mod hooks;

/// The in-process transport: what the detours call, delivery of effects, timers (task 2.2.7).
#[cfg(feature = "dll")]
pub mod transport;

#[cfg(all(windows, feature = "dll"))]
pub mod entry;
#[cfg(all(windows, feature = "dll"))]
pub mod hook;
#[cfg(all(windows, feature = "dll"))]
pub mod proxy_dll;
