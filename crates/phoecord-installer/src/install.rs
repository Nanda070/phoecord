//! The install steps in order: data folder and CA, trust, autostart. Each step is idempotent and
//! logged to `install.log`; see [`crate::uninstall::uninstall_all`] for the way back.
//!
//! Installing the loader DLL into a client (stage 1.5) will add its own steps here, taking the
//! [`crate::discover::Client`] to patch.

use crate::Channel;
use crate::autostart::install_autostart;
use crate::ca::ensure_ca;
use crate::log::{Entry, InstallLog};
use crate::shortcut::{Outcome, install_shortcut};
use crate::system::{Env, System};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug)]
pub struct InstallReport {
    pub ca_pem: PathBuf,
    /// Everything now recorded in the log (earlier runs included).
    pub log: Vec<Entry>,
    /// Number of log lines this run added; 0 means everything was already in place.
    pub added: usize,
    /// What happened to the menu shortcut.
    pub shortcut: Outcome,
}

/// Create and trust the CA and register autostart. Stops at the first failing step; what was done
/// before it stays logged, so `uninstall_all` still undoes it.
pub fn install(env: &Env, sys: &dyn System) -> Result<InstallReport> {
    install_with(env, sys, None)
}

/// [`install`], and with a `channel` also a menu shortcut that starts that client through the proxy.
pub fn install_with(
    env: &Env,
    sys: &dyn System,
    channel: Option<Channel>,
) -> Result<InstallReport> {
    let log = InstallLog::in_dir(&env.data_dir);
    let before = log.read()?.len();
    let ca_pem = ensure_ca(env, sys, &log)?;
    install_autostart(env, sys, &log)?;
    let shortcut = match channel {
        Some(c) => install_shortcut(env, sys, &log, c)?,
        None => Outcome::NotRequested,
    };
    let entries = log.read()?;
    Ok(InstallReport {
        ca_pem,
        added: entries.len() - before,
        shortcut,
        log: entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Step;
    use crate::system::{
        Os,
        fake::{FakeSystem, env},
    };

    fn run(os: Os) -> (tempfile::TempDir, Env, FakeSystem, InstallReport) {
        let t = tempfile::tempdir().unwrap();
        let e = env(os, t.path());
        let sys = FakeSystem::default();
        let r = install(&e, &sys).unwrap();
        (t, e, sys, r)
    }

    #[test]
    fn linux_writes_unit_and_logs_every_step() {
        let (_t, e, sys, r) = run(Os::Linux);
        assert!(e.data_dir.join("ca.pem").is_file() && e.data_dir.join("ca.key").is_file());
        let unit =
            std::fs::read_to_string(e.config_home.join("systemd/user/phoecord.service")).unwrap();
        assert!(unit.contains("phoecord\" run"));
        assert_eq!(
            sys.calls(),
            vec![
                format!("trust_ca {}", e.data_dir.join("ca.pem").display()),
                "systemd_enable phoecord.service".to_string()
            ]
        );
        let steps: Vec<_> = r.log.iter().map(|e| &e.step).collect();
        assert!(steps.contains(&&Step::SystemdUnit {
            unit: "phoecord.service".into()
        }));
        assert!(steps.iter().any(|s| matches!(s, Step::CaTrusted { .. })));
        // Folders the installer made are logged, outermost first.
        let first = steps
            .iter()
            .position(|s| matches!(s, Step::Dir { .. }))
            .unwrap();
        assert_eq!(first, 0);
    }

    #[test]
    fn windows_sets_run_key() {
        let (_t, e, sys, _r) = run(Os::Windows);
        let want = format!("run_key_set PhoeCord \"{}\" run", e.proxy_exe.display());
        assert!(sys.calls().contains(&want), "{:?}", sys.calls());
    }

    #[test]
    fn macos_writes_plist_and_loads_it() {
        let (_t, e, sys, _r) = run(Os::MacOs);
        let plist = crate::autostart::plist_path(&e);
        let text = std::fs::read_to_string(&plist).unwrap();
        assert!(text.contains("dev.phoecord.proxy") && text.contains("<string>run</string>"));
        assert!(
            sys.calls()
                .contains(&format!("launchctl_load {}", plist.display()))
        );
    }

    #[test]
    fn second_run_changes_nothing() {
        for os in [Os::Windows, Os::MacOs, Os::Linux] {
            let (_t, e, sys, first) = run(os);
            let calls = sys.calls().len();
            let again = install(&e, &sys).unwrap();
            assert_eq!(again.added, 0, "{os:?}");
            assert_eq!(again.log, first.log, "{os:?}");
            assert_eq!(sys.calls().len(), calls, "{os:?}: no system call repeated");
        }
    }

    #[test]
    fn existing_ca_is_not_claimed_by_the_log() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        phoecord_proxy::ca::Authority::load_or_create(&e.data_dir).unwrap();
        let r = install(&e, &FakeSystem::default()).unwrap();
        assert!(!r.log.iter().any(|e| matches!(&e.step, Step::File { path } if path.ends_with("ca.pem") || path.ends_with("ca.key"))));
    }

    #[test]
    fn failed_trust_leaves_no_false_log_line() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        *sys.fail_on.borrow_mut() = Some("trust_ca".into());
        assert!(install(&e, &sys).is_err());
        let log = InstallLog::in_dir(&e.data_dir).read().unwrap();
        assert!(!log.iter().any(|e| matches!(e.step, Step::CaTrusted { .. })));
        assert!(log.iter().any(|e| matches!(e.step, Step::File { .. })));
    }
}
