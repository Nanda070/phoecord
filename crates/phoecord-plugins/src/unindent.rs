//! Unindent: strips the indentation shared by every line of a code block you send
//! (Vencord: Unindent).
//!
//! Only fenced blocks (```` ``` ````) in `content` change: the opening line with the language
//! stays as it is, and inside the block the common leading spaces or tabs are removed. Text
//! outside code blocks, inline code and single-line blocks are never touched.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Verdict};

pub struct Unindent {
    manifest: Manifest,
}

impl Unindent {
    pub fn new() -> Unindent {
        Unindent {
            manifest: {
                let mut m = Manifest::new(
                    "unindent",
                    "Unindent",
                    env!("CARGO_PKG_VERSION"),
                    "Strip shared indentation from code blocks you send",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("Unindent".into());
                m
            },
        }
    }
}

impl Default for Unindent {
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

fn is_indent(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// The leading spaces and tabs of `line`.
fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches(is_indent).len()]
}

/// The longest common prefix of `a` and `b` (both are spaces and tabs only).
fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    &a[..n]
}

/// The body of one code block with the shared indentation removed. Blank lines do not count
/// towards the shared indentation and lose their whitespace.
fn unindent_block(body: &str) -> String {
    let shared = body
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(indent_of)
        .reduce(common_prefix)
        .unwrap_or("");
    if shared.is_empty() {
        return body.to_string();
    }
    body.split('\n')
        .map(|l| {
            l.strip_prefix(shared)
                .unwrap_or_else(|| l.trim_start_matches(is_indent))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` with every fenced code block unindented, or `None` if nothing changed.
pub fn unindent_text(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    while let Some(open) = rest.find("```") {
        out.push_str(&rest[..open + 3]);
        let after = &rest[open + 3..];
        let Some(close) = after.find("```") else {
            rest = after;
            break;
        };
        match after[..close].find('\n') {
            // ```code``` on one line: nothing to unindent.
            None => out.push_str(&after[..close + 3]),
            Some(nl) => {
                let body = &after[nl + 1..close];
                let new = unindent_block(body);
                changed |= new != body;
                out.push_str(&after[..=nl]);
                out.push_str(&new);
                out.push_str("```");
            }
        }
        rest = &after[close + 3..];
    }
    out.push_str(rest);
    changed.then_some(out)
}

impl Plugin for Unindent {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_message_send(method, route))
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
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
        let Some(content) = body.get_mut("content") else {
            return Verdict::Pass;
        };
        let Some(new) = content.as_str().and_then(unindent_text) else {
            return Verdict::Pass;
        };
        *content = content.str_like(&new);
        Verdict::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_core::Val;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    #[test]
    fn matches_send_and_edit_only() {
        assert!(is_message_send("POST", "/channels/1/messages"));
        assert!(is_message_send("PATCH", "/channels/1/messages/2"));
        assert!(!is_message_send("GET", "/channels/1/messages"));
        assert!(!is_message_send("POST", "/channels/1/typing"));
    }

    #[test]
    fn strips_shared_indentation() {
        assert_eq!(
            unindent_text("```rs\n    fn a() {\n        b();\n    }\n```").as_deref(),
            Some("```rs\nfn a() {\n    b();\n}\n```")
        );
        assert_eq!(
            unindent_text("\t\tx\n```\n\t\tx = 1\n\t\ty = 2\n```").as_deref(),
            Some("\t\tx\n```\nx = 1\ny = 2\n```"),
            "tabs work, text outside the block stays"
        );
    }

    #[test]
    fn blank_lines_and_an_indented_fence() {
        assert_eq!(
            unindent_text("```\n    a\n\n      b\n    ```").as_deref(),
            Some("```\na\n\n  b\n```")
        );
        assert_eq!(
            unindent_text("```\n    a\n  \n    b\n```").as_deref(),
            Some("```\na\n\nb\n```")
        );
    }

    #[test]
    fn every_block_on_its_own() {
        assert_eq!(
            unindent_text("one\n```\n  a\n```\ntwo\n```py\n    b\n```").as_deref(),
            Some("one\n```\na\n```\ntwo\n```py\nb\n```")
        );
    }

    #[test]
    fn leaves_the_rest_alone() {
        assert_eq!(unindent_text("no code here"), None);
        assert_eq!(
            unindent_text("```\na\n  b\n```"),
            None,
            "already flush left"
        );
        assert_eq!(unindent_text("inline ```  x  ``` code"), None);
        assert_eq!(unindent_text("```\n    unclosed"), None);
        assert_eq!(unindent_text("    indented, but not in a block"), None);
    }

    fn message(content: &str) -> Val {
        let mut body = Val::map();
        body.set("content", Val::Str(content.into()));
        body
    }

    #[test]
    fn chain_rewrites_sent_and_edited_messages() {
        let chain = Chain::new(vec![Box::new(Unindent::new())]);
        let mut host = NullHost::default();

        let mut body = message("```\n  a\n```");
        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
        assert_eq!(out, Outcome::Changed);
        assert_eq!(body.str_at(&["content"]), Some("```\na\n```"));

        let mut body = message("```\n  a\n```");
        let out = chain.rest_request(
            &mut host,
            "PATCH",
            "/channels/1/messages/2",
            Some(&mut body),
        );
        assert_eq!(out, Outcome::Changed);

        let mut body = message("hi");
        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
        assert_eq!(out, Outcome::Untouched);
    }
}
