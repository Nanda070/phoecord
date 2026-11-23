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
    let trusted = Step::CaTrusted {
        ca_pem: ca.ca_pem_path.clone(),
    };
    if !log.contains(&trusted)? {
        sys.trust_ca(&ca.ca_pem_path)?;
        log.record(trusted)?;
    }
    Ok(ca.ca_pem_path)
}
