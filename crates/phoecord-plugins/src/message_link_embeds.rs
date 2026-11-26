//! MessageLinkEmbeds: previews of linked Discord messages (data part).
//!
//! Ported from Vencord `src/plugins/messageLinkEmbeds/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors TheSun, Ven, RyanCaoDev). Vencord renders the linked
//! message inside the client with its own message component. PhoeCord has no message component to
//! borrow yet, so the data part attaches an ordinary rich **embed** to the message that contains
//! the link: author name and avatar, the text (cut to 600 characters, or "[no content, N
//! attachments]"), the first image attachment, the original timestamp, and a footer with the
//! channel and message ids. Every client draws that without changes. It is the same thing Vencord's
//! "automod embed" mode shows. The richer look (message background colour, the real message
//! component) comes in Phase 2.
//!
//! How it works: a message (new, edited, or a page of history) is scanned for
//! `https://discord.com/channels/{guild|@me}/{channel}/{message}` links (also `ptb.`, `canary.`
//! and `discordapp.com`; `<link>` with angle brackets is left alone, as Vencord does). The
//! linked message is loaded with `GET /channels/{id}/messages?limit=1&around={id}` through
//! `Host::rest`, so only what your account may read is shown; a 403 or 404 is remembered and the
//! link is skipped. Up to `max_links` (default 3) links per message, `max_per_minute` lookups in
//! total (default 20), 200 embeds cached in memory.
//!
//! Settings (`[plugins.message_link_embeds]`), names as in Vencord: `listMode` (`blacklist`, the
//! default, or `whitelist`) and `idList` (comma-separated guild, channel or author ids). Not ported
//! settings: `messageBackgroundColor`, `automodEmbeds` (both about the Phase 2 renderer).
//!
//! `/phoecord linkembeds [clear]` shows how many linked messages are cached; `clear` empties the
//! cache (Vencord's "Clear the linked message cache" button).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, dispatch_data_mut};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

const PLUGIN_ID: &str = "message_link_embeds";
const DEFAULT_MAX_LINKS: i64 = 3;
const DEFAULT_PER_MINUTE: i64 = 20;
const CACHE_CAP: usize = 200;
const MAX_TEXT: usize = 600;
const EMBED_COLOR: i64 = 0x2b2d31;

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

/// Link -> the finished embed (`None`: not readable, do not ask again).
#[derive(Default)]
struct State {
    cache: HashMap<(u64, u64), Option<Val>>,
    order: VecDeque<(u64, u64)>,
    budget: Budget,
}

