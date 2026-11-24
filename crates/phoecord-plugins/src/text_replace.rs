//! TextReplace: your own find/replace rules applied to the messages you send (Vencord: TextReplace).
//!
//! Ported from Vencord `src/plugins/textReplace/index.tsx` (GPL-3.0-or-later, Copyright (c) 2023
//! Vendicated and contributors; authors AutumnVN, TheKodeToad). `applyRules` and `stringToRegex`
//! are translated one to one; the settings editor and the rule tester are UI (Phase 2).
//!
//! Settings (`[plugins.text_replace]`), Vencord's names in snake_case. Each is a list of tables
//! with `find`, `replace`, `only_if_includes` (optional) and `name` (ignored):
//! - `string_rules`: plain text find/replace (every occurrence).
//! - `regex_rules`: `find` is a regex, written `pattern` or `/pattern/flags`. Without flags it is
//!   global (like Vencord); with flags it is global only when they contain `g`.
//!
//! Differences from Vencord, all caused by the regex engine: rules use the Rust `regex` crate, so
//! JavaScript-only syntax (look-around, back-references) makes a rule invalid; the flag `y` is
//! unsupported (rule invalid), `u` and `v` are accepted and ignored. An invalid rule is skipped and
//! logged once. `$1`, `$<name>`, `$&`, `` $` ``, `$'` and `$$` in `replace` work as in JavaScript.
//!
//! Vencord applies the rules on send only; this plugin also applies them to message edits, as the
//! task asked. Messages in Vencord's rule-sharing channel are left alone, as in Vencord.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::route::message_route_channel;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use regex::{Captures, Regex, RegexBuilder};
use std::collections::HashSet;
use std::sync::Mutex;

/// Vencord's `#textreplace-rules` channel: rules are shared there, applying them would be messy.
const RULES_CHANNEL: u64 = 1102784112584040479;

pub struct TextReplace {
    manifest: Manifest,
    /// Problems already logged.
    reported: Mutex<HashSet<String>>,
}

