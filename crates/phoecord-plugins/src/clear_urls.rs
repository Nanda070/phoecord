//! ClearURLs: strips tracking parameters from links in the messages you send (Vencord: ClearURLs).
//!
//! Only the query string of `http(s)://` links in `content` changes; the path, the fragment, every
//! other parameter and all other text stay byte for byte. Extra rules come from the `extra_rules`
//! setting: `"ref"` (any site), `"ref_*"` (prefix) or `"amazon.com:tag"` (one site and its
//! subdomains).

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

pub struct ClearUrls {
    manifest: Manifest,
}

impl ClearUrls {
    pub fn new() -> ClearUrls {
        ClearUrls {
            manifest: {
                let mut m = Manifest::new(
                    "clear_urls",
                    "ClearURLs",
                    env!("CARGO_PKG_VERSION"),
                    "Strip tracking parameters from links you send",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ClearURLs".into());
                m
            },
        }
    }
}

impl Default for ClearUrls {
    fn default() -> Self {
        Self::new()
    }
}

/// Parameters removed on every site. A trailing `*` matches a prefix.
const GLOBAL: &[&str] = &[
    "utm_*", "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "yclid",
    "twclid", "igshid", "igsh", "mc_cid", "mc_eid", "_ga", "_gl", "_hsenc", "_hsmi", "mkt_tok",
];

/// Parameters removed only on these sites and their subdomains.
const PER_SITE: &[(&str, &[&str])] = &[
    ("youtube.com", &["si", "pp", "feature"]),
    ("youtu.be", &["si", "feature"]),
    ("spotify.com", &["si"]),
    ("twitter.com", &["s", "t", "ref_src", "ref_url"]),
    ("x.com", &["s", "t", "ref_src", "ref_url"]),
    (
        "tiktok.com",
        &["_r", "_t", "is_from_webapp", "sender_device"],
    ),
    ("reddit.com", &["share_id", "rdt"]),
];

/// `POST /channels/{id}/messages` (send) and `PATCH /channels/{id}/messages/{id}` (edit).
fn is_message_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    matches!(
        (method, seg.as_slice()),
        ("POST", ["channels", _, "messages"]) | ("PATCH", ["channels", _, "messages", _])
    )
}

fn rule_matches(rule: &str, name: &str) -> bool {
    match rule.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == rule,
    }
}

/// `host` is `site` or one of its subdomains.
fn on_site(host: &str, site: &str) -> bool {
    host == site
        || host
            .strip_suffix(site)
            .is_some_and(|rest| rest.ends_with('.'))
}

fn is_tracking(host: &str, name: &str, extra: &[String]) -> bool {
    let name = name.to_ascii_lowercase();
    GLOBAL.iter().any(|r| rule_matches(r, &name))
        || PER_SITE.iter().any(|(site, rules)| {
            on_site(host, site) && rules.iter().any(|r| rule_matches(r, &name))
        })
        || extra.iter().any(|e| match e.split_once(':') {
            Some((site, rule)) => on_site(host, site) && rule_matches(rule, &name),
            None => rule_matches(e, &name),
        })
}

