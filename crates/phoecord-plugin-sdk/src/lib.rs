//! PhoeCord plugin SDK: the crate third-party plugin authors depend on.
//!
//! Planned shape (docs/ROADMAP.md, milestone 1.3): a plugin is a `wasm32-wasip2` component that exports
//! `on-event` and imports the `host` interface, both generated from `wit/phoecord.wit`, which mirrors
//! `phoecord_core::plugin`. This crate will wrap the generated bindings in a `#[phoecord::plugin]`
//! macro so a plugin is one `impl` block plus a `plugin.toml`.
//!
//! It deliberately does not depend on `phoecord-core`: guests must build without the host's deps.

/// Plugin API version this SDK targets. Must equal `phoecord_core::plugin::API_VERSION`.
pub const API_VERSION: u32 = 1;
