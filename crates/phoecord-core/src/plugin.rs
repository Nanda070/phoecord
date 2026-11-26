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
//!
//! # Stability (v1 is frozen)
//!
//! [`API_VERSION`] `1` is frozen (task 2.4.3). From here on only additive changes are made
//! without a major bump: a new [`Event`], [`EventKind`], [`UiEvent`], [`Verdict`] or [`Layer`]
//! variant (all `#[non_exhaustive]`, so code outside this crate matches them with a `_` arm), a new
//! field of [`Manifest`], [`MenuEntry`] or [`HostResponse`] (`#[non_exhaustive]`, built through
//! their `new` constructors), a new [`Host`] method with a default, and a new dispatch name such
//! as [`TIMER_EVENT`]. Renaming, removing or changing any of them is a major bump.
//!
//! At the WASM boundary (`wit/phoecord.wit`) the same rules are stricter, because wasmtime refuses
//! a component whose WIT `variant` or `record` has a different set of cases or fields: there, only
//! new host functions, new UI event names under the generic `ui` case and new dispatch names are
//! additive. `wit/frozen/` holds the frozen v1 contract that the runtime tests load.
//!
//! Fallible [`Host`] methods return `Result<_, String>`. The text is for logs and people, not a
//! stable contract: plugins must not match on it.

use crate::Val;

/// Version of this contract. Plugins declare the version they were built for in their manifest.
pub const API_VERSION: u32 = 1;

/// Name of the dispatch a plugin receives when one of its timers ([`Host::schedule`]) is due.
///
/// The host delivers `Event::Dispatch { name: TIMER_EVENT, payload }` with the payload
/// `{"op":0,"t":"PHOECORD_TIMER","d":{"tag":<tag>}}` to the plugin that scheduled it and to no
/// other. A plugin that wants timers says yes to `EventKind::Dispatch { name: TIMER_EVENT }` in
/// [`Plugin::wants`]. Discord never sends a dispatch with this name.
pub const TIMER_EVENT: &str = "PHOECORD_TIMER";

/// Name of the dispatch the Phoenix loader sends when the user deletes a message, before Discord
/// confirms it (the client's optimistic delete, task 2.3.6).
///
/// The payload is a whole frame shaped like Discord's `MESSAGE_DELETE`:
/// `{"op":0,"t":"PHOECORD_LOCAL_DELETE","d":{"id":<message>,"channel_id":<channel>,"guild_id":<guild>}}`
/// (`guild_id` only in a guild). It is an observation: dropping it does not stop the delete, and
/// Discord's own `MESSAGE_DELETE` still follows when the server confirms. Discord never sends a
/// dispatch with this name.
pub const LOCAL_DELETE_EVENT: &str = "PHOECORD_LOCAL_DELETE";

/// Where a plugin works. Decides when it can run and what breaks it (see docs/ARCHITECTURE.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Layer {
    /// REST and gateway traffic. Independent of the client binary.
    Network,
    /// Palette, fonts and spacing of the GPUI client.
    Theme,
    /// Hooks into the client's own code (settings tab, rendering). Tied to a client build.
    Ui,
}

/// What a plugin is, as read from its `plugin.toml` (or declared in code by built-in plugins).
///
/// Built with [`Manifest::new`]; set the other public fields afterwards.
#[derive(Clone, Debug)]
#[non_exhaustive]
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
    /// Outside hosts `Host::fetch` may call: exact host names (no scheme or port), or a pattern
    /// `*.example.com` that matches exactly one more DNS label (see [`fetch_host_matches`]).
    pub fetch: Vec<String>,
    /// Entries the plugin adds to the message context menu on Phoenix (task 2.3.5). Clicking one
    /// delivers [`UiEvent::MenuAction`] with its [`MenuEntry::id`]. WASM packages declare them as
    /// `[[menu]]` in `plugin.toml`.
    pub menu: Vec<MenuEntry>,
}

impl Manifest {
    /// A network plugin built for this [`API_VERSION`], with no authors, no Vencord counterpart,
    /// no fetch hosts and no menu entries.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
    ) -> Manifest {
        Manifest {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: description.into(),
            authors: Vec::new(),
            vencord: None,
            layers: vec![Layer::Network],
            api: API_VERSION,
            fetch: Vec::new(),
            menu: Vec::new(),
        }
    }
}

