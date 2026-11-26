//! Theme engine (Phase 2, task 2.3.1), first part: the `theme.toml` format and live reload.
//!
//! A theme is a flat map of palette field names to colours, read from `theme.toml`:
//!
//! ```toml
//! name = "Midnight"
//!
//! [colors]
//! background = "#1e1f22"
//! text = "hsla(220, 13%, 91%, 1)"
//! "element.hover" = "rgba(255, 255, 255, 0.06)"
//! ```
//!
//! Colours become [`Hsla`] (hue, saturation, lightness, alpha, each `0.0..=1.0`), the colour type
//! GPUI's palette uses (`docs/research/gpui.md` §1). A bad entry is skipped and reported; the rest
//! of the file still applies, so one typo does not undo a whole theme.
//!
//! [`ThemeWatcher`] polls the file every [`POLL_INTERVAL`] and hands back the new theme when it
//! changed: "editing `theme.toml` changes the colours within 2 seconds".
//!
//! `themes/default.toml` ships a starting-point theme with Discord's own published surface
//! colours, under field names of our own choosing — not yet a true no-op default, since Phoenix's
//! real palette field names are still unknown.
//!
//! Not here yet, because it needs an anchor the private `phoecord-recon` notes do not have:
//! which field names Phoenix's palette has, and the hook on the active theme global that would
//! write them (`docs/research/gpui.md` describes the general GPUI/Zed pattern — `cx.global::<T>()`
//! behind an `ActiveTheme`-style accessor — as a template to look for, not a finding about
//! Phoenix). Until the field names are known, they stay free-form (`lower_case`, digits, `_`,
//! `.`, `-`).

mod parse;

