//! Plugin packages: a folder `plugins/<id>/` holding `plugin.toml` and `plugin.wasm`.
//!
//! ```toml
//! id = "clear_urls"            # snake_case, equals the folder name
//! name = "ClearURLs"
//! version = "0.1.0"
//! description = "Strips tracking parameters from links"
//! authors = ["someone"]
//! vencord = "ClearURLs"        # optional
//! layers = ["network"]         # network (default), theme, ui
//! api = 1                      # Plugin API version, must equal this host's
//!
//! # Events the plugin may see. `kind` or `kind:glob`; `*` and `?` match any text / one character.
//! #   rest_request / rest_response: glob over "METHOD route", e.g. "rest_request:POST /channels/*/messages"
//! #   dispatch: glob over the event name, e.g. "dispatch:MESSAGE_*"
//! #   send: glob over the op number, e.g. "send:3"
//! #   ui: glob over the UI event name (`UiEvent::name`), e.g. "ui:menu_action"
//! events = ["dispatch:MESSAGE_CREATE"]
//! rights = ["notify"]          # which `Host` methods it may call (see `wasm::Right`)
//! fetch = ["api.example.com"]  # outside hosts `Host::fetch` may call (needs the `fetch` right);
//!                               # "*.example.com" matches exactly one more label
//!
//! # Message context menu entries on Phoenix: clicking one sends `ui:menu_action` with its `id`.
//! [[menu]]
//! id = "copy_raw"              # snake_case, unique in the plugin
//! label = "menu-copy-raw"      # a key of the plugin's own i18n/<lang>.ftl
//!
//! # Settings shown in the PhoeCord settings tab, in this order. Values are still read with
//! # `Host::setting(key)` from the plugin's `config.toml` section; this only describes them.
//! [[setting]]
//! key = "mode"                 # the config key, unique in the plugin
//! kind = "choice"              # bool, number, text, choice (needs `options`, strings or numbers) or list
//! default = "fast"             # must fit the kind (a choice: one of `options`)
//! options = ["fast", "exact"]
//! label = "setting-mode"       # a key of the plugin's own i18n/<lang>.ftl
//! ```
//!
//! Rights (`rights = [...]`): a call without its right is an error for the guest and one log line
//! per right. `setting`, `self_id` and `log` need none.
//!
//! | right           | `Host` methods                                    |
//! |:----------------|:--------------------------------------------------|
//! | `kv`            | `kv_get`, `kv_set`                                |
//! | `notify`        | `notify`                                          |
//! | `emit_send`     | `emit_send` (client to server frames)             |
//! | `emit_dispatch` | `emit_dispatch` (server to client frames, local)  |
//! | `guilds`        | `guild_emojis`, `channel_guild`                   |
//! | `fetch`         | `fetch` (HTTPS to external services)              |
//! | `rest`          | `rest` (Discord API on the user's behalf)         |
//! | `plugins`       | `plugin_enabled`, `set_plugin_enabled`            |
//! | `data`          | `append_data` (`data/plugins/<id>/<file>`)        |
//!
//! `t` (the plugin's own strings) needs no right.
//!
//! Strings: an optional `i18n/<lang>.ftl` (Fluent; `<lang>` is `en` or `ru`) next to the manifest
//! holds the plugin's own messages. `Host::t` reads them in the user's language, falling back to
//! English and then to the key. They load with the package and again on every hot reload.
//!
//! A [`PackagePlugin`] swaps in a changed package between events; [`PackageSet::sync`] (woken by
//! [`crate::watch`] notifications) reloads it at once.

use crate::wasm::{Setup, WasmPlugin, WasmRuntime};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{
    Event, EventKind, Host, Layer, Manifest, MenuEntry, Plugin, SettingKind, SettingSpec, Val,
    Verdict,
};
use phoecord_i18n::PluginStrings;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

