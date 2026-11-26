//! Desktop Phoenix: which transport a launch uses, and how the client is started (task 2.2.8).
//!
//! The rulings of 2026-11-26, in code:
//!
//! - The choice is made by `phoecord launch --channel phoenix` before the client starts, because
//!   `--proxy-server` can only be passed at process start. [`choose`] runs the loader's hook
//!   locators on `Discord.exe` on disk ([`phoecord_loader::check::transport_plan`], the same code
//!   as `phoecord check` and the loader) and caches the verdict in `phoenix-check.json` in the data
//!   folder, keyed by the client's `.text` hash and the signature folder's hash
//!   ([`phoecord_loader::choice::CheckCache`]). A Phoenix update or a signature hot-fix misses the
//!   cache and re-runs the check; nothing else needs reconciling after an update, since the loader
//!   is injected at every launch and nothing is copied into Phoenix's folder.
//! - All or nothing per launch: every Engine entry point found → [`Mode::InProcess`], no proxy
//!   process at all; any one missing → [`Mode::Proxy`] for the whole launch, and the loader does the
//!   UI only.
//! - A build marked failed (a hook failed in an in-process session: the loader writes the mark,
//!   [`phoecord_loader::choice::mark_failed`]) goes straight to the proxy; a changed signature
//!   folder is a new key, so the mark no longer applies.
//! - `--transport` or `[phoenix] transport` forces a mode (default `auto`); on macOS and Linux
//!   Phoenix gets the proxy and a plain launch.
//!
//! The loader is injected in both modes (it does the UI either way) when it is there; without it a
//! launch is a plain proxy launch. Every decision is tested here against fake locator results
//! ([`Probe`]); the real one reads the file ([`DiskProbe`]).

use crate::discover::Client;
use crate::system::Os;
use anyhow::{Context, Result};
use phoecord_loader::choice::{
    CheckCache, ENV_CACHE, ENV_MODE, ENV_SIGNATURES_HASH, ExeStamp, Mode, Verdict,
};
use phoecord_loader::plan::Plan;
use phoecord_proxy::config::PhoenixTransport;
use std::path::{Path, PathBuf};

pub use phoecord_loader::choice::CACHE_FILE;

/// Why a launch got its transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// `--transport` or `[phoenix] transport` named the mode.
    Forced,
    /// In-process runs on Windows only.
    NotWindows,
    /// There is no loader DLL to inject (it was looked for at this path).
    NoLoader(PathBuf),
    /// There is no signature folder: nothing to find the hooks with.
    NoSignatures,
    /// The exe could not be checked; the error.
    CheckFailed(String),
    /// A hook failed in an earlier in-process session of this build with these signatures.
    MarkedFailed,
    /// The check ran at an earlier launch of this build with these signatures.
    Cached,
    /// The check ran now.
    Checked,
}

/// What [`choose`] decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub mode: Mode,
    pub why: Why,
    /// The entry points that were not found, one line each (a check that chose the proxy).
    pub missing: Vec<String>,
    /// The loader DLL to inject; `None` for a plain launch.
    pub loader: Option<PathBuf>,
    /// The signature folder's hash, when it was taken: the loader keys its "failed" mark by it.
    pub signatures_hash: Option<String>,
    /// The cache could not be written (the choice stands; the next launch checks again).
    pub cache_error: Option<String>,
}

/// In-process was forced but cannot run here.
#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable(pub Why);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Why::NoLoader(p) => write!(
                f,
                "Discord Phoenix cannot run in-process: no loader at {}",
                p.display()
            ),
            _ => f.write_str("Discord Phoenix runs in-process only on Windows"),
        }
    }
}

impl std::error::Error for Unavailable {}

/// What a check needs from the client exe. [`DiskProbe`] reads the file; tests give fake results.
pub trait Probe {
    /// The lowercase hex SHA-256 of the exe's `.text` section.
    fn text_hash(&mut self, exe: &Path) -> Result<String>;
    /// The loader's degradation plan for the exe with the signatures in `signatures`.
    fn plan(&mut self, exe: &Path, signatures: &Path) -> Result<Plan>;
}

