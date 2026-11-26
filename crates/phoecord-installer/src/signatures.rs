//! Signature hot-fix (task 2.2.10): put new `signatures/*.toml` files where the loader reads
//! them. The loader loads the folder at every client start, so a new table works after a restart,
//! without a new DLL.
//!
//! Every source file is checked before anything is written, each file is replaced in one rename,
//! and the files and folders this creates are logged, so `uninstall` removes them. A file that was
//! already there without a log line is someone else's: it is replaced (that is what was asked for)
//! but stays after `uninstall`.

use crate::ca::create_dir_logged;
use crate::log::{InstallLog, Step};
use anyhow::{Context, Result, bail};
use phoecord_loader::signatures::SignatureSet;
use std::path::{Path, PathBuf};

/// One file [`install`] wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub path: PathBuf,
    /// Whether a file of this name was there before.
    pub replaced: bool,
    /// The build keys the file has tables for.
    pub builds: Vec<String>,
    /// `(build, file name)`: builds that a file read later also defines, so the loader takes
    /// that file's table instead of this one's.
    pub overridden: Vec<(String, String)>,
}

/// The `*.toml` files of `source`: the file itself, or those in the folder, by name.
fn sources(source: &Path) -> Result<Vec<PathBuf>> {
    let is_toml = |p: &Path| p.extension().is_some_and(|x| x == "toml");
    if source.is_dir() {
        let mut files = Vec::new();
        for entry in
            std::fs::read_dir(source).with_context(|| format!("reading {}", source.display()))?
        {
            let path = entry?.path();
            if path.is_file() && is_toml(&path) {
                files.push(path);
            }
        }
        files.sort();
        if files.is_empty() {
            bail!("no *.toml file in {}", source.display());
        }
        return Ok(files);
    }
    if !is_toml(source) {
        bail!(
            "{} is not a *.toml file: the loader would not read it",
            source.display()
        );
    }
    Ok(vec![source.to_path_buf()])
}

/// The build keys of a signature file's text; an error when it does not load or holds nothing.
fn builds_of(path: &Path, text: &str) -> Result<Vec<String>> {
    let set = SignatureSet::parse(text).with_context(|| path.display().to_string())?;
    let builds: Vec<String> = set
        .builds()
        .filter(|(_, count)| *count > 0)
        .map(|(key, _)| key.to_string())
        .collect();
    if builds.is_empty() {
        bail!("{} holds no signature", path.display());
    }
    Ok(builds)
}

/// Copy the signature files of `source` into `target` (the loader's folder). Nothing is written
/// unless every file loads.
pub fn install(log: &InstallLog, source: &Path, target: &Path) -> Result<Vec<Installed>> {
    let mut checked = Vec::new();
    for path in sources(source)? {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let builds = builds_of(&path, &text)?;
        let name = path
            .file_name()
            .with_context(|| format!("{} has no file name", path.display()))?
            .to_owned();
        checked.push((name, text, builds));
    }
    if let Some(data) = log.path().parent() {
        create_dir_logged(log, data)?;
    }
    create_dir_logged(log, target)?;
    let mut done = Vec::new();
    for (name, text, builds) in checked {
        let path = target.join(&name);
        let replaced = path.exists();
        // Not `*.toml`, so a client starting now never reads half a file.
        let mut tmp_name = name.clone();
        tmp_name.push(".phoecord-new");
        let tmp = target.join(tmp_name);
        let written = std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .with_context(|| format!("writing {}", path.display()));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        if !replaced {
            log.record(Step::File { path: path.clone() })?;
        }
        done.push(Installed {
            path,
            replaced,
            builds,
            overridden: Vec::new(),
        });
    }
    let present = list(target)?;
    for file in &mut done {
        for build in &file.builds {
            // The loader reads files in path order and the last table of a key wins.
            let winner = present
                .iter()
                .rfind(|f| f.builds.as_ref().is_ok_and(|b| b.contains(build)));
            if let Some(winner) = winner.filter(|w| w.path > file.path) {
                file.overridden.push((build.clone(), winner.name()));
            }
        }
    }
    Ok(done)
}

/// One file of the loader's folder.
#[derive(Clone, Debug)]
pub struct Present {
    pub path: PathBuf,
    /// The build keys, or why the loader skips the file.
    pub builds: Result<Vec<String>, String>,
}

impl Present {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }
}

