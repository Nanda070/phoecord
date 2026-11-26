//! Plugin runtime: runs the plugin chain for every event, whatever a plugin is made of.
//!
//! Today it drives native plugins (`phoecord-plugins`). The WASM backend (wasmtime + a WIT world that
//! mirrors `phoecord_core::plugin`) plugs in behind the same [`Chain`] (docs/ROADMAP.md, milestone 1.3).

pub mod package;
pub mod wasm;
pub mod watch;

use phoecord_core::{ChainHost, Event, EventKind, Host, Manifest, Plugin, UiEvent, Val, Verdict};
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// One plugin's run of one event, handed to the chain's observer (the dev console).
pub struct RunRecord<'a> {
    pub plugin: &'a str,
    pub kind: &'a EventKind<'a>,
    /// `pass`, `changed`, `drop`, `reply <status>` or `panic`.
    pub verdict: std::borrow::Cow<'a, str>,
    pub elapsed: Duration,
}

/// Called after every plugin run while set with [`Chain::set_observer`]. Must be quick: it runs on
/// the traffic path.
pub type Observer = Arc<dyn Fn(&RunRecord<'_>) + Send + Sync>;

/// Plugins in the order they run. Order matters: the first `Drop` or `Reply` wins.
///
/// The chain has two segments: the plugins it was built with, and a *dynamic* segment after them
/// that [`Chain::replace_dynamic`] swaps while the chain is live (plugin packages that appear and
/// disappear on disk). An id is unique across both; a built-in always wins.
///
/// Only *enabled* plugins run (all of them until [`Chain::set_enabled`] is called). A plugin that
/// panics is disabled for the rest of the session and the chain carries on without it.
#[derive(Default)]
pub struct Chain {
    plugins: Vec<Arc<dyn Plugin>>,
    dynamic: RwLock<Vec<Arc<dyn Plugin>>>,
    /// `None` = every plugin is enabled.
    enabled: RwLock<Option<HashSet<String>>>,
    crashed: RwLock<HashSet<String>>,
    /// Told about every plugin run (the dev console); `None` costs one lock read per event.
    observer: RwLock<Option<Observer>>,
}

/// What the chain decided, folded over all plugins.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Untouched,
    Changed,
    Dropped {
        by: String,
    },
    Replied {
        by: String,
        status: u16,
        body: Option<Val>,
    },
}

impl Chain {
    pub fn new(plugins: Vec<Box<dyn Plugin>>) -> Chain {
        Chain {
            plugins: plugins.into_iter().map(Arc::from).collect(),
            ..Chain::default()
        }
    }

    /// Every plugin, in run order: the built-in segment, then the dynamic one.
    fn all(&self) -> Vec<Arc<dyn Plugin>> {
        let mut all = self.plugins.clone();
        all.extend(self.dynamic.read().unwrap().iter().cloned());
        all
    }

    /// The plugins of the dynamic segment, for a caller that wants to keep the ones that stay.
    pub fn dynamic_plugins(&self) -> Vec<Arc<dyn Plugin>> {
        self.dynamic.read().unwrap().clone()
    }

    /// Replace the dynamic segment (plugins that run after the built-in ones). A plugin whose id
    /// is already taken by a built-in or by an earlier one in `plugins` is left out; the ids left
    /// out are returned. Events already running finish with the plugins they started with.
    pub fn replace_dynamic(&self, plugins: Vec<Arc<dyn Plugin>>) -> Vec<String> {
        let mut taken: HashSet<String> = self
            .plugins
            .iter()
            .map(|p| p.manifest().id.clone())
            .collect();
        let mut kept = Vec::new();
        let mut skipped = Vec::new();
        for p in plugins {
            let id = p.manifest().id.clone();
            if taken.insert(id.clone()) {
                kept.push(p);
            } else {
                skipped.push(id);
            }
        }
        *self.dynamic.write().unwrap() = kept;
        skipped
    }

    /// Watch every plugin run (kind, verdict, time taken). `None` stops watching.
    pub fn set_observer(&self, observer: Option<Observer>) {
        *self.observer.write().unwrap() = observer;
    }

    /// Whether `id` is in the dynamic segment (a WASM package) and not built in.
    pub fn is_dynamic(&self, id: &str) -> bool {
        self.dynamic
            .read()
            .unwrap()
            .iter()
            .any(|p| p.manifest().id == id)
    }