// ---------------------------------------------------------------------------------------------
// `plugin.toml`
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    authors: Vec<String>,
    vencord: Option<String>,
    #[serde(default = "default_layers")]
    layers: Vec<String>,
    api: u32,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    rights: Vec<String>,
    /// Outside hosts `Host::fetch` may call, e.g. `["translate.googleapis.com"]`.
    #[serde(default)]
    fetch: Vec<String>,
    /// `[[menu]]` tables: message context menu entries.
    #[serde(default)]
    menu: Vec<RawMenuEntry>,
    /// `[[setting]]` tables: what the settings tab shows.
    #[serde(default)]
    setting: Vec<RawSetting>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetting {
    key: String,
    kind: String,
    default: toml::Value,
    #[serde(default)]
    options: Vec<toml::Value>,
    label: String,
}

/// A `plugin.toml` value as the [`Val`] `Host::setting` would hand back; `None` for a table or
/// a date, which no setting kind takes.
fn toml_to_val(v: &toml::Value) -> Option<Val> {
    Some(match v {
        toml::Value::String(s) => Val::Str(s.clone()),
        toml::Value::Integer(n) => Val::Int(*n),
        toml::Value::Float(f) => Val::Float(*f),
        toml::Value::Boolean(b) => Val::Bool(*b),
        toml::Value::Array(items) => {
            Val::List(items.iter().map(toml_to_val).collect::<Option<_>>()?)
        }
        toml::Value::Table(_) | toml::Value::Datetime(_) => return None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMenuEntry {
    id: String,
    label: String,
}

fn default_layers() -> Vec<String> {
    vec!["network".into()]
}

/// Everything `plugin.toml` says, validated.
#[derive(Clone, Debug)]
pub struct PackageManifest {
    pub manifest: Manifest,
    pub events: EventFilter,
    /// The `rights` list as written (checked by the runtime when the plugin is built).
    pub rights: Vec<String>,
}

impl PackageManifest {
    pub fn parse(toml_text: &str) -> Result<PackageManifest, String> {
        let raw: RawManifest = toml::from_str(toml_text).map_err(|e| e.to_string())?;
        if raw.api != API_VERSION {
            return Err(format!(
                "the plugin targets Plugin API v{}, this host speaks v{API_VERSION}",
                raw.api
            ));
        }
        if !valid_id(&raw.id) {
            return Err(format!(
                "id {:?} must be snake_case (a-z, 0-9, _), starting with a letter",
                raw.id
            ));
        }
        let layers = raw
            .layers
            .iter()
            .map(|l| match l.as_str() {
                "network" => Ok(Layer::Network),
                "theme" => Ok(Layer::Theme),
                "ui" => Ok(Layer::Ui),
                other => Err(format!("unknown layer {other:?}")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let events = EventFilter::parse(&raw.events)?;
        let fetch = raw
            .fetch
            .iter()
            .map(|h| valid_fetch_host(h))
            .collect::<Result<Vec<_>, _>>()?;
        let mut menu: Vec<MenuEntry> = Vec::new();
        for entry in raw.menu {
            if !valid_id(&entry.id) {
                return Err(format!(
                    "menu id {:?} must be snake_case (a-z, 0-9, _), starting with a letter",
                    entry.id
                ));
            }
            if entry.label.trim().is_empty() {
                return Err(format!("menu entry {:?} has an empty label", entry.id));
            }
            if menu.iter().any(|m| m.id == entry.id) {
                return Err(format!("menu id {:?} is listed twice", entry.id));
            }
            menu.push(MenuEntry::new(entry.id, entry.label));
        }
        let mut settings: Vec<SettingSpec> = Vec::new();
        for raw in raw.setting {
            let kind = SettingKind::from_name(&raw.kind).ok_or_else(|| {
                format!(
                    "setting {:?}: unknown kind {:?} (bool, number, text, choice, list)",
                    raw.key, raw.kind
                )
            })?;
            let default = toml_to_val(&raw.default).ok_or_else(|| {
                format!("setting {:?}: the default is a table or a date", raw.key)
            })?;
            let mut spec = SettingSpec::new(raw.key, kind, default, raw.label);
            spec.options = raw
                .options
                .iter()
                .map(toml_to_val)
                .collect::<Option<_>>()
                .ok_or_else(|| format!("setting {:?}: an option is a table or a date", spec.key))?;
            if let Some(why) = spec.problem() {
                return Err(why);
            }
            if settings.iter().any(|s| s.key == spec.key) {
                return Err(format!("setting {:?} is listed twice", spec.key));
            }
            settings.push(spec);
        }
        Ok(PackageManifest {
            manifest: {
                let mut m = Manifest::new(raw.id, raw.name, raw.version, raw.description);
                m.authors = raw.authors;
                m.vencord = raw.vencord;
                m.layers = layers;
                m.api = raw.api;
                m.fetch = fetch;
                m.menu = menu;
                m.settings = settings;
                m
            },
            events,
            rights: raw.rights,
        })
    }
}

/// A `fetch` entry is a bare lower-case host name (no scheme, port or path), or `*.` and such a
/// name: the pattern matches exactly one more DNS label (`*.discordsays.com` allows
/// `123.discordsays.com`, not `discordsays.com` nor `a.b.discordsays.com`).
fn valid_fetch_host(h: &str) -> Result<String, String> {
    let h = h.trim().to_ascii_lowercase();
    let name = h.strip_prefix("*.").unwrap_or(&h);
    let ok = !name.is_empty()
        && name.split('.').count() >= 2
        && name.split('.').all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if ok {
        Ok(h)
    } else {
        Err(format!(
            "fetch host {h:?} must be a plain host name such as \"api.example.com\" or a pattern \
             such as \"*.example.com\""
        ))
    }
}

fn valid_id(id: &str) -> bool {
    id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ---------------------------------------------------------------------------------------------
// Event patterns
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    RestRequest,
    RestResponse,
    Dispatch,
    Send,
    Ui,
}

#[derive(Clone, Debug)]
struct Pattern {
    kind: Kind,
    /// `None` matches every event of the kind.
    glob: Option<String>,
}

/// The events a plugin subscribed to. An empty filter delivers nothing.
#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    patterns: Vec<Pattern>,
}

impl EventFilter {
    /// Every event (used for plugins that are not loaded from a package).
    pub fn all() -> EventFilter {
        EventFilter {
            patterns: [
                Kind::RestRequest,
                Kind::RestResponse,
                Kind::Dispatch,
                Kind::Send,
                Kind::Ui,
            ]
            .into_iter()
            .map(|kind| Pattern { kind, glob: None })
            .collect(),
        }
    }

    pub fn parse(specs: &[String]) -> Result<EventFilter, String> {
        let patterns = specs
            .iter()
            .map(|spec| {
                let (kind, glob) = match spec.split_once(':') {
                    Some((k, g)) => (k, Some(g.to_string())),
                    None => (spec.as_str(), None),
                };
                let kind = match kind {
                    "rest_request" => Kind::RestRequest,
                    "rest_response" => Kind::RestResponse,
                    "dispatch" => Kind::Dispatch,
                    "send" => Kind::Send,
                    "ui" => Kind::Ui,
                    other => return Err(format!("unknown event kind {other:?} in {spec:?}")),
                };
                Ok(Pattern { kind, glob })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(EventFilter { patterns })
    }

    pub fn matches(&self, event: &EventKind) -> bool {
        self.patterns.iter().any(|p| {
            let (kind, subject) = match event {
                EventKind::RestRequest { method, route } => {
                    (Kind::RestRequest, format!("{method} {route}"))
                }
                EventKind::RestResponse { method, route } => {
                    (Kind::RestResponse, format!("{method} {route}"))
                }
                EventKind::Dispatch { name } => (Kind::Dispatch, (*name).to_string()),
                EventKind::Send { op } => (Kind::Send, op.to_string()),
                EventKind::Ui { name } => (Kind::Ui, (*name).to_string()),
                // A kind `plugin.toml` cannot name yet is never let through.
                _ => return false,
            };
            p.kind == kind && p.glob.as_deref().is_none_or(|g| glob_match(g, &subject))
        })
    }

    /// Whether any pattern is about the gateway.
    pub fn wants_gateway(&self) -> bool {
        self.patterns
            .iter()
            .any(|p| matches!(p.kind, Kind::Dispatch | Kind::Send))
    }
}

/// `*` matches any run of characters (also none), `?` exactly one.
fn glob_match(glob: &str, text: &str) -> bool {
    let (g, t): (Vec<char>, Vec<char>) = (glob.chars().collect(), text.chars().collect());
    let (mut gi, mut ti) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while ti < t.len() {
        if gi < g.len() && (g[gi] == '?' || g[gi] == t[ti]) {
            gi += 1;
            ti += 1;
        } else if gi < g.len() && g[gi] == '*' {
            backtrack = Some((gi, ti));
            gi += 1;
        } else if let Some((bg, bt)) = backtrack {
            gi = bg + 1;
            ti = bt + 1;
            backtrack = Some((bg, bt + 1));
        } else {
            return false;
        }
    }
    g[gi..].iter().all(|c| *c == '*')
}

// ---------------------------------------------------------------------------------------------
// Loading and reloading
// ---------------------------------------------------------------------------------------------

/// What identifies a version of a package on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    toml: Option<(SystemTime, u64)>,
    wasm: Option<(SystemTime, u64)>,
    /// `i18n/*.ftl`: file name, modified time, size; sorted by name.
    i18n: Vec<(String, SystemTime, u64)>,
}

fn stamp(dir: &Path) -> Stamp {
    let one = |name: &str| {
        let m = std::fs::metadata(dir.join(name)).ok()?;
        Some((m.modified().ok()?, m.len()))
    };
    let mut i18n: Vec<(String, SystemTime, u64)> = ftl_files(dir)
        .into_iter()
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?;
            Some((
                p.file_name()?.to_str()?.to_string(),
                m.modified().ok()?,
                m.len(),
            ))
        })
        .collect();
    i18n.sort();
    Stamp {
        toml: one("plugin.toml"),
        wasm: one("plugin.wasm"),
        i18n,
    }
}

/// `dir/i18n/*.ftl`, the plugin's own strings (file stem = language).
fn ftl_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join("i18n"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ftl") && p.is_file())
        .collect()
}

/// The largest `i18n/<lang>.ftl` the runtime reads; a bigger file is skipped and reported.
pub const MAX_FTL_BYTES: u64 = 256 * 1024;

/// The package's strings as `(language, ftl text)`. An unreadable file is skipped and reported
/// in `problems`.
fn read_i18n(dir: &Path, problems: &mut Vec<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in ftl_files(dir) {
        let lang = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > MAX_FTL_BYTES) {
            problems.push(format!(
                "{} is larger than {} KB and was skipped",
                p.display(),
                MAX_FTL_BYTES / 1024
            ));
            continue;
        }
        match std::fs::read_to_string(&p) {
            Ok(text) => out.push((lang.to_string(), text)),
            Err(e) => problems.push(format!("cannot read {}: {e}", p.display())),
        }
    }
    out.sort();
    out
}

/// A package read from disk: the compiled plugin and its strings.
struct Built {
    plugin: WasmPlugin,
    i18n: Vec<(String, String)>,
    /// Non-fatal problems (an unreadable `.ftl`).
    problems: Vec<String>,
}

fn build(runtime: &Arc<WasmRuntime>, dir: &Path, expect_id: Option<&str>) -> Result<Built, String> {
    let read =
        |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("cannot read {name}: {e}"));
    let text = String::from_utf8(read("plugin.toml")?)
        .map_err(|_| "plugin.toml is not UTF-8".to_string())?;
    let package = PackageManifest::parse(&text).map_err(|e| format!("plugin.toml: {e}"))?;
    let folder = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if package.manifest.id != folder {
        return Err(format!(
            "plugin.toml: id {:?} must equal the folder name {folder:?}",
            package.manifest.id
        ));
    }
    if let Some(id) = expect_id
        && id != package.manifest.id
    {
        return Err(format!(
            "plugin.toml: id changed from {id:?} to {:?}; restart to load it",
            package.manifest.id
        ));
    }
    let wasm = read("plugin.wasm")?;
    let plugin = WasmPlugin::new(runtime, &wasm, Setup::from_package(package)?)
        .map_err(|e| format!("plugin.wasm: {e:#}"))?;
    let mut problems = Vec::new();
    let i18n = read_i18n(dir, &mut problems);
    Ok(Built {
        plugin,
        i18n,
        problems,
    })
}

/// A plugin loaded from a package folder that follows the files on disk.
pub struct PackagePlugin {
    runtime: Arc<WasmRuntime>,
    dir: PathBuf,
    /// The manifest of the first load. The id never changes; other fields (name, version, …) show
    /// the first load until PhoeCord restarts, because [`Plugin::manifest`] hands out a reference.
    manifest: Manifest,
    current: RwLock<WasmPlugin>,
    /// Where the package's `i18n/<lang>.ftl` files go (the host's plugin strings), if anywhere.
    strings: Option<Arc<PluginStrings>>,
    watch: Mutex<Watch>,
    /// Reload problems, logged on the next event (there is no host to log to while checking).
    problems: Mutex<Vec<String>>,
}

struct Watch {
    seen: Stamp,
    checked: Instant,
    every: Duration,
}

impl PackagePlugin {
    /// Load `dir` (`plugins/<id>`). Fails when the package is invalid.
    pub fn load(runtime: &Arc<WasmRuntime>, dir: &Path) -> Result<PackagePlugin, String> {
        PackagePlugin::load_with(runtime, dir, None)
    }

    /// Like [`PackagePlugin::load`], and publish the package's `i18n/<lang>.ftl` files to
    /// `strings` now and on every reload (this is what makes `Host::t` find them).
    pub fn load_with(
        runtime: &Arc<WasmRuntime>,
        dir: &Path,
        strings: Option<Arc<PluginStrings>>,
    ) -> Result<PackagePlugin, String> {
        let seen = stamp(dir);
        let built = build(runtime, dir, None)?;
        let manifest = built.plugin.manifest().clone();
        if let Some(s) = &strings {
            s.replace_plugin(&manifest.id, &built.i18n);
        }
        Ok(PackagePlugin {
            runtime: runtime.clone(),
            dir: dir.to_path_buf(),
            manifest,
            current: RwLock::new(built.plugin),
            strings,
            watch: Mutex::new(Watch {
                seen,
                checked: Instant::now(),
                every: Duration::from_secs(1),
            }),
            problems: Mutex::new(built.problems),
        })
    }

    /// How often the files are looked at (default one second; checked when an event is asked about).
    pub fn set_check_interval(&self, every: Duration) {
        self.watch.lock().unwrap().every = every;
    }

    /// Whether the plugin is switched off by a fault (a reload of a changed package switches it on).
    pub fn is_disabled(&self) -> bool {
        self.current.read().unwrap().is_disabled()
    }

    /// Swap in the package if its files changed (lazily: at most once per check interval).
    fn refresh(&self) {
        {
            let mut w = self.watch.lock().unwrap();
            if w.checked.elapsed() < w.every {
                return;
            }
            w.checked = Instant::now();
        }
        if let Some(line) = self.reload_if_changed() {
            self.problems.lock().unwrap().push(line);
        }
    }

    /// Swap in the package now if its files changed, whatever the check interval. Returns the
    /// line to log: the package was reloaded, or it changed but the old version keeps running.
    /// `None` when nothing changed. A broken new version leaves the old one running.
    pub fn reload_if_changed(&self) -> Option<String> {
        {
            let mut w = self.watch.lock().unwrap();
            let now = stamp(&self.dir);
            if now == w.seen {
                return None;
            }
            w.seen = now;
        }
        match build(&self.runtime, &self.dir, Some(&self.manifest.id)) {
            Ok(new) => {
                if let Some(s) = &self.strings {
                    s.replace_plugin(&self.manifest.id, &new.i18n);
                }
                *self.current.write().unwrap() = new.plugin;
                self.problems.lock().unwrap().extend(new.problems);
                Some(format!("plugin {} reloaded", self.manifest.id))
            }
            Err(e) => Some(format!(
                "plugin {} was changed on disk but not reloaded (the old version keeps running): {e}",
                self.manifest.id
            )),
        }
    }
}

impl Plugin for PackagePlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        self.refresh();
        self.current.read().unwrap().wants(event)
    }

    fn wants_gateway(&self) -> bool {
        self.current.read().unwrap().wants_gateway()
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        self.refresh();
        for line in self.problems.lock().unwrap().drain(..) {
            host.log(&line);
        }
        self.current.read().unwrap().on_event(host, event)
    }
}