impl TextReplace {
    pub fn new() -> TextReplace {
        TextReplace {
            manifest: Manifest {
                id: "text_replace".into(),
                name: "TextReplace".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Replace text in the messages you send".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("TextReplace".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            reported: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for TextReplace {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/messages` (send) and `PATCH /channels/{id}/messages/{id}` (edit).
fn is_message_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    matches!(
        (method, seg.as_slice()),
        ("POST", ["channels", _, "messages"]) | ("PATCH", ["channels", _, "messages", _])
    )
}

/// One rule of either list.
#[derive(Debug, PartialEq)]
pub struct Rule {
    pub find: String,
    pub replace: String,
    pub only_if_includes: String,
}

fn rules(host: &dyn Host, key: &str) -> Vec<Rule> {
    let Some(list) = host.setting(key) else {
        return Vec::new();
    };
    let text = |r: &Val, k: &str| r.get(k).and_then(Val::as_str).unwrap_or("").to_string();
    list.as_list()
        .iter()
        .map(|r| Rule {
            find: text(r, "find"),
            // Vencord: `replace.replaceAll("\\n", "\n")`.
            replace: text(r, "replace").replace("\\n", "\n"),
            only_if_includes: text(r, "only_if_includes"),
        })
        .collect()
}

/// What a match looks like to a JavaScript replacement string.
struct Match<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    /// The regex and captures, `None` for a string pattern (which has no groups).
    caps: Option<(&'a Regex, &'a Captures<'a>)>,
}

/// JavaScript's replacement patterns: `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn`, `$<name>`. Anything
/// else after a `$` stays literal, as in JavaScript.
fn expand(rep: &str, out: &mut String, m: &Match) {
    let groups = m.caps.map_or(0, |(_, c)| c.len() - 1);
    let b = rep.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' || i + 1 >= b.len() {
            let ch = rep[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        match b[i + 1] {
            b'$' => {
                out.push('$');
                i += 2;
            }
            b'&' => {
                out.push_str(&m.text[m.start..m.end]);
                i += 2;
            }
            b'`' => {
                out.push_str(&m.text[..m.start]);
                i += 2;
            }
            b'\'' => {
                out.push_str(&m.text[m.end..]);
                i += 2;
            }
            d @ b'0'..=b'9' => {
                let one = (d - b'0') as usize;
                let two = b
                    .get(i + 2)
                    .filter(|c| c.is_ascii_digit())
                    .map(|c| one * 10 + (c - b'0') as usize);
                let (n, len) = match two {
                    Some(n) if (1..=groups).contains(&n) => (n, 3),
                    _ if (1..=groups).contains(&one) => (one, 2),
                    _ => {
                        out.push('$');
                        i += 1;
                        continue;
                    }
                };
                if let Some(g) = m.caps.and_then(|(_, c)| c.get(n)) {
                    out.push_str(g.as_str());
                }
                i += len;
            }
            b'<' => {
                let name = rep[i + 2..].find('>').map(|e| &rep[i + 2..i + 2 + e]);
                match (name, m.caps) {
                    (Some(name), Some((re, c)))
                        if re.capture_names().flatten().any(|n| n == name) =>
                    {
                        if let Some(g) = c.name(name) {
                            out.push_str(g.as_str());
                        }
                        i += name.len() + 3;
                    }
                    _ => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
}

/// JavaScript `text.replaceAll(find, replace)` for a string pattern.
fn replace_all_str(text: &str, find: &str, replace: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, found) in text.match_indices(find) {
        out.push_str(&text[last..at]);
        let m = Match {
            text,
            start: at,
            end: at + found.len(),
            caps: None,
        };
        expand(replace, &mut out, &m);
        last = m.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Vencord's `stringToRegex` split into pattern and flags: `pattern` or `/pattern/flags`; with no
/// `/flags` the flags are `g`.
fn split_regex(s: &str) -> (&str, &str) {
    let start = usize::from(s.starts_with('/') && s.len() > 1);
    let is_flags = |f: &str| f.chars().all(|c| "gimsuyv".contains(c));
    for (p, c) in s.char_indices() {
        if p > start && c == '/' && is_flags(&s[p + 1..]) {
            return (&s[start..p], &s[p + 1..]);
        }
    }
    (&s[start..], "g")
}

/// A compiled regex rule and whether it replaces every match.
fn compile(find: &str) -> Result<(Regex, bool), String> {
    let (pattern, flags) = split_regex(find);
    let mut b = RegexBuilder::new(pattern);
    for f in flags.chars() {
        match f {
            'i' => b.case_insensitive(true),
            'm' => b.multi_line(true),
            's' => b.dot_matches_new_line(true),
            'g' | 'u' | 'v' => &mut b,
            other => return Err(format!("flag `{other}` is not supported")),
        };
    }
    let re = b.build().map_err(|e| e.to_string())?;
    Ok((re, flags.contains('g')))
}

fn replace_regex(text: &str, re: &Regex, global: bool, replace: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in re.captures_iter(text) {
        let whole = caps.get(0).unwrap();
        out.push_str(&text[last..whole.start()]);
        let m = Match {
            text,
            start: whole.start(),
            end: whole.end(),
            caps: Some((re, &caps)),
        };
        expand(replace, &mut out, &m);
        last = whole.end();
        if !global {
            break;
        }
    }
    out.push_str(&text[last..]);
    out
}

fn skipped(r: &Rule, content: &str) -> bool {
    r.find.is_empty() || (!r.only_if_includes.is_empty() && !content.contains(&r.only_if_includes))
}

/// Vencord's `applyRules`. `invalid` receives `(find, reason)` for every regex rule that cannot be
/// compiled.
pub fn apply_rules(
    content: &str,
    string_rules: &[Rule],
    regex_rules: &[Rule],
    invalid: &mut dyn FnMut(&str, &str),
) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut content = content.to_string();
    for r in string_rules {
        if skipped(r, &content) {
            continue;
        }
        let padded = replace_all_str(&format!(" {content} "), &r.find, &r.replace);
        // `.replace(/^\s|\s$/g, "")`: one whitespace character off each end.
        let mut s = padded.as_str();
        if let Some(rest) = s.strip_prefix(char::is_whitespace) {
            s = rest;
        }
        if let Some(rest) = s.strip_suffix(char::is_whitespace) {
            s = rest;
        }
        content = s.to_string();
    }
    for r in regex_rules {
        if skipped(r, &content) {
            continue;
        }
        match compile(&r.find) {
            Ok((re, global)) => content = replace_regex(&content, &re, global, &r.replace),
            Err(e) => invalid(&r.find, &e),
        }
    }
    content.trim().to_string()
}

impl Plugin for TextReplace {
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
        if !is_message_send(method, route) || message_route_channel(route) == Some(RULES_CHANNEL) {
            return Verdict::Pass;
        }
        let Some(content) = body.get_mut("content") else {
            return Verdict::Pass;
        };
        let Some(old) = content.as_str() else {
            return Verdict::Pass;
        };
        let (strings, regexes) = (rules(host, "string_rules"), rules(host, "regex_rules"));
        let mut problems = Vec::new();
        let new = apply_rules(old, &strings, &regexes, &mut |find, why| {
            problems.push(format!(
                "TextReplace: invalid regex `{find}` skipped: {why}"
            ));
        });
        let changed = new != old;
        if changed {
            *content = content.str_like(&new);
        }
        let mut reported = self.reported.lock().unwrap();
        for p in problems {
            if reported.insert(p.clone()) {
                host.log(&p);
            }
        }
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
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn rule(find: &str, replace: &str) -> Rule {
        Rule {
            find: find.into(),
            replace: replace.into(),
            only_if_includes: String::new(),
        }
    }

    fn apply(text: &str, s: &[Rule], r: &[Rule]) -> String {
        apply_rules(text, s, r, &mut |_, _| {})
    }

    #[test]
    fn matches_send_and_edit_only() {
        assert!(is_message_send("POST", "/channels/1/messages"));
        assert!(is_message_send("PATCH", "/channels/1/messages/2"));
        assert!(!is_message_send("GET", "/channels/1/messages"));
        assert!(!is_message_send("POST", "/channels/1/messages/2"));
        assert!(!is_message_send("POST", "/channels/1/typing"));
    }

    #[test]
    fn string_rules_replace_every_occurrence() {
        let s = [rule("teh", "the"), rule("(c)", "©")];
        assert_eq!(apply("teh cat, teh dog (c)", &s, &[]), "the cat, the dog ©");
        // Empty find is skipped; only_if_includes gates a rule.
        let mut gated = rule("a", "b");
        gated.only_if_includes = "zzz".into();
        assert_eq!(apply("aaa", &[rule("", "x"), gated], &[]), "aaa");
        // Whitespace around the message is trimmed.
        assert_eq!(apply("  hi  ", &[], &[]), "hi");
        assert_eq!(apply("a;b", &[rule(";", "\n")], &[]), "a\nb");
        assert_eq!(apply("x", &[rule("x", "$&$&-$$")], &[]), "xx-$");
    }

    #[test]
    fn regex_rules_follow_vencords_flag_rule() {
        // No flags: global.
        assert_eq!(apply("a1 b22", &[], &[rule(r"\d+", "#")]), "a# b#");
        // `/…/i` has flags but no `g`: first match only, case-insensitive.
        assert_eq!(apply("Aa aA", &[], &[rule("/a/i", "x")]), "xa aA");
        assert_eq!(apply("Aa aA", &[], &[rule("/a/gi", "x")]), "xx xx");
        // Groups, named groups, and `$` that is not a group.
        assert_eq!(
            apply("john smith", &[], &[rule(r"(\w+) (\w+)", "$2, $1")]),
            "smith, john"
        );
        assert_eq!(
            apply(
                "2024-05",
                &[],
                &[rule(r"(?P<y>\d+)-(?P<m>\d+)", "$<m>/$<y>")]
            ),
            "05/2024"
        );
        assert_eq!(apply("a", &[], &[rule("a", "$5 $")]), "$5 $");
        // A slash inside the pattern that is not followed by flags belongs to the pattern.
        assert_eq!(apply("a/b", &[], &[rule("a/b", "x")]), "x");
    }

    #[test]
    fn invalid_regex_is_skipped_and_reported() {
        let mut seen = Vec::new();
        let out = apply_rules(
            "keep",
            &[],
            &[
                rule("(unclosed", "x"),
                rule("/a/y", "x"),
                rule("(?<=k)e", "x"),
            ],
            &mut |f, _| seen.push(f.to_string()),
        );
        assert_eq!(out, "keep");
        assert_eq!(seen, ["(unclosed", "/a/y", "(?<=k)e"]);
    }

    fn cfg() -> Cfg {
        Cfg::default()
            .with(
                "string_rules",
                j(r#"[{"name":"","find":"teh","replace":"the","only_if_includes":""}]"#),
            )
            .with(
                "regex_rules",
                j(r#"[{"find":"(bad","replace":"x"},{"find":"\\bcolour\\b","replace":"color"}]"#),
            )
    }

    #[test]
    fn chain_rewrites_send_and_edit_and_logs_invalid_once() {
        let chain = Chain::new(vec![Box::new(TextReplace::new())]);
        let mut h = cfg();
        let mut body = j(r#"{"content":"teh colour","nonce":"1"}"#);
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut body));
        assert_eq!(out, Outcome::Changed);
        assert_eq!(body.str_at(&["content"]), Some("the color"));
        assert_eq!(body.str_at(&["nonce"]), Some("1"));

        let mut edit = j(r#"{"content":"teh"}"#);
        let out = chain.rest_request(&mut h, "PATCH", "/channels/9/messages/5", Some(&mut edit));
        assert_eq!(out, Outcome::Changed);
        assert_eq!(edit.str_at(&["content"]), Some("the"));
        assert_eq!(h.logs.len(), 1, "{:?}", h.logs);

        let mut same = j(r#"{"content":"fine"}"#);
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut same));
        assert_eq!(out, Outcome::Untouched);
        // The rule-sharing channel is left alone.
        let mut shared = j(r#"{"content":"teh"}"#);
        let route = format!("/channels/{RULES_CHANNEL}/messages");
        let out = chain.rest_request(&mut h, "POST", &route, Some(&mut shared));
        assert_eq!(out, Outcome::Untouched);
    }
}