/// The real [`Probe`]: reads the exe once, however many questions it is asked.
#[derive(Default)]
pub struct DiskProbe {
    bytes: Option<(PathBuf, Vec<u8>)>,
}

impl DiskProbe {
    fn bytes(&mut self, exe: &Path) -> Result<&[u8]> {
        if self.bytes.as_ref().is_none_or(|(p, _)| p != exe) {
            let b = std::fs::read(exe).with_context(|| format!("reading {}", exe.display()))?;
            self.bytes = Some((exe.to_path_buf(), b));
        }
        Ok(self
            .bytes
            .as_ref()
            .map(|(_, b)| b.as_slice())
            .unwrap_or(&[]))
    }
}

impl Probe for DiskProbe {
    fn text_hash(&mut self, exe: &Path) -> Result<String> {
        let v = phoecord_loader::version::detect(self.bytes(exe)?)
            .map_err(|e| anyhow::anyhow!("{}: {e}", exe.display()))?;
        Ok(v.text_hash)
    }

    fn plan(&mut self, exe: &Path, signatures: &Path) -> Result<Plan> {
        let (_, plan) = phoecord_loader::check::transport_plan(self.bytes(exe)?, signatures)
            .map_err(|e| anyhow::anyhow!("{}: {e}", exe.display()))?;
        Ok(plan)
    }
}

/// What [`choose`] looks at.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    pub os: Os,
    /// `--transport`, else `[phoenix] transport`.
    pub requested: PhoenixTransport,
    /// Phoenix's `Discord.exe`.
    pub exe: &'a Path,
    /// The loader DLL to inject ([`crate::loader::default_source`]).
    pub loader: &'a Path,
    /// The folder the loader reads signatures from; `None` when there is none on this OS.
    pub signatures: Option<&'a Path>,
    /// `phoenix-check.json` in the data folder.
    pub cache: &'a Path,
}

/// `(length, modification time in ns)` of a file, to tell whether it changed since it was hashed.
fn file_stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let ns = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((m.len(), u64::try_from(ns).ok()?))
}

