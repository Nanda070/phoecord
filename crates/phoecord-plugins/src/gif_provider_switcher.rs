//! GifProviderSwitcher: Tenor or Giphy answer the GIF picker instead of Klipy (Vencord:
//! GifProviderSwitcher).
//!
//! Ported from Vencord `src/plugins/gifProviderSwitcher/{index.tsx,tenor.ts,giphy.ts}`
//! (GPL-3.0-or-later, Copyright (c) 2026 Vendicated and contributors; authors Lunascape, Ven).
//! Vencord patches the client's functions; here the same REST calls are answered before they
//! reach Discord: `GET /gifs/search`, `/gifs/suggest`, `/gifs/trending` (categories),
//! `/gifs/trending-gifs` and `POST /gifs/select` (Tenor's share registration; Giphy has none).
//! The answers have the shape Vencord hands the client, with Vencord's limits (100 search
//! results, 50 trending, 5 suggestions).
//!
//! Setting, with Vencord's name: `provider` = `tenor` (default), `giphy` or `klipy` (leave the
//! picker alone, as does any other value). The API keys are the ones Vencord ships (owner's
//! ruling, 2026-11-26); `tenorApiKey` / `giphyApiKey` (PhoeCord's own settings) are tried when a
//! provider refuses the shipped key.
//!
//! Differences: Vencord shows an error in the picker when the provider fails; here the call goes
//! on to Discord, and the provider is not asked again for [`PAUSE`] so that every keystroke does
//! not wait for a timeout. The provider selector in the picker, the wording of its placeholder,
//! the `/tenor` command and integration search are client UI (Phase 2).
//!
//! Privacy: the chosen provider receives the search text and the locale instead of Discord. The
//! search text is never logged. Unverified against a live client, including whether the client
//! may load pictures from the providers' media hosts.

use crate::state::Shared;
use phoecord_core::plugin::API_VERSION;
use phoecord_core::route::query_pairs;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const TENOR_HOST: &str = "api.tenor.com";
const GIPHY_HOST: &str = "api.giphy.com";
/// As in Vencord: the key of the GBoard app on iOS.
const TENOR_KEY: &str = "3Z0688EVWYKH";
/// As in Vencord: the key of the giphy.com website.
const GIPHY_KEY: &str = "Gc7131jiJuvI7IdN0HZ1D7nh0ow5BU6g";
const PAGE: usize = 50;
const SEARCH_LIMIT: usize = 100;
const TRENDING_LIMIT: usize = 50;
/// How long a provider is left alone after a failed call.
const PAUSE: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Provider {
    Tenor,
    Giphy,
}

/// The client calls this plugin answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Search,
    Suggest,
    Categories,
    TrendingGifs,
    Select,
}

fn call_of(method: &str, route: &str) -> Option<Call> {
    match (method, route) {
        ("GET", "/gifs/search") => Some(Call::Search),
        ("GET", "/gifs/suggest") => Some(Call::Suggest),
        ("GET", "/gifs/trending") => Some(Call::Categories),
        ("GET", "/gifs/trending-gifs") => Some(Call::TrendingGifs),
        ("POST", "/gifs/select") => Some(Call::Select),
        _ => None,
    }
}

fn chosen(host: &dyn Host) -> Option<Provider> {
    match host.setting("provider") {
        None => Some(Provider::Tenor),
        Some(v) => match v.as_str() {
            Some("tenor") => Some(Provider::Tenor),
            Some("giphy") => Some(Provider::Giphy),
            _ => None,
        },
    }
}

/// What the client's query says.
struct Query {
    q: String,
    locale: String,
    /// The client asks for WebP (Linux); otherwise it plays video.
    webp: bool,
}

fn client_query(raw: Option<String>) -> Query {
    let mut query = Query {
        q: String::new(),
        locale: "en-US".into(),
        webp: false,
    };
    for (key, value) in query_pairs(raw.as_deref().unwrap_or_default()) {
        match key.as_str() {
            "q" => query.q = value,
            "locale" if !value.is_empty() => query.locale = value,
            "media_format" => query.webp = value == "tinywebp",
            _ => {}
        }
    }
    query
}

