//! PhoeCord native loader (UI layer). Windows first.
//!
//! Built as a `cdylib` named like a system DLL the client loads from its own folder (`version.dll`
//! or `winhttp.dll`). On load it forwards every real export to the system copy, then, on a worker
//! thread, finds client functions by AOB signature ([`aob`]) and installs the theme and UI hooks.
//! Nothing here is needed for network plugins: if a signature misses after a Discord update, the
//! loader turns the UI layer off and the proxy keeps working (ARCHITECTURE.md, "Degradation").
//!
//! Status: only the signature scanner exists. DllMain, export forwarding and hooks are milestone 1.5
//! (prototype on today's client) and phase 2 (Phoenix).

pub mod aob;
