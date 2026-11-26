//! The per-launch transport choice for Desktop Phoenix (task 2.2.8): what `phoecord launch` tells
//! the loader it injects, and the check cache both of them write.
//!
//! `phoecord launch --channel phoenix` picks the transport before the client starts, because
//! `--proxy-server` can only be passed at process start (rulings, 2026-11-26). It runs the hook
//! locators on `Discord.exe` on disk ([`crate::check::transport_plan`]) and keeps the verdict in
//! [`CheckCache`] (`phoenix-check.json` in PhoeCord's data folder), keyed by the client's `.text`
//! hash and the signature folder's hash ([`crate::signatures::folder_hash`]): a Phoenix update or
//! a signature hot-fix both miss the cache and re-run the check.
//!
//! The launcher passes the decision to the injected loader in its environment:
//!
//! | Variable | Value |
//! |:---|:---|
//! | [`ENV_MODE`] | `in-process` or `proxy` ([`Mode`]) |
//! | [`ENV_CACHE`] | the cache file, for [`mark_failed`] |
//! | [`ENV_SIGNATURES_HASH`] | the signature folder's hash the decision was made with |
//!
//! Two seams are left for other tasks, both documented here and not called yet:
//!
//! - [`Launched::from_env`]: the loader reads its mode. In `proxy` mode it must not install the
//!   network hooks (the traffic would pass the plugin chain twice) and does the UI only; in
//!   `in-process` mode it runs the `Engine` itself and reads `config.toml` (2.2.7).
//! - [`mark_failed`]: a hook that fails to install or panics during an in-process session marks
//!   the build failed, so the next launch goes straight to the proxy until the signature folder
//!   changes (ruling, 2026-11-26). Written from inside the loader once the detours exist (2.2.2,
//!   2.2.3, 2.2.7).
//!
//! Plain file and environment access, no OS code: built with and without the `dll` feature.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The launch's transport, as the launcher decided it.
pub const ENV_MODE: &str = "PHOECORD_TRANSPORT";
/// The check cache file the loader writes the "failed" mark to.
pub const ENV_CACHE: &str = "PHOECORD_CHECK_CACHE";
/// The signature folder's hash the launch was decided with (the "failed" mark is keyed by it).
pub const ENV_SIGNATURES_HASH: &str = "PHOECORD_SIGNATURES_HASH";
/// The cache's file name in PhoeCord's data folder.
pub const CACHE_FILE: &str = "phoenix-check.json";

/// Where a Phoenix launch's network traffic goes. All or nothing per launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// The loader's hooks run the plugin chain; no proxy process at all.
    InProcess,
    /// The proxy runs the plugin chain; the loader does the UI only.
    Proxy,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::InProcess => "in-process",
            Mode::Proxy => "proxy",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "in-process" => Some(Mode::InProcess),
            "proxy" => Some(Mode::Proxy),
            _ => None,
        }
    }
}

/// What `phoecord launch` told this process (the loader side of the seam).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launched {
    pub mode: Mode,
    /// Where to write the "failed" mark; `None` when the launcher gave none (a forced mode).
    pub cache: Option<PathBuf>,
    pub signatures_hash: Option<String>,
}

impl Launched {
    /// From this process's environment; `None` when it was not started by `phoecord launch` (an
    /// Electron client with the sideloaded DLL, `phoecord dev inject`): such a process keeps the
    /// proxy-era behaviour.
    pub fn from_env() -> Option<Launched> {
        Launched::from_vars(|k| std::env::var(k).ok())
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Launched> {
        let mode = Mode::parse(&get(ENV_MODE)?)?;
        Some(Launched {
            mode,
            cache: get(ENV_CACHE).filter(|s| !s.is_empty()).map(PathBuf::from),
            signatures_hash: get(ENV_SIGNATURES_HASH).filter(|s| !s.is_empty()),
        })
    }
}

/// What the check found for one client build with one signature folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub text_hash: String,
    pub signatures_hash: String,
    /// What the on-disk check decided; `None` when the entry is only a "failed" mark.
    #[serde(default)]
    pub mode: Option<Mode>,
    /// The entry points that were not found, one line each (empty for in-process).
    #[serde(default)]
    pub missing: Vec<String>,
    /// A hook failed during a session of this build with these signatures: the next launches use
    /// the proxy until the signature folder changes.
    #[serde(default)]
    pub failed: bool,
}

/// The `.text` hash of the exe as last read, so a launch with an unchanged file does not hash its
/// few hundred MB again. Only a shortcut to the key: the key itself is the `.text` hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExeStamp {
    pub path: PathBuf,
    pub len: u64,
    /// Modification time, nanoseconds since the Unix epoch.
    pub modified: u64,
    pub text_hash: String,
}

/// `phoenix-check.json`: the verdicts of the last few builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckCache {
    #[serde(default)]
    pub builds: Vec<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe: Option<ExeStamp>,
}

impl CheckCache {
    /// How many builds are kept, newest first.
    pub const MAX_BUILDS: usize = 8;

