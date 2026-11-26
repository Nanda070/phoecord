//! PhoeCord core: the shared value model and the Plugin API.
//!
//! Every other crate depends on this one and on nothing else of ours. It must stay small, free of
//! I/O and stable: third-party plugins are built against the API defined in [`plugin`].

pub mod etf;
pub mod plugin;
pub mod route;
mod val;

pub use plugin::{
    ChainHost, Event, EventKind, FilePart, Host, HostResponse, LOCAL_DELETE_EVENT, Layer, Manifest,
    MenuEntry, Plugin, TIMER_EVENT, UiEvent, Verdict,
};
pub use val::Val;
