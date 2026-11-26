//! ValidReply: fix "Message could not be loaded" in replies.
//!
//! Ported from Vencord `src/plugins/validReply/index.ts` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; author newwares). Vencord waits until you hover the broken reply
//! preview, then loads the referenced message with `GET /channels/{id}/messages?limit=1&around={id}`
//! and, if the answer is a different message, marks the reply as deleted. Here the proxy does it
//! before the client renders: a reply (`message_reference` set, `referenced_message` missing)
//! gets the referenced message filled in. If Discord says the message is gone (the closest message
//! has another id) `referenced_message` becomes `null`, which the client shows as "Original
//! message was deleted". Replies whose `referenced_message` is already there (even `null`) are left
//! alone. In a page of history the plugin first looks for the message in the same page, which costs
//! no request.
//!
//! The `around` form is used because `GET /channels/{id}/messages/{id}` is closed to user accounts.
//!
//! Limits: `max_per_message` lookups per message (default 3), `max_per_minute` in total (default
//! 30); the host allows 5 Discord calls per 5 s per plugin, and the first refusal ends the
//! lookups of that message. A small in-memory cache (100 messages) answers repeated replies.
//!
//! Switching it on: `[plugins.valid_reply] enabled = true`, or `/phoecord toggle valid_reply`.
//!
//! Not ported: the hover trigger and the client's reply store (Phase 2).

use crate::state::{Shared, dispatch_data_mut};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

const DEFAULT_PER_MESSAGE: i64 = 3;
const DEFAULT_PER_MINUTE: i64 = 30;
const CACHE_CAP: usize = 100;
/// Message type `REPLY`.
const TYPE_REPLY: i64 = 19;

#[derive(Default)]
struct Budget {
    start: Option<Instant>,
    used: i64,
}

impl Budget {
    fn take(&mut self, per_minute: i64) -> bool {
        let now = Instant::now();
        if self
            .start
            .is_none_or(|s| now.duration_since(s) >= Duration::from_secs(60))
        {
            self.start = Some(now);
            self.used = 0;
        }
        if self.used >= per_minute {
            return false;
        }
        self.used += 1;
        true
    }
}

/// What is known about a referenced message: its object, or `None` when it no longer exists.
#[derive(Default)]
struct State {
    cache: HashMap<u64, Option<Val>>,
    order: VecDeque<u64>,
    budget: Budget,
}

