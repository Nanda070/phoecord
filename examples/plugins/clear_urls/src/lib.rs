//! ClearURLs as a WASM plugin (Vencord: ClearURLs).
//!
//! The matching logic below is copied from the built-in plugin,
//! `crates/phoecord-plugins/src/clear_urls.rs` (by matvee, PR #42). Keep the two in step. Only the
//! wiring differs: it talks to the host through the WIT bindings instead of `Plugin` and `Val`.

wit_bindgen::generate!({
    path: "../../../wit",
    world: "plugin",
});

use phoecord::plugin::host;
use phoecord::plugin::types::{Event, EventKind, Layer, Manifest, Node, Outcome, Tree, Verdict};

struct ClearUrls;

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
fn clean_text(text: &str, extra: &[String]) -> Option<String> {
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

/// The `extra_rules` setting (a list of strings), lowercased; anything else is ignored.
fn extra_rules() -> Vec<String> {
    let Some(tree) = host::setting("extra_rules") else {
        return Vec::new();
    };
    let Some(Node::List(items)) = tree.nodes.first() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|&i| match tree.nodes.get(i as usize) {
            Some(Node::String(s)) => Some(s.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn pass() -> Outcome {
    Outcome {
        verdict: Verdict::Pass,
        payload: None,
        extra: Vec::new(),
    }
}

/// Index of the `content` string node of a message body, if it has one.
fn content_node(body: &Tree) -> Option<usize> {
    let Some(Node::Map(pairs)) = body.nodes.first() else {
        return None;
    };
    pairs
        .iter()
        .find_map(|&(k, v)| match body.nodes.get(k as usize) {
            Some(Node::String(key)) if key == "content" => Some(v as usize),
            _ => None,
        })
}

impl Guest for ClearUrls {
    fn manifest() -> Manifest {
        Manifest {
            id: "clear_urls_wasm".into(),
            name: "ClearURLs (WASM example)".into(),
            version: "0.1.0".into(),
            description: "Strips tracking parameters from links you send".into(),
            authors: vec!["matvee".into(), "Cheterin Group".into()],
            vencord: Some("ClearURLs".into()),
            layers: vec![Layer::Network],
            api: 1,
        }
    }

    fn wants(kind: EventKind) -> bool {
        matches!(&kind, EventKind::RestRequest(k) if is_message_send(&k.method, &k.route))
    }

    fn on_event(event: Event) -> Outcome {
        let Event::RestRequest(req) = event else {
            return pass();
        };
        if !is_message_send(&req.method, &req.route) {
            return pass();
        }
        let Some(mut body) = req.body else {
            return pass();
        };
        let Some(at) = content_node(&body) else {
            return pass();
        };
        let Some(Node::String(text)) = body.nodes.get(at) else {
            return pass();
        };
        let Some(clean) = clean_text(text, &extra_rules()) else {
            return pass();
        };
        body.nodes[at] = Node::String(clean);
        Outcome {
            verdict: Verdict::Changed,
            payload: Some(body),
            extra: Vec::new(),
        }
    }
}

export!(ClearUrls);
