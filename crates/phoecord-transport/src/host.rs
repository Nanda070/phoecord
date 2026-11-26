//! [`ProxyHost`]: the [`Host`] the engine hands to plugins, plus the state it reads (account, KV
//! store, rate limits). The only network-shaped part is the [`Network`] a transport supplies.

use crate::account::Account;
use crate::config::Config;
use crate::timers::Timers;
use phoecord_core::{ChainHost, FilePart, Host, HostResponse, Val};
use phoecord_i18n::PluginStrings;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// At most this many Discord API calls a second across all plugins.
pub const GLOBAL_PER_SECOND: usize = 5;

/// What a transport supplies for `Host::rest`, `Host::rest_files` and `Host::fetch`: the client's
/// own Discord session and the outgoing HTTPS. Everything about headers, TLS and the upstream
/// proxy stays behind it. `upstream` is the raw `upstream` setting of the config (`None` or empty
/// means connect directly); the implementation parses it and reports a bad value as an `Err`.
pub trait Network: Send + Sync {
    /// Whether the client's session is known, so `rest` can be made as the client.
    fn signed_in(&self) -> bool;

    /// A Discord API call as the client's own session.
    fn rest(
        &self,
        method: &str,
        route: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String>;

    /// A Discord API call with files (`multipart/form-data`).
    fn rest_files(
        &self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
        upstream: Option<&str>,
    ) -> Result<HostResponse, String>;

    /// HTTPS to an outside host of the plugin's `fetch` list (`allowed`), without credentials.
    fn fetch(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String>;

    /// [`Network::fetch`] with the caller's extra request headers.
    fn fetch_with(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String>;
}

/// A transport without network access: every call fails with a clear error.
pub struct NoNetwork;

const NO_NETWORK: &str = "this transport has no network access for plugins";

impl Network for NoNetwork {
    fn signed_in(&self) -> bool {
        false
    }
    fn rest(
        &self,
        _: &str,
        _: &str,
        _: Option<&Val>,
        _: Option<&str>,
    ) -> Result<HostResponse, String> {
        Err(NO_NETWORK.into())
    }
    fn rest_files(
        &self,
        _: &str,
        _: &str,
        _: Option<&Val>,
        _: &[FilePart],
        _: Option<&str>,
    ) -> Result<HostResponse, String> {
        Err(NO_NETWORK.into())
    }
    fn fetch(
        &self,
        _: &[String],
        _: &str,
        _: &str,
        _: Option<&Val>,
        _: Option<&str>,
    ) -> Result<HostResponse, String> {
        Err(NO_NETWORK.into())
    }
    fn fetch_with(
        &self,
        _: &[String],
        _: &str,
        _: &str,
        _: &[(&str, &str)],
        _: Option<&Val>,
        _: Option<&str>,
    ) -> Result<HostResponse, String> {
        Err(NO_NETWORK.into())
    }
}

/// At most `max` hits per key within `window`.
pub struct RateLimiter {
    max: usize,
    window: Duration,
    hits: HashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> RateLimiter {
        RateLimiter {
            max,
            window,
            hits: HashMap::new(),
        }
    }

    /// Record a hit for `key` at `now` and say whether it is within the limit.
    pub fn allow(&mut self, key: &str, now: Instant) -> bool {
        self.check(key, now).is_ok()
    }

    /// Record a hit for `key` at `now`, or refuse it with how long until the next hit fits.
    pub fn check(&mut self, key: &str, now: Instant) -> Result<(), Duration> {
        let q = self.hits.entry(key.to_string()).or_default();
        while q
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= self.window)
        {
            q.pop_front();
        }
        if q.len() >= self.max {
            let oldest = q.front().copied().unwrap_or(now);
            return Err(self
                .window
                .saturating_sub(now.saturating_duration_since(oldest)));
        }
        q.push_back(now);
        Ok(())
    }
}

/// Per-plugin key/value files: `data/plugins/<id>/kv.json`, cached in memory, written atomically.
pub struct KvStore {
    dir: PathBuf,
    cache: HashMap<String, serde_json::Map<String, serde_json::Value>>,
}

impl KvStore {
    pub fn new(plugins_dir: PathBuf) -> KvStore {
        KvStore {
            dir: plugins_dir,
            cache: HashMap::new(),
        }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(id).join("kv.json")
    }

    fn map(&mut self, id: &str) -> &mut serde_json::Map<String, serde_json::Value> {
        if !self.cache.contains_key(id) {
            let loaded = std::fs::read(self.file(id))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            self.cache.insert(id.to_string(), loaded);
        }
        self.cache.get_mut(id).unwrap()
    }

    /// A copy of every key of `id`.
    pub fn dump(&mut self, id: &str) -> serde_json::Map<String, serde_json::Value> {
        self.map(id).clone()
    }

    pub fn get(&mut self, id: &str, key: &str) -> Option<Val> {
        self.map(id).get(key).cloned().map(Val::from_json)
    }

    pub fn set(&mut self, id: &str, key: &str, value: &Val) -> std::io::Result<()> {
        self.map(id).insert(key.to_string(), value.to_json());
        let bytes = serde_json::to_vec_pretty(self.map(id))?;
        let path = self.file(id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)
    }
}

/// Desktop notifications (D-Bus on Linux, toasts on Windows, Notification Center on macOS).
pub fn desktop_notify(title: &str, body: &str) {
    tracing::info!("notify: {title}: {body}");
    if std::env::var_os("PHOECORD_NO_NOTIFY").is_some() {
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        if let Err(e) = notify_rust::Notification::new()
            .appname("PhoeCord")
            .summary(&title)
            .body(&body)
            .show()
        {
            tracing::warn!("notification failed: {e}");
        }
    });
}

type Notifier = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Discord API calls (`Host::rest`, `Host::rest_files`) one plugin may make per
/// [`PLUGIN_REST_WINDOW`]: the same 1 a second on average, with room for a short burst.
pub const PLUGIN_REST_BURST: usize = 5;
pub const PLUGIN_REST_WINDOW: Duration = Duration::from_secs(5);

/// State shared by every [`ProxyHost`]. Each lock is taken per call, never held across the chain.
pub struct Shared {
    pub account: Mutex<Account>,
    /// The client's Discord session and the HTTP client behind `Host::rest` / `Host::fetch`,
    /// supplied by the transport.
    pub net: Arc<dyn Network>,
    rest_global: Mutex<RateLimiter>,
    kv: Mutex<KvStore>,
    plugins_dir: PathBuf,
    /// `config.toml`, for `Host::set_plugin_enabled` (empty in tests that never toggle).
    pub config_path: PathBuf,
    config_lock: Mutex<()>,
    notify_limit: Mutex<RateLimiter>,
    rest_limit: Mutex<RateLimiter>,
    notifier: Notifier,
    /// Pending plugin timers (`Host::schedule`), fired by the proxy's timer driver.
    pub timers: Arc<Timers>,
    /// What the dev console is attached to (task 1.3.9): log lines, plugin runs, replayable events.
    pub dev: Arc<crate::dev::DevBus>,
    /// The plugins' own strings (`Host::t`): built-in bundles and the `i18n/` of WASM packages.
    pub strings: Arc<PluginStrings>,
    /// `--lang`, the first source of the language `Host::t` answers in.
    lang_flag: RwLock<Option<String>>,
    /// The operating system language, looked up once (the last source before English).
    os_lang: OnceLock<Option<String>>,
}

impl Shared {
    /// Set the `--lang` value. `language` in the config and the OS language follow it.
    pub fn set_language_flag(&self, flag: Option<String>) {
        *self.lang_flag.write().unwrap() = flag;
    }

