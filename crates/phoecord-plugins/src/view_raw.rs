//! ViewRaw: the raw JSON of a message, channel, server, role or user (Vencord: ViewRaw).
//!
//! Ported from Vencord `src/plugins/viewRaw/index.tsx` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors KingFish, Ven, rad, ImLvna). Vencord adds "View Raw" to
//! context menus and opens a modal with the object as JSON (4-space indent), keys sorted
//! (`sortKeys`, default on) and the user's email, phone, store country and personal connection id
//! removed (`stripSensitiveData`, default on). For a message it shows the raw content first.
//!
//! The menus and the modal wait for Phase 2. Until then: `/phoecord viewraw kind:<message |
//! channel | guild | role | user> id:<id, mention or message link>`, answered with a code block
//! only the user sees. The objects come from Discord through `Host::rest`:
//!
//! - message: `GET /channels/{id}/messages?limit=1&around={id}` (channel from the link, the
//!   `channel` option or where the command was typed);
//! - channel: `GET /channels/{id}`; guild: `GET /guilds/{id}`;
//! - role: `GET /guilds/{id}/roles` of the guild the command was typed in;
//! - user: `GET /users/{id}/profile`.
//!
//! These are what Discord sends, which is close to but not the same as the client's stores that
//! Vencord prints. An answer longer than Discord's 2000 characters is cut.

use crate::commands::{
    Kept, Subcommand, invocation, is_interaction, kept_answer, message_ref, snowflake_arg,
};
use phoecord_core::plugin::HostResponse;
use phoecord_core::{Event, EventKind, Host, Manifest, MenuEntry, Plugin, UiEvent, Val, Verdict};

/// The message context menu entry (Phoenix, task 2.4.2 d): the clicked message's raw JSON.
const MENU_VIEW: &str = "view_raw";

const PLUGIN_ID: &str = "view_raw";
const SUB: &str = "viewraw";
/// Discord's message length limit.
const MAX_LEN: usize = 2000;
/// Fields `stripSensitiveData` removes from a user object (Vencord's list, plus the API's names).
const SENSITIVE: &[&str] = &[
    "email",
    "phone",
    "storeCountry",
    "personalConnectionId",
    "store_country",
    "personal_connection_id",
];

pub struct ViewRaw {
    manifest: Manifest,
    kept: Kept,
}