    /// Put `plugin` in the place of the dynamic plugin with the same id. `false` when there is none.
    /// The new code gets a fresh start: if the old one was disabled by a panic, the id runs again.
    pub fn swap_dynamic(&self, plugin: Arc<dyn Plugin>) -> bool {
        let mut dynamic = self.dynamic.write().unwrap();
        let id = plugin.manifest().id.clone();
        match dynamic.iter_mut().find(|p| p.manifest().id == id) {
            Some(slot) => {
                *slot = plugin;
                self.crashed.write().unwrap().remove(&id);
                true
            }
            None => false,
        }
    }

    /// Whether `id` was disabled for the session by a panic.
    pub fn is_crashed(&self, id: &str) -> bool {
        self.crashed.read().unwrap().contains(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.all().iter().map(|p| p.manifest().id.clone()).collect()
    }

    pub fn manifests(&self) -> Vec<Manifest> {
        self.all().iter().map(|p| p.manifest().clone()).collect()
    }

    /// Replace the set of enabled plugin ids (called on every config reload).
    pub fn set_enabled(&self, ids: impl IntoIterator<Item = String>) {
        *self.enabled.write().unwrap() = Some(ids.into_iter().collect());
    }

    /// Ids that currently run, in chain order.
    pub fn enabled_ids(&self) -> Vec<String> {
        self.active()
            .iter()
            .map(|p| p.manifest().id.clone())
            .collect()
    }

    /// Ids of plugins disabled for the session after a panic.
    pub fn crashed_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.crashed.read().unwrap().iter().cloned().collect();
        v.sort();
        v
    }

    fn active(&self) -> Vec<Arc<dyn Plugin>> {
        let enabled = self.enabled.read().unwrap().clone();
        let crashed = self.crashed.read().unwrap().clone();
        let mut all = self.all();
        all.retain(|p| {
            let id = &p.manifest().id;
            !crashed.contains(id) && enabled.as_ref().is_none_or(|e| e.contains(id))
        });
        all
    }

    /// Whether any enabled plugin wants this kind of event (lets the proxy skip decoding).
    pub fn wants(&self, kind: &EventKind) -> bool {
        self.active().iter().any(|p| p.wants(kind))
    }

    /// Whether any enabled plugin may want gateway events at all.
    pub fn wants_gateway(&self) -> bool {
        self.active().iter().any(|p| p.wants_gateway())
    }

    /// Whether the plugin with this id may want gateway events (enabled or not).
    pub fn uses_gateway(&self, id: &str) -> bool {
        self.all()
            .iter()
            .any(|p| p.manifest().id == id && p.wants_gateway())
    }

    /// Run one event through every interested plugin. `call` builds the [`Event`] for each plugin
    /// (re-borrowing the possibly already changed payload) and hands it to `Plugin::on_event`.
    pub fn run(
        &self,
        host: &mut dyn ChainHost,
        kind: &EventKind,
        call: impl FnMut(&dyn Plugin, &mut dyn Host) -> Verdict,
    ) -> Outcome {
        self.run_on(&self.active(), host, kind, call)
    }