/// Picks the transport of one Phoenix launch (see the module docs).
pub fn choose(i: &Inputs<'_>, probe: &mut dyn Probe) -> Result<Choice, Unavailable> {
    let loader = i.loader.is_file().then(|| i.loader.to_path_buf());
    let mut choice = Choice {
        mode: Mode::Proxy,
        why: Why::Forced,
        missing: Vec::new(),
        loader: loader.clone().filter(|_| i.os == Os::Windows),
        signatures_hash: None,
        cache_error: None,
    };
    if i.os != Os::Windows {
        if i.requested == PhoenixTransport::InProcess {
            return Err(Unavailable(Why::NotWindows));
        }
        choice.why = Why::NotWindows;
        return Ok(choice);
    }
    match i.requested {
        PhoenixTransport::Proxy => return Ok(choice),
        PhoenixTransport::InProcess if loader.is_none() => {
            return Err(Unavailable(Why::NoLoader(i.loader.to_path_buf())));
        }
        PhoenixTransport::InProcess => {
            choice.mode = Mode::InProcess;
            choice.signatures_hash = i
                .signatures
                .and_then(|d| phoecord_loader::signatures::folder_hash(d).ok());
            return Ok(choice);
        }
        PhoenixTransport::Auto => {}
    }
    if loader.is_none() {
        choice.why = Why::NoLoader(i.loader.to_path_buf());
        return Ok(choice);
    }
    let Some(signatures) = i.signatures.filter(|d| d.is_dir()) else {
        choice.why = Why::NoSignatures;
        return Ok(choice);
    };
    let signatures_hash = match phoecord_loader::signatures::folder_hash(signatures) {
        Ok(h) => h,
        Err(e) => {
            choice.why = Why::CheckFailed(format!("{}: {e}", signatures.display()));
            return Ok(choice);
        }
    };
    choice.signatures_hash = Some(signatures_hash.clone());
    let mut cache = CheckCache::load(i.cache);
    // The `.text` hash is the key; an unchanged file is not hashed again.
    let stamp = file_stamp(i.exe);
    let known = cache.exe.as_ref().filter(|s| {
        s.path == i.exe
            && stamp.is_some_and(|(len, modified)| s.len == len && s.modified == modified)
    });
    let text_hash = match known {
        Some(s) => s.text_hash.clone(),
        None => match probe.text_hash(i.exe) {
            Ok(h) => {
                cache.exe = stamp.map(|(len, modified)| ExeStamp {
                    path: i.exe.to_path_buf(),
                    len,
                    modified,
                    text_hash: h.clone(),
                });
                h
            }
            Err(e) => {
                choice.why = Why::CheckFailed(format!("{e:#}"));
                return Ok(choice);
            }
        },
    };
    match cache.lookup(&text_hash, &signatures_hash) {
        Some(v) if v.failed => choice.why = Why::MarkedFailed,
        Some(Verdict {
            mode: Some(mode),
            missing,
            ..
        }) => {
            choice.mode = *mode;
            choice.missing = missing.clone();
            choice.why = Why::Cached;
        }
        _ => {
            let plan = match probe.plan(i.exe, signatures) {
                Ok(p) => p,
                Err(e) => {
                    choice.why = Why::CheckFailed(format!("{e:#}"));
                    return Ok(choice);
                }
            };
            choice.mode = if plan.all_in_process() {
                Mode::InProcess
            } else {
                Mode::Proxy
            };
            choice.missing = plan.net_report();
            choice.why = Why::Checked;
            cache.record(Verdict {
                text_hash,
                signatures_hash,
                mode: Some(choice.mode),
                missing: choice.missing.clone(),
                failed: false,
            });
        }
    }
    if let Err(e) = cache.save(i.cache) {
        choice.cache_error = Some(format!("{}: {e}", i.cache.display()));
    }
    Ok(choice)
}

/// How an injected Phoenix is started: its exe, arguments, the variables set over this process's
/// environment, the loader DLL and the user's other DLLs (`[phoenix] also_inject`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injected {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub dll: PathBuf,
    /// Loaded after [`Injected::dll`], in this order.
    pub also: Vec<PathBuf>,
}

/// The injected start for `choice`, or `None` when it has no loader (then `launch` starts the
/// client plainly, as [`crate::launch::client_command`] does). `proxy` is the proxy's address in
/// proxy mode; `cache` is where the loader writes the "failed" mark; `also` are the user's other
/// DLLs, loaded after PhoeCord's.
pub fn injected(
    client: &Client,
    choice: &Choice,
    proxy: Option<&str>,
    cache: &Path,
    also: &[PathBuf],
) -> Option<Injected> {
    let dll = choice.loader.clone()?;
    let mut args = Vec::new();
    let mut env = vec![(ENV_MODE.to_string(), choice.mode.name().to_string())];
    match (choice.mode, proxy) {
        (Mode::Proxy, Some(addr)) => {
            let url = format!("http://{addr}");
            args.push(format!("--proxy-server={url}"));
            for k in ["HTTPS_PROXY", "https_proxy"] {
                env.push((k.to_string(), url.clone()));
            }
            env.push(("NO_PROXY".into(), "localhost,127.0.0.1,::1".into()));
        }
        (Mode::Proxy, None) => {}
        (Mode::InProcess, _) => {
            env.push((ENV_CACHE.to_string(), cache.display().to_string()));
            if let Some(h) = &choice.signatures_hash {
                env.push((ENV_SIGNATURES_HASH.to_string(), h.clone()));
            }
        }
    }
    Some(Injected {
        exe: client.exe.clone(),
        args,
        env,
        dll,
        also: also.to_vec(),
    })
}