    /// Fix the OS language instead of asking the system (tests).
    pub fn set_os_language(&self, lang: Option<String>) {
        let _ = self.os_lang.set(lang);
    }

    /// The language plugins' strings are shown in: `--lang`, config `language`, the OS, `en`.
    fn language(&self, config: &Config) -> String {
        let os = self.os_lang.get_or_init(phoecord_i18n::detect_os_language);
        phoecord_i18n::resolve_language(
            self.lang_flag.read().unwrap().as_deref(),
            config.language.as_deref(),
            os.as_deref(),
        )
        .lang
    }
}

impl Shared {
    /// Write `enabled` for plugin `id` into `config.toml` (what `Host::set_plugin_enabled` and the
    /// dev console do). The caller applies the new config.
    pub fn persist_enabled(&self, id: &str, on: bool) -> Result<(), String> {
        let path = &self.config_path;
        if path.as_os_str().is_empty() {
            return Err("no config file".into());
        }
        let _guard = self.config_lock.lock().unwrap();
        let mut cfg = Config::load(path).map_err(|e| format!("{e:#}"))?;
        cfg.set_enabled(id, on).map_err(|e| format!("{e:#}"))?;
        cfg.save(path).map_err(|e| format!("{e:#}"))
    }

    /// Everything in `id`'s `kv.json`.
    pub fn kv_dump(&self, id: &str) -> serde_json::Value {
        serde_json::Value::Object(self.kv.lock().unwrap().dump(id))
    }
}

impl Shared {
    pub fn new(plugins_dir: PathBuf) -> Shared {
        Shared::with_notifier(plugins_dir, Box::new(desktop_notify))
    }

