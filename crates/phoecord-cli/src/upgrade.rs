//! PhoeCord's own updates (task 2.4.5): `phoecord upgrade`, `phoecord stop`, and the automatic
//! update at the start of `phoecord run`. The logic is `phoecord_installer::update`; this file only
//! prints.

use crate::tf;
use anyhow::Result;
use phoecord_i18n::Localizer;
use phoecord_installer::system::{Env, RealSystem};
use phoecord_installer::update::{self, Checked, HttpFetch, Updater, Who};
use phoecord_proxy::config::{Config, Paths};
use std::path::PathBuf;

/// The installer window, next to this binary.
fn window_exe() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!(
        "phoecord-installer{}",
        std::env::consts::EXE_SUFFIX
    ))
}

fn updater<'a>(paths: &Paths, fetch: &'a HttpFetch) -> Updater<'a> {
    let exe = std::env::current_exe().unwrap_or_default();
    Updater::system(&paths.data, &exe, fetch)
}

fn auto_enabled(paths: &Paths) -> bool {
    Config::load(&paths.config).map_or(true, |c| update::auto_enabled(&c.extra))
}

/// Start of `phoecord run` (autostart): say what was updated, install a downloaded update, and
/// check for a new one in the background. `true`: an update is being installed, exit now (the
/// setup or launchd starts the new `phoecord run`).
pub fn at_run_start(paths: &Paths, l10n: &Localizer) -> bool {
    let fetch = HttpFetch;
    let u = updater(paths, &fetch);
    if let Ok(Some(a)) = u.settle() {
        // The window shows it too (and marks it shown); here it is one log line.
        println!(
            "{}",
            tf(
                l10n,
                "cli-upgrade-applied",
                &[("from", &a.from), ("to", &a.to)]
            )
        );
    }
    if !auto_enabled(paths) {
        return false;
    }
    match u.ready() {
        Ok(Some((p, file))) => {
            let env = Env::system().ok();
            let autostart = env
                .as_ref()
                .is_some_and(phoecord_installer::autostart::is_installed);
            match update::plan(&u.kind, &file, Who::Proxy, &window_exe(), autostart) {
                Some(actions) => {
                    println!(
                        "{}",
                        tf(l10n, "cli-upgrade-installing", &[("version", &p.version)])
                    );
                    let restart = || stop_all().map(|_| ());
                    let _ = u.begin_apply();
                    match update::execute(&actions, &restart) {
                        Ok(_) => return true,
                        Err(e) => {
                            let why = format!("{e:#}");
                            let _ = u.record_error(&why);
                            eprintln!("{}", tf(l10n, "cli-upgrade-failed", &[("why", &why)]));
                        }
                    }
                }
                None => println!(
                    "{}",
                    tf(l10n, "cli-upgrade-use-window", &[("version", &p.version)])
                ),
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("update not applied: {e:#}"),
    }
    let data = paths.data.clone();
    std::thread::spawn(move || {
        let fetch = HttpFetch;
        let exe = std::env::current_exe().unwrap_or_default();
        match Updater::system(&data, &exe, &fetch).check(update::now(), false) {
            Ok(Checked::Ready(p)) => {
                tracing::info!(
                    "PhoeCord {} downloaded; installed at the next start",
                    p.version
                )
            }
            Ok(Checked::Manual { version, page }) => {
                tracing::info!("PhoeCord {version} is out: {page}")
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("update check: {e:#}"),
        }
    });
    false
}

/// `phoecord upgrade`: check now (ignoring the daily limit) and download a newer release.
pub fn run(paths: &Paths, l10n: &Localizer) -> Result<()> {
    let fetch = HttpFetch;
    let u = updater(paths, &fetch);
    if let Some(a) = u.settle()? {
        println!(
            "{}",
            tf(
                l10n,
                "cli-upgrade-applied",
                &[("from", &a.from), ("to", &a.to)]
            )
        );
    }
    let current = update::current_version().to_string();
    match u.check(update::now(), true) {
        Ok(Checked::UpToDate | Checked::NotDue) => {
            println!(
                "{}",
                tf(l10n, "cli-upgrade-up-to-date", &[("version", &current)])
            );
        }
        Ok(Checked::Ready(p)) => {
            println!(
                "{}",
                tf(l10n, "cli-upgrade-ready", &[("version", &p.version)])
            );
            if !auto_enabled(paths) {
                println!("{}", l10n.t("cli-upgrade-auto-off"));
            }
        }
        Ok(Checked::Manual { version, page }) => {
            println!(
                "{}",
                tf(
                    l10n,
                    "cli-upgrade-manual",
                    &[("version", &version), ("page", &page)]
                )
            );
        }
        Err(e) => {
            let key = if e.downcast_ref::<update::Refused>().is_some() {
                "cli-upgrade-refused"
            } else {
                "cli-upgrade-check-failed"
            };
            anyhow::bail!(tf(l10n, key, &[("why", &format!("{e:#}"))]))
        }
    }
    Ok(())
}

fn stop_all() -> Result<Vec<u32>> {
    let env = Env::system()?;
    phoecord_installer::uninstall::stop_proxies(&env, &RealSystem)
}

/// `phoecord stop`: stop the running proxies of this install (the Windows setup runs it before it
/// replaces files).
pub fn stop(l10n: &Localizer) -> Result<()> {
    let pids = stop_all()?;
    if pids.is_empty() {
        println!("{}", l10n.t("cli-stop-none"));
    }
    for pid in pids {
        println!(
            "{}",
            tf(l10n, "cli-uninstall-stopped", &[("pid", &pid.to_string())])
        );
    }
    Ok(())
}
