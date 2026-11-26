//! Install step (opt-in, Windows only): the native loader DLL.
//!
//! `phoecord setup --loader` copies `version.dll` (shipped next to the `phoecord` binary) into the
//! newest `app-x.y.z` folder of the chosen client; the Discord client loads it from there (DLL
//! sideloading). Discord installs every update into a fresh `app-x.y.z` folder, so the copy would
//! be lost on the next update: [`ensure_loader`] runs on every `phoecord launch` and `phoecord run`
//! (autostart) and puts the DLL into the newest folder again.
//!
//! Ownership is decided by content. Every copy is logged as [`Step::LoaderCopy`] with the SHA-256 of
//! what was written. A `version.dll` that is already in the target folder is overwritten only when
//! its hash is one PhoeCord recorded for that file; any other file belongs to someone else and is
//! left alone ([`Outcome::Foreign`]). `uninstall` removes every logged copy (old app folders
//! included) that still holds our bytes, and nothing else.
//!
//! The user's choice itself is logged as [`Step::LoaderWanted`]; without it [`ensure_loader`] does
//! nothing.

use crate::discover::{Client, newest_app_dir};
use crate::log::{Entry, InstallLog, Step};
use crate::system::{Env, Os};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The name the loader has in the client's folder (the system DLL it stands in for).
pub const DLL_NAME: &str = "version.dll";

/// What happened to the loader of one client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The loader is Windows-only; nothing was done or recorded.
    Unsupported(Os),
    /// The DLL was copied (or updated) to `path`.
    Installed { path: PathBuf },
    /// Our DLL, with the current content, is already at `path`.
    UpToDate { path: PathBuf },
    /// `path` holds a `version.dll` PhoeCord did not write; it was left alone.
    Foreign { path: PathBuf },
    /// The DLL to copy is not at `source` (it ships next to the `phoecord` binary).
    SourceMissing { source: PathBuf },
    /// `client_dir` holds no `app-x.y.z` build to put the DLL in.
    NoBuild { client_dir: PathBuf },
    /// The copy failed (for example the folder is not writable, or Discord holds the old file open).
    Failed(String),
}

/// One client [`ensure_loader`] looked at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciled {
    /// The channel name as recorded at setup (`stable`, `ptb`, `canary`); empty when `install.log`
    /// itself could not be read (the outcome is then [`Outcome::Failed`]).
    pub channel: String,
    pub outcome: Outcome,
}

/// The folder of the Windows install that holds the DLL (ruling 2.4.5 Q3): a `version.dll` next
/// to `phoecord.exe` would be loaded into PhoeCord's own programs by the DLL search order.
pub const DLL_DIR: &str = "loader";

/// Where the DLL to install is: `version.dll` next to the running `phoecord` binary (the release
/// `.zip`, a build from source), else in its `loader` folder (the installed copy). When neither
/// exists, the first, so the error names the place the archive uses.
pub fn default_source(env: &Env) -> PathBuf {
    let Some(dir) = env.proxy_exe.parent() else {
        return PathBuf::from(DLL_NAME);
    };
    let beside = dir.join(DLL_NAME);
    let installed = dir.join(DLL_DIR).join(DLL_NAME);
    if !beside.is_file() && installed.is_file() {
        installed
    } else {
        beside
    }
}

/// The channel folder that holds a client's `app-x.y.z` builds: the parent of `client.root` when
/// that is an `app-x.y.z` folder, else `client.root` itself.
pub fn client_dir(client: &Client) -> PathBuf {
    let is_build = client
        .root
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("app-"));
    match client.root.parent() {
        Some(p) if is_build => p.to_path_buf(),
        _ => client.root.clone(),
    }
}