impl State {
    fn remember(&mut self, key: (u64, u64), embed: Option<Val>) {
        if self.cache.insert(key, embed).is_none() {
            self.order.push_back(key);
            while self.order.len() > CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }
}

pub struct MessageLinkEmbeds {
    manifest: Manifest,
    state: Shared<State>,
}

impl MessageLinkEmbeds {
    pub fn new() -> MessageLinkEmbeds {
        MessageLinkEmbeds {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "MessageLinkEmbeds",
                    env!("CARGO_PKG_VERSION"),
                    "Previews of linked Discord messages",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("MessageLinkEmbeds".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// `/phoecord linkembeds [clear]`. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "linkembeds",
            "Show or clear the cache of linked message previews",
            move |_host, args| {
                let mut st = state.lock();
                let n = st.cache.values().filter(|e| e.is_some()).count();
                if args.get("action").map(str::trim) == Some("clear") {
                    st.cache.clear();
                    st.order.clear();
                    format!("Cleared the linked message cache ({n} previews).")
                } else {
                    format!(
                        "{n} linked message previews cached ({} links unreadable). Use `clear` to empty it.",
                        st.cache.len() - n
                    )
                }
            },
        )
        .option("action", "`clear` to empty the cache", false)
        .boxed()
    }
}

impl Default for MessageLinkEmbeds {
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

/// One message link found in text.
#[derive(Clone, Debug, PartialEq)]
struct Link {
    url: String,
    guild: Option<u64>,
    channel: u64,
    message: u64,
}

fn is_discord_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    ["discord.com", "discordapp.com"]
        .iter()
        .any(|d| h == *d || h.strip_suffix(d).is_some_and(|p| p.ends_with('.')))
}

fn snowflake(s: &str) -> Option<u64> {
    (17..=20)
        .contains(&s.len())
        .then(|| s.parse().ok())
        .flatten()
}

/// Message links in `content`, in order, without duplicates.
fn find_links(content: &str, max: usize) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    for (start, _) in content.match_indices("http") {
        if out.len() >= max {
            break;
        }
        // `<https://…>` is how Discord writes "no embed"; Vencord skips it too.
        if content[..start].ends_with('<') {
            continue;
        }
        let tail = &content[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ')' | ']' | '|' | '"'))
            .unwrap_or(tail.len());
        let url = &tail[..end];
        let Some(rest) = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
        else {
            continue;
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        if !is_discord_host(host) {
            continue;
        }
        let path = path.split(['?', '#']).next().unwrap_or("");
        let seg: Vec<&str> = path.split('/').collect();
        let ["channels", g, c, m, ..] = seg.as_slice() else {
            continue;
        };
        let (guild, ok) = match *g {
            "@me" => (None, true),
            g => (snowflake(g), snowflake(g).is_some()),
        };
        let (Some(channel), Some(message)) = (snowflake(c), snowflake(m)) else {
            continue;
        };
        if !ok {
            continue;
        }
        if out.iter().any(|l| l.message == message) {
            continue;
        }
        out.push(Link {
            url: url.to_string(),
            guild,
            channel,
            message,
        });
    }
    out
}

/// Blacklist / whitelist check against `idList` for guild, channel and author.
fn allowed(host: &dyn Host, link: &Link, author: Option<u64>) -> bool {
    let list = host
        .setting("idList")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let ids: Vec<u64> = list
        .split([',', ' ', '\n'])
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let listed = [link.guild, Some(link.channel), author]
        .into_iter()
        .flatten()
        .any(|id| ids.contains(&id));
    match host
        .setting("listMode")
        .and_then(|v| v.as_str().map(str::to_string))
    {
        Some(m) if m == "whitelist" => listed,
        _ => !listed,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

/// The embed that stands for `linked` (a message object) behind `link`.
fn build_embed(link: &Link, linked: &Val) -> Val {
    let attachments = linked.get("attachments").map(Val::as_list).unwrap_or(&[]);
    let embeds = linked.get("embeds").map(Val::as_list).unwrap_or(&[]);
    let text = linked.str_at(&["content"]).unwrap_or("");
    let description = if text.is_empty() {
        match (attachments.len(), embeds.len()) {
            (0, 0) => String::new(),
            (a, 0) => format!("[no content, {a} attachment{}]", plural(a)),
            (0, e) => format!("[no content, {e} embed{}]", plural(e)),
            (a, e) => format!(
                "[no content, {a} attachment{} and {e} embed{}]",
                plural(a),
                plural(e)
            ),
        }
    } else {
        truncate(text, MAX_TEXT)
    };
    let name = linked
        .str_at(&["author", "global_name"])
        .filter(|s| !s.is_empty())
        .or_else(|| linked.str_at(&["author", "username"]))
        .unwrap_or("Unknown user");
    let mut author = Val::map();
    author.set("name", Val::Str(name.to_string()));
    author.set("url", Val::Str(link.url.clone()));
    if let (Some(uid), Some(hash)) = (
        linked.str_at(&["author", "id"]),
        linked.str_at(&["author", "avatar"]),
    ) {
        author.set(
            "icon_url",
            Val::Str(format!(
                "https://cdn.discordapp.com/avatars/{uid}/{hash}.png?size=64"
            )),
        );
    }
    let mut footer = Val::map();
    footer.set(
        "text",
        Val::Str(format!(
            "Linked message · {} / {}",
            link.channel, link.message
        )),
    );
    let mut e = Val::map();
    e.set("type", Val::Str("rich".into()));
    e.set("url", Val::Str(link.url.clone()));
    e.set("color", Val::Int(EMBED_COLOR));
    e.set("author", author);
    if !description.is_empty() {
        e.set("description", Val::Str(description));
    }
    e.set("footer", footer);
    if let Some(ts) = linked.str_at(&["timestamp"]) {
        e.set("timestamp", Val::Str(ts.to_string()));
    }
    let image = attachments.iter().find(|a| {
        a.str_at(&["content_type"])
            .is_some_and(|t| t.starts_with("image/"))
    });
    if let Some(url) = image.and_then(|a| a.str_at(&["url"])) {
        let mut img = Val::map();
        img.set("url", Val::Str(url.to_string()));
        e.set("image", img);
    }
    e
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Fetch (or recall) the embed for a link. `Err(())`: stop looking up for this message.
fn embed_for(
    host: &mut dyn Host,
    st: &mut State,
    link: &Link,
    per_minute: i64,
) -> Result<Option<Val>, ()> {
    let key = (link.channel, link.message);
    if let Some(hit) = st.cache.get(&key) {
        return Ok(hit.clone());
    }
    if !st.budget.take(per_minute) {
        return Err(());
    }
    let route = format!(
        "/channels/{}/messages?limit=1&around={}",
        link.channel, link.message
    );
    match host.rest("GET", &route, None) {
        Ok(r) if r.status == 200 => {
            let first = r.body.as_ref().and_then(|b| b.as_list().first().cloned());
            let embed = first
                .filter(|m| m.id_at(&["id"]) == Some(link.message))
                .map(|m| build_embed(link, &m));
            st.remember(key, embed.clone());
            Ok(embed)
        }
        Ok(r) if r.status == 403 || r.status == 404 => {
            st.remember(key, None);
            Ok(None)
        }
        Ok(_) => Err(()),
        Err(e) => {
            host.log(&format!("linked message lookup skipped: {e}"));
            Err(())
        }
    }
}

fn fix_message(host: &mut dyn Host, st: &mut State, msg: &mut Val) -> bool {
    let Some(content) = msg
        .str_at(&["content"])
        .filter(|c| c.contains("/channels/"))
    else {
        return false;
    };
    let max = host
        .setting("max_links")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(DEFAULT_MAX_LINKS) as usize;
    let per_minute = host
        .setting("max_per_minute")
        .and_then(|v| v.as_i64())
        .filter(|n| *n >= 0)
        .unwrap_or(DEFAULT_PER_MINUTE);
    let links = find_links(content, max);
    let author = msg.id_at(&["author", "id"]);
    let have: Vec<String> = msg
        .get("embeds")
        .map(Val::as_list)
        .unwrap_or(&[])
        .iter()
        .filter(|e| e.str_at(&["type"]) == Some("rich"))
        .filter_map(|e| e.str_at(&["url"]).map(str::to_string))
        .collect();
    let mut add = Vec::new();
    for link in links {
        if have.contains(&link.url) || !allowed(host, &link, author) {
            continue;
        }
        match embed_for(host, st, &link, per_minute) {
            Ok(Some(e)) => add.push(e),
            Ok(None) => {}
            Err(()) => break,
        }
    }
    if add.is_empty() {
        return false;
    }
    if let Some(list) = msg.get_mut("embeds").and_then(Val::as_list_mut) {
        list.extend(add);
    } else {
        msg.set("embeds", Val::List(add));
    }
    true
}

impl Plugin for MessageLinkEmbeds {
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
                if fix_message(host, &mut st, dispatch_data_mut(payload)) {
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
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    const LINK: &str =
        "https://discord.com/channels/111111111111111111/222222222222222222/333333333333333333";
    const ROUTE: &str = "/channels/222222222222222222/messages?limit=1&around=333333333333333333";
    const LINKED: &str = r#"[{"id":"333333333333333333","content":"hello world","timestamp":"2026-01-01T00:00:00.000000+00:00",
        "author":{"id":"5","username":"zed","global_name":"Zed","avatar":"abc"},
        "attachments":[{"url":"https://cdn/x.png","content_type":"image/png"}]}]"#;

    fn create(p: &MessageLinkEmbeds, h: &mut Rec, payload: &mut Val) -> Verdict {
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

    fn msg(content: &str) -> Val {
        let mut m = j(r#"{"id":"1","author":{"id":"9"},"embeds":[]}"#);
        m.set("content", Val::Str(content.into()));
        m
    }

    #[test]
    fn finds_links_and_skips_suppressed_ones() {
        let c = format!(
            "see {LINK} and <{LINK}> and https://ptb.discord.com/channels/@me/444444444444444444/555555555555555555?x=1 and https://example.com/channels/1/2/3"
        );
        let found = find_links(&c, 5);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].guild, Some(111111111111111111));
        assert_eq!(found[1].guild, None);
        assert!(find_links("no link /channels/1/2/3", 5).is_empty());
        assert_eq!(find_links(&format!("{LINK} {LINK}"), 5).len(), 1);
    }

    #[test]
    fn link_gets_an_embed() {
        let p = MessageLinkEmbeds::new();
        let mut h = Rec::default().answer("GET", ROUTE, 200, LINKED);
        let mut m = msg(&format!("look {LINK}"));
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Changed);
        let e = &m.get("embeds").unwrap().as_list()[0];
        assert_eq!(e.str_at(&["description"]), Some("hello world"));
        assert_eq!(e.str_at(&["author", "name"]), Some("Zed"));
        assert_eq!(e.str_at(&["image", "url"]), Some("https://cdn/x.png"));
        // Same link again: from the cache, and not twice on the same message.
        let mut again = msg(LINK);
        assert_eq!(create(&p, &mut h, &mut again), Verdict::Changed);
        assert_eq!(h.rest_calls.len(), 1);
        assert_eq!(
            create(&p, &mut h, &mut m),
            Verdict::Pass,
            "already embedded"
        );
    }

    #[test]
    fn link_in_a_real_gateway_frame_gets_an_embed() {
        use phoecord_plugin_runtime::{Chain, Outcome};
        let chain = Chain::new(vec![Box::new(MessageLinkEmbeds::new())]);
        let mut h = Rec::default().answer("GET", ROUTE, 200, LINKED);
        let mut frame = j(
            r#"{"op":0,"t":"MESSAGE_CREATE","s":1,"d":{"id":"1","author":{"id":"9"},"embeds":[]}}"#,
        );
        frame
            .get_mut("d")
            .unwrap()
            .set("content", Val::Str(format!("look {LINK}")));
        let out = chain.dispatch(&mut h, "MESSAGE_CREATE", &mut frame, &mut Vec::new());
        assert_eq!(out, Outcome::Changed);
        let e = &frame.at(&["d", "embeds"]).unwrap().as_list()[0];
        assert_eq!(e.str_at(&["description"]), Some("hello world"));
    }

    #[test]
    fn unreadable_blacklisted_and_junk_are_skipped() {
        let p = MessageLinkEmbeds::new();
        let mut h = Rec::default().answer("GET", ROUTE, 403, "{}");
        let mut m = msg(LINK);
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Pass);
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Pass);
        assert_eq!(h.rest_calls.len(), 1, "403 is remembered");
        // Blacklisted channel: no lookup at all.
        let mut h = Rec::default()
            .answer("GET", ROUTE, 200, LINKED)
            .with("idList", Val::Str("222222222222222222".into()));
        let mut m = msg(LINK);
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Pass);
        assert!(h.rest_calls.is_empty());
        // Whitelist mode with that id lets it through (fresh plugin: empty cache).
        let p = MessageLinkEmbeds::new();
        let mut h = h.with("listMode", Val::Str("whitelist".into()));
        let mut m = msg(LINK);
        assert_eq!(create(&p, &mut h, &mut m), Verdict::Changed);
        // Content that is not text.
        let mut odd = j(r#"{"content":5,"embeds":"x"}"#);
        assert_eq!(create(&p, &mut h, &mut odd), Verdict::Pass);
    }

    #[test]
    fn command_reports_and_clears() {
        let p = MessageLinkEmbeds::new();
        let mut h = Rec::default().answer("GET", ROUTE, 200, LINKED);
        create(&p, &mut h, &mut msg(LINK));
        let cmd = p.command();
        let none = Args { options: vec![] };
        assert!(
            cmd.run(&mut h, &none)
                .starts_with("1 linked message previews")
        );
        let clear = Args {
            options: vec![("action".into(), "clear".into())],
        };
        assert!(cmd.run(&mut h, &clear).starts_with("Cleared"));
        assert!(cmd.run(&mut h, &none).starts_with("0 linked"));
    }
}
