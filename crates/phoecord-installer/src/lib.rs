//! PhoeCord installer: everything that changes the user's machine, in one auditable place.
//!
//! Planned (docs/ROADMAP.md, milestones 1.4–1.5): find installed clients (Stable / PTB / Canary, later
//! Phoenix), create and trust the local CA, install or remove the loader DLL, import Vencord
//! settings, and undo all of it. A GUI front end sits on top of this library; the CLI calls the
//! same functions.

/// Discord release channels the installer knows how to find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Ptb,
    Canary,
    /// The native Rust/GPUI client. Install paths unknown until it ships.
    Phoenix,
}

pub mod launch;
