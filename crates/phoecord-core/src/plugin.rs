//! The unified Plugin API (v1).
//!
//! The v0 proxy gave each plugin a trait with eight hook methods and compiled every plugin into the binary.
//! PhoeCord replaces that with **one entry point, one event enum and one verdict**, so the same
//! contract can be implemented by a native Rust plugin today and by a sandboxed WASM plugin through
//! `phoecord-plugin-runtime` (a single exported `on_event` function), without a stable Rust ABI.
//!
//! Rules:
//! - Plugins never touch sockets, files or the process. Everything goes through [`Host`].
//! - Events and verdicts are plain data, so they can cross the WASM boundary unchanged.
//! - Adding an [`Event`] variant is a minor API bump; changing one is a major bump ([`API_VERSION`]).

use crate::Val;

/// Version of this contract. Plugins declare the version they were built for in their manifest.
pub const API_VERSION: u32 = 1;

/// Where a plugin works. Decides when it can run and what breaks it (see docs/ARCHITECTURE.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// REST and gateway traffic. Independent of the client binary.
    Network,
    /// Palette, fonts and spacing of the GPUI client.
    Theme,
    /// Hooks into the client's own code (settings tab, rendering). Tied to a client build.
    Ui,
}

/// What a plugin is, as read from its `plugin.toml` (or declared in code by built-in plugins).
#[derive(Clone, Debug)]
pub struct Manifest {
    /// Stable id, `snake_case`, unique. Used as the config section name.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    /// The Vencord plugin this one mirrors, if any (used by the Vencord settings import).
    pub vencord: Option<String>,
    pub layers: Vec<Layer>,
    /// [`API_VERSION`] the plugin was built against.
    pub api: u32,
}

/// One thing a plugin may react to.
#[derive(Debug)]
pub enum Event<'a> {
    /// A REST call on its way to Discord. `route` has no `/api/vN` prefix (see [`crate::route`]).
    RestRequest {
        method: &'a str,
        route: &'a str,
        body: Option<&'a mut Val>,
    },
    /// A REST answer from Discord. A plugin may rewrite `body` in place and return
    /// [`Verdict::Changed`]; the proxy then sends the rewritten JSON to the client.
    RestResponse {
        method: &'a str,
        route: &'a str,
        status: u16,
        body: &'a mut Val,
    },
    /// A server → client gateway dispatch (`op 0`). `extra` holds payloads to emit after this one.
    Dispatch {
        name: &'a str,
        payload: &'a mut Val,
        extra: &'a mut Vec<Val>,
    },
    /// A client → server gateway payload (identify, presence update, typing…).
    Send { op: i64, payload: &'a mut Val },
}

/// [`Event`] without its data, for [`Plugin::wants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind<'a> {
    RestRequest { method: &'a str, route: &'a str },
    RestResponse { method: &'a str, route: &'a str },
    Dispatch { name: &'a str },
    Send { op: i64 },
}

/// What a plugin decided about an event.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Not interested or nothing changed.
    Pass,
    /// The payload was changed in place; it must be re-encoded.
    Changed,
    /// Swallow the event: the other side never sees it. Stops the plugin chain.
    Drop,
    /// Answer a REST request locally with this status and optional JSON body. Stops the chain.
    Reply { status: u16, body: Option<Val> },
}

/// Services the host gives a plugin. The only door to the outside world.
pub trait Host {
    /// This plugin's settings (its section of `config.toml`), e.g. `setting("emoji_size")`.
    fn setting(&self, key: &str) -> Option<Val>;
    /// Small per-plugin key/value store that survives restarts.
    fn kv_get(&self, key: &str) -> Option<Val>;
    fn kv_set(&mut self, key: &str, value: Val);
    /// The signed-in user's id, once the gateway READY has been seen.
    fn self_id(&self) -> Option<u64>;
    /// Desktop notification (rate-limited by the host).
    fn notify(&mut self, title: &str, body: &str);
    fn log(&mut self, message: &str);
    /// Queue a client → server gateway payload (for example a re-sent presence). The proxy sends
    /// the queue after the frame being handled, through the plugin chain's `Send` event.
    fn emit_send(&mut self, op: i64, payload: Val);
    /// Custom emoji ids of a guild the user is in (empty when unknown).
    fn guild_emojis(&self, guild_id: u64) -> Vec<u64>;
    /// The guild a channel belongs to (`None` for DMs and unknown channels).
    fn channel_guild(&self, channel_id: u64) -> Option<u64>;
    /// HTTPS to an external service. Only hosts listed in the plugin's manifest are allowed.
    fn fetch(
        &mut self,
        method: &str,
        url: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let _ = (method, url, body);
        Err("Host::fetch is not available in this host".into())
    }
    /// A Discord API call on the user's behalf, through the host's shared rate limiter.
    fn rest(
        &mut self,
        method: &str,
        route: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let _ = (method, route, body);
        Err("Host::rest is not available in this host".into())
    }
    /// Queue a server → client gateway payload (a whole frame such as `{"op":0,"t":"MESSAGE_CREATE",
    /// "d":{…}}`) that only this client sees, e.g. a local answer to a slash command. Sent right
    /// after the frame being handled; from a REST event it goes to the connected gateway at once.
    fn emit_dispatch(&mut self, payload: Val) {
        let _ = payload;
    }
    /// Whether the plugin with this id is switched on in the config (`None` for unknown ids).
    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        let _ = id;
        None
    }
    /// Switch a plugin on or off in the config file; the proxy applies it within seconds.
    fn set_plugin_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
        let _ = (id, on);
        Err("Host::set_plugin_enabled is not available in this host".into())
    }
    /// Append one line to `data/plugins/<id>/<file>` (JSON lines logs). `file` is a bare file name.
    fn append_data(&mut self, file: &str, line: &str) -> Result<(), String> {
        let _ = (file, line);
        Err("Host::append_data is not available in this host".into())
    }
    /// Called by the plugin chain before each plugin runs, so the host can scope `setting`,
    /// `kv_*`, `log` and rate limits to it. Plugins never call this.
    fn begin_plugin(&mut self, id: &str) {
        let _ = id;
    }
}

/// An answer to [`Host::fetch`] or [`Host::rest`].
#[derive(Clone, Debug, PartialEq)]
pub struct HostResponse {
    pub status: u16,
    /// The JSON body, if there was one and it parsed.
    pub body: Option<Val>,
}

/// A plugin, native or behind the WASM runtime.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &Manifest;
    /// Cheap pre-filter so the proxy can skip decoding traffic nobody wants.
    fn wants(&self, event: &EventKind) -> bool;
    /// Whether the plugin may want any gateway event. The proxy decodes the gateway only when some
    /// enabled plugin says yes; `wants` then filters each frame. Default: no.
    fn wants_gateway(&self) -> bool {
        false
    }
    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict;
}