    /// [`Chain::run`] over `plugins` (already filtered to enabled ones): the `wants` check,
    /// `begin_plugin`, panic isolation and the verdict fold.
    fn run_on(
        &self,
        plugins: &[Arc<dyn Plugin>],
        host: &mut dyn ChainHost,
        kind: &EventKind,
        mut call: impl FnMut(&dyn Plugin, &mut dyn Host) -> Verdict,
    ) -> Outcome {
        let mut changed = false;
        let observer = self.observer.read().unwrap().clone();
        for p in plugins.iter().map(Arc::as_ref).filter(|p| p.wants(kind)) {
            host.begin_plugin(&p.manifest().id);
            host.set_fetch_hosts(&p.manifest().fetch);
            let started = observer.as_ref().map(|_| Instant::now());
            let result = catch_unwind(AssertUnwindSafe(|| call(p, &mut *host)));
            if let (Some(obs), Some(started)) = (&observer, started) {
                let verdict = match &result {
                    Ok(Verdict::Pass) => "pass".into(),
                    Ok(Verdict::Changed) => "changed".into(),
                    Ok(Verdict::Drop) => "drop".into(),
                    Ok(Verdict::Reply { status, .. }) => format!("reply {status}").into(),
                    // A verdict this host does not know counts as `Pass`.
                    Ok(_) => "pass".into(),
                    Err(_) => "panic".into(),
                };
                obs(&RunRecord {
                    plugin: &p.manifest().id,
                    kind,
                    verdict,
                    elapsed: started.elapsed(),
                });
            }
            let verdict = match result {
                Ok(v) => v,
                Err(_) => {
                    let id = p.manifest().id.clone();
                    host.log(&format!(
                        "plugin {id} panicked and is disabled for this session"
                    ));
                    self.crashed.write().unwrap().insert(id);
                    continue;
                }
            };
            match verdict {
                Verdict::Pass => {}
                Verdict::Changed => changed = true,
                Verdict::Drop => {
                    return Outcome::Dropped {
                        by: p.manifest().id.clone(),
                    };
                }
                Verdict::Reply { status, body } => {
                    return Outcome::Replied {
                        by: p.manifest().id.clone(),
                        status,
                        body,
                    };
                }
                // A verdict this host does not know counts as `Pass`.
                _ => {}
            }
        }
        if changed {
            Outcome::Changed
        } else {
            Outcome::Untouched
        }
    }

    /// A REST request through the chain.
    pub fn rest_request(
        &self,
        host: &mut dyn ChainHost,
        method: &str,
        route: &str,
        mut body: Option<&mut Val>,
    ) -> Outcome {
        self.run(host, &EventKind::RestRequest { method, route }, |p, h| {
            p.on_event(
                h,
                Event::RestRequest {
                    method,
                    route,
                    body: body.as_deref_mut(),
                },
            )
        })
    }

    /// A REST answer through the chain (plugins may rewrite the body).
    pub fn rest_response(
        &self,
        host: &mut dyn ChainHost,
        method: &str,
        route: &str,
        status: u16,
        body: &mut Val,
    ) -> Outcome {
        self.run(host, &EventKind::RestResponse { method, route }, |p, h| {
            p.on_event(
                h,
                Event::RestResponse {
                    method,
                    route,
                    status,
                    body: &mut *body,
                },
            )
        })
    }

    /// A client → server gateway payload through the chain.
    pub fn send(&self, host: &mut dyn ChainHost, op: i64, payload: &mut Val) -> Outcome {
        self.run(host, &EventKind::Send { op }, |p, h| {
            p.on_event(h, Event::Send { op, payload })
        })
    }

    /// A gateway dispatch through the chain. Payloads plugins add end up in `extra`.
    pub fn dispatch(
        &self,
        host: &mut dyn ChainHost,
        name: &str,
        payload: &mut Val,
        extra: &mut Vec<Val>,
    ) -> Outcome {
        self.run(host, &EventKind::Dispatch { name }, |p, h| {
            p.on_event(
                h,
                Event::Dispatch {
                    name,
                    payload,
                    extra,
                },
            )
        })
    }

    /// One event for one plugin only, with the same rules as the whole chain: the plugin must be
    /// enabled, not crashed and want the event; `begin_plugin` scopes the host to it and a panic
    /// disables it. `None` when the plugin did not run (unknown, off, crashed or not interested).
    pub fn run_to(
        &self,
        plugin_id: &str,
        host: &mut dyn ChainHost,
        kind: &EventKind,
        call: impl FnMut(&dyn Plugin, &mut dyn Host) -> Verdict,
    ) -> Option<Outcome> {
        let target: Vec<Arc<dyn Plugin>> = self
            .active()
            .into_iter()
            .filter(|p| p.manifest().id == plugin_id && p.wants(kind))
            .collect();
        if target.is_empty() {
            return None;
        }
        Some(self.run_on(&target, host, kind, call))
    }

    /// A gateway dispatch for one plugin only (a timer it scheduled, see
    /// [`phoecord_core::TIMER_EVENT`]); see [`Chain::run_to`].
    pub fn dispatch_to(
        &self,
        host: &mut dyn ChainHost,
        plugin_id: &str,
        name: &str,
        payload: &mut Val,
        extra: &mut Vec<Val>,
    ) -> Option<Outcome> {
        self.run_to(plugin_id, host, &EventKind::Dispatch { name }, |p, h| {
            p.on_event(
                h,
                Event::Dispatch {
                    name,
                    payload,
                    extra,
                },
            )
        })
    }