/// The result of [`load_dir`].
pub struct Loaded {
    pub plugins: Vec<PackagePlugin>,
    /// Folders that could not be loaded, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

/// Load every package under `root` (`root/<id>/plugin.toml`), in folder-name order. A broken
/// package is reported in `errors` and does not stop the others.
pub fn load_dir(runtime: &Arc<WasmRuntime>, root: &Path) -> Loaded {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("plugin.toml").is_file())
        .collect();
    dirs.sort();
    let mut out = Loaded {
        plugins: Vec::new(),
        errors: Vec::new(),
    };
    for dir in dirs {
        match PackagePlugin::load(runtime, &dir) {
            Ok(p) => out.plugins.push(p),
            Err(e) => out.errors.push((dir, e)),
        }
    }
    out
}

/// Keeps a live [`crate::Chain`] in step with a `plugins/` folder: a package copied in is loaded,
/// a folder removed is unloaded, a package that is already running is left alone (it follows its
/// own files, see [`PackagePlugin`]). Call [`PackageSet::sync`] on every config reload or folder
/// scan; it is cheap when nothing changed.
pub struct PackageSet {
    runtime: Arc<WasmRuntime>,
    root: PathBuf,
    /// Packages that failed to load, with the files they had then: tried again only when the
    /// files change, so a half-copied package is picked up once it is complete and a broken one
    /// is reported once.
    failed: HashMap<String, Stamp>,
    /// Receives the strings of every package this set loads.
    strings: Option<Arc<PluginStrings>>,
    /// The packages this set loaded, by id, so a change on disk reloads them at once instead of
    /// at their next event.
    loaded: HashMap<String, Arc<PackagePlugin>>,
}