impl ViewRaw {
    pub fn new() -> ViewRaw {
        ViewRaw {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ViewRaw",
                    env!("CARGO_PKG_VERSION"),
                    "View the raw JSON of a message, channel, server, role or user",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ViewRaw".into());
                m.menu = vec![MenuEntry::new(MENU_VIEW, "menu-view-raw")];
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord viewraw`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            SUB,
            "View the raw JSON of a message, channel, server, role or user",
            &self.kept,
        )
        .option("kind", "message, channel, guild, role or user", true)
        .option("id", "Id, mention or message link", true)
        .option(
            "channel",
            "message: channel id (default: this channel or the link's)",
            false,
        )
        .boxed()
    }
}

impl Default for ViewRaw {
    fn default() -> Self {
        Self::new()
    }
}

fn get(host: &mut dyn Host, route: &str) -> Result<Val, String> {
    match host.rest("GET", route, None)? {
        HostResponse {
            status: 200..=299,
            body,
            ..
        } => Ok(body.unwrap_or(Val::Null)),
        HostResponse { status, .. } => Err(format!("HTTP {status}")),
    }
}

fn setting_on(host: &dyn Host, key: &str) -> bool {
    host.setting(key).and_then(|v| v.as_bool()).unwrap_or(true)
}

/// `value` without the sensitive user fields, at the top and in `user` / `author`.
fn redact(value: &mut Val) {
    let strip = |v: &mut Val| {
        if let Val::Map(fields) = v {
            fields.retain(|(k, _)| !k.as_str().is_some_and(|k| SENSITIVE.contains(&k)));
        }
    };
    strip(value);
    for key in ["user", "author"] {
        if let Some(u) = value.get_mut(key) {
            strip(u);
        }
    }
}

/// Keys sorted, at every level (Vencord's `sortObject`).
fn sort_keys(value: &mut Val) {
    match value {
        Val::Map(fields) => {
            fields.sort_by_key(|(a, _)| key_text(a));
            fields.iter_mut().for_each(|(_, v)| sort_keys(v));
        }
        Val::List(items) | Val::Tuple(items) => items.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

fn key_text(k: &Val) -> String {
    match k {
        Val::Bin(b) | Val::Charlist(b) => String::from_utf8_lossy(b).into_owned(),
        k => k.as_str().map_or_else(|| format!("{k:?}"), str::to_string),
    }
}

/// `value` as JSON with a 4-space indent, like `JSON.stringify(v, null, 4)`.
pub fn pretty(value: &Val) -> String {
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

fn write(v: &Val, depth: usize, out: &mut String) {
    let pad = |d: usize, out: &mut String| out.push_str(&"    ".repeat(d));
    match v {
        Val::Null => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Int(n) => out.push_str(&n.to_string()),
        Val::Big(n) => out.push_str(&n.to_string()),
        Val::Float(f) if f.is_finite() => out.push_str(&f.to_string()),
        Val::Float(_) => out.push_str("null"),
        Val::Str(s) | Val::Atom(s) => quote(s, out),
        Val::Bin(b) | Val::Charlist(b) => quote(&String::from_utf8_lossy(b), out),
        Val::List(items) | Val::Tuple(items) if items.is_empty() => out.push_str("[]"),
        Val::List(items) | Val::Tuple(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                pad(depth + 1, out);
                write(item, depth + 1, out);
            }
            out.push('\n');
            pad(depth, out);
            out.push(']');
        }
        Val::Map(fields) if fields.is_empty() => out.push_str("{}"),
        Val::Map(fields) => {
            out.push('{');
            for (i, (k, val)) in fields.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                pad(depth + 1, out);
                quote(&key_text(k), out);
                out.push_str(": ");
                write(val, depth + 1, out);
            }
            out.push('\n');
            pad(depth, out);
            out.push('}');
        }
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A code block that never closes early: three backticks inside get a zero-width space.
fn block(lang: &str, text: &str) -> String {
    format!("```{lang}\n{}\n```", text.replace("```", "`\u{200b}``"))
}

/// The whole answer, cut to [`MAX_LEN`] characters with the block still closed.
fn answer(host: &dyn Host, title: &str, content: Option<&str>, json: &str) -> String {
    let mut head = format!("**{title}**\n");
    if let Some(c) = content.filter(|c| !c.is_empty()) {
        head.push_str(&block("", c));
        head.push('\n');
    }
    let full = format!("{head}{}", block("json", json));
    if full.chars().count() <= MAX_LEN {
        return full;
    }
    let note = host.t("view_raw-cut", &[]);
    let room = MAX_LEN.saturating_sub(head.chars().count() + note.chars().count() + 16);
    let cut: String = json.chars().take(room).collect();
    format!("{head}{}\n{note}", block("json", &cut))
}

impl ViewRaw {
    fn run(&self, host: &mut dyn Host, inv: &crate::commands::Invocation) -> String {
        let kind = inv.get("kind").unwrap_or("").to_ascii_lowercase();
        let id = inv.get("id").unwrap_or("");
        let fetched = match kind.as_str() {
            "message" => {
                let Some((linked, message)) = message_ref(id) else {
                    return host.t("view_raw-bad-id", &[]);
                };
                let channel = match inv.get("channel").map(snowflake_arg) {
                    Some(None) => return host.t("view_raw-bad-channel", &[]),
                    given => given.flatten().or(linked).or(inv.channel_id),
                };
                let Some(channel) = channel else {
                    return host.t("view_raw-bad-channel", &[]);
                };
                get(
                    host,
                    &format!("/channels/{channel}/messages?limit=1&around={message}"),
                )
                .map(|page| {
                    page.as_list()
                        .iter()
                        .find(|m| m.id_at(&["id"]) == Some(message))
                        .cloned()
                })
            }
            "channel" | "guild" | "user" | "role" => {
                let Some(id) = snowflake_arg(id) else {
                    return host.t("view_raw-bad-id", &[]);
                };
                match kind.as_str() {
                    "channel" => get(host, &format!("/channels/{id}")).map(Some),
                    "guild" => get(host, &format!("/guilds/{id}")).map(Some),
                    "user" => get(host, &format!("/users/{id}/profile")).map(Some),
                    _ => {
                        let Some(guild) = inv.guild_id else {
                            return host.t("view_raw-need-guild", &[]);
                        };
                        get(host, &format!("/guilds/{guild}/roles")).map(|roles| {
                            roles
                                .as_list()
                                .iter()
                                .find(|r| r.id_at(&["id"]) == Some(id))
                                .cloned()
                        })
                    }
                }
            }
            _ => return host.t("view_raw-bad-kind", &[("kind", &kind)]),
        };
        let mut value = match fetched {
            Ok(Some(v)) => v,
            Ok(None) => return host.t("view_raw-not-found", &[]),
            Err(e) => return host.t("view_raw-failed", &[("error", &e)]),
        };
        if setting_on(host, "stripSensitiveData") {
            redact(&mut value);
        }
        if setting_on(host, "sortKeys") {
            sort_keys(&mut value);
        }
        let title = host.t(&format!("view_raw-title-{kind}"), &[]);
        let content = (kind == "message")
            .then(|| value.str_at(&["content"]).map(str::to_string))
            .flatten();
        answer(host, &title, content.as_deref(), &pretty(&value))
    }
}

impl Plugin for ViewRaw {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Ui { name } => *name == "menu_action",
            _ => false,
        }
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Ui(UiEvent::MenuAction {
            entry,
            channel_id,
            message_id,
        }) = event
        {
            if entry == MENU_VIEW {
                let inv = crate::commands::Invocation {
                    sub: SUB.into(),
                    options: vec![
                        ("kind".into(), "message".into()),
                        ("id".into(), message_id.to_string()),
                        ("channel".into(), channel_id.to_string()),
                    ],
                    channel_id: Some(channel_id),
                    guild_id: host.channel_guild(channel_id),
                };
                let text = self.run(host, &inv);
                host.emit_dispatch(crate::commands::local_message(channel_id, &text));
            }
            return Verdict::Pass;
        }
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && inv.sub == SUB
        {
            let answer = self.run(host, &inv);
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

    fn run(p: &ViewRaw, h: &mut Rec, kind: &str, id: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","guild_id":"7","data":{{"name":"phoecord","options":[{{"type":1,"name":"viewraw","options":[
                {{"type":3,"name":"kind","value":"{kind}"}},{{"type":3,"name":"id","value":"{id}"}}]}}]}}}}"#
        ));
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        p.command().run(h, &Args { options: vec![] })
    }

    fn click(p: &dyn Plugin, h: &mut Rec, entry: &str) {
        assert!(p.wants(&EventKind::Ui {
            name: "menu_action"
        }));
        p.on_event(
            h,
            Event::Ui(UiEvent::MenuAction {
                entry,
                channel_id: 9,
                message_id: 5,
            }),
        );
    }

    fn local_text(h: &Rec) -> String {
        let f = h.dispatched.last().expect("a local message");
        assert_eq!(f.str_at(&["t"]), Some("MESSAGE_CREATE"));
        assert_eq!(f.str_at(&["d", "channel_id"]), Some("9"));
        assert_eq!(f.at(&["d", "flags"]), Some(&Val::Int(64)));
        f.str_at(&["d", "content"]).unwrap().to_string()
    }

    #[test]
    fn the_menu_entry_shows_the_clicked_message() {
        let p = ViewRaw::new();
        assert_eq!(p.manifest().menu[0].id, MENU_VIEW);
        let mut h = Rec::default().answer(
            "GET",
            "/channels/9/messages?limit=1&around=5",
            200,
            r#"[{"id":"5","content":"hi"}]"#,
        );
        click(&p, &mut h, MENU_VIEW);
        let text = local_text(&h);
        assert!(text.contains("\"content\": \"hi\""), "{text}");
    }

    #[test]
    fn pretty_prints_like_json_stringify_with_four_spaces() {
        let v = j(r#"{"b":[1,{"c":null}],"a":"x\"y\n","e":{},"f":[],"g":true,"h":1.5}"#);
        assert_eq!(
            pretty(&v),
            "{\n    \"b\": [\n        1,\n        {\n            \"c\": null\n        }\n    ],\n    \
             \"a\": \"x\\\"y\\n\",\n    \"e\": {},\n    \"f\": [],\n    \"g\": true,\n    \"h\": 1.5\n}"
        );
    }

    #[test]
    fn a_message_shows_its_content_then_sorted_data_without_the_email() {
        let p = ViewRaw::new();
        let mut h = Rec::default().answer(
            "GET",
            "/channels/9/messages?limit=1&around=5",
            200,
            r#"[{"id":"5","content":"hi ```x```","author":{"username":"me","email":"a@b.c","id":"1"}}]"#,
        );
        let out = run(&p, &mut h, "message", "5");
        assert!(out.starts_with("**Raw Message Data**\n```\nhi `\u{200b}``x`\u{200b}``\n```\n```json\n{\n    \"author\": {\n        \"id\": \"1\""), "{out}");
        assert!(!out.contains("a@b.c"));
        assert!(out.ends_with("\"id\": \"5\"\n}\n```"), "{out}");
    }

