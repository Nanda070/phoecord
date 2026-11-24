//! iLoveSpam: stops Discord hiding messages from "likely spammers" (Vencord: iLoveSpam).
//!
//! Ported from Vencord `src/plugins/iLoveSpam/index.ts` (GPL-3.0-or-later, Copyright (c) Vendicated,
//! botato, Nyako and contributors). Vencord patches the client's `hasFlag` so that user flag
//! `1 << 20` (`LIKELY_SPAMMER`) reads as unset. On the wire that flag is a bit of the user's
//! `flags` / `public_flags`, so this plugin clears the bit on the users in the data the client
//! receives: message authors, mentioned users, replied-to authors, guild members and message
//! history.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

/// `UserFlags.LIKELY_SPAMMER`, the bit Vencord's patch clears.
const LIKELY_SPAMMER: i64 = 1 << 20;

pub struct ILoveSpam {
    manifest: Manifest,
}

impl ILoveSpam {
    pub fn new() -> ILoveSpam {
        ILoveSpam {
            manifest: Manifest {
                id: "i_love_spam".into(),
                name: "iLoveSpam".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Do not hide messages from 'likely spammers'".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("iLoveSpam".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
        }
    }
}

impl Default for ILoveSpam {
    fn default() -> Self {
        Self::new()
    }
}

/// Gateway events that carry users the plugin cleans.
fn dispatch_matches(name: &str) -> bool {
    matches!(
        name,
        "MESSAGE_CREATE"
            | "MESSAGE_UPDATE"
            | "GUILD_MEMBER_ADD"
            | "GUILD_MEMBER_UPDATE"
            | "GUILD_MEMBERS_CHUNK"
    )
}

/// `GET /channels/{id}/messages`: message history.
fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

/// Clear the bit in one numeric field; true if it was set.
fn clear_field(user: &mut Val, key: &str) -> bool {
    let Some(flags) = user.get_mut(key) else {
        return false;
    };
    match flags.as_i64() {
        Some(n) if n & LIKELY_SPAMMER != 0 => {
            *flags = Val::Int(n & !LIKELY_SPAMMER);
            true
        }
        _ => false,
    }
}

fn clean_user(user: &mut Val) -> bool {
    let a = clear_field(user, "flags");
    let b = clear_field(user, "public_flags");
    a || b
}

/// Clean the user fields of a message (author, member, mentions, the replied-to message).
fn clean_message(msg: &mut Val) -> bool {
    let mut changed = false;
    if let Some(u) = msg.get_mut("author") {
        changed |= clean_user(u);
    }
    if let Some(u) = msg.get_mut("member").and_then(|m| m.get_mut("user")) {
        changed |= clean_user(u);
    }
    if let Some(Val::List(list)) = msg.get_mut("mentions") {
        for u in list {
            changed |= clean_user(u);
        }
    }
    if let Some(r) = msg.get_mut("referenced_message") {
        changed |= clean_message(r);
    }
    changed
}

fn clean_member(member: &mut Val) -> bool {
    member.get_mut("user").is_some_and(clean_user)
}

fn clean_dispatch(name: &str, d: &mut Val) -> bool {
    match name {
        "MESSAGE_CREATE" | "MESSAGE_UPDATE" => clean_message(d),
        // GUILD_MEMBER_ADD carries the member with `user`; GUILD_MEMBER_UPDATE has `user` at the top.
        "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => clean_member(d),
        "GUILD_MEMBERS_CHUNK" => match d.get_mut("members") {
            Some(Val::List(list)) => list.iter_mut().fold(false, |c, m| clean_member(m) | c),
            _ => false,
        },
        _ => false,
    }
}

impl Plugin for ILoveSpam {
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

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let changed = match event {
            Event::Dispatch { name, payload, .. } if dispatch_matches(name) => payload
                .get_mut("d")
                .is_some_and(|d| clean_dispatch(name, d)),
            Event::RestResponse {
                method,
                route,
                body: Val::List(list),
                ..
            } if is_history(method, route) => {
                list.iter_mut().fold(false, |c, m| clean_message(m) | c)
            }
            _ => false,
        };
        if changed {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    const SPAM: i64 = 1 << 20;

    fn flags(v: &Val, path: &[&str]) -> Option<i64> {
        v.at(path).and_then(Val::as_i64)
    }

    #[test]
    fn matching_rules() {
        assert!(dispatch_matches("MESSAGE_CREATE"));
        assert!(dispatch_matches("GUILD_MEMBERS_CHUNK"));
        assert!(!dispatch_matches("TYPING_START"));
        assert!(is_history("GET", "/channels/9/messages"));
        assert!(!is_history("POST", "/channels/9/messages"));
        assert!(!is_history("GET", "/channels/9/messages/1"));
        let p = ILoveSpam::new();
        assert!(p.wants(&EventKind::Dispatch {
            name: "MESSAGE_UPDATE"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/channels/9/messages"
        }));
        assert!(!p.wants(&EventKind::Send { op: 3 }));
    }

    #[test]
    fn clears_only_the_spammer_bit() {
        let mut u = j(&format!(
            r#"{{"id":"2","flags":{},"public_flags":{SPAM}}}"#,
            SPAM | 64
        ));
        assert!(clean_user(&mut u));
        assert_eq!(flags(&u, &["flags"]), Some(64));
        assert_eq!(flags(&u, &["public_flags"]), Some(0));
        assert!(!clean_user(&mut u), "already clean");
        let mut none = j(r#"{"id":"3"}"#);
        assert!(!clean_user(&mut none));
    }

    #[test]
    fn cleans_author_mentions_member_and_reply() {
        let mut m = j(&format!(
            r#"{{"id":"5","author":{{"id":"2","public_flags":{SPAM}}},
               "member":{{"user":{{"id":"2","flags":{SPAM}}}}},
               "mentions":[{{"id":"4","public_flags":{SPAM}}}],
               "referenced_message":{{"id":"4","author":{{"id":"6","public_flags":{SPAM}}}}}}}"#
        ));
        assert!(clean_message(&mut m));
        assert_eq!(flags(&m, &["author", "public_flags"]), Some(0));
        assert_eq!(flags(&m, &["member", "user", "flags"]), Some(0));
        assert_eq!(
            m.at(&["mentions"]).unwrap().as_list()[0]
                .get("public_flags")
                .and_then(Val::as_i64),
            Some(0)
        );
        assert_eq!(
            flags(&m, &["referenced_message", "author", "public_flags"]),
            Some(0)
        );
    }

