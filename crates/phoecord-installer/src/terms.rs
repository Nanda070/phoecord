//! Terms of use (`TERMS.md`, `TERMS.ru.md`): the embedded texts and which version the user accepted.
//!
//! The accepted version is a one-line file in the data folder, recorded in `install.log` so
//! `uninstall` removes it. Nothing is installed before [`needs_acceptance`] is false.

use crate::ca::create_dir_logged;
use crate::log::{InstallLog, Step};
use crate::system::Env;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// The version of the terms in the files; keep in step with the `Version:` line in both.
pub const TERMS_VERSION: u32 = 1;
/// File in the data folder holding the accepted version.
pub const FILE_NAME: &str = "terms-accepted";

const EN: &str = include_str!("../../../TERMS.md");
const RU: &str = include_str!("../../../TERMS.ru.md");

/// The terms in `lang` (`"ru"`; anything else is English).
pub fn text(lang: &str) -> &'static str {
    if lang == "ru" { RU } else { EN }
}

fn path(env: &Env) -> PathBuf {
    env.data_dir.join(FILE_NAME)
}

/// The accepted version, or `None` when nothing (or something unreadable) is stored.
pub fn accepted(env: &Env) -> Option<u32> {
    std::fs::read_to_string(path(env)).ok()?.trim().parse().ok()
}

/// True when nothing is stored or the stored version is lower than [`TERMS_VERSION`].
pub fn needs_acceptance(env: &Env) -> bool {
    accepted(env).is_none_or(|v| v < TERMS_VERSION)
}

/// Store `version` as accepted and log the file so `uninstall` removes it.
pub fn accept(env: &Env, version: u32) -> Result<()> {
    let log = InstallLog::in_dir(&env.data_dir);
    create_dir_logged(&log, &env.data_dir)?;
    let p = path(env);
    std::fs::write(&p, format!("{version}\n"))
        .with_context(|| format!("writing {}", p.display()))?;
    log.record(Step::File { path: p })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{
        Os,
        fake::{FakeSystem, env},
    };

    #[test]
    fn files_carry_the_version_constant() {
        assert!(text("en").contains(&format!("Version: {TERMS_VERSION}")));
        assert!(text("ru").contains(&format!("Версия: {TERMS_VERSION}")));
        assert_ne!(text("en"), text("ru"));
    }

    #[test]
    fn fresh_folder_needs_acceptance_then_not() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        assert!(needs_acceptance(&e));
        accept(&e, TERMS_VERSION).unwrap();
        assert_eq!(accepted(&e), Some(TERMS_VERSION));
        assert!(!needs_acceptance(&e));
    }

    #[test]
    fn newer_terms_version_asks_again() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        // What a user who accepted the previous version has stored.
        accept(&e, TERMS_VERSION - 1).unwrap();
        assert!(needs_acceptance(&e));
        accept(&e, TERMS_VERSION).unwrap();
        assert!(!needs_acceptance(&e));
    }

    #[test]
    fn garbage_counts_as_not_accepted() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        std::fs::create_dir_all(&e.data_dir).unwrap();
        std::fs::write(path(&e), "yes").unwrap();
        assert!(needs_acceptance(&e));
    }

    #[test]
    fn accepting_twice_logs_once_and_uninstall_removes_it() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Linux, t.path());
        accept(&e, TERMS_VERSION).unwrap();
        accept(&e, TERMS_VERSION).unwrap();
        let files = InstallLog::in_dir(&e.data_dir)
            .read()
            .unwrap()
            .iter()
            .filter(|x| matches!(&x.step, Step::File { path } if path.ends_with(FILE_NAME)))
            .count();
        assert_eq!(files, 1);
        let r = crate::uninstall::uninstall_all(&e, &FakeSystem::default()).unwrap();
        assert!(r.is_clean());
        assert!(!path(&e).exists());
    }
}
