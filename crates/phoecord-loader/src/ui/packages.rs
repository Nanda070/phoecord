//! Installing and removing WASM packages from the PhoeCord settings tab in Phoenix (task 2.3.3).
//!
//! Ruling 2026-11-27 (issue #206, a1): the tab links `phoecord-catalog::Store` and works on local
//! packages only. The user picks a `.zip` or a package folder, [`stage`] checks it into
//! `plugins-staging/`, [`confirm`] turns what it asks for into plain words, and on the user's yes
//! the package is committed in this process. The community catalog from the tab (a2) comes after
//! a separate review, so nothing here downloads.
//!
//! Like [`super::settings`], everything is plain data: the native page draws a [`Confirm`] or a
//! [`RemoveConfirm`] and calls back; the words come from the same strings as the installer's
//! Plugins tab (`gui-confirm-*`, `gui-asks-*`, `gui-remove-*`, `gui-pkg-err-*`).
//!
//! In an in-process launch [`engine_install`] / [`engine_remove`] also load or unload the package
//! in the loader's own engine. On the proxy transport the proxy watches `data/plugins/` and
//! `config.toml`, so [`Store::commit`] and [`Store::remove`] are all the tab has to call.

use phoecord_catalog::store::{self, Candidate, Error, Store};
use std::path::Path;

/// Rights that let a plugin act as the user; the confirm dialog marks them.
const STRONG_RIGHTS: &[&str] = &["rest", "emit_send"];

/// Words for the tab: a key and its Fluent arguments.
pub type Ui<'a> = &'a dyn Fn(&str, &[(&str, &str)]) -> String;

/// One line of the confirm dialog; `strong` lines are drawn as a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub strong: bool,
}

/// The dialog shown before a staged package is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub id: String,
    /// "Install <name>?"
    pub title: String,
    pub description: String,
    /// Version and authors.
    pub byline: String,
    /// "Replaces the installed version <v>." on an update.
    pub replaces: Option<String>,
    pub sees_title: String,
    /// The events it reads; one "nothing" line when it reads none.
    pub sees: Vec<Line>,
    pub can_title: String,
    /// What it may do, in plain words; one "nothing beyond reading" line when it asks no rights.
    pub can: Vec<Line>,
    pub connects_title: String,
    /// The hosts it may `fetch`; empty when it connects nowhere.
    pub connects: Vec<String>,
    pub trust: String,
    pub yes: String,
    pub no: String,
}

/// The dialog shown before a package is removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveConfirm {
    pub id: String,
    pub title: String,
    pub body: String,
    pub yes: String,
    pub no: String,
}

/// The package store over the data folder and `config.toml`.
pub fn store(data: &Path, config: &Path) -> Store {
    Store::new(data, config)
}

/// Stage the package at `path` (a `.zip`, a package folder, or its `plugin.toml` /
/// `plugin.wasm`), checking that its code loads. Takes a while (the WASM is compiled), so the
/// native page calls it off the client's UI thread.
pub fn stage(store: &Store, path: &Path) -> Result<Candidate, Error> {
    store.stage_path(path, &store::verify_package)
}

/// What the user is asked before `c` is installed.
pub fn confirm(c: &Candidate, ui: Ui<'_>) -> Confirm {
    let m = &c.manifest.manifest;
    let byline = if m.authors.is_empty() {
        ui("gui-plugins-meta-version", &[("version", &m.version)])
    } else {
        ui(
            "gui-plugins-meta",
            &[("version", &m.version), ("authors", &m.authors.join(", "))],
        )
    };
    let mut sees: Vec<Line> = c
        .asks
        .events
        .iter()
        .map(|e| Line {
            text: event_text(ui, e),
            strong: false,
        })
        .collect();
    if sees.is_empty() {
        sees.push(Line {
            text: ui("gui-asks-sees-nothing", &[]),
            strong: false,
        });
    }
    let mut can: Vec<Line> = c
        .asks
        .rights
        .iter()
        .map(|r| Line {
            text: right_text(ui, r),
            strong: STRONG_RIGHTS.contains(&r.as_str()),
        })
        .collect();
    if can.is_empty() {
        can.push(Line {
            text: ui("gui-asks-can-nothing", &[]),
            strong: false,
        });
    }
    Confirm {
        id: m.id.clone(),
        title: ui("gui-confirm-plugin-title", &[("name", &m.name)]),
        description: m.description.clone(),
        byline,
        replaces: c
            .replaces
            .as_ref()
            .map(|v| ui("gui-confirm-plugin-replaces", &[("version", v)])),
        sees_title: ui("gui-asks-sees", &[]),
        sees,
        can_title: ui("gui-asks-can", &[]),
        can,
        connects_title: ui("gui-asks-connects", &[]),
        connects: c.asks.fetch.clone(),
        trust: ui("gui-confirm-plugin-trust", &[]),
        yes: ui("gui-confirm-install", &[]),
        no: ui("gui-cancel", &[]),
    }
}

