//! The PhoeCord settings tab in Phoenix (task 2.3.3): the part that does not depend on GPUI.
//!
//! The tab is drawn by the loader from what the engine and `config.toml` say (ruling 2026-11-26:
//! no `SettingsRender` event, nothing in a plugin fills the tab). This module turns the plugins'
//! manifests, their [`SettingSpec`]s and the live [`Config`] into a [`Page`] of plain data, and
//! turns what the user does on that page into an [`Edit`] that [`commit`] writes to `config.toml`
//! and applies to the running engine. The native page (a hook on the user settings section render
//! and the Mana builders it uses, located in the private recon notes) only draws a [`Page`] and
//! reports [`Edit`]s, so everything a user can change is tested here on plain data.
//!
//! Two ways in: [`engine_page`] / [`engine_commit`] in an in-process launch, where the loader runs
//! its own engine, and [`ipc_page`] / [`file_commit`] on the proxy transport, where the proxy has
//! the engine: the plugins come over the loader IPC (`plugins.get`, labels already translated)
//! and an edit is written to `config.toml`, which the proxy reloads live.
//!
//! Installing and removing WASM packages from the tab is in [`super::packages`].

use phoecord_core::{Manifest, SettingKind, SettingSpec, Val};
use phoecord_transport::config::Config;

/// Everything the tab shows, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub title: String,
    pub subtitle: String,
    /// Installed WASM packages first, then the built-in plugins; an empty section is left out.
    pub sections: Vec<Section>,
    /// Shown instead of the sections when the search matches nothing.
    pub empty: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Row>,
}

/// One plugin: its switch, what it is, and its settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub description: String,
    /// "1.2.0 · by Alice, Bob", or the version alone.
    pub byline: String,
    pub enabled: bool,
    /// The plugin panicked and the chain turned it off for this run.
    pub crashed: Option<String>,
    /// What an installed package may do, in words (empty for built-in plugins, which run with
    /// the host's full trust).
    pub rights: Vec<String>,
    pub controls: Vec<Control>,
}

/// One setting, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub key: String,
    pub label: String,
    pub kind: SettingKind,
    /// What the plugin uses now: the stored value, or the default.
    pub value: Val,
    pub default: Val,
    /// For a choice: the options in order, each with the text to show.
    pub options: Vec<(Val, String)>,
    pub stored: Stored,
    /// For a text, number or list control: [`input_text`] of [`Control::value`].
    pub text: String,
}

/// Where a control's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    /// Nothing in `config.toml`: the default.
    Default,
    /// The user's value.
    Set,
    /// `config.toml` holds a value of the wrong shape; the plugin falls back to its default and
    /// the tab shows the default with a warning, so saving any value repairs the file.
    Invalid,
}

/// The data the page is built from, with the strings behind closures so tests need no bundles.
pub struct Source<'a> {
    pub manifests: &'a [Manifest],
    /// Ids of the manifests that are WASM packages.
    pub packages: &'a [String],
    pub crashed: &'a [String],
    pub config: &'a Config,
    /// The rights `plugin.toml` of package `id` asks for (names as written there).
    pub rights: &'a dyn Fn(&str) -> Vec<String>,
    /// A string of the tab itself (`phoenix-settings-*`, `gui-right-*` in PhoeCord's bundle).
    pub ui: &'a dyn Fn(&str) -> String,
    /// A plugin's own string (`SettingSpec::label`), the key itself when the plugin lacks it.
    pub plugin_t: &'a dyn Fn(&str, &str) -> String,
}

/// The keys of the tab's own strings, so a test can check both languages have them.
pub const KEYS: &[&str] = &[
    "phoenix-settings-title",
    "phoenix-settings-sub",
    "phoenix-settings-packages",
    "phoenix-settings-builtin",
    "phoenix-settings-no-match",
    "phoenix-settings-by",
    "phoenix-settings-crashed",
    "phoenix-settings-invalid",
    "phoenix-settings-reset",
    "phoenix-settings-on",
    "phoenix-settings-off",
    "phoenix-settings-list-hint",
    "phoenix-settings-rights",
    "phoenix-settings-rights-none",
    "phoenix-settings-err-number",
    "phoenix-settings-err-choice",
    "phoenix-settings-err-kind",
    "phoenix-settings-err-unknown",
];

