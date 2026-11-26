//! ValidUser: fix mentions of users the client does not know ("@unknown-user").
//!
//! Ported from Vencord `src/plugins/validUser/index.tsx` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors Ven, Dolfies). Vencord waits until you hover an unknown
//! mention and then fetches `GET /users/{id}` and feeds the answer to the client's user store. Here
//! the proxy does it before the client renders: when a message (a new one, an edit, or a page of
//! history) mentions `<@id>` and that user is not in the message's `mentions` list and was never
//! seen before, the plugin asks `GET /users/{id}` through `Host::rest` and adds the user object to
//! the message's `mentions`, which is where the client reads mention names from.
//!
//! Limits: at most `max_per_message` lookups per message (default 3), at most `max_per_minute`
//! lookups in total (default 20); the host allows 5 Discord calls per 5 s per plugin, and the
//! first refusal ends the lookups of that message. Users Discord says do not exist are remembered
//! and not asked again. Only ids, names and avatar hashes are kept, bounded to 500 users.
//!
//! Switching it on: `[plugins.valid_user] enabled = true`, or `/phoecord toggle valid_user`.
//!
//! Not ported: the hover trigger, the fake profile badges and the profile fetch-failure dispatch
//! (they only exist for the client's user popout, Phase 2).

use crate::state::{Shared, Users, dispatch_data, dispatch_data_mut, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

const DEFAULT_PER_MESSAGE: i64 = 3;
const DEFAULT_PER_MINUTE: i64 = 20;
const MAX_IDS_PER_MESSAGE: usize = 20;
const CACHE_CAP: usize = 500;
/// Fields of a user object worth keeping; everything else (email, flags of the account…) is dropped.
const USER_FIELDS: &[&str] = &[
    "id",
    "username",
    "discriminator",
    "global_name",
    "avatar",
    "bot",
    "public_flags",
    "avatar_decoration_data",
];

/// A fixed one-minute window of allowed lookups.
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

#[derive(Default)]
struct State {
    seen: Users,
    fetched: HashMap<u64, Val>,
    order: VecDeque<u64>,
    missing: HashSet<u64>,
    budget: Budget,
}

impl State {
    fn remember(&mut self, id: u64, user: Val) {
        if self.fetched.insert(id, user).is_none() {
            self.order.push_back(id);
            while self.order.len() > CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.fetched.remove(&old);
                }
            }
        }
    }
}

pub struct ValidUser {
    manifest: Manifest,
    state: Shared<State>,
}

