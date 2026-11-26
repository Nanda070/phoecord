//! PhoeCord's own updates in the window (task 2.4.5, ruling Q4): before the window opens, a
//! downloaded and verified update is installed (the setup or the new window takes over and this
//! process exits); once it is open, a background check downloads the next one. The logic is
//! `phoecord_installer::update`; nothing here runs in `--dry-run`.

use crate::backend::{Backend, Mode, Msg};
use phoecord_installer::system::{Env, RealSystem};
use phoecord_installer::update::{self, Checked, HttpFetch, Updater, Who};
use phoecord_proxy::config::Config;

/// What the window tells the user about updates (a toast, and the Settings line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// The update to `to` went in since the last start.
    Applied { from: String, to: String },
    /// An update could not be installed; the window opens anyway.
    Failed(String),
    /// Downloaded and checked; installed the next time the window opens.
    Ready(String),
    /// A newer release exists but this copy is not updated automatically.
    Manual { version: String, page: String },
}

fn auto_enabled(mode: &Mode) -> bool {
    Config::load(&mode.paths().config).map_or(true, |c| update::auto_enabled(&c.extra))
}

fn this_exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// The `phoecord` binary next to this window decides the install kind, like the CLI's own check.
fn proxy_exe() -> std::path::PathBuf {
    this_exe().with_file_name(format!("phoecord{}", std::env::consts::EXE_SUFFIX))
}

/// Before the window opens. Exits the process when an update takes over.
pub fn at_start(mode: &Mode) -> Vec<Note> {
    if mode.is_dry() {
        return Vec::new();
    }
    let fetch = HttpFetch;
    let data = mode.paths().data;
    let u = Updater::system(&data, &proxy_exe(), &fetch);
    let mut notes = Vec::new();
    if let Ok(Some(a)) = u.settle() {
        let _ = u.mark_shown();
        notes.push(Note::Applied {
            from: a.from,
            to: a.to,
        });
    }
    if !auto_enabled(mode) {
        return notes;
    }
    if let Ok(Some((_, file))) = u.ready() {
        let autostart = Env::system()
            .as_ref()
            .is_ok_and(phoecord_installer::autostart::is_installed);
        if let Some(actions) = update::plan(&u.kind, &file, Who::Window, &this_exe(), autostart) {
            let restart = || {
                let env = Env::system()?;
                phoecord_installer::uninstall::stop_proxies(&env, &RealSystem).map(|_| ())
            };
            let _ = u.begin_apply();
            match update::execute(&actions, &restart) {
                Ok(true) => std::process::exit(0),
                Ok(false) => {}
                Err(e) => {
                    let why = format!("{e:#}");
                    let _ = u.record_error(&why);
                    notes.push(Note::Failed(why));
                }
            }
        }
    }
    notes
}

/// Once the window is open: check (at most once a day) and download in the background.
pub fn check_in_background(backend: &Backend) {
    if backend.mode.is_dry() || !auto_enabled(&backend.mode) {
        return;
    }
    let data = backend.mode.paths().data;
    backend.spawn_job("update-check", move || {
        let fetch = HttpFetch;
        let u = Updater::system(&data, &proxy_exe(), &fetch);
        match u.check(update::now(), false) {
            Ok(Checked::Ready(p)) => Some(Msg::Update(Note::Ready(p.version))),
            Ok(Checked::Manual { version, page }) => {
                Some(Msg::Update(Note::Manual { version, page }))
            }
            // A failed check is not worth a toast: it is kept in `state.json` (`error`), which
            // the Settings line shows.
            Ok(_) | Err(_) => None,
        }
    });
}

/// The last update error kept in `state.json`, for the Settings line.
pub fn last_error(mode: &Mode) -> Option<String> {
    if mode.is_dry() {
        return None;
    }
    let fetch = HttpFetch;
    Updater::system(&mode.paths().data, &proxy_exe(), &fetch)
        .state()
        .error
}