    #[test]
    fn cleans_member_events() {
        let mut d = j(&format!(r#"{{"user":{{"id":"2","public_flags":{SPAM}}}}}"#));
        assert!(clean_dispatch("GUILD_MEMBER_UPDATE", &mut d));
        assert_eq!(flags(&d, &["user", "public_flags"]), Some(0));
        let mut chunk = j(&format!(
            r#"{{"members":[{{"user":{{"id":"2","public_flags":{SPAM}}}}}]}}"#
        ));
        assert!(clean_dispatch("GUILD_MEMBERS_CHUNK", &mut chunk));
    }

    #[test]
    fn works_on_etf_integers() {
        // ETF: ids are integers, keys are atoms.
        let user = Val::Map(vec![
            (Val::Atom("id".into()), Val::Int(2)),
            (Val::Atom("public_flags".into()), Val::Int(SPAM | 1)),
        ]);
        let mut d = Val::Map(vec![
            (Val::Atom("id".into()), Val::Int(5)),
            (Val::Atom("author".into()), user),
        ]);
        assert!(clean_message(&mut d));
        assert_eq!(flags(&d, &["author", "public_flags"]), Some(1));
    }

    #[test]
    fn chain_cleans_dispatch_and_history() {
        let chain = Chain::new(vec![Box::new(ILoveSpam::new())]);
        let mut host = NullHost::default();

        let mut frame = j(&format!(
            r#"{{"op":0,"t":"MESSAGE_CREATE","d":{{"id":"5","author":{{"id":"2","public_flags":{SPAM}}}}}}}"#
        ));
        let mut extra = Vec::new();
        let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut frame, &mut extra);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(flags(&frame, &["d", "author", "public_flags"]), Some(0));

        let mut history = j(&format!(
            r#"[{{"id":"1","author":{{"id":"2","flags":{SPAM}}}}},{{"id":"2","author":{{"id":"3"}}}}]"#
        ));
        let out = chain.rest_response(&mut host, "GET", "/channels/9/messages", 200, &mut history);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(flags(&history.as_list()[0], &["author", "flags"]), Some(0));

        let mut clean = j(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"6","author":{"id":"3"}}}"#);
        let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut clean, &mut extra);
        assert_eq!(out, Outcome::Untouched);
        let out = chain.dispatch(&mut host, "TYPING_START", &mut clean, &mut extra);
        assert_eq!(out, Outcome::Untouched);
    }
}