    pub fn with_notifier(plugins_dir: PathBuf, notifier: Notifier) -> Shared {
        Shared::with_parts(plugins_dir, notifier, Arc::new(NoNetwork))
    }

    /// State with the transport's `net` behind `Host::rest` and `Host::fetch`.
    pub fn with_network(plugins_dir: PathBuf, net: Arc<dyn Network>) -> Shared {
        Shared::with_parts(plugins_dir, Box::new(desktop_notify), net)
    }

    fn with_parts(plugins_dir: PathBuf, notifier: Notifier, net: Arc<dyn Network>) -> Shared {
        Shared {
            account: Mutex::new(Account::default()),
            net,
            rest_global: Mutex::new(RateLimiter::new(GLOBAL_PER_SECOND, Duration::from_secs(1))),
            kv: Mutex::new(KvStore::new(plugins_dir.clone())),
            plugins_dir,
            config_path: PathBuf::new(),
            config_lock: Mutex::new(()),
            // 5 notifications a minute per plugin.
            notify_limit: Mutex::new(RateLimiter::new(5, Duration::from_secs(60))),
            // Discord API calls per plugin: 1 a second on average, in bursts of up to 5.
            rest_limit: Mutex::new(RateLimiter::new(PLUGIN_REST_BURST, PLUGIN_REST_WINDOW)),
            notifier,
            timers: Arc::new(Timers::default()),
            dev: Arc::new(crate::dev::DevBus::default()),
            strings: Arc::new(PluginStrings::new()),
            lang_flag: RwLock::new(None),
            os_lang: OnceLock::new(),
        }
    }
}

/// The [`Host`] for one event: scoped to the plugin the chain announces through `begin_plugin`.
pub struct ProxyHost<'a> {
    shared: &'a Shared,
    config: &'a Config,
    plugin: String,
    /// `fetch` hosts of the plugin being run (its manifest).
    fetch_hosts: Vec<String>,
    /// Client → server payloads queued by plugins; the gateway sends them after the current frame.
    pub sends: Vec<(i64, Val)>,
    /// Server → client payloads queued by plugins with `Host::emit_dispatch`.
    pub locals: Vec<Val>,
    /// Query string of the REST call being handled ([`Host::request_query`]).
    query: Option<String>,
}

impl<'a> ProxyHost<'a> {
    pub fn new(shared: &'a Shared, config: &'a Config) -> ProxyHost<'a> {
        ProxyHost {
            shared,
            config,
            plugin: String::new(),
            fetch_hosts: Vec::new(),
            sends: Vec::new(),
            locals: Vec::new(),
            query: None,
        }
    }

    /// The same host for one REST call with this query string (no leading `?`).
    pub fn with_query(mut self, query: Option<&str>) -> ProxyHost<'a> {
        self.query = query.filter(|q| !q.is_empty()).map(str::to_string);
        self
    }
}