/// One entry of the message context menu, declared in [`Manifest::menu`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuEntry {
    /// Stable within the plugin, `snake_case`; comes back as [`UiEvent::MenuAction`]'s `entry`.
    pub id: String,
    /// A key of the plugin's own `i18n/<lang>.ftl`, shown through [`Host::t`] (the key itself
    /// when the bundle lacks it).
    pub label: String,
}

impl MenuEntry {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> MenuEntry {
        MenuEntry {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// One thing a plugin may react to.
#[derive(Debug)]
#[non_exhaustive]
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
    /// Something the user did in the Phoenix UI, reported by a loader UI hook. Read-only: the
    /// verdict only matters for the log.
    Ui(UiEvent<'a>),
}

impl Event<'_> {
    /// This event without its data, as [`Plugin::wants`] sees it.
    pub fn kind(&self) -> EventKind<'_> {
        match self {
            Event::RestRequest { method, route, .. } => EventKind::RestRequest { method, route },
            Event::RestResponse { method, route, .. } => EventKind::RestResponse { method, route },
            Event::Dispatch { name, .. } => EventKind::Dispatch { name },
            Event::Send { op, .. } => EventKind::Send { op: *op },
            Event::Ui(ui) => EventKind::Ui { name: ui.name() },
        }
    }
}

/// A UI event from a Phoenix loader hook, carried by [`Event::Ui`].
///
/// Each variant has a stable `snake_case` [`name`](UiEvent::name) and a [`Val`] form
/// ([`UiEvent::to_val`]): that is how it reaches WASM plugins (WIT `ui-event { name, payload }`),
/// the loader's IPC (`ui.event`) and `plugin.toml` filters (`ui:<name>`). Ids are Discord
/// snowflakes; in the `Val` form they are strings, as in Discord's JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEvent<'a> {
    /// The user clicked one of this plugin's [`Manifest::menu`] entries in a message's context
    /// menu (task 2.3.5). Delivered to the plugin that declared the entry only.
    /// Val form: `{"entry": "copy_raw", "channel_id": "1", "message_id": "2"}`.
    MenuAction {
        /// The [`MenuEntry::id`].
        entry: &'a str,
        channel_id: u64,
        message_id: u64,
    },
}

impl<'a> UiEvent<'a> {
    /// The stable name, e.g. `menu_action`.
    pub fn name(&self) -> &'static str {
        match self {
            UiEvent::MenuAction { .. } => "menu_action",
        }
    }

    /// The event's data as a map (see the variants for the shapes).
    pub fn to_val(&self) -> Val {
        let id = |n: u64| Val::Str(n.to_string());
        match *self {
            UiEvent::MenuAction {
                entry,
                channel_id,
                message_id,
            } => Val::Map(vec![
                (Val::Str("entry".into()), Val::Str(entry.into())),
                (Val::Str("channel_id".into()), id(channel_id)),
                (Val::Str("message_id".into()), id(message_id)),
            ]),
        }
    }

    /// The event back from its `name` and [`UiEvent::to_val`] form, e.g. a loader's `ui.event`.
    /// `None` for an unknown name or a payload that misses a field.
    pub fn from_val(name: &str, payload: &'a Val) -> Option<UiEvent<'a>> {
        match name {
            "menu_action" => Some(UiEvent::MenuAction {
                entry: payload.get("entry")?.as_str()?,
                channel_id: payload.get("channel_id")?.snowflake()?,
                message_id: payload.get("message_id")?.snowflake()?,
            }),
            _ => None,
        }
    }
}

/// [`Event`] without its data, for [`Plugin::wants`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventKind<'a> {
    RestRequest {
        method: &'a str,
        route: &'a str,
    },
    RestResponse {
        method: &'a str,
        route: &'a str,
    },
    Dispatch {
        name: &'a str,
    },
    Send {
        op: i64,
    },
    /// An [`Event::Ui`]; `name` is [`UiEvent::name`].
    Ui {
        name: &'a str,
    },
}

