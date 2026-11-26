//! WhoReacted: who reacted to a message (Vencord: WhoReacted).
//!
//! Ported from Vencord `src/plugins/whoReacted/index.tsx` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors Ven, KannaDev, newwares, paige). Vencord fetches the
//! reactors of every reaction on screen with `GET /channels/{c}/messages/{m}/reactions/{emoji}`
//! (`limit=100`) and draws their avatars next to the count. This is the data part, on demand:
//!
//! - `/phoecord reactions <channel_id> <message_id>` reads the message and lists its reactions
//!   with their counts (one API call);
//! - `/phoecord reactions <channel_id> <message_id> <emoji>` lists up to 100 people who reacted
//!   with that emoji (one API call). `<emoji>` is the emoji itself (`👍`), a custom emoji as
//!   pasted (`<:name:123>` or `<a:name:123>`) or as `name:123`.
//!
//! Why not track `MESSAGE_REACTION_ADD`/`REMOVE` instead: the gateway only tells what changes
//! while PhoeCord is running, never who reacted before, so the list would be wrong for almost
//! every message. The REST lookup is complete. The cost is the host's limit of one API call per
//! second per plugin, which is why one command makes one call (it would otherwise need a sleep).
//!
//! Switching it on: `[plugins.who_reacted] enabled = true`, or `/phoecord toggle who_reacted`.
//!
//! Not ported: the avatars under each reaction, the `clickableAvatars` setting (Phase 2 UI hooks).

use crate::commands::{FnCmd, Subcommand};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "who_reacted";
/// Names shown for one emoji (Discord returns at most 100 per call).
const SHOW: usize = 40;
/// Reactions listed for one message.
const MAX_EMOJIS: usize = 30;

pub struct WhoReacted {
    manifest: Manifest,
}

impl WhoReacted {
    pub fn new() -> WhoReacted {
        WhoReacted {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "WhoReacted",
                    env!("CARGO_PKG_VERSION"),
                    "Lists who reacted to a message",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("WhoReacted".into());
                m
            },
        }
    }

    /// The `/phoecord reactions` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        FnCmd::new(
            PLUGIN_ID,
            "reactions",
            "List the reactions of a message, or who reacted with one emoji",
            |host, args| {
                let (Some(channel), Some(message)) = (
                    args.get("channel_id").and_then(digits),
                    args.get("message_id").and_then(digits),
                ) else {
                    return "Give a numeric channel id and message id.".into();
                };
                match args.get("emoji").map(str::trim).filter(|e| !e.is_empty()) {
                    None => list_reactions(host, channel, message),
                    Some(emoji) => list_reactors(host, channel, message, emoji),
                }
            },
        )
        .option("channel_id", "Channel id", true)
        .option("message_id", "Message id", true)
        .option(
            "emoji",
            "Emoji to list the reactors of; empty lists the reactions",
            false,
        )
        .boxed()
    }
}

impl Default for WhoReacted {
    fn default() -> Self {
        Self::new()
    }
}

/// An id made only of digits (ids are not parsed to numbers so any width works).
fn digits(s: &str) -> Option<&str> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// The `name` or `name:id` key Discord's reactions route wants, from what a user typed.
fn emoji_key(input: &str) -> String {
    let t = input.trim();
    match t.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        Some(inner) => inner
            .trim_start_matches('a')
            .trim_start_matches(':')
            .to_string(),
        None => t.trim_start_matches(':').to_string(),
    }
}

