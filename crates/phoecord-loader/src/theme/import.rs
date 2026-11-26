//! Vencord theme import (task 2.3.2): a Vencord / BetterDiscord CSS theme to a `theme.toml`.
//!
//! A Vencord theme is CSS for the web client. What carries over to a native palette are its colour
//! custom properties: `--background-primary: #313338;` and the like, set on `:root`, `.theme-dark`
//! or `.visual-refresh`. [`import`] reads every such declaration, resolves `var(--x)` references
//! inside the file, keeps the ones [`MAP`] knows (the last one wins, as in CSS) and reports the
//! rest as unsupported. Everything else in the file (selectors, layouts, fonts, images) has no
//! counterpart and is not looked at.
//!
//! The palette field names are PhoeCord's own (`themes/default.toml`) until Phoenix's are known
//! (2.3.1); the map moves with them.

use super::parse::parse_color;
use std::collections::BTreeMap;

/// Discord CSS variable → PhoeCord palette field. Several variables may feed one field; the one
/// that appears last in the file wins.
pub const MAP: &[(&str, &str)] = &[
    ("background-primary", "background"),
    ("background-base-lower", "background"),
    ("bg-base-primary", "background"),
    ("background-secondary", "background.secondary"),
    ("background-base-lowest", "background.secondary"),
    ("bg-base-secondary", "background.secondary"),
    ("background-secondary-alt", "background.secondary"),
    ("background-tertiary", "background.tertiary"),
    ("bg-base-tertiary", "background.tertiary"),
    ("background-floating", "background.floating"),
    ("bg-surface-overlay", "background.floating"),
    ("text-normal", "text"),
    ("text-default", "text"),
    ("text-primary", "text"),
    ("text-muted", "text.muted"),
    ("text-secondary", "text.muted"),
    ("text-link", "text.link"),
    ("brand-experiment", "brand"),
    ("brand-500", "brand"),
    ("brand-new-500", "brand"),
    ("status-positive", "online"),
    ("green-360", "online"),
    ("status-warning", "idle"),
    ("yellow-300", "idle"),
    ("status-danger", "dnd"),
    ("red-400", "dnd"),
    ("status-offline", "offline"),
    ("primary-400", "offline"),
];

/// What [`import`] made of a CSS file.
#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    /// Palette field → colour as written in the file (after `var()` resolution).
    pub colors: BTreeMap<String, String>,
    /// Colour variables with no palette field, in file order, once each.
    pub unsupported: Vec<String>,
    /// Mapped variables whose value is not a colour PhoeCord reads (`name: why`).
    pub bad: Vec<String>,
}

impl Imported {
    /// The `theme.toml` text: `name`, then `[colors]` in field order.
    pub fn to_toml(&self, name: &str) -> String {
        let mut out = format!("name = {}\n\n[colors]\n", toml_str(name));
        for (k, v) in &self.colors {
            let key = if k.contains('.') {
                toml_str(k)
            } else {
                k.clone()
            };
            out.push_str(&format!("{key} = {}\n", toml_str(v)));
        }
        out
    }
}

fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `/* ... */` removed, so a commented-out declaration is not read.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Every `--name: value` declaration, in file order, `!important` dropped.
fn declarations(css: &str) -> Vec<(String, String)> {
    let css = strip_comments(css);
    let mut out = Vec::new();
    for chunk in css.split(['{', '}', ';']) {
        let chunk = chunk.trim();
        let Some(rest) = chunk.strip_prefix("--") else {
            continue;
        };
        let Some((name, value)) = rest.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        let value = value.trim().trim_end_matches("!important").trim();
        if !value.is_empty() {
            out.push((name.to_ascii_lowercase(), value.to_string()));
        }
    }
    out
}

