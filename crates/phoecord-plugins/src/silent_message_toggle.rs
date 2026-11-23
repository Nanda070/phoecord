//! SilentMessageToggle: send messages without notifying anyone (Vencord: SilentMessageToggle).
//!
//! Ported from Vencord `src/plugins/silentMessageToggle/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; authors Nuckyz, CatNoir). Vencord's chat bar button flips a
//! state, and the pre-send listener prefixes `@silent ` to the text, which the client turns into
//! the message flag `SUPPRESS_NOTIFICATIONS`. Here the flag itself is set on the wire:
//! `flags |= 1 << 12` on `POST /channels/{id}/messages`. Edits and other routes are untouched, and
//! so is a message that already has the flag.
//!
//! Switching it on:
//! - `[plugins.silent_message_toggle] enabled = true`: every message is silent.
//! - `/phoecord silent [on|off]` (no argument flips it): the next message(s) are silent. This is
//!   the toggle Vencord's button has. It lives in memory and starts off, like Vencord's default
//!   (`persist_state = false`); `/phoecord toggle silent_message_toggle` switches the plugin.
//!
//! Setting `auto_disable` (default `true`, as Vencord): after one message is sent silently through
//! the command's toggle it switches itself off. It does not apply to `enabled = true`.
//!
//! Not ported: the chat bar button and its icons (Phase 2), `persist_state` (the toggle is global,
//! not per channel, so there is nothing to persist across channel changes).

use crate::commands::{Args, Subcommand};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Message flag `SUPPRESS_NOTIFICATIONS`.
const SUPPRESS_NOTIFICATIONS: i64 = 1 << 12;
const PLUGIN_ID: &str = "silent_message_toggle";

pub struct SilentMessageToggle {
    manifest: Manifest,
    /// Set by `/phoecord silent`.
    armed: Arc<AtomicBool>,
}

impl SilentMessageToggle {
    pub fn new() -> SilentMessageToggle {
        SilentMessageToggle {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "SilentMessageToggle".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Send messages without notifications".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("SilentMessageToggle".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
            },
            armed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The `/phoecord silent` command, sharing this plugin's toggle. Register it with
    /// `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        Box::new(SilentCmd {
            armed: self.armed.clone(),
        })
    }
}

