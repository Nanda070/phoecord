//! NoBlockedMessages: messages from users you blocked (or ignored) never reach the client
//! (Vencord: NoBlockedMessages).
//!
//! Ported from Vencord `src/plugins/NoBlockedMessages/index.ts` (GPL-3.0-or-later, Copyright (c)
//! Vendicated, rushii, Samu, jamesbt365 and contributors). Vencord hides the "N blocked messages"
//! placeholder in the chat view and, with `ignoreMessages`, makes the message and read-state stores
//! skip `MESSAGE_CREATE` from blocked or ignored users (`RelationshipStore.isBlocked` /
//! `isIgnored`). On the wire that is: learn who is blocked (relationship type 2) or ignored
//! (`user_ignored`) from `READY` and the `RELATIONSHIP_*` events, then drop `MESSAGE_CREATE` and
//! `MESSAGE_UPDATE` from them and, if `filter_history` is on, remove their messages from
//! message-history answers.
//!
//! Settings (`[plugins.no_blocked_messages]`):
//! - `apply_to_ignored_users` (default `true`), as in Vencord.
//! - `filter_history` (default `false`). When `false`, `GET /channels/{id}/messages` answers are
//!   left untouched and only live messages are dropped, so old messages from blocked users still
//!   show when you scroll back. When `true`, history is filtered too, but a page then has fewer
//!   messages than the client asked for, and the client may take that as "no older messages" and
//!   stop loading further back. Turn it on only if you accept that trade-off.
//!
//! Vencord's `ignoreMessages` has no separate switch here: dropping the live message is always on.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashSet;
use std::sync::Mutex;

/// Relationship type of a blocked user.
const BLOCKED: i64 = 2;

#[derive(Default)]
struct State {
    blocked: HashSet<u64>,
    ignored: HashSet<u64>,
}

pub struct NoBlockedMessages {
    manifest: Manifest,
    state: Mutex<State>,
}

impl NoBlockedMessages {
    pub fn new() -> NoBlockedMessages {
        NoBlockedMessages {
            manifest: {
                let mut m = Manifest::new(
                    "no_blocked_messages",
                    "NoBlockedMessages",
                    env!("CARGO_PKG_VERSION"),
                    "Hide all blocked/ignored messages from chat completely",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("NoBlockedMessages".into());
                m
            },
            state: Mutex::new(State::default()),
        }
    }
}

impl Default for NoBlockedMessages {
    fn default() -> Self {
        Self::new()
    }
}

fn dispatch_matches(name: &str) -> bool {
    matches!(
        name,
        "READY"
            | "RELATIONSHIP_ADD"
            | "RELATIONSHIP_UPDATE"
            | "RELATIONSHIP_REMOVE"
            | "MESSAGE_CREATE"
            | "MESSAGE_UPDATE"
    )
}

/// `GET /channels/{id}/messages`: message history.
fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

/// The user a relationship entry is about: `id`, else `user_id`, else `user.id`.
fn relationship_user(r: &Val) -> Option<u64> {
    r.id_at(&["id"])
        .or_else(|| r.id_at(&["user_id"]))
        .or_else(|| r.id_at(&["user", "id"]))
}

impl State {
    fn set(&mut self, r: &Val) {
        let Some(id) = relationship_user(r) else {
            return;
        };
        if r.at(&["type"]).and_then(Val::as_i64) == Some(BLOCKED) {
            self.blocked.insert(id);
        } else {
            self.blocked.remove(&id);
        }
        if r.at(&["user_ignored"]).and_then(Val::as_bool) == Some(true) {
            self.ignored.insert(id);
        } else {
            self.ignored.remove(&id);
        }
    }

    fn remove(&mut self, r: &Val) {
        if let Some(id) = relationship_user(r) {
            self.blocked.remove(&id);
            self.ignored.remove(&id);
        }
    }

    fn track(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                self.blocked.clear();
                self.ignored.clear();
                for r in d.get("relationships").map(Val::as_list).unwrap_or_default() {
                    self.set(r);
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => self.set(d),
            "RELATIONSHIP_REMOVE" => self.remove(d),
            _ => {}
        }
    }