/// `value` with `var(--x)` / `var(--x, fallback)` replaced from `vars`, up to a few levels.
fn resolve(value: &str, vars: &BTreeMap<String, String>) -> String {
    let mut v = value.to_string();
    for _ in 0..8 {
        let Some(start) = v.find("var(") else {
            return v;
        };
        let Some(len) = v[start..].find(')') else {
            return v;
        };
        let inner = &v[start + 4..start + len];
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner.trim(), None),
        };
        let name = name.trim_start_matches("--").to_ascii_lowercase();
        let with = match vars.get(&name) {
            Some(x) => x.clone(),
            None => match fallback {
                Some(f) => f.to_string(),
                None => return v,
            },
        };
        v.replace_range(start..start + len + 1, &with);
    }
    v
}

/// Looks like a colour value (so a non-colour variable such as `--font-display` is not listed as
/// unsupported).
fn colorish(v: &str) -> bool {
    let v = v.trim().to_ascii_lowercase();
    v.starts_with('#')
        || ["rgb(", "rgba(", "hsl(", "hsla("]
            .iter()
            .any(|p| v.starts_with(p))
}

/// Reads a Vencord CSS theme.
pub fn import(css: &str) -> Imported {
    let decls = declarations(css);
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for (n, v) in &decls {
        vars.insert(n.clone(), v.clone());
    }
    let mut out = Imported::default();
    for (name, raw) in &decls {
        let value = resolve(raw, &vars);
        match MAP.iter().find(|(css, _)| css == name) {
            Some((_, field)) => match parse_color(&value) {
                Ok(_) => {
                    out.colors.insert((*field).to_string(), value);
                }
                Err(e) => out.bad.push(format!("--{name}: {e}")),
            },
            None if colorish(&value) => {
                let n = format!("--{name}");
                if !out.unsupported.contains(&n) {
                    out.unsupported.push(n);
                }
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    const CSS: &str = r#"
/**
 * @name Midnight
 * @author someone
 */
@import url("https://example.com/base.css");
:root {
    --accent: #ff0080;
    --background-primary: #000;   /* the main panel */
    --background-secondary: rgb(10, 10, 12) !important;
    --text-normal: var(--accent);
    --text-link: var(--missing, #00f);
    --brand-500: var(--missing);
    --font-display: "gg sans";
    --custom-glow: hsl(300 100% 50%);
    /* --background-tertiary: #fff; */
}
.theme-dark { --background-primary: #050505; --status-danger: banana; }
"#;

    #[test]
    fn maps_resolves_and_reports() {
        let r = import(CSS);
        assert_eq!(
            r.colors.get("background").map(String::as_str),
            Some("#050505")
        );
        assert_eq!(
            r.colors.get("background.secondary").map(String::as_str),
            Some("rgb(10, 10, 12)")
        );
        assert_eq!(r.colors.get("text").map(String::as_str), Some("#ff0080"));
        assert_eq!(r.colors.get("text.link").map(String::as_str), Some("#00f"));
        assert!(
            !r.colors.contains_key("background.tertiary"),
            "commented out"
        );
        assert_eq!(r.unsupported, ["--accent", "--custom-glow"]);
        assert_eq!(r.bad.len(), 2, "{:?}", r.bad);
        assert!(r.bad.iter().any(|b| b.starts_with("--brand-500")));
        assert!(r.bad.iter().any(|b| b.starts_with("--status-danger")));
    }

    #[test]
    fn the_toml_loads_back_as_a_theme() {
        let text = import(CSS).to_toml("Mid\"night");
        let (theme, problems) = Theme::from_toml(&text, "x").unwrap();
        assert_eq!(theme.name, "Mid\"night");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(theme.colors.len(), 4);
    }

    #[test]
    fn an_empty_or_odd_file_gives_nothing() {
        assert_eq!(import(""), Imported::default());
        assert_eq!(import("a { color: red } /* unclosed"), Imported::default());
        let r = import("--x: var(--x); --text-muted: var(--x);");
        assert!(r.colors.is_empty());
    }
}