/// Lowercase host of `https://user@Host:443/path`, without the port.
fn host_of(base: &str) -> String {
    let after = base.split_once("://").map_or(base, |(_, a)| a);
    let authority = after.split('/').next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// The link without its tracking parameters, or `None` if it has none.
fn clean_url(url: &str, extra: &[String]) -> Option<String> {
    let (main, fragment) = url.split_at(url.find('#').unwrap_or(url.len()));
    let (base, query) = main.split_once('?')?;
    let host = host_of(base);
    let params: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = params
        .iter()
        .copied()
        .filter(|p| !is_tracking(&host, p.split('=').next().unwrap_or(p), extra))
        .collect();
    if kept.len() == params.len() {
        return None;
    }
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(fragment);
    Some(out)
}

/// Where a link that starts `tail` ends: at whitespace or `<>"\``, minus trailing punctuation,
/// Markdown and a `)` that closes a Markdown link rather than belonging to the URL.
fn url_end(tail: &str) -> usize {
    let mut end = tail
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
        .unwrap_or(tail.len());
    loop {
        let s = &tail[..end];
        let trim = match s.chars().last() {
            Some('.' | ',' | '!' | '?' | ';' | ':' | '\'' | '*' | '~' | '|' | ']') => true,
            Some(')') => s.matches('(').count() < s.matches(')').count(),
            _ => false,
        };
        if !trim {
            return end;
        }
        end -= 1;
    }
}

fn next_url(text: &str) -> Option<usize> {
    match (text.find("https://"), text.find("http://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// `text` with every link cleaned, or `None` if nothing changed.
pub fn clean_text(text: &str, extra: &[String]) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    while let Some(start) = next_url(rest) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = url_end(tail);
        let url = &tail[..end];
        match clean_url(url, extra) {
            Some(clean) => {
                out.push_str(&clean);
                changed = true;
            }
            None => out.push_str(url),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// The `extra_rules` setting, lowercased; missing or malformed entries are ignored.
fn extra_rules(host: &dyn Host) -> Vec<String> {
    host.setting("extra_rules")
        .map(|v| {
            v.as_list()
                .iter()
                .filter_map(Val::as_str)
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

impl Plugin for ClearUrls {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_message_send(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
        else {
            return Verdict::Pass;
        };
        if !is_message_send(method, route) {
            return Verdict::Pass;
        }
        let extra = extra_rules(host);
        let Some(content) = body.get_mut("content") else {
            return Verdict::Pass;
        };
        let Some(clean) = content.as_str().and_then(|s| clean_text(s, &extra)) else {
            return Verdict::Pass;
        };
        *content = content.str_like(&clean);
        Verdict::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn clean(text: &str) -> Option<String> {
        clean_text(text, &[])
    }

    #[test]
    fn matches_send_and_edit_only() {
        assert!(is_message_send("POST", "/channels/1/messages"));
        assert!(is_message_send("PATCH", "/channels/1/messages/2"));
        assert!(!is_message_send("GET", "/channels/1/messages"));
        assert!(!is_message_send("POST", "/channels/1/messages/2"));
        assert!(!is_message_send("PATCH", "/channels/1/messages"));
        assert!(!is_message_send("POST", "/channels/1/typing"));
    }

    #[test]
    fn strips_tracking_and_keeps_the_rest() {
        assert_eq!(
            clean("look https://example.com/a?utm_source=x&id=5&utm_medium=y ok").as_deref(),
            Some("look https://example.com/a?id=5 ok")
        );
        assert_eq!(
            clean("https://example.com/?fbclid=1").as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(
            clean("https://e.com/p?UTM_Campaign=a#top").as_deref(),
            Some("https://e.com/p#top")
        );
    }

    #[test]
    fn site_rules_apply_only_on_their_site() {
        assert_eq!(
            clean("https://youtu.be/abc?si=XYZ").as_deref(),
            Some("https://youtu.be/abc")
        );
        assert_eq!(
            clean("https://www.youtube.com/watch?v=abc&si=1&t=30").as_deref(),
            Some("https://www.youtube.com/watch?v=abc&t=30")
        );
        assert_eq!(
            clean("https://open.spotify.com/track/1?si=abc").as_deref(),
            Some("https://open.spotify.com/track/1")
        );
        assert_eq!(clean("https://example.com/?si=1&t=2"), None);
        assert_eq!(clean("https://notyoutube.com/?si=1"), None);
    }

    #[test]
    fn leaves_clean_text_alone() {
        assert_eq!(clean("no links here"), None);
        assert_eq!(clean("https://example.com/search?q=rust&page=2"), None);
        assert_eq!(clean("https://example.com/path"), None);
    }

    #[test]
    fn handles_punctuation_and_markdown() {
        assert_eq!(
            clean("see https://e.com/?utm_source=a.").as_deref(),
            Some("see https://e.com/.")
        );
        assert_eq!(
            clean("[x](https://e.com/?gclid=1)").as_deref(),
            Some("[x](https://e.com/)")
        );
        assert_eq!(
            clean("<https://e.com/?utm_x=1>").as_deref(),
            Some("<https://e.com/>")
        );
        assert_eq!(
            clean("https://en.wikipedia.org/wiki/Rust_(language)?utm_source=a").as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust_(language)")
        );
        assert_eq!(
            clean("два https://e.com/?fbclid=1 и https://e.com/?gclid=2").as_deref(),
            Some("два https://e.com/ и https://e.com/")
        );
    }

    #[test]
    fn extra_rules_from_settings() {
        let extra = vec!["ref".to_string(), "shop.com:tag".to_string()];
        assert_eq!(
            clean_text("https://e.com/?ref=a&id=1", &extra).as_deref(),
            Some("https://e.com/?id=1")
        );
        assert_eq!(
            clean_text("https://www.shop.com/p?tag=x&id=1", &extra).as_deref(),
            Some("https://www.shop.com/p?id=1")
        );
        assert_eq!(clean_text("https://e.com/?tag=x", &extra), None);
    }

    fn message(content: &str) -> Val {
        let mut body = Val::map();
        body.set("content", Val::Str(content.into()));
        body
    }

    #[test]
    fn chain_rewrites_sent_and_edited_messages() {
        let chain = Chain::new(vec![Box::new(ClearUrls::new())]);
        let mut host = NullHost::default();

        let mut body = message("https://e.com/?utm_source=a&id=1");
        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
        assert_eq!(out, Outcome::Changed);
        assert_eq!(body.str_at(&["content"]), Some("https://e.com/?id=1"));

        let mut body = message("https://e.com/?fbclid=1");
        let out = chain.rest_request(
            &mut host,
            "PATCH",
            "/channels/1/messages/2",
            Some(&mut body),
        );
        assert_eq!(out, Outcome::Changed);
        assert_eq!(body.str_at(&["content"]), Some("https://e.com/"));
    }

    #[test]
    fn chain_leaves_other_requests_alone() {
        let chain = Chain::new(vec![Box::new(ClearUrls::new())]);
        let mut host = NullHost::default();

        let mut body = message("https://e.com/?id=1");
        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
        assert_eq!(out, Outcome::Untouched);

        let mut body = message("https://e.com/?utm_source=a");
        let out = chain.rest_request(&mut host, "GET", "/channels/1/messages", Some(&mut body));
        assert_eq!(out, Outcome::Untouched);

        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", None);
        assert_eq!(out, Outcome::Untouched);
    }
}