impl Default for SilentMessageToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/messages`.
fn is_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "POST" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

/// Sets the flag; `false` if the message already had it.
fn add_flag(body: &mut Val) -> bool {
    let flags = body.get("flags").and_then(Val::as_i64).unwrap_or(0);
    if flags & SUPPRESS_NOTIFICATIONS != 0 {
        return false;
    }
    body.set("flags", Val::Int(flags | SUPPRESS_NOTIFICATIONS));
    true
}

impl Plugin for SilentMessageToggle {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_send(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
        else {
            return Verdict::Pass;
        };
        if !is_send(method, route) {
            return Verdict::Pass;
        }
        let always = host.setting("enabled").and_then(|v| v.as_bool()) == Some(true);
        let armed = self.armed.load(Ordering::Relaxed);
        if !always && !armed {
            return Verdict::Pass;
        }
        // Vencord disables the toggle when the message is sent, whatever it contains.
        if armed && host.setting("auto_disable").and_then(|v| v.as_bool()) != Some(false) {
            self.armed.store(false, Ordering::Relaxed);
        }
        if add_flag(body) {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

/// `/phoecord silent [state]`.
struct SilentCmd {
    armed: Arc<AtomicBool>,
}

impl Subcommand for SilentCmd {
    fn name(&self) -> &str {
        "silent"
    }
    fn description(&self) -> &str {
        "Send your next message without notifications"
    }
    fn options(&self) -> Vec<crate::commands::CommandOption> {
        vec![crate::commands::CommandOption {
            name: "state",
            description: "on or off; leave empty to flip",
            required: false,
        }]
    }
    fn run(&self, host: &mut dyn Host, args: &Args) -> String {
        if host.plugin_enabled(PLUGIN_ID) == Some(false) {
            return format!(
                "The `{PLUGIN_ID}` plugin is off. Switch it on with `/phoecord toggle {PLUGIN_ID}`."
            );
        }
        let now = self.armed.load(Ordering::Relaxed);
        let want = match args
            .get("state")
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("") => !now,
            Some("on" | "true" | "1" | "yes") => true,
            Some("off" | "false" | "0" | "no") => false,
            Some(other) => return format!("Unknown state `{other}`. Use `on` or `off`."),
        };
        self.armed.store(want, Ordering::Relaxed);
        if want {
            "Silent message is **on**: your next message sends without a notification.".into()
        } else {
            "Silent message is **off**.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn send(chain: &Chain, h: &mut Cfg, json: &str) -> (Outcome, Val) {
        let mut body = j(json);
        let out = chain.rest_request(h, "POST", "/channels/9/messages", Some(&mut body));
        (out, body)
    }

    fn args(state: Option<&str>) -> Args {
        Args {
            options: state
                .map(|s| vec![("state".to_string(), s.to_string())])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn matching_rules() {
        assert!(is_send("POST", "/channels/9/messages"));
        assert!(!is_send("PATCH", "/channels/9/messages/1"));
        assert!(!is_send("GET", "/channels/9/messages"));
        assert!(!is_send("POST", "/channels/9/typing"));
        let p = SilentMessageToggle::new();
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/channels/9/messages"
        }));
    }

    #[test]
    fn flag_is_added_without_losing_others() {
        let mut b = j(r#"{"content":"hi"}"#);
        assert!(add_flag(&mut b));
        assert_eq!(b.at(&["flags"]).and_then(Val::as_i64), Some(4096));
        let mut b = j(r#"{"flags":2}"#);
        assert!(add_flag(&mut b));
        assert_eq!(b.at(&["flags"]).and_then(Val::as_i64), Some(4098));
        assert!(!add_flag(&mut b));
    }

    #[test]
    fn always_on_setting_marks_every_message() {
        let chain = Chain::new(vec![Box::new(SilentMessageToggle::new())]);
        let mut h = Cfg::default().with("enabled", Val::Bool(true));
        for _ in 0..2 {
            let (out, body) = send(&chain, &mut h, r#"{"content":"hi"}"#);
            assert_eq!(out, Outcome::Changed);
            assert_eq!(body.at(&["flags"]).and_then(Val::as_i64), Some(4096));
        }
        let mut edit = j(r#"{"content":"x"}"#);
        let out = chain.rest_request(&mut h, "PATCH", "/channels/9/messages/1", Some(&mut edit));
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn command_arms_the_next_message_and_auto_disables() {
        let plugin = SilentMessageToggle::new();
        let cmd = plugin.command();
        let chain = Chain::new(vec![Box::new(plugin)]);
        let mut h = Cfg::default();
        assert_eq!(
            send(&chain, &mut h, r#"{"content":"a"}"#).0,
            Outcome::Untouched
        );
        assert!(cmd.run(&mut h, &args(None)).contains("**on**"));
        assert_eq!(
            send(&chain, &mut h, r#"{"content":"b"}"#).0,
            Outcome::Changed
        );
        // auto_disable: the next one is normal again.
        assert_eq!(
            send(&chain, &mut h, r#"{"content":"c"}"#).0,
            Outcome::Untouched
        );
        // auto_disable = false keeps it armed until switched off.
        let mut keep = Cfg::default().with("auto_disable", Val::Bool(false));
        cmd.run(&mut keep, &args(Some("on")));
        assert_eq!(
            send(&chain, &mut keep, r#"{"content":"d"}"#).0,
            Outcome::Changed
        );
        assert_eq!(
            send(&chain, &mut keep, r#"{"content":"e"}"#).0,
            Outcome::Changed
        );
        assert!(cmd.run(&mut keep, &args(Some("off"))).contains("**off**"));
        assert_eq!(
            send(&chain, &mut keep, r#"{"content":"f"}"#).0,
            Outcome::Untouched
        );
        assert!(cmd.run(&mut keep, &args(Some("maybe"))).contains("Unknown"));
    }

    #[test]
    fn command_refuses_while_the_plugin_is_off() {
        let cmd = SilentMessageToggle::new().command();
        let mut h = Cfg::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args(None)).contains("is off"));
    }
}
