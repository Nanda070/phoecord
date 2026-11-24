//! NoReplyMention: replies do not ping the author by default (Vencord: NoReplyMention).
//!
//! Ported from Vencord `src/plugins/NoReplyMention/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! Vendicated, DustyAngel47, rae, pylix, outfoxxed and contributors). Vencord decides in
//! `shouldMention(message, isHoldingShift)` whether the reply button pings the author; on the wire
//! that decision is `allowed_mentions.replied_user` of `POST /channels/{id}/messages` with a
//! `message_reference`. This plugin sets it to `false` when Vencord's rule says "do not ping".
//!
//! The author of the replied-to message is not in the request, so the plugin remembers the author
//! (and, in servers, the member's roles) of messages it saw in `MESSAGE_CREATE`, `MESSAGE_UPDATE`
//! and message history. An unknown message counts as "not listed".
//!
//! Settings (`[plugins.no_reply_mention]`), names as Vencord's in snake_case:
//! - `user_list`: user ids, separated by commas or spaces (a string, or a list). Default empty.
//! - `role_list`: role ids, same format. Default empty. Roles are known only for authors seen in a
//!   server message event; history answers carry no member data.
//! - `should_ping_listed` (default `true`): `true` pings only the listed users and roles, `false`
//!   pings everyone except them.
//!
//! Not ported: `inverseShiftReply`. Holding Shift is a keyboard state of the client; the request
//! does not say whether it was held.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// How many messages' authors are remembered.
const CACHE: usize = 5000;

#[derive(Default)]
struct Seen {
    /// message id -> author id, oldest first in `order`.
    authors: HashMap<u64, u64>,
    order: VecDeque<u64>,
    /// author id -> role ids from the latest server message.
    roles: HashMap<u64, Vec<u64>>,
}

impl Seen {
    fn remember(&mut self, msg: &Val) {
        let (Some(id), Some(author)) = (msg.id_at(&["id"]), msg.id_at(&["author", "id"])) else {
            return;
        };
        if self.authors.insert(id, author).is_none() {
            self.order.push_back(id);
            if self.order.len() > CACHE
                && let Some(old) = self.order.pop_front()
            {
                self.authors.remove(&old);
            }
        }
        if let Some(roles) = msg.at(&["member", "roles"]) {
            if self.roles.len() > CACHE {
                self.roles.clear();
            }
            self.roles.insert(
                author,
                roles.as_list().iter().filter_map(Val::snowflake).collect(),
            );
        }
    }
}

pub struct NoReplyMention {
    manifest: Manifest,
    seen: Mutex<Seen>,
}