impl State {
    fn remember(&mut self, id: u64, msg: Option<Val>) {
        if self.cache.insert(id, msg).is_none() {
            self.order.push_back(id);
            while self.order.len() > CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }
}

pub struct ValidReply {
    manifest: Manifest,
    state: Shared<State>,
}

impl ValidReply {
    pub fn new() -> ValidReply {
        ValidReply {
            manifest: {
                let mut m = Manifest::new(
                    "valid_reply",
                    "ValidReply",
                    env!("CARGO_PKG_VERSION"),
                    "Fix \"Message could not be loaded\" in replies",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ValidReply".into());
                m
            },
            state: Shared::default(),
        }
    }
}

impl Default for ValidReply {
    fn default() -> Self {
        Self::new()
    }
}

fn is_history(method: &str, route: &str) -> bool {
    let path = route.split('?').next().unwrap_or("");
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn is_message_event(name: &str) -> bool {
    matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE")
}

fn limit(host: &dyn Host, key: &str, default: i64) -> i64 {
    host.setting(key)
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(default)
}

/// `(channel id, message id)` of a reply that needs its `referenced_message`.
fn needs_fill(msg: &Val) -> Option<(u64, u64)> {
    if msg.get("referenced_message").is_some() {
        return None;
    }
    msg.get("message_reference")?;
    // Type 1 is a forward, not a reply.
    if msg.at(&["message_reference", "type"]).and_then(Val::as_i64) == Some(1) {
        return None;
    }
    if msg
        .get("type")
        .and_then(Val::as_i64)
        .is_some_and(|t| t != TYPE_REPLY)
    {
        return None;
    }
    let mid = msg.id_at(&["message_reference", "message_id"])?;
    let cid = msg
        .id_at(&["message_reference", "channel_id"])
        .or_else(|| msg.id_at(&["channel_id"]))?;
    Some((cid, mid))
}

fn fill(msg: &mut Val, found: Option<Val>) {
    msg.set("referenced_message", found.unwrap_or(Val::Null));
}

/// The referenced message from the cache or one lookup: `Ok(Some(found))` (`found` is `None` when it
/// is gone), `Ok(None)` when it cannot be read, `Err(())` when lookups should stop.
fn resolve(
    host: &mut dyn Host,
    st: &mut State,
    cid: u64,
    mid: u64,
    per_minute: i64,
) -> Result<Option<Option<Val>>, ()> {
    if let Some(hit) = st.cache.get(&mid) {
        return Ok(Some(hit.clone()));
    }
    if !st.budget.take(per_minute) {
        return Err(());
    }
    match host.rest(
        "GET",
        &format!("/channels/{cid}/messages?limit=1&around={mid}"),
        None,
    ) {
        Ok(r) if r.status == 200 => {
            let first = r.body.as_ref().and_then(|b| b.as_list().first().cloned());
            let found = first.filter(|m| m.id_at(&["id"]) == Some(mid));
            st.remember(mid, found.clone());
            Ok(Some(found))
        }
        // No access to the channel: leave the reply as the client has it.
        Ok(r) if r.status == 403 || r.status == 404 => Ok(None),
        Ok(_) => Err(()),
        Err(e) => {
            host.log(&format!("reply lookup skipped: {e}"));
            Err(())
        }
    }
}

fn fix_message(
    host: &mut dyn Host,
    st: &mut State,
    msg: &mut Val,
    page: &HashMap<u64, Val>,
) -> bool {
    let Some((cid, mid)) = needs_fill(msg) else {
        return false;
    };
    if let Some(m) = page.get(&mid) {
        fill(msg, Some(m.clone()));
        return true;
    }
    let per_minute = limit(host, "max_per_minute", DEFAULT_PER_MINUTE);
    match resolve(host, st, cid, mid, per_minute) {
        Ok(Some(found)) => {
            fill(msg, found);
            true
        }
        _ => false,
    }
}

impl Plugin for ValidReply {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => is_message_event(name),
            EventKind::RestResponse { method, route } => is_history(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if is_message_event(name) => {
                let mut st = self.state.lock();
                if fix_message(host, &mut st, dispatch_data_mut(payload), &HashMap::new()) {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } if is_history(method, route) => {
                let Some(list) = body.as_list_mut() else {
                    return Verdict::Pass;
                };
                let page: HashMap<u64, Val> = list
                    .iter()
                    .filter_map(|m| m.id_at(&["id"]).map(|id| (id, m.clone())))
                    .collect();
                let per_message = limit(host, "max_per_message", DEFAULT_PER_MESSAGE);
                let mut st = self.state.lock();
                let mut changed = false;
                let mut lookups = 0;
                for msg in list.iter_mut() {
                    let Some((_, mid)) = needs_fill(msg) else {
                        continue;
                    };
                    // Lookups per page, not per message, so one page cannot fire dozens.
                    if !page.contains_key(&mid) && !st.cache.contains_key(&mid) {
                        if lookups >= per_message {
                            continue;
                        }
                        lookups += 1;
                    }
                    changed |= fix_message(host, &mut st, msg, &page);
                }
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};

    fn create(p: &ValidReply, h: &mut Rec, payload: &mut Val) -> Verdict {
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name: "MESSAGE_CREATE",
                payload,
                extra: &mut extra,
            },
        )
    }

    const REPLY: &str = r#"{"id":"100","channel_id":"7","type":19,"content":"yes",
        "message_reference":{"channel_id":"7","message_id":"50"}}"#;

    #[test]
    fn missing_referenced_message_is_fetched() {
        let p = ValidReply::new();
        let mut h = Rec::default().answer(
            "GET",
            "/channels/7/messages?limit=1&around=50",
            200,
            r#"[{"id":"50","content":"original"}]"#,
        );
        let mut msg = j(REPLY);
        assert_eq!(create(&p, &mut h, &mut msg), Verdict::Changed);
        assert_eq!(
            msg.str_at(&["referenced_message", "content"]),
            Some("original")
        );
        // The cache answers the second reply to the same message.
        let mut again = j(REPLY);
        assert_eq!(create(&p, &mut h, &mut again), Verdict::Changed);
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn missing_referenced_message_is_fetched_in_a_real_gateway_frame() {
        use phoecord_plugin_runtime::{Chain, Outcome};
        let chain = Chain::new(vec![Box::new(ValidReply::new())]);
        let mut h = Rec::default().answer(
            "GET",
            "/channels/7/messages?limit=1&around=50",
            200,
            r#"[{"id":"50","content":"original"}]"#,
        );
        let mut frame = j(&format!(
            r#"{{"op":0,"t":"MESSAGE_CREATE","s":1,"d":{REPLY}}}"#
        ));
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut frame, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        assert_eq!(
            frame.str_at(&["d", "referenced_message", "content"]),
            Some("original")
        );
    }

    #[test]
    fn deleted_message_becomes_null() {
        let p = ValidReply::new();
        let mut h = Rec::default().answer(
            "GET",
            "/channels/7/messages?limit=1&around=50",
            200,
            r#"[{"id":"49","content":"neighbour"}]"#,
        );
        let mut msg = j(REPLY);
        assert_eq!(create(&p, &mut h, &mut msg), Verdict::Changed);
        assert_eq!(msg.get("referenced_message"), Some(&Val::Null));
    }

    #[test]
    fn complete_and_foreign_messages_are_left_alone() {
        let p = ValidReply::new();
        let mut h = Rec::default();
        for json in [
            r#"{"id":"1","type":19,"referenced_message":null,"message_reference":{"message_id":"2","channel_id":"7"}}"#,
            r#"{"id":"1","type":0,"content":"plain"}"#,
            r#"{"id":"1","type":19,"message_reference":{"channel_id":"7"}}"#,
            r#"{"id":"1","type":19,"message_reference":"junk"}"#,
        ] {
            let mut m = j(json);
            assert_eq!(create(&p, &mut h, &mut m), Verdict::Pass, "{json}");
        }
        assert!(h.rest_calls.is_empty());
        // Access denied: the reply stays as it was.
        let mut h =
            Rec::default().answer("GET", "/channels/7/messages?limit=1&around=50", 403, "{}");
        let mut m = j(REPLY);
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Pass);
        assert!(m.get("referenced_message").is_none());
    }

    #[test]
    fn history_uses_the_page_before_asking() {
        let p = ValidReply::new();
        let mut h = Rec::default();
        let mut body = j(
            r#"[{"id":"100","type":19,"message_reference":{"channel_id":"7","message_id":"50"}},
                {"id":"50","content":"orig"}]"#,
        );
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/channels/7/messages?limit=50",
                status: 200,
                body: &mut body,
            },
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(
            body.as_list()[0].str_at(&["referenced_message", "content"]),
            Some("orig")
        );
        assert!(h.rest_calls.is_empty());
    }
}
