//! PhoeCord network layer: the local MITM proxy between the client and Discord.
//!
//! Status: the leaf modules are ported from the v0 proxy (gateway codecs, local CA, trust stores). The
//! proxy loop, REST and gateway handlers (`proxy.rs`, `rest.rs`, `gateway.rs`) come next, rewired
//! onto `phoecord_core::Plugin` instead of the v0 built-in trait (ROADMAP.md, milestone 1.1).

pub mod ca;
pub mod codec;
pub mod trust;
