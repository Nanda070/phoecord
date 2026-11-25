//! Localization for PhoeCord: Fluent bundles embedded in the binary.
//!
//! Lookup order for a key: requested language, then English, then the key itself
//! (a missing key is logged once). Other crates keep their own strings in
//! `crates/<crate>/i18n/{en,ru}/main.ftl`; `scripts/check_i18n.py` keeps the two in sync.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

pub use fluent_bundle::{FluentArgs as Args, FluentValue};

const EN: &str = include_str!("../i18n/en/main.ftl");
const RU: &str = include_str!("../i18n/ru/main.ftl");

type Bundle = FluentBundle<FluentResource>;

/// Languages with a translation. English is the source and the fallback.
pub const SUPPORTED: &[&str] = &["en", "ru"];

/// Detects the OS language and returns a supported code (`"en"`, `"ru"`), or `None`
/// when the OS language is unknown or has no translation.
pub fn detect_os_language() -> Option<String> {
    sys_locale::get_locale().and_then(|tag| normalize(&tag))
}

/// Maps a BCP 47 / POSIX tag (`ru-RU`, `ru_RU.UTF-8`, `RU`) to a supported code.
pub fn normalize(tag: &str) -> Option<String> {
    let tag = tag.split(['.', '@']).next()?.replace('_', "-");
    let id: LanguageIdentifier = tag.parse().ok()?;
    let lang = id.language.as_str();
    SUPPORTED.contains(&lang).then(|| lang.to_string())
}

/// Where the language in effect came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The `--lang` flag.
    Flag,
    /// `language` in `config.toml`.
    Config,
    /// The operating system language.
    Os,
    /// Nothing usable was set: English.
    Default,
}

impl Source {
    /// Stable id, also the suffix of the `lang-source-*` messages.
    pub fn id(self) -> &'static str {
        match self {
            Source::Flag => "flag",
            Source::Config => "config",
            Source::Os => "os",
            Source::Default => "default",
        }
    }
}

/// The result of [`resolve_language`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub lang: String,
    pub source: Source,
    /// One line per value that was set but unusable and skipped.
    pub warnings: Vec<String>,
}

/// Picks the language: `--lang` flag, then `language` in the config, then the OS language
/// (`os`, see [`detect_os_language`]), then `en`. An unsupported or malformed flag or
/// config value is skipped with a warning and the next source decides.
pub fn resolve_language(flag: Option<&str>, config: Option<&str>, os: Option<&str>) -> Resolved {
    let mut warnings = Vec::new();
    for (value, source, name) in [
        (flag, Source::Flag, "--lang"),
        (config, Source::Config, "config.toml `language`"),
    ] {
        let Some(v) = value else { continue };
        match normalize(v) {
            Some(lang) => {
                return Resolved {
                    lang,
                    source,
                    warnings,
                };
            }
            None => warnings.push(format!(
                "{name}: unsupported language {v:?} (supported: {}); ignored",
                SUPPORTED.join(", ")
            )),
        }
    }
    match os.and_then(normalize) {
        Some(lang) => Resolved {
            lang,
            source: Source::Os,
            warnings,
        },
        None => Resolved {
            lang: "en".into(),
            source: Source::Default,
            warnings,
        },
    }
}

pub struct Localizer {
    lang: String,
    primary: Option<Bundle>,
    english: Bundle,
    reported: Mutex<HashSet<String>>,
}

impl Localizer {
    /// `lang` is a language tag such as `ru` or `ru-RU`; an unknown one means English.
    pub fn new(lang: &str) -> Self {
        let code = normalize(lang).unwrap_or_else(|| "en".to_string());
        let primary = (code != "en").then(|| bundle(&code, source(&code)));
        Self {
            lang: code,
            primary,
            english: bundle("en", EN),
            reported: Mutex::new(HashSet::new()),
        }
    }

    /// The language in effect (`"en"` or `"ru"`).
    pub fn language(&self) -> &str {
        &self.lang
    }

    pub fn t(&self, key: &str) -> String {
        self.lookup(key, None)
    }

    pub fn t_args(&self, key: &str, args: &FluentArgs) -> String {
        self.lookup(key, Some(args))
    }

    /// The message for `key` in the language in effect or English, or `None` when neither has it
    /// (no warning is logged: the caller has its own fallback).
    pub fn try_t(&self, key: &str) -> Option<String> {
        self.primary
            .iter()
            .chain(std::iter::once(&self.english))
            .find_map(|b| format(b, key, None))
    }

