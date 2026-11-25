//! Plugin packages on disk: `data/plugins/<id>/` with `plugin.toml` and `plugin.wasm`, the folder
//! the proxy watches (it loads a new package and drops a removed one without a restart).
//!
//! Installing is two steps so the user can read what a plugin asks for before anything lands in
//! `plugins/`: [`Store::stage_path`] / [`Store::stage_zip`] unpack into `plugins-staging/` and check
//! the package, [`Store::commit`] moves it into place and switches it on.

use phoecord_plugin_runtime::package::PackageManifest;
use phoecord_proxy::config::Config;
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The largest package accepted (unpacked), and the largest download.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// A package's `i18n/<lang>.ftl` files: at most this many, each at most this big.
const MAX_FTL_FILES: usize = 16;
const MAX_FTL_BYTES: u64 = phoecord_plugin_runtime::package::MAX_FTL_BYTES;

/// `i18n/<lang>.ftl` with `<lang>` like `ru` or `pt-BR`, nothing deeper.
fn is_ftl(rel: &str) -> bool {
    let Some(lang) = rel
        .strip_prefix("i18n/")
        .and_then(|r| r.strip_suffix(".ftl"))
    else {
        return false;
    };
    let b = lang.as_bytes();
    let lower = |x: &[u8]| x.iter().all(u8::is_ascii_lowercase);
    match b.len() {
        2 => lower(b),
        5 => lower(&b[..2]) && b[2] == b'-' && b[3..].iter().all(u8::is_ascii_uppercase),
        _ => false,
    }
}

/// Files copied out of a package; anything else in it is ignored.
const KEEP: &[&str] = &[
    "plugin.toml",
    "plugin.wasm",
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
];

/// Why a package could not be added; each maps to a message that says what to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Neither a `.zip` nor a folder with `plugin.toml` and `plugin.wasm`.
    NotAPackage,
    /// The package has `plugin.toml` but no `plugin.wasm`.
    NoWasm,
    TooBig,
    /// A path in the archive points outside it.
    UnsafePath(String),
    /// `plugin.toml` is wrong (the parser's own text).
    Manifest(String),
    /// The plugin code does not load (the runtime's own text).
    Broken(String),
    NotHttps,
    Download(String),
    Io(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// What the plugin asks for, as written in `plugin.toml` (for the confirm dialog).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Asks {
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub rights: Vec<String>,
    #[serde(default)]
    pub fetch: Vec<String>,
}

/// A checked package waiting in `plugins-staging/` for the user's yes.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// `plugins-staging/<n>/<id>`.
    pub dir: PathBuf,
    pub manifest: PackageManifest,
    pub asks: Asks,
    /// The version of the installed package this one replaces.
    pub replaces: Option<String>,
}

impl Candidate {
    pub fn id(&self) -> &str {
        &self.manifest.manifest.id
    }
}

/// An installed package; `manifest` is `None` when its `plugin.toml` no longer parses.
#[derive(Clone, Debug)]
pub struct Installed {
    pub id: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub dir: PathBuf,
    pub manifest: Option<PackageManifest>,
    pub problem: Option<String>,
}

/// Results of plugin jobs, for the window.
#[derive(Debug)]
pub enum Msg {
    Staged(Result<Box<Candidate>, Error>),
    Installed(Result<String, Error>),
    Removed(Result<String, Error>),
}

/// Checks that the plugin code really loads; the window passes the WASM runtime's check, tests a
/// stand-in.
pub type Verify = dyn Fn(&Path) -> Result<(), String> + Send + Sync;

#[derive(Clone, Debug)]
pub struct Store {
    /// `data/plugins`.
    pub root: PathBuf,
    /// `data/plugins-staging`.
    pub staging: PathBuf,
    pub config: PathBuf,
}

impl Store {
    pub fn new(data: &Path, config: &Path) -> Store {
        Store {
            root: data.join("plugins"),
            staging: data.join("plugins-staging"),
            config: config.to_path_buf(),
        }
    }

