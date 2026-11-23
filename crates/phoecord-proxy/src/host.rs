//! [`ProxyHost`]: the [`Host`] the proxy hands to plugins, plus the state it reads (account, KV
//! store, rate limits).

use crate::account::Account;
use crate::config::Config;
use phoecord_core::{Host, HostResponse, Val};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
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
    kv: Mutex<KvStore>,
    notify_limit: Mutex<RateLimiter>,
    rest_limit: Mutex<RateLimiter>,
    notifier: Notifier,
}

impl Shared {
    pub fn new(plugins_dir: PathBuf) -> Shared {
        Shared::with_notifier(plugins_dir, Box::new(desktop_notify))
    }

    pub fn with_notifier(plugins_dir: PathBuf, notifier: Notifier) -> Shared {
        Shared {
            account: Mutex::new(Account::default()),
            kv: Mutex::new(KvStore::new(plugins_dir)),
            // 5 notifications a minute per plugin.
            notify_limit: Mutex::new(RateLimiter::new(5, Duration::from_secs(60))),
            // 1 Discord API call a second per plugin.
            rest_limit: Mutex::new(RateLimiter::new(1, Duration::from_secs(1))),
            notifier,
        }
    }
}

/// The [`Host`] for one event: scoped to the plugin the chain announces through `begin_plugin`.
pub struct ProxyHost<'a> {
    shared: &'a Shared,
    config: &'a Config,
    plugin: String,
    /// Client → server payloads queued by plugins; the gateway sends them after the current frame.
    pub sends: Vec<(i64, Val)>,
}

impl<'a> ProxyHost<'a> {
    pub fn new(shared: &'a Shared, config: &'a Config) -> ProxyHost<'a> {
        ProxyHost {
            shared,
            config,
            plugin: String::new(),
            sends: Vec::new(),
        }
    }
}

impl Host for ProxyHost<'_> {
    fn begin_plugin(&mut self, id: &str) {
        self.plugin.clear();
        self.plugin.push_str(id);
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

    fn emit_send(&mut self, op: i64, payload: Val) {
        self.sends.push((op, payload));
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

    fn rest(
        &mut self,
        method: &str,
        route: &str,
        _body: Option<&Val>,
    ) -> Result<HostResponse, String> {
        let ok = self
            .shared
            .rest_limit
            .lock()
            .unwrap()
            .allow(&self.plugin, Instant::now());
        if !ok {
            return Err("rate limited: at most 1 Discord API call per second per plugin".into());
        }
        // The upstream call itself lands with the first plugin that needs it (stage 1.2).
        Err(format!("Host::rest {method} {route} is not wired up yet"))
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

    #[test]
    fn rest_is_rate_limited() {
        let shared = Shared::new(temp("rest"));
        let cfg = Config::default();
        let mut h = ProxyHost::new(&shared, &cfg);
        h.begin_plugin("p");
        let first = h.rest("GET", "/users/@me", None).unwrap_err();
        assert!(first.contains("not wired"), "{first}");
        let second = h.rest("GET", "/users/@me", None).unwrap_err();
        assert!(second.contains("rate limited"), "{second}");
    }
}