    fn lookup(&self, key: &str, args: Option<&FluentArgs>) -> String {
        for b in self.primary.iter().chain(std::iter::once(&self.english)) {
            if let Some(text) = format(b, key, args) {
                return text;
            }
        }
        let first = self
            .reported
            .lock()
            .map(|mut set| set.insert(key.to_string()))
            .unwrap_or(false);
        if first {
            tracing::warn!(key, lang = %self.lang, "missing translation key");
        }
        key.to_string()
    }
}

/// The strings of plugins: one Fluent bundle per `(plugin id, language)`, from the plugin's own
/// `i18n/<lang>.ftl` (built-in plugins ship theirs in `crates/phoecord-plugins/i18n/<lang>/<id>.ftl`).
/// Behind `Host::t`. Lookup: the requested language, then English, then the key (logged once).
#[derive(Default)]
pub struct PluginStrings {
    bundles: RwLock<HashMap<(String, String), Bundle>>,
    reported: Mutex<HashSet<String>>,
}

impl PluginStrings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or replace) the bundle of `plugin` for `lang`. A syntax error keeps the messages
    /// that did parse.
    pub fn set(&self, plugin: &str, lang: &str, ftl: &str) {
        let Some(code) = normalize(lang) else {
            tracing::warn!(
                plugin,
                lang,
                "plugin strings: unsupported language; ignored"
            );
            return;
        };
        let b = bundle(&code, ftl);
        self.bundles
            .write()
            .unwrap()
            .insert((plugin.to_string(), code), b);
    }

    /// Replace everything `plugin` has with `files` (`(lang, ftl text)` pairs). Used when a plugin
    /// package is (re)loaded, so a removed `.ftl` file disappears too.
    pub fn replace_plugin(&self, plugin: &str, files: &[(String, String)]) {
        let mut next = HashMap::new();
        for (lang, ftl) in files {
            match normalize(lang) {
                Some(code) => {
                    next.insert((plugin.to_string(), code.clone()), bundle(&code, ftl));
                }
                None => tracing::warn!(
                    plugin,
                    lang,
                    "plugin strings: unsupported language; ignored"
                ),
            }
        }
        let mut all = self.bundles.write().unwrap();
        all.retain(|(p, _), _| p != plugin);
        all.extend(next);
    }

    /// Drop every bundle of `plugin`.
    pub fn remove_plugin(&self, plugin: &str) {
        self.bundles
            .write()
            .unwrap()
            .retain(|(p, _), _| p != plugin);
    }

    /// The message `key` of `plugin` in `lang`, English, or the key itself. `args` are string
    /// values for `{ $name }` placeholders.
    pub fn t(&self, plugin: &str, lang: &str, key: &str, args: &[(&str, &str)]) -> String {
        let mut fluent = FluentArgs::new();
        for (k, v) in args {
            fluent.set(k.to_string(), v.to_string());
        }
        let code = normalize(lang).unwrap_or_else(|| "en".to_string());
        let bundles = self.bundles.read().unwrap();
        for l in [code.as_str(), "en"] {
            if let Some(text) = bundles
                .get(&(plugin.to_string(), l.to_string()))
                .and_then(|b| format(b, key, Some(&fluent)))
            {
                return text;
            }
        }
        drop(bundles);
        let first = self
            .reported
            .lock()
            .map(|mut set| set.insert(format!("{plugin}/{key}")))
            .unwrap_or(false);
        if first {
            tracing::warn!(plugin, key, lang = %code, "missing plugin translation key");
        }
        key.to_string()
    }
}

fn source(code: &str) -> &'static str {
    match code {
        "ru" => RU,
        _ => EN,
    }
}

fn bundle(code: &str, ftl: &str) -> Bundle {
    let id: LanguageIdentifier = code.parse().expect("built-in language code is valid");
    // On a syntax error keep the messages that did parse.
    let res = FluentResource::try_new(ftl.to_string()).unwrap_or_else(|(res, _errors)| res);
    let mut b = FluentBundle::new_concurrent(vec![id]);
    b.set_use_isolating(false);
    b.add_resource_overriding(res);
    b
}

