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
//! events = ["dispatch:MESSAGE_CREATE"]
//! rights = ["notify"]          # which `Host` methods it may call (see `wasm::Right`)
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
//! A [`PackagePlugin`] watches its two files and swaps in a changed package between events.

use crate::wasm::{Setup, WasmPlugin, WasmRuntime};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Verdict};
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
        Ok(PackageManifest {
            manifest: Manifest {
                id: raw.id,
                name: raw.name,
                version: raw.version,
                description: raw.description,
                authors: raw.authors,
                vencord: raw.vencord,
                layers,
                api: raw.api,
            },
            events,
            rights: raw.rights,
        })
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    toml: Option<(SystemTime, u64)>,
    wasm: Option<(SystemTime, u64)>,
}

fn stamp(dir: &Path) -> Stamp {
    let one = |name: &str| {
        let m = std::fs::metadata(dir.join(name)).ok()?;
        Some((m.modified().ok()?, m.len()))
    };
    Stamp {
        toml: one("plugin.toml"),
        wasm: one("plugin.wasm"),
    }
}

fn build(
    runtime: &Arc<WasmRuntime>,
    dir: &Path,
    expect_id: Option<&str>,
) -> Result<WasmPlugin, String> {
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
    WasmPlugin::new(runtime, &wasm, Setup::from_package(package)?)
        .map_err(|e| format!("plugin.wasm: {e:#}"))
}

/// A plugin loaded from a package folder that follows the files on disk.
pub struct PackagePlugin {
    runtime: Arc<WasmRuntime>,
    dir: PathBuf,
    /// The manifest of the first load. The id never changes; other fields (name, version, …) show
    /// the first load until PhoeCord restarts, because [`Plugin::manifest`] hands out a reference.
    manifest: Manifest,
    current: RwLock<WasmPlugin>,
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
        let seen = stamp(dir);
        let plugin = build(runtime, dir, None)?;
        Ok(PackagePlugin {
            runtime: runtime.clone(),
            dir: dir.to_path_buf(),
            manifest: plugin.manifest().clone(),
            current: RwLock::new(plugin),
            watch: Mutex::new(Watch {
                seen,
                checked: Instant::now(),
                every: Duration::from_secs(1),
            }),
            problems: Mutex::new(Vec::new()),
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

    /// Swap in the package if its files changed. A broken new version leaves the old one running.
    fn refresh(&self) {
        {
            let mut w = self.watch.lock().unwrap();
            if w.checked.elapsed() < w.every {
                return;
            }
            w.checked = Instant::now();
            let now = stamp(&self.dir);
            if now == w.seen {
                return;
            }
            w.seen = now;
        }
        match build(&self.runtime, &self.dir, Some(&self.manifest.id)) {
            Ok(new) => {
                *self.current.write().unwrap() = new;
                self.problems
                    .lock()
                    .unwrap()
                    .push(format!("plugin {} reloaded", self.manifest.id));
            }
            Err(e) => self.problems.lock().unwrap().push(format!(
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
}

impl PackageSet {
    pub fn new(runtime: Arc<WasmRuntime>, root: PathBuf) -> PackageSet {
        PackageSet {
            runtime,
            root,
            failed: HashMap::new(),
        }
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
                continue;
            }
            let now = stamp(dir);
            if self.failed.get(name) == Some(&now) {
                continue;
            }
            match PackagePlugin::load(&self.runtime, dir) {
                Ok(p) => {
                    self.failed.remove(name);
                    logs.push(format!("plugin {name} loaded from {}", dir.display()));
                    next.push(Arc::new(p));
                }
                Err(e) => {
                    self.failed.insert(name.clone(), now);
                    logs.push(format!("plugin package {} not loaded: {e}", dir.display()));
                }
            }
        }
        self.failed.retain(|name, _| present.contains(name));
        for p in &running {
            if !present.contains(&p.manifest().id) {
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
        let rest_only = ok.replace(r#"["dispatch:MESSAGE_*", "send"]"#, r#"["rest_request"]"#);
        assert!(
            !PackageManifest::parse(&rest_only)
                .unwrap()
                .events
                .wants_gateway()
        );
    }
}