/// SHA-256 (lower-case hex) of the file at `path`; `None` when there is no file.
pub fn file_sha256(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(phoecord_proxy::trust::sha256_hex(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Whether `entries` record that PhoeCord wrote content with hash `sha256` to `path`.
pub fn is_our_copy(entries: &[Entry], path: &Path, sha256: &str) -> bool {
    entries.iter().any(
        |e| matches!(&e.step, Step::LoaderCopy { path: p, sha256: h } if p == path && h == sha256),
    )
}

/// `phoecord setup --loader`: remember that `client` wants the loader, then copy `source` into its
/// newest `app-x.y.z` folder. Safe to repeat. On another OS than Windows nothing is recorded and the
/// result is [`Outcome::Unsupported`], not an error.
pub fn install_loader(
    env: &Env,
    log: &InstallLog,
    client: &Client,
    source: &Path,
) -> Result<Outcome> {
    if env.os != Os::Windows {
        return Ok(Outcome::Unsupported(env.os));
    }
    let dir = client_dir(client);
    log.record(Step::LoaderWanted {
        channel: client.channel.name().to_string(),
        client_dir: dir.clone(),
    })?;
    place(log, &dir, source)
}

/// Put the loader back where it is missing: for every client the user opted in for, copy `source`
/// into the newest `app-x.y.z` folder unless our current DLL is already there. Idempotent and never
/// fatal: every problem is a [`Outcome`] for the caller to report. Empty when the loader was never
/// opted in.
pub fn ensure_loader(env: &Env, source: &Path) -> Vec<Reconciled> {
    let log = InstallLog::in_dir(&env.data_dir);
    let entries = match log.read() {
        Ok(e) => e,
        Err(e) => {
            return vec![Reconciled {
                channel: String::new(),
                outcome: Outcome::Failed(format!("{e:#}")),
            }];
        }
    };
    // One look per client folder, in the order the user opted in.
    let mut wanted: Vec<(String, PathBuf)> = Vec::new();
    for e in &entries {
        if let Step::LoaderWanted {
            channel,
            client_dir,
        } = &e.step
            && !wanted.iter().any(|(_, d)| d == client_dir)
        {
            wanted.push((channel.clone(), client_dir.clone()));
        }
    }
    wanted
        .into_iter()
        .map(|(channel, dir)| {
            let outcome = if env.os != Os::Windows {
                Outcome::Unsupported(env.os)
            } else {
                place(&log, &dir, source).unwrap_or_else(|e| Outcome::Failed(format!("{e:#}")))
            };
            Reconciled { channel, outcome }
        })
        .collect()
}

/// Copy `source` to `<newest app-x.y.z in client_dir>\version.dll` and log it, unless our current
/// DLL is already there or the file there is not ours.
fn place(log: &InstallLog, client_dir: &Path, source: &Path) -> Result<Outcome> {
    let Some((_, app)) = newest_app_dir(client_dir) else {
        return Ok(Outcome::NoBuild {
            client_dir: client_dir.to_path_buf(),
        });
    };
    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Outcome::SourceMissing {
                source: source.to_path_buf(),
            });
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", source.display())),
    };
    let hash = phoecord_proxy::trust::sha256_hex(&bytes);
    let target = app.join(DLL_NAME);
    let entries = log.read()?;
    match file_sha256(&target)? {
        Some(h) if !is_our_copy(&entries, &target, &h) => {
            return Ok(Outcome::Foreign { path: target });
        }
        Some(h) if h == hash => return Ok(Outcome::UpToDate { path: target }),
        _ => {}
    }
    // Write next to the target and rename over it, so a failed write never leaves half a DLL that
    // Discord would try to load.
    let tmp = app.join(format!("{DLL_NAME}.phoecord-new"));
    let written = std::fs::write(&tmp, &bytes)
        .and_then(|()| std::fs::rename(&tmp, &target))
        .with_context(|| format!("copying {} to {}", source.display(), target.display()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    log.record(Step::LoaderCopy {
        path: target.clone(),
        sha256: hash,
    })?;
    Ok(Outcome::Installed { path: target })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Channel;
    use crate::discover::Source;
    use crate::install::install;
    use crate::system::fake::{FakeSystem, env};
    use crate::uninstall::{Kept, uninstall_all};

    /// A temp root with an installed PhoeCord (`install`), our DLL next to the binary, and a
    /// Discord Canary channel folder holding `app-<version>` builds.
    struct World {
        _t: tempfile::TempDir,
        env: Env,
        sys: FakeSystem,
        source: PathBuf,
        channel_dir: PathBuf,
    }

    impl World {
        fn new(os: Os) -> World {
            let t = tempfile::tempdir().unwrap();
            let env = env(os, t.path());
            let sys = FakeSystem::default();
            install(&env, &sys).unwrap();
            let source = default_source(&env);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(&source, b"phoecord loader v1").unwrap();
            let channel_dir = t.path().join("appdata/Local/DiscordCanary");
            let w = World {
                _t: t,
                env,
                sys,
                source,
                channel_dir,
            };
            w.build("1.0.10");
            w
        }

        /// Discord installs (or updates into) `app-<version>`.
        fn build(&self, version: &str) -> PathBuf {
            let app = self.channel_dir.join(format!("app-{version}"));
            std::fs::create_dir_all(&app).unwrap();
            std::fs::write(app.join("DiscordCanary.exe"), b"exe").unwrap();
            app
        }

        fn client(&self) -> Client {
            let (version, root) = newest_app_dir(&self.channel_dir).unwrap();
            Client {
                channel: Channel::Canary,
                source: Source::Native,
                exe: root.join("DiscordCanary.exe"),
                root,
                version: Some(version),
            }
        }

        fn log(&self) -> InstallLog {
            InstallLog::in_dir(&self.env.data_dir)
        }

        fn setup(&self) -> Outcome {
            install_loader(&self.env, &self.log(), &self.client(), &self.source).unwrap()
        }

        fn ensure(&self) -> Vec<Outcome> {
            ensure_loader(&self.env, &self.source)
                .into_iter()
                .map(|r| {
                    assert_eq!(r.channel, "canary");
                    r.outcome
                })
                .collect()
        }

        fn dll(&self, version: &str) -> PathBuf {
            self.channel_dir
                .join(format!("app-{version}"))
                .join(DLL_NAME)
        }

        fn copies(&self) -> usize {
            self.log()
                .read()
                .unwrap()
                .iter()
                .filter(|e| matches!(e.step, Step::LoaderCopy { .. }))
                .count()
        }
    }

    #[test]
    fn fresh_install_copies_into_the_newest_build_and_logs_choice_and_hash() {
        let w = World::new(Os::Windows);
        w.build("1.0.9");
        let o = w.setup();
        assert_eq!(
            o,
            Outcome::Installed {
                path: w.dll("1.0.10")
            }
        );
        assert_eq!(
            std::fs::read(w.dll("1.0.10")).unwrap(),
            b"phoecord loader v1"
        );
        assert!(!w.dll("1.0.9").exists());
        assert!(!w.dll("1.0.10").with_extension("dll.phoecord-new").exists());
        let steps: Vec<Step> = w
            .log()
            .read()
            .unwrap()
            .into_iter()
            .map(|e| e.step)
            .collect();
        assert!(steps.contains(&Step::LoaderWanted {
            channel: "canary".into(),
            client_dir: w.channel_dir.clone(),
        }));
        assert!(steps.contains(&Step::LoaderCopy {
            path: w.dll("1.0.10"),
            sha256: phoecord_proxy::trust::sha256_hex(b"phoecord loader v1"),
        }));
    }

    #[test]
    fn re_running_changes_nothing() {
        let w = World::new(Os::Windows);
        w.setup();
        let before = w.log().read().unwrap();
        assert_eq!(
            w.setup(),
            Outcome::UpToDate {
                path: w.dll("1.0.10")
            }
        );
        assert_eq!(
            w.ensure(),
            vec![Outcome::UpToDate {
                path: w.dll("1.0.10")
            }]
        );
        assert_eq!(w.log().read().unwrap(), before);
    }

    #[test]
    fn a_discord_update_gets_the_dll_on_the_next_start() {
        let w = World::new(Os::Windows);
        w.setup();
        w.build("1.0.11");
        assert_eq!(
            w.ensure(),
            vec![Outcome::Installed {
                path: w.dll("1.0.11")
            }]
        );
        assert!(w.dll("1.0.11").is_file() && w.dll("1.0.10").is_file());
        assert_eq!(w.copies(), 2);
        assert_eq!(
            w.ensure(),
            vec![Outcome::UpToDate {
                path: w.dll("1.0.11")
            }]
        );
    }

    #[test]
    fn a_new_phoecord_dll_replaces_our_old_one() {
        let w = World::new(Os::Windows);
        w.setup();
        std::fs::write(&w.source, b"phoecord loader v2").unwrap();
        assert_eq!(
            w.ensure(),
            vec![Outcome::Installed {
                path: w.dll("1.0.10")
            }]
        );
        assert_eq!(
            std::fs::read(w.dll("1.0.10")).unwrap(),
            b"phoecord loader v2"
        );
    }

    #[test]
    fn a_foreign_dll_is_refused_and_kept() {
        let w = World::new(Os::Windows);
        std::fs::write(w.dll("1.0.10"), b"someone else's dll").unwrap();
        assert_eq!(
            w.setup(),
            Outcome::Foreign {
                path: w.dll("1.0.10")
            }
        );
        assert_eq!(
            w.ensure(),
            vec![Outcome::Foreign {
                path: w.dll("1.0.10")
            }]
        );
        assert_eq!(
            std::fs::read(w.dll("1.0.10")).unwrap(),
            b"someone else's dll"
        );
        assert_eq!(w.copies(), 0);
    }

    #[test]
    fn our_own_bytes_we_never_wrote_there_count_as_foreign() {
        // Copied by hand, not by PhoeCord: uninstall must not take it, so setup does not claim it.
        let w = World::new(Os::Windows);
        std::fs::copy(&w.source, w.dll("1.0.10")).unwrap();
        assert_eq!(
            w.setup(),
            Outcome::Foreign {
                path: w.dll("1.0.10")
            }
        );
    }

    #[test]
    fn uninstall_removes_every_copy_and_keeps_foreign_files() {
        let w = World::new(Os::Windows);
        w.setup();
        w.build("1.0.11");
        w.ensure();
        w.build("1.0.12");
        w.ensure();
        // Someone replaced our copy in the newest build after we made it.
        std::fs::write(w.dll("1.0.12"), b"replaced by another tool").unwrap();
        // And a foreign DLL sits in a build we never touched.
        let other = w.build("0.9.0").join(DLL_NAME);
        std::fs::write(&other, b"foreign").unwrap();

        let r = uninstall_all(&w.env, &w.sys).unwrap();
        assert!(r.is_clean(), "{:?}", r.failed);
        assert!(!w.dll("1.0.10").exists() && !w.dll("1.0.11").exists());
        assert_eq!(
            std::fs::read(w.dll("1.0.12")).unwrap(),
            b"replaced by another tool"
        );
        assert!(other.is_file());
        assert!(r.kept.contains(&(
            Step::LoaderCopy {
                path: w.dll("1.0.12"),
                sha256: phoecord_proxy::trust::sha256_hex(b"phoecord loader v1"),
            },
            Kept::NotOurs
        )));
        assert!(
            r.removed
                .iter()
                .any(|s| matches!(s, Step::LoaderWanted { .. }))
        );
        // Discord's own files stay.
        assert!(w.channel_dir.join("app-1.0.10/DiscordCanary.exe").is_file());
        // Nothing left that would make a later start install the loader again.
        assert!(ensure_loader(&w.env, &w.source).is_empty());
    }

    #[test]
    fn uninstall_after_a_dll_update_removes_the_updated_copy() {
        let w = World::new(Os::Windows);
        w.setup();
        std::fs::write(&w.source, b"phoecord loader v2").unwrap();
        w.ensure();
        let r = uninstall_all(&w.env, &w.sys).unwrap();
        assert!(r.is_clean(), "{:?}", r.failed);
        assert!(!w.dll("1.0.10").exists());
        assert!(!r.kept.iter().any(|(_, k)| *k == Kept::NotOurs), "{r:?}");
    }

    #[test]
    fn other_systems_are_unsupported_and_record_nothing() {
        for os in [Os::MacOs, Os::Linux] {
            let w = World::new(os);
            assert_eq!(w.setup(), Outcome::Unsupported(os));
            assert!(!w.dll("1.0.10").exists());
            assert!(
                !w.log()
                    .read()
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e.step, Step::LoaderWanted { .. }))
            );
            assert!(ensure_loader(&w.env, &w.source).is_empty());
        }
    }

    #[test]
    fn not_opted_in_means_ensure_does_nothing() {
        let w = World::new(Os::Windows);
        let before = w.log().read().unwrap();
        assert!(ensure_loader(&w.env, &w.source).is_empty());
        assert!(!w.dll("1.0.10").exists());
        assert_eq!(w.log().read().unwrap(), before);
        // No data folder at all (PhoeCord never set up): still nothing, still no error.
        let t = tempfile::tempdir().unwrap();
        assert!(ensure_loader(&env(Os::Windows, t.path()), &w.source).is_empty());
    }

    #[test]
    fn problems_are_outcomes_not_errors() {
        let w = World::new(Os::Windows);
        w.setup();
        std::fs::remove_file(&w.source).unwrap();
        w.build("1.0.11");
        assert_eq!(
            w.ensure(),
            vec![Outcome::SourceMissing {
                source: w.source.clone()
            }]
        );
        std::fs::remove_dir_all(&w.channel_dir).unwrap();
        assert_eq!(
            w.ensure(),
            vec![Outcome::NoBuild {
                client_dir: w.channel_dir.clone()
            }]
        );
        std::fs::write(w.log().path(), "{not json}\n").unwrap();
        let r = ensure_loader(&w.env, &w.source);
        assert!(matches!(
            r[..],
            [Reconciled {
                outcome: Outcome::Failed(_),
                ..
            }]
        ));
    }

    #[test]
    fn client_dir_is_the_channel_folder() {
        let w = World::new(Os::Windows);
        assert_eq!(client_dir(&w.client()), w.channel_dir);
        let mut c = w.client();
        c.root = w.channel_dir.clone();
        assert_eq!(client_dir(&c), w.channel_dir);
    }

    #[test]
    fn the_installed_copy_finds_the_dll_in_its_loader_folder() {
        let t = tempfile::tempdir().unwrap();
        let env = env(Os::Windows, t.path());
        let dir = env.proxy_exe.parent().unwrap().to_path_buf();
        // Neither: the place the release archive uses, so the error names it.
        assert_eq!(default_source(&env), dir.join(DLL_NAME));
        std::fs::create_dir_all(dir.join(DLL_DIR)).unwrap();
        std::fs::write(dir.join(DLL_DIR).join(DLL_NAME), b"installed").unwrap();
        assert_eq!(default_source(&env), dir.join(DLL_DIR).join(DLL_NAME));
        // Next to the binary wins (the archive, a build from source).
        std::fs::write(dir.join(DLL_NAME), b"beside").unwrap();
        assert_eq!(default_source(&env), dir.join(DLL_NAME));
    }
}