/// The page for `src`, keeping the plugins whose name, id or description contains `query`
/// (case-insensitive; an empty query keeps all).
pub fn page(src: &Source<'_>, query: &str) -> Page {
    let q = query.trim().to_lowercase();
    let matches = |m: &Manifest| {
        q.is_empty()
            || [&m.name, &m.id, &m.description]
                .iter()
                .any(|s| s.to_lowercase().contains(&q))
    };
    let mut packages = Vec::new();
    let mut builtin = Vec::new();
    for m in src.manifests.iter().filter(|m| matches(m)) {
        let is_package = src.packages.contains(&m.id);
        let r = row(src, m, is_package);
        if is_package {
            packages.push(r);
        } else {
            builtin.push(r);
        }
    }
    for rows in [&mut packages, &mut builtin] {
        rows.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
    }
    let mut sections = Vec::new();
    if !packages.is_empty() {
        sections.push(Section {
            title: (src.ui)("phoenix-settings-packages"),
            rows: packages,
        });
    }
    if !builtin.is_empty() {
        sections.push(Section {
            title: (src.ui)("phoenix-settings-builtin"),
            rows: builtin,
        });
    }
    Page {
        title: (src.ui)("phoenix-settings-title"),
        subtitle: (src.ui)("phoenix-settings-sub"),
        empty: sections
            .is_empty()
            .then(|| (src.ui)("phoenix-settings-no-match")),
        sections,
    }
}

fn row(src: &Source<'_>, m: &Manifest, is_package: bool) -> Row {
    let byline = if m.authors.is_empty() {
        m.version.clone()
    } else {
        format!(
            "{} · {} {}",
            m.version,
            (src.ui)("phoenix-settings-by"),
            m.authors.join(", ")
        )
    };
    let rights = if is_package {
        let names = (src.rights)(&m.id);
        if names.is_empty() {
            vec![(src.ui)("phoenix-settings-rights-none")]
        } else {
            names.iter().map(|r| right_text(src.ui, r)).collect()
        }
    } else {
        Vec::new()
    };
    Row {
        id: m.id.clone(),
        name: m.name.clone(),
        description: m.description.clone(),
        byline,
        enabled: src.config.is_enabled(&m.id),
        crashed: src
            .crashed
            .contains(&m.id)
            .then(|| (src.ui)("phoenix-settings-crashed")),
        rights,
        controls: m
            .settings
            .iter()
            .filter(|s| s.problem().is_none())
            .map(|s| control(src, &m.id, s))
            .collect(),
    }
}

/// The words for right `name` (`emit_send` → `gui-right-emit-send`), the name itself when the
/// bundle lacks it.
fn right_text(ui: &dyn Fn(&str) -> String, name: &str) -> String {
    let key = format!("gui-right-{}", name.replace('_', "-"));
    let text = ui(&key);
    if text == key { name.to_string() } else { text }
}

fn control(src: &Source<'_>, plugin: &str, spec: &SettingSpec) -> Control {
    let raw = stored_value(src.config, plugin, &spec.key);
    let (value, stored) = match raw {
        None => (spec.default.clone(), Stored::Default),
        Some(v) if spec.kind.accepts(&v, &spec.options) => (v, Stored::Set),
        Some(_) => (spec.default.clone(), Stored::Invalid),
    };
    Control {
        key: spec.key.clone(),
        label: (src.plugin_t)(plugin, &spec.label),
        kind: spec.kind,
        text: input_text(&value),
        options: spec
            .options
            .iter()
            .map(|o| (o.clone(), input_text(o)))
            .collect(),
        default: spec.default.clone(),
        value,
        stored,
    }
}

/// The value `plugin`'s table in `config.toml` holds for `key`, as a [`Val`].
fn stored_value(cfg: &Config, plugin: &str, key: &str) -> Option<Val> {
    let v = cfg.plugins.get(plugin)?.get(key)?;
    Some(Val::from_json(serde_json::to_value(v).ok()?))
}