    /// A UI event from a loader hook through the chain (read-only: every verdict counts as `Pass`,
    /// so no plugin can stop it from reaching the next). [`UiEvent::MenuAction`] is never broadcast:
    /// it goes only to the plugin that declared the entry, through [`Chain::ui_to`].
    pub fn ui(&self, host: &mut dyn ChainHost, event: UiEvent<'_>) -> Outcome {
        if matches!(event, UiEvent::MenuAction { .. }) {
            return Outcome::Untouched;
        }
        self.run(host, &EventKind::Ui { name: event.name() }, |p, h| {
            p.on_event(h, Event::Ui(event));
            Verdict::Pass
        })
    }

    /// A UI event for one plugin only, such as [`UiEvent::MenuAction`] for the plugin that
    /// declared the entry; see [`Chain::run_to`]. Read-only: the verdict counts as `Pass`.
    pub fn ui_to(
        &self,
        host: &mut dyn ChainHost,
        plugin_id: &str,
        event: UiEvent<'_>,
    ) -> Option<Outcome> {
        self.run_to(
            plugin_id,
            host,
            &EventKind::Ui { name: event.name() },
            |p, h| {
                p.on_event(h, Event::Ui(event));
                Verdict::Pass
            },
        )
    }
}

/// A [`Host`] that remembers nothing, for tests and the self-test.
#[derive(Default)]
pub struct NullHost {
    pub logs: Vec<String>,
    /// Frames passed to `emit_dispatch`.
    pub dispatched: Vec<Val>,
    /// Known plugin ids and whether they are on (`set_plugin_enabled` changes known ones only).
    pub plugins: std::collections::HashMap<String, bool>,
    /// `(file, line)` pairs passed to `append_data`.
    pub data: Vec<(String, String)>,
    /// `(after_ms, tag)` pairs passed to `schedule` (`cancel_schedule` removes a tag's entries).
    pub scheduled: Vec<(u64, String)>,
}

impl Host for NullHost {
    fn setting(&self, _key: &str) -> Option<Val> {
        None
    }
    fn kv_get(&self, _key: &str) -> Option<Val> {
        None
    }
    fn kv_set(&mut self, _key: &str, _value: Val) {}
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, title: &str, body: &str) {
        self.logs.push(format!("notify: {title}: {body}"));
    }
    fn log(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }
    fn emit_send(&mut self, _op: i64, _payload: Val) {}
    fn guild_emojis(&self, _guild_id: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _channel_id: u64) -> Option<u64> {
        None
    }
    fn emit_dispatch(&mut self, payload: Val) {
        self.dispatched.push(payload);
    }
    fn plugin_enabled(&self, id: &str) -> Option<bool> {
        self.plugins.get(id).copied()
    }
    fn set_plugin_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
        match self.plugins.get_mut(id) {
            Some(slot) => {
                *slot = on;
                Ok(())
            }
            None => Err(format!("unknown plugin {id}")),
        }
    }
    fn append_data(&mut self, file: &str, line: &str) -> Result<(), String> {
        self.data.push((file.into(), line.into()));
        Ok(())
    }
    fn schedule(&mut self, after_ms: u64, tag: &str) -> Result<(), String> {
        self.scheduled.push((after_ms, tag.into()));
        Ok(())
    }
    fn cancel_schedule(&mut self, tag: &str) {
        self.scheduled.retain(|(_, t)| t != tag);
    }
}