impl ValidUser {
    pub fn new() -> ValidUser {
        ValidUser {
            manifest: {
                let mut m = Manifest::new(
                    "valid_user",
                    "ValidUser",
                    env!("CARGO_PKG_VERSION"),
                    "Fix mentions of unknown users showing as @unknown-user",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ValidUser".into());
                m
            },
            state: Shared::default(),
        }
    }
}

impl Default for ValidUser {
    fn default() -> Self {
        Self::new()
    }
}

/// `GET /channels/{id}/messages` (a page of history).
fn is_history(method: &str, route: &str) -> bool {
    let path = route.split('?').next().unwrap_or("");
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn is_message_event(name: &str) -> bool {
    matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE")
}

/// Ids in `<@id>` and `<@!id>`; role mentions (`<@&id>`) do not count.
fn mention_ids(content: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(i) = rest.find("<@") {
        rest = &rest[i + 2..];
        let r = rest.strip_prefix('!').unwrap_or(rest);
        let digits = r.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && r[digits..].starts_with('>')
            && let Ok(id) = r[..digits].parse::<u64>()
            && !out.contains(&id)
        {
            out.push(id);
            if out.len() >= MAX_IDS_PER_MESSAGE {
                break;
            }
        }
    }
    out
}

/// A copy of a user object with only [`USER_FIELDS`].
fn slim(user: &Val) -> Option<Val> {
    user.id_at(&["id"])?;
    let mut out = Val::map();
    for f in USER_FIELDS {
        if let Some(v) = user.get(f) {
            out.set(f, v.clone());
        }
    }
    Some(out)
}

fn limit(host: &dyn Host, key: &str, default: i64) -> i64 {
    host.setting(key)
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(default)
}

/// Adds the missing mentioned users to `msg["mentions"]`. `true` if the message changed.
fn fix_message(host: &mut dyn Host, st: &mut State, msg: &mut Val) -> bool {
    let wanted: Vec<u64> = match msg.str_at(&["content"]) {
        Some(c) if c.contains("<@") => mention_ids(c),
        _ => Vec::new(),
    };
    if wanted.is_empty() {
        st.seen.observe("MESSAGE_CREATE", msg);
        return false;
    }
    let listed: HashSet<u64> = msg
        .get("mentions")
        .map(Val::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|u| u.id_at(&["id"]))
        .collect();
    let per_message = limit(host, "max_per_message", DEFAULT_PER_MESSAGE);
    let per_minute = limit(host, "max_per_minute", DEFAULT_PER_MINUTE);
    let mut lookups = 0;
    let mut add = Vec::new();
    for id in wanted {
        if listed.contains(&id) {
            continue;
        }
        if let Some(u) = st.fetched.get(&id) {
            add.push(u.clone());
            continue;
        }
        // Seen before in any user object: the client knows this user already.
        if st.seen.name(id).is_some() || st.missing.contains(&id) {
            continue;
        }
        if lookups >= per_message || !st.budget.take(per_minute) {
            break;
        }
        lookups += 1;
        match host.rest("GET", &format!("/users/{id}"), None) {
            Ok(r) if r.status == 200 => {
                if let Some(u) = r.body.as_ref().and_then(slim) {
                    st.remember(id, u.clone());
                    add.push(u);
                }
            }
            Ok(r) if r.status == 404 || r.status == 403 => {
                if st.missing.len() >= CACHE_CAP {
                    st.missing.clear();
                }
                st.missing.insert(id);
            }
            // 429, 5xx or our own limiter: try again on a later message.
            Ok(_) => break,
            Err(e) => {
                host.log(&format!("lookup of user {id} skipped: {e}"));
                break;
            }
        }
    }
    let changed = !add.is_empty();
    if changed {
        if let Some(list) = msg.get_mut("mentions").and_then(Val::as_list_mut) {
            list.extend(add);
        } else {
            msg.set("mentions", Val::List(add));
        }
    }
    st.seen.observe("MESSAGE_CREATE", msg);
    changed
}

impl Plugin for ValidUser {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => {
                is_message_event(name) || is_one_of(name, Users::EVENTS)
            }
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
                if fix_message(host, &mut st, dispatch_data_mut(payload)) {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::Dispatch { name, payload, .. } if is_one_of(name, Users::EVENTS) => {
                self.state.lock().seen.observe(name, dispatch_data(payload));
                Verdict::Pass
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
                let mut st = self.state.lock();
                let mut changed = false;
                for msg in list.iter_mut() {
                    changed |= fix_message(host, &mut st, msg);
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

    fn dispatch(p: &ValidUser, h: &mut Rec, name: &str, payload: &mut Val) -> Verdict {
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name,
                payload,
                extra: &mut extra,
            },
        )
    }

    const USER: &str =
        r#"{"id":"55","username":"zed","global_name":"Zed","avatar":"abc","email":"secret@x"}"#;

    #[test]
    fn parses_user_mentions_only() {
        assert_eq!(
            mention_ids("hi <@12> and <@!34> <@&56> <@x> <@78 <@12>"),
            [12, 34]
        );
    }

    #[test]
    fn unknown_mention_is_looked_up_and_added() {
        let p = ValidUser::new();
        let mut h = Rec::default().answer("GET", "/users/55", 200, USER);
        let mut msg = j(
            r#"{"id":"1","content":"hello <@55>","mentions":[],"author":{"id":"9","username":"a"}}"#,
        );
        assert_eq!(
            dispatch(&p, &mut h, "MESSAGE_CREATE", &mut msg),
            Verdict::Changed
        );
        let m = &msg.get("mentions").unwrap().as_list()[0];
        assert_eq!(m.str_at(&["username"]), Some("zed"));
        assert!(m.get("email").is_none(), "only safe fields are copied");
        // Asked once, and the second message uses the cache.
        let mut again = j(r#"{"id":"2","content":"<@55> again","mentions":[]}"#);
        assert_eq!(
            dispatch(&p, &mut h, "MESSAGE_CREATE", &mut again),
            Verdict::Changed
        );
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn unknown_mention_is_added_in_a_real_gateway_frame() {
        use phoecord_plugin_runtime::{Chain, Outcome};
        let chain = Chain::new(vec![Box::new(ValidUser::new())]);
        let mut h = Rec::default().answer("GET", "/users/55", 200, USER);
        let mut frame = j(
            r#"{"op":0,"t":"MESSAGE_CREATE","s":1,"d":{"id":"1","content":"hello <@55>","mentions":[],
               "author":{"id":"9","username":"a"}}}"#,
        );
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut frame, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        let m = &frame.at(&["d", "mentions"]).unwrap().as_list()[0];
        assert_eq!(m.str_at(&["username"]), Some("zed"));
    }

    #[test]
    fn users_seen_in_a_real_gateway_frame_cost_no_lookup() {
        use phoecord_plugin_runtime::{Chain, Outcome};
        let chain = Chain::new(vec![Box::new(ValidUser::new())]);
        let mut h = Rec::default();
        let mut presence = j(
            r#"{"op":0,"t":"PRESENCE_UPDATE","s":1,"d":{"user":{"id":"55","username":"zed"},"status":"online"}}"#,
        );
        chain.dispatch(&mut h, "PRESENCE_UPDATE", &mut presence, &mut Vec::new());
        let mut msg = j(r#"{"op":0,"t":"MESSAGE_CREATE","s":2,"d":{"content":"<@55>"}}"#);
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut msg, &mut Vec::new());
        assert_eq!(out, Outcome::Untouched);
        assert!(h.rest_calls.is_empty());
    }

    #[test]
    fn listed_and_seen_users_cost_no_lookup() {
        let p = ValidUser::new();
        let mut h = Rec::default();
        let mut known = j(r#"{"content":"<@5>","mentions":[{"id":"5","username":"e"}]}"#);
        assert_eq!(
            dispatch(&p, &mut h, "MESSAGE_CREATE", &mut known),
            Verdict::Pass
        );
        // User 5 is now known from that mentions list.
        let mut later = j(r#"{"content":"<@5>"}"#);
        assert_eq!(
            dispatch(&p, &mut h, "MESSAGE_UPDATE", &mut later),
            Verdict::Pass
        );
        assert!(h.rest_calls.is_empty());
    }

    #[test]
    fn failed_lookups_are_remembered_and_budgeted() {
        let p = ValidUser::new();
        let mut h = Rec::default()
            .answer("GET", "/users/7", 404, "{}")
            .with("max_per_message", Val::Int(1));
        let mut m = j(r#"{"content":"<@7> <@8> <@9>"}"#);
        assert_eq!(
            dispatch(&p, &mut h, "MESSAGE_CREATE", &mut m),
            Verdict::Pass
        );
        assert_eq!(h.rest_calls.len(), 1, "max_per_message");
        let mut m = j(r#"{"content":"<@7>"}"#);
        dispatch(&p, &mut h, "MESSAGE_CREATE", &mut m);
        assert_eq!(h.rest_calls.len(), 1, "404 is not asked again");
    }

    #[test]
    fn history_pages_are_fixed_and_junk_does_not_panic() {
        let p = ValidUser::new();
        let mut h = Rec::default().answer("GET", "/users/55", 200, USER);
        let mut body = j(r#"[{"content":"x <@55>"},{"content":null},"junk",{"mentions":3}]"#);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/channels/3/messages?limit=50",
                status: 200,
                body: &mut body,
            },
        );
        assert_eq!(v, Verdict::Changed);
        assert_eq!(
            body.as_list()[0].get("mentions").unwrap().as_list().len(),
            1
        );
        let mut bad = j(r#"{"not":"a list"}"#);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/channels/3/messages",
                status: 200,
                body: &mut bad,
            },
        );
        assert_eq!(v, Verdict::Pass);
    }
}