/// How a value reads in a text field: a string as it is, a number in decimal, a list one item
/// per line.
pub fn input_text(v: &Val) -> String {
    match v {
        Val::Str(s) | Val::Atom(s) => s.clone(),
        Val::Int(i) => i.to_string(),
        Val::Big(i) => i.to_string(),
        Val::Float(f) => f.to_string(),
        Val::Bool(b) => b.to_string(),
        Val::List(items) => items.iter().map(input_text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Why an edit was refused; [`EditError::text`] gives the words for the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// A number field holds something that is not a finite number.
    NotANumber,
    /// The value is not one of the choice's options.
    NotAnOption,
    /// The value does not have the setting's shape (a bug in the native page).
    WrongKind,
    /// No such plugin or setting.
    Unknown,
    /// `config.toml` could not be read or written.
    Config(String),
}

impl EditError {
    pub fn text(&self, ui: &dyn Fn(&str) -> String) -> String {
        match self {
            EditError::NotANumber => ui("phoenix-settings-err-number"),
            EditError::NotAnOption => ui("phoenix-settings-err-choice"),
            EditError::WrongKind => ui("phoenix-settings-err-kind"),
            EditError::Unknown => ui("phoenix-settings-err-unknown"),
            EditError::Config(e) => e.clone(),
        }
    }
}

/// The value a text, number or list field's `text` stands for. A number is an integer when it
/// reads as one, else a finite decimal; a list is its non-empty lines, trimmed.
pub fn parse_input(kind: SettingKind, text: &str) -> Result<Val, EditError> {
    match kind {
        SettingKind::Text => Ok(Val::Str(text.to_string())),
        SettingKind::Number => {
            let t = text.trim();
            if let Ok(i) = t.parse::<i64>() {
                return Ok(Val::Int(i));
            }
            match t.parse::<f64>() {
                Ok(f) if f.is_finite() => Ok(Val::Float(f)),
                _ => Err(EditError::NotANumber),
            }
        }
        SettingKind::List => Ok(Val::List(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|l| Val::Str(l.to_string()))
                .collect(),
        )),
        _ => Err(EditError::WrongKind),
    }
}

/// What the user did on the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// The plugin's switch.
    Enable { plugin: String, on: bool },
    /// A new value for a setting (a switch, a picked option, a parsed field).
    Set {
        plugin: String,
        key: String,
        value: Val,
    },
    /// Back to the default: the key leaves `config.toml`.
    Reset { plugin: String, key: String },
}

/// `edit` applied to `cfg`, checked against the plugins' `manifests`: only known plugins and
/// declared settings, and only values of the declared shape.
pub fn apply(cfg: &mut Config, manifests: &[Manifest], edit: &Edit) -> Result<(), EditError> {
    let find = |plugin: &str| manifests.iter().find(|m| m.id == plugin);
    match edit {
        Edit::Enable { plugin, on } => {
            find(plugin).ok_or(EditError::Unknown)?;
            cfg.set_enabled(plugin, *on)
                .map_err(|e| EditError::Config(format!("{e:#}")))
        }
        Edit::Set { plugin, key, value } => {
            let spec = spec_of(find(plugin), key)?;
            if !spec.kind.accepts(value, &spec.options) {
                return Err(match spec.kind {
                    SettingKind::Choice => EditError::NotAnOption,
                    _ => EditError::WrongKind,
                });
            }
            let v = to_toml(value).ok_or(EditError::WrongKind)?;
            cfg.plugins
                .entry(plugin.clone())
                .or_default()
                .insert(key.clone(), v);
            Ok(())
        }
        Edit::Reset { plugin, key } => {
            spec_of(find(plugin), key)?;
            if let Some(t) = cfg.plugins.get_mut(plugin) {
                t.remove(key);
            }
            Ok(())
        }
    }
}

fn spec_of<'m>(m: Option<&'m Manifest>, key: &str) -> Result<&'m SettingSpec, EditError> {
    m.and_then(|m| {
        m.settings
            .iter()
            .find(|s| s.key == key && s.problem().is_none())
    })
    .ok_or(EditError::Unknown)
}

/// A setting value as TOML; `None` for shapes a setting never has.
fn to_toml(v: &Val) -> Option<toml::Value> {
    Some(match v {
        Val::Bool(b) => toml::Value::Boolean(*b),
        Val::Int(i) => toml::Value::Integer(*i),
        Val::Big(i) => toml::Value::Integer(i64::try_from(*i).ok()?),
        Val::Float(f) if f.is_finite() => toml::Value::Float(*f),
        Val::Str(s) => toml::Value::String(s.clone()),
        Val::List(items) => toml::Value::Array(items.iter().map(to_toml).collect::<Option<_>>()?),
        _ => return None,
    })
}

/// `edit` written to `config.toml` through `persist` (the engine's config lock, a fresh read of
/// the file) and the saved config handed to `live` (the engine's `apply`), so the change takes
/// effect at once.
pub fn commit(
    manifests: &[Manifest],
    edit: &Edit,
    persist: impl FnOnce(&mut dyn FnMut(&mut Config) -> Result<(), String>) -> Result<Config, String>,
    live: impl FnOnce(Config),
) -> Result<(), EditError> {
    let mut refused = None;
    let saved = persist(&mut |cfg| {
        apply(cfg, manifests, edit).map_err(|e| {
            let text = format!("{e:?}");
            refused = Some(e);
            text
        })
    });
    match (saved, refused) {
        (_, Some(e)) => Err(e),
        (Ok(cfg), None) => {
            live(cfg);
            Ok(())
        }
        (Err(e), None) => Err(EditError::Config(e)),
    }
}