impl NoReplyMention {
    pub fn new() -> NoReplyMention {
        NoReplyMention {
            manifest: Manifest {
                id: "no_reply_mention".into(),
                name: "NoReplyMention".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Disables reply pings by default".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("NoReplyMention".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            seen: Mutex::new(Seen::default()),
        }
    }
}

impl Default for NoReplyMention {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/messages`.
fn is_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "POST" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

/// `GET /channels/{id}/messages`.
fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn dispatch_matches(name: &str) -> bool {
    matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE")
}

/// A setting holding ids: a string separated by commas or spaces, or a list of strings/integers.
fn id_set(host: &dyn Host, key: &str) -> HashSet<u64> {
    match host.setting(key) {
        Some(Val::Str(s)) => s
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter_map(|t| t.parse().ok())
            .collect(),
        Some(v) => v.as_list().iter().filter_map(Val::snowflake).collect(),
        None => HashSet::new(),
    }
}

/// Vencord's `shouldMention` without Shift: should this reply ping the author?
fn should_mention(listed: bool, ping_listed: bool) -> bool {
    if ping_listed { listed } else { !listed }
}

impl Plugin for NoReplyMention {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_send(method, route),
            EventKind::RestResponse { method, route } => is_history(method, route),
            EventKind::Dispatch { name } => dispatch_matches(name),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if dispatch_matches(name) => {
                if let Some(m) = payload.get("d") {
                    self.seen.lock().unwrap().remember(m);
                }
                Verdict::Pass
            }
            Event::RestResponse {
                method,
                route,
                body,
                ..
            } if is_history(method, route) => {
                let mut seen = self.seen.lock().unwrap();
                for m in body.as_list() {
                    seen.remember(m);
                }
                Verdict::Pass
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_send(method, route) => {
                // A forward (`message_reference.type` 1) is not a reply.
                let Some(reference) = body.get("message_reference") else {
                    return Verdict::Pass;
                };
                if reference.at(&["type"]).and_then(Val::as_i64) == Some(1) {
                    return Verdict::Pass;
                }
                let Some(target) = reference.id_at(&["message_id"]) else {
                    return Verdict::Pass;
                };
                let users = id_set(host, "user_list");
                let roles = id_set(host, "role_list");
                let ping_listed = host
                    .setting("should_ping_listed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let listed = {
                    let seen = self.seen.lock().unwrap();
                    seen.authors.get(&target).is_some_and(|author| {
                        users.contains(author)
                            || seen
                                .roles
                                .get(author)
                                .is_some_and(|rs| rs.iter().any(|r| roles.contains(r)))
                    })
                };
                if should_mention(listed, ping_listed) {
                    return Verdict::Pass;
                }
                set_replied_user_false(body)
            }
            _ => Verdict::Pass,
        }
    }
}

/// `allowed_mentions.replied_user = false`, creating `allowed_mentions` the way the client
/// writes it (parse everything) when the message has none.
fn set_replied_user_false(body: &mut Val) -> Verdict {
    match body.get_mut("allowed_mentions") {
        Some(am @ Val::Map(_)) => {
            if am.at(&["replied_user"]).and_then(Val::as_bool) == Some(false) {
                return Verdict::Pass;
            }
            am.set("replied_user", Val::Bool(false));
        }
        _ => {
            let mut am = Val::map();
            am.set(
                "parse",
                Val::List(
                    ["users", "roles", "everyone"]
                        .iter()
                        .map(|s| Val::Str((*s).into()))
                        .collect(),
                ),
            );
            am.set("replied_user", Val::Bool(false));
            body.set("allowed_mentions", am);
        }
    }
    Verdict::Changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    #[derive(Default)]
    struct Cfg(HashMap<String, Val>);

    impl Host for Cfg {
        fn setting(&self, key: &str) -> Option<Val> {
            self.0.get(key).cloned()
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            None
        }
        fn notify(&mut self, _: &str, _: &str) {}
        fn log(&mut self, _: &str) {}
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, _: u64) -> Vec<u64> {
            Vec::new()
        }
        fn channel_guild(&self, _: u64) -> Option<u64> {
            None
        }
    }

    fn reply(chain: &Chain, host: &mut Cfg, body: &mut Val) -> Outcome {
        chain.rest_request(host, "POST", "/channels/9/messages", Some(body))
    }

    fn seen_message(chain: &Chain, host: &mut Cfg, json: &str) {
        let mut frame = j(json);
        chain.dispatch(host, "MESSAGE_CREATE", &mut frame, &mut Vec::new());
    }

    #[test]
    fn matching_rules() {
        assert!(is_send("POST", "/channels/9/messages"));
        assert!(!is_send("POST", "/channels/9/messages/1"));
        assert!(!is_send("GET", "/channels/9/messages"));
        assert!(is_history("GET", "/channels/9/messages"));
        assert!(dispatch_matches("MESSAGE_UPDATE"));
        assert!(!dispatch_matches("MESSAGE_DELETE"));
        let p = NoReplyMention::new();
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/channels/9/messages"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "PATCH",
            route: "/channels/9/messages/1"
        }));
    }

    #[test]
    fn vencord_rule_without_shift() {
        // shouldPingListed = true: ping only the listed.
        assert!(should_mention(true, true));
        assert!(!should_mention(false, true));
        // shouldPingListed = false: ping everyone but the listed.
        assert!(!should_mention(true, false));
        assert!(should_mention(false, false));
    }

    #[test]
    fn reply_does_not_ping_by_default() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        let mut body = j(r#"{"content":"hi","message_reference":{"message_id":"50"}}"#);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Changed);
        assert_eq!(
            body.at(&["allowed_mentions", "replied_user"])
                .and_then(Val::as_bool),
            Some(false)
        );
        assert_eq!(
            body.at(&["allowed_mentions", "parse"])
                .unwrap()
                .as_list()
                .len(),
            3
        );
    }

    #[test]
    fn keeps_existing_allowed_mentions() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        let mut body = j(
            r#"{"message_reference":{"message_id":"50"},"allowed_mentions":{"parse":["users"],"replied_user":true}}"#,
        );
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Changed);
        assert_eq!(
            body.at(&["allowed_mentions", "replied_user"])
                .and_then(Val::as_bool),
            Some(false)
        );
        assert_eq!(
            body.at(&["allowed_mentions", "parse"])
                .unwrap()
                .as_list()
                .len(),
            1
        );
    }

    #[test]
    fn listed_users_and_roles_still_get_pinged() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        h.0.insert("user_list".into(), Val::Str("7, 8".into()));
        h.0.insert("role_list".into(), Val::Str("300".into()));
        seen_message(
            &chain,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"50","author":{"id":"8"}}}"#,
        );
        seen_message(
            &chain,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"51","author":{"id":"20"},"member":{"roles":["300"]}}}"#,
        );
        seen_message(
            &chain,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"52","author":{"id":"21"}}}"#,
        );
        for (id, expected) in [
            ("50", Outcome::Untouched),
            ("51", Outcome::Untouched),
            ("52", Outcome::Changed),
            ("99", Outcome::Changed),
        ] {
            let mut body = j(&format!(
                r#"{{"message_reference":{{"message_id":"{id}"}}}}"#
            ));
            assert_eq!(reply(&chain, &mut h, &mut body), expected, "reply to {id}");
        }
    }

    #[test]
    fn inverted_behaviour_exempts_listed_users_from_not_pinging() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        h.0.insert("should_ping_listed".into(), Val::Bool(false));
        h.0.insert("user_list".into(), Val::List(vec![Val::Str("8".into())]));
        seen_message(
            &chain,
            &mut h,
            r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"50","author":{"id":"8"}}}"#,
        );
        // Listed + shouldPingListed=false: listed users are not pinged.
        let mut body = j(r#"{"message_reference":{"message_id":"50"}}"#);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Changed);
        // Unlisted users are pinged.
        let mut body = j(r#"{"message_reference":{"message_id":"99"}}"#);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Untouched);
    }

    #[test]
    fn learns_authors_from_history_and_handles_etf_ids() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        h.0.insert("user_list".into(), Val::Str("8".into()));
        let mut history = j(r#"[{"id":"60","author":{"id":"8"}}]"#);
        chain.rest_response(&mut h, "GET", "/channels/9/messages", 200, &mut history);
        // ETF request: atom keys, integer ids.
        let k = |s: &str| Val::Atom(s.into());
        let refr = Val::Map(vec![(k("message_id"), Val::Int(60))]);
        let mut body = Val::Map(vec![(k("message_reference"), refr)]);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Untouched);
        let refr = Val::Map(vec![(k("message_id"), Val::Int(61))]);
        let mut body = Val::Map(vec![(k("message_reference"), refr)]);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Changed);
        assert_eq!(
            body.at(&["allowed_mentions", "replied_user"])
                .and_then(Val::as_bool),
            Some(false)
        );
    }

    #[test]
    fn plain_messages_and_forwards_are_left_alone() {
        let chain = Chain::new(vec![Box::new(NoReplyMention::new())]);
        let mut h = Cfg::default();
        let mut body = j(r#"{"content":"hi"}"#);
        assert_eq!(reply(&chain, &mut h, &mut body), Outcome::Untouched);
        let mut fwd = j(r#"{"message_reference":{"type":1,"message_id":"50","channel_id":"3"}}"#);
        assert_eq!(reply(&chain, &mut h, &mut fwd), Outcome::Untouched);
        let mut edit = j(r#"{"message_reference":{"message_id":"50"}}"#);
        let out = chain.rest_request(&mut h, "PATCH", "/channels/9/messages/1", Some(&mut edit));
        assert_eq!(out, Outcome::Untouched);
    }
}
