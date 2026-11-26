//! CustomCommands: your own text snippets ("tags") with arguments, sent with a command (Vencord:
//! CustomCommands, formerly MessageTags).
//!
//! Ported from Vencord `src/plugins/customCommands/` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors Ven, Luna). A tag is a name and a message. The message
//! may hold arguments `{{name}}` or `{{name=default}}`; `\n` in it becomes a new line. Vencord
//! registers every tag as its own slash command and manages them with `/tags create | list |
//! delete` (create opens a modal).
//!
//! PhoeCord's commands are fixed subcommands of `/phoecord`, so here:
//!
//! - `/phoecord tags action:create name:<tag> message:<text>` makes or overwrites a tag (the
//!   argument name `ephemeral` is reserved, as in Vencord's modal);
//! - `/phoecord tags action:list` and `action:delete name:<tag>`, with Vencord's answers;
//! - `/phoecord tag name:<tag> [args:"a=1; b=2"] [ephemeral:true]` fills the arguments (typed
//!   value, else the default, else the `{{…}}` stays) and sends the result to the channel with
//!   `POST /channels/{id}/messages`, or with `ephemeral` shows it only to you.
//!
//! Tags live in the plugin's kv store under `tagsList` (Vencord's setting name), as
//! `{ "<name>": { "name": …, "message": … } }`. Not ported: answering the reply you were writing.

use crate::commands::{Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer};
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "custom_commands";
const KV_TAGS: &str = "tagsList";
/// Vencord's preview length in `/tags list`.
const PREVIEW: usize = 72;

pub struct CustomCommands {
    manifest: Manifest,
    kept: Kept,
}

