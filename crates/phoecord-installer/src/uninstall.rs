//! `uninstall_all`: undo every step in `install.log`, newest first, and say what was removed.
//!
//! Only what the log lists is touched. A step that fails stays in the log (so running uninstall again
//! retries it) and is reported; the other steps still run.

use crate::log::{Entry, InstallLog, Step};
use crate::system::{Env, System};
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct UninstallReport {
    /// What was removed.
    pub removed: Vec<Step>,
    /// Things left on purpose (a folder that still holds the user's files) or already gone.
    pub kept: Vec<(Step, Kept)>,
    /// `(step, why)` for steps that could not be undone; they remain in the log. `why` is the
    /// system's own error text.
    pub failed: Vec<(Step, String)>,
    /// Process ids of running PhoeCord proxies that were stopped first.
    pub stopped: Vec<u32>,
    /// Why the running proxies could not be stopped (the files are removed anyway).
    pub stop_failed: Option<String>,
}

/// Why a step was not undone but is not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    AlreadyGone,
    NotEmpty,
    /// The data folder, which still holds the log of failed steps.
    HoldsLog,
    /// A loader DLL whose content is no longer what PhoeCord wrote there: someone else's file now.
    NotOurs,
}

impl Kept {
    /// English text, for logs; the CLI localizes by variant.
    pub fn describe(self) -> &'static str {
        match self {
            Kept::AlreadyGone => "already gone",
            Kept::NotEmpty => "not empty",
            Kept::HoldsLog => "holds the log of failed steps",
            Kept::NotOurs => "changed by another program, left in place",
        }
    }
}

impl UninstallReport {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty() && self.stop_failed.is_none()
    }
}

enum Outcome {
    Removed,
    Kept(Kept),
}

fn remove_file(path: &Path) -> Result<Outcome> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(Outcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Outcome::Kept(Kept::AlreadyGone)),
        Err(e) => Err(e.into()),
    }
}

fn remove_dir(path: &Path) -> Result<Outcome> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(Outcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Outcome::Kept(Kept::AlreadyGone)),
        // Not empty: it holds something that is not ours (plugins, config). Leave it.
        Err(_) if path.read_dir().is_ok_and(|mut d| d.next().is_some()) => {
            Ok(Outcome::Kept(Kept::NotEmpty))
        }
        Err(e) => Err(e.into()),
    }
}

/// Undo one `step`; `entries` is the whole log (a loader copy checks the hashes recorded for it).
fn undo(step: &Step, entries: &[Entry], sys: &dyn System) -> Result<Outcome> {
    match step {
        Step::Dir { path } => remove_dir(path),
        Step::File { path } => remove_file(path),
        Step::CaTrusted { .. } => sys.untrust_ca().map(|()| Outcome::Removed),
        Step::RunKey { name, .. } => sys.run_key_delete(name).map(|()| Outcome::Removed),
        Step::LaunchAgent { plist, .. } => {
            // Unloading a plist that is already gone fails; that is not worth failing the step.
            if plist.exists() {
                sys.launchctl_unload(plist)?;
            }
            Ok(Outcome::Removed)
        }
        Step::SystemdUnit { unit } => sys.systemd_disable(unit).map(|()| Outcome::Removed),
        // Only a record of the user's choice: nothing on disk to undo.
        Step::LoaderWanted { .. } => Ok(Outcome::Removed),
        // Ours only while it still holds bytes PhoeCord wrote there (this copy or a later update of
        // it); anything else is someone else's file now and stays.
        Step::LoaderCopy { path, .. } => match crate::loader::file_sha256(path)? {
            None => Ok(Outcome::Kept(Kept::AlreadyGone)),
            Some(h) if crate::loader::is_our_copy(entries, path, &h) => remove_file(path),
            Some(_) => Ok(Outcome::Kept(Kept::NotOurs)),
        },
    }
}

/// Stop the running PhoeCord proxies (`phoecord stop`): those of this binary and of every binary
/// the install log names. The Windows setup runs this before it replaces files (ruling 2.4.5 Q7);
/// the autostart entry or the window starts the new one.
pub fn stop_proxies(env: &Env, sys: &dyn System) -> Result<Vec<u32>> {
    let entries = InstallLog::in_dir(&env.data_dir).read()?;
    sys.stop_proxies(&proxy_exes(env, &entries))
}