/// What a plugin decided about an event. A host that meets a verdict it does not know treats it
/// as [`Verdict::Pass`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
///
/// Methods with a default came after the first hosts; a host without one answers
/// `Err("Host::<name> is not supported by this host")` (or `None`, nothing, or the key for `t`).
/// Error texts are not a stable contract.
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
    /// the queue after the frame being handled, through the plugin chain's `Send` event; from a
    /// REST event or a [`TIMER_EVENT`] it goes to the connected gateway at once.
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
        Err("Host::fetch is not supported by this host".into())
    }
    /// [`Host::fetch`] with extra request headers, e.g. `("Referer", "https://...")`. Same rules as
    /// `fetch` (manifest hosts only, HTTPS, no redirects, no Discord credentials or cookies); the
    /// host refuses hop-by-hop and credential headers (`Host`, `Authorization`, `Cookie`,
    /// `Proxy-*`, `Connection`, `Content-Length`, `Transfer-Encoding`, `TE`, `Upgrade`,
    /// `Keep-Alive`, `Trailer`). The default (a host without it) returns `Err`.
    fn fetch_with(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let _ = (method, url, headers, body);
        Err("Host::fetch_with is not supported by this host".into())
    }
    /// A Discord API call on the user's behalf, through the host's shared rate limiter.
    fn rest(
        &mut self,
        method: &str,
        route: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let _ = (method, route, body);
        Err("Host::rest is not supported by this host".into())
    }
    /// A Discord API call with files, sent as `multipart/form-data` the way the client uploads
    /// attachments: `payload` becomes the `payload_json` part, each file a `files[n]` part. Same
    /// session, route rules and rate limits as [`Host::rest`].
    fn rest_files(
        &mut self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
    ) -> Result<HostResponse, String> {
        let _ = (method, route, payload, files);
        Err("Host::rest_files is not supported by this host".into())
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
        Err("Host::set_plugin_enabled is not supported by this host".into())
    }
    /// Append one line to `data/plugins/<id>/<file>` (JSON lines logs). `file` is a bare file name.
    fn append_data(&mut self, file: &str, line: &str) -> Result<(), String> {
        let _ = (file, line);
        Err("Host::append_data is not supported by this host".into())
    }
    /// A string of this plugin's own bundle (`i18n/<lang>.ftl`) in the user's language, with
    /// `args` filled into its `{ $name }` placeholders. Lookup: the user's language, then English,
    /// then the key itself. The default returns the key, so a host without bundles still works.
    fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        let _ = args;
        key.to_string()
    }
    /// Ask for a [`TIMER_EVENT`] dispatch carrying `tag` in about `after_ms` milliseconds, so a
    /// plugin can run delayed work (a pause between deletes, a heartbeat) without client traffic.
    ///
    /// When due, the host delivers `Event::Dispatch { name: TIMER_EVENT, payload:
    /// {"op":0,"t":"PHOECORD_TIMER","d":{"tag":<tag>}} }` to this plugin only, only while it is
    /// enabled, and only if its `wants(EventKind::Dispatch { name: TIMER_EVENT })` is true. Payloads
    /// it queues there (`emit_send`, `emit_dispatch`) go out at once.
    ///
    /// Rules: a delay under 50 ms is raised to 50 ms, more than 24 h is an `Err`, and at most 64
    /// timers may be pending per plugin (the 65th is an `Err`). Scheduling the same tag twice gives
    /// two timers. Timers live in memory only: a restart forgets them. The default (a host without
    /// timers) returns `Err`.
    fn schedule(&mut self, after_ms: u64, tag: &str) -> Result<(), String> {
        let _ = (after_ms, tag);
        Err("Host::schedule is not supported by this host".into())
    }
    /// Cancel every pending timer of this plugin with this `tag` (nothing pending: no-op).
    fn cancel_schedule(&mut self, tag: &str) {
        let _ = tag;
    }
    /// The raw query string (no leading `?`) of the REST call being handled, while a plugin
    /// handles its [`Event::RestRequest`] or [`Event::RestResponse`]; `None` for other events and
    /// for calls without one. [`Event`] routes never carry the query, so `wants` matches the
    /// path alone; a plugin that needs a parameter (`GET /gifs/search?q=…`) reads it here and
    /// decodes it with [`crate::route::query_pairs`]. The query can hold what the user typed, so
    /// do not log it. Added to v1 as an additive method; the default (a host without it) is `None`.
    fn request_query(&self) -> Option<String> {
        None
    }
}