/// Starts `start` suspended, injects its loader and lets it run (`phoecord_loader::inject`, 2.1.7);
/// returns the process id. The target's current folder is the exe's own, as Phoenix's shortcut
/// starts it.
#[cfg(windows)]
pub fn spawn(start: &Injected) -> Result<u32> {
    use phoecord_loader::inject;
    let args: Vec<&str> = start.args.iter().map(String::as_str).collect();
    let env: Vec<(&str, &str)> = start
        .env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let p = inject::spawn_suspended_with_env(&start.exe, &args, &env)
        .with_context(|| format!("starting {}", start.exe.display()))?;
    // Queued APCs run in order on the main thread before any of Phoenix's code: PhoeCord's
    // loader first, then the user's other DLLs.
    for dll in std::iter::once(&start.dll).chain(&start.also) {
        inject::queue_load_library(&p, dll)
            .with_context(|| format!("injecting {}", dll.display()))?;
    }
    Ok(inject::resume(p)?)
}

/// Blocks until the Phoenix started by [`spawn`] as `pid` exits, however it ends (closed, crashed,
/// killed in Task Manager); returns its exit code.
#[cfg(windows)]
pub fn wait_exit(pid: u32) -> Result<u32> {
    phoecord_loader::inject::wait_exit(pid).with_context(|| format!("waiting for process {pid}"))
}

/// Nothing is ever injected elsewhere, so there is nothing to wait for.
#[cfg(not(windows))]
pub fn wait_exit(_pid: u32) -> Result<u32> {
    anyhow::bail!("Windows only")
}

/// Injection is Windows-only; [`choose`] never sets a loader elsewhere.
#[cfg(not(windows))]
pub fn spawn(start: &Injected) -> Result<u32> {
    anyhow::bail!(
        "cannot inject {} into {}: Windows only",
        start.dll.display(),
        start.exe.display()
    )
}

