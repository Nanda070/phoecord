//! CustomIdle: change or switch off the time before Discord shows you as idle (Vencord:
//! CustomIdle).
//!
//! Ported from Vencord `src/plugins/customIdle/index.ts` (GPL-3.0-or-later, Copyright (c) 2024
//! Vendicated and contributors; author newwares). Vencord patches the client's 10-minute idle
//! timer: `idleTimeout` minutes (default 10, `0` = never idle) and `remainInIdle` (default on:
//! coming back keeps you idle until you press "Exit idle" on a notice).
//!
//! The proxy does not see your mouse, only the presence the client sends. When the client goes
//! idle on its own it sends a presence update (`op 3`) with `afk: true` and `since` (ms). So:
//!
//! - `idleTimeout = 0`: that presence goes out as your active one (`afk: false`, `since: 0`, the
//!   status you had before);
//! - `idleTimeout` above 10: the active presence goes out now, and the idle one is sent after
//!   the extra minutes with a plugin timer, unless you come back first;
//! - `idleTimeout` up to 10: the client's own 10 minutes stay (a shorter time needs the input
//!   the proxy cannot see);
//! - `remainInIdle`: once you were shown idle, the client's "I am back" presence is kept idle and
//!   a notification says to run `/phoecord exitidle`, which sends your active presence.
//!
//! Settings (`[plugins.custom_idle]`, Vencord's names): `idleTimeout` (minutes), `remainInIdle`.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer};
use crate::presence::{self, PRESENCE};
use crate::state::{Shared, dispatch_data};
use phoecord_core::plugin::TIMER_EVENT;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "custom_idle";
const TAG: &str = "idle";
/// The client's own idle time, minutes.
const CLIENT_MINUTES: f64 = 10.0;

#[derive(Default)]
struct State {
    /// Your last active presence (`afk: false`), what `exitidle` sends.
    active: Option<Val>,
    /// The client's idle presence, waiting for the timer.
    held: Option<Val>,
    /// The next idle presence is the held one coming back from the timer: let it through.
    releasing: bool,
    /// Others see you idle (the client's idle presence went out).
    shown_idle: Option<Val>,
    /// Already told to run `/phoecord exitidle` for this idle spell.
    noticed: bool,
}

pub struct CustomIdle {
    manifest: Manifest,
    kept: Kept,
    state: Shared<State>,
}

impl CustomIdle {
    pub fn new() -> CustomIdle {
        CustomIdle {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "CustomIdle",
                    env!("CARGO_PKG_VERSION"),
                    "Change or switch off the time before you show as idle",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("CustomIdle".into());
                m
            },
            kept: Kept::default(),
            state: Shared::default(),
        }
    }

    /// `/phoecord exitidle`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            "exitidle",
            "Exit idle and show as online again",
            &self.kept,
        )
        .boxed()
    }
}

impl Default for CustomIdle {
    fn default() -> Self {
        Self::new()
    }
}

fn minutes(host: &dyn Host) -> f64 {
    match host.setting("idleTimeout") {
        Some(Val::Float(f)) => f.max(0.0),
        Some(v) => v.as_i64().map_or(CLIENT_MINUTES, |n| n.max(0) as f64),
        None => CLIENT_MINUTES,
    }
}