fn format(b: &Bundle, key: &str, args: Option<&FluentArgs>) -> Option<String> {
    let pattern = b.get_message(key)?.value()?;
    let mut errors = vec![];
    let text = b.format_pattern(pattern, args, &mut errors);
    Some(text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(name: &str) -> FluentArgs<'static> {
        let mut a = FluentArgs::new();
        a.set("name", name.to_string());
        a
    }

    #[test]
    fn lookup_in_both_languages() {
        assert_eq!(Localizer::new("en").t("proxy-stopped"), "Proxy stopped.");
        assert_eq!(
            Localizer::new("ru").t("proxy-stopped"),
            "Прокси остановлен."
        );
    }

    #[test]
    fn arguments_are_substituted() {
        assert_eq!(
            Localizer::new("en").t_args("plugin-enabled", &args("Spoof")),
            "Plugin Spoof enabled."
        );
        assert_eq!(
            Localizer::new("ru").t_args("plugin-enabled", &args("Spoof")),
            "Плагин Spoof включён."
        );
    }

    #[test]
    fn missing_key_returns_the_key() {
        let l = Localizer::new("ru");
        assert_eq!(l.t("no-such-key"), "no-such-key");
        assert_eq!(l.t("no-such-key"), "no-such-key"); // second call is silent
        assert_eq!(l.reported.lock().unwrap().len(), 1);
    }

    #[test]
    fn russian_falls_back_to_english() {
        // `en` has the key, a stripped `ru` bundle does not.
        let mut l = Localizer::new("ru");
        l.primary = Some(bundle("ru", "other = x"));
        assert_eq!(l.t("proxy-stopped"), "Proxy stopped.");
    }

    #[test]
    fn unknown_language_is_english() {
        for tag in ["xx", "", "tlh-KX", "not a tag"] {
            let l = Localizer::new(tag);
            assert_eq!(l.language(), "en");
            assert_eq!(l.t("proxy-stopped"), "Proxy stopped.");
        }
    }

    #[test]
    fn region_and_posix_tags_normalize() {
        assert_eq!(Localizer::new("ru-RU").language(), "ru");
        assert_eq!(Localizer::new("ru_RU.UTF-8").language(), "ru");
        assert_eq!(normalize("de-DE"), None);
    }

    #[test]
    fn resolution_order() {
        let r = resolve_language;
        let x = r(Some("ru"), Some("en"), Some("en"));
        assert_eq!((x.lang.as_str(), x.source), ("ru", Source::Flag));
        let x = r(None, Some("ru"), Some("en"));
        assert_eq!((x.lang.as_str(), x.source), ("ru", Source::Config));
        let x = r(None, None, Some("ru-RU"));
        assert_eq!((x.lang.as_str(), x.source), ("ru", Source::Os));
        let x = r(None, None, None);
        assert_eq!((x.lang.as_str(), x.source), ("en", Source::Default));
        assert!(x.warnings.is_empty());
    }

    #[test]
    fn bad_values_fall_through_with_a_warning() {
        let x = resolve_language(Some("de"), Some("ru"), None);
        assert_eq!((x.lang.as_str(), x.source), ("ru", Source::Config));
        assert_eq!(x.warnings.len(), 1);
        let x = resolve_language(Some("??"), Some("fr"), Some("ru"));
        assert_eq!((x.lang.as_str(), x.source), ("ru", Source::Os));
        assert_eq!(x.warnings.len(), 2);
        let x = resolve_language(Some("de"), None, Some("de"));
        assert_eq!((x.lang.as_str(), x.source), ("en", Source::Default));
        assert_eq!(x.warnings.len(), 1);
    }

    #[test]
    fn plugin_strings_fall_back_to_english_then_the_key() {
        let s = PluginStrings::new();
        s.set(
            "p",
            "en",
            "hi = Hello, { $who }!
only-en = English only",
        );
        s.set("p", "ru", "hi = Привет, { $who }!");
        assert_eq!(s.t("p", "ru", "hi", &[("who", "Мир")]), "Привет, Мир!");
        assert_eq!(s.t("p", "en", "hi", &[("who", "World")]), "Hello, World!");
        assert_eq!(s.t("p", "ru", "only-en", &[]), "English only");
        assert_eq!(s.t("p", "ru", "nope", &[]), "nope");
        assert_eq!(
            s.t("other", "ru", "hi", &[]),
            "hi",
            "bundles are per plugin"
        );
        s.replace_plugin("p", &[("en".into(), "hi = New".into())]);
        assert_eq!(s.t("p", "ru", "hi", &[]), "New");
        s.remove_plugin("p");
        assert_eq!(s.t("p", "en", "hi", &[]), "hi");
    }
}