/// Giphy takes two-letter codes, plus `zh-CN` and `zh-TW`.
fn giphy_lang(locale: &str) -> String {
    let l = locale.to_ascii_lowercase();
    if l.starts_with("zh") {
        let traditional = l.contains("tw") || l.contains("hant");
        return if traditional { "zh-TW" } else { "zh-CN" }.into();
    }
    l.split('-').next().unwrap_or_default().to_string()
}

/// A query component as `URLSearchParams` writes it.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url(provider: Provider, path: &str, key: &str, locale: &str, params: &[(&str, &str)]) -> String {
    let (host, key_name, lang_name, lang) = match provider {
        Provider::Tenor => (
            TENOR_HOST,
            "key",
            "locale",
            locale.replacen('-', "_", 1).to_ascii_lowercase(),
        ),
        Provider::Giphy => (GIPHY_HOST, "api_key", "lang", giphy_lang(locale)),
    };
    let mut url = format!(
        "https://{host}/v1{path}?{key_name}={}&{lang_name}={}",
        encode(key),
        encode(&lang)
    );
    for (k, v) in params {
        url.push_str(&format!("&{k}={}", encode(v)));
    }
    url
}

fn text<'a>(v: &'a Val, path: &[&str]) -> Option<&'a str> {
    v.str_at(path).filter(|s| !s.is_empty())
}

/// A Tenor result in the shape of a Discord GIF; `None` when a needed part is missing.
fn tenor_gif(item: &Val, webp: bool) -> Option<Value> {
    let media = item.get("media")?.as_list().first()?;
    let shown = media.get(if webp { "webp" } else { "tinywebm" })?;
    let dims = shown.get("dims")?.as_list();
    Some(json!({
        "id": text(item, &["id"])?,
        "title": text(item, &["title"]).or_else(|| text(item, &["h1_title"])).unwrap_or_default(),
        "url": text(item, &["itemurl"])?,
        "gif_src": text(media, &["gif", "url"])?,
        "src": text(shown, &["url"])?,
        "width": dims.first()?.as_i64()?,
        "height": dims.get(1)?.as_i64()?,
        "preview": text(shown, &["preview"]).unwrap_or_default(),
    }))
}

/// A Giphy result in the shape of a Discord GIF; `None` when a needed part is missing.
fn giphy_gif(item: &Val, webp: bool) -> Option<Value> {
    let original = text(item, &["images", "original", "url"])?;
    let shown = item.at(&["images", "fixed_height"])?;
    let number = |key: &str| text(shown, &[key])?.parse::<i64>().ok();
    Some(json!({
        "id": text(item, &["id"])?,
        "title": item.str_at(&["title"]).unwrap_or_default(),
        "url": text(item, &["url"])?,
        "gif_src": original,
        "src": text(shown, &[if webp { "webp" } else { "mp4" }])?,
        "width": number("width")?,
        "height": number("height")?,
        "preview": text(item, &["images", "fixed_height_still", "url"]).unwrap_or(original),
    }))
}

#[derive(Default)]
struct State {
    paused_until: HashMap<Provider, Instant>,
    /// Categories answers, kept as Vencord keeps them.
    categories: HashMap<(Provider, String), Value>,
}

pub struct GifProviderSwitcher {
    manifest: Manifest,
    state: Shared<State>,
}

/// One conversation with a provider on behalf of one client call.
struct Api<'a> {
    host: &'a mut dyn Host,
    provider: Provider,
    locale: String,
}

