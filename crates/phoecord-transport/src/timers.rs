//! Plugin timers (`Host::schedule`): a store of pending timers shared by every [`ProxyHost`]
//! and the driver task that fires them.
//!
//! A due timer becomes a [`TIMER_EVENT`] dispatch for the plugin that scheduled it alone
//! ([`Engine::fire_timer`]); what the plugin queues there goes out at once. Whoever owns the clock
//! (the proxy's driver task, or the loader's own loop) asks [`Timers::next_due`] and
//! [`Timers::take_due`] and calls [`Engine::fire_timer`].
//!
//! [`ProxyHost`]: crate::host::ProxyHost
//! [`Engine::fire_timer`]: crate::Engine::fire_timer

use phoecord_core::TIMER_EVENT;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// Shortest delay: anything smaller is raised to this.
pub const MIN_DELAY: Duration = Duration::from_millis(50);
/// Longest delay: anything larger is refused.
pub const MAX_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
/// Most timers one plugin may have pending.
pub const MAX_PENDING: usize = 64;

struct Timer {
    plugin: String,
    tag: String,
    due: Instant,
    /// Order of scheduling, so timers due at the same instant fire in the order they were set.
    seq: u64,
}

#[derive(Default)]
struct Pending {
    timers: Vec<Timer>,
    seq: u64,
}

/// Pending timers of every plugin. Cheap to lock: a handful of entries per plugin at most.
#[derive(Default)]
pub struct Timers {
    pending: Mutex<Pending>,
    /// Wakes the driver when the earliest due time may have changed.
    wake: Notify,
}

impl Timers {
    /// Schedule a timer for `plugin`, due `after_ms` after `now`, with the caps of
    /// `Host::schedule`: the delay is raised to [`MIN_DELAY`], over [`MAX_DELAY`] is refused, and
    /// a plugin may have at most [`MAX_PENDING`] timers.
    pub fn schedule(
        &self,
        plugin: &str,
        after_ms: u64,
        tag: &str,
        now: Instant,
    ) -> Result<(), String> {
        if plugin.is_empty() {
            return Err("no plugin is running".into());
        }
        let delay = Duration::from_millis(after_ms);
        if delay > MAX_DELAY {
            return Err(format!(
                "a timer may be at most 24 h away ({after_ms} ms asked)"
            ));
        }
        let delay = delay.max(MIN_DELAY);
        let mut p = self.pending.lock().unwrap();
        if p.timers.iter().filter(|t| t.plugin == plugin).count() >= MAX_PENDING {
            return Err(format!(
                "too many timers: at most {MAX_PENDING} may be pending per plugin"
            ));
        }
        p.seq += 1;
        let seq = p.seq;
        p.timers.push(Timer {
            plugin: plugin.to_string(),
            tag: tag.to_string(),
            due: now + delay,
            seq,
        });
        drop(p);
        self.wake.notify_one();
        Ok(())
    }

    /// Cancel every pending timer of `plugin` with this `tag`.
    pub fn cancel(&self, plugin: &str, tag: &str) {
        let mut p = self.pending.lock().unwrap();
        let before = p.timers.len();
        p.timers.retain(|t| !(t.plugin == plugin && t.tag == tag));
        let changed = p.timers.len() != before;
        drop(p);
        if changed {
            self.wake.notify_one();
        }
    }

    /// How many timers `plugin` has pending.
    pub fn pending(&self, plugin: &str) -> usize {
        let p = self.pending.lock().unwrap();
        p.timers.iter().filter(|t| t.plugin == plugin).count()
    }