/// The page of the running `engine`; packages live in `plugins/<id>/` (their `plugin.toml` says
/// what rights they ask for). Strings follow the config's language, else the system's.
pub fn engine_page(
    engine: &phoecord_transport::Engine,
    plugins: &std::path::Path,
    query: &str,
) -> Page {
    let cfg = engine.cfg();
    let lang = language(&cfg);
    let tr = phoecord_i18n::Localizer::new(&lang);
    let manifests = engine.chain.manifests();
    let packages: Vec<String> = manifests
        .iter()
        .map(|m| m.id.clone())
        .filter(|id| engine.chain.is_dynamic(id))
        .collect();
    let rights = |id: &str| package_rights(plugins, id);
    let ui = |key: &str| tr.t(key);
    let plugin_t = |plugin: &str, key: &str| engine.shared.strings.t(plugin, &lang, key, &[]);
    page(
        &Source {
            manifests: &manifests,
            packages: &packages,
            crashed: &engine.chain.crashed_ids(),
            config: &cfg,
            rights: &rights,
            ui: &ui,
            plugin_t: &plugin_t,
        },
        query,
    )
}

/// The rights in `plugins/<id>/plugin.toml`; none when the file is missing or broken (the
/// runtime refused such a package already).
fn package_rights(plugins: &std::path::Path, id: &str) -> Vec<String> {
    std::fs::read_to_string(plugins.join(id).join("plugin.toml"))
        .ok()
        .and_then(|t| phoecord_plugin_runtime::package::PackageManifest::parse(&t).ok())
        .map(|m| m.rights)
        .unwrap_or_default()
}

/// `edit` written to `config.toml` and applied to the running `engine`.
pub fn engine_commit(engine: &phoecord_transport::Engine, edit: &Edit) -> Result<(), EditError> {
    let manifests = engine.chain.manifests();
    commit(
        &manifests,
        edit,
        |f| engine.shared.persist(f),
        |cfg| engine.apply(cfg),
    )
}

/// A `plugins.list` answer in the form [`page`] and [`apply`] take.
#[derive(Default)]
pub struct FromIpc {
    pub manifests: Vec<Manifest>,
    /// Ids of the WASM packages.
    pub packages: Vec<String>,
    pub crashed: Vec<String>,
    /// Each package's rights.
    pub rights: std::collections::HashMap<String, Vec<String>>,
}

/// The plugins of a `plugins.list` answer as manifests, plus which are packages, which crashed
/// and their rights. A setting with an unknown kind (a newer proxy) is left out.
pub fn from_ipc(infos: &[crate::ipc::PluginInfo]) -> FromIpc {
    let mut out = FromIpc::default();
    for i in infos {
        let mut m = Manifest::new(&i.id, &i.name, &i.version, &i.description);
        m.authors = i.authors.clone();
        m.settings = i
            .settings
            .iter()
            .filter_map(|s| {
                let kind = SettingKind::from_name(&s.kind)?;
                let default = Val::from_json(s.default.clone());
                Some(if kind == SettingKind::Choice {
                    let options = s.options.iter().cloned().map(Val::from_json).collect();
                    SettingSpec::choice(&s.key, options, default, &s.label)
                } else {
                    SettingSpec::new(&s.key, kind, default, &s.label)
                })
            })
            .collect();
        if i.package {
            out.packages.push(i.id.clone());
            out.rights.insert(i.id.clone(), i.rights.clone());
        }
        if i.crashed {
            out.crashed.push(i.id.clone());
        }
        out.manifests.push(m);
    }
    out
}

/// The page on the proxy transport: the plugins from `plugins.list`, the values from `config`
/// (the loader reads `config.toml` itself), the tab's strings in `lang`.
pub fn ipc_page(
    infos: &[crate::ipc::PluginInfo],
    config: &Config,
    lang: &str,
    query: &str,
) -> Page {
    let FromIpc {
        manifests,
        packages,
        crashed,
        rights,
    } = from_ipc(infos);
    let tr = phoecord_i18n::Localizer::new(lang);
    let ui = |key: &str| tr.t(key);
    let rights = |id: &str| rights.get(id).cloned().unwrap_or_default();
    // The labels came translated.
    let plugin_t = |_: &str, label: &str| label.to_string();
    page(
        &Source {
            manifests: &manifests,
            packages: &packages,
            crashed: &crashed,
            config,
            rights: &rights,
            ui: &ui,
            plugin_t: &plugin_t,
        },
        query,
    )
}

/// The language of the tab: the config's, else the system's, else English.
pub fn language(config: &Config) -> String {
    phoecord_i18n::resolve_language(
        None,
        config.language.as_deref(),
        phoecord_i18n::detect_os_language().as_deref(),
    )
    .lang
}