pub use parse::{ColorError, parse_color};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How often [`ThemeWatcher`] looks at the file.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A colour as GPUI keeps it: hue, saturation, lightness and alpha, each `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsla {
    pub h: f32,
    pub s: f32,
    pub l: f32,
    pub a: f32,
}

/// A loaded theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Theme {
    /// `name`, or the file name without `.toml`.
    pub name: String,
    /// Palette field name → colour, sorted by name.
    pub colors: BTreeMap<String, Hsla>,
}

/// An entry of `theme.toml` that was skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The key under `[colors]`, or the top-level key.
    pub key: String,
    pub reason: String,
}

/// Why a theme file could not be read at all.
#[derive(Debug)]
pub enum ThemeError {
    Io(std::io::Error),
    /// Not TOML, or `colors` is not a table.
    Format(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeError::Io(e) => write!(f, "cannot read the theme: {e}"),
            ThemeError::Format(e) => write!(f, "not a theme file: {e}"),
        }
    }
}

impl std::error::Error for ThemeError {}

/// Whether `key` can be a palette field name.
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
}

impl Theme {
    /// Read a theme from TOML text; `fallback_name` names it when the file has no `name`.
    pub fn from_toml(text: &str, fallback_name: &str) -> Result<(Theme, Vec<Problem>), ThemeError> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| ThemeError::Format(e.message().to_string()))?;
        let mut problems = Vec::new();
        let name = match table.get("name") {
            Some(toml::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(_) => {
                problems.push(Problem {
                    key: "name".into(),
                    reason: "not a text".into(),
                });
                fallback_name.to_string()
            }
            None => fallback_name.to_string(),
        };
        for key in table.keys() {
            if key != "name" && key != "colors" {
                problems.push(Problem {
                    key: key.clone(),
                    reason: "unknown key (only `name` and `[colors]`)".into(),
                });
            }
        }
        let mut colors = BTreeMap::new();
        match table.get("colors") {
            None => {}
            Some(toml::Value::Table(entries)) => {
                for (key, value) in entries {
                    let problem = |reason: String| Problem {
                        key: key.clone(),
                        reason,
                    };
                    if !valid_key(key) {
                        problems.push(problem(
                            "not a field name (lower case, digits, `_`, `.`, `-`)".into(),
                        ));
                        continue;
                    }
                    match value {
                        toml::Value::String(s) => match parse_color(s) {
                            Ok(c) => {
                                colors.insert(key.clone(), c);
                            }
                            Err(e) => problems.push(problem(e.to_string())),
                        },
                        _ => problems.push(problem("a colour is text, e.g. \"#1e1f22\"".into())),
                    }
                }
            }
            Some(_) => return Err(ThemeError::Format("`colors` must be a table".into())),
        }
        Ok((Theme { name, colors }, problems))
    }

    /// Read a theme file.
    pub fn load(path: &Path) -> Result<(Theme, Vec<Problem>), ThemeError> {
        let text = std::fs::read_to_string(path).map_err(ThemeError::Io)?;
        let fallback = path
            .file_stem()
            .map_or_else(|| "theme".into(), |s| s.to_string_lossy().into_owned());
        Theme::from_toml(&text, &fallback)
    }
}

/// Watches one theme file and reloads it when it changes.
pub struct ThemeWatcher {
    path: PathBuf,
    /// Modification time and length last seen; `None` before the first look or while missing.
    seen: Option<(SystemTime, u64)>,
}

/// What [`ThemeWatcher::poll`] found.
#[derive(Debug)]
pub enum Change {
    /// The file is new or changed: the theme, or why it could not be read.
    Loaded(Result<(Theme, Vec<Problem>), String>),
    /// The file was there and is gone: go back to the client's own colours.
    Removed,
}

impl ThemeWatcher {
    pub fn new(path: impl Into<PathBuf>) -> ThemeWatcher {
        ThemeWatcher {
            path: path.into(),
            seen: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Look at the file once (call every [`POLL_INTERVAL`]); `None` when nothing changed.
    pub fn poll(&mut self) -> Option<Change> {
        let stamp = std::fs::metadata(&self.path)
            .ok()
            .and_then(|m| Some((m.modified().ok()?, m.len())));
        match (stamp, self.seen) {
            (None, None) => None,
            (None, Some(_)) => {
                self.seen = None;
                Some(Change::Removed)
            }
            (Some(now), Some(before)) if now == before => None,
            (Some(now), _) => {
                self.seen = Some(now);
                Some(Change::Loaded(
                    Theme::load(&self.path).map_err(|e| e.to_string()),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hsla(h: f32, s: f32, l: f32, a: f32) -> Hsla {
        Hsla { h, s, l, a }
    }

    #[test]
    fn reads_a_theme_and_reports_bad_entries() {
        let (theme, problems) = Theme::from_toml(
            r##"
            name = "Midnight"
            extra = 1
            [colors]
            background = "#000000"
            text = "hsla(0, 0%, 100%, 0.5)"
            "element.hover" = "#fff"
            Border = "#123456"
            border = "pink"
            panel = 5
            "##,
            "file",
        )
        .unwrap();
        assert_eq!(theme.name, "Midnight");
        assert_eq!(theme.colors.len(), 3);
        assert_eq!(theme.colors["background"], hsla(0.0, 0.0, 0.0, 1.0));
        assert_eq!(theme.colors["text"], hsla(0.0, 0.0, 1.0, 0.5));
        assert_eq!(theme.colors["element.hover"], hsla(0.0, 0.0, 1.0, 1.0));
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, vec!["extra", "Border", "border", "panel"]);
    }

    /// The shipped `themes/default.toml` (task 2.3.1) parses cleanly: every key is valid and every
    /// colour decodes, so a typo there is caught here rather than by whoever loads it first.
    #[test]
    fn the_shipped_default_theme_parses_with_no_problems() {
        let text = include_str!("../../../../themes/default.toml");
        let (theme, problems) = Theme::from_toml(text, "default").unwrap();
        assert_eq!(theme.name, "Discord (default)");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(theme.colors.len(), 12);
    }

    #[test]
    fn a_file_without_a_name_or_colors_still_loads() {
        let (theme, problems) = Theme::from_toml("", "plain").unwrap();
        assert_eq!(theme.name, "plain");
        assert!(theme.colors.is_empty() && problems.is_empty());
        assert!(Theme::from_toml("colors = 5", "x").is_err());
        assert!(Theme::from_toml("not toml [", "x").is_err());
    }

    #[test]
    fn the_watcher_reports_new_changed_and_removed_files() {
        let dir = std::env::temp_dir().join(format!("phoecord-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("theme.toml");
        let _ = std::fs::remove_file(&path);
        let mut w = ThemeWatcher::new(&path);
        assert!(w.poll().is_none(), "missing from the start: nothing to do");

        std::fs::write(&path, "[colors]\nbackground = \"#000\"\n").unwrap();
        match w.poll() {
            Some(Change::Loaded(Ok((t, _)))) => assert_eq!(t.name, "theme"),
            other => panic!("{other:?}"),
        }
        assert!(w.poll().is_none(), "unchanged");

        // A different length is seen even within the file system's time resolution.
        std::fs::write(&path, "name = \"B\"\n[colors]\nbackground = \"#ffffff\"\n").unwrap();
        match w.poll() {
            Some(Change::Loaded(Ok((t, _)))) => assert_eq!(t.name, "B"),
            other => panic!("{other:?}"),
        }
        std::fs::write(&path, "colors = 7 # broken while typing\n").unwrap();
        assert!(matches!(w.poll(), Some(Change::Loaded(Err(_)))));

        std::fs::remove_file(&path).unwrap();
        assert!(matches!(w.poll(), Some(Change::Removed)));
        assert!(w.poll().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