/// Remove the check cache from `data_dir` (uninstall). `true` when there was one.
pub fn remove_cache(data_dir: &Path) -> bool {
    std::fs::remove_file(data_dir.join(CACHE_FILE)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Channel;
    use crate::discover::Source;
    use phoecord_loader::plan::{EntryPoint, Probe as LocatorProbe, Target};

    /// Fake locator results, the way `phoecord_loader::plan` is tested: everything resolves except
    /// `missing`.
    fn plan_without(missing: &'static [Target]) -> Plan {
        Plan::resolve(move |t| -> LocatorProbe {
            if missing.contains(&t) {
                Err("not found".into())
            } else {
                Ok(())
            }
        })
    }

    /// A probe with a fixed `.text` hash and plan that counts how often it is asked.
    struct Fake {
        hash: Result<String, String>,
        plan: Plan,
        hashed: usize,
        checked: usize,
    }

    impl Fake {
        fn new(hash: &str, plan: Plan) -> Fake {
            Fake {
                hash: Ok(hash.into()),
                plan,
                hashed: 0,
                checked: 0,
            }
        }
    }

    impl Probe for Fake {
        fn text_hash(&mut self, _: &Path) -> Result<String> {
            self.hashed += 1;
            self.hash.clone().map_err(anyhow::Error::msg)
        }

        fn plan(&mut self, _: &Path, _: &Path) -> Result<Plan> {
            self.checked += 1;
            Ok(self.plan.clone())
        }
    }

    /// A data folder, a signature folder with one file, a loader and a fake `Discord.exe`.
    struct Machine {
        _t: tempfile::TempDir,
        exe: PathBuf,
        loader: PathBuf,
        signatures: PathBuf,
        cache: PathBuf,
    }

    impl Machine {
        fn new() -> Machine {
            let t = tempfile::tempdir().unwrap();
            let app = t.path().join("DiscordPhoenix").join("app");
            let signatures = t.path().join("PhoeCord").join("signatures");
            std::fs::create_dir_all(&app).unwrap();
            std::fs::create_dir_all(&signatures).unwrap();
            std::fs::write(signatures.join("phoenix.toml"), "[build.\"1\"]\n").unwrap();
            let exe = app.join("Discord.exe");
            std::fs::write(&exe, b"build one").unwrap();
            let loader = t.path().join("bin").join("version.dll");
            std::fs::create_dir_all(loader.parent().unwrap()).unwrap();
            std::fs::write(&loader, b"dll").unwrap();
            let cache = t.path().join("data").join(CACHE_FILE);
            Machine {
                exe,
                loader,
                signatures,
                cache,
                _t: t,
            }
        }

        fn inputs(&self, requested: PhoenixTransport) -> Inputs<'_> {
            Inputs {
                os: Os::Windows,
                requested,
                exe: &self.exe,
                loader: &self.loader,
                signatures: Some(&self.signatures),
                cache: &self.cache,
            }
        }

        fn auto(&self, probe: &mut Fake) -> Choice {
            choose(&self.inputs(PhoenixTransport::Auto), probe).unwrap()
        }
    }

    #[test]
    fn every_entry_point_found_runs_in_process_and_injects() {
        let m = Machine::new();
        let mut p = Fake::new("t1", plan_without(&[]));
        let c = m.auto(&mut p);
        assert_eq!(c.mode, Mode::InProcess);
        assert_eq!(c.why, Why::Checked);
        assert!(c.missing.is_empty());
        assert_eq!(c.loader.as_deref(), Some(m.loader.as_path()));
        assert!(c.signatures_hash.is_some() && c.cache_error.is_none());
        // A missing UI locator is not the transport's business.
        let m = Machine::new();
        let c = m.auto(&mut Fake::new(
            "t1",
            plan_without(&[Target::Ui(phoecord_loader::plan::UiFeature::Theme)]),
        ));
        assert_eq!(c.mode, Mode::InProcess);
    }

    #[test]
    fn one_missing_entry_point_sends_the_whole_launch_through_the_proxy() {
        for point in EntryPoint::ALL {
            let m = Machine::new();
            let missing: &'static [Target] = Box::leak(Box::new([Target::Net(point)]));
            let c = m.auto(&mut Fake::new("t1", plan_without(missing)));
            assert_eq!(c.mode, Mode::Proxy, "{point:?}");
            assert_eq!(c.why, Why::Checked);
            assert!(
                c.missing.iter().any(|l| l.starts_with(point.name())),
                "{point:?}: {:?}",
                c.missing
            );
            // The loader still goes in, for the UI.
            assert!(c.loader.is_some());
        }
        // An unknown build too.
        let m = Machine::new();
        let c = m.auto(&mut Fake::new("t1", Plan::unknown_build()));
        assert_eq!(c.mode, Mode::Proxy);
        assert_eq!(c.missing.len(), EntryPoint::ALL.len());
    }

    #[test]
    fn the_check_runs_once_per_build_and_signature_folder() {
        let m = Machine::new();
        let mut p = Fake::new("t1", plan_without(&[]));
        assert_eq!(m.auto(&mut p).why, Why::Checked);
        let again = m.auto(&mut p);
        assert_eq!((again.mode, again.why), (Mode::InProcess, Why::Cached));
        assert_eq!(p.checked, 1);
        // The unchanged exe was not hashed again either.
        assert_eq!(p.hashed, 1);
        // A Phoenix update: a new file and a new `.text` hash.
        std::fs::write(&m.exe, b"build two, longer").unwrap();
        let mut update = Fake::new("t2", plan_without(&[Target::Net(EntryPoint::Send)]));
        let c = m.auto(&mut update);
        assert_eq!((c.mode, c.why), (Mode::Proxy, Why::Checked));
        assert_eq!((update.hashed, update.checked), (1, 1));
        // A signature hot-fix: same build, new folder hash, checked again.
        std::fs::write(m.signatures.join("hotfix.toml"), "[build.\"2\"]\n").unwrap();
        let mut fixed = Fake::new("t2", plan_without(&[]));
        let c = m.auto(&mut fixed);
        assert_eq!((c.mode, c.why), (Mode::InProcess, Why::Checked));
        assert_eq!((fixed.hashed, fixed.checked), (0, 1));
    }

    #[test]
    fn a_build_marked_failed_uses_the_proxy_until_the_signatures_change() {
        let m = Machine::new();
        let mut p = Fake::new("t1", plan_without(&[]));
        let first = m.auto(&mut p);
        assert_eq!(first.mode, Mode::InProcess);
        // The loader marks the build failed during the session (the seam it will call).
        phoecord_loader::choice::mark_failed(
            &m.cache,
            "t1",
            first.signatures_hash.as_deref().unwrap(),
        )
        .unwrap();
        let c = m.auto(&mut p);
        assert_eq!((c.mode, c.why), (Mode::Proxy, Why::MarkedFailed));
        assert_eq!(p.checked, 1, "a marked build is not checked again");
        // A hot-fix changes the folder's hash: the mark no longer applies.
        std::fs::write(
            m.signatures.join("phoenix.toml"),
            "# fixed\n[build.\"1\"]\n",
        )
        .unwrap();
        let c = m.auto(&mut p);
        assert_eq!((c.mode, c.why), (Mode::InProcess, Why::Checked));
        assert_eq!(p.checked, 2);
    }

    #[test]
    fn transport_overrides_the_check() {
        let m = Machine::new();
        let mut p = Fake::new("t1", plan_without(&[]));
        let c = choose(&m.inputs(PhoenixTransport::Proxy), &mut p).unwrap();
        assert_eq!((c.mode, c.why), (Mode::Proxy, Why::Forced));
        assert!(c.loader.is_some());
        let mut p = Fake::new("t1", Plan::unknown_build());
        let c = choose(&m.inputs(PhoenixTransport::InProcess), &mut p).unwrap();
        assert_eq!((c.mode, c.why), (Mode::InProcess, Why::Forced));
        assert!(c.signatures_hash.is_some());
        // Neither ran the check or wrote the cache.
        assert_eq!((p.hashed, p.checked), (0, 0));
        assert!(!m.cache.exists());
    }

    #[test]
    fn in_process_needs_windows_and_the_loader() {
        let m = Machine::new();
        let mut p = Fake::new("t1", plan_without(&[]));
        for os in [Os::Linux, Os::MacOs] {
            let mut i = m.inputs(PhoenixTransport::Auto);
            i.os = os;
            let c = choose(&i, &mut p).unwrap();
            assert_eq!(
                (c.mode, &c.why, &c.loader),
                (Mode::Proxy, &Why::NotWindows, &None)
            );
            i.requested = PhoenixTransport::InProcess;
            assert_eq!(choose(&i, &mut p), Err(Unavailable(Why::NotWindows)));
        }
        std::fs::remove_file(&m.loader).unwrap();
        let c = m.auto(&mut p);
        assert_eq!(c.mode, Mode::Proxy);
        assert_eq!(c.why, Why::NoLoader(m.loader.clone()));
        assert!(c.loader.is_none());
        assert_eq!(
            choose(&m.inputs(PhoenixTransport::InProcess), &mut p),
            Err(Unavailable(Why::NoLoader(m.loader.clone())))
        );
        assert_eq!((p.hashed, p.checked), (0, 0));
    }

    #[test]
    fn no_signatures_or_an_unreadable_exe_is_the_proxy_and_not_cached() {
        let m = Machine::new();
        let mut i = m.inputs(PhoenixTransport::Auto);
        i.signatures = None;
        let mut p = Fake::new("t1", plan_without(&[]));
        assert_eq!(choose(&i, &mut p).unwrap().why, Why::NoSignatures);
        std::fs::remove_dir_all(&m.signatures).unwrap();
        assert_eq!(m.auto(&mut p).why, Why::NoSignatures);
        let m = Machine::new();
        let mut broken = Fake::new("t1", plan_without(&[]));
        broken.hash = Err("not a PE".into());
        let c = m.auto(&mut broken);
        assert_eq!(c.mode, Mode::Proxy);
        assert!(matches!(&c.why, Why::CheckFailed(e) if e.contains("not a PE")));
        assert!(!m.cache.exists());
    }

    #[test]
    fn a_cache_that_cannot_be_written_does_not_stop_the_launch() {
        let m = Machine::new();
        // The cache's folder is a file.
        std::fs::write(m.cache.parent().unwrap(), b"").unwrap();
        let c = m.auto(&mut Fake::new("t1", plan_without(&[])));
        assert_eq!(c.mode, Mode::InProcess);
        assert!(c.cache_error.is_some());
    }

    #[test]
    fn the_real_probe_reads_a_pe_and_refuses_garbage() {
        let m = Machine::new();
        let mut p = DiskProbe::default();
        assert!(p.text_hash(&m.exe).is_err());
        assert!(p.plan(&m.exe, &m.signatures).is_err());
        assert!(p.text_hash(&m.exe.with_file_name("absent.exe")).is_err());
    }

    fn phoenix(exe: &Path) -> Client {
        Client {
            channel: Channel::Phoenix,
            source: Source::Native,
            root: exe.parent().unwrap().to_path_buf(),
            exe: exe.to_path_buf(),
            version: None,
        }
    }

    #[test]
    fn in_process_starts_without_proxy_flags_and_tells_the_loader() {
        let m = Machine::new();
        let c = m.auto(&mut Fake::new("t1", plan_without(&[])));
        let s = injected(&phoenix(&m.exe), &c, None, &m.cache, &[]).unwrap();
        assert!(s.also.is_empty());
        assert_eq!(s.exe, m.exe);
        assert_eq!(s.dll, m.loader);
        assert!(s.args.is_empty());
        let env = |k: &str| s.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(env(ENV_MODE), Some("in-process"));
        assert_eq!(env(ENV_CACHE), Some(m.cache.display().to_string().as_str()));
        assert_eq!(env(ENV_SIGNATURES_HASH), c.signatures_hash.as_deref());
        assert_eq!(env("HTTPS_PROXY"), None);
    }

    #[test]
    fn proxy_mode_injects_with_the_proxy_flags() {
        let m = Machine::new();
        let c = m.auto(&mut Fake::new("t1", Plan::unknown_build()));
        let sunbird = PathBuf::from("C:/Sunbird/phoenix.dll");
        let s = injected(
            &phoenix(&m.exe),
            &c,
            Some("127.0.0.1:18463"),
            &m.cache,
            std::slice::from_ref(&sunbird),
        )
        .unwrap();
        assert_eq!(s.also, [sunbird]);
        assert_eq!(s.args, ["--proxy-server=http://127.0.0.1:18463"]);
        let env = |k: &str| s.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(env(ENV_MODE), Some("proxy"));
        assert_eq!(env("HTTPS_PROXY"), Some("http://127.0.0.1:18463"));
        assert_eq!(env("NO_PROXY"), Some("localhost,127.0.0.1,::1"));
        assert_eq!(env(ENV_CACHE), None);
        // Without a loader there is nothing to inject: `launch` starts it plainly.
        let plain = Choice { loader: None, ..c };
        assert!(injected(&phoenix(&m.exe), &plain, Some("127.0.0.1:1"), &m.cache, &[]).is_none());
    }

    #[test]
    fn uninstall_removes_the_cache() {
        let t = tempfile::tempdir().unwrap();
        assert!(!remove_cache(t.path()));
        std::fs::write(t.path().join(CACHE_FILE), b"{}").unwrap();
        assert!(remove_cache(t.path()));
        assert!(!t.path().join(CACHE_FILE).exists());
    }
}