    /// The cache at `path`; empty when there is none or it does not parse (a broken cache only
    /// costs one check, never a launch).
    pub fn load(path: &Path) -> CheckCache {
        fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Writes the cache in one rename, so a reader never sees half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, body)?;
        fs::rename(&tmp, path)
    }

    /// The verdict for this build with this signature folder, if one was recorded.
    pub fn lookup(&self, text_hash: &str, signatures_hash: &str) -> Option<&Verdict> {
        self.builds
            .iter()
            .find(|v| v.text_hash == text_hash && v.signatures_hash == signatures_hash)
    }

    /// Records a verdict. It replaces whatever was kept for the same build, including a "failed"
    /// mark made with an older signature folder: a hot-fix lifts the mark this way.
    pub fn record(&mut self, verdict: Verdict) {
        self.builds.retain(|v| v.text_hash != verdict.text_hash);
        self.builds.insert(0, verdict);
        self.builds.truncate(Self::MAX_BUILDS);
    }

    /// Marks this build with this signature folder failed (item 4 of 2.2.8).
    pub fn mark_failed(&mut self, text_hash: &str, signatures_hash: &str) {
        let mut v = self
            .lookup(text_hash, signatures_hash)
            .cloned()
            .unwrap_or_else(|| Verdict {
                text_hash: text_hash.to_string(),
                signatures_hash: signatures_hash.to_string(),
                mode: None,
                missing: Vec::new(),
                failed: false,
            });
        v.failed = true;
        self.record(v);
    }
}

/// Marks a build failed in the cache file at `cache`: the next `phoecord launch --channel phoenix`
/// uses the proxy until the signature folder's hash changes. The seam for the loader (see the
/// module docs); `text_hash` is the running build's, `signatures_hash` the one the launcher passed
/// in [`ENV_SIGNATURES_HASH`].
pub fn mark_failed(cache: &Path, text_hash: &str, signatures_hash: &str) -> io::Result<()> {
    let mut c = CheckCache::load(cache);
    c.mark_failed(text_hash, signatures_hash);
    c.save(cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(text: &str, sigs: &str, mode: Mode) -> Verdict {
        Verdict {
            text_hash: text.into(),
            signatures_hash: sigs.into(),
            mode: Some(mode),
            missing: Vec::new(),
            failed: false,
        }
    }

    #[test]
    fn modes_round_trip_by_name() {
        for m in [Mode::InProcess, Mode::Proxy] {
            assert_eq!(Mode::parse(m.name()), Some(m));
        }
        assert_eq!(Mode::parse("auto"), None);
    }

    #[test]
    fn the_loader_reads_what_the_launcher_set() {
        let vars = |mode: &'static str| {
            move |k: &str| match k {
                ENV_MODE => Some(mode.to_string()),
                ENV_CACHE => Some("/data/phoenix-check.json".to_string()),
                ENV_SIGNATURES_HASH => Some("ab".to_string()),
                _ => None,
            }
        };
        assert_eq!(
            Launched::from_vars(vars("in-process")),
            Some(Launched {
                mode: Mode::InProcess,
                cache: Some("/data/phoenix-check.json".into()),
                signatures_hash: Some("ab".into()),
            })
        );
        // Not started by `phoecord launch`, or a value it never writes: nothing.
        assert_eq!(Launched::from_vars(|_| None), None);
        assert_eq!(Launched::from_vars(vars("auto")), None);
    }

    #[test]
    fn a_verdict_is_found_only_under_both_hashes() {
        let mut c = CheckCache::default();
        c.record(verdict("t1", "s1", Mode::InProcess));
        assert_eq!(c.lookup("t1", "s1").unwrap().mode, Some(Mode::InProcess));
        assert!(c.lookup("t1", "s2").is_none());
        assert!(c.lookup("t2", "s1").is_none());
    }

    #[test]
    fn a_new_signature_folder_replaces_the_failed_mark() {
        let mut c = CheckCache::default();
        c.record(verdict("t1", "s1", Mode::InProcess));
        c.mark_failed("t1", "s1");
        let marked = c.lookup("t1", "s1").unwrap();
        assert!(marked.failed);
        assert_eq!(marked.mode, Some(Mode::InProcess));
        // The hot-fix's check is recorded for the same build: one entry, no mark.
        c.record(verdict("t1", "s2", Mode::InProcess));
        assert_eq!(c.builds.len(), 1);
        assert!(!c.lookup("t1", "s2").unwrap().failed);
    }

    #[test]
    fn a_mark_without_a_check_is_kept_too() {
        let mut c = CheckCache::default();
        c.mark_failed("t9", "s9");
        let v = c.lookup("t9", "s9").unwrap();
        assert!(v.failed && v.mode.is_none());
    }

    #[test]
    fn only_the_newest_builds_are_kept() {
        let mut c = CheckCache::default();
        for i in 0..CheckCache::MAX_BUILDS + 3 {
            c.record(verdict(&format!("t{i}"), "s", Mode::Proxy));
        }
        assert_eq!(c.builds.len(), CheckCache::MAX_BUILDS);
        assert_eq!(
            c.builds[0].text_hash,
            format!("t{}", CheckCache::MAX_BUILDS + 2)
        );
        assert!(c.lookup("t0", "s").is_none());
    }

    #[test]
    fn the_file_round_trips_and_a_broken_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("phoecord-choice-{}", std::process::id()));
        let path = dir.join("sub").join(CACHE_FILE);
        assert_eq!(CheckCache::load(&path), CheckCache::default());
        mark_failed(&path, "t1", "s1").unwrap();
        let mut c = CheckCache::load(&path);
        assert!(c.lookup("t1", "s1").unwrap().failed);
        c.exe = Some(ExeStamp {
            path: "/x/Discord.exe".into(),
            len: 5,
            modified: 7,
            text_hash: "t1".into(),
        });
        c.save(&path).unwrap();
        assert_eq!(CheckCache::load(&path), c);
        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(CheckCache::load(&path), CheckCache::default());
        fs::remove_dir_all(&dir).unwrap();
    }
}
