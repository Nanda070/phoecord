//! Plugin packages and the community catalog, shared by `phoecord` (CLI) and the installer window.
//!
//! - [`store`]: stage a package into `plugins-staging/`, check it, commit it into `plugins/<id>`,
//!   remove it (what the window's Plugins tab always did; it moved here so the CLI can use it).
//! - [`index`]: the catalog file `catalog/plugins.json`: parsing, validation, update detection.
//! - [`catalog`]: fetching the index and a hash-checked download of an entry.
//! - [`asks`]: a package's events and rights in words, for every confirm dialog.

pub mod asks;
pub mod catalog;
pub mod index;
pub mod net;
pub mod store;
