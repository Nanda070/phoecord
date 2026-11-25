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

/// The body of a [`FnCmd`]: gets the host and the parsed arguments, returns the text to show.
pub type CmdFn = Box<dyn Fn(&mut dyn Host, &Args) -> String + Send + Sync>;

/// A `/phoecord <name>` command made from a closure, owned by one plugin: it answers "the plugin
/// is off" by itself while that plugin is disabled, so the closure only runs when it is on.
/// Plugins of stages 1.2.16 and 1.2.18 use it to show what their data part collected.
pub struct FnCmd {
    name: &'static str,
    description: &'static str,
    options: Vec<CommandOption>,
    plugin_id: &'static str,
    run: CmdFn,
}

impl FnCmd {
    pub fn new(
        plugin_id: &'static str,
        name: &'static str,
        description: &'static str,
        run: impl Fn(&mut dyn Host, &Args) -> String + Send + Sync + 'static,
    ) -> FnCmd {
        FnCmd {
            name,
            description,
            options: Vec::new(),
            plugin_id,
            run: Box::new(run),
        }
    }

    /// Add a string option.
    pub fn option(
        mut self,
        name: &'static str,
        description: &'static str,
        required: bool,
    ) -> FnCmd {
        self.options.push(CommandOption {
            name,
            description,
            required,
        });
        self
    }

    pub fn boxed(self) -> Box<dyn Subcommand> {
        Box::new(self)
    }
}

impl Subcommand for FnCmd {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn options(&self) -> Vec<CommandOption> {
        self.options
            .iter()
            .map(|o| CommandOption {
                name: o.name,
                description: o.description,
                required: o.required,
            })
            .collect()
    }
    fn run(&self, host: &mut dyn Host, args: &Args) -> String {
        if host.plugin_enabled(self.plugin_id) == Some(false) {
            return host.t("plugin-off", &[("id", self.plugin_id)]);
        }
        (self.run)(host, args)
    }
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
        let mut out = format!("**{}**", host.t("plugins-header", &[]));
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
            return host.t("toggle-unknown", &[("id", id)]);
        };
        if p.id == COMMAND_PLUGIN_ID {
            return host.t("toggle-self", &[("id", id)]);
        }
        let on = !host.plugin_enabled(id).unwrap_or(false);
        match host.set_plugin_enabled(id, on) {
            Ok(()) => {
                let mut out = host.t(if on { "toggle-on" } else { "toggle-off" }, &[("id", id)]);
                if p.gateway {
                    out.push(' ');
                    out.push_str(&host.t("toggle-reconnect", &[]));
                }
                out
            }
            Err(e) => host.t("toggle-failed", &[("id", id), ("error", &e)]),
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
            return Some(host.t("unknown-command", &[("name", &name)]));
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
        // An empty answer shows nothing: the command already did its work in the open (for
        // example it sent a real message to the channel).
        if !text.is_empty() {
            frames.push(frame("MESSAGE_CREATE", msg));
        }
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
pub fn is_interaction(method: &str, route: &str) -> bool {
    method == "POST" && route.trim_end_matches('/') == "/interactions"
}

/// One `/phoecord <sub>` call as the plugin that owns `sub` reads it from `POST /interactions`.
///
/// A subcommand body runs inside this plugin, so its `Host::rest`, `fetch` and timers would count
/// as `commands`. A plugin that needs them (or the channel the command was typed in) watches
/// `POST /interactions` itself (it sits before this plugin in the chain), does the work as itself
/// with [`invocation`], keeps the answer, and its [`FnCmd`] returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// The subcommand name.
    pub sub: String,
    /// `(option name, value)` pairs, as typed.
    pub options: Vec<(String, String)>,
    /// The channel the command was typed in.
    pub channel_id: Option<u64>,
    /// Its guild (`None` in DMs).
    pub guild_id: Option<u64>,
}

impl Invocation {
    /// An option's value, trimmed; `None` when missing or blank.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    }
}

/// The answer a plugin keeps for its [`kept_answer`] command between handling the interaction
/// and the command plugin showing it.
pub type Kept = crate::state::Shared<Option<String>>;

/// A `/phoecord <name>` command whose work the owning plugin already did (see [`Invocation`]): it
/// shows the answer the plugin kept in `kept` (an empty one shows nothing), or `owned-no-reply`
/// when the plugin did not run.
pub fn kept_answer(
    plugin_id: &'static str,
    name: &'static str,
    description: &'static str,
    kept: &Kept,
) -> FnCmd {
    let kept = kept.clone();
    FnCmd::new(plugin_id, name, description, move |host, _| {
        kept.lock()
            .take()
            .unwrap_or_else(|| host.t("owned-no-reply", &[("id", plugin_id), ("name", name)]))
    })
}

/// A snowflake typed as digits or as a mention (`<@1>`, `<@!1>`, `<#1>`, `<@&1>`).
pub fn snowflake_arg(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .map_or(s, |s| s.trim_start_matches(['@', '#', '!', '&']));
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// A message given as an id or a link (`https://discord.com/channels/<guild or @me>/<channel>/<id>`,
/// also `ptb.`, `canary.` and `discordapp.com`): `(channel from the link, message id)`.
pub fn message_ref(s: &str) -> Option<(Option<u64>, u64)> {
    let s = s.trim().trim_start_matches('<').trim_end_matches('>');
    if let Some(id) = snowflake_arg(s) {
        return Some((None, id));
    }
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    let host = host
        .trim_start_matches("ptb.")
        .trim_start_matches("canary.");
    if host != "discord.com" && host != "discordapp.com" {
        return None;
    }
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["channels", g, c, m] if *g == "@me" || snowflake_arg(g).is_some() => {
            Some((Some(snowflake_arg(c)?), snowflake_arg(m)?))
        }
        _ => None,
    }
}

