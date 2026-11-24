//! The install log: `install.log` in PhoeCord's data folder, one JSON object per line.
//!
//! Every change the installer makes to the machine is appended here *after* it succeeded, with exactly
//! what was created, so `uninstall_all` can undo precisely that and nothing else.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "install.log";

/// One thing the installer created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// A folder the installer created (removed again only while empty).
    Dir { path: PathBuf },
    /// A file the installer created (`ca.pem`, `ca.key`, a unit file, a plist).
    File { path: PathBuf },
    /// The local CA was added to the OS trust stores. `fingerprint` is the SHA-256 (hex) of the
    /// certificate that was trusted; old logs have none, which means "unknown": trust again.
    CaTrusted {
        ca_pem: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fingerprint: Option<String>,
    },
    /// Windows per-user Run key value `HKCU\...\Run\<name>`.
    RunKey { name: String, value: String },
    /// macOS LaunchAgent loaded from `plist` (the file itself is a separate `File` step).
    LaunchAgent { label: String, plist: PathBuf },
    /// Linux systemd user unit enabled (the file itself is a separate `File` step).
    SystemdUnit { unit: String },
}

impl Step {
    /// A short human description, for reports.
    pub fn describe(&self) -> String {
        match self {
            Step::Dir { path } => format!("folder {}", path.display()),
            Step::File { path } => format!("file {}", path.display()),
            Step::CaTrusted { ca_pem, .. } => {
                format!("CA {} from the system trust stores", ca_pem.display())
            }
            Step::RunKey { name, .. } => format!("Run key value {name}"),
            Step::LaunchAgent { label, .. } => format!("LaunchAgent {label}"),
            Step::SystemdUnit { unit } => format!("systemd user unit {unit}"),
        }
    }
}

/// A log line: when, and what.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the Unix epoch.
    pub ts: u64,
    #[serde(flatten)]
    pub step: Step,
}

#[derive(Clone, Debug)]
pub struct InstallLog {
    path: PathBuf,
}

impl InstallLog {
    /// The log inside the data folder `data`.
    pub fn in_dir(data: &Path) -> InstallLog {
        InstallLog {
            path: data.join(FILE_NAME),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All entries, oldest first. A missing file is an empty log.
    pub fn read(&self) -> Result<Vec<Entry>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        };
        text.lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| {
                serde_json::from_str(l).with_context(|| {
                    format!("{} line {}: not a log entry", self.path.display(), i + 1)
                })
            })
            .collect()
    }

    pub fn contains(&self, step: &Step) -> Result<bool> {
        Ok(self.read()?.iter().any(|e| &e.step == step))
    }

    /// Append `step` unless it is already logged. Returns whether a line was written.
    pub fn record(&self, step: Step) -> Result<bool> {
        if self.contains(&step)? {
            return Ok(false);
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = serde_json::to_string(&Entry { ts, step })?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        writeln!(f, "{line}")?;
        f.sync_all()?;
        Ok(true)
    }

    /// Replace the log with `entries`; an empty list deletes the file.
    pub fn rewrite(&self, entries: &[Entry]) -> Result<()> {
        if entries.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            };
        }
        let mut text = String::new();
        for e in entries {
            text.push_str(&serde_json::to_string(e)?);
            text.push('\n');
        }
        std::fs::write(&self.path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_dedupe() {
        let t = tempfile::tempdir().unwrap();
        let log = InstallLog::in_dir(t.path());
        assert!(log.read().unwrap().is_empty());
        let a = Step::File {
            path: "/x/ca.pem".into(),
        };
        assert!(log.record(a.clone()).unwrap());
        assert!(!log.record(a.clone()).unwrap());
        log.record(Step::SystemdUnit {
            unit: "phoecord.service".into(),
        })
        .unwrap();
        let got = log.read().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].step, a);
        let raw = std::fs::read_to_string(log.path()).unwrap();
        assert!(raw.lines().next().unwrap().contains(r#""step":"file""#));
        log.rewrite(&got[1..]).unwrap();
        assert_eq!(log.read().unwrap().len(), 1);
        log.rewrite(&[]).unwrap();
        assert!(!log.path().exists());
    }

    #[test]
    fn corrupt_line_is_an_error_not_silence() {
        let t = tempfile::tempdir().unwrap();
        let log = InstallLog::in_dir(t.path());
        std::fs::write(log.path(), "{not json}\n").unwrap();
        assert!(log.read().is_err());
    }
}