impl Api<'_> {
    /// The JSON answer of `path`, with the shipped key and then, if that one is refused, the
    /// user's. Errors name no URL: it holds the search text.
    fn get(&mut self, path: &str, params: &[(&str, &str)]) -> Result<Val, String> {
        let (shipped, setting) = match self.provider {
            Provider::Tenor => (TENOR_KEY, "tenorApiKey"),
            Provider::Giphy => (GIPHY_KEY, "giphyApiKey"),
        };
        let own = self
            .host
            .setting(setting)
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|k| !k.is_empty());
        let mut refused = String::new();
        for key in std::iter::once(shipped).chain(own.as_deref()) {
            let url = url(self.provider, path, key, &self.locale, params);
            let answer = self
                .host
                .fetch("GET", &url, None)
                .map_err(|_| "no answer".to_string())?;
            match answer.status {
                200 => return answer.body.ok_or_else(|| "not a JSON answer".into()),
                s @ (401 | 403 | 429) => refused = format!("status {s}"),
                s => return Err(format!("status {s}")),
            }
        }
        Err(refused)
    }

    /// Up to `limit` GIFs without repeats, paged as Vencord pages.
    fn gifs(&mut self, search: Option<&str>, limit: usize, webp: bool) -> Result<Value, String> {
        let mut seen = HashSet::new();
        let mut gifs = Vec::new();
        // Results taken from the provider, including those that cannot be shown.
        let mut taken = 0;
        let mut pos = String::new();
        let mut offset = 0;
        while taken < limit && offset < limit {
            let count = (limit - taken.max(offset)).min(PAGE).to_string();
            let mut params: Vec<(&str, &str)> = Vec::new();
            if let Some(q) = search {
                params.push(("q", q));
            }
            params.push(("limit", &count));
            let skip = offset.to_string();
            let (path, list) = match self.provider {
                Provider::Tenor => {
                    if !pos.is_empty() {
                        params.push(("pos", &pos));
                    }
                    let path = if search.is_some() {
                        "/search"
                    } else {
                        "/trending"
                    };
                    (path, "results")
                }
                Provider::Giphy => {
                    params.push(("offset", &skip));
                    let path = if search.is_some() {
                        "/gifs/search"
                    } else {
                        "/gifs/trending"
                    };
                    (path, "data")
                }
            };
            let answer = self.get(path, &params)?;
            let page = answer.get(list).map(Val::as_list).unwrap_or_default();
            let before = taken;
            for item in page {
                if taken < limit
                    && let Some(id) = text(item, &["id"])
                    && seen.insert(id.to_string())
                {
                    taken += 1;
                    gifs.extend(match self.provider {
                        Provider::Tenor => tenor_gif(item, webp),
                        Provider::Giphy => giphy_gif(item, webp),
                    });
                }
            }
            match self.provider {
                // Tenor says where the next page starts.
                Provider::Tenor => match text(&answer, &["next"]) {
                    Some(next) if next != pos && taken > before => pos = next.to_string(),
                    _ => break,
                },
                // Giphy pages by offset; a short or empty page is the last one.
                Provider::Giphy => {
                    if page.len() < PAGE {
                        break;
                    }
                    offset += PAGE;
                }
            }
        }
        Ok(Value::Array(gifs))
    }

    fn suggestions(&mut self, q: &str) -> Result<Value, String> {
        let params = [("q", q), ("limit", "5")];
        let words: Vec<Value> = match self.provider {
            Provider::Tenor => {
                let answer = self.get("/search_suggestions", &params)?;
                let list = answer.get("results").map(Val::as_list).unwrap_or_default();
                list.iter()
                    .filter_map(Val::as_str)
                    .map(Value::from)
                    .collect()
            }
            Provider::Giphy => {
                let answer = self.get("/gifs/search/tags", &params)?;
                let list = answer.get("data").map(Val::as_list).unwrap_or_default();
                list.iter()
                    .filter_map(|t| t.str_at(&["name"]))
                    .map(Value::from)
                    .collect()
            }
        };
        Ok(Value::Array(words))
    }

    /// `{categories: [{name, src}], gifs: [{src}]}`, the second being the trending preview.
    fn categories(&mut self) -> Result<Value, String> {
        let (answer, list, name, src): (Val, &str, &str, &[&str]) = match self.provider {
            Provider::Tenor => (
                self.get("/categories", &[("type", "featured")])?,
                "tags",
                "searchterm",
                &["image"],
            ),
            Provider::Giphy => (
                self.get("/gifs/categories", &[])?,
                "data",
                "name",
                &["gif", "images", "fixed_height", "url"],
            ),
        };
        let categories: Vec<Value> = answer
            .get(list)
            .map(Val::as_list)
            .unwrap_or_default()
            .iter()
            .filter_map(|c| Some(json!({ "name": text(c, &[name])?, "src": text(c, src)? })))
            .collect();
        let Some(first) = categories.first() else {
            return Err("no categories".into());
        };
        let preview = json!({ "src": first["src"] });
        Ok(json!({ "categories": categories, "gifs": [preview] }))
    }

    /// Tell Tenor which result was sent.
    fn share(&mut self, id: &str, q: &str) -> Result<(), String> {
        match self.provider {
            Provider::Tenor => self
                .get("/registershare", &[("id", id), ("q", q)])
                .map(|_| ()),
            Provider::Giphy => Ok(()),
        }
    }
}

