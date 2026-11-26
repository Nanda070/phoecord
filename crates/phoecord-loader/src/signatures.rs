//! `signatures/*.toml`: named AOB signatures per client build, and their resolution against the
//! bytes of the client's `.text` section. Format: `signatures/README.md`.
//!
//! ```toml
//! [build."0.0.123"]
//! theme_palette = { aob = "48 8D 0D ?? ?? ?? ?? E8", rip = 3 }
//! ```
//!
//! A signature resolves only if it matches exactly once; with `rip = N` the match is a
//! `rel32` operand at byte `N` of the match (instruction end = `N + 4`) and the result is the address
//! it points to, otherwise the result is the start of the match. Results are RVAs.

use crate::aob::Pattern;
use crate::version::ClientVersion;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
};

/// Where the loader reads signatures: `%LOCALAPPDATA%\PhoeCord\signatures`, given the value of
/// `LOCALAPPDATA`. The installer writes hot-fixes to the same place (2.2.10).
pub fn default_dir(local_app_data: Option<OsString>) -> Option<PathBuf> {
    Some(
        PathBuf::from(local_app_data?)
            .join("PhoeCord")
            .join("signatures"),
    )
}

#[derive(Debug)]
pub enum SignatureError {
    Toml(String),
    Aob {
        build: String,
        name: String,
        error: String,
    },
    Io(io::Error),
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureError::Toml(e) => write!(f, "invalid signature file: {e}"),
            SignatureError::Aob { build, name, error } => {
                write!(f, "build {build}, signature {name}: {error}")
            }
            SignatureError::Io(e) => write!(f, "cannot read signature file: {e}"),
        }
    }
}

impl std::error::Error for SignatureError {}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    NotFound,
    Ambiguous(usize),
    /// The `rip` operand lies outside the scanned bytes or the target is outside the module.
    BadRip,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::NotFound => f.write_str("no match"),
            ResolveError::Ambiguous(n) => write!(f, "ambiguous: {n}+ matches"),
            ResolveError::BadRip => f.write_str("rip operand out of range"),
        }
    }
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    build: BTreeMap<String, BTreeMap<String, RawEntry>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    aob: String,
    rip: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Signature {
    pub name: String,
    pub pattern: Pattern,
    pub rip: Option<usize>,
}

impl Signature {
    /// Resolves against `.text` bytes; `text_rva` is where `.text` starts in the module.
    pub fn resolve(&self, text: &[u8], text_rva: u32) -> Result<u32, ResolveError> {
        let mut it = self.pattern.find_all(text);
        let at = it.next().ok_or(ResolveError::NotFound)?;
        if it.next().is_some() {
            return Err(ResolveError::Ambiguous(2 + it.count()));
        }
        let rva = i64::from(text_rva) + at as i64;
        let target = match self.rip {
            None => rva,
            Some(n) => {
                let op = at.checked_add(n).ok_or(ResolveError::BadRip)?;
                let disp = text
                    .get(op..op + 4)
                    .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .ok_or(ResolveError::BadRip)?;
                rva + n as i64 + 4 + i64::from(disp)
            }
        };
        u32::try_from(target).map_err(|_| ResolveError::BadRip)
    }
}

/// All signatures from one or more files, keyed by build key.
#[derive(Debug, Default)]
pub struct SignatureSet {
    builds: BTreeMap<String, Vec<Signature>>,
}

impl SignatureSet {
    pub fn parse(toml_text: &str) -> Result<SignatureSet, SignatureError> {
        let raw: RawFile =
            toml::from_str(toml_text).map_err(|e| SignatureError::Toml(e.to_string()))?;
        let mut set = SignatureSet::default();
        for (build, entries) in raw.build {
            let mut sigs = Vec::with_capacity(entries.len());
            for (name, e) in entries {
                let pattern = Pattern::parse(&e.aob).map_err(|err| SignatureError::Aob {
                    build: build.clone(),
                    name: name.clone(),
                    error: err.to_string(),
                })?;
                sigs.push(Signature {
                    name,
                    pattern,
                    rip: e.rip,
                });
            }
            set.builds.insert(build, sigs);
        }
        Ok(set)
    }

