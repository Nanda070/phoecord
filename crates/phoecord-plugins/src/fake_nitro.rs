//! FakeNitro (emoji part): use any emoji without Nitro (Vencord: FakeNitro). Emoji the account
//! cannot send for real are swapped for a small linked image just before the message leaves, so
//! they show up for everyone. The client is told you have Nitro so its pickers offer every emoji.
//!
//! Ported from the v0 implementation. Stickers and streaming-quality unlocks are not part of this
//! task. The real Nitro tier is remembered here and never sent upstream: the plugin only edits
//! what the client *receives*, and a user who really has Nitro is left completely alone.
//!
//! Settings (`[plugins.fake_nitro]`): `emoji_size` (default 48), `hyperlinks` (default `true`:
//! send `[name](link)` instead of the bare link), `unlock_pickers` (default `true`).
//!
//! The plugin learns your tier from `READY`, so after switching it on press Ctrl+R once; until
//! then it does nothing.

use phoecord_core::route::message_route_channel;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::sync::Mutex;

/// What the client is told its tier is while the pickers are unlocked (Nitro).
const FAKE_PREMIUM: i64 = 2;
const DEFAULT_EMOJI_SIZE: u32 = 48;

#[derive(Default)]
struct State {
    user_id: Option<u64>,
    /// The real tier (0 = none), as seen in `READY` / `USER_UPDATE` before this plugin edits them.
    premium_type: Option<i64>,
}

pub struct FakeNitro {
    manifest: Manifest,
    state: Mutex<State>,
}

impl FakeNitro {
    pub fn new() -> FakeNitro {
        FakeNitro {
            manifest: {
                let mut m = Manifest::new(
                    "fake_nitro",
                    "FakeNitro",
                    env!("CARGO_PKG_VERSION"),
                    "Send any emoji without Nitro (as linked images)",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("FakeNitro".into());
                m
            },
            state: Mutex::new(State::default()),
        }
    }
}

impl Default for FakeNitro {
    fn default() -> Self {
        Self::new()
    }
}

/// The matching rule for REST: sending or editing a message.
fn matches_request(method: &str, route: &str) -> bool {
    matches!(method, "POST" | "PATCH") && message_route_channel(route).is_some()
}

/// The matching rule for the gateway: the events that carry your own user object.
fn matches_dispatch(name: &str) -> bool {
    matches!(name, "READY" | "USER_UPDATE")
}

/// Rewrite `<:name:id>` / `<a:name:id>` emoji that `usable(id)` rejects.
fn rewrite(content: &str, usable: impl Fn(u64) -> bool, size: u32, hyperlinks: bool) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        match parse_emoji(tail) {
            Some((animated, name, id, len)) => {
                if usable(id) {
                    out.push_str(&tail[..len]);
                } else {
                    let ext = if animated { "gif" } else { "webp" };
                    let url = format!(
                        "https://cdn.discordapp.com/emojis/{id}.{ext}?size={size}&name={name}"
                    );
                    if hyperlinks {
                        out.push_str(&format!("[{name}]({url})"));
                    } else {
                        out.push_str(&url);
                    }
                }
                rest = &tail[len..];
            }
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parse `<a?:name:id>` at the start of `s`: `(animated, name, id, byte length)`.
/// `name` is 1-64 of `[A-Za-z0-9_~]`, `id` is 5-25 digits.
fn parse_emoji(s: &str) -> Option<(bool, &str, u64, usize)> {
    let b = s.as_bytes();
    let mut i = 1;
    let animated = b.get(i) == Some(&b'a');
    if animated {
        i += 1;
    }
    if b.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let name_start = i;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'~') {
        i += 1;
    }
    let name_len = i - name_start;
    if !(1..=64).contains(&name_len) || b.get(i) != Some(&b':') {
        return None;
    }
    let name = &s[name_start..i];
    i += 1;
    let id_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if !(5..=25).contains(&(i - id_start)) || b.get(i) != Some(&b'>') {
        return None;
    }
    let id = s[id_start..i].parse().unwrap_or(0);
    Some((animated, name, id, i + 1))
}

/// Your user object inside a `READY` or `USER_UPDATE` frame (`{"op":0,"t":..,"d":..}`).
fn user_of<'a>(name: &str, frame: &'a mut Val) -> Option<&'a mut Val> {
    let d = frame.get_mut("d")?;
    if name == "READY" {
        d.get_mut("user")
    } else {
        Some(d)
    }
}

impl Plugin for FakeNitro {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => matches_request(method, route),
            EventKind::Dispatch { name } => matches_dispatch(name),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if matches_request(method, route) => rewrite_message(self, host, route, body),
            Event::Dispatch { name, payload, .. } if matches_dispatch(name) => {
                unlock_pickers(self, host, name, payload)
            }
            _ => Verdict::Pass,
        }
    }
}

