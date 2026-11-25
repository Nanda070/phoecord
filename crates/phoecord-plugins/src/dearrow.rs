//! Dearrow: calmer titles and thumbnails on YouTube embeds, from DeArrow (Vencord: Dearrow).
//!
//! Ported from Vencord `src/plugins/dearrow/index.tsx` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; author Ven). Vencord rewrites the embed when it is shown; here the
//! message is rewritten on its way to the client: `MESSAGE_CREATE` / `MESSAGE_UPDATE` and the
//! answer of `GET /channels/{id}/messages`. For an embed of provider `YouTube` whose video URL is
//! `https://www.youtube.com/embed/<id>`, `https://sponsor.ajay.app/api/branding?videoID=<id>` is
//! asked; the first title (if its votes are not negative) replaces `title`, and the first
//! thumbnail (votes not negative, not the original) replaces `thumbnail.proxy_url`.
//!
//! Settings, with Vencord's names: `replaceElements` (`0` titles and thumbnails, the default;
//! `1` titles only; `2` thumbnails only) and `dearrowByDefault` (default `true`). Vencord's toggle
//! button is Phase 2, so with `dearrowByDefault = false` there is nothing to switch with and
//! embeds are left alone. `hideButton` has nothing to hide.
//!
//! Privacy and cost: DeArrow learns the ids of the videos in the messages you receive. Answers are
//! kept in memory, and at most [`MAX_LOOKUPS`] new videos are asked about per event, because the
//! event waits for them.
//!
//! Unverified against a live client: whether it shows a thumbnail from `dearrow-thumb.ajay.app`
//! (Vencord relaxes the client's content security policy for it; PhoeCord does not). If pictures
//! break, set `replaceElements = 1`.

use crate::state::{Shared, dispatch_data_mut};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::{HashMap, VecDeque};

const API_HOST: &str = "sponsor.ajay.app";
/// New videos looked up for one event.
const MAX_LOOKUPS: usize = 3;
/// Videos whose answer is remembered.
const CACHE_LIMIT: usize = 500;

/// What DeArrow offers for a video.
#[derive(Clone, Debug, Default, PartialEq)]
struct Branding {
    title: Option<String>,
    thumbnail: Option<String>,
}

#[derive(Default)]
struct Cache {
    by_id: HashMap<String, Branding>,
    order: VecDeque<String>,
}

impl Cache {
    fn insert(&mut self, id: String, branding: Branding) {
        if self.by_id.insert(id.clone(), branding).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > CACHE_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.by_id.remove(&old);
            }
        }
    }
}

pub struct Dearrow {
    manifest: Manifest,
    cache: Shared<Cache>,
}

impl Dearrow {
    pub fn new() -> Dearrow {
        Dearrow {
            manifest: Manifest {
                id: "dearrow".into(),
                name: "Dearrow".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Less sensational YouTube embed titles and thumbnails (DeArrow)"
                    .into(),
                authors: vec!["Ven".into()],
                vencord: Some("Dearrow".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: vec![API_HOST.into()],
            },
            cache: Shared::default(),
        }
    }
}

impl Default for Dearrow {
    fn default() -> Self {
        Self::new()
    }
}

fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn is_message_event(name: &str) -> bool {
    matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE")
}

/// A string in any gateway shape (JSON string, ETF atom or binary).
fn text(v: &Val) -> Option<&str> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => std::str::from_utf8(b).ok(),
        v => v.as_str(),
    }
}

/// A new string shaped like the one it replaces.
fn shaped(old: &Val, new: &str) -> Val {
    match old {
        Val::Bin(_) => Val::Bin(new.as_bytes().to_vec()),
        v => v.str_like(new),
    }
}

/// The video id of a YouTube embed, as Vencord's regex finds it.
fn video_id(embed: &Val) -> Option<String> {
    if text(embed.at(&["provider", "name"])?)? != "YouTube" {
        return None;
    }
    let url = text(embed.at(&["video", "url"])?)?;
    let rest = url.split("https://www.youtube.com/embed/").nth(1)?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(11)
        .collect();
    (id.len() == 11).then_some(id)
}

/// DeArrow marks words it must not re-capitalise with `>`; Vencord removes the mark:
/// `/(^|\s)>(\S)/g` becomes `$1$2`.
fn clean_title(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let at_word_start = i == 0 || chars[i - 1].is_whitespace();
        let marks_a_word = chars.get(i + 1).is_some_and(|n| !n.is_whitespace());
        if !(c == '>' && at_word_start && marks_a_word) {
            out.push(c);
        }
    }
    out
}

/// What the branding answer offers, by Vencord's rules.
fn branding(video: &str, answer: &Val) -> Branding {
    let first = |key: &str| answer.get(key).and_then(|l| l.as_list().first());
    let voted = |v: &&Val| v.get("votes").and_then(Val::as_i64).is_some_and(|n| n >= 0);
    let title = first("titles")
        .filter(voted)
        .and_then(|t| t.str_at(&["title"]))
        .map(clean_title);
    let thumbnail = first("thumbnails")
        .filter(voted)
        .filter(|t| t.get("original").and_then(Val::as_bool) != Some(true))
        .and_then(|t| match t.get("timestamp")? {
            Val::Int(n) => Some(n.to_string()),
            Val::Float(f) if f.is_finite() => Some(f.to_string()),
            _ => None,
        })
        .map(|time| {
            format!(
                "https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID={video}&time={time}"
            )
        });
    Branding { title, thumbnail }
}