impl PackageSet {
    pub fn new(runtime: Arc<WasmRuntime>, root: PathBuf) -> PackageSet {
        PackageSet {
            runtime,
            root,
            failed: HashMap::new(),
            strings: None,
            loaded: HashMap::new(),
        }
    }

    /// The folder holding the packages.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publish the `i18n/<lang>.ftl` files of loaded packages to `strings`, and remove them when
    /// a package is unloaded.
    pub fn with_strings(mut self, strings: Arc<PluginStrings>) -> PackageSet {
        self.strings = Some(strings);
        self
    }

    /// Reload the package `id` from its folder now, whether its files changed or not (the dev
    /// console). The new version replaces the running one in the chain; if it fails to load, the
    /// old one keeps running and the error is returned.
    pub fn reload(&mut self, chain: &crate::Chain, id: &str) -> Result<(), String> {
        if !chain.is_dynamic(id) {
            return Err(format!("{id} is not a WASM package"));
        }
        let dir = self.root.join(id);
        let plugin = PackagePlugin::load_with(&self.runtime, &dir, self.strings.clone())?;
        if plugin.manifest().id != id {
            return Err(format!(
                "the package in {} now has the id {}: restart PhoeCord to rename a plugin",
                dir.display(),
                plugin.manifest().id
            ));
        }
        self.failed.remove(id);
        let plugin = self.adopt(id, plugin);
        if chain.swap_dynamic(plugin) {
            Ok(())
        } else {
            self.loaded.remove(id);
            Err(format!("{id} was unloaded meanwhile"))
        }
    }