/// The host side of [`Host`] that only the plugin chain uses: plugins get `&mut dyn Host` and
/// cannot reach these. A host that does not scope per plugin implements it with no methods.
pub trait ChainHost: Host {
    /// Called by the plugin chain before each plugin runs, so the host can scope `setting`,
    /// `kv_*`, `log` and rate limits to it.
    fn begin_plugin(&mut self, id: &str) {
        let _ = id;
    }
    /// Called by the plugin chain right after `begin_plugin` with the hosts the plugin's manifest
    /// lists in `fetch`, so [`Host::fetch`] can check its allow-list.
    fn set_fetch_hosts(&mut self, hosts: &[String]) {
        let _ = hosts;
    }
}

/// Request headers [`Host::fetch_with`] refuses (lower-case; `proxy-` is a prefix).
pub const FETCH_REFUSED_HEADERS: &[&str] = &[
    "host",
    "authorization",
    "cookie",
    "connection",
    "content-length",
    "transfer-encoding",
    "te",
    "upgrade",
    "keep-alive",
    "trailer",
];

/// Whether [`Host::fetch_with`] refuses a request header name.
pub fn fetch_header_refused(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n.starts_with("proxy-") || FETCH_REFUSED_HEADERS.contains(&n.as_str())
}

/// Whether a manifest `fetch` entry allows `host`. An entry is an exact host name, or
/// `*.example.com`, which matches exactly one more DNS label: `123.example.com`, but not
/// `example.com` nor `a.b.example.com`. Case does not matter.
pub fn fetch_host_matches(entry: &str, host: &str) -> bool {
    let (entry, host) = (entry.to_ascii_lowercase(), host.to_ascii_lowercase());
    match entry.strip_prefix("*.") {
        Some(base) => host
            .strip_suffix(base)
            .and_then(|label| label.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.') && !base.is_empty()),
        None => entry == host,
    }
}

/// An answer to [`Host::fetch`] or [`Host::rest`]. Built with [`HostResponse::new`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct HostResponse {
    pub status: u16,
    /// The JSON body, if there was one and it parsed.
    pub body: Option<Val>,
    /// The body as text when it was not JSON (UTF-8, lossy), so plugins can read plain answers.
    pub text: Option<String>,
    /// Response headers that matter to plugins (lower-case names): `content-type`, `retry-after`,
    /// `x-ratelimit-*`, `link`, `location`. Never cookies.
    pub headers: Vec<(String, String)>,
    /// The raw body when it was not JSON, e.g. an image a plugin fetched. `None` for JSON and empty
    /// bodies.
    pub bytes: Option<Vec<u8>>,
}

impl HostResponse {
    /// An answer with this status and no body, text, headers or bytes.
    pub fn new(status: u16) -> HostResponse {
        HostResponse {
            status,
            body: None,
            text: None,
            headers: Vec::new(),
            bytes: None,
        }
    }
}