#[derive(Clone, Copy)]
struct Replace {
    titles: bool,
    thumbnails: bool,
}

fn what_to_replace(host: &dyn Host) -> Option<Replace> {
    if host.setting("dearrowByDefault").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let choice = host
        .setting("replaceElements")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Some(Replace {
        titles: choice != 2,
        thumbnails: choice != 1,
    })
}

impl Dearrow {
    /// The cached or fetched branding; `None` when it could not be had (not remembered).
    fn lookup(&self, host: &mut dyn Host, id: &str, budget: &mut usize) -> Option<Branding> {
        if let Some(known) = self.cache.lock().by_id.get(id) {
            return Some(known.clone());
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let url = format!("https://{API_HOST}/api/branding?videoID={id}");
        let found = match host.fetch("GET", &url, None) {
            Ok(r) if r.status == 200 => branding(id, r.body.as_ref()?),
            // DeArrow knows nothing about this video.
            Ok(r) if r.status == 404 => Branding::default(),
            Ok(r) => {
                host.log(&format!("DeArrow answered {}", r.status));
                return None;
            }
            Err(e) => {
                host.log(&format!("DeArrow: {e}"));
                // Do not wait for an unreachable service once per video.
                *budget = 0;
                return None;
            }
        };
        self.cache.lock().insert(id.to_string(), found.clone());
        Some(found)
    }

    /// Rewrite the YouTube embeds of one message.
    fn message(
        &self,
        host: &mut dyn Host,
        message: &mut Val,
        replace: Replace,
        budget: &mut usize,
    ) -> bool {
        let Some(embeds) = message.get_mut("embeds").and_then(Val::as_list_mut) else {
            return false;
        };
        let mut changed = false;
        for embed in embeds {
            let Some(id) = video_id(embed) else {
                continue;
            };
            let Some(found) = self.lookup(host, &id, budget) else {
                continue;
            };
            if replace.titles
                && let Some(title) = &found.title
                && let Some(slot) = embed.get_mut("title")
            {
                *slot = shaped(slot, title);
                changed = true;
            }
            if replace.thumbnails
                && let Some(url) = &found.thumbnail
                && let Some(slot) = embed
                    .get_mut("thumbnail")
                    .and_then(|t| t.get_mut("proxy_url"))
            {
                *slot = shaped(slot, url);
                changed = true;
            }
        }
        changed
    }
}

impl Plugin for Dearrow {
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
        let mut budget = MAX_LOOKUPS;
        let changed = match event {
            Event::Dispatch { name, payload, .. } if is_message_event(name) => {
                let Some(replace) = what_to_replace(host) else {
                    return Verdict::Pass;
                };
                self.message(host, dispatch_data_mut(payload), replace, &mut budget)
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } if is_history(method, route) => {
                let Some(replace) = what_to_replace(host) else {
                    return Verdict::Pass;
                };
                let mut any = false;
                for message in body.as_list_mut().into_iter().flatten() {
                    any |= self.message(host, message, replace, &mut budget);
                }
                any
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
    use crate::testutil::{Cfg, j};
    use phoecord_core::plugin::HostResponse;
    use phoecord_plugin_runtime::{Chain, Outcome};

    /// [`Cfg`] plus a `fetch` that answers every call the same way and records the URLs.
    struct Net {
        cfg: Cfg,
        answer: Result<(u16, &'static str), &'static str>,
        urls: Vec<String>,
    }

    impl Net {
        fn new(answer: Result<(u16, &'static str), &'static str>) -> Net {
            Net {
                cfg: Cfg::default(),
                answer,
                urls: Vec::new(),
            }
        }
    }

    impl Host for Net {
        fn setting(&self, key: &str) -> Option<Val> {
            self.cfg.setting(key)
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
        fn fetch(&mut self, _: &str, url: &str, _: Option<&Val>) -> Result<HostResponse, String> {
            self.urls.push(url.into());
            let (status, body) = self.answer.map_err(str::to_string)?;
            Ok(HostResponse {
                status,
                body: (!body.is_empty()).then(|| j(body)),
                text: None,
                bytes: None,
                headers: Vec::new(),
            })
        }
    }

    const BRANDING: &str = r#"{"titles":[{"title":">iPhone is a >phone > ok","votes":0}],
        "thumbnails":[{"timestamp":12.5,"votes":1,"original":false}]}"#;

    fn message(id: &str) -> String {
        format!(
            r#"{{"embeds":[{{"provider":{{"name":"YouTube"}},"title":"WOW",
            "video":{{"url":"https://www.youtube.com/embed/{id}?x=1"}},
            "thumbnail":{{"url":"u","proxy_url":"p"}}}}]}}"#
        )
    }

    fn frame(id: &str) -> Val {
        j(&format!(
            r#"{{"op":0,"t":"MESSAGE_CREATE","d":{}}}"#,
            message(id)
        ))
    }

