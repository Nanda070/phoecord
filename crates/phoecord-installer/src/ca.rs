//! Install step: create the local CA and trust it.

use crate::log::{InstallLog, Step};
use crate::system::{Env, System};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Create `dir` and every missing parent, logging each folder that did not exist (outermost first).
pub(crate) fn create_dir_logged(log: &InstallLog, dir: &Path) -> Result<()> {
    let mut missing: Vec<&Path> = dir
        .ancestors()
        .take_while(|p| !p.exists())
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    missing.reverse();
    // Create first, log after: the log itself lives in the data folder that may be one of these.
    let mut made = Vec::new();
    for p in missing {
        std::fs::create_dir(p).with_context(|| format!("creating {}", p.display()))?;
        made.push(p);
    }
    for p in made {
        log.record(Step::Dir {
            path: p.to_path_buf(),
        })?;
    }
    Ok(())
}

/// Make sure the data folder, `ca.pem` and `ca.key` exist, and that the CA is trusted.
/// Returns the path of `ca.pem`. Re-running changes nothing that is already in place.
pub fn ensure_ca(env: &Env, sys: &dyn System, log: &InstallLog) -> Result<PathBuf> {
    create_dir_logged(log, &env.data_dir)?;
    let pem = env.data_dir.join("ca.pem");
    let key = env.data_dir.join("ca.key");
    let had = (pem.exists(), key.exists());
    let ca = phoecord_proxy::ca::Authority::load_or_create(&env.data_dir)?;
    // Only files this call created go in the log: uninstall never deletes what it did not create.
    if !had.0 {
        log.record(Step::File { path: pem.clone() })?;
    }
    if !had.1 {
        log.record(Step::File { path: key })?;
    }
    let fingerprint = phoecord_proxy::trust::fingerprint(&ca.ca_pem_path)?;
    let entries = log.read()?;
    let logged = |e: &crate::log::Entry| matches!(&e.step, Step::CaTrusted { ca_pem, .. } if *ca_pem == ca.ca_pem_path);
    let current = entries.iter().any(|e| {
        matches!(&e.step, Step::CaTrusted { ca_pem, fingerprint: Some(f) }
            if *ca_pem == ca.ca_pem_path && *f == fingerprint)
    });
    // Same certificate as last time: only trust again if the stores no longer hold it. A failing check
    // counts as "still trusted" so a missing tool cannot make every `setup` prompt again.
    if !current || !sys.is_trusted(&ca.ca_pem_path).unwrap_or(true) {
        sys.trust_ca(&ca.ca_pem_path)?;
        // One line per CA file: uninstall removes the CA by name, so a stale line must not stay.
        if entries.iter().any(logged) {
            let kept: Vec<_> = entries.iter().filter(|e| !logged(e)).cloned().collect();
            log.rewrite(&kept)?;
        }
        log.record(Step::CaTrusted {
            ca_pem: ca.ca_pem_path.clone(),
            fingerprint: Some(fingerprint),
        })?;
    }
    Ok(ca.ca_pem_path)
}

/// Is the CA in `ca_pem` missing from the OS trust stores? `false` when it is trusted and also when
/// the check itself cannot run: a warning that may be wrong is worse than none.
pub fn is_untrusted(sys: &dyn System, ca_pem: &Path) -> bool {
    matches!(sys.is_trusted(ca_pem), Ok(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{
        Os,
        fake::{FakeSystem, env},
    };

    fn trust_calls(sys: &FakeSystem) -> usize {
        sys.calls()
            .iter()
            .filter(|c| c.starts_with("trust_ca"))
            .count()
    }

    fn recreate(e: &Env) {
        std::fs::remove_file(e.data_dir.join("ca.pem")).unwrap();
        std::fs::remove_file(e.data_dir.join("ca.key")).unwrap();
    }

    #[test]
    fn recreated_ca_is_trusted_again() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        let pem = ensure_ca(&e, &sys, &log).unwrap();
        let first = phoecord_proxy::trust::fingerprint(&pem).unwrap();
        recreate(&e);
        ensure_ca(&e, &sys, &log).unwrap();
        let second = phoecord_proxy::trust::fingerprint(&pem).unwrap();
        assert_ne!(first, second);
        assert_eq!(trust_calls(&sys), 2);
        assert!(sys.is_trusted(&pem).unwrap());
        // One CaTrusted line, holding the new fingerprint.
        let lines: Vec<_> = log
            .read()
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.step {
                Step::CaTrusted { fingerprint, .. } => Some(fingerprint),
                _ => None,
            })
            .collect();
        assert_eq!(lines, vec![Some(second)]);
    }

    #[test]
    fn same_ca_is_not_trusted_twice() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        ensure_ca(&e, &sys, &log).unwrap();
        ensure_ca(&e, &sys, &log).unwrap();
        assert_eq!(trust_calls(&sys), 1);
    }

    #[test]
    fn ca_gone_from_the_store_is_trusted_again() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        ensure_ca(&e, &sys, &log).unwrap();
        sys.store.borrow_mut().clear();
        ensure_ca(&e, &sys, &log).unwrap();
        assert_eq!(trust_calls(&sys), 2);
    }

    #[test]
    fn failing_check_does_not_retrust_a_logged_ca() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        ensure_ca(&e, &sys, &log).unwrap();
        *sys.check_fails.borrow_mut() = true;
        ensure_ca(&e, &sys, &log).unwrap();
        assert_eq!(trust_calls(&sys), 1);
    }

    #[test]
    fn old_log_line_without_fingerprint_trusts_again() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        let pem = ensure_ca(&e, &sys, &log).unwrap();
        // The log as an older PhoeCord wrote it: no fingerprint field.
        let old = format!(
            "{{\"ts\":1,\"step\":\"ca_trusted\",\"ca_pem\":{}}}\n",
            serde_json::to_string(&pem).unwrap()
        );
        std::fs::write(log.path(), old).unwrap();
        assert!(matches!(
            log.read().unwrap()[0].step,
            Step::CaTrusted {
                fingerprint: None,
                ..
            }
        ));
        ensure_ca(&e, &sys, &log).unwrap();
        assert_eq!(trust_calls(&sys), 2);
        ensure_ca(&e, &sys, &log).unwrap();
        assert_eq!(trust_calls(&sys), 2);
    }

    #[test]
    fn untrusted_warning_only_when_not_trusted() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        let sys = FakeSystem::default();
        let log = InstallLog::in_dir(&e.data_dir);
        let pem = phoecord_proxy::ca::Authority::load_or_create(&e.data_dir)
            .unwrap()
            .ca_pem_path;
        assert!(is_untrusted(&sys, &pem));
        ensure_ca(&e, &sys, &log).unwrap();
        assert!(!is_untrusted(&sys, &pem));
        recreate(&e);
        let pem = phoecord_proxy::ca::Authority::load_or_create(&e.data_dir)
            .unwrap()
            .ca_pem_path;
        assert!(is_untrusted(&sys, &pem), "a new CA is not the trusted one");
        *sys.check_fails.borrow_mut() = true;
        assert!(!is_untrusted(&sys, &pem), "unknown is not a warning");
    }
}