/// One file for [`Host::rest_files`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePart {
    /// The file name Discord shows, e.g. `petpet.gif`.
    pub filename: String,
    /// MIME type, e.g. `image/gif`.
    pub content_type: String,
    pub data: Vec<u8>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that implements only the required methods.
    struct Bare;

    impl Host for Bare {
        fn setting(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            None
        }
        fn notify(&mut self, _: &str, _: &str) {}
        fn log(&mut self, _: &str) {}
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, _: u64) -> Vec<u64> {
            Vec::new()
        }
        fn channel_guild(&self, _: u64) -> Option<u64> {
            None
        }
    }

    #[test]
    fn fetch_patterns_match_exactly_one_more_label() {
        let p = "*.discordsays.com";
        assert!(fetch_host_matches(p, "123.discordsays.com"));
        assert!(fetch_host_matches(p, "ABC.DiscordSays.com"));
        assert!(!fetch_host_matches(p, "discordsays.com"));
        assert!(!fetch_host_matches(p, ".discordsays.com"));
        assert!(!fetch_host_matches(p, "a.b.discordsays.com"));
        assert!(!fetch_host_matches(p, "evildiscordsays.com"));
        assert!(!fetch_host_matches(p, "123.discordsays.com.evil.example"));
        assert!(fetch_host_matches("api.example.com", "API.example.com"));
        assert!(!fetch_host_matches("api.example.com", "x.api.example.com"));
        assert!(!fetch_host_matches("*.", "a."));
    }

    #[test]
    fn fetch_with_refuses_credential_and_hop_by_hop_headers() {
        for n in [
            "Host",
            "authorization",
            "Cookie",
            "Proxy-Authorization",
            "proxy-x",
            "TE",
            "Content-Length",
        ] {
            assert!(fetch_header_refused(n), "{n}");
        }
        for n in [
            "Referer",
            "X-Auth-Token",
            "Content-Type",
            "X-Discord-Quest-ID",
        ] {
            assert!(!fetch_header_refused(n), "{n}");
        }
        let mut h = Bare;
        assert!(
            h.fetch_with("GET", "https://x.example/", &[], None)
                .is_err()
        );
    }

    #[test]
    fn timers_are_refused_by_a_host_without_them() {
        let mut h = Bare;
        assert_eq!(
            h.schedule(1000, "tick"),
            Err("Host::schedule is not supported by this host".to_string())
        );
        h.cancel_schedule("tick");
        assert_eq!(TIMER_EVENT, "PHOECORD_TIMER");
    }

    #[test]
    fn ui_events_round_trip_through_their_val_form() {
        let ev = UiEvent::MenuAction {
            entry: "copy_raw",
            channel_id: 1,
            message_id: 1_234_567_890_123_456_789,
        };
        assert_eq!(ev.name(), "menu_action");
        let val = ev.to_val();
        assert_eq!(val.str_at(&["message_id"]), Some("1234567890123456789"));
        assert_eq!(UiEvent::from_val("menu_action", &val), Some(ev));
        assert_eq!(UiEvent::from_val("settings_render", &val), None);
        let missing = Val::Map(vec![(Val::Str("entry".into()), Val::Str("x".into()))]);
        assert_eq!(UiEvent::from_val("menu_action", &missing), None);
        assert_eq!(
            Event::Ui(ev).kind(),
            EventKind::Ui {
                name: "menu_action"
            }
        );
        assert_eq!(LOCAL_DELETE_EVENT, "PHOECORD_LOCAL_DELETE");
    }

    #[test]
    fn every_event_knows_its_kind() {
        let mut v = Val::Null;
        let ev = Event::RestRequest {
            method: "GET",
            route: "/a",
            body: None,
        };
        assert_eq!(
            ev.kind(),
            EventKind::RestRequest {
                method: "GET",
                route: "/a"
            }
        );
        let ev = Event::Send {
            op: 3,
            payload: &mut v,
        };
        assert_eq!(ev.kind(), EventKind::Send { op: 3 });
    }

    #[test]
    fn constructors_fill_the_defaults() {
        let m = Manifest::new("p", "P", "1.0.0", "d");
        assert_eq!(m.api, API_VERSION);
        assert_eq!(m.layers, vec![Layer::Network]);
        assert!(m.authors.is_empty() && m.fetch.is_empty() && m.menu.is_empty());
        assert!(m.vencord.is_none());
        let r = HostResponse::new(204);
        assert_eq!(r.status, 204);
        assert!(r.body.is_none() && r.text.is_none() && r.bytes.is_none() && r.headers.is_empty());
        assert_eq!(MenuEntry::new("a", "b").label, "b");
    }

    #[test]
    fn a_host_without_the_newer_methods_says_so() {
        let mut h = Bare;
        let msg = |name: &str| Err(format!("Host::{name} is not supported by this host"));
        assert_eq!(h.fetch("GET", "https://x/", None).map(|_| ()), msg("fetch"));
        assert_eq!(h.rest("GET", "/x", None).map(|_| ()), msg("rest"));
        assert_eq!(h.append_data("f", "l"), msg("append_data"));
        assert_eq!(h.set_plugin_enabled("p", true), msg("set_plugin_enabled"));
        assert_eq!(h.request_query(), None);
        assert_eq!(h.t("key", &[]), "key");
    }
}