/// The `/phoecord` subcommand an interaction body calls, or `None` if it is not `/phoecord`.
pub fn invocation(interaction: &Val) -> Option<Invocation> {
    let data = interaction.get("data")?;
    if data.get("name").and_then(text).as_deref() != Some(COMMAND_NAME) {
        return None;
    }
    let sub = data
        .get("options")?
        .as_list()
        .iter()
        .find(|o| o.get("type").and_then(Val::as_i64) == Some(SUB_COMMAND))?;
    let options = sub
        .get("options")
        .map(Val::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|o| {
            let v = match o.get("value")? {
                Val::Int(n) => n.to_string(),
                Val::Bool(b) => b.to_string(),
                v => text(v)?,
            };
            Some((text(o.get("name")?)?, v))
        })
        .collect();
    let id = |key: &str| {
        interaction.get(key).and_then(|v| match v {
            Val::Int(_) | Val::Big(_) => v.as_i64().and_then(|n| u64::try_from(n).ok()),
            v => text(v)?.parse().ok(),
        })
    };
    Some(Invocation {
        sub: sub.get("name").and_then(text)?,
        options,
        channel_id: id("channel_id"),
        guild_id: id("guild_id"),
    })
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
        fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
            crate::testutil::tr("", key, args)
        }
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

#[cfg(test)]
mod fn_cmd_tests {
    use super::*;
    use crate::testutil::Rec;

    #[test]
    fn fn_cmd_runs_only_while_its_plugin_is_on() {
        let cmd = FnCmd::new("demo", "demo", "Show demo", |_, a: &Args| {
            format!("got {}", a.get("x").unwrap_or("-"))
        })
        .option("x", "a value", false);
        assert_eq!(cmd.options().len(), 1);
        let args = Args {
            options: vec![("x".into(), "1".into())],
        };
        let mut host = Rec::default();
        assert_eq!(cmd.run(&mut host, &args), "got 1");
        host.plugins.insert("demo".into(), false);
        assert!(cmd.run(&mut host, &args).contains("is off"));
    }
}

#[cfg(test)]
mod invocation_tests {
    use super::*;
    use crate::testutil::j;

    #[test]
    fn invocation_reads_the_sub_its_options_and_where_it_was_typed() {
        let body = j(
            r#"{"type":2,"channel_id":"9","guild_id":"7","data":{"name":"phoecord",
            "options":[{"type":1,"name":"tag","options":[
                {"type":3,"name":"name","value":" hi "},{"type":4,"name":"n","value":3},
                {"type":5,"name":"ephemeral","value":true}]}]}}"#,
        );
        let inv = invocation(&body).unwrap();
        assert_eq!(inv.sub, "tag");
        assert_eq!(inv.get("name"), Some("hi"));
        assert_eq!(inv.get("n"), Some("3"));
        assert_eq!(inv.get("ephemeral"), Some("true"));
        assert_eq!(inv.get("missing"), None);
        assert_eq!((inv.channel_id, inv.guild_id), (Some(9), Some(7)));

        let dm = j(r#"{"type":2,"channel_id":9,"data":{"name":"phoecord",
            "options":[{"type":1,"name":"plugins"}]}}"#);
        let inv = invocation(&dm).unwrap();
        assert_eq!((inv.channel_id, inv.guild_id), (Some(9), None));
        assert!(inv.options.is_empty());

        assert_eq!(
            invocation(&j(r#"{"type":2,"data":{"name":"giphy"}}"#)),
            None
        );
    }

    #[test]
    fn an_empty_answer_only_ends_the_interaction() {
        let p = Commands::empty();
        let body = j(r#"{"type":2,"channel_id":"9","nonce":"5","data":{"name":"phoecord"}}"#);
        let frames = p.local_frames(&body, "");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].str_at(&["t"]), Some("INTERACTION_SUCCESS"));
        assert_eq!(p.local_frames(&body, "hi").len(), 2);
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;
    use crate::testutil::Rec;

    #[test]
    fn ids_and_message_links() {
        assert_eq!(snowflake_arg(" 123 "), Some(123));
        assert_eq!(snowflake_arg("<@!42>"), Some(42));
        assert_eq!(snowflake_arg("<#7>"), Some(7));
        assert_eq!(snowflake_arg("12a"), None);
        assert_eq!(snowflake_arg(""), None);
        assert_eq!(snowflake_arg("123456789012345678901"), None);
        assert_eq!(message_ref("99"), Some((None, 99)));
        assert_eq!(
            message_ref("https://discord.com/channels/1/2/3"),
            Some((Some(2), 3))
        );
        assert_eq!(
            message_ref("<https://canary.discord.com/channels/@me/2/3>"),
            Some((Some(2), 3))
        );
        assert_eq!(message_ref("https://evil.example/channels/1/2/3"), None);
        assert_eq!(message_ref("https://discord.com/channels/1/2"), None);
    }

    #[test]
    fn kept_answer_shows_the_kept_text_once() {
        let kept = Kept::default();
        let cmd = kept_answer("demo", "demo", "Demo", &kept);
        let mut host = Rec::default();
        let args = Args { options: vec![] };
        *kept.lock() = Some("done".into());
        assert_eq!(cmd.run(&mut host, &args), "done");
        assert!(cmd.run(&mut host, &args).contains("did not run"));
    }
}
