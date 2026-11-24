//! `/phoecord` slash commands: a small framework plus the first two commands.
//!
//! The Discord client finds slash commands by asking the REST API (`application-commands/search`
//! and `application-command-index`). This plugin adds one command, `/phoecord`, to those answers.
//! Every PhoeCord command is a **subcommand** of it. When the user runs one, the client posts
//! `POST /interactions`; the plugin answers it itself (status 204, nothing reaches Discord) and
//! shows the result as a message only the user sees, made with `Host::emit_dispatch`.
//!
//! Adding a command (docs/PORTING_PLUGINS.md): implement [`Subcommand`] in your plugin's file and
//! register it in `builtin()` with [`Commands::with`].
//!
//! Unverified against a live client: the shapes below follow the public API documentation and
//! Vencord's `sendBotMessage`; check them with `phoecord launch` on a test account.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::sync::atomic::{AtomicU64, Ordering};

/// Fake application that "owns" `/phoecord` and authors its answers.
pub const APP_ID: &str = "1000000000000000001";
/// Id of the `/phoecord` command itself.
pub const COMMAND_ID: &str = "1000000000000000002";
pub const COMMAND_NAME: &str = "phoecord";

/// Option types of the application command API.
const SUB_COMMAND: i64 = 1;
const STRING: i64 = 3;
/// Message flag: only the invoking user sees it ("ephemeral").
const EPHEMERAL: i64 = 64;
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

/// A string option of a subcommand.
pub struct CommandOption {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// What a subcommand was called with.
pub struct Args {
    /// `(option name, value)` pairs, as typed.
    pub options: Vec<(String, String)>,
}

impl Args {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One `/phoecord <name>` command.
pub trait Subcommand: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn options(&self) -> Vec<CommandOption> {
        Vec::new()
    }
    /// Run it; the returned text is shown to the user as a local message.
    fn run(&self, host: &mut dyn Host, args: &Args) -> String;
}

/// A plugin as `/phoecord plugins` shows it.
#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Needs a gateway reconnect (Ctrl+R) to start or stop.
    pub gateway: bool,
}

impl PluginInfo {
    pub fn of(p: &dyn Plugin) -> PluginInfo {
        let m = p.manifest();
        PluginInfo {
            id: m.id.clone(),
            name: m.name.clone(),
            description: m.description.clone(),
            gateway: p.wants_gateway(),
        }
    }
}

/// `/phoecord plugins`: every plugin and whether it is on.
pub struct PluginsCmd {
    plugins: Vec<PluginInfo>,
}

impl Subcommand for PluginsCmd {
    fn name(&self) -> &str {
        "plugins"
    }
    fn description(&self) -> &str {
        "List PhoeCord plugins and whether they are on"
    }
    fn run(&self, host: &mut dyn Host, _args: &Args) -> String {
        let mut out = String::from("**PhoeCord plugins**");
        for p in &self.plugins {
            let on = host.plugin_enabled(&p.id).unwrap_or(false);
            out.push_str(&format!(
                "\n{} `{}` {}: {}",
                if on { "🟢" } else { "⚫" },
                p.id,
                p.name,
                p.description
            ));
        }
        out
    }
}

/// `/phoecord toggle <id>`: switch a plugin on or off.
pub struct ToggleCmd {
    plugins: Vec<PluginInfo>,
}

impl Subcommand for ToggleCmd {
    fn name(&self) -> &str {
        "toggle"
    }
    fn description(&self) -> &str {
        "Switch a plugin on or off"
    }
    fn options(&self) -> Vec<CommandOption> {
        vec![CommandOption {
            name: "id",
            description: "Plugin id, see /phoecord plugins",
            required: true,
        }]
    }
    fn run(&self, host: &mut dyn Host, args: &Args) -> String {
        let id = args.get("id").unwrap_or("").trim();
        let Some(p) = self.plugins.iter().find(|p| p.id == id) else {
            return format!("No plugin named `{id}`. Try `/phoecord plugins`.");
        };
        if p.id == COMMAND_PLUGIN_ID {
            return format!("`{id}` cannot switch itself off here; use `phoecord disable {id}`.");
        }
        let on = !host.plugin_enabled(id).unwrap_or(false);
        match host.set_plugin_enabled(id, on) {
            Ok(()) => {
                let note = if p.gateway {
                    " Takes effect when Discord reconnects (Ctrl+R)."
                } else {
                    ""
                };
                format!("`{id}` is now {}.{note}", if on { "on" } else { "off" })
            }
            Err(e) => format!("Could not switch `{id}`: {e}"),
        }
    }
}