impl ChainHost for ProxyHost<'_> {
    fn begin_plugin(&mut self, id: &str) {
        self.plugin.clear();
        self.plugin.push_str(id);
        self.fetch_hosts.clear();
    }

    fn set_fetch_hosts(&mut self, hosts: &[String]) {
        self.fetch_hosts = hosts.to_vec();
        self.fetch_hosts
            .extend(user_fetch_hosts(self.config, &self.plugin));
    }
}

/// Extra `fetch` hosts the user allowed for one plugin in `config.toml`
/// (`[plugins.<id>] fetch_hosts = ["lb.example.org"]`, task 2.4.2 c: a self-hosted ListenBrainz).
/// Only the user writes `config.toml`, never a plugin, so this widens nothing a plugin can choose.
/// Exact host names only: a pattern, a scheme, a port or a path is skipped.
pub fn user_fetch_hosts(config: &Config, plugin: &str) -> Vec<String> {
    let Some(list) = config
        .plugins
        .get(plugin)
        .and_then(|t| t.get("fetch_hosts"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| v.as_str())
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| is_plain_host(h))
        .collect()
}

/// `a.b` style DNS name: labels of `a-z`, `0-9` and `-`, at least two, none empty or starting or
/// ending with `-`.
fn is_plain_host(h: &str) -> bool {
    h.len() <= 253
        && h.contains('.')
        && h.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

impl Host for ProxyHost<'_> {
    fn setting(&self, key: &str) -> Option<Val> {
        let v = self.config.plugins.get(&self.plugin)?.get(key)?;
        Some(Val::from_json(serde_json::to_value(v).ok()?))
    }

    fn kv_get(&self, key: &str) -> Option<Val> {
        self.shared.kv.lock().unwrap().get(&self.plugin, key)
    }

    fn kv_set(&mut self, key: &str, value: Val) {
        if let Err(e) = self
            .shared
            .kv
            .lock()
            .unwrap()
            .set(&self.plugin, key, &value)
        {
            tracing::warn!(plugin = %self.plugin, "could not save kv.json: {e}");
        }
    }

    fn self_id(&self) -> Option<u64> {
        self.shared.account.lock().unwrap().user_id
    }

    fn request_query(&self) -> Option<String> {
        self.query.clone()
    }

    fn notify(&mut self, title: &str, body: &str) {
        let ok = self
            .shared
            .notify_limit
            .lock()
            .unwrap()
            .allow(&self.plugin, Instant::now());
        if ok {
            (self.shared.notifier)(title, body);
        } else {
            tracing::debug!(plugin = %self.plugin, "notification dropped: rate limit");
        }
    }

    fn log(&mut self, message: &str) {
        tracing::info!(plugin = %self.plugin, "{message}");
        self.shared.dev.log(&self.plugin, message);
    }

    fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
        let lang = self.shared.language(self.config);
        self.shared.strings.t(&self.plugin, &lang, key, args)
    }

    fn emit_send(&mut self, op: i64, payload: Val) {
        self.sends.push((op, payload));
    }

    fn emit_dispatch(&mut self, payload: Val) {
        self.locals.push(payload);
    }

    fn schedule(&mut self, after_ms: u64, tag: &str) -> Result<(), String> {
        self.shared
            .timers
            .schedule(&self.plugin, after_ms, tag, Instant::now())
    }

    fn cancel_schedule(&mut self, tag: &str) {
        self.shared.timers.cancel(&self.plugin, tag);
    }

    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        Some(self.config.is_enabled(id))
    }

    fn set_plugin_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
        self.shared.persist_enabled(id, on)
    }

    fn guild_emojis(&self, guild_id: u64) -> Vec<u64> {
        let a = self.shared.account.lock().unwrap();
        let mut v: Vec<u64> = a
            .guild_emojis
            .get(&guild_id)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        v.sort_unstable();
        v
    }

    fn channel_guild(&self, channel_id: u64) -> Option<u64> {
        self.shared
            .account
            .lock()
            .unwrap()
            .channel_guild
            .get(&channel_id)
            .copied()
    }

    fn append_data(&mut self, file: &str, line: &str) -> Result<(), String> {
        use std::io::Write;
        if self.plugin.is_empty() || file.is_empty() || file.contains(['/', '\\']) || file == ".." {
            return Err(format!("invalid data file name {file:?}"));
        }
        let path = self.shared.plugins_dir.join(&self.plugin).join(file);
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(f, "{line}")
        };
        write().map_err(|e| e.to_string())
    }

    fn rest(
        &mut self,
        method: &str,
        route: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        self.rest_gate()?;
        self.shared.net.rest(method, route, body, self.upstream())
    }

    fn rest_files(
        &mut self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
    ) -> Result<HostResponse, String> {
        self.rest_gate()?;
        self.shared
            .net
            .rest_files(method, route, payload, files, self.upstream())
    }

    fn fetch(
        &mut self,
        method: &str,
        url: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        self.shared
            .net
            .fetch(&self.fetch_hosts, method, url, body, self.upstream())
    }

    fn fetch_with(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        self.shared.net.fetch_with(
            &self.fetch_hosts,
            method,
            url,
            headers,
            body,
            self.upstream(),
        )
    }
}