/// Undo everything the installer logged, newest step first.
pub fn uninstall_all(env: &Env, sys: &dyn System) -> Result<UninstallReport> {
    let log = InstallLog::in_dir(&env.data_dir);
    let entries = log.read()?;
    let mut report = UninstallReport::default();
    // A proxy that keeps running holds the port and the CA in memory: stop it before undoing anything.
    match sys.stop_proxies(&proxy_exes(env, &entries)) {
        Ok(pids) => report.stopped = pids,
        Err(e) => report.stop_failed = Some(format!("{e:#}")),
    }
    let mut failed: Vec<Entry> = Vec::new(); // newest first
    remove_own_ca_files(env, &entries, &mut report);
    let had_log = log.path().exists();
    for (i, entry) in entries.iter().enumerate().rev() {
        // The log lives in the data folder: once nothing but folders and the log are left to undo,
        // it goes too, or the folder could never be removed.
        if matches!(&entry.step, Step::Dir { path } if *path == env.data_dir)
            && failed.is_empty()
            && entries[..i]
                .iter()
                .all(|e| matches!(e.step, Step::Dir { .. }))
            && std::fs::remove_file(log.path()).is_ok()
        {
            report.removed.push(Step::File {
                path: log.path().to_path_buf(),
            });
        }
        let is_data_dir = matches!(&entry.step, Step::Dir { path } if *path == env.data_dir);
        let outcome = if is_data_dir && !failed.is_empty() {
            // The log (and what the failed steps still need) lives here: try again next run.
            failed.push(entry.clone());
            report.kept.push((entry.step.clone(), Kept::HoldsLog));
            None
        } else {
            Some(undo(&entry.step, &entries, sys))
        };
        match outcome {
            None => {}
            Some(r) => match r {
                Ok(Outcome::Removed) => report.removed.push(entry.step.clone()),
                Ok(Outcome::Kept(why)) => report.kept.push((entry.step.clone(), why)),
                Err(e) => {
                    report.failed.push((entry.step.clone(), format!("{e:#}")));
                    failed.push(entry.clone());
                }
            },
        }
        // Keep the log in step with reality so an interrupted run can be resumed. Best effort: once
        // the data folder is gone there is no log left to write.
        let mut rest = entries[..i].to_vec();
        rest.extend(failed.iter().rev().cloned());
        if log.path().parent().is_some_and(Path::exists) {
            let _ = log.rewrite(&rest);
        }
    }
    if failed.is_empty() {
        remove_leftovers(env, &log, had_log, &mut report);
    }
    Ok(report)
}

/// The binaries that may be running as the proxy: the one recorded in the autostart entry, and the
/// current one.
fn proxy_exes(env: &Env, entries: &[Entry]) -> Vec<PathBuf> {
    let mut out = vec![env.proxy_exe.clone()];
    let mut add = |p: Option<PathBuf>| {
        if let Some(p) = p.filter(|p| !out.contains(p)) {
            out.push(p);
        }
    };
    for e in entries {
        match &e.step {
            Step::RunKey { value, .. } => {
                // `"<exe>" run`
                add(value
                    .strip_prefix('"')
                    .and_then(|v| v.split_once('"'))
                    .map(|(exe, _)| PathBuf::from(exe)));
            }
            Step::LaunchAgent { plist, .. } => {
                let text = std::fs::read_to_string(plist).unwrap_or_default();
                let after = text.split("<array>").nth(1).unwrap_or("");
                add(after
                    .split_once("<string>")
                    .and_then(|(_, r)| r.split_once("</string>"))
                    .map(|(exe, _)| PathBuf::from(exe)));
            }
            Step::File { path } if path.extension().is_some_and(|x| x == "service") => {
                let text = std::fs::read_to_string(path).unwrap_or_default();
                add(text
                    .lines()
                    .find_map(|l| l.strip_prefix("ExecStart=\""))
                    .and_then(|l| l.split_once('"'))
                    .map(|(exe, _)| PathBuf::from(exe)));
            }
            _ => {}
        }
    }
    out
}

