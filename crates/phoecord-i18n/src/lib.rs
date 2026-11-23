//! Localization for PhoeCord: Fluent bundles embedded in the binary.
//!
//! Lookup order for a key: requested language, then English, then the key itself
//! (a missing key is logged once). Other crates keep their own strings in
//! `crates/<crate>/i18n/{en,ru}/main.ftl`; `scripts/check_i18n.py` keeps the two in sync.

use std::collections::HashSet;
use std::sync::Mutex;

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
}
