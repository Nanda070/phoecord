//! UnsuppressEmbeds: show the embeds of a message again, or hide them (Vencord: UnsuppressEmbeds).
//!
//! Ported from Vencord `src/plugins/unsuppressEmbeds/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors rad, HypedDomi). Vencord adds "Unsuppress Embeds" /
//! "Suppress Embeds" to the message context menu and flips the `SUPPRESS_EMBEDS` flag (`1 << 2`)
//! with `PATCH /channels/{id}/messages/{id}`. The context menu waits for Phase 2; until then the
//! same toggle is `/phoecord unsuppress message:<id or link> [channel:<id>]`:
//!
//! 1. the message is loaded with `GET /channels/{id}/messages?limit=1&around={id}` (what the
//!    client uses to jump to a message) to read its `flags`;
//! 2. `PATCH` sends the flags with that bit flipped, keeping the others.
//!
//! Discord decides who may do it: your own messages, or others' with Manage Messages.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer, message_ref};
use phoecord_core::plugin::{API_VERSION, HostResponse};
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "unsuppress_embeds";
const SUB: &str = "unsuppress";
/// Message flag: the message's embeds are hidden.
const SUPPRESS_EMBEDS: i64 = 1 << 2;

pub struct UnsuppressEmbeds {
    manifest: Manifest,
    kept: Kept,
}

impl UnsuppressEmbeds {
    pub fn new() -> UnsuppressEmbeds {
        UnsuppressEmbeds {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "UnsuppressEmbeds".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Show the embeds of a message again, or hide them".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("UnsuppressEmbeds".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord unsuppress`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            SUB,
            "Show the embeds of a message again, or hide them",
            &self.kept,
        )
        .option("message", "Message id or link", true)
        .option(
            "channel",
            "Channel id (default: this channel or the link's)",
            false,
        )
        .boxed()
    }
}

impl Default for UnsuppressEmbeds {
    fn default() -> Self {
        Self::new()
    }
}

fn ok_body(r: Result<HostResponse, String>) -> Result<Val, String> {
    match r? {
        HostResponse {
            status: 200..=299,
            body,
            ..
        } => Ok(body.unwrap_or(Val::Null)),
        HostResponse { status, .. } => Err(format!("HTTP {status}")),
    }
}

/// Flip the flag on one message; the answer to show.
fn toggle(host: &mut dyn Host, channel: u64, message: u64) -> String {
    let page = ok_body(host.rest(
        "GET",
        &format!("/channels/{channel}/messages?limit=1&around={message}"),
        None,
    ));
    let page = match page {
        Ok(p) => p,
        Err(e) => return host.t("unsuppress_embeds-failed", &[("error", &e)]),
    };
    let Some(msg) = page
        .as_list()
        .iter()
        .find(|m| m.id_at(&["id"]) == Some(message))
    else {
        return host.t("unsuppress_embeds-not-found", &[]);
    };
    let flags = msg.get("flags").and_then(Val::as_i64).unwrap_or(0);
    let hidden = flags & SUPPRESS_EMBEDS != 0;
    let mut body = Val::map();
    body.set("flags", Val::Int(flags ^ SUPPRESS_EMBEDS));
    match ok_body(host.rest(
        "PATCH",
        &format!("/channels/{channel}/messages/{message}"),
        Some(&body),
    )) {
        Ok(_) if hidden => host.t("unsuppress_embeds-shown", &[]),
        Ok(_) => host.t("unsuppress_embeds-hidden", &[]),
        Err(e) => host.t("unsuppress_embeds-failed", &[("error", &e)]),
    }
}

impl Plugin for UnsuppressEmbeds {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_interaction(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && inv.sub == SUB
        {
            let given = inv.get("channel").map(crate::commands::snowflake_arg);
            let answer = match (inv.get("message").and_then(message_ref), given) {
                (None, _) => host.t("unsuppress_embeds-bad-message", &[]),
                (_, Some(None)) => host.t("unsuppress_embeds-bad-channel", &[]),
                (Some((linked, message)), given) => {
                    match given.flatten().or(linked).or(inv.channel_id) {
                        Some(channel) => toggle(host, channel, message),
                        None => host.t("unsuppress_embeds-bad-channel", &[]),
                    }
                }
            };
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    const AROUND: &str = "/channels/9/messages?limit=1&around=5";

    fn run(p: &UnsuppressEmbeds, h: &mut Rec, opts: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"unsuppress","options":[{opts}]}}]}}}}"#
        ));
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
    fn shows_hidden_embeds_and_keeps_other_flags() {
        let p = UnsuppressEmbeds::new();
        let mut h = Rec::default()
            .answer("GET", AROUND, 200, r#"[{"id":"5","flags":6}]"#)
            .answer("PATCH", "/channels/9/messages/5", 200, "{}");
        let out = run(&p, &mut h, r#"{"type":3,"name":"message","value":"5"}"#);
        assert_eq!(out, "Embeds are shown again.");
        assert_eq!(h.rest_calls[1].0, "PATCH /channels/9/messages/5");
        assert_eq!(
            h.rest_calls[1].1.as_ref().unwrap().get("flags"),
            Some(&Val::Int(2))
        );
    }

    #[test]
    fn hides_shown_embeds_in_the_linked_channel() {
        let p = UnsuppressEmbeds::new();
        let mut h = Rec::default()
            .answer(
                "GET",
                "/channels/8/messages?limit=1&around=5",
                200,
                r#"[{"id":"5"}]"#,
            )
            .answer("PATCH", "/channels/8/messages/5", 200, "{}");
        let out = run(
            &p,
            &mut h,
            r#"{"type":3,"name":"message","value":"https://discord.com/channels/1/8/5"}"#,
        );
        assert_eq!(out, "Embeds are hidden.");
        assert_eq!(
            h.rest_calls[1].1.as_ref().unwrap().get("flags"),
            Some(&Val::Int(4))
        );
    }

    #[test]
    fn explains_bad_input_and_failures() {
        let p = UnsuppressEmbeds::new();
        let mut h = Rec::default();
        let out = run(&p, &mut h, r#"{"type":3,"name":"message","value":"hello"}"#);
        assert!(out.contains("Not a message"), "{out}");
        let out = run(
            &p,
            &mut h,
            r#"{"type":3,"name":"message","value":"5"},{"type":3,"name":"channel","value":"x"}"#,
        );
        assert!(out.contains("Not a channel"), "{out}");
        assert!(h.rest_calls.is_empty());
        let mut h = Rec::default().answer("GET", AROUND, 200, r#"[{"id":"4"}]"#);
        let out = run(&p, &mut h, r#"{"type":3,"name":"message","value":"5"}"#);
        assert!(out.contains("not found"), "{out}");
        let mut h = Rec::default();
        let out = run(&p, &mut h, r#"{"type":3,"name":"message","value":"5"}"#);
        assert!(out.contains("HTTP 404"), "{out}");
    }
}