    /// Installed packages, by id.
    pub fn list(&self) -> Vec<Installed> {
        let mut out: Vec<Installed> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("plugin.toml").is_file())
            .filter_map(|dir| {
                let id = dir.file_name()?.to_str()?.to_string();
                let text = std::fs::read_to_string(dir.join("plugin.toml")).unwrap_or_default();
                let (manifest, problem) = match PackageManifest::parse(&text) {
                    Ok(m) => (Some(m), None),
                    Err(e) => (None, Some(e)),
                };
                Some(Installed {
                    id,
                    dir,
                    manifest,
                    problem,
                })
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    fn fresh_staging(&self) -> Result<PathBuf, Error> {
        std::fs::create_dir_all(&self.staging)?;
        for n in 0u32.. {
            let d = self.staging.join(format!("{}-{n}", std::process::id()));
            if std::fs::create_dir(&d).is_ok() {
                return Ok(d);
            }
        }
        unreachable!()
    }

    /// Stage a `.zip`, a package folder, or a `plugin.toml` / `plugin.wasm` inside one.
    pub fn stage_path(&self, src: &Path, verify: &Verify) -> Result<Candidate, Error> {
        if src.is_dir() {
            return self.stage_dir(src, verify);
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name == "plugin.toml" || name == "plugin.wasm" {
            return self.stage_dir(src.parent().ok_or(Error::NotAPackage)?, verify);
        }
        if name.ends_with(".zip") {
            if std::fs::metadata(src)?.len() > MAX_BYTES {
                return Err(Error::TooBig);
            }
            return self.stage_zip(&std::fs::read(src)?, verify);
        }
        Err(Error::NotAPackage)
    }

    fn stage_dir(&self, dir: &Path, verify: &Verify) -> Result<Candidate, Error> {
        if !dir.join("plugin.toml").is_file() {
            return Err(Error::NotAPackage);
        }
        if !dir.join("plugin.wasm").is_file() {
            return Err(Error::NoWasm);
        }
        let mut total = 0;
        for f in KEEP {
            if let Ok(m) = std::fs::metadata(dir.join(f)) {
                total += m.len();
            }
        }
        // `i18n/<lang>.ftl`: a file name that does not fit the pattern is ignored.
        let mut ftl: Vec<(String, PathBuf)> = std::fs::read_dir(dir.join("i18n"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = format!("i18n/{}", e.file_name().to_str()?);
                (is_ftl(&name) && e.path().is_file()).then(|| (name, e.path()))
            })
            .collect();
        ftl.sort();
        if ftl.len() > MAX_FTL_FILES {
            return Err(Error::TooBig);
        }
        for (_, path) in &ftl {
            let len = std::fs::metadata(path)?.len();
            if len > MAX_FTL_BYTES {
                return Err(Error::TooBig);
            }
            total += len;
        }
        if total > MAX_BYTES {
            return Err(Error::TooBig);
        }
        let work = self.fresh_staging()?;
        let unpacked = work.join("package");
        std::fs::create_dir(&unpacked)?;
        for f in KEEP {
            let from = dir.join(f);
            if from.is_file() {
                std::fs::copy(&from, unpacked.join(f))?;
            }
        }
        if !ftl.is_empty() {
            std::fs::create_dir(unpacked.join("i18n"))?;
            for (name, from) in &ftl {
                std::fs::copy(from, unpacked.join(name))?;
            }
        }
        self.finish(work, verify)
    }

    /// Stage a zipped package: `plugin.toml` at the top of the archive or in one folder.
    pub fn stage_zip(&self, bytes: &[u8], verify: &Verify) -> Result<Candidate, Error> {
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| Error::NotAPackage)?;
        // Where plugin.toml is decides the package's top folder.
        let mut prefix: Option<PathBuf> = None;
        for i in 0..zip.len() {
            let f = zip.by_index(i).map_err(|_| Error::NotAPackage)?;
            let Some(p) = f.enclosed_name() else {
                return Err(Error::UnsafePath(f.name().to_string()));
            };
            if p.file_name().is_some_and(|n| n == "plugin.toml")
                && p.components().count() <= 2
                && !p.starts_with("__MACOSX")
            {
                let top = p.parent().map(Path::to_path_buf).unwrap_or_default();
                if prefix
                    .as_ref()
                    .is_none_or(|old| top.as_os_str().len() < old.as_os_str().len())
                {
                    prefix = Some(top);
                }
            }
        }
        let prefix = prefix.ok_or(Error::NotAPackage)?;
        let work = self.fresh_staging()?;
        let unpacked = work.join("package");
        std::fs::create_dir(&unpacked)?;
        let mut total = 0u64;
        let mut ftl_files = 0usize;
        let mut unpack = |work: &Path| -> Result<(), Error> {
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(|_| Error::NotAPackage)?;
                let Some(p) = f.enclosed_name() else {
                    return Err(Error::UnsafePath(f.name().to_string()));
                };
                let Ok(rel) = p.strip_prefix(&prefix) else {
                    continue;
                };
                // Compare with `/` between components on every OS.
                let name = rel
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap_or("\u{0}"))
                    .collect::<Vec<_>>()
                    .join("/");
                let ftl = is_ftl(&name);
                if !(KEEP.contains(&name.as_str()) || ftl) || f.is_dir() {
                    continue;
                }
                let cap = if ftl {
                    ftl_files += 1;
                    if ftl_files > MAX_FTL_FILES {
                        return Err(Error::TooBig);
                    }
                    MAX_FTL_BYTES
                } else {
                    MAX_BYTES
                };
                let mut buf = Vec::new();
                (&mut f).take(cap + 1).read_to_end(&mut buf)?;
                total += buf.len() as u64;
                if buf.len() as u64 > cap || total > MAX_BYTES {
                    return Err(Error::TooBig);
                }
                let to = work.join("package").join(&name);
                if ftl {
                    std::fs::create_dir_all(to.parent().unwrap())?;
                }
                std::fs::write(to, buf)?;
            }
            Ok(())
        };
        if let Err(e) = unpack(&work) {
            let _ = std::fs::remove_dir_all(&work);
            self.tidy();
            return Err(e);
        }
        if !unpacked.join("plugin.wasm").is_file() {
            let _ = std::fs::remove_dir_all(&work);
            self.tidy();
            return Err(Error::NoWasm);
        }
        self.finish(work, verify)
    }