const COMMAND_PLUGIN_ID: &str = "commands";

/// The plugin: owns the registry of subcommands.
pub struct Commands {
    manifest: Manifest,
    subs: Vec<Box<dyn Subcommand>>,
    counter: AtomicU64,
}

impl Commands {
    /// An empty registry; add commands with [`Commands::with`].
    pub fn empty() -> Commands {
        Commands {
            manifest: Manifest {
                id: COMMAND_PLUGIN_ID.into(),
                name: "Commands".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "The /phoecord slash command: plugins, toggle".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("CommandsAPI".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            subs: Vec::new(),
            counter: AtomicU64::new(0),
        }
    }

    /// `plugins` is what `/phoecord plugins` lists (take it from the other built-ins).
    pub fn new(plugins: Vec<PluginInfo>) -> Commands {
        Commands::empty()
            .with(Box::new(PluginsCmd {
                plugins: plugins.clone(),
            }))
            .with(Box::new(ToggleCmd { plugins }))
    }

    /// Register one more `/phoecord <name>` command.
    pub fn with(mut self, sub: Box<dyn Subcommand>) -> Commands {
        self.subs.push(sub);
        self
    }

    fn options_json(sub: &dyn Subcommand) -> Val {
        let opts: Vec<Val> = sub
            .options()
            .iter()
            .map(|o| {
                obj(vec![
                    ("type", Val::Int(STRING)),
                    ("name", s(o.name)),
                    ("description", s(o.description)),
                    ("required", Val::Bool(o.required)),
                ])
            })
            .collect();
        obj(vec![
            ("type", Val::Int(SUB_COMMAND)),
            ("name", s(sub.name())),
            ("description", s(sub.description())),
            ("options", Val::List(opts)),
        ])
    }

    /// The `/phoecord` application command, as the search API returns commands.
    pub fn command(&self) -> Val {
        obj(vec![
            ("id", s(COMMAND_ID)),
            ("application_id", s(APP_ID)),
            ("version", s(COMMAND_ID)),
            ("type", Val::Int(1)),
            ("name", s(COMMAND_NAME)),
            ("description", s("PhoeCord: plugins and settings")),
            ("dm_permission", Val::Bool(true)),
            ("nsfw", Val::Bool(false)),
            (
                "options",
                Val::List(self.subs.iter().map(|c| Self::options_json(&**c)).collect()),
            ),
        ])
    }

    /// The fake application entry that goes with the command.
    pub fn application() -> Val {
        obj(vec![
            ("id", s(APP_ID)),
            ("name", s("PhoeCord")),
            ("icon", Val::Null),
            ("description", s("PhoeCord, local to your client")),
            ("bot_id", s(APP_ID)),
        ])
    }

    /// Add `/phoecord` to a command list response (once). The search text is in the query string,
    /// which plugin routes do not carry, so the command is always offered; the client filters.
    fn merge(&self, body: &mut Val) -> Verdict {
        if body.get("application_commands").is_none_or(|c| {
            c.as_list()
                .iter()
                .any(|c| c.str_at(&["id"]) == Some(COMMAND_ID))
        }) {
            return Verdict::Pass;
        }
        let command = self.command();
        if let Some(list) = body
            .get_mut("application_commands")
            .and_then(Val::as_list_mut)
        {
            list.insert(0, command);
        }
        match body.get_mut("applications").and_then(Val::as_list_mut) {
            Some(apps) => {
                if !apps.iter().any(|a| a.str_at(&["id"]) == Some(APP_ID)) {
                    apps.push(Self::application());
                }
            }
            None => body.set("applications", Val::List(vec![Self::application()])),
        }
        Verdict::Changed
    }

    /// Run the subcommand an interaction asked for; the text to show, or `None` if it is not ours.
    fn answer(&self, host: &mut dyn Host, interaction: &Val) -> Option<String> {
        let data = interaction.get("data")?;
        if data.get("name").and_then(text).as_deref() != Some(COMMAND_NAME) {
            return None;
        }
        let sub = data
            .get("options")?
            .as_list()
            .iter()
            .find(|o| o.get("type").and_then(Val::as_i64) == Some(SUB_COMMAND))?;
        let name = sub.get("name").and_then(text)?;
        let Some(cmd) = self.subs.iter().find(|c| c.name() == name.as_str()) else {
            return Some(format!("Unknown command `{name}`."));
        };
        let options = sub
            .get("options")
            .map(Val::as_list)
            .unwrap_or_default()
            .iter()
            .filter_map(|o| {
                let v = match o.get("value")? {
                    Val::Int(n) => n.to_string(),
                    v => text(v)?,
                };
                Some((text(o.get("name")?)?, v))
            })
            .collect();
        Some(cmd.run(host, &Args { options }))
    }

    fn next_snowflake(&self) -> String {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let n = self.counter.fetch_add(1, Ordering::Relaxed) & 0xfff;
        (((ms.saturating_sub(DISCORD_EPOCH_MS)) << 22) | n).to_string()
    }

    /// The gateway frames that make the client show `text` as a message only the user sees.
    fn local_frames(&self, interaction: &Val, text: &str) -> Vec<Val> {
        let mut msg = obj(vec![
            ("id", Val::Str(self.next_snowflake())),
            ("type", Val::Int(0)),
            ("content", s(text)),
            ("flags", Val::Int(EPHEMERAL)),
            ("tts", Val::Bool(false)),
            ("pinned", Val::Bool(false)),
            ("mention_everyone", Val::Bool(false)),
            ("mentions", Val::List(Vec::new())),
            ("mention_roles", Val::List(Vec::new())),
            ("attachments", Val::List(Vec::new())),
            ("embeds", Val::List(Vec::new())),
            ("components", Val::List(Vec::new())),
            ("timestamp", Val::Str(timestamp())),
            ("edited_timestamp", Val::Null),
            (
                "author",
                obj(vec![
                    ("id", s(APP_ID)),
                    ("username", s("PhoeCord")),
                    ("discriminator", s("0000")),
                    ("avatar", Val::Null),
                    ("bot", Val::Bool(true)),
                ]),
            ),
        ]);
        for key in ["channel_id", "guild_id"] {
            if let Some(v) = interaction.get(key) {
                msg.set(key, v.clone());
            }
        }
        let mut frames = Vec::new();
        if let Some(nonce) = interaction.get("nonce") {
            // The client waits for this; without it the command ends in "did not respond".
            frames.push(frame(
                "INTERACTION_SUCCESS",
                obj(vec![
                    ("nonce", nonce.clone()),
                    ("id", Val::Str(self.next_snowflake())),
                ]),
            ));
        }
        frames.push(frame("MESSAGE_CREATE", msg));
        frames
    }
}

/// A string value in any shape: JSON string, ETF atom or binary.
fn text(v: &Val) -> Option<String> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => Some(String::from_utf8_lossy(b).into_owned()),
        v => v.as_str().map(str::to_string),
    }
}