/// `edit` written to the `config.toml` at `path` (read fresh; written only when the edit is
/// accepted) on the proxy transport, where the proxy reloads the file live. Returns the saved
/// config, for the next page.
pub fn file_commit(
    path: &std::path::Path,
    infos: &[crate::ipc::PluginInfo],
    edit: &Edit,
) -> Result<Config, EditError> {
    let manifests = from_ipc(infos).manifests;
    let mut saved = None;
    commit(
        &manifests,
        edit,
        |f| {
            let mut cfg = if path.exists() {
                Config::load(path).map_err(|e| format!("{e:#}"))?
            } else {
                Config::default()
            };
            f(&mut cfg)?;
            cfg.save(path).map_err(|e| format!("{e:#}"))?;
            Ok(cfg)
        },
        |cfg| saved = Some(cfg),
    )?;
    Ok(saved.expect("commit applied the saved config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, name: &str, settings: Vec<SettingSpec>) -> Manifest {
        let mut m = Manifest::new(id, name, "1.0.0", format!("{name} does things"));
        m.settings = settings;
        m
    }

    fn manifests() -> Vec<Manifest> {
        let mut clear = manifest(
            "clear_urls",
            "ClearURLs",
            vec![
                SettingSpec::new("strip_all", SettingKind::Bool, Val::Bool(false), "s-strip"),
                SettingSpec::new("extra", SettingKind::List, Val::List(vec![]), "s-extra"),
            ],
        );
        clear.authors = vec!["Alice".into(), "Bob".into()];
        vec![
            manifest("no_track", "NoTrack", vec![]),
            clear,
            manifest(
                "translate",
                "Translate",
                vec![
                    SettingSpec::choice(
                        "target",
                        vec![Val::Str("en".into()), Val::Str("ru".into())],
                        Val::Str("en".into()),
                        "s-target",
                    ),
                    SettingSpec::new("delay", SettingKind::Number, Val::Int(3), "s-delay"),
                    // Broken spec: never shown, never written.
                    SettingSpec::new("bad", SettingKind::Bool, Val::Int(1), "s-bad"),
                ],
            ),
            manifest("hello_wasm", "Hello", vec![]),
        ]
    }

    fn ui(key: &str) -> String {
        match key {
            "gui-right-emit-send" => "Send live events as you".into(),
            // A missing key comes back as itself, as from the real bundle.
            other if other.starts_with("gui-right-") => other.into(),
            other => format!("<{other}>"),
        }
    }

    fn plugin_t(plugin: &str, key: &str) -> String {
        format!("{plugin}:{key}")
    }

    fn rights(id: &str) -> Vec<String> {
        if id == "hello_wasm" {
            vec!["emit_send".into(), "made_up".into()]
        } else {
            vec![]
        }
    }

    fn build(cfg: &Config, query: &str) -> Page {
        let ms = manifests();
        let packages = vec!["hello_wasm".to_string()];
        let crashed = vec!["translate".to_string()];
        page(
            &Source {
                manifests: &ms,
                packages: &packages,
                crashed: &crashed,
                config: cfg,
                rights: &rights,
                ui: &ui,
                plugin_t: &plugin_t,
            },
            query,
        )
    }

    fn row<'p>(p: &'p Page, id: &str) -> &'p Row {
        p.sections
            .iter()
            .flat_map(|s| &s.rows)
            .find(|r| r.id == id)
            .unwrap()
    }

    #[test]
    fn packages_come_first_then_built_ins_by_name() {
        let p = build(&Config::default(), "");
        assert_eq!(p.title, "<phoenix-settings-title>");
        assert_eq!(p.sections.len(), 2);
        assert_eq!(p.sections[0].title, "<phoenix-settings-packages>");
        assert_eq!(p.sections[0].rows[0].id, "hello_wasm");
        let names: Vec<_> = p.sections[1].rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["ClearURLs", "NoTrack", "Translate"]);
        assert!(p.empty.is_none());
    }

    #[test]
    fn switches_follow_the_config_and_its_defaults() {
        let mut cfg = Config::default();
        let p = build(&cfg, "");
        assert!(row(&p, "no_track").enabled, "on by default");
        assert!(!row(&p, "clear_urls").enabled);
        cfg.set_enabled("no_track", false).unwrap();
        cfg.set_enabled("clear_urls", true).unwrap();
        let p = build(&cfg, "");
        assert!(!row(&p, "no_track").enabled);
        assert!(row(&p, "clear_urls").enabled);
    }

    #[test]
    fn rows_carry_byline_rights_and_crash_state() {
        let p = build(&Config::default(), "");
        assert_eq!(
            row(&p, "clear_urls").byline,
            "1.0.0 · <phoenix-settings-by> Alice, Bob"
        );
        assert_eq!(row(&p, "no_track").byline, "1.0.0");
        assert_eq!(
            row(&p, "hello_wasm").rights,
            ["Send live events as you", "made_up"],
            "a right with no words shows its name"
        );
        assert!(row(&p, "clear_urls").rights.is_empty(), "built-in");
        assert_eq!(
            row(&p, "translate").crashed.as_deref(),
            Some("<phoenix-settings-crashed>")
        );
        assert!(row(&p, "no_track").crashed.is_none());
    }

    #[test]
    fn a_package_without_rights_says_so() {
        let ms = manifests();
        let p = page(
            &Source {
                manifests: &ms,
                packages: &["translate".to_string()],
                crashed: &[],
                config: &Config::default(),
                rights: &|_| vec![],
                ui: &ui,
                plugin_t: &plugin_t,
            },
            "",
        );
        assert_eq!(
            row(&p, "translate").rights,
            ["<phoenix-settings-rights-none>"]
        );
    }

    #[test]
    fn controls_show_defaults_stored_values_and_bad_values() {
        let mut cfg = Config::default();
        let p = build(&cfg, "");
        let t = &row(&p, "translate").controls;
        assert_eq!(t.len(), 2, "the broken spec is left out");
        assert_eq!(t[0].label, "translate:s-target");
        assert_eq!(t[0].stored, Stored::Default);
        assert_eq!(t[0].value, Val::Str("en".into()));
        assert_eq!(
            t[0].options,
            [
                (Val::Str("en".into()), "en".to_string()),
                (Val::Str("ru".into()), "ru".to_string())
            ]
        );
        assert_eq!(t[1].text, "3");

        let table = cfg.plugins.entry("translate".into()).or_default();
        table.insert("target".into(), "ru".into());
        table.insert("delay".into(), "soon".into());
        let p = build(&cfg, "");
        let t = &row(&p, "translate").controls;
        assert_eq!(
            (t[0].stored, &t[0].value),
            (Stored::Set, &Val::Str("ru".into()))
        );
        assert_eq!((t[1].stored, &t[1].value), (Stored::Invalid, &Val::Int(3)));
    }

    #[test]
    fn a_list_reads_one_item_per_line() {
        let mut cfg = Config::default();
        cfg.plugins.entry("clear_urls".into()).or_default().insert(
            "extra".into(),
            toml::Value::Array(vec!["utm_*".into(), "fbclid".into()]),
        );
        let p = build(&cfg, "");
        let c = &row(&p, "clear_urls").controls[1];
        assert_eq!(c.stored, Stored::Set);
        assert_eq!(c.text, "utm_*\nfbclid");
    }

    #[test]
    fn search_filters_by_name_id_and_description() {
        let cfg = Config::default();
        let p = build(&cfg, "  clear ");
        assert_eq!(p.sections.len(), 1);
        assert_eq!(p.sections[0].rows[0].id, "clear_urls");
        assert_eq!(
            build(&cfg, "HELLO_WASM").sections[0].rows[0].id,
            "hello_wasm"
        );
        assert_eq!(build(&cfg, "does things").sections.len(), 2);
        let none = build(&cfg, "zzz");
        assert!(none.sections.is_empty());
        assert_eq!(none.empty.as_deref(), Some("<phoenix-settings-no-match>"));
    }

    #[test]
    fn fields_parse_to_the_setting_shape() {
        assert_eq!(parse_input(SettingKind::Number, " 42 "), Ok(Val::Int(42)));
        assert_eq!(parse_input(SettingKind::Number, "1.5"), Ok(Val::Float(1.5)));
        for bad in ["", "abc", "inf", "NaN"] {
            assert_eq!(
                parse_input(SettingKind::Number, bad),
                Err(EditError::NotANumber),
                "{bad:?}"
            );
        }
        assert_eq!(
            parse_input(SettingKind::List, " a \n\n b\r\n"),
            Ok(Val::List(vec![Val::Str("a".into()), Val::Str("b".into())]))
        );
        assert_eq!(
            parse_input(SettingKind::Text, " keep spaces "),
            Ok(Val::Str(" keep spaces ".into()))
        );
        assert_eq!(
            parse_input(SettingKind::Bool, "true"),
            Err(EditError::WrongKind)
        );
    }

    #[test]
    fn edits_are_checked_against_the_manifests() {
        let ms = manifests();
        let mut cfg = Config::default();
        let set = |plugin: &str, key: &str, value: Val| Edit::Set {
            plugin: plugin.into(),
            key: key.into(),
            value,
        };
        apply(
            &mut cfg,
            &ms,
            &set("translate", "target", Val::Str("ru".into())),
        )
        .unwrap();
        apply(&mut cfg, &ms, &set("translate", "delay", Val::Float(2.5))).unwrap();
        apply(
            &mut cfg,
            &ms,
            &set("clear_urls", "extra", Val::List(vec![Val::Str("x".into())])),
        )
        .unwrap();
        let t = &cfg.plugins["translate"];
        assert_eq!(t["target"].as_str(), Some("ru"));
        assert_eq!(t["delay"].as_float(), Some(2.5));
        assert_eq!(
            cfg.plugins["clear_urls"]["extra"].as_array().unwrap().len(),
            1
        );

        assert_eq!(
            apply(
                &mut cfg,
                &ms,
                &set("translate", "target", Val::Str("de".into()))
            ),
            Err(EditError::NotAnOption)
        );
        assert_eq!(
            apply(
                &mut cfg,
                &ms,
                &set("translate", "delay", Val::Str("1".into()))
            ),
            Err(EditError::WrongKind)
        );
        assert_eq!(
            apply(&mut cfg, &ms, &set("translate", "bad", Val::Bool(true))),
            Err(EditError::Unknown),
            "a broken spec is never written"
        );
        assert_eq!(
            apply(&mut cfg, &ms, &set("translate", "nope", Val::Bool(true))),
            Err(EditError::Unknown)
        );
        assert_eq!(
            apply(
                &mut cfg,
                &ms,
                &Edit::Enable {
                    plugin: "ghost".into(),
                    on: true
                }
            ),
            Err(EditError::Unknown)
        );

        apply(
            &mut cfg,
            &ms,
            &Edit::Reset {
                plugin: "translate".into(),
                key: "target".into(),
            },
        )
        .unwrap();
        assert!(!cfg.plugins["translate"].contains_key("target"));
        assert!(
            cfg.plugins["translate"].contains_key("delay"),
            "others kept"
        );

        apply(
            &mut cfg,
            &ms,
            &Edit::Enable {
                plugin: "clear_urls".into(),
                on: true,
            },
        )
        .unwrap();
        assert!(cfg.is_enabled("clear_urls"));
    }

    #[test]
    fn commit_saves_then_applies_and_never_saves_a_refused_edit() {
        let ms = manifests();
        let on_disk = std::cell::RefCell::new(Config::default());
        let persist = |edit: &mut dyn FnMut(&mut Config) -> Result<(), String>| {
            let mut cfg = on_disk.borrow().clone();
            edit(&mut cfg)?;
            *on_disk.borrow_mut() = cfg.clone();
            Ok(cfg)
        };
        let mut applied = None;
        commit(
            &ms,
            &Edit::Enable {
                plugin: "clear_urls".into(),
                on: true,
            },
            persist,
            |c| applied = Some(c),
        )
        .unwrap();
        assert!(on_disk.borrow().is_enabled("clear_urls"));
        assert!(applied.unwrap().is_enabled("clear_urls"));

        let mut applied = false;
        let err = commit(
            &ms,
            &Edit::Set {
                plugin: "translate".into(),
                key: "target".into(),
                value: Val::Str("de".into()),
            },
            persist,
            |_| applied = true,
        );
        assert_eq!(err, Err(EditError::NotAnOption));
        assert!(!applied);
        assert!(!on_disk.borrow().plugins.contains_key("translate"));

        let err = commit(
            &ms,
            &Edit::Enable {
                plugin: "no_track".into(),
                on: false,
            },
            |_| Err("disk full".to_string()),
            |_| panic!("not applied"),
        );
        assert_eq!(err, Err(EditError::Config("disk full".into())));
    }

    #[test]
    fn written_values_read_back_as_set() {
        let ms = manifests();
        let mut cfg = Config::default();
        let value = parse_input(SettingKind::Number, "7").unwrap();
        apply(
            &mut cfg,
            &ms,
            &Edit::Set {
                plugin: "translate".into(),
                key: "delay".into(),
                value,
            },
        )
        .unwrap();
        // Through the file, as the next launch reads it.
        let text = toml::to_string(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        let p = build(&back, "translate");
        let c = &row(&p, "translate").controls[1];
        assert_eq!((c.stored, &c.value), (Stored::Set, &Val::Int(7)));
    }

    #[test]
    fn errors_have_words() {
        assert_eq!(
            EditError::NotANumber.text(&ui),
            "<phoenix-settings-err-number>"
        );
        assert_eq!(EditError::Config("io".into()).text(&ui), "io");
    }

    #[test]
    fn every_string_of_the_tab_is_in_english_and_russian() {
        for lang in ["en", "ru"] {
            let tr = phoecord_i18n::Localizer::new(lang);
            for key in KEYS {
                assert!(tr.try_t(key).is_some(), "{lang}: {key} missing");
            }
        }
    }

    #[test]
    fn the_engine_page_and_commit_go_through_config_toml() {
        let dir = std::env::temp_dir().join(format!("phoecord-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut shared = phoecord_transport::host::Shared::new(dir.join("plugins"));
        shared.config_path = dir.join("config.toml");
        let engine = phoecord_transport::Engine::new(
            phoecord_plugin_runtime::Chain::new(phoecord_plugins::builtin()),
            shared,
            Config::default(),
        );
        let p = engine_page(&engine, &dir.join("plugins"), "");
        assert_eq!(p.sections.len(), 1, "no packages installed");
        let first = &p.sections[0].rows[0];
        let id = first.id.clone();
        engine_commit(
            &engine,
            &Edit::Enable {
                plugin: id.clone(),
                on: !first.enabled,
            },
        )
        .unwrap();
        assert_eq!(engine.cfg().is_enabled(&id), !first.enabled, "applied live");
        let on_disk = Config::load(&dir.join("config.toml")).unwrap();
        assert_eq!(on_disk.is_enabled(&id), !first.enabled, "written");
        assert_eq!(
            engine_commit(
                &engine,
                &Edit::Enable {
                    plugin: "ghost".into(),
                    on: true
                }
            ),
            Err(EditError::Unknown)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn infos() -> Vec<crate::ipc::PluginInfo> {
        use crate::ipc::{PluginInfo, SettingInfo};
        use serde_json::json;
        vec![
            PluginInfo {
                id: "translate".into(),
                name: "Translate".into(),
                version: "1.0.0".into(),
                description: "Translates".into(),
                authors: vec!["Alice".into()],
                package: false,
                crashed: true,
                rights: vec![],
                settings: vec![
                    SettingInfo {
                        key: "target".into(),
                        kind: "choice".into(),
                        default: json!("en"),
                        options: vec![json!("en"), json!("ru")],
                        label: "Target language".into(),
                    },
                    SettingInfo {
                        key: "future".into(),
                        kind: "colour".into(),
                        default: json!("#fff"),
                        options: vec![],
                        label: "From a newer proxy".into(),
                    },
                ],
            },
            PluginInfo {
                id: "hello_wasm".into(),
                name: "Hello".into(),
                version: "0.1.0".into(),
                description: "A package".into(),
                authors: vec![],
                package: true,
                crashed: false,
                rights: vec!["notify".into()],
                settings: vec![],
            },
        ]
    }

    #[test]
    fn the_proxy_list_becomes_the_same_page() {
        let p = ipc_page(&infos(), &Config::default(), "en", "");
        assert_eq!(p.title, "PhoeCord");
        assert_eq!(p.sections.len(), 2);
        let hello = row(&p, "hello_wasm");
        assert_eq!(hello.rights, ["Show notifications"]);
        let t = row(&p, "translate");
        assert!(t.crashed.is_some());
        assert_eq!(t.byline, "1.0.0 · by Alice");
        assert_eq!(t.controls.len(), 1, "an unknown kind is left out");
        assert_eq!(t.controls[0].label, "Target language");
        assert_eq!(t.controls[0].options.len(), 2);
        let ru = ipc_page(&infos(), &Config::default(), "ru", "");
        assert_eq!(ru.sections[1].title, "Встроенные плагины");
    }

    #[test]
    fn on_the_proxy_transport_an_edit_goes_to_the_file() {
        let dir =
            std::env::temp_dir().join(format!("phoecord-settings-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let set = |v: &str| Edit::Set {
            plugin: "translate".into(),
            key: "target".into(),
            value: Val::Str(v.into()),
        };
        let cfg = file_commit(&path, &infos(), &set("ru")).unwrap();
        assert_eq!(cfg.plugins["translate"]["target"].as_str(), Some("ru"));
        let on_disk = Config::load(&path).unwrap();
        assert_eq!(on_disk.plugins["translate"]["target"].as_str(), Some("ru"));
        assert!(
            on_disk.is_enabled("no_track"),
            "a new file starts from the defaults"
        );

        let before = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            file_commit(&path, &infos(), &set("de")).err(),
            Some(EditError::NotAnOption)
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "not written"
        );
        let p = ipc_page(&infos(), &on_disk, "en", "");
        assert_eq!(row(&p, "translate").controls[0].stored, Stored::Set);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