    /// Loads every `*.toml` in `dir` (sorted by name; a later file replaces a build of the same
    /// key). Files that fail to parse are skipped and reported in the second element.
    pub fn load_dir(dir: &Path) -> io::Result<(SignatureSet, Vec<(String, SignatureError)>)> {
        let mut paths: Vec<_> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        paths.sort();
        let mut all = SignatureSet::default();
        let mut errors = Vec::new();
        for p in paths {
            let label = p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into());
            match fs::read_to_string(&p)
                .map_err(SignatureError::Io)
                .and_then(|t| SignatureSet::parse(&t))
            {
                Ok(s) => all.builds.extend(s.builds),
                Err(e) => errors.push((label, e)),
            }
        }
        Ok((all, errors))
    }

    pub fn is_empty(&self) -> bool {
        self.builds.is_empty()
    }

    /// Every build key with the number of signatures in its table.
    pub fn builds(&self) -> impl Iterator<Item = (&str, usize)> {
        self.builds.iter().map(|(k, s)| (k.as_str(), s.len()))
    }

    /// Picks the table for a client: first `"<version>+<hash prefix>"` (prefix of at least 8 hex
    /// digits of the `.text` hash), then plain `"<version>"`. Returns the matching key too.
    pub fn select(&self, v: &ClientVersion) -> Option<(&str, &[Signature])> {
        let version = v.file_version.as_deref()?;
        let by_hash = self.builds.iter().find(|(k, _)| {
            k.split_once('+').is_some_and(|(ver, h)| {
                ver == version && h.len() >= 8 && v.text_hash.starts_with(&h.to_ascii_lowercase())
            })
        });
        by_hash
            .or_else(|| self.builds.get_key_value(version))
            .map(|(k, s)| (k.as_str(), s.as_slice()))
    }
}