impl ProxyHost<'_> {
    /// Sign-in and the per-plugin and global caps shared by `rest` and `rest_files`.
    fn rest_gate(&mut self) -> Result<(), String> {
        if !self.shared.net.signed_in() {
            return Err("client not signed in yet".into());
        }
        self.rest_limits(Instant::now())
    }

    /// The per-plugin and global caps at `now`. A refusal says how long to wait, like a 429.
    fn rest_limits(&mut self, now: Instant) -> Result<(), String> {
        let per_plugin = self
            .shared
            .rest_limit
            .lock()
            .unwrap()
            .check(&self.plugin, now);
        if let Err(wait) = per_plugin {
            return Err(format!(
                "rate limited: at most {PLUGIN_REST_BURST} Discord API calls per {} s per plugin, retry in {:.1}s",
                PLUGIN_REST_WINDOW.as_secs(),
                wait.as_secs_f32()
            ));
        }
        if let Err(wait) = self.shared.rest_global.lock().unwrap().check("all", now) {
            return Err(format!(
                "rate limited: at most {GLOBAL_PER_SECOND} Discord API calls per second in total, retry in {:.1}s",
                wait.as_secs_f32()
            ));
        }
        Ok(())
    }

    /// The `upstream` setting for the transport's [`Network`]; empty counts as unset.
    fn upstream(&self) -> Option<&str> {
        self.config
            .upstream
            .as_deref()
            .filter(|s| !s.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("phoecord-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_user_can_add_fetch_hosts_for_one_plugin_only() {
        let cfg: Config = toml::from_str(
            "[plugins.music_rich_presence]
fetch_hosts = [\"LB.example.org \", \"*.evil.com\",              \"https://x.org\", \"x.org:443\", \"nodot\", 5]
",
        )
        .unwrap();
        let shared = Shared::new(temp("fetchhosts"));
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("music_rich_presence");
        h.set_fetch_hosts(&["api.listenbrainz.org".into()]);
        assert_eq!(h.fetch_hosts, ["api.listenbrainz.org", "lb.example.org"]);
        h.begin_plugin("other");
        h.set_fetch_hosts(&[]);
        assert!(h.fetch_hosts.is_empty());
    }

    #[test]
    fn kv_round_trip_survives_a_restart() {
        let dir = temp("kv");
        let cfg = Config::default();
        let shared = Shared::new(dir.clone());
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("a");
        assert_eq!(h.kv_get("n"), None);
        h.kv_set("n", Val::Int(7));
        h.begin_plugin("b");
        assert_eq!(h.kv_get("n"), None, "plugins do not share a store");
        // A fresh Shared reads the file back.
        let shared2 = Shared::new(dir.clone());
        let mut h2 = ProxyHost::new(&shared2, &cfg);
        h2.begin_plugin("a");
        assert_eq!(h2.kv_get("n"), Some(Val::Int(7)));
        assert!(dir.join("a").join("kv.json").exists());
        assert!(!dir.join("a").join("kv.json.tmp").exists());
    }

    #[test]
    fn append_data_adds_lines_inside_the_plugin_dir() {
        let dir = temp("append");
        let shared = Shared::new(dir.clone());
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("p");
        h.append_data("log.jsonl", "{\"a\":1}").unwrap();
        h.append_data("log.jsonl", "{\"a\":2}").unwrap();
        let text = std::fs::read_to_string(dir.join("p").join("log.jsonl")).unwrap();
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            vec!["{\"a\":1}", "{\"a\":2}"]
        );
        assert!(h.append_data("../x", "l").is_err());
        assert!(h.append_data(r"a\b", "l").is_err());
    }

    #[test]
    fn plugins_can_be_switched_through_the_host() {
        let dir = temp("toggle");
        let cfg_path = dir.join("config.toml");
        Config::default().save(&cfg_path).unwrap();
        let mut shared = Shared::new(dir.clone());
        shared.config_path = cfg_path.clone();
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        assert_eq!(h.plugin_enabled("no_track"), Some(true));
        assert_eq!(h.plugin_enabled("message_logger"), Some(false));
        h.set_plugin_enabled("message_logger", true).unwrap();
        assert!(
            Config::load(&cfg_path)
                .unwrap()
                .is_enabled("message_logger")
        );
        assert!(h.set_plugin_enabled("../x", true).is_err());
        h.emit_dispatch(Val::map());
        assert_eq!(h.locals.len(), 1);
    }

    #[test]
    fn strings_come_from_the_plugin_bundle_in_the_configured_language() {
        let shared = Shared::new(temp("strings"));
        shared.set_os_language(Some("en".into()));
        shared
            .strings
            .set("p", "en", "hi = Hello, { $who }!\nonly-en = English");
        shared.strings.set("p", "ru", "hi = Привет, { $who }!");
        let ru: Config = toml::from_str("language = \"ru\"\n").unwrap();
        let en = Config::default();
        let mut h = ProxyHost::new(&shared, &ru);
        h.begin_plugin("p");
        assert_eq!(h.t("hi", &[("who", "Мир")]), "Привет, Мир!");
        assert_eq!(h.t("only-en", &[]), "English", "falls back to English");
        assert_eq!(h.t("nope", &[]), "nope", "then to the key");
        h.begin_plugin("q");
        assert_eq!(
            h.t("hi", &[]),
            "hi",
            "another plugin's bundle is not visible"
        );
        let mut h = ProxyHost::new(&shared, &en);
        h.begin_plugin("p");
        assert_eq!(h.t("hi", &[("who", "World")]), "Hello, World!");
        // `--lang` beats the config.
        shared.set_language_flag(Some("en".into()));
        let mut h = ProxyHost::new(&shared, &ru);
        h.begin_plugin("p");
        assert_eq!(h.t("hi", &[("who", "World")]), "Hello, World!");
    }

    #[test]
    fn rate_limiter_windows() {
        let mut r = RateLimiter::new(2, Duration::from_secs(60));
        let t = Instant::now();
        assert!(r.allow("a", t));
        assert!(r.allow("a", t));
        assert!(!r.allow("a", t));
        assert!(r.allow("b", t), "keys are independent");
        assert!(r.allow("a", t + Duration::from_secs(61)));
    }

    #[test]
    fn a_plugin_may_burst_five_discord_calls_and_keeps_one_a_second_on_average() {
        let shared = Shared::new(temp("rest-limit"));
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("p");
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        // Five quick calls (200 ms apart, like an edit then a delete) are fine.
        for i in 0..5 {
            h.rest_limits(ms(i * 200)).unwrap();
        }
        let err = h.rest_limits(ms(1000)).unwrap_err();
        assert!(
            err.starts_with(
                "rate limited: at most 5 Discord API calls per 5 s per plugin, retry in "
            ),
            "{err}"
        );
        assert!(err.ends_with("retry in 4.0s"), "{err}");
        assert!(h.rest_limits(ms(4999)).is_err());
        // The first call leaves the window after 5 s.
        h.rest_limits(ms(5000)).unwrap();
        // Over a long run the average stays at one call a second.
        let mut ok = 6;
        for i in 0..200u64 {
            if h.rest_limits(ms(5001 + i * 100)).is_ok() {
                ok += 1;
            }
        }
        // 25.1 s in total: at most 5 per started 5 s window.
        assert!(ok <= 30, "{ok} calls in about 25 s");
        // Another plugin has its own budget; the global cap still applies across plugins.
        let shared = Shared::new(temp("rest-global"));
        let mut h = ProxyHost::new(&shared, &cfg);
        for id in ["a", "b", "c", "d", "e"] {
            h.begin_plugin(id);
            h.rest_limits(t).unwrap();
        }
        h.begin_plugin("f");
        let err = h.rest_limits(t).unwrap_err();
        assert!(
            err.starts_with(
                "rate limited: at most 5 Discord API calls per second in total, retry in "
            ),
            "{err}"
        );
    }

    #[test]
    fn timers_are_scheduled_for_the_current_plugin() {
        let shared = Shared::new(temp("timers"));
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        assert!(h.schedule(100, "x").is_err(), "no plugin announced yet");
        h.begin_plugin("p");
        h.schedule(100, "x").unwrap();
        h.schedule(100, "x").unwrap();
        h.begin_plugin("q");
        h.schedule(100, "x").unwrap();
        h.cancel_schedule("x");
        assert_eq!(shared.timers.pending("p"), 2, "q cancels only its own");
        assert_eq!(shared.timers.pending("q"), 0);
        assert!(h.schedule(24 * 60 * 60 * 1000 + 1, "far").is_err());
    }

    #[test]
    fn notify_is_limited_to_five_a_minute_per_plugin() {
        let seen = Arc::new(Mutex::new(0usize));
        let s = seen.clone();
        let shared = Shared::with_notifier(
            temp("notify"),
            Box::new(move |_, _| *s.lock().unwrap() += 1),
        );
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("p");
        for _ in 0..8 {
            h.notify("t", "b");
        }
        assert_eq!(*seen.lock().unwrap(), 5);
        h.begin_plugin("q");
        h.notify("t", "b");
        assert_eq!(*seen.lock().unwrap(), 6);
    }

    #[test]
    fn settings_come_from_the_current_plugin_table() {
        let cfg: Config =
            toml::from_str("[plugins.fake_nitro]\nemoji_size = 64\nhyperlinks = true\n").unwrap();
        let shared = Shared::new(temp("settings"));
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("fake_nitro");
        assert_eq!(h.setting("emoji_size"), Some(Val::Int(64)));
        assert_eq!(h.setting("hyperlinks"), Some(Val::Bool(true)));
        assert_eq!(h.setting("missing"), None);
        h.begin_plugin("unknown_plugin");
        assert_eq!(h.setting("emoji_size"), None);
    }

    #[test]
    fn account_facts_and_send_queue() {
        let shared = Shared::new(temp("account"));
        let cfg = Config::default();
        shared.account.lock().unwrap().track(
            "READY",
            &Val::from_json(
                serde_json::from_str(
                    r#"{"user":{"id":"1","username":"me"},
                        "guilds":[{"id":"10","emojis":[{"id":"100"},{"id":"101"}],"channels":[{"id":"11"}]}]}"#,
                )
                .unwrap(),
            ),
        );
        let mut h = ProxyHost::new(&shared, &cfg);
        assert_eq!(h.self_id(), Some(1));
        assert_eq!(h.guild_emojis(10), vec![100, 101]);
        assert_eq!(h.guild_emojis(99), Vec::<u64>::new());
        assert_eq!(h.channel_guild(11), Some(10));
        assert_eq!(h.channel_guild(12), None);
        h.emit_send(3, Val::map());
        assert_eq!(h.sends.len(), 1);
    }
}