/// Percent-encode everything but unreserved characters (a `:` stays, Discord wants `name:id`).
fn encode(key: &str) -> String {
    let mut out = String::new();
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// How an emoji is shown and typed back: unicode as is, custom as `<:name:id>`.
fn emoji_label(e: &Val) -> String {
    let name = e.str_at(&["name"]).unwrap_or("?");
    match e.id_at(&["id"]) {
        Some(id) => format!("<:{name}:{id}>"),
        None => name.to_string(),
    }
}

fn get(host: &mut dyn Host, route: &str) -> Result<Val, String> {
    let r = host.rest("GET", route, None)?;
    match (r.status, r.body) {
        (200, Some(b)) => Ok(b),
        (403, _) => Err("Discord answered 403: you cannot read that channel or message".into()),
        (404, _) => Err("Discord answered 404: no such message (check the ids)".into()),
        (s, _) => Err(format!("Discord answered {s}")),
    }
}

fn list_reactions(host: &mut dyn Host, channel: &str, message: &str) -> String {
    let msg = match get(host, &format!("/channels/{channel}/messages/{message}")) {
        Ok(m) => m,
        Err(e) => return format!("Could not read the message: {e}."),
    };
    let reactions = msg.get("reactions").map(Val::as_list).unwrap_or(&[]);
    if reactions.is_empty() {
        return "That message has no reactions.".into();
    }
    let mut out = format!("**{} reactions**", reactions.len());
    for r in reactions.iter().take(MAX_EMOJIS) {
        let count = r.get("count").and_then(Val::as_i64).unwrap_or(0);
        let Some(e) = r.get("emoji") else { continue };
        out.push_str(&format!("\n- {} x{count}", emoji_label(e)));
    }
    out.push_str("\nAdd the emoji as the last argument to see who reacted with it.");
    out
}

fn list_reactors(host: &mut dyn Host, channel: &str, message: &str, emoji: &str) -> String {
    let key = emoji_key(emoji);
    if key.is_empty() {
        return "Give the emoji itself or a custom emoji like `<:name:123>`.".into();
    }
    let route = format!(
        "/channels/{channel}/messages/{message}/reactions/{}?limit=100&type=0",
        encode(&key)
    );
    let users = match get(host, &route) {
        Ok(b) => b,
        Err(e) => return format!("Could not read the reactors: {e}."),
    };
    let users = users.as_list();
    if users.is_empty() {
        return format!("Nobody reacted with {emoji}.");
    }
    let names: Vec<String> = users
        .iter()
        .map(|u| {
            let name = u
                .str_at(&["global_name"])
                .filter(|s| !s.is_empty())
                .or_else(|| u.str_at(&["username"]))
                .unwrap_or("unknown");
            match u.id_at(&["id"]) {
                Some(id) => format!("{name} (<@{id}>)"),
                None => name.to_string(),
            }
        })
        .collect();
    let mut out = format!("**{} reacted with {emoji}**", names.len());
    for n in names.iter().take(SHOW) {
        out.push_str(&format!("\n- {n}"));
    }
    if names.len() > SHOW {
        out.push_str(&format!("\n...and {} more", names.len() - SHOW));
    }
    out
}

impl Plugin for WhoReacted {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// It only answers its command; there is no traffic to watch.
    fn wants(&self, _event: &EventKind) -> bool {
        false
    }

    fn on_event(&self, _host: &mut dyn Host, _event: Event<'_>) -> Verdict {
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::Rec;

    fn run(h: &mut Rec, opts: &[(&str, &str)]) -> String {
        WhoReacted::new().command().run(
            h,
            &Args {
                options: opts
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            },
        )
    }

    const MSG: &str = r#"{"id":"9","reactions":[
        {"count":3,"emoji":{"id":null,"name":"👍"}},
        {"count":1,"emoji":{"id":"555","name":"party"}}]}"#;

    #[test]
    fn lists_reactions_with_counts() {
        let mut h = Rec::default().answer("GET", "/channels/1/messages/9", 200, MSG);
        let out = run(&mut h, &[("channel_id", "1"), ("message_id", "9")]);
        assert!(out.contains("2 reactions"), "{out}");
        assert!(out.contains("\u{1f44d} x3"));
        assert!(out.contains("<:party:555> x1"));
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn lists_reactors_for_unicode_and_custom_emoji() {
        let route = "/channels/1/messages/9/reactions/%F0%9F%91%8D?limit=100&type=0";
        let mut h = Rec::default().answer(
            "GET",
            route,
            200,
            r#"[{"id":"5","username":"ann"},{"id":"6","username":"bob","global_name":"Bob"},{"x":1}]"#,
        );
        let out = run(
            &mut h,
            &[
                ("channel_id", "1"),
                ("message_id", "9"),
                ("emoji", "\u{1f44d}"),
            ],
        );
        assert!(out.contains("3 reacted with"), "{out}");
        assert!(
            out.contains("ann (<@5>)") && out.contains("Bob (<@6>)") && out.contains("unknown")
        );

        let route = "/channels/1/messages/9/reactions/party:555?limit=100&type=0";
        let mut h = Rec::default().answer("GET", route, 200, "[]");
        let out = run(
            &mut h,
            &[
                ("channel_id", "1"),
                ("message_id", "9"),
                ("emoji", "<:party:555>"),
            ],
        );
        assert!(out.starts_with("Nobody reacted"), "{out}");
        assert_eq!(h.rest_calls[0].0, format!("GET {route}"));
    }

    #[test]
    fn keys_and_encoding() {
        assert_eq!(emoji_key("<a:wave:42>"), "wave:42");
        assert_eq!(emoji_key("<:a_b:42>"), "a_b:42");
        assert_eq!(emoji_key("name:42"), "name:42");
        assert_eq!(encode("\u{1f44d}"), "%F0%9F%91%8D");
    }

    #[test]
    fn errors_bad_ids_and_disabled() {
        let mut h = Rec::default();
        assert!(run(&mut h, &[("channel_id", "x"), ("message_id", "9")]).contains("numeric"));
        assert!(run(&mut h, &[("channel_id", "1")]).contains("numeric"));
        assert!(h.rest_calls.is_empty());
        let out = run(&mut h, &[("channel_id", "1"), ("message_id", "9")]);
        assert!(out.contains("404"), "{out}");
        let mut h =
            Rec::default().answer("GET", "/channels/1/messages/9", 200, r#"{"reactions":"?"}"#);
        assert!(run(&mut h, &[("channel_id", "1"), ("message_id", "9")]).contains("no reactions"));
        let mut h = Rec::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(run(&mut h, &[("channel_id", "1"), ("message_id", "9")]).contains("is off"));
        assert!(h.rest_calls.is_empty());
    }

    #[test]
    fn observer_is_inert() {
        let p = WhoReacted::new();
        assert!(!p.wants(&EventKind::Dispatch {
            name: "MESSAGE_REACTION_ADD"
        }));
        assert!(!p.wants_gateway());
    }
}