impl CustomCommands {
    pub fn new() -> CustomCommands {
        CustomCommands {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "CustomCommands",
                    env!("CARGO_PKG_VERSION"),
                    "Your own text snippets with arguments, sent with a command",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("CustomCommands".into());
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord tags` and `/phoecord tag`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        vec![
            kept_answer(PLUGIN_ID, "tags", "Manage all custom commands", &self.kept)
                .option("action", "create, list or delete", true)
                .option("name", "create and delete: the name of the tag", false)
                .option(
                    "message",
                    "create: the text; {{arg}} or {{arg=default}} for arguments, \\n for a new line",
                    false,
                )
                .boxed(),
            kept_answer(PLUGIN_ID, "tag", "Send one of your tags", &self.kept)
                .option("name", "The name of the tag", true)
                .option("args", "Arguments: name=value; other=value", false)
                .option("ephemeral", "true: only you see it", false)
                .boxed(),
        ]
    }
}

impl Default for CustomCommands {
    fn default() -> Self {
        Self::new()
    }
}

/// The `{{…}}` arguments of a message: `(lower-case name, default)`, first one of a name wins
/// (Vencord's `parseTagArguments`).
pub fn arguments(message: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for inner in placeholders(message).into_iter().map(|(_, inner)| inner) {
        let (name, default) = split_arg(inner);
        if name.is_empty() || out.iter().any(|(n, _)| *n == name) {
            continue;
        }
        out.push((name, default));
    }
    out
}

/// Every `{{…}}` (non-greedy, at least one character inside) as `(byte range, inside)`.
fn placeholders(message: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = message[from..].find("{{").map(|i| from + i) {
        let inner_start = open + 2;
        // `.+?` needs one character before the closing braces.
        let Some(close) = message
            .get(inner_start + 1..)
            .and_then(|s| s.find("}}"))
            .map(|i| inner_start + 1 + i)
        else {
            break;
        };
        out.push((open..close + 2, &message[inner_start..close]));
        from = close + 2;
    }
    out
}

/// `name = default` → (`name` lower-cased, `Some(default)`); no `=` → no default.
fn split_arg(inner: &str) -> (String, Option<String>) {
    let mut parts = inner.split('=').map(str::trim);
    let name = parts.next().unwrap_or("").to_lowercase();
    (name, parts.next().map(str::to_string))
}

/// `a=1; b = two` as pairs (names lower-cased).
fn typed_args(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            let k = k.trim().to_lowercase();
            (!k.is_empty()).then(|| (k, v.trim().to_string()))
        })
        .collect()
}

/// The message with its arguments filled (Vencord's `execute`).
pub fn fill(message: &str, typed: &[(String, String)]) -> String {
    let mut out = String::with_capacity(message.len());
    let mut last = 0;
    for (range, inner) in placeholders(message) {
        out.push_str(&message[last..range.start]);
        let (name, default) = split_arg(inner);
        let value = typed
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
            .or(default);
        out.push_str(value.as_deref().unwrap_or(&message[range.clone()]));
        last = range.end;
    }
    out.push_str(&message[last..]);
    out.replace("\\n", "\n")
}

fn tags(host: &dyn Host) -> Vec<(String, String)> {
    let Some(Val::Map(fields)) = host.kv_get(KV_TAGS) else {
        return Vec::new();
    };
    fields
        .iter()
        .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.str_at(&["message"])?.to_string())))
        .collect()
}

fn save(host: &mut dyn Host, tags: &[(String, String)]) {
    let map = tags
        .iter()
        .map(|(name, message)| {
            let mut tag = Val::map();
            tag.set("name", Val::Str(name.clone()));
            tag.set("message", Val::Str(message.clone()));
            (Val::Str(name.clone()), tag)
        })
        .collect();
    host.kv_set(KV_TAGS, Val::Map(map));
}

fn manage(host: &mut dyn Host, inv: &Invocation) -> String {
    let name = inv.get("name").unwrap_or("").to_string();
    let mut all = tags(host);
    match inv
        .get("action")
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "list" => {
            let lines: Vec<String> = all
                .iter()
                .map(|(name, message)| {
                    let preview: String = message.chars().take(PREVIEW).collect();
                    let more = if message.chars().count() > PREVIEW {
                        "..."
                    } else {
                        ""
                    };
                    format!("`{name}`: {}{more}", preview.replace("\\n", " "))
                })
                .collect();
            if lines.is_empty() {
                host.t("custom_commands-none", &[])
            } else {
                lines.join("\n")
            }
        }
        "delete" => {
            let before = all.len();
            all.retain(|(n, _)| *n != name);
            if all.len() == before {
                return host.t("custom_commands-missing", &[("name", &name)]);
            }
            save(host, &all);
            host.t("custom_commands-deleted", &[("name", &name)])
        }
        "create" => {
            let Some(message) = inv.get("message").map(str::to_string) else {
                return host.t("custom_commands-need-name-and-message", &[]);
            };
            if name.is_empty() {
                return host.t("custom_commands-need-name-and-message", &[]);
            }
            if arguments(&message).iter().any(|(n, _)| n == "ephemeral") {
                return host.t("custom_commands-reserved", &[]);
            }
            let existed = all.iter().any(|(n, _)| *n == name);
            all.retain(|(n, _)| *n != name);
            all.push((name.clone(), message));
            save(host, &all);
            host.t(
                if existed {
                    "custom_commands-overwritten"
                } else {
                    "custom_commands-created"
                },
                &[("name", &name)],
            )
        }
        other => host.t("custom_commands-bad-action", &[("action", other)]),
    }
}

fn send_tag(host: &mut dyn Host, inv: &Invocation) -> String {
    let name = inv.get("name").unwrap_or("");
    let Some((_, message)) = tags(host).into_iter().find(|(n, _)| n == name) else {
        return host.t("custom_commands-missing", &[("name", name)]);
    };
    let typed = typed_args(inv.get("args").unwrap_or(""));
    let text = fill(&message, &typed);
    if matches!(
        inv.get("ephemeral").map(str::to_ascii_lowercase).as_deref(),
        Some("true" | "yes" | "1")
    ) {
        return text;
    }
    let Some(channel) = inv.channel_id else {
        return text;
    };
    let mut body = Val::map();
    body.set("content", Val::Str(text));
    match host.rest(
        "POST",
        &format!("/channels/{channel}/messages"),
        Some(&body),
    ) {
        Ok(HostResponse {
            status: 200..=299, ..
        }) => String::new(),
        Ok(HostResponse { status, .. }) => host.t(
            "custom_commands-send-failed",
            &[("error", &format!("HTTP {status}"))],
        ),
        Err(e) => host.t("custom_commands-send-failed", &[("error", &e)]),
    }
}

impl Plugin for CustomCommands {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_interaction(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
        {
            let answer = match inv.sub.as_str() {
                "tags" => manage(host, &inv),
                "tag" => send_tag(host, &inv),
                _ => return Verdict::Pass,
            };
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn run(p: &CustomCommands, h: &mut Rec, sub: &str, opts: &[(&str, &str)]) -> String {
        let options: Vec<String> = opts
            .iter()
            .map(|(k, v)| {
                format!(
                    r#"{{"type":3,"name":"{k}","value":{}}}"#,
                    serde_json::to_string(v).unwrap()
                )
            })
            .collect();
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"{sub}","options":[{}]}}]}}}}"#,
            options.join(",")
        ));
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        let cmds = p.commands();
        cmds.iter()
            .find(|c| c.name() == sub)
            .unwrap()
            .run(h, &Args { options: vec![] })
    }

    #[test]
    fn arguments_and_filling_follow_vencord() {
        assert_eq!(
            arguments("Hi {{Name}}, {{ mood = fine }} {{name}} {{x}}"),
            vec![
                ("name".to_string(), None),
                ("mood".to_string(), Some("fine".to_string())),
                ("x".to_string(), None),
            ]
        );
        // `{{}}` is no argument: like the JS regex `/{{(.+?)}}/`, the match runs on to the next
        // closing braces.
        assert_eq!(arguments("{{}} {{x}}"), vec![("}} {{x".to_string(), None)]);
        let typed = typed_args("name = Ann; ignored");
        assert_eq!(
            fill("Hi {{name}}!\\nYou feel {{mood=fine}}. {{x}}", &typed),
            "Hi Ann!\nYou feel fine. {{x}}"
        );
        assert_eq!(fill("{{}} {{a}}}", &[]), "{{}} {{a}}}");
    }

    #[test]
    fn create_list_send_and_delete_a_tag() {
        let p = CustomCommands::new();
        let mut h = Rec::default().answer("POST", "/channels/9/messages", 200, "{}");
        assert_eq!(
            run(&p, &mut h, "tags", &[("action", "list")]),
            "Woops! There are no tags yet, use `/phoecord tags action:create` to create one!"
        );
        let msg = "Hello {{who=world}}!\\nBye";
        assert_eq!(
            run(
                &p,
                &mut h,
                "tags",
                &[("action", "create"), ("name", "greet"), ("message", msg)]
            ),
            "Created the tag **greet**."
        );
        assert_eq!(
            run(&p, &mut h, "tags", &[("action", "list")]),
            "`greet`: Hello {{who=world}}! Bye"
        );
        assert_eq!(run(&p, &mut h, "tag", &[("name", "greet")]), "");
        let sent = h.rest_calls[0].1.as_ref().unwrap();
        assert_eq!(sent.str_at(&["content"]), Some("Hello world!\nBye"));
        assert_eq!(
            run(
                &p,
                &mut h,
                "tag",
                &[
                    ("name", "greet"),
                    ("args", "who=Ann"),
                    ("ephemeral", "true")
                ]
            ),
            "Hello Ann!\nBye"
        );
        assert_eq!(h.rest_calls.len(), 1, "ephemeral sends nothing");
        assert_eq!(
            run(
                &p,
                &mut h,
                "tags",
                &[("action", "delete"), ("name", "greet")]
            ),
            "Successfully deleted the tag **greet**!"
        );
        assert_eq!(
            run(
                &p,
                &mut h,
                "tags",
                &[("action", "delete"), ("name", "greet")]
            ),
            "A Tag with the name **greet** does not exist!"
        );
    }

    #[test]
    fn create_refuses_bad_input_and_says_when_it_overwrites() {
        let p = CustomCommands::new();
        let mut h = Rec::default();
        assert!(run(&p, &mut h, "tags", &[("action", "create"), ("name", "a")]).contains("Give"));
        assert!(
            run(
                &p,
                &mut h,
                "tags",
                &[
                    ("action", "create"),
                    ("name", "a"),
                    ("message", "{{ephemeral}}")
                ]
            )
            .contains("reserved")
        );
        run(
            &p,
            &mut h,
            "tags",
            &[("action", "create"), ("name", "a"), ("message", "1")],
        );
        assert!(
            run(
                &p,
                &mut h,
                "tags",
                &[("action", "create"), ("name", "a"), ("message", "2")]
            )
            .contains("overwritten")
        );
        assert_eq!(tags(&h), vec![("a".to_string(), "2".to_string())]);
        assert!(run(&p, &mut h, "tags", &[("action", "rename")]).contains("Unknown action"));
        let long = "x".repeat(80);
        run(
            &p,
            &mut h,
            "tags",
            &[("action", "create"), ("name", "b"), ("message", &long)],
        );
        assert!(
            run(&p, &mut h, "tags", &[("action", "list")])
                .ends_with(&format!("`b`: {}...", "x".repeat(72)))
        );
    }
}
