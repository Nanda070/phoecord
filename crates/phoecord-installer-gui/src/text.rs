//! Localized strings for the window: `gui-*` messages in `phoecord-i18n` (English source,
//! Russian translation). The language follows `language` in `config.toml`, else the OS.

use phoecord_i18n::{Localizer, resolve_language};

/// The window's strings in one language.
pub struct Tr {
    l: Localizer,
}

impl Tr {
    /// `setting`: `language` from the config (`None`: the OS language).
    pub fn new(setting: Option<&str>) -> Tr {
        let os = phoecord_i18n::detect_os_language();
        let r = resolve_language(None, setting, os.as_deref());
        Tr {
            l: Localizer::new(&r.lang),
        }
    }

    pub fn lang(&self) -> &str {
        self.l.language()
    }

    pub fn t(&self, key: &str) -> String {
        self.l.t(key)
    }

    /// A message with `{ $name }` variables.
    pub fn f(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let mut a = phoecord_i18n::Args::new();
        for (k, v) in vars {
            a.set(*k, v.to_string());
        }
        self.l.t_args(key, &a)
    }

    pub fn try_t(&self, key: &str) -> Option<String> {
        self.l.try_t(key)
    }

    /// The translated description of a built-in plugin, else `fallback`.
    pub fn plugin_desc(&self, id: &str, fallback: &str) -> String {
        self.l
            .try_t(&format!("plugin-desc-{id}"))
            .unwrap_or_else(|| fallback.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `"gui-…"` literal in the window's sources.
    fn requested_keys() -> Vec<String> {
        let mut keys = Vec::new();
        for src in [
            include_str!("app.rs"),
            include_str!("tabs/catalog.rs"),
            include_str!("tabs/install.rs"),
            include_str!("tabs/plugins.rs"),
            include_str!("tabs/settings.rs"),
            include_str!("update.rs"),
        ] {
            for part in src.split('"').skip(1).step_by(2) {
                if part.starts_with("gui-")
                    && !part.ends_with('-')
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    keys.push(part.to_string());
                }
            }
        }
        for k in [
            "gui-err-cert",
            "gui-err-autostart",
            "gui-err-shortcut",
            "gui-err-generic",
        ] {
            keys.push(k.to_string());
            keys.push(format!("{k}-title"));
        }
        for r in phoecord_plugin_runtime::wasm::Right::ALL {
            keys.push(format!("gui-right-{}", r.name().replace('_', "-")));
        }
        keys.sort();
        keys.dedup();
        keys
    }

    #[test]
    fn every_key_exists_in_english_and_russian() {
        let en = Tr::new(Some("en"));
        let ru = Tr::new(Some("ru"));
        let keys = requested_keys();
        assert!(keys.len() > 100);
        for k in keys {
            assert!(en.try_t(&k).is_some(), "missing in en: {k}");
            // Russian falls back to English, so compare with the English text.
            let r = Localizer::new("ru").try_t(&k);
            assert!(r.is_some(), "missing: {k}");
            if ![
                "gui-client-stable",
                "gui-client-ptb",
                "gui-client-canary",
                "gui-client-phoenix",
            ]
            .contains(&k.as_str())
            {
                assert_ne!(ru.t(&k), en.t(&k), "not translated: {k}");
            }
        }
    }
}