fn remain(host: &dyn Host) -> bool {
    host.setting("remainInIdle")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn is_afk(p: &Val) -> bool {
    p.get("afk").and_then(Val::as_bool) == Some(true)
}

/// `idle` as the active presence: your status before, `afk: false`, `since: 0`.
fn as_active(idle: &mut Val, active: Option<&Val>) {
    let status = active
        .and_then(|a| a.str_at(&["status"]))
        .filter(|s| *s != "idle")
        .unwrap_or("online")
        .to_string();
    idle.set("status", Val::Str(status));
    idle.set("afk", Val::Bool(false));
    idle.set("since", Val::Int(0));
}

/// `back` as idle again, with the `since` others already saw.
fn as_idle(back: &mut Val, shown: &Val) {
    back.set("status", Val::Str("idle".into()));
    back.set("afk", Val::Bool(true));
    back.set("since", shown.get("since").cloned().unwrap_or(Val::Int(0)));
}

impl CustomIdle {
    fn on_presence(&self, host: &mut dyn Host, p: &mut Val) -> bool {
        let mut st = self.state.lock();
        if is_afk(p) {
            if std::mem::take(&mut st.releasing) {
                st.shown_idle = Some(p.clone());
                return false;
            }
            let minutes = minutes(host);
            if minutes == 0.0 {
                as_active(p, st.active.as_ref());
                return true;
            }
            if minutes <= CLIENT_MINUTES {
                st.shown_idle = Some(p.clone());
                return false;
            }
            let extra = ((minutes - CLIENT_MINUTES) * 60_000.0) as u64;
            if host.schedule(extra, TAG).is_err() {
                st.shown_idle = Some(p.clone());
                return false;
            }
            st.held = Some(p.clone());
            as_active(p, st.active.as_ref());
            return true;
        }
        // Active again.
        if st.held.take().is_some() {
            host.cancel_schedule(TAG);
        }
        st.active = Some(p.clone());
        match st.shown_idle.clone() {
            Some(shown) if remain(host) => {
                as_idle(p, &shown);
                if !std::mem::replace(&mut st.noticed, true) {
                    host.notify(
                        &host.t("custom_idle-back-title", &[]),
                        &host.t("custom_idle-back-body", &[]),
                    );
                }
                true
            }
            _ => {
                st.shown_idle = None;
                st.noticed = false;
                false
            }
        }
    }

    fn exit_idle(&self, host: &mut dyn Host) -> String {
        let mut st = self.state.lock();
        if st.shown_idle.take().is_none() {
            return host.t("custom_idle-not-idle", &[]);
        }
        st.noticed = false;
        let mut active = st.active.clone().unwrap_or_else(|| {
            let mut p = Val::map();
            p.set("activities", Val::List(Vec::new()));
            p
        });
        as_active(&mut active, st.active.as_ref());
        drop(st);
        host.emit_send(PRESENCE, presence::update_frame(active));
        host.t("custom_idle-online", &[])
    }
}

impl Plugin for CustomIdle {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Send { op } => *op == PRESENCE,
            EventKind::Dispatch { name } => *name == TIMER_EVENT,
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if op == PRESENCE => {
                let changed =
                    presence::presence_mut(payload).is_some_and(|p| self.on_presence(host, p));
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::Dispatch { name, payload, .. } if name == TIMER_EVENT => {
                if dispatch_data(payload).str_at(&["tag"]) == Some(TAG) {
                    let mut st = self.state.lock();
                    if let Some(idle) = st.held.take() {
                        st.releasing = true;
                        drop(st);
                        host.emit_send(PRESENCE, presence::update_frame(idle));
                    }
                }
                Verdict::Pass
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if let Some(inv) = invocation(body)
                    && inv.sub == "exitidle"
                {
                    let answer = self.exit_idle(host);
                    *self.kept.lock() = Some(answer);
                }
                Verdict::Pass
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    const ACTIVE: &str = r#"{"op":3,"d":{"status":"dnd","since":0,"afk":false,"activities":[]}}"#;
    const IDLE: &str =
        r#"{"op":3,"d":{"status":"idle","since":1700000000000,"afk":true,"activities":[]}}"#;

    fn send(p: &CustomIdle, h: &mut Rec, frame: &str) -> (Verdict, Val) {
        let mut v = j(frame);
        let out = p.on_event(
            h,
            Event::Send {
                op: 3,
                payload: &mut v,
            },
        );
        (out, v)
    }

    fn tick(p: &CustomIdle, h: &mut Rec) {
        let mut f = Rec::timer_frame(TAG);
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut extra,
            },
        );
    }

    fn exit(p: &CustomIdle, h: &mut Rec) -> String {
        let mut body = j(
            r#"{"type":2,"channel_id":"9","data":{"name":"phoecord","options":[{"type":1,"name":"exitidle"}]}}"#,
        );
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        p.command().run(h, &Args { options: vec![] })
    }

    #[test]
    fn zero_never_shows_idle() {
        let p = CustomIdle::new();
        let mut h = Rec::default().with("idleTimeout", Val::Int(0));
        send(&p, &mut h, ACTIVE);
        let (v, f) = send(&p, &mut h, IDLE);
        assert_eq!(v, Verdict::Changed);
        assert_eq!(f.str_at(&["d", "status"]), Some("dnd"));
        assert_eq!(f.at(&["d", "afk"]), Some(&Val::Bool(false)));
        assert_eq!(f.at(&["d", "since"]), Some(&Val::Int(0)));
    }

    #[test]
    fn a_longer_timeout_holds_the_idle_presence_until_the_timer() {
        let p = CustomIdle::new();
        let mut h = Rec::default()
            .with("idleTimeout", Val::Int(30))
            .with("remainInIdle", Val::Bool(false));
        send(&p, &mut h, ACTIVE);
        let (v, f) = send(&p, &mut h, IDLE);
        assert_eq!(v, Verdict::Changed);
        assert_eq!(f.str_at(&["d", "status"]), Some("dnd"));
        assert_eq!(h.scheduled, vec![(20 * 60_000, TAG.to_string())]);
        tick(&p, &mut h);
        let (op, sent) = h.sent.pop().unwrap();
        assert_eq!(op, 3);
        assert_eq!(sent.str_at(&["d", "status"]), Some("idle"));
        // The released idle presence comes back through the chain and passes.
        let mut again = sent.clone();
        assert_eq!(
            p.on_event(
                &mut h,
                Event::Send {
                    op: 3,
                    payload: &mut again
                }
            ),
            Verdict::Pass
        );
        // Back before the timer: cancelled, nothing sent later.
        send(&p, &mut h, ACTIVE);
        send(&p, &mut h, IDLE);
        send(&p, &mut h, ACTIVE);
        assert_eq!(h.cancelled, vec![TAG.to_string()]);
        tick(&p, &mut h);
        assert!(h.sent.is_empty());
    }

    #[test]
    fn remain_in_idle_until_exitidle() {
        let p = CustomIdle::new();
        let mut h = Rec::default();
        assert_eq!(exit(&p, &mut h), "You are not idle.");
        send(&p, &mut h, ACTIVE);
        assert_eq!(
            send(&p, &mut h, IDLE).0,
            Verdict::Pass,
            "10 minutes: the client's own"
        );
        let (v, f) = send(&p, &mut h, ACTIVE);
        assert_eq!(v, Verdict::Changed);
        assert_eq!(f.str_at(&["d", "status"]), Some("idle"));
        assert_eq!(f.at(&["d", "since"]), Some(&Val::Int(1_700_000_000_000)));
        assert_eq!(h.notes.len(), 1);
        send(&p, &mut h, ACTIVE);
        assert_eq!(h.notes.len(), 1, "told once");
        assert_eq!(exit(&p, &mut h), "You are online again.");
        let (_, sent) = h.sent.pop().unwrap();
        assert_eq!(sent.str_at(&["d", "status"]), Some("dnd"));
        let mut back = sent.clone();
        assert_eq!(
            p.on_event(
                &mut h,
                Event::Send {
                    op: 3,
                    payload: &mut back
                }
            ),
            Verdict::Pass
        );
    }
}