    /// Resolves once the set of timers changed (a timer was added or cancelled); a driver
    /// sleeping until [`Timers::next_due`] waits on it as well.
    pub fn changed(&self) -> tokio::sync::futures::Notified<'_> {
        self.wake.notified()
    }

    /// When the earliest timer is due.
    pub fn next_due(&self) -> Option<Instant> {
        let p = self.pending.lock().unwrap();
        p.timers.iter().map(|t| t.due).min()
    }

    /// Remove and return the timers due at `now` as `(plugin, tag)`, earliest first.
    pub fn take_due(&self, now: Instant) -> Vec<(String, String)> {
        let mut p = self.pending.lock().unwrap();
        let (mut due, rest): (Vec<Timer>, Vec<Timer>) = std::mem::take(&mut p.timers)
            .into_iter()
            .partition(|t| t.due <= now);
        p.timers = rest;
        due.sort_by_key(|t| (t.due, t.seq));
        due.into_iter().map(|t| (t.plugin, t.tag)).collect()
    }
}

/// The payload of a timer dispatch: `{"op":0,"t":"PHOECORD_TIMER","d":{"tag":<tag>}}`.
pub fn timer_payload(tag: &str) -> phoecord_core::Val {
    use phoecord_core::Val;
    let mut d = Val::map();
    d.set("tag", Val::Str(tag.to_string()));
    let mut v = Val::map();
    v.set("op", Val::Int(0));
    v.set("t", Val::Str(TIMER_EVENT.to_string()));
    v.set("d", d);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_per_plugin() {
        let t = Timers::default();
        let now = Instant::now();
        for i in 0..MAX_PENDING {
            t.schedule("p", 1000, &format!("t{i}"), now).unwrap();
        }
        let err = t.schedule("p", 1000, "one more", now).unwrap_err();
        assert!(err.contains("at most 64"), "{err}");
        t.schedule("q", 1000, "x", now)
            .expect("another plugin has its own cap");
        assert_eq!(t.pending("p"), 64);
        assert!(t.schedule("", 1000, "x", now).is_err());
    }

    #[test]
    fn short_delays_are_raised_and_long_ones_refused() {
        let t = Timers::default();
        let now = Instant::now();
        t.schedule("p", 0, "now", now).unwrap();
        assert_eq!(t.next_due(), Some(now + MIN_DELAY));
        assert!(t.take_due(now + Duration::from_millis(49)).is_empty());
        assert_eq!(
            t.take_due(now + MIN_DELAY),
            vec![("p".to_string(), "now".to_string())]
        );
        let day = 24 * 60 * 60 * 1000;
        t.schedule("p", day, "a day", now).unwrap();
        let err = t.schedule("p", day + 1, "too far", now).unwrap_err();
        assert!(err.contains("24 h"), "{err}");
        t.schedule("p", u64::MAX, "x", now).unwrap_err();
    }

    #[test]
    fn cancel_by_tag_and_due_order() {
        let t = Timers::default();
        let now = Instant::now();
        t.schedule("p", 300, "late", now).unwrap();
        t.schedule("p", 100, "tick", now).unwrap();
        t.schedule("p", 100, "tick", now).unwrap();
        t.schedule("q", 100, "tick", now).unwrap();
        t.schedule("p", 200, "mid", now).unwrap();
        t.cancel("p", "tick");
        assert_eq!(t.pending("p"), 2, "both of p's `tick` timers are gone");
        assert_eq!(t.pending("q"), 1, "q's timer with the same tag stays");
        let fired = t.take_due(now + Duration::from_secs(1));
        assert_eq!(
            fired,
            vec![
                ("q".to_string(), "tick".to_string()),
                ("p".to_string(), "mid".to_string()),
                ("p".to_string(), "late".to_string()),
            ]
        );
        assert_eq!(t.next_due(), None);
    }

    #[test]
    fn the_payload_carries_the_tag() {
        let v = timer_payload("hb");
        assert_eq!(v.str_at(&["t"]), Some(TIMER_EVENT));
        assert_eq!(v.str_at(&["d", "tag"]), Some("hb"));
        assert_eq!(v.get("op").and_then(phoecord_core::Val::as_i64), Some(0));
    }
}
