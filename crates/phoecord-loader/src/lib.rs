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
//! is 1.5.6.
//!
//! The `dll` feature (on by default) is the DLL itself. The proxy uses this crate without it, for
//! the IPC protocol and the pipe server: that is the only way the `unsafe` of the pipe stays in
//! this crate.

pub mod aob;
pub mod ipc;
pub mod log;
pub mod pe;
pub mod signatures;
pub mod version;

#[cfg(windows)]
pub mod pipe;

#[cfg(all(windows, feature = "dll"))]
pub mod entry;
#[cfg(all(windows, feature = "dll"))]
pub mod hook;
#[cfg(all(windows, feature = "dll"))]
pub mod proxy_dll;