/// `ca.pem` and `ca.key` in the data folder are PhoeCord's by definition, whichever command made them
/// (`launch`, `run` and `paths` create the CA too, and only `setup` logs). Files the log lists are
/// undone by their own step.
fn remove_own_ca_files(env: &Env, entries: &[Entry], report: &mut UninstallReport) {
    for name in ["ca.pem", "ca.key"] {
        let path = env.data_dir.join(name);
        let logged = entries
            .iter()
            .any(|e| matches!(&e.step, Step::File { path: p } if *p == path));
        if !logged && std::fs::remove_file(&path).is_ok() {
            report.removed.push(Step::File { path });
        }
    }
}

/// With every step undone: drop `install.log` if still there and the data folder if it is empty
/// (it may predate PhoeCord's log). `config.toml` and anything else the user put there stays.
fn remove_leftovers(env: &Env, log: &InstallLog, had_log: bool, report: &mut UninstallReport) {
    // The loop deletes the log itself once nothing is left to write to it; report it all the same.
    let _ = std::fs::remove_file(log.path());
    let already = report
        .removed
        .iter()
        .any(|s| matches!(s, Step::File { path } if path == log.path()));
    if had_log && !already {
        report.removed.push(Step::File {
            path: log.path().to_path_buf(),
        });
    }
    if std::fs::remove_dir(&env.data_dir).is_ok() {
        report.removed.push(Step::Dir {
            path: env.data_dir.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::install;
    use crate::system::{
        Os,
        fake::{FakeSystem, env},
    };

    /// Every file and folder under `root`, as relative paths.
    fn tree(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                out.push(
                    e.path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
                if e.path().is_dir() {
                    walk(&e.path(), root, out);
                }
            }
        }
        let mut v = Vec::new();
        walk(root, root, &mut v);
        v.sort();
        v
    }

    #[test]
    fn round_trip_leaves_nothing_on_every_os() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let t = tempfile::tempdir().unwrap();
            let e = env(os, t.path());
            let sys = FakeSystem::default();
            install(&e, &sys).unwrap();
            assert!(!tree(t.path()).is_empty());

            let r = uninstall_all(&e, &sys).unwrap();
            assert!(r.is_clean(), "{os:?}: {:?}", r.failed);
            assert!(r.kept.is_empty(), "{os:?}: {:?}", r.kept);
            assert_eq!(tree(t.path()), Vec::<String>::new(), "{os:?}: leftovers");

            let calls = sys.calls();
            let undo_call = match os {
                Os::Windows => "run_key_delete PhoeCord",
                Os::MacOs => "launchctl_unload",
                Os::Linux => "systemd_disable phoecord.service",
            };
            let at = |needle: &str| calls.iter().position(|c| c.starts_with(needle)).unwrap();
            assert!(calls.contains(&"untrust_ca".to_string()));
            // Reverse order: autostart is undone before the CA.
            assert!(at(undo_call) < at("untrust_ca"), "{os:?}: {calls:?}");
            // And the report names what it removed.
            assert!(
                r.removed
                    .iter()
                    .any(|s| matches!(s, Step::CaTrusted { .. }))
            );
        }
    }

    #[test]
    fn keeps_folders_that_hold_other_files_and_what_it_did_not_create() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        // The user already had a plugins folder in the data dir, and an unrelated ca pair is theirs.
        std::fs::create_dir_all(e.data_dir.join("plugins")).unwrap();
        std::fs::write(e.data_dir.join("plugins/mine.wasm"), b"x").unwrap();
        let sys = FakeSystem::default();
        install(&e, &sys).unwrap();
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.is_clean());
        assert!(e.data_dir.join("plugins/mine.wasm").is_file());
        assert!(!e.data_dir.join("ca.key").exists());
        assert!(!e.data_dir.join("install.log").exists());
    }

    #[test]
    fn failed_step_stays_in_the_log_and_the_rest_still_runs() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        install(&e, &sys).unwrap();
        *sys.fail_on.borrow_mut() = Some("untrust_ca".into());
        let r = uninstall_all(&e, &sys).unwrap();
        assert_eq!(r.failed.len(), 1);
        assert!(matches!(r.failed[0].0, Step::CaTrusted { .. }));
        assert!(!e.config_home.join("systemd/user/phoecord.service").exists());
        let left = InstallLog::in_dir(&e.data_dir).read().unwrap();
        assert!(
            left.iter()
                .any(|x| matches!(x.step, Step::CaTrusted { .. }))
        );
        // The data folder cannot go while the log is in it; ca.key is gone though.
        assert!(!e.data_dir.join("ca.key").exists());

        // Fixed: a second run finishes the job.
        *sys.fail_on.borrow_mut() = None;
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.is_clean(), "{:?}", r.failed);
        assert!(!e.data_dir.exists());
    }

    #[test]
    fn removes_ca_files_made_by_launch_and_keeps_the_users_config() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        // `launch` made the CA before `setup` ran, so the log never claimed it.
        phoecord_proxy::ca::Authority::load_or_create(&e.data_dir).unwrap();
        std::fs::write(e.data_dir.join("config.toml"), b"x").unwrap();
        let sys = FakeSystem::default();
        install(&e, &sys).unwrap();
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.is_clean(), "{:?}", r.failed);
        for gone in ["ca.pem", "ca.key", "install.log"] {
            assert!(!e.data_dir.join(gone).exists(), "{gone} left");
            assert!(
                r.removed
                    .iter()
                    .any(|s| matches!(s, Step::File { path } if path.ends_with(gone))),
                "{gone} not reported: {:?}",
                r.removed
            );
        }
        assert!(e.data_dir.join("config.toml").is_file());
        assert!(e.data_dir.exists());
    }

    #[test]
    fn removes_unlogged_ca_and_the_empty_folder_without_any_log() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        phoecord_proxy::ca::Authority::load_or_create(&e.data_dir).unwrap();
        let r = uninstall_all(&e, &FakeSystem::default()).unwrap();
        assert!(r.is_clean());
        assert!(!e.data_dir.exists());
        assert!(
            r.removed
                .iter()
                .any(|s| matches!(s, Step::Dir { path } if *path == e.data_dir))
        );
    }

    #[test]
    fn stops_running_proxies_first_and_reports_them() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        install(&e, &sys).unwrap();
        // The Run key may name another binary than the one running uninstall.
        let log = InstallLog::in_dir(&e.data_dir);
        let mut entries = log.read().unwrap();
        for en in &mut entries {
            if let Step::RunKey { value, .. } = &mut en.step {
                *value = "\"D:/old/phoecord.exe\" run".into();
            }
        }
        log.rewrite(&entries).unwrap();
        *sys.running.borrow_mut() = vec![4242];
        let r = uninstall_all(&e, &sys).unwrap();
        assert_eq!(r.stopped, vec![4242]);
        assert!(r.is_clean());
        let calls = sys.calls();
        let stop = calls
            .iter()
            .position(|c| c.starts_with("stop_proxies"))
            .unwrap();
        assert_eq!(
            stop,
            calls.len()
                - calls
                    .iter()
                    .rev()
                    .position(|c| c.starts_with("stop_proxies"))
                    .unwrap()
                - 1
        );
        assert!(stop < calls.iter().position(|c| c == "untrust_ca").unwrap());
        assert!(
            calls[stop].contains("D:/old/phoecord.exe"),
            "{}",
            calls[stop]
        );
        assert!(calls[stop].contains(&e.proxy_exe.display().to_string()));
    }

    #[test]
    fn a_proxy_that_cannot_be_stopped_is_reported_and_the_rest_still_runs() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        install(&e, &sys).unwrap();
        *sys.fail_on.borrow_mut() = Some("stop_proxies".into());
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.stop_failed.is_some() && !r.is_clean());
        assert!(!e.data_dir.join("ca.key").exists());
        assert!(sys.calls().contains(&"untrust_ca".to_string()));
    }

    #[test]
    fn nothing_logged_means_nothing_to_do() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.removed.is_empty() && r.kept.is_empty() && r.is_clean());
        assert!(r.stopped.is_empty());
        // Only the look for a running proxy; nothing else touched.
        assert_eq!(
            sys.calls(),
            vec![format!("stop_proxies {}", e.proxy_exe.display())]
        );
    }

    #[test]
    fn stop_only_stops_the_proxies_of_logged_binaries() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        assert!(!crate::autostart::is_installed(&e));
        install(&e, &sys).unwrap();
        assert!(crate::autostart::is_installed(&e));
        let before = sys.calls().len();
        stop_proxies(&e, &sys).unwrap();
        let calls = sys.calls();
        assert_eq!(calls.len(), before + 1, "{calls:?}");
        assert!(calls[before].starts_with("stop_proxies"));
        assert!(calls[before].contains(&e.proxy_exe.display().to_string()));
        assert!(InstallLog::in_dir(&e.data_dir).path().exists());
    }
}
