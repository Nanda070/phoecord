//! [`ProxyHost`]: the [`Host`] the proxy hands to plugins, plus the state it reads (account, KV
//! store, rate limits).

use crate::account::Account;
use crate::config::Config;
use crate::hostnet::{GLOBAL_PER_SECOND, Net};
use phoecord_core::{FilePart, Host, HostResponse, Val};
use phoecord_i18n::PluginStrings;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

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
        let q = self.hits.entry(key.to_string()).or_default();
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.window)
        {
            q.pop_front();
        }
        if q.len() >= self.max {
            return false;
        }
        q.push_back(now);
        true
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

/// State shared by every [`ProxyHost`]. Each lock is taken per call, never held across the chain.
pub struct Shared {
    pub account: Mutex<Account>,
    /// The client's Discord session and the HTTP client behind `Host::rest` / `Host::fetch`.
    pub net: Net,
    rest_global: Mutex<RateLimiter>,
    kv: Mutex<KvStore>,
    plugins_dir: PathBuf,
    /// `config.toml`, for `Host::set_plugin_enabled` (empty in tests that never toggle).
    pub config_path: PathBuf,
    config_lock: Mutex<()>,
    notify_limit: Mutex<RateLimiter>,
    rest_limit: Mutex<RateLimiter>,
    notifier: Notifier,
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
    pub fn new(plugins_dir: PathBuf) -> Shared {
        Shared::with_notifier(plugins_dir, Box::new(desktop_notify))
    }

    pub fn with_notifier(plugins_dir: PathBuf, notifier: Notifier) -> Shared {
        Shared {
            account: Mutex::new(Account::default()),
            net: Net::new(),
            rest_global: Mutex::new(RateLimiter::new(GLOBAL_PER_SECOND, Duration::from_secs(1))),
            kv: Mutex::new(KvStore::new(plugins_dir.clone())),
            plugins_dir,
            config_path: PathBuf::new(),
            config_lock: Mutex::new(()),
            // 5 notifications a minute per plugin.
            notify_limit: Mutex::new(RateLimiter::new(5, Duration::from_secs(60))),
            // 1 Discord API call a second per plugin.
            rest_limit: Mutex::new(RateLimiter::new(1, Duration::from_secs(1))),
            notifier,
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
        }
    }
}

impl Host for ProxyHost<'_> {
    fn begin_plugin(&mut self, id: &str) {
        self.plugin.clear();
        self.plugin.push_str(id);
        self.fetch_hosts.clear();
    }

    fn set_fetch_hosts(&mut self, hosts: &[String]) {
        self.fetch_hosts = hosts.to_vec();
    }

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

    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        Some(self.config.is_enabled(id))
    }

    fn set_plugin_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
        let path = &self.shared.config_path;
        if path.as_os_str().is_empty() {
            return Err("no config file".into());
        }
        let _guard = self.shared.config_lock.lock().unwrap();
        let mut cfg = Config::load(path).map_err(|e| format!("{e:#}"))?;
        cfg.set_enabled(id, on).map_err(|e| format!("{e:#}"))?;
        cfg.save(path).map_err(|e| format!("{e:#}"))
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
        let upstream = self.upstream()?;
        self.shared.net.rest(method, route, body, upstream)
    }

    fn rest_files(
        &mut self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
    ) -> Result<HostResponse, String> {
        self.rest_gate()?;
        let upstream = self.upstream()?;
        self.shared
            .net
            .rest_files(method, route, payload, files, upstream)
    }

    fn fetch(
        &mut self,
        method: &str,
        url: &str,
        body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let upstream = self.upstream()?;
        self.shared
            .net
            .fetch(&self.fetch_hosts, method, url, body, upstream)
    }
}

impl ProxyHost<'_> {
    /// Sign-in and the per-plugin and global caps shared by `rest` and `rest_files`.
    fn rest_gate(&mut self) -> Result<(), String> {
        if !self.shared.net.signed_in() {
            return Err("client not signed in yet".into());
        }
        let now = Instant::now();
        if !self
            .shared
            .rest_limit
            .lock()
            .unwrap()
            .allow(&self.plugin, now)
        {
            return Err("rate limited: at most 1 Discord API call per second per plugin".into());
        }
        if !self.shared.rest_global.lock().unwrap().allow("all", now) {
            return Err(format!(
                "rate limited: at most {GLOBAL_PER_SECOND} Discord API calls per second in total"
            ));
        }
        Ok(())
    }

    fn upstream(&self) -> Result<Option<crate::egress::Upstream>, String> {
        self.config
            .upstream
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(crate::egress::Upstream::parse)
            .transpose()
            .map_err(|e| format!("{e:#}"))
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