    fn dispatch(p: &Dearrow, h: &mut Net, payload: &mut Val) -> Verdict {
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

    fn embed(frame: &Val) -> &Val {
        &frame.at(&["d", "embeds"]).unwrap().as_list()[0]
    }

    #[test]
    fn matching_rules() {
        let p = Dearrow::new();
        assert!(p.wants(&EventKind::Dispatch {
            name: "MESSAGE_UPDATE"
        }));
        assert!(!p.wants(&EventKind::Dispatch {
            name: "MESSAGE_DELETE"
        }));
        assert!(is_history("GET", "/channels/1/messages"));
        assert!(!is_history("POST", "/channels/1/messages"));
        assert!(!is_history("GET", "/channels/1/messages/2"));
        assert_eq!(p.manifest().fetch, ["sponsor.ajay.app"]);
    }

    #[test]
    fn title_marks_are_removed_like_vencords_regex() {
        assert_eq!(
            clean_title(">iPhone is >a > b a>b >"),
            "iPhone is a > b a>b >"
        );
    }

    #[test]
    fn replaces_title_and_thumbnail_and_remembers_the_answer() {
        let p = Dearrow::new();
        let mut h = Net::new(Ok((200, BRANDING)));
        let mut f = frame("dQw4w9WgXcQ");
        assert_eq!(dispatch(&p, &mut h, &mut f), Verdict::Changed);
        assert_eq!(embed(&f).str_at(&["title"]), Some("iPhone is a phone > ok"));
        assert_eq!(
            embed(&f).str_at(&["thumbnail", "proxy_url"]),
            Some(
                "https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID=dQw4w9WgXcQ&time=12.5"
            )
        );
        assert_eq!(embed(&f).str_at(&["thumbnail", "url"]), Some("u"));
        dispatch(&p, &mut h, &mut frame("dQw4w9WgXcQ"));
        assert_eq!(
            h.urls,
            ["https://sponsor.ajay.app/api/branding?videoID=dQw4w9WgXcQ"]
        );
    }

    #[test]
    fn settings_choose_what_changes() {
        for (choice, title, thumb) in [(1, true, false), (2, false, true)] {
            let p = Dearrow::new();
            let mut h = Net::new(Ok((200, BRANDING)));
            h.cfg = Cfg::default().with("replaceElements", Val::Int(choice));
            let mut f = frame("dQw4w9WgXcQ");
            dispatch(&p, &mut h, &mut f);
            assert_eq!(embed(&f).str_at(&["title"]) != Some("WOW"), title);
            assert_eq!(
                embed(&f).str_at(&["thumbnail", "proxy_url"]) != Some("p"),
                thumb
            );
        }
        let mut h = Net::new(Ok((200, BRANDING)));
        h.cfg = Cfg::default().with("dearrowByDefault", Val::Bool(false));
        let v = dispatch(&Dearrow::new(), &mut h, &mut frame("dQw4w9WgXcQ"));
        assert_eq!(v, Verdict::Pass);
        assert!(h.urls.is_empty());
    }

    #[test]
    fn downvoted_original_and_unknown_videos_stay() {
        let bad = r#"{"titles":[{"title":"t","votes":-1}],
            "thumbnails":[{"timestamp":1,"votes":3,"original":true}]}"#;
        for answer in [
            (200, bad),
            (200, r#"{"titles":[],"thumbnails":[]}"#),
            (404, ""),
        ] {
            let mut h = Net::new(Ok(answer));
            let mut f = frame("dQw4w9WgXcQ");
            assert_eq!(dispatch(&Dearrow::new(), &mut h, &mut f), Verdict::Pass);
            assert_eq!(embed(&f).str_at(&["title"]), Some("WOW"));
        }
    }

    #[test]
    fn other_embeds_ask_nothing_and_failures_are_not_remembered() {
        let mut h = Net::new(Err("offline"));
        let p = Dearrow::new();
        let mut short = frame("short");
        dispatch(&p, &mut h, &mut short);
        assert!(h.urls.is_empty());
        dispatch(&p, &mut h, &mut frame("dQw4w9WgXcQ"));
        dispatch(&p, &mut h, &mut frame("dQw4w9WgXcQ"));
        assert_eq!(h.urls.len(), 2);
    }

    #[test]
    fn history_is_rewritten_through_a_chain_with_a_lookup_limit() {
        let chain = Chain::new(vec![Box::new(Dearrow::new())]);
        let mut h = Net::new(Ok((200, BRANDING)));
        let list: Vec<String> = (0..5).map(|i| message(&format!("video_____{i}"))).collect();
        let mut body = j(&format!("[{}]", list.join(",")));
        let out = chain.rest_response(&mut h, "GET", "/channels/1/messages", 200, &mut body);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(h.urls.len(), MAX_LOOKUPS);
    }

    #[test]
    fn etf_binaries_keep_their_shape() {
        let old = Val::Bin(b"x".to_vec());
        assert_eq!(shaped(&old, "y"), Val::Bin(b"y".to_vec()));
        assert_eq!(text(&old), Some("x"));
    }
}