fn s(v: &str) -> Val {
    Val::Str(v.into())
}

fn obj(fields: Vec<(&str, Val)>) -> Val {
    Val::Map(fields.into_iter().map(|(k, v)| (s(k), v)).collect())
}

fn frame(name: &str, data: Val) -> Val {
    obj(vec![
        ("op", Val::Int(0)),
        ("s", Val::Null),
        ("t", s(name)),
        ("d", data),
    ])
}

fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000000+00:00",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// `GET /channels/{id}/application-commands/search`, `GET /channels/{id}/application-command-index`
/// and `GET /guilds/{id}/application-command-index`: the answers that list commands.
fn is_command_list(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET"
        && matches!(
            seg.as_slice(),
            ["channels", _, "application-commands", "search"]
                | ["channels" | "guilds", _, "application-command-index"]
        )
}

/// `POST /interactions`.
fn is_interaction(method: &str, route: &str) -> bool {
    method == "POST" && route.trim_end_matches('/') == "/interactions"
}

impl Plugin for Commands {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::RestResponse { method, route } => is_command_list(method, route),
            _ => false,
        }
    }

    /// Needed so the proxy keeps the gateway bridge that carries the local answers.
    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestResponse {
                method,
                route,
                status,
                body,
            } if is_command_list(method, route) && status == 200 => self.merge(body),
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => match self.answer(host, body) {
                Some(text) => {
                    for f in self.local_frames(body, &text) {
                        host.emit_dispatch(f);
                    }
                    Verdict::Reply {
                        status: 204,
                        body: None,
                    }
                }
                None => Verdict::Pass,
            },
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn info(id: &str, gateway: bool) -> PluginInfo {
        PluginInfo {
            id: id.into(),
            name: id.to_uppercase(),
            description: format!("does {id}"),
            gateway,
        }
    }

    fn plugin() -> Commands {
        Commands::new(vec![
            info("no_track", false),
            info("message_logger", true),
            info("commands", true),
        ])
    }

    #[derive(Default)]
    struct TestHost {
        on: Vec<String>,
        locals: Vec<Val>,
        set: Vec<(String, bool)>,
    }

    impl Host for TestHost {
        fn setting(&self, _: &str) -> Option<Val> {
            None
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
        fn emit_dispatch(&mut self, payload: Val) {
            self.locals.push(payload);
        }
        fn plugin_enabled(&self, id: &str) -> Option<bool> {
            Some(self.on.iter().any(|x| x == id))
        }
        fn set_plugin_enabled(&mut self, id: &str, on: bool) -> Result<(), String> {
            self.set.push((id.into(), on));
            Ok(())
        }
    }

    fn j(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    fn interaction(sub: &str, options: &str) -> Val {
        j(&format!(
            r#"{{"type":2,"application_id":"{APP_ID}","channel_id":"9","guild_id":"7","nonce":"123",
                "data":{{"id":"{COMMAND_ID}","name":"phoecord","type":1,
                "options":[{{"name":"{sub}","type":1,"options":{options}}}]}}}}"#
        ))
    }

    fn list_event(body: &mut Val) -> Event<'_> {
        Event::RestResponse {
            method: "GET",
            route: "/channels/9/application-commands/search",
            status: 200,
            body,
        }
    }

    fn run(p: &Commands, host: &mut TestHost, body: &mut Val) -> Verdict {
        p.on_event(
            host,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(body),
            },
        )
    }

    #[test]
    fn matching_rules() {
        assert!(is_command_list(
            "GET",
            "/channels/1/application-commands/search"
        ));
        assert!(is_command_list(
            "GET",
            "/channels/1/application-command-index"
        ));
        assert!(is_command_list(
            "GET",
            "/guilds/1/application-command-index"
        ));
        assert!(!is_command_list(
            "POST",
            "/guilds/1/application-command-index"
        ));
        assert!(!is_command_list("GET", "/channels/1/messages"));
        assert!(is_interaction("POST", "/interactions"));
        assert!(!is_interaction("GET", "/interactions"));
        assert!(!is_interaction("POST", "/interactions/1/token/callback"));
        let p = plugin();
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/interactions"
        }));
        assert!(!p.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
    }

    #[test]
    fn search_response_gets_the_command_once() {
        let p = plugin();
        let mut h = TestHost::default();
        let mut body = j(
            r#"{"application_commands":[{"id":"5","name":"giphy"}],"applications":[],"cursor":null}"#,
        );

        assert_eq!(p.on_event(&mut h, list_event(&mut body)), Verdict::Changed);
        let cmds = body.get("application_commands").unwrap().as_list();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].str_at(&["name"]), Some("phoecord"));
        let subs = cmds[0].get("options").unwrap().as_list();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[1].str_at(&["name"]), Some("toggle"));
        assert_eq!(subs[1].at(&["options"]).unwrap().as_list().len(), 1);
        assert_eq!(body.get("applications").unwrap().as_list().len(), 1);
        // A second pass adds nothing.
        assert_eq!(p.on_event(&mut h, list_event(&mut body)), Verdict::Pass);
        assert_eq!(body.get("application_commands").unwrap().as_list().len(), 2);
    }

    #[test]
    fn index_without_a_command_list_is_left_alone() {
        let p = plugin();
        let mut h = TestHost::default();
        let mut body = j(r#"{"error":"x"}"#);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/guilds/1/application-command-index",
                status: 200,
                body: &mut body,
            },
        );
        assert_eq!(v, Verdict::Pass);
        let v = p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/guilds/1/application-command-index",
                status: 403,
                body: &mut body,
            },
        );
        assert_eq!(v, Verdict::Pass);
    }

    #[test]
    fn plugins_command_lists_state_locally() {
        let p = plugin();
        let mut h = TestHost {
            on: vec!["no_track".into()],
            ..TestHost::default()
        };
        let mut body = interaction("plugins", "[]");
        assert_eq!(
            run(&p, &mut h, &mut body),
            Verdict::Reply {
                status: 204,
                body: None
            }
        );
        assert_eq!(h.locals.len(), 2);
        assert_eq!(h.locals[0].str_at(&["t"]), Some("INTERACTION_SUCCESS"));
        assert_eq!(h.locals[0].str_at(&["d", "nonce"]), Some("123"));
        let m = &h.locals[1];
        assert_eq!(m.str_at(&["t"]), Some("MESSAGE_CREATE"));
        assert_eq!(m.str_at(&["d", "channel_id"]), Some("9"));
        assert_eq!(m.at(&["d", "flags"]).and_then(Val::as_i64), Some(64));
        let text = m.str_at(&["d", "content"]).unwrap();
        assert!(text.contains("🟢 `no_track`"), "{text}");
        assert!(text.contains("⚫ `message_logger`"), "{text}");
    }

    #[test]
    fn toggle_flips_a_known_plugin_and_rejects_others() {
        let p = plugin();
        let mut h = TestHost::default();
        let opts = r#"[{"name":"id","type":3,"value":"message_logger"}]"#;
        run(&p, &mut h, &mut interaction("toggle", opts));
        assert_eq!(h.set, vec![("message_logger".to_string(), true)]);
        let text = h.locals.last().unwrap().str_at(&["d", "content"]).unwrap();
        assert!(text.contains("now on") && text.contains("Ctrl+R"), "{text}");

        let mut h = TestHost {
            on: vec!["no_track".into()],
            ..TestHost::default()
        };
        let opts = r#"[{"name":"id","type":3,"value":"no_track"}]"#;
        run(&p, &mut h, &mut interaction("toggle", opts));
        assert_eq!(h.set, vec![("no_track".to_string(), false)]);

        let mut h = TestHost::default();
        for id in ["nope", "commands"] {
            let opts = format!(r#"[{{"name":"id","type":3,"value":"{id}"}}]"#);
            run(&p, &mut h, &mut interaction("toggle", &opts));
        }
        assert!(
            h.set.is_empty(),
            "unknown ids and the commands plugin are refused"
        );
        assert_eq!(h.locals.len(), 4);
    }

    #[test]
    fn other_interactions_are_forwarded_and_ids_may_be_integers() {
        let p = plugin();
        let mut h = TestHost::default();
        let mut other = j(r#"{"type":2,"data":{"name":"giphy","options":[]}}"#);
        assert_eq!(run(&p, &mut h, &mut other), Verdict::Pass);
        assert!(h.locals.is_empty());
        // The same call in ETF shape: atoms for keys, integer ids, binaries for text.
        let k = |x: &str| Val::Atom(x.into());
        let mut etf = Val::Map(vec![
            (k("channel_id"), Val::Int(9)),
            (
                k("data"),
                Val::Map(vec![
                    (k("name"), Val::Bin(b"phoecord".to_vec())),
                    (
                        k("options"),
                        Val::List(vec![Val::Map(vec![
                            (k("name"), Val::Bin(b"plugins".to_vec())),
                            (k("type"), Val::Int(1)),
                        ])]),
                    ),
                ]),
            ),
        ]);
        assert!(matches!(
            run(&p, &mut h, &mut etf),
            Verdict::Reply { status: 204, .. }
        ));
        // No nonce: only the message.
        assert_eq!(h.locals.len(), 1);
    }

    #[test]
    fn extra_commands_can_be_registered() {
        struct Ping;
        impl Subcommand for Ping {
            fn name(&self) -> &str {
                "ping"
            }
            fn description(&self) -> &str {
                "pong"
            }
            fn run(&self, _: &mut dyn Host, _: &Args) -> String {
                "pong".into()
            }
        }
        let p = plugin().with(Box::new(Ping));
        assert_eq!(p.command().get("options").unwrap().as_list().len(), 3);
        let mut h = TestHost::default();
        run(&p, &mut h, &mut interaction("ping", "[]"));
        assert_eq!(h.locals[1].str_at(&["d", "content"]), Some("pong"));
    }

    #[test]
    fn chain_answers_an_interaction_without_forwarding() {
        let chain = Chain::new(vec![Box::new(plugin())]);
        let mut h = NullHost::default();
        let mut body = Some(interaction("plugins", "[]"));
        let out = chain.rest_request(&mut h, "POST", "/interactions", body.as_mut());
        assert!(matches!(out, Outcome::Replied { status: 204, .. }));
        let mut other = Some(j(r#"{"type":2,"data":{"name":"giphy"}}"#));
        let out = chain.rest_request(&mut h, "POST", "/interactions", other.as_mut());
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn snowflakes_are_unique_and_timestamps_valid() {
        let p = plugin();
        let a = p.next_snowflake();
        let b = p.next_snowflake();
        assert_ne!(a, b);
        assert!(a.parse::<u64>().unwrap() > 1 << 40);
        let t = timestamp();
        assert!(t.starts_with("20") && t.ends_with("+00:00"), "{t}");
    }
}