    #[test]
    fn channel_guild_user_and_role_routes() {
        let p = ViewRaw::new();
        let mut h = Rec::default()
            .answer("GET", "/channels/3", 200, r#"{"id":"3","name":"general"}"#)
            .answer("GET", "/guilds/7", 200, r#"{"id":"7"}"#)
            .answer(
                "GET",
                "/users/2/profile",
                200,
                r#"{"user":{"id":"2","phone":"+1"}}"#,
            )
            .answer(
                "GET",
                "/guilds/7/roles",
                200,
                r#"[{"id":"8","name":"mod"}]"#,
            );
        assert!(run(&p, &mut h, "channel", "<#3>").contains("\"name\": \"general\""));
        assert!(run(&p, &mut h, "guild", "7").starts_with("**Raw Guild Data**"));
        let user = run(&p, &mut h, "user", "<@2>");
        assert!(
            user.contains("\"id\": \"2\"") && !user.contains("+1"),
            "{user}"
        );
        assert!(run(&p, &mut h, "role", "8").contains("\"name\": \"mod\""));
        assert!(run(&p, &mut h, "role", "9").contains("Not found"));
    }

    #[test]
    fn settings_off_keep_order_and_fields() {
        let p = ViewRaw::new();
        let mut h = Rec::default()
            .with("sortKeys", Val::Bool(false))
            .with("stripSensitiveData", Val::Bool(false))
            .answer(
                "GET",
                "/users/2/profile",
                200,
                r#"{"z":1,"email":"a@b.c","a":2}"#,
            );
        let out = run(&p, &mut h, "user", "2");
        let (z, a) = (out.find("\"z\"").unwrap(), out.find("\"a\"").unwrap());
        assert!(z < a && out.contains("a@b.c"), "{out}");
    }

    #[test]
    fn long_answers_are_cut_and_bad_input_is_explained() {
        let p = ViewRaw::new();
        let long = "x".repeat(3000);
        let mut h = Rec::default().answer(
            "GET",
            "/channels/3",
            200,
            &format!(r#"{{"topic":"{long}"}}"#),
        );
        let out = run(&p, &mut h, "channel", "3");
        assert!(out.chars().count() <= MAX_LEN, "{}", out.len());
        assert!(
            out.contains("```\n") && out.ends_with("cut to fit Discord's 2000 characters)"),
            "{out}"
        );
        assert!(run(&p, &mut h, "emoji", "3").contains("Unknown kind"));
        assert!(run(&p, &mut h, "guild", "abc").contains("Not an id"));
        assert!(run(&p, &mut h, "guild", "4").contains("HTTP 404"));
    }
}
