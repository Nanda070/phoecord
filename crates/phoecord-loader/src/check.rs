//! Offline signature check (task 2.2.6): run the signature table of a client exe against the exe
//! on disk, the way the loader would inside the process. Pure: `phoecord check` prints the result.
//!
//! Only the AOB signatures of `signatures/*.toml` are checked. The typed locators for the hook
//! targets (2.2.5, part 2) do not exist yet; when they do, they belong in the same report.
//!
//! [`transport_plan`] is the same check as the in-process transport sees it: the hook locators of
//! [`crate::found`] run on the exe on disk and give a [`Plan`]. `phoecord launch --channel
//! phoenix` picks the launch's transport from it (2.2.8).

use crate::found::{HookPlan, Scan};
use crate::pe::Pe;
use crate::plan::Plan;
use crate::signatures::{ResolveError, SignatureError, SignatureSet, resolve_all};
use crate::version::{self, ClientVersion, VersionError};
use std::{fmt, io, path::Path};

#[derive(Debug)]
pub enum CheckError {
    /// The signature folder cannot be listed.
    Signatures(io::Error),
    /// The file is not a PE executable with a `.text` section.
    Exe(VersionError),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Signatures(e) => write!(f, "cannot read the signature folder: {e}"),
            CheckError::Exe(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CheckError {}

/// One signature of the selected table: its name and the RVA it resolved to.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub result: Result<u32, ResolveError>,
}

#[derive(Debug)]
pub struct Report {
    pub version: ClientVersion,
    /// Key of the table picked for this build; `None` when no table matches it.
    pub table: Option<String>,
    /// In name order; empty without a table.
    pub entries: Vec<Entry>,
    /// Signature files that were skipped because they do not load.
    pub skipped: Vec<(String, SignatureError)>,
}

impl Report {
    pub fn found(&self) -> usize {
        self.entries.iter().filter(|e| e.result.is_ok()).count()
    }