/// `rest_request:POST /channels/*/messages` → "Requests your Discord sends: POST …".
fn event_text(ui: Ui<'_>, event: &str) -> String {
    let (kind, detail) = event.split_once(':').unwrap_or((event, ""));
    let key = match kind {
        "rest_request" => "gui-event-rest-request",
        "rest_response" => "gui-event-rest-response",
        "dispatch" => "gui-event-dispatch",
        "send" => "gui-event-send",
        _ => "",
    };
    let what = if key.is_empty() {
        kind.to_string()
    } else {
        ui(key, &[])
    };
    let which = if detail.is_empty() || detail == "*" {
        ui("gui-event-all", &[])
    } else {
        detail.to_string()
    };
    format!("{what}: {which}")
}

/// The words for right `name`, the name itself when the bundle lacks them.
fn right_text(ui: Ui<'_>, name: &str) -> String {
    let key = format!("gui-right-{}", name.replace('_', "-"));
    let text = ui(&key, &[]);
    if text == key { name.to_string() } else { text }
}

/// What the user is asked before package `id` (shown as `name`) is removed.
pub fn remove_confirm(id: &str, name: &str, ui: Ui<'_>) -> RemoveConfirm {
    RemoveConfirm {
        id: id.to_string(),
        title: ui("gui-remove-title", &[("name", name)]),
        body: ui("gui-remove-body", &[]),
        yes: ui("gui-remove", &[]),
        no: ui("gui-cancel", &[]),
    }
}

/// Plain words for a package error, with what to do next.
pub fn error_text(e: &Error, ui: Ui<'_>) -> String {
    match e {
        Error::NotAPackage => ui("gui-pkg-err-not-package", &[]),
        Error::NoWasm => ui("gui-pkg-err-no-wasm", &[]),
        Error::TooBig => ui(
            "gui-pkg-err-too-big",
            &[(
                "mb",
                &(phoecord_catalog::store::MAX_BYTES / 1024 / 1024).to_string(),
            )],
        ),
        Error::UnsafePath(p) => ui("gui-pkg-err-unsafe", &[("path", p)]),
        Error::Manifest(why) => ui("gui-pkg-err-manifest", &[("why", why)]),
        Error::Broken(why) => ui("gui-pkg-err-broken", &[("why", why)]),
        Error::NotHttps => ui("gui-pkg-err-https", &[]),
        Error::Download(why) => ui("gui-pkg-err-download", &[("why", why)]),
        Error::Io(why) => ui("gui-pkg-err-io", &[("why", why)]),
        Error::Index(why) => ui("gui-pkg-err-index", &[("why", why)]),
        Error::NotInCatalog(_) => ui("gui-pkg-err-not-in-catalog", &[]),
        Error::HashMismatch => ui("gui-pkg-err-hash", &[]),
        Error::CatalogMismatch(why) => ui("gui-pkg-err-catalog-mismatch", &[("why", why)]),
    }
}

/// [`Ui`] over a localizer.
pub fn localized(tr: &phoecord_i18n::Localizer) -> impl Fn(&str, &[(&str, &str)]) -> String + '_ {
    move |key, vars| {
        let mut a = phoecord_i18n::Args::new();
        for (k, v) in vars {
            a.set(*k, v.to_string());
        }
        tr.t_args(key, &a)
    }
}