fn rewrite_message(p: &FakeNitro, host: &dyn Host, route: &str, body: &mut Val) -> Verdict {
    // Until READY has been seen the account is unknown: leave messages alone.
    let premium = p.state.lock().unwrap().premium_type;
    if host.self_id().is_none() || premium.is_none_or(|t| t > 0) {
        return Verdict::Pass;
    }
    let Some(channel) = message_route_channel(route) else {
        return Verdict::Pass;
    };
    // Emoji of the server the message goes to are free to use; in DMs nothing is.
    let allowed = host
        .channel_guild(channel)
        .map(|g| host.guild_emojis(g))
        .unwrap_or_default();
    let Some(text) = body.str_at(&["content"]) else {
        return Verdict::Pass;
    };
    let size = host
        .setting("emoji_size")
        .and_then(|v| v.as_i64())
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_EMOJI_SIZE);
    let hyperlinks = host
        .setting("hyperlinks")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let fixed = rewrite(text, |id| allowed.contains(&id), size, hyperlinks);
    if fixed == text {
        return Verdict::Pass;
    }
    body.set("content", Val::Str(fixed));
    Verdict::Changed
}

fn unlock_pickers(p: &FakeNitro, host: &dyn Host, name: &str, frame: &mut Val) -> Verdict {
    let unlock = host
        .setting("unlock_pickers")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut st = p.state.lock().unwrap();
    let Some(user) = user_of(name, frame) else {
        return Verdict::Pass;
    };
    if name == "READY" {
        st.user_id = user.id_at(&["id"]);
    } else if user.id_at(&["id"]).is_none() || user.id_at(&["id"]) != st.user_id {
        return Verdict::Pass; // somebody else's USER_UPDATE
    }
    // Remember the real tier before anything is edited; never send it anywhere.
    if let Some(t) = user.at(&["premium_type"]).and_then(Val::as_i64) {
        st.premium_type = Some(t);
    } else if name == "READY" {
        st.premium_type = Some(0);
    }
    if unlock && st.premium_type == Some(0) {
        user.set("premium_type", Val::Int(FAKE_PREMIUM));
        return Verdict::Changed;
    }
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn json(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    /// A host that knows one account: user 1, guild 10 with emoji 111111, channel 11 in it.
    #[derive(Default)]
    struct TestHost {
        settings: Vec<(&'static str, Val)>,
        signed_in: bool,
    }

    impl Host for TestHost {
        fn setting(&self, key: &str) -> Option<Val> {
            self.settings
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            self.signed_in.then_some(1)
        }
        fn notify(&mut self, _: &str, _: &str) {}
        fn log(&mut self, _: &str) {}
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, guild_id: u64) -> Vec<u64> {
            if guild_id == 10 { vec![111111] } else { vec![] }
        }
        fn channel_guild(&self, channel_id: u64) -> Option<u64> {
            (channel_id == 11).then_some(10)
        }
    }

    impl phoecord_core::ChainHost for TestHost {}

    fn ready(premium: i64) -> Val {
        json(&format!(
            r#"{{"op":0,"t":"READY","d":{{"user":{{"id":"1","premium_type":{premium}}}}}}}"#
        ))
    }

    fn send(chain: &Chain, host: &mut TestHost, route: &str, content: &str) -> (Outcome, Val) {
        let mut body = json(&format!(r#"{{"content":"{content}","nonce":"1"}}"#));
        let out = chain.rest_request(host, "POST", route, Some(&mut body));
        (out, body)
    }

    #[test]
    fn matching_rules() {
        assert!(matches_request("POST", "/channels/11/messages"));
        assert!(matches_request("PATCH", "/channels/11/messages/5"));
        assert!(!matches_request("GET", "/channels/11/messages"));
        assert!(!matches_request("POST", "/channels/11/typing"));
        assert!(matches_dispatch("READY"));
        assert!(matches_dispatch("USER_UPDATE"));
        assert!(!matches_dispatch("MESSAGE_CREATE"));
    }

    #[test]
    fn rewrites_only_unusable_emoji() {
        let s = "hi <:ember:111111> and <a:spin:222222>";
        let out = rewrite(s, |id| id == 111111, 48, true);
        assert_eq!(
            out,
            "hi <:ember:111111> and [spin](https://cdn.discordapp.com/emojis/222222.gif?size=48&name=spin)"
        );
        assert_eq!(
            rewrite("<:x:333333>", |_| false, 64, false),
            "https://cdn.discordapp.com/emojis/333333.webp?size=64&name=x"
        );
    }

    #[test]
    fn leaves_lookalikes_alone() {
        for s in [
            "<:x:123>", // id too short
            "<:bad name:12345>",
            "<::12345>",
            "<@123456> <#123456>",
            "<:x:12345",
            "a < b > c <",
            "<é:x:12345>",
        ] {
            assert_eq!(rewrite(s, |_| false, 48, true), s);
        }
        assert_eq!(
            rewrite("<<:x:12345>>", |_| false, 48, false),
            "<https://cdn.discordapp.com/emojis/12345.webp?size=48&name=x>"
        );
    }

    #[test]
    fn chain_end_to_end() {
        let chain = Chain::new(vec![Box::new(FakeNitro::new())]);
        let mut host = TestHost {
            signed_in: true,
            ..TestHost::default()
        };
        // READY: pickers are unlocked, the real tier is kept.
        let mut r = ready(0);
        let mut extra = Vec::new();
        assert_eq!(
            chain.dispatch(&mut host, "READY", &mut r, &mut extra),
            Outcome::Changed
        );
        assert_eq!(r.at(&["d", "user", "premium_type"]), Some(&Val::Int(2)));
        // Outgoing message: the foreign emoji becomes a link, the guild's own stays.
        let (out, body) = send(
            &chain,
            &mut host,
            "/channels/11/messages",
            "<:ember:111111> <:other:999999>",
        );
        assert_eq!(out, Outcome::Changed);
        let c = body.str_at(&["content"]).unwrap();
        assert!(c.starts_with("<:ember:111111> [other]("), "{c}");
        // In a DM nothing is usable.
        let (out, body) = send(
            &chain,
            &mut host,
            "/channels/99/messages",
            "<:ember:111111>",
        );
        assert_eq!(out, Outcome::Changed);
        assert!(body.str_at(&["content"]).unwrap().starts_with("[ember]("));
        // Plain text is untouched.
        let (out, _) = send(&chain, &mut host, "/channels/11/messages", "hello");
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn settings_are_read() {
        let chain = Chain::new(vec![Box::new(FakeNitro::new())]);
        let mut host = TestHost {
            signed_in: true,
            settings: vec![
                ("emoji_size", Val::Int(96)),
                ("hyperlinks", Val::Bool(false)),
                ("unlock_pickers", Val::Bool(false)),
            ],
        };
        let mut r = ready(0);
        let mut extra = Vec::new();
        // unlock_pickers off: READY is left as is.
        assert_eq!(
            chain.dispatch(&mut host, "READY", &mut r, &mut extra),
            Outcome::Untouched
        );
        let (_, body) = send(&chain, &mut host, "/channels/11/messages", "<:x:333333>");
        assert_eq!(
            body.str_at(&["content"]),
            Some("https://cdn.discordapp.com/emojis/333333.webp?size=96&name=x")
        );
    }

    #[test]
    fn real_nitro_is_never_touched() {
        let chain = Chain::new(vec![Box::new(FakeNitro::new())]);
        let mut host = TestHost {
            signed_in: true,
            ..TestHost::default()
        };
        let mut r = ready(2);
        let mut extra = Vec::new();
        assert_eq!(
            chain.dispatch(&mut host, "READY", &mut r, &mut extra),
            Outcome::Untouched
        );
        let (out, body) = send(
            &chain,
            &mut host,
            "/channels/11/messages",
            "<:other:999999>",
        );
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(body.str_at(&["content"]), Some("<:other:999999>"));
    }

    #[test]
    fn user_update_only_for_me() {
        let chain = Chain::new(vec![Box::new(FakeNitro::new())]);
        let mut host = TestHost {
            signed_in: true,
            ..TestHost::default()
        };
        let mut extra = Vec::new();
        let mut r = ready(0);
        chain.dispatch(&mut host, "READY", &mut r, &mut extra);
        let mut other = json(r#"{"op":0,"t":"USER_UPDATE","d":{"id":"7","premium_type":0}}"#);
        assert_eq!(
            chain.dispatch(&mut host, "USER_UPDATE", &mut other, &mut extra),
            Outcome::Untouched
        );
        let mut me = json(r#"{"op":0,"t":"USER_UPDATE","d":{"id":"1","premium_type":0}}"#);
        assert_eq!(
            chain.dispatch(&mut host, "USER_UPDATE", &mut me, &mut extra),
            Outcome::Changed
        );
        assert_eq!(me.at(&["d", "premium_type"]), Some(&Val::Int(2)));
    }

    #[test]
    fn does_nothing_before_ready() {
        let chain = Chain::new(vec![Box::new(FakeNitro::new())]);
        let mut host = NullHost::default();
        let mut body = json(r#"{"content":"<:other:999999>"}"#);
        assert_eq!(
            chain.rest_request(&mut host, "POST", "/channels/11/messages", Some(&mut body)),
            Outcome::Untouched
        );
        // Signed in per the host, but this plugin has not seen READY: tier unknown, so no edit.
        let mut host = TestHost {
            signed_in: true,
            ..TestHost::default()
        };
        let (out, _) = send(
            &chain,
            &mut host,
            "/channels/11/messages",
            "<:other:999999>",
        );
        assert_eq!(out, Outcome::Untouched);
    }
}