    fn hides(&self, author: Option<u64>, with_ignored: bool) -> bool {
        author.is_some_and(|a| {
            self.blocked.contains(&a) || (with_ignored && self.ignored.contains(&a))
        })
    }
}

fn filter_history(host: &dyn Host) -> bool {
    host.setting("filter_history")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn with_ignored(host: &dyn Host) -> bool {
    host.setting("apply_to_ignored_users")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

impl Plugin for NoBlockedMessages {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => dispatch_matches(name),
            EventKind::RestResponse { method, route } => is_history(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if dispatch_matches(name) => {
                let Some(d) = payload.get("d") else {
                    return Verdict::Pass;
                };
                let mut st = self.state.lock().unwrap();
                match name {
                    "MESSAGE_CREATE" | "MESSAGE_UPDATE" => {
                        if st.hides(d.id_at(&["author", "id"]), with_ignored(host)) {
                            Verdict::Drop
                        } else {
                            Verdict::Pass
                        }
                    }
                    _ => {
                        st.track(name, d);
                        Verdict::Pass
                    }
                }
            }
            Event::RestResponse {
                method,
                route,
                body,
                ..
            } if is_history(method, route) => {
                if !filter_history(host) {
                    return Verdict::Pass;
                }
                let with_ignored = with_ignored(host);
                let st = self.state.lock().unwrap();
                let Some(list) = body.as_list_mut() else {
                    return Verdict::Pass;
                };
                let before = list.len();
                list.retain(|m| !st.hides(m.id_at(&["author", "id"]), with_ignored));
                if list.len() == before {
                    Verdict::Pass
                } else {
                    Verdict::Changed
                }
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, Outcome};
    use std::collections::HashMap;

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    /// A host with settings.
    #[derive(Default)]
    struct Cfg(HashMap<String, Val>);

    impl Host for Cfg {
        fn setting(&self, key: &str) -> Option<Val> {
            self.0.get(key).cloned()
        }
        fn kv_get(&self, k: &str) -> Option<Val> {
            {
                let _ = k;
                None
            }
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

    impl phoecord_core::ChainHost for Cfg {}

    const READY: &str = r#"{"op":0,"t":"READY","d":{"relationships":[
        {"id":"10","type":2},{"id":"11","type":1},{"id":"12","type":1,"user_ignored":true}]}}"#;

    fn dispatch(chain: &Chain, host: &mut dyn phoecord_core::ChainHost, frame: &str) -> Outcome {
        let mut v = j(frame);
        let name = v.str_at(&["t"]).unwrap().to_string();
        chain.dispatch(host, &name, &mut v, &mut Vec::new())
    }

    fn msg(t: &str, author: &str) -> String {
        format!(
            r#"{{"op":0,"t":"{t}","d":{{"id":"50","channel_id":"9","author":{{"id":"{author}"}}}}}}"#
        )
    }

    #[test]
    fn matching_rules() {
        assert!(dispatch_matches("READY"));
        assert!(dispatch_matches("MESSAGE_CREATE"));
        assert!(!dispatch_matches("MESSAGE_DELETE"));
        assert!(is_history("GET", "/channels/9/messages"));
        assert!(!is_history("GET", "/channels/9/pins"));
        let p = NoBlockedMessages::new();
        assert!(p.wants(&EventKind::RestResponse {
            method: "GET",
            route: "/channels/9/messages"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/channels/9/messages"
        }));
    }

    #[test]
    fn learns_blocked_and_ignored_from_ready_and_events() {
        let p = NoBlockedMessages::new();
        let mut h = Cfg::default();
        let ready = j(READY);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "READY",
                payload: &mut ready.clone(),
                extra: &mut Vec::new(),
            },
        );
        {
            let st = p.state.lock().unwrap();
            assert!(st.blocked.contains(&10) && !st.blocked.contains(&11));
            assert!(st.ignored.contains(&12));
        }
        let mut add = j(r#"{"d":{"id":"11","type":2}}"#);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "RELATIONSHIP_ADD",
                payload: &mut add,
                extra: &mut Vec::new(),
            },
        );
        assert!(p.state.lock().unwrap().blocked.contains(&11));
        let mut rm = j(r#"{"d":{"id":"10","type":2}}"#);
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "RELATIONSHIP_REMOVE",
                payload: &mut rm,
                extra: &mut Vec::new(),
            },
        );
        assert!(!p.state.lock().unwrap().blocked.contains(&10));
    }

    #[test]
    fn chain_drops_blocked_messages_and_filters_history() {
        let chain = Chain::new(vec![Box::new(NoBlockedMessages::new())]);
        let mut h = Cfg::default();
        h.0.insert("filter_history".into(), Val::Bool(true));
        dispatch(&chain, &mut h, READY);

        assert!(matches!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "10")),
            Outcome::Dropped { .. }
        ));
        assert!(matches!(
            dispatch(&chain, &mut h, &msg("MESSAGE_UPDATE", "12")),
            Outcome::Dropped { .. }
        ));
        assert_eq!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "11")),
            Outcome::Untouched
        );

        let mut history = j(
            r#"[{"id":"1","author":{"id":"10"}},{"id":"2","author":{"id":"11"}},{"id":"3","author":{"id":"12"}}]"#,
        );
        let out = chain.rest_response(&mut h, "GET", "/channels/9/messages", 200, &mut history);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(history.as_list().len(), 1);
        assert_eq!(history.as_list()[0].str_at(&["id"]), Some("2"));

        let mut other = j(r#"[{"id":"1","author":{"id":"10"}}]"#);
        let out = chain.rest_response(&mut h, "GET", "/channels/9/pins", 200, &mut other);
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn history_is_left_alone_unless_filter_history_is_on() {
        let chain = Chain::new(vec![Box::new(NoBlockedMessages::new())]);
        let mut h = Cfg::default();
        dispatch(&chain, &mut h, READY);
        let json = r#"[{"id":"1","author":{"id":"10"}},{"id":"2","author":{"id":"11"}}]"#;
        let mut history = j(json);
        let out = chain.rest_response(&mut h, "GET", "/channels/9/messages", 200, &mut history);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(history, j(json));
        // Live messages are dropped either way.
        assert!(matches!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "10")),
            Outcome::Dropped { .. }
        ));
        h.0.insert("filter_history".into(), Val::Bool(false));
        let mut history = j(json);
        let out = chain.rest_response(&mut h, "GET", "/channels/9/messages", 200, &mut history);
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn apply_to_ignored_users_can_be_switched_off() {
        let chain = Chain::new(vec![Box::new(NoBlockedMessages::new())]);
        let mut h = Cfg::default();
        h.0.insert("apply_to_ignored_users".into(), Val::Bool(false));
        dispatch(&chain, &mut h, READY);
        assert_eq!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "12")),
            Outcome::Untouched
        );
        assert!(matches!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "10")),
            Outcome::Dropped { .. }
        ));
    }

    #[test]
    fn works_on_etf_integers() {
        // ETF: atom keys, integer ids.
        let k = |s: &str| Val::Atom(s.into());
        let rel = Val::Map(vec![(k("id"), Val::Int(10)), (k("type"), Val::Int(2))]);
        let ready_d = Val::Map(vec![(k("relationships"), Val::List(vec![rel]))]);
        let mut ready = Val::Map(vec![(k("t"), k("READY")), (k("d"), ready_d)]);
        let chain = Chain::new(vec![Box::new(NoBlockedMessages::new())]);
        let mut h = Cfg::default();
        chain.dispatch(&mut h, "READY", &mut ready, &mut Vec::new());

        let author = Val::Map(vec![(k("id"), Val::Int(10))]);
        let d = Val::Map(vec![(k("id"), Val::Int(50)), (k("author"), author)]);
        let mut frame = Val::Map(vec![(k("t"), k("MESSAGE_CREATE")), (k("d"), d)]);
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut frame, &mut Vec::new());
        assert!(matches!(out, Outcome::Dropped { .. }));
    }

    #[test]
    fn a_new_ready_forgets_old_blocks() {
        let chain = Chain::new(vec![Box::new(NoBlockedMessages::new())]);
        let mut h = Cfg::default();
        dispatch(&chain, &mut h, READY);
        dispatch(
            &chain,
            &mut h,
            r#"{"op":0,"t":"READY","d":{"relationships":[]}}"#,
        );
        assert_eq!(
            dispatch(&chain, &mut h, &msg("MESSAGE_CREATE", "10")),
            Outcome::Untouched
        );
    }
}
