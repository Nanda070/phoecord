//! `uninstall_all`: undo every step in `install.log`, newest first, and say what was removed.
//!
//! Only what the log lists is touched. A step that fails stays in the log (so running uninstall again
//! retries it) and is reported; the other steps still run.

use crate::log::{Entry, InstallLog, Step};
use crate::system::{Env, System};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Default)]
pub struct UninstallReport {
    /// Human descriptions of what was removed.
    pub removed: Vec<String>,
    /// Things left on purpose (a folder that still holds the user's files) or already gone.
    pub kept: Vec<String>,
    /// `(what, why)` for steps that could not be undone; they remain in the log.
    pub failed: Vec<(String, String)>,
}

impl UninstallReport {
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }
}

enum Outcome {
    Removed,
    Kept(&'static str),
}

fn remove_file(path: &Path) -> Result<Outcome> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(Outcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Outcome::Kept("already gone")),
        Err(e) => Err(e.into()),
    }
}

fn remove_dir(path: &Path) -> Result<Outcome> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(Outcome::Removed),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Outcome::Kept("already gone")),
        // Not empty: it holds something that is not ours (plugins, config). Leave it.
        Err(_) if path.read_dir().is_ok_and(|mut d| d.next().is_some()) => {
            Ok(Outcome::Kept("not empty"))
        }
        Err(e) => Err(e.into()),
    }
}

fn undo(step: &Step, sys: &dyn System) -> Result<Outcome> {
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
    }
}

/// Undo everything the installer logged, newest step first.
pub fn uninstall_all(env: &Env, sys: &dyn System) -> Result<UninstallReport> {
    let log = InstallLog::in_dir(&env.data_dir);
    let entries = log.read()?;
    let mut report = UninstallReport::default();
    let mut failed: Vec<Entry> = Vec::new(); // newest first
    for (i, entry) in entries.iter().enumerate().rev() {
        let what = entry.step.describe();
        // The log lives in the data folder: once nothing but folders and the log are left to undo,
        // it goes too, or the folder could never be removed.
        if matches!(&entry.step, Step::Dir { path } if *path == env.data_dir)
            && failed.is_empty()
            && entries[..i]
                .iter()
                .all(|e| matches!(e.step, Step::Dir { .. }))
        {
            let _ = std::fs::remove_file(log.path());
        }
        let is_data_dir = matches!(&entry.step, Step::Dir { path } if *path == env.data_dir);
        let outcome = if is_data_dir && !failed.is_empty() {
            // The log (and what the failed steps still need) lives here: try again next run.
            failed.push(entry.clone());
            report
                .kept
                .push(format!("{what} (holds the log of failed steps)"));
            None
        } else {
            Some(undo(&entry.step, sys))
        };
        match outcome {
            None => {}
            Some(r) => match r {
                Ok(Outcome::Removed) => report.removed.push(what),
                Ok(Outcome::Kept(why)) => report.kept.push(format!("{what} ({why})")),
                Err(e) => {
                    report.failed.push((what, format!("{e:#}")));
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
    Ok(report)
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
            assert!(r.removed.iter().any(|s| s.contains("CA")));
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
        assert!(r.failed[0].0.contains("CA"));
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
    fn nothing_logged_means_nothing_to_do() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let r = uninstall_all(&e, &sys).unwrap();
        assert!(r.removed.is_empty() && r.kept.is_empty() && r.is_clean());
        assert!(sys.calls().is_empty());
    }
}