    /// Parse and check the unpacked package in `work/package`, rename it to `work/<id>`.
    fn finish(&self, work: PathBuf, verify: &Verify) -> Result<Candidate, Error> {
        let r = (|| {
            let unpacked = work.join("package");
            let text = std::fs::read_to_string(unpacked.join("plugin.toml"))
                .map_err(|_| Error::Manifest("plugin.toml is not UTF-8 text".into()))?;
            let manifest = PackageManifest::parse(&text).map_err(Error::Manifest)?;
            let asks: Asks = toml::from_str(&text).unwrap_or_default();
            let dir = work.join(&manifest.manifest.id);
            std::fs::rename(&unpacked, &dir)?;
            verify(&dir).map_err(Error::Broken)?;
            let replaces = self
                .list()
                .into_iter()
                .find(|p| p.id == manifest.manifest.id)
                .map(|p| {
                    p.manifest
                        .map(|m| m.manifest.version)
                        .unwrap_or_else(|| "?".into())
                });
            Ok(Candidate {
                dir,
                manifest,
                asks,
                replaces,
            })
        })();
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&work);
            self.tidy();
        }
        r
    }

    /// Throw a staged package away (the user said no).
    pub fn discard(&self, c: &Candidate) {
        if let Some(work) = c.dir.parent() {
            let _ = std::fs::remove_dir_all(work);
        }
        self.tidy();
    }

    /// Remove `plugins-staging/` once nothing is waiting in it.
    fn tidy(&self) {
        let _ = std::fs::remove_dir(&self.staging);
    }

    /// Move a staged package into `plugins/<id>` (replacing an older one) and switch it on.
    pub fn commit(&self, c: &Candidate) -> Result<String, Error> {
        let id = c.id().to_string();
        std::fs::create_dir_all(&self.root)?;
        let target = self.root.join(&id);
        let work = c.dir.parent().ok_or(Error::NotAPackage)?.to_path_buf();
        if target.exists() {
            // Move the old one aside first so the folder is never half old, half new.
            let old = work.join("previous");
            std::fs::rename(&target, &old)?;
            if let Err(e) = std::fs::rename(&c.dir, &target) {
                let _ = std::fs::rename(&old, &target);
                return Err(e.into());
            }
        } else {
            std::fs::rename(&c.dir, &target)?;
        }
        let _ = std::fs::remove_dir_all(&work);
        self.tidy();
        self.set_enabled(&id, true)?;
        Ok(id)
    }

    /// Delete `plugins/<id>` (the package and the data it kept there) and its config entry.
    pub fn remove(&self, id: &str) -> Result<String, Error> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::Io(format!("invalid plugin id {id:?}")));
        }
        let dir = self.root.join(id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        if let Ok(mut cfg) = Config::load(&self.config)
            && cfg.plugins.remove(id).is_some()
        {
            cfg.save(&self.config)
                .map_err(|e| Error::Io(format!("{e:#}")))?;
        }
        Ok(id.to_string())
    }

    pub fn set_enabled(&self, id: &str, on: bool) -> Result<(), Error> {
        let io = |e: anyhow::Error| Error::Io(format!("{e:#}"));
        let mut cfg = Config::load_or_init(&self.config).map_err(io)?;
        cfg.set_enabled(id, on).map_err(io)?;
        cfg.save(&self.config).map_err(io)
    }
}