/// The `*.toml` files in `dir`, in the order the loader reads them. A missing folder is empty.
pub fn list(dir: &Path) -> Result<Vec<Present>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|path| {
            let builds = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| SignatureSet::parse(&t).map_err(|e| e.to_string()))
                .map(|set| set.builds().map(|(k, _)| k.to_string()).collect());
            Present { path, builds }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Os;
    use crate::system::fake::{FakeSystem, env};
    use crate::uninstall::uninstall_all;

    const OLD: &str = "[build.\"1.0.0.1\"]\nf = { aob = \"55 8B EC\" }";
    const NEW: &str = "[build.\"1.0.0.1\"]\nf = { aob = \"CC CC CC\" }";

    struct World {
        _t: tempfile::TempDir,
        log: InstallLog,
        source: PathBuf,
        target: PathBuf,
        env: crate::system::Env,
    }

    fn world() -> World {
        let t = tempfile::tempdir().unwrap();
        let env = env(Os::Windows, t.path());
        let source = t.path().join("download");
        std::fs::create_dir_all(&source).unwrap();
        let target = t.path().join("Local/PhoeCord/signatures");
        let log = InstallLog::in_dir(&env.data_dir);
        World {
            _t: t,
            log,
            source,
            target,
            env,
        }
    }

    impl World {
        fn put(&self, name: &str, text: &str) -> PathBuf {
            let path = self.source.join(name);
            std::fs::write(&path, text).unwrap();
            path
        }

        fn pattern(&self) -> String {
            std::fs::read_to_string(self.target.join("phoenix.toml")).unwrap()
        }
    }

    #[test]
    fn a_new_file_replaces_the_old_one_and_the_loader_sees_it() {
        let w = world();
        let file = w.put("phoenix.toml", OLD);
        let first = install(&w.log, &file, &w.target).unwrap();
        assert!(!first[0].replaced);
        assert_eq!(first[0].builds, ["1.0.0.1"]);
        w.put("phoenix.toml", NEW);
        let second = install(&w.log, &file, &w.target).unwrap();
        assert!(second[0].replaced && second[0].overridden.is_empty());
        assert_eq!(w.pattern(), NEW);
        let (set, errors) = SignatureSet::load_dir(&w.target).unwrap();
        assert!(errors.is_empty() && !set.is_empty());
        // No temporary file stays behind.
        assert_eq!(std::fs::read_dir(&w.target).unwrap().count(), 1);
    }

    #[test]
    fn one_bad_file_means_nothing_is_written() {
        let w = world();
        w.put("a.toml", OLD);
        for bad in ["garbage [", "[build.\"1\"]", ""] {
            w.put("b.toml", bad);
            assert!(install(&w.log, &w.source, &w.target).is_err(), "{bad:?}");
            assert!(!w.target.exists());
        }
        let text = w.put("notes.txt", OLD);
        assert!(install(&w.log, &text, &w.target).is_err());
        std::fs::remove_file(w.source.join("a.toml")).unwrap();
        std::fs::remove_file(w.source.join("b.toml")).unwrap();
        assert!(install(&w.log, &w.source, &w.target).is_err());
    }

    #[test]
    fn a_later_file_with_the_same_build_is_reported() {
        let w = world();
        std::fs::create_dir_all(&w.target).unwrap();
        std::fs::write(w.target.join("z-old.toml"), OLD).unwrap();
        std::fs::write(w.target.join("a-older.toml"), OLD).unwrap();
        let file = w.put("phoenix.toml", NEW);
        let done = install(&w.log, &file, &w.target).unwrap();
        assert_eq!(
            done[0].overridden,
            [("1.0.0.1".to_string(), "z-old.toml".to_string())]
        );
    }

    #[test]
    fn uninstall_removes_what_was_created_and_nothing_else() {
        let w = world();
        std::fs::create_dir_all(&w.target).unwrap();
        std::fs::write(w.target.join("mine.toml"), OLD).unwrap();
        w.put("mine.toml", NEW);
        w.put("phoenix.toml", NEW);
        install(&w.log, &w.source, &w.target).unwrap();
        uninstall_all(&w.env, &FakeSystem::default()).unwrap();
        assert!(!w.target.join("phoenix.toml").exists());
        assert!(w.target.join("mine.toml").exists());
    }

    #[test]
    fn list_names_broken_files_and_accepts_a_missing_folder() {
        let w = world();
        assert!(list(&w.target).unwrap().is_empty());
        std::fs::create_dir_all(&w.target).unwrap();
        std::fs::write(w.target.join("b.toml"), "garbage [").unwrap();
        std::fs::write(w.target.join("a.toml"), OLD).unwrap();
        std::fs::write(w.target.join("c.txt"), OLD).unwrap();
        let files = list(&w.target).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].builds, Ok(vec!["1.0.0.1".to_string()]));
        assert!(files[1].builds.is_err());
    }
}