    /// Everything resolved: a table was picked, it is not empty, every entry matched exactly once
    /// and no signature file was skipped (a skipped file may hold entries nobody checked).
    pub fn passed(&self) -> bool {
        self.table.is_some()
            && !self.entries.is_empty()
            && self.found() == self.entries.len()
            && self.skipped.is_empty()
    }
}

/// Checks the bytes of a client exe against every table in `signatures_dir`.
pub fn check(exe: &[u8], signatures_dir: &Path) -> Result<Report, CheckError> {
    let version = version::detect(exe).map_err(CheckError::Exe)?;
    // `detect` has already read `.text`, so neither step can fail here; no panic either way.
    let text = Pe::parse(exe)
        .map_err(|e| CheckError::Exe(e.into()))?
        .section_bytes(".text")
        .ok_or(CheckError::Exe(VersionError::NoTextSection))?;
    let (set, skipped) = SignatureSet::load_dir(signatures_dir).map_err(CheckError::Signatures)?;
    let (table, entries) = match set.select(&version) {
        Some((key, sigs)) => {
            let entries = resolve_all(sigs, text, version.text_rva)
                .into_iter()
                .map(|(name, result)| Entry {
                    name: name.to_string(),
                    result,
                })
                .collect();
            (Some(key.to_string()), entries)
        }
        None => (None, Vec::new()),
    };
    Ok(Report {
        version,
        table,
        entries,
        skipped,
    })
}

/// The degradation plan the loader would make for this exe: the build's signature table (the same
/// selection as [`check`]), the hook locators run over its `.text`, then [`Plan::from_hooks`]. A
/// build without a table is [`Plan::unknown_build`]. Signature files that do not load are skipped,
/// as the loader skips them; what they held is then simply not found.
pub fn transport_plan(
    exe: &[u8],
    signatures_dir: &Path,
) -> Result<(ClientVersion, Plan), CheckError> {
    let version = version::detect(exe).map_err(CheckError::Exe)?;
    let text = Pe::parse(exe)
        .map_err(|e| CheckError::Exe(e.into()))?
        .section_bytes(".text")
        .ok_or(CheckError::Exe(VersionError::NoTextSection))?;
    let image_len = Pe::image_size(exe)
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    let (set, _skipped) = SignatureSet::load_dir(signatures_dir).map_err(CheckError::Signatures)?;
    let plan = match set.select(&version) {
        Some((_, sigs)) => Plan::from_hooks(&HookPlan::locate(&Scan {
            sigs,
            text,
            text_rva: version.text_rva,
            image_len,
        })),
        None => Plan::unknown_build(),
    };
    Ok((version, plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::testutil::build_pe;
    use std::path::PathBuf;

    /// A fresh folder with the given signature files; removed on drop.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str, files: &[(&str, &str)]) -> Dir {
            let dir =
                std::env::temp_dir().join(format!("phoecord-check-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for (file, text) in files {
                std::fs::write(dir.join(file), text).unwrap();
            }
            Dir(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Made-up code: `55 8B EC` once, `90 90` twice.
    fn exe() -> Vec<u8> {
        build_pe(
            &[0x90, 0x90, 0x55, 0x8B, 0xEC, 0x90, 0x90, 0xC3],
            &[],
            Some((1, 0, 0, 1)),
        )
    }

    #[test]
    fn every_signature_found_passes() {
        let dir = Dir::new(
            "pass",
            &[("a.toml", "[build.\"1.0.0.1\"]\nf = { aob = \"55 8B EC\" }")],
        );
        let r = check(&exe(), &dir.0).unwrap();
        assert_eq!(r.table.as_deref(), Some("1.0.0.1"));
        assert_eq!(
            r.entries,
            [Entry {
                name: "f".into(),
                result: Ok(0x202)
            }]
        );
        assert!(r.passed());
    }

    #[test]
    fn a_miss_and_an_ambiguous_match_fail() {
        let dir = Dir::new(
            "miss",
            &[(
                "a.toml",
                "[build.\"1.0.0.1\"]\n\
                 a = { aob = \"55 8B EC\" }\n\
                 b = { aob = \"90 90\" }\n\
                 c = { aob = \"11 22\" }",
            )],
        );
        let r = check(&exe(), &dir.0).unwrap();
        let results: Vec<_> = r.entries.iter().map(|e| &e.result).collect();
        assert_eq!(
            results,
            [
                &Ok(0x202),
                &Err(ResolveError::Ambiguous(2)),
                &Err(ResolveError::NotFound)
            ]
        );
        assert_eq!(r.found(), 1);
        assert!(!r.passed());
    }

    #[test]
    fn unknown_build_and_empty_table_fail() {
        let other = Dir::new(
            "other",
            &[("a.toml", "[build.\"9.9.9.9\"]\nf = { aob = \"55 8B EC\" }")],
        );
        let r = check(&exe(), &other.0).unwrap();
        assert!(r.table.is_none() && r.entries.is_empty() && !r.passed());
        let empty = Dir::new("empty", &[("a.toml", "[build.\"1.0.0.1\"]")]);
        let r = check(&exe(), &empty.0).unwrap();
        assert!(r.table.is_some() && !r.passed());
    }

    #[test]
    fn a_skipped_file_fails_even_when_the_rest_resolves() {
        let dir = Dir::new(
            "skipped",
            &[
                ("a.toml", "[build.\"1.0.0.1\"]\nf = { aob = \"55 8B EC\" }"),
                ("b.toml", "garbage ["),
            ],
        );
        let r = check(&exe(), &dir.0).unwrap();
        assert_eq!(r.found(), 1);
        assert_eq!(r.skipped[0].0, "b.toml");
        assert!(!r.passed());
    }

    #[test]
    fn bad_exe_and_missing_folder_are_errors() {
        let dir = Dir::new("bad", &[]);
        assert!(matches!(check(b"nope", &dir.0), Err(CheckError::Exe(_))));
        assert!(matches!(
            check(&exe(), &dir.0.join("absent")),
            Err(CheckError::Signatures(_))
        ));
    }

    /// [`exe`] with `SizeOfImage` set, as a real image has it (the locators check results against it).
    fn exe_with_image_size() -> Vec<u8> {
        let mut bytes = exe();
        let at = 0x80 + 24 + 56;
        bytes[at..at + 4].copy_from_slice(&0x10_000u32.to_le_bytes());
        bytes
    }

    #[test]
    fn the_transport_plan_runs_the_hook_locators() {
        use crate::plan::{EntryPoint, Reason, Route};
        let dir = Dir::new(
            "plan",
            &[(
                "a.toml",
                "[build.\"1.0.0.1\"]\n\
                 http_send = { aob = \"55 8B EC\" }\n\
                 http_poll = { aob = \"8B EC 90\" }",
            )],
        );
        let (version, plan) = transport_plan(&exe_with_image_size(), &dir.0).unwrap();
        assert_eq!(version.file_version.as_deref(), Some("1.0.0.1"));
        assert_eq!(plan.route(EntryPoint::Request), &Route::InProcess);
        assert_eq!(plan.route(EntryPoint::Response), &Route::InProcess);
        assert!(matches!(
            plan.route(EntryPoint::Dispatch),
            Route::Proxy(Reason::Locator(why)) if why.contains("gateway_registry")
        ));
        assert!(!plan.all_in_process());
    }

    #[test]
    fn a_build_without_a_table_is_an_unknown_build() {
        let dir = Dir::new(
            "plan-unknown",
            &[("a.toml", "[build.\"9.9.9.9\"]\nf = { aob = \"55 8B EC\" }")],
        );
        let (_, plan) = transport_plan(&exe_with_image_size(), &dir.0).unwrap();
        assert_eq!(plan, Plan::unknown_build());
        assert!(matches!(
            transport_plan(b"nope", &dir.0),
            Err(CheckError::Exe(_))
        ));
    }
}