/// Install staged `c` and load it into the running `engine`: a first install is switched on, an
/// update keeps its data and switch. Compiles the WASM, so call it off the client's UI thread.
pub fn engine_install(
    engine: &phoecord_transport::Engine,
    store: &Store,
    c: &Candidate,
) -> Result<String, Error> {
    let id = store.commit(c)?;
    refresh(engine);
    Ok(id)
}

/// Remove package `id` (its folder, its data and its config entry) and unload it from `engine`.
pub fn engine_remove(
    engine: &phoecord_transport::Engine,
    store: &Store,
    id: &str,
) -> Result<String, Error> {
    let id = store.remove(id)?;
    refresh(engine);
    Ok(id)
}

/// The store wrote `config.toml` itself: hand the engine the file as it is now (under the config
/// lock), then load and unload packages to match `data/plugins/`.
fn refresh(engine: &phoecord_transport::Engine) {
    if let Ok(cfg) = engine.shared.persist(|_| Ok(())) {
        engine.apply(cfg);
    }
    engine.sync_packages();
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"
id = "hello_wasm"
name = "Hello"
version = "1.2.0"
description = "Says hello"
authors = ["Aigerim"]
api = 1
events = ["dispatch:MESSAGE_CREATE", "rest_request:*"]
rights = ["kv", "rest"]
fetch = ["api.example.com"]
"#;

    /// Keys come back as `key{arg=value,…}`, so a test sees which string and arguments were used.
    fn ui(key: &str, vars: &[(&str, &str)]) -> String {
        if vars.is_empty() {
            return key.to_string();
        }
        let args: Vec<String> = vars.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{key}{{{}}}", args.join(","))
    }

    fn package(dir: &Path, toml: &str) -> std::path::PathBuf {
        let p = dir.join("src-pkg");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("plugin.toml"), toml).unwrap();
        std::fs::write(p.join("plugin.wasm"), b"\0asm").unwrap();
        p
    }

    fn staged(store: &Store, dir: &Path, toml: &str) -> Candidate {
        store
            .stage_path(&package(dir, toml), &|_: &Path| Ok(()))
            .unwrap()
    }

    fn fixture() -> (tempfile::TempDir, Store) {
        let t = tempfile::tempdir().unwrap();
        let s = store(&t.path().join("data"), &t.path().join("config.toml"));
        (t, s)
    }

    #[test]
    fn confirm_says_what_the_package_asks_for() {
        let (t, s) = fixture();
        let c = staged(&s, t.path(), TOML);
        let d = confirm(&c, &ui);
        assert_eq!(d.id, "hello_wasm");
        assert_eq!(d.title, "gui-confirm-plugin-title{name=Hello}");
        assert_eq!(d.byline, "gui-plugins-meta{version=1.2.0,authors=Aigerim}");
        assert_eq!(d.replaces, None);
        let sees: Vec<&str> = d.sees.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            sees,
            [
                "gui-event-dispatch: MESSAGE_CREATE",
                "gui-event-rest-request: gui-event-all"
            ]
        );
        let can: Vec<(&str, bool)> = d.can.iter().map(|l| (l.text.as_str(), l.strong)).collect();
        assert_eq!(can, [("kv", false), ("rest", true)]);
        assert_eq!(d.connects, ["api.example.com"]);
        assert_eq!(
            (d.yes.as_str(), d.no.as_str()),
            ("gui-confirm-install", "gui-cancel")
        );
    }

    #[test]
    fn confirm_of_a_quiet_package_says_nothing_is_asked() {
        let (t, s) = fixture();
        let quiet = "id = \"quiet\"\nname = \"Quiet\"\nversion = \"0.1.0\"\napi = 1\n";
        let d = confirm(&staged(&s, t.path(), quiet), &ui);
        assert_eq!(d.byline, "gui-plugins-meta-version{version=0.1.0}");
        assert_eq!(d.sees.len(), 1);
        assert_eq!(d.sees[0].text, "gui-asks-sees-nothing");
        assert_eq!(d.can.len(), 1);
        assert_eq!(d.can[0].text, "gui-asks-can-nothing");
        assert!(d.connects.is_empty());
    }

    #[test]
    fn install_switches_on_and_an_update_says_what_it_replaces() {
        let (t, s) = fixture();
        let c = staged(&s, t.path(), TOML);
        assert_eq!(s.commit(&c).unwrap(), "hello_wasm");
        let cfg = phoecord_transport::config::Config::load(&s.config).unwrap();
        assert!(cfg.is_enabled("hello_wasm"));
        assert!(s.root.join("hello_wasm/plugin.wasm").is_file());

        std::fs::remove_dir_all(t.path().join("src-pkg")).unwrap();
        let newer = TOML.replace("1.2.0", "1.3.0");
        let d = confirm(&staged(&s, t.path(), &newer), &ui);
        assert_eq!(
            d.replaces.as_deref(),
            Some("gui-confirm-plugin-replaces{version=1.2.0}")
        );
    }

    #[test]
    fn a_staged_package_said_no_to_leaves_nothing() {
        let (t, s) = fixture();
        let c = staged(&s, t.path(), TOML);
        s.discard(&c);
        assert!(!s.staging.exists());
        assert!(s.list().is_empty());
    }

    #[test]
    fn remove_deletes_the_package_and_its_switch() {
        let (t, s) = fixture();
        s.commit(&staged(&s, t.path(), TOML)).unwrap();
        let d = remove_confirm("hello_wasm", "Hello", &ui);
        assert_eq!(d.title, "gui-remove-title{name=Hello}");
        assert_eq!(s.remove(&d.id).unwrap(), "hello_wasm");
        assert!(!s.root.join("hello_wasm").exists());
        let cfg = phoecord_transport::config::Config::load(&s.config).unwrap();
        assert!(!cfg.plugins.contains_key("hello_wasm"));
    }

    #[test]
    fn not_a_package_is_refused_with_what_to_do() {
        let (t, s) = fixture();
        let file = t.path().join("notes.txt");
        std::fs::write(&file, "hi").unwrap();
        let e = stage(&s, &file).unwrap_err();
        assert_eq!(e, Error::NotAPackage);
        assert_eq!(error_text(&e, &ui), "gui-pkg-err-not-package");
        assert_eq!(
            error_text(&Error::TooBig, &ui),
            format!(
                "gui-pkg-err-too-big{{mb={}}}",
                phoecord_catalog::store::MAX_BYTES / 1024 / 1024
            )
        );
    }

    #[test]
    fn every_string_the_dialogs_use_exists_in_both_languages() {
        let keys = [
            "gui-confirm-plugin-title",
            "gui-confirm-plugin-replaces",
            "gui-confirm-plugin-trust",
            "gui-confirm-install",
            "gui-cancel",
            "gui-plugins-meta",
            "gui-plugins-meta-version",
            "gui-asks-sees",
            "gui-asks-sees-nothing",
            "gui-asks-can",
            "gui-asks-can-nothing",
            "gui-asks-connects",
            "gui-event-rest-request",
            "gui-event-rest-response",
            "gui-event-dispatch",
            "gui-event-send",
            "gui-event-all",
            "gui-remove-title",
            "gui-remove-body",
            "gui-remove",
            "gui-pkg-err-not-package",
            "gui-pkg-err-no-wasm",
            "gui-pkg-err-too-big",
            "gui-pkg-err-unsafe",
            "gui-pkg-err-manifest",
            "gui-pkg-err-broken",
            "gui-pkg-err-https",
            "gui-pkg-err-download",
            "gui-pkg-err-io",
            "gui-pkg-err-index",
            "gui-pkg-err-not-in-catalog",
            "gui-pkg-err-hash",
            "gui-pkg-err-catalog-mismatch",
        ];
        for lang in ["en", "ru"] {
            let tr = phoecord_i18n::Localizer::new(lang);
            for k in keys {
                assert!(tr.try_t(k).is_some(), "{lang} lacks {k}");
            }
        }
    }
}