    /// Keep `plugin` as the instance of folder `name` that [`PackageSet::sync`] reloads. Its own
    /// lazy check is turned off: a reload on a traffic thread would leave `sync` with nothing to
    /// report, and the transport would not replay gateway state for the new subscriptions.
    fn adopt(&mut self, name: &str, plugin: PackagePlugin) -> Arc<PackagePlugin> {
        plugin.set_check_interval(Duration::MAX);
        let plugin = Arc::new(plugin);
        self.loaded.insert(name.to_string(), plugin.clone());
        plugin
    }

    /// Load new packages and drop removed ones. Returns the lines to log, empty when nothing
    /// changed. A broken package never affects the plugins that are running.
    pub fn sync(&mut self, chain: &crate::Chain) -> Vec<String> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("plugin.toml").is_file())
            .collect();
        dirs.sort();
        let present: Vec<String> = dirs
            .iter()
            .filter_map(|d| d.file_name()?.to_str().map(str::to_string))
            .collect();
        let running = chain.dynamic_plugins();
        let mut logs = Vec::new();
        let mut next: Vec<Arc<dyn Plugin>> = Vec::new();
        for (dir, name) in dirs.iter().zip(&present) {
            if let Some(p) = running.iter().find(|p| &p.manifest().id == name) {
                next.push(p.clone());
                if let Some(typed) = self.loaded.get(name)
                    && let Some(line) = typed.reload_if_changed()
                {
                    logs.push(line);
                }
                continue;
            }
            let now = stamp(dir);
            if self.failed.get(name) == Some(&now) {
                continue;
            }
            match PackagePlugin::load_with(&self.runtime, dir, self.strings.clone()) {
                Ok(p) => {
                    self.failed.remove(name);
                    logs.push(format!("plugin {name} loaded from {}", dir.display()));
                    next.push(self.adopt(name, p));
                }
                Err(e) => {
                    self.failed.insert(name.clone(), now);
                    logs.push(format!("plugin package {} not loaded: {e}", dir.display()));
                }
            }
        }
        self.failed.retain(|name, _| present.contains(name));
        self.loaded.retain(|name, _| present.contains(name));
        for p in &running {
            if !present.contains(&p.manifest().id) {
                if let Some(s) = &self.strings {
                    s.remove_plugin(&p.manifest().id);
                }
                logs.push(format!(
                    "plugin {} unloaded: its folder is gone",
                    p.manifest().id
                ));
            }
        }
        let unchanged = next.len() == running.len()
            && next.iter().zip(&running).all(|(a, b)| Arc::ptr_eq(a, b));
        if !unchanged {
            for id in chain.replace_dynamic(next) {
                logs.push(format!(
                    "plugin {id} not loaded: a plugin with this id already exists"
                ));
            }
        }
        logs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("MESSAGE_*", "MESSAGE_CREATE"));
        assert!(!glob_match("MESSAGE_*", "TYPING_START"));
        assert!(glob_match(
            "POST /channels/*/messages",
            "POST /channels/12/messages"
        ));
        assert!(!glob_match(
            "POST /channels/*/messages",
            "POST /channels/12/pins"
        ));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", ""));
        assert!(glob_match("*a*b*", "xxaxxbxx"));
    }

    #[test]
    fn fetch_entries_are_hosts_or_one_label_patterns() {
        assert_eq!(
            valid_fetch_host(" API.example.com ").unwrap(),
            "api.example.com"
        );
        assert_eq!(
            valid_fetch_host("*.DiscordSays.com").unwrap(),
            "*.discordsays.com"
        );
        for bad in [
            "*.com",
            "*",
            "*.",
            "**.x.com",
            "a.*.com",
            "https://x.com",
            "x.com:443",
            "x.com/p",
            "*.*.x.com",
        ] {
            assert!(valid_fetch_host(bad).is_err(), "{bad}");
        }
        let toml = "id = \"p\"\nname = \"p\"\nversion = \"0.1.0\"\napi = 1\nfetch = [\"*.discordsays.com\"]\n";
        let p = PackageManifest::parse(toml).unwrap();
        assert_eq!(p.manifest.fetch, vec!["*.discordsays.com".to_string()]);
    }

    #[test]
    fn settings_come_from_plugin_toml_in_order() {
        let base = "id = \"p\"
name = \"p\"
version = \"0.1.0\"
api = 1
";
        let toml = format!(
            "{base}[[setting]]
key = \"mode\"
kind = \"choice\"
default = \"fast\"
             options = [\"fast\", \"exact\"]
label = \"setting-mode\"
             [[setting]]
key = \"autoTranslate\"
kind = \"bool\"
default = false
label = \"s-auto\"
             [[setting]]
key = \"ids\"
kind = \"list\"
default = [\"1\"]
label = \"s-ids\"
"
        );
        let s = PackageManifest::parse(&toml).unwrap().manifest.settings;
        assert_eq!(
            s,
            vec![
                SettingSpec::choice(
                    "mode",
                    vec![Val::Str("fast".into()), Val::Str("exact".into())],
                    Val::Str("fast".into()),
                    "setting-mode"
                ),
                SettingSpec::new(
                    "autoTranslate",
                    SettingKind::Bool,
                    Val::Bool(false),
                    "s-auto"
                ),
                SettingSpec::new(
                    "ids",
                    SettingKind::List,
                    Val::List(vec![Val::Str("1".into())]),
                    "s-ids"
                ),
            ]
        );
        for bad in [
            "[[setting]]
key = \"a\"
kind = \"slider\"
default = 1
label = \"x\"
",
            "[[setting]]
key = \"a\"
kind = \"bool\"
default = 1
label = \"x\"
",
            "[[setting]]
key = \"a\"
kind = \"choice\"
default = \"z\"
options = [\"y\"]
label = \"x\"
",
            "[[setting]]
key = \"a\"
kind = \"text\"
default = {}
label = \"x\"
",
            "[[setting]]
key = \"a\"
kind = \"bool\"
default = true
label = \"x\"
             [[setting]]
key = \"a\"
kind = \"bool\"
default = true
label = \"y\"
",
            "[[setting]]
key = \"a\"
kind = \"bool\"
default = true
label = \"x\"
hint = \"h\"
",
        ] {
            assert!(
                PackageManifest::parse(&format!("{base}{bad}")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn menu_entries_come_from_plugin_toml() {
        let base = "id = \"p\"\nname = \"p\"\nversion = \"0.1.0\"\napi = 1\n";
        let toml = format!(
            "{base}[[menu]]\nid = \"copy_raw\"\nlabel = \"menu-copy-raw\"\n\
             [[menu]]\nid = \"quote\"\nlabel = \"menu-quote\"\n"
        );
        let p = PackageManifest::parse(&toml).unwrap();
        assert_eq!(
            p.manifest.menu,
            vec![
                MenuEntry::new("copy_raw", "menu-copy-raw"),
                MenuEntry::new("quote", "menu-quote")
            ]
        );
        assert!(
            PackageManifest::parse(base)
                .unwrap()
                .manifest
                .menu
                .is_empty()
        );
        for bad in [
            "[[menu]]\nid = \"Copy\"\nlabel = \"x\"\n",
            "[[menu]]\nid = \"a\"\nlabel = \" \"\n",
            "[[menu]]\nid = \"a\"\nlabel = \"x\"\n[[menu]]\nid = \"a\"\nlabel = \"y\"\n",
            "[[menu]]\nid = \"a\"\nlabel = \"x\"\nicon = \"y\"\n",
        ] {
            assert!(
                PackageManifest::parse(&format!("{base}{bad}")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn manifest_validation() {
        let ok = r#"
            id = "clear_urls"
            name = "ClearURLs"
            version = "0.1.0"
            api = 1
            events = ["dispatch:MESSAGE_*", "send"]
        "#;
        let p = PackageManifest::parse(ok).unwrap();
        assert_eq!(p.manifest.layers, vec![Layer::Network]);
        assert!(p.events.matches(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
        assert!(!p.events.matches(&EventKind::Dispatch { name: "READY" }));
        assert!(p.events.matches(&EventKind::Send { op: 3 }));
        assert!(!p.events.matches(&EventKind::RestRequest {
            method: "GET",
            route: "/x"
        }));
        assert!(p.events.wants_gateway());

        let bad_api = ok.replace("api = 1", "api = 2");
        assert!(
            PackageManifest::parse(&bad_api)
                .unwrap_err()
                .contains("API v2")
        );
        let bad_id = ok.replace("clear_urls", "Clear-URLs");
        assert!(PackageManifest::parse(&bad_id).is_err());
        let bad_event = ok.replace("send", "sendd");
        assert!(PackageManifest::parse(&bad_event).is_err());
        let unknown_key = format!("{ok}\nsurprise = 1");
        assert!(PackageManifest::parse(&unknown_key).is_err());
        let ui_only = ok.replace(r#"["dispatch:MESSAGE_*", "send"]"#, r#"["ui:menu_action"]"#);
        let ui_only = PackageManifest::parse(&ui_only).unwrap();
        assert!(ui_only.events.matches(&EventKind::Ui {
            name: "menu_action"
        }));
        assert!(!ui_only.events.matches(&EventKind::Ui { name: "other" }));
        assert!(!ui_only.events.wants_gateway());
        let rest_only = ok.replace(r#"["dispatch:MESSAGE_*", "send"]"#, r#"["rest_request"]"#);
        assert!(
            !PackageManifest::parse(&rest_only)
                .unwrap()
                .events
                .wants_gateway()
        );
    }
}