impl ChainHost for NullHost {}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Manifest, Verdict);

    fn manifest(id: &str) -> Manifest {
        Manifest::new(id, id, "0", String::new())
    }

    fn fixed(id: &str, v: Verdict) -> Box<dyn Plugin> {
        Box::new(Fixed(manifest(id), v))
    }

    impl Plugin for Fixed {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
            self.1.clone()
        }
    }

    struct Panics(Manifest);

    impl Plugin for Panics {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
            panic!("boom")
        }
    }

    #[test]
    fn first_stop_wins_and_changes_fold() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            fixed("a", Verdict::Changed),
            fixed("b", Verdict::Drop),
            fixed(
                "c",
                Verdict::Reply {
                    status: 204,
                    body: None,
                },
            ),
        ]);
        let mut p = Val::map();
        let mut extra = vec![];
        assert_eq!(
            chain.dispatch(&mut h, "MESSAGE_CREATE", &mut p, &mut extra),
            Outcome::Dropped { by: "b".into() }
        );
        let chain = Chain::new(vec![
            fixed("a", Verdict::Changed),
            fixed("b", Verdict::Pass),
        ]);
        assert_eq!(
            chain.rest_request(&mut h, "GET", "/x", None),
            Outcome::Changed
        );
    }

    /// Logs the menu actions it gets and wants only UI events.
    struct Menu(Manifest);

    impl Plugin for Menu {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, k: &EventKind) -> bool {
            matches!(
                k,
                EventKind::Ui {
                    name: "menu_action"
                }
            )
        }
        fn on_event(&self, h: &mut dyn Host, e: Event<'_>) -> Verdict {
            if let Event::Ui(UiEvent::MenuAction {
                entry, message_id, ..
            }) = e
            {
                h.log(&format!("{} {entry} {message_id}", self.0.id));
            }
            Verdict::Pass
        }
    }

    #[test]
    fn ui_events_reach_only_the_plugins_that_want_them() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            Box::new(Menu(manifest("a"))) as Box<dyn Plugin>,
            Box::new(Menu(manifest("b"))),
            fixed("all", Verdict::Pass),
        ]);
        chain.set_enabled(["a".to_string(), "b".to_string(), "all".to_string()]);
        let click = UiEvent::MenuAction {
            entry: "copy_raw",
            channel_id: 1,
            message_id: 2,
        };
        assert_eq!(chain.ui_to(&mut h, "b", click), Some(Outcome::Untouched));
        assert_eq!(h.logs, vec!["b copy_raw 2".to_string()]);
        // A menu click is never broadcast: only the declaring plugin gets it, through `ui_to`.
        assert_eq!(chain.ui(&mut h, click), Outcome::Untouched);
        assert_eq!(h.logs.len(), 1);
        assert_eq!(chain.ui_to(&mut h, "nobody", click), None);
    }

    #[test]
    fn ui_events_are_read_only() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            fixed("drop", Verdict::Drop),
            fixed(
                "reply",
                Verdict::Reply {
                    status: 204,
                    body: None,
                },
            ),
            fixed("changed", Verdict::Changed),
        ]);
        chain.set_enabled(["drop", "reply", "changed"].map(String::from));
        let click = UiEvent::MenuAction {
            entry: "copy_raw",
            channel_id: 1,
            message_id: 2,
        };
        for id in ["drop", "reply", "changed"] {
            assert_eq!(
                chain.ui_to(&mut h, id, click),
                Some(Outcome::Untouched),
                "{id}"
            );
        }
    }

    /// Counts its events and wants only the timer dispatch.
    struct Timed(Manifest, std::sync::atomic::AtomicUsize);

    impl Plugin for Timed {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, k: &EventKind) -> bool {
            matches!(k, EventKind::Dispatch { name } if *name == phoecord_core::TIMER_EVENT)
        }
        fn on_event(&self, h: &mut dyn Host, _: Event<'_>) -> Verdict {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            h.log(&format!("ran {}", self.0.id));
            Verdict::Changed
        }
    }

    #[test]
    fn dispatch_to_reaches_only_the_target_and_skips_a_disabled_one() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let a = Arc::new(Timed(manifest("a"), AtomicUsize::new(0)));
        let b = Arc::new(Timed(manifest("b"), AtomicUsize::new(0)));
        let chain = Chain::default();
        chain.replace_dynamic(vec![
            a.clone(),
            b.clone(),
            Arc::from(fixed("c", Verdict::Drop)),
        ]);
        let mut h = NullHost::default();
        let (mut p, mut extra) = (Val::map(), Vec::new());
        let t = phoecord_core::TIMER_EVENT;
        assert_eq!(
            chain.dispatch_to(&mut h, "a", t, &mut p, &mut extra),
            Some(Outcome::Changed)
        );
        assert_eq!(a.1.load(Ordering::Relaxed), 1);
        assert_eq!(
            b.1.load(Ordering::Relaxed),
            0,
            "the other plugin never sees it"
        );
        assert_eq!(h.logs, vec!["ran a".to_string()]);
        // "c" would drop it, but it is not the target.
        chain.set_enabled(["b".to_string(), "c".to_string()]);
        assert_eq!(chain.dispatch_to(&mut h, "a", t, &mut p, &mut extra), None);
        assert_eq!(
            a.1.load(Ordering::Relaxed),
            1,
            "a disabled plugin is skipped"
        );
        assert_eq!(
            chain.dispatch_to(&mut h, "b", "OTHER", &mut p, &mut extra),
            None,
            "an event the plugin does not want"
        );
        assert_eq!(
            chain.dispatch_to(&mut h, "nobody", t, &mut p, &mut extra),
            None
        );
    }

    #[test]
    fn the_observer_hears_every_run_with_its_verdict() {
        use std::sync::Mutex;
        let chain = Chain::new(vec![
            fixed("a", Verdict::Changed),
            fixed("b", Verdict::Drop),
            fixed("c", Verdict::Pass),
        ]);
        let seen: Arc<Mutex<Vec<(String, String, String)>>> = Arc::default();
        let sink = seen.clone();
        chain.set_observer(Some(Arc::new(move |r: &RunRecord<'_>| {
            sink.lock().unwrap().push((
                r.plugin.to_string(),
                format!("{:?}", r.kind),
                r.verdict.to_string(),
            ));
        })));
        let mut h = NullHost::default();
        chain.rest_request(&mut h, "GET", "/x", None);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen.iter()
                .map(|(p, _, v)| (p.as_str(), v.as_str()))
                .collect::<Vec<_>>(),
            vec![("a", "changed"), ("b", "drop")],
            "the plugin after a Drop never runs"
        );
        assert!(seen[0].1.contains("RestRequest"));
        chain.set_observer(None);
    }

    #[test]
    fn dispatch_to_isolates_a_panic() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![Box::new(Panics(manifest("bad")))]);
        let (mut p, mut extra) = (Val::map(), Vec::new());
        assert_eq!(
            chain.dispatch_to(&mut h, "bad", "X", &mut p, &mut extra),
            Some(Outcome::Untouched)
        );
        assert_eq!(chain.crashed_ids(), vec!["bad".to_string()]);
        assert_eq!(
            chain.dispatch_to(&mut h, "bad", "X", &mut p, &mut extra),
            None
        );
    }

    #[test]
    fn swapping_in_new_code_lifts_the_crash() {
        let mut h = NullHost::default();
        let chain = Chain::default();
        chain.replace_dynamic(vec![Arc::new(Panics(manifest("pkg")))]);
        let (mut p, mut extra) = (Val::map(), Vec::new());
        chain.dispatch_to(&mut h, "pkg", "X", &mut p, &mut extra);
        assert!(chain.is_crashed("pkg"));
        assert!(chain.swap_dynamic(Arc::from(fixed("pkg", Verdict::Changed))));
        assert!(!chain.is_crashed("pkg"), "the fixed code runs again");
        assert_eq!(
            chain.dispatch_to(&mut h, "pkg", "X", &mut p, &mut extra),
            Some(Outcome::Changed)
        );
    }

    #[test]
    fn disabled_and_panicking_plugins_do_not_run() {
        let mut h = NullHost::default();
        let chain = Chain::new(vec![
            Box::new(Panics(manifest("bad"))),
            fixed("a", Verdict::Changed),
            fixed("off", Verdict::Drop),
        ]);
        chain.set_enabled(["bad".to_string(), "a".to_string()]);
        let mut p = Val::map();
        assert_eq!(chain.send(&mut h, 3, &mut p), Outcome::Changed);
        assert_eq!(chain.crashed_ids(), vec!["bad".to_string()]);
        assert_eq!(chain.enabled_ids(), vec!["a".to_string()]);
        // The crashed plugin stays out for the rest of the session.
        assert_eq!(
            chain.rest_response(&mut h, "GET", "/x", 200, &mut Val::Null),
            Outcome::Changed
        );
        assert_eq!(chain.crashed_ids().len(), 1);
    }
}