impl GifProviderSwitcher {
    pub fn new() -> GifProviderSwitcher {
        GifProviderSwitcher {
            manifest: Manifest {
                id: "gif_provider_switcher".into(),
                name: "GifProviderSwitcher".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Tenor or Giphy for GIF search instead of Klipy".into(),
                authors: vec!["Lunascape".into(), "Ven".into()],
                vencord: Some("GifProviderSwitcher".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: vec![TENOR_HOST.into(), GIPHY_HOST.into()],
            },
            state: Shared::default(),
        }
    }

    fn paused(&self, provider: Provider) -> bool {
        self.state
            .lock()
            .paused_until
            .get(&provider)
            .is_some_and(|t| Instant::now() < *t)
    }

    /// The local answer to a client call; `None` lets the call go on to Discord.
    fn answer(
        &self,
        host: &mut dyn Host,
        provider: Provider,
        call: Call,
        body: Option<&Val>,
    ) -> Option<Verdict> {
        let query = client_query(host.request_query());
        let mut api = Api {
            host,
            provider,
            locale: query.locale.clone(),
        };
        if call == Call::Select {
            // Vencord never tells Discord about a pick from another provider.
            let field = |key: &str| body.and_then(|b| b.str_at(&[key])).unwrap_or_default();
            let id = field("id");
            if !id.is_empty()
                && !self.paused(provider)
                && let Err(why) = api.share(id, field("q"))
            {
                api.host.log(&format!("{provider:?} share: {why}"));
            }
            return Some(Verdict::Reply {
                status: 204,
                body: None,
            });
        }
        if matches!(call, Call::Search | Call::Suggest) && query.q.is_empty() {
            return None;
        }
        let key = (provider, query.locale.clone());
        if call == Call::Categories
            && let Some(known) = self.state.lock().categories.get(&key)
        {
            return Some(reply(known.clone()));
        }
        if self.paused(provider) {
            return None;
        }
        let result = match call {
            Call::Search => api.gifs(Some(&query.q), SEARCH_LIMIT, query.webp),
            Call::TrendingGifs => api.gifs(None, TRENDING_LIMIT, query.webp),
            Call::Suggest => api.suggestions(&query.q),
            Call::Categories => api.categories(),
            Call::Select => return None,
        };
        match result {
            Ok(value) => {
                if call == Call::Categories {
                    self.state.lock().categories.insert(key, value.clone());
                }
                Some(reply(value))
            }
            Err(why) => {
                api.host.log(&format!("{provider:?}: {why}"));
                self.state
                    .lock()
                    .paused_until
                    .insert(provider, Instant::now() + PAUSE);
                None
            }
        }
    }
}

fn reply(body: Value) -> Verdict {
    Verdict::Reply {
        status: 200,
        body: Some(Val::from_json(body)),
    }
}

impl Default for GifProviderSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for GifProviderSwitcher {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if call_of(method, route).is_some())
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestRequest {
            method,
            route,
            body,
        } = event
        else {
            return Verdict::Pass;
        };
        let (Some(call), Some(provider)) = (call_of(method, route), chosen(host)) else {
            return Verdict::Pass;
        };
        self.answer(host, provider, call, body.as_deref())
            .unwrap_or(Verdict::Pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_core::plugin::HostResponse;

    /// [`Cfg`] plus a query and a `fetch` that answers from a queue (the last answer repeats)
    /// and records the URLs.
    #[derive(Default)]
    struct Net {
        cfg: Cfg,
        query: Option<String>,
        answers: Vec<Result<(u16, String), String>>,
        urls: Vec<String>,
    }

    impl Net {
        fn new(query: &str, answers: &[Result<(u16, &str), &str>]) -> Net {
            Net {
                query: Some(query.into()),
                answers: answers
                    .iter()
                    .map(|a| a.map(|(s, b)| (s, b.to_string())).map_err(str::to_string))
                    .collect(),
                ..Net::default()
            }
        }

        fn set(mut self, key: &str, value: &str) -> Net {
            self.cfg = self.cfg.with(key, Val::from_json(json!(value)));
            self
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
        fn log(&mut self, message: &str) {
            self.cfg.log(message);
        }
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, _: u64) -> Vec<u64> {
            Vec::new()
        }
        fn channel_guild(&self, _: u64) -> Option<u64> {
            None
        }
        fn request_query(&self) -> Option<String> {
            self.query.clone()
        }
        fn fetch(&mut self, _: &str, url: &str, _: Option<&Val>) -> Result<HostResponse, String> {
            self.urls.push(url.into());
            let answer = if self.answers.len() > 1 {
                self.answers.remove(0)
            } else {
                self.answers[0].clone()
            };
            let (status, body) = answer?;
            Ok(HostResponse {
                status,
                body: (!body.is_empty()).then(|| j(&body)),
                text: None,
                bytes: None,
                headers: Vec::new(),
            })
        }
    }

    const TENOR: &str = r#"{"results":[{"id":"1","title":"","h1_title":"Cat","itemurl":"i",
        "media":[{"gif":{"url":"g"},
        "tinywebm":{"url":"v","dims":[10,20],"preview":"p"},
        "webp":{"url":"w","dims":[1,2],"preview":"q"}}]},
        {"id":"broken"}],"next":""}"#;

    const GIPHY: &str = r#"{"data":[{"id":"a","title":"Dog","url":"u","images":{
        "original":{"url":"o"},
        "fixed_height":{"mp4":"m","webp":"w","width":"30","height":"40"}}}]}"#;

    fn run(p: &GifProviderSwitcher, h: &mut Net, method: &str, route: &str) -> Verdict {
        p.on_event(
            h,
            Event::RestRequest {
                method,
                route,
                body: None,
            },
        )
    }

    fn body(v: Verdict) -> Val {
        match v {
            Verdict::Reply {
                status: 200,
                body: Some(b),
            } => b,
            other => panic!("not an answer: {other:?}"),
        }
    }

    #[test]
    fn only_the_gif_calls_are_wanted() {
        let p = GifProviderSwitcher::new();
        let wants = |method, route| p.wants(&EventKind::RestRequest { method, route });
        assert!(wants("GET", "/gifs/search"));
        assert!(wants("POST", "/gifs/select"));
        assert!(!wants("GET", "/gifs/select"));
        assert!(!wants("GET", "/gifs/other"));
        assert_eq!(p.manifest().fetch, ["api.tenor.com", "api.giphy.com"]);
    }

    #[test]
    fn tenor_search_is_the_default_and_has_discords_shape() {
        let mut h = Net::new(
            "q=cat+%26+dog&locale=pt-BR&media_format=webm",
            &[Ok((200, TENOR))],
        );
        let out = body(run(
            &GifProviderSwitcher::new(),
            &mut h,
            "GET",
            "/gifs/search",
        ));
        assert_eq!(
            out,
            j(
                r#"[{"id":"1","title":"Cat","url":"i","gif_src":"g","src":"v",
                "width":10,"height":20,"preview":"p"}]"#
            )
        );
        assert_eq!(
            h.urls,
            [format!(
                "https://api.tenor.com/v1/search?key={TENOR_KEY}&locale=pt_br&q=cat+%26+dog&limit=50"
            )]
        );
        let mut h = Net::new("q=cat&media_format=tinywebp", &[Ok((200, TENOR))]);
        let out = body(run(
            &GifProviderSwitcher::new(),
            &mut h,
            "GET",
            "/gifs/search",
        ));
        assert_eq!(out.as_list()[0].str_at(&["src"]), Some("w"));
    }

    #[test]
    fn giphy_trending_and_languages() {
        let mut h = Net::new("locale=zh-TW", &[Ok((200, GIPHY))]).set("provider", "giphy");
        let p = GifProviderSwitcher::new();
        let out = body(run(&p, &mut h, "GET", "/gifs/trending-gifs"));
        assert_eq!(
            out,
            j(
                r#"[{"id":"a","title":"Dog","url":"u","gif_src":"o","src":"m",
                "width":30,"height":40,"preview":"o"}]"#
            )
        );
        assert_eq!(
            h.urls,
            [format!(
                "https://api.giphy.com/v1/gifs/trending?api_key={GIPHY_KEY}&lang=zh-TW&limit=50&offset=0"
            )]
        );
        assert_eq!(giphy_lang("zh-CN"), "zh-CN");
        assert_eq!(giphy_lang("en-GB"), "en");
    }

    fn page(from: usize, count: usize, next: &str) -> String {
        let items: Vec<String> = (from..from + count)
            .map(|i| format!(r#"{{"id":"{i}"}}"#))
            .collect();
        let list = items.join(",");
        format!(r#"{{"results":[{list}],"data":[{list}],"next":"{next}"}}"#)
    }

    #[test]
    fn search_pages_up_to_a_hundred_results() {
        for provider in ["tenor", "giphy"] {
            let (first, second) = (page(0, 50, "50"), page(50, 50, "100"));
            let mut h =
                Net::new("q=x", &[Ok((200, &first)), Ok((200, &second))]).set("provider", provider);
            body(run(
                &GifProviderSwitcher::new(),
                &mut h,
                "GET",
                "/gifs/search",
            ));
            assert_eq!(h.urls.len(), 2, "{provider}");
            let marker = if provider == "tenor" {
                "&pos=50"
            } else {
                "&offset=50"
            };
            assert!(h.urls[1].contains(marker), "{}", h.urls[1]);
        }
        // A page that repeats what is already there ends the Tenor paging.
        let same = page(0, 50, "50");
        let again = page(0, 50, "100");
        let mut h = Net::new("q=x", &[Ok((200, &same)), Ok((200, &again))]);
        body(run(
            &GifProviderSwitcher::new(),
            &mut h,
            "GET",
            "/gifs/search",
        ));
        assert_eq!(h.urls.len(), 2);
    }

    #[test]
    fn suggestions_and_categories() {
        let p = GifProviderSwitcher::new();
        let mut h = Net::new("q=ca", &[Ok((200, r#"{"results":["cat","car"]}"#))]);
        assert_eq!(
            body(run(&p, &mut h, "GET", "/gifs/suggest")),
            j(r#"["cat","car"]"#)
        );
        assert!(h.urls[0].contains("/search_suggestions?"));

        let tags = r#"{"tags":[{"searchterm":"hi","image":"h.gif"}]}"#;
        let mut h = Net::new("locale=en-US", &[Ok((200, tags))]);
        let want = j(r#"{"categories":[{"name":"hi","src":"h.gif"}],"gifs":[{"src":"h.gif"}]}"#);
        assert_eq!(body(run(&p, &mut h, "GET", "/gifs/trending")), want);
        assert_eq!(body(run(&p, &mut h, "GET", "/gifs/trending")), want);
        assert_eq!(h.urls.len(), 1);

        let cats = r#"{"data":[{"name":"no picture"},
            {"name":"yo","gif":{"images":{"fixed_height":{"url":"y.gif"}}}}]}"#;
        let mut h = Net::new("", &[Ok((200, cats))]).set("provider", "giphy");
        let out = body(run(&p, &mut h, "GET", "/gifs/trending"));
        assert_eq!(out.get("categories").unwrap().as_list().len(), 1);
    }

    #[test]
    fn klipy_unknown_values_and_empty_searches_go_to_discord() {
        for provider in ["klipy", "Tenor", ""] {
            let mut h = Net::new("q=cat", &[Err("unused")]).set("provider", provider);
            for route in ["/gifs/search", "/gifs/trending"] {
                let v = run(&GifProviderSwitcher::new(), &mut h, "GET", route);
                assert_eq!(v, Verdict::Pass);
            }
            let v = run(&GifProviderSwitcher::new(), &mut h, "POST", "/gifs/select");
            assert_eq!(v, Verdict::Pass);
            assert!(h.urls.is_empty());
        }
        let mut h = Net::new("q=", &[Err("unused")]);
        let v = run(&GifProviderSwitcher::new(), &mut h, "GET", "/gifs/search");
        assert_eq!(v, Verdict::Pass);
        assert!(h.urls.is_empty());
    }

    #[test]
    fn a_failure_goes_to_discord_pauses_the_provider_and_logs_no_query() {
        for answer in [Err("https://api.tenor.com/?q=secret"), Ok((503, ""))] {
            let p = GifProviderSwitcher::new();
            let mut h = Net::new("q=secret", &[answer]);
            assert_eq!(run(&p, &mut h, "GET", "/gifs/search"), Verdict::Pass);
            assert_eq!(run(&p, &mut h, "GET", "/gifs/search"), Verdict::Pass);
            assert_eq!(h.urls.len(), 1);
            assert!(!h.cfg.logs.is_empty());
            assert!(h.cfg.logs.iter().all(|l| !l.contains("secret")));
            // The other provider is not paused.
            h = h.set("provider", "giphy");
            run(&p, &mut h, "GET", "/gifs/search");
            assert_eq!(h.urls.len(), 2);
        }
    }

    #[test]
    fn the_users_key_is_tried_when_the_shipped_one_is_refused() {
        let mut h =
            Net::new("q=cat", &[Ok((403, "")), Ok((200, TENOR))]).set("tenorApiKey", "mine");
        body(run(
            &GifProviderSwitcher::new(),
            &mut h,
            "GET",
            "/gifs/search",
        ));
        assert!(h.urls[0].contains(TENOR_KEY));
        assert!(h.urls[1].contains("key=mine&"));
        // Without one the call goes on to Discord.
        let mut h = Net::new("q=cat", &[Ok((429, ""))]);
        let v = run(&GifProviderSwitcher::new(), &mut h, "GET", "/gifs/search");
        assert_eq!(v, Verdict::Pass);
        assert_eq!(h.urls.len(), 1);
    }

    #[test]
    fn a_pick_is_registered_with_tenor_and_kept_from_discord() {
        let swallowed = Verdict::Reply {
            status: 204,
            body: None,
        };
        for (provider, calls) in [("tenor", 1), ("giphy", 0)] {
            let mut h = Net::new("", &[Ok((200, "{}"))]).set("provider", provider);
            let mut pick = j(r#"{"id":"42","q":"cat"}"#);
            let v = GifProviderSwitcher::new().on_event(
                &mut h,
                Event::RestRequest {
                    method: "POST",
                    route: "/gifs/select",
                    body: Some(&mut pick),
                },
            );
            assert_eq!(v, swallowed);
            assert_eq!(h.urls.len(), calls);
            if calls == 1 {
                assert!(h.urls[0].contains("/registershare?"));
                assert!(h.urls[0].ends_with("&id=42&q=cat"));
            }
        }
    }
}