/// Is `url` an `https://` link with a host?
pub fn is_https(url: &str) -> bool {
    let u = url.trim();
    u.len() > "https://".len()
        && u[..8].eq_ignore_ascii_case("https://")
        && !u[8..].starts_with('/')
}

/// Download a zipped package over HTTPS only (redirects too), at most [`MAX_BYTES`].
pub fn download(url: &str) -> Result<Vec<u8>, Error> {
    if !is_https(url) {
        return Err(Error::NotHttps);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(5)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .get(url.trim())
        .call()
        .map_err(|e| Error::Download(e.to_string()))?;
    resp.body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => Error::TooBig,
            e => Error::Download(e.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TOML: &str = r#"
id = "hello_wasm"
name = "Hello"
version = "1.2.0"
description = "Says hello"
authors = ["Aigerim"]
api = 1
events = ["dispatch:MESSAGE_CREATE"]
rights = ["notify", "fetch"]
fetch = ["api.example.com"]
"#;

    fn ok(_: &Path) -> Result<(), String> {
        Ok(())
    }

    fn store(t: &Path) -> Store {
        Store::new(&t.join("data"), &t.join("config.toml"))
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_in_a_folder_stages_commits_and_switches_on() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let z = zip_of(&[
            ("hello/plugin.toml", TOML.as_bytes()),
            ("hello/plugin.wasm", b"\0asm"),
            ("hello/notes.txt", b"ignored"),
            ("__MACOSX/hello/._plugin.toml", b"junk"),
        ]);
        let c = s.stage_zip(&z, &ok).unwrap();
        assert_eq!(c.id(), "hello_wasm");
        assert_eq!(c.asks.rights, vec!["notify", "fetch"]);
        assert_eq!(c.asks.fetch, vec!["api.example.com"]);
        assert!(c.replaces.is_none());
        // Nothing in plugins/ before the yes.
        assert!(s.list().is_empty());
        s.commit(&c).unwrap();
        let list = s.list();
        assert_eq!(list.len(), 1);
        assert!(list[0].dir.join("plugin.wasm").is_file());
        assert!(!list[0].dir.join("notes.txt").exists());
        assert!(Config::load(&s.config).unwrap().is_enabled("hello_wasm"));
        // The staging folder is cleaned up.
        assert!(!s.staging.exists());
    }

    #[test]
    fn second_install_reports_and_replaces_the_old_version() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let z = zip_of(&[("plugin.toml", TOML.as_bytes()), ("plugin.wasm", b"v1")]);
        s.commit(&s.stage_zip(&z, &ok).unwrap()).unwrap();
        let newer = TOML.replace("1.2.0", "1.3.0");
        let z = zip_of(&[("plugin.toml", newer.as_bytes()), ("plugin.wasm", b"v2")]);
        let c = s.stage_zip(&z, &ok).unwrap();
        assert_eq!(c.replaces.as_deref(), Some("1.2.0"));
        s.commit(&c).unwrap();
        let wasm = std::fs::read(s.root.join("hello_wasm/plugin.wasm")).unwrap();
        assert_eq!(wasm, b"v2");
    }

    #[test]
    fn folder_and_toml_file_both_work() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let src = t.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("plugin.toml"), TOML).unwrap();
        std::fs::write(src.join("plugin.wasm"), b"\0asm").unwrap();
        let a = s.stage_path(&src, &ok).unwrap();
        s.discard(&a);
        let b = s.stage_path(&src.join("plugin.toml"), &ok).unwrap();
        assert_eq!(b.id(), "hello_wasm");
        // The source folder is untouched.
        assert!(src.join("plugin.wasm").is_file());
    }

    #[test]
    fn bad_packages_are_refused_and_leave_nothing_behind() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        assert_eq!(
            s.stage_zip(b"not a zip", &ok).unwrap_err(),
            Error::NotAPackage
        );
        let z = zip_of(&[("plugin.toml", TOML.as_bytes())]);
        assert_eq!(s.stage_zip(&z, &ok).unwrap_err(), Error::NoWasm);
        let z = zip_of(&[("plugin.toml", b"id = 3"), ("plugin.wasm", b"x")]);
        assert!(matches!(
            s.stage_zip(&z, &ok).unwrap_err(),
            Error::Manifest(_)
        ));
        let z = zip_of(&[("plugin.toml", TOML.as_bytes()), ("plugin.wasm", b"x")]);
        let broken = |_: &Path| Err("not a component".to_string());
        assert_eq!(
            s.stage_zip(&z, &broken).unwrap_err(),
            Error::Broken("not a component".into())
        );
        assert_eq!(
            s.stage_path(&t.path().join("x.exe"), &ok).unwrap_err(),
            Error::NotAPackage
        );
        assert!(s.list().is_empty());
        assert!(!s.staging.exists());
    }

    #[test]
    fn path_traversal_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let z = zip_of(&[
            ("plugin.toml", TOML.as_bytes()),
            ("plugin.wasm", b"x"),
            ("../evil.txt", b"x"),
        ]);
        assert!(matches!(
            s.stage_zip(&z, &ok).unwrap_err(),
            Error::UnsafePath(_)
        ));
        assert!(!t.path().join("data/evil.txt").exists());
    }

    #[test]
    fn only_plain_language_files_count_as_strings() {
        for ok in ["i18n/ru.ftl", "i18n/en.ftl", "i18n/pt-BR.ftl"] {
            assert!(is_ftl(ok), "{ok}");
        }
        for bad in [
            "i18n/RU.ftl",
            "i18n/rus.ftl",
            "i18n/pt-br.ftl",
            "i18n/sub/ru.ftl",
            "i18n/../x.ftl",
            "i18n/ru.txt",
            "ru.ftl",
            "xi18n/ru.ftl",
        ] {
            assert!(!is_ftl(bad), "{bad}");
        }
    }

    #[test]
    fn strings_survive_staging_from_a_zip_and_a_folder() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let z = zip_of(&[
            ("pkg/plugin.toml", TOML.as_bytes()),
            ("pkg/plugin.wasm", b"x"),
            ("pkg/i18n/ru.ftl", "hi = Привет".as_bytes()),
            ("pkg/i18n/deeper/en.ftl", b"hi = no"),
            ("pkg/i18n/notes.txt", b"no"),
        ]);
        let c = s.stage_zip(&z, &ok).unwrap();
        assert_eq!(
            std::fs::read_to_string(c.dir.join("i18n/ru.ftl")).unwrap(),
            "hi = Привет"
        );
        assert!(!c.dir.join("i18n/deeper").exists());
        assert!(!c.dir.join("i18n/notes.txt").exists());
        s.discard(&c);

        let src = t.path().join("src");
        std::fs::create_dir_all(src.join("i18n")).unwrap();
        std::fs::write(src.join("plugin.toml"), TOML).unwrap();
        std::fs::write(src.join("plugin.wasm"), b"x").unwrap();
        std::fs::write(src.join("i18n/en.ftl"), "hi = Hi").unwrap();
        std::fs::write(src.join("i18n/junk.ftl"), "hi = no").unwrap();
        let c = s.stage_path(&src, &ok).unwrap();
        assert!(c.dir.join("i18n/en.ftl").is_file());
        assert!(!c.dir.join("i18n/junk.ftl").exists());
    }

    #[test]
    fn unsafe_oversized_and_too_many_strings_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let base = [("plugin.toml", TOML.as_bytes()), ("plugin.wasm", &b"x"[..])];
        let with = |extra: &[(&str, &[u8])]| zip_of(&[&base[..], extra].concat());
        // A real escape is refused; `i18n/../x.ftl` stays inside the archive, is not an
        // `i18n/<lang>.ftl` and is ignored.
        assert!(matches!(
            s.stage_zip(&with(&[("../x.ftl", b"x")]), &ok).unwrap_err(),
            Error::UnsafePath(_)
        ));
        let c = s.stage_zip(&with(&[("i18n/../x.ftl", b"x")]), &ok).unwrap();
        assert!(!c.dir.join("x.ftl").exists() && !c.dir.join("i18n").exists());
        s.discard(&c);
        let big = vec![b'a'; MAX_FTL_BYTES as usize + 1];
        assert_eq!(
            s.stage_zip(&with(&[("i18n/ru.ftl", &big)]), &ok)
                .unwrap_err(),
            Error::TooBig
        );
        let names: Vec<String> = (b'a'..=b'z')
            .take(17)
            .map(|c| format!("i18n/{}{}.ftl", c as char, c as char))
            .collect();
        let many: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"k = v"[..])).collect();
        assert_eq!(s.stage_zip(&with(&many), &ok).unwrap_err(), Error::TooBig);
        // A folder with an oversized file is refused too, and nothing is left staged.
        let src = t.path().join("src");
        std::fs::create_dir_all(src.join("i18n")).unwrap();
        std::fs::write(src.join("plugin.toml"), TOML).unwrap();
        std::fs::write(src.join("plugin.wasm"), b"x").unwrap();
        std::fs::write(src.join("i18n/ru.ftl"), &big).unwrap();
        assert_eq!(s.stage_path(&src, &ok).unwrap_err(), Error::TooBig);
        assert!(!s.staging.exists());
    }

    #[test]
    fn remove_deletes_folder_and_config_entry() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let z = zip_of(&[("plugin.toml", TOML.as_bytes()), ("plugin.wasm", b"x")]);
        s.commit(&s.stage_zip(&z, &ok).unwrap()).unwrap();
        std::fs::write(s.root.join("hello_wasm/kv.json"), b"{}").unwrap();
        s.remove("hello_wasm").unwrap();
        assert!(!s.root.join("hello_wasm").exists());
        assert!(
            !Config::load(&s.config)
                .unwrap()
                .plugins
                .contains_key("hello_wasm")
        );
        assert!(s.remove("../x").is_err());
    }

    #[test]
    fn only_https_links() {
        assert!(is_https("https://example.com/p.zip"));
        assert!(is_https("  HTTPS://example.com/p.zip "));
        for bad in [
            "http://example.com/p.zip",
            "file:///etc/passwd",
            "https://",
            "ftp://x",
        ] {
            assert!(!is_https(bad), "{bad}");
            assert_eq!(download(bad).unwrap_err(), Error::NotHttps);
        }
    }
}