/// Resolves every signature; each result is independent, so one miss disables only its feature.
pub fn resolve_all<'a>(
    sigs: &'a [Signature],
    text: &[u8],
    text_rva: u32,
) -> Vec<(&'a str, Result<u32, ResolveError>)> {
    sigs.iter()
        .map(|s| (s.name.as_str(), s.resolve(text, text_rva)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ver(v: &str, hash: &str) -> ClientVersion {
        ClientVersion {
            file_version: Some(v.into()),
            text_hash: hash.into(),
            text_rva: 0x1000,
            text_len: 0,
        }
    }

    const TOML: &str = r#"
        [build."1.0.0.1"]
        plain = { aob = "55 8B EC" }
        via_rip = { aob = "48 8D 0D ?? ?? ?? ?? E8", rip = 3 }

        [build."1.0.0.2"]
        plain = { aob = "55 8B EC" }

        [build."1.0.0.2+deadbeef"]
        plain = { aob = "CC CC CC" }
    "#;

    #[test]
    fn parses_and_selects() {
        let set = SignatureSet::parse(TOML).unwrap();
        let (k, s) = set.select(&ver("1.0.0.1", "00")).unwrap();
        assert_eq!((k, s.len()), ("1.0.0.1", 2));
        // The hash-qualified key wins over the plain version key.
        let (k, _) = set.select(&ver("1.0.0.2", "deadbeef0000")).unwrap();
        assert_eq!(k, "1.0.0.2+deadbeef");
        let (k, _) = set.select(&ver("1.0.0.2", "0123abcd")).unwrap();
        assert_eq!(k, "1.0.0.2");
        assert!(set.select(&ver("9.9.9.9", "00")).is_none());
        assert!(
            set.select(&ClientVersion {
                file_version: None,
                text_hash: "00".into(),
                text_rva: 0,
                text_len: 0
            })
            .is_none()
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(SignatureSet::parse("[build.\"1\"]\nx = { aob = \"ZZ\" }").is_err());
        assert!(SignatureSet::parse("[build.\"1\"]\nx = { aob = \"?? ??\" }").is_err());
        assert!(SignatureSet::parse("[build.\"1\"]\nx = { aob = \"55\", typo = 1 }").is_err());
        assert!(SignatureSet::parse("not toml [").is_err());
    }

    #[test]
    fn plain_and_rip_resolution() {
        let set = SignatureSet::parse(TOML).unwrap();
        let (_, sigs) = set.select(&ver("1.0.0.1", "00")).unwrap();
        // text: pad, `lea rcx,[rip+0x10]; call`, pad ... target = 0x10 past the lea's end.
        let mut text = vec![0u8; 0x40];
        text[4..7].copy_from_slice(&[0x48, 0x8D, 0x0D]);
        text[7..11].copy_from_slice(&0x10i32.to_le_bytes());
        text[11] = 0xE8;
        text[0x20..0x23].copy_from_slice(&[0x55, 0x8B, 0xEC]);
        let r = resolve_all(sigs, &text, 0x1000);
        let get = |n: &str| r.iter().find(|(k, _)| *k == n).unwrap().1.as_ref().copied();
        assert_eq!(get("plain"), Ok(0x1020));
        // match at 4, operand at 7, instruction end at 11, target = 11 + 0x10.
        assert_eq!(get("via_rip"), Ok(0x1000 + 11 + 0x10));
    }

    #[test]
    fn negative_rip_displacement() {
        let sig = Signature {
            name: "x".into(),
            pattern: Pattern::parse("E8 ?? ?? ?? ??").unwrap(),
            rip: Some(1),
        };
        let mut text = vec![0u8; 0x20];
        text[0x10] = 0xE8;
        text[0x11..0x15].copy_from_slice(&(-0x10i32).to_le_bytes());
        // end = 0x15, target = 0x05.
        assert_eq!(sig.resolve(&text, 0x2000), Ok(0x2005));
    }

    #[test]
    fn misses_are_distinguished() {
        let p = |s| Signature {
            name: "x".into(),
            pattern: Pattern::parse(s).unwrap(),
            rip: None,
        };
        let text = [1u8, 2, 3, 1, 2, 3];
        assert_eq!(p("09 09").resolve(&text, 0), Err(ResolveError::NotFound));
        assert_eq!(
            p("01 02").resolve(&text, 0),
            Err(ResolveError::Ambiguous(2))
        );
        let near_end = Signature {
            rip: Some(4),
            ..p("03")
        };
        assert_eq!(near_end.resolve(&[0, 3], 0), Err(ResolveError::BadRip));
        let before_zero = Signature {
            rip: Some(1),
            ..p("E8 ?? ?? ?? ??")
        };
        let t = [0xE8, 0, 0, 0, 0x80]; // disp = i32::MIN
        assert_eq!(before_zero.resolve(&t, 0), Err(ResolveError::BadRip));
    }

    /// The same logical function, located by one signature in two adjacent builds where its
    /// absolute position and the surrounding addresses differ.
    #[test]
    fn same_signature_finds_function_in_two_builds() {
        let set = SignatureSet::parse(
            "[build.\"1\"]\nf = { aob = \"48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B D8\", rip = 3 }\n\
             [build.\"2\"]\nf = { aob = \"48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B D8\", rip = 3 }",
        )
        .unwrap();
        let func = [0x55u8, 0x48, 0x89, 0xE5, 0x5D, 0xC3];
        let build = |pad: usize, call_disp: i32| {
            let mut t = vec![0xCCu8; pad];
            // lea rcx,[rip+0x30]; call rel32; mov rbx,rax. The lea ends 0x30 bytes before the
            // function.
            t.extend([0x48, 0x8D, 0x0D]);
            t.extend(0x30i32.to_le_bytes());
            let target = t.len() + 0x30;
            t.push(0xE8);
            t.extend(call_disp.to_le_bytes());
            t.extend([0x48, 0x8B, 0xD8]);
            t.resize(target, 0xCC);
            t.extend(func);
            (t, target as u32)
        };
        for (key, pad, disp) in [("1", 0x20usize, 0x1234), ("2", 0x91, -0x777)] {
            let (text, expected) = build(pad, disp);
            let (_, sigs) = set
                .select(&ver(key, "00"))
                .map(|(k, s)| (k.to_owned(), s))
                .unwrap();
            let rva = sigs[0].resolve(&text, 0x1000).unwrap();
            assert_eq!(rva, 0x1000 + expected);
            assert_eq!(&text[expected as usize..][..func.len()], &func);
        }
    }

    #[test]
    fn load_dir_skips_bad_files() {
        let dir = std::env::temp_dir().join(format!("phoecord-sig-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), TOML).unwrap();
        std::fs::write(dir.join("b.toml"), "garbage [").unwrap();
        std::fs::write(dir.join("c.txt"), "ignored").unwrap();
        let (set, errors) = SignatureSet::load_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!set.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "b.toml");
    }
}
