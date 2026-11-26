//! Automatic updates from this repository's GitHub Releases (task 2.4.5, ruling Q4).
//!
//! Three moments, never in the middle of a session:
//!
//! 1. **Check** ([`Updater::check`], at most once a day, in the background of the installer window
//!    and of `phoecord run`): read the release list, take the newest release that is not a draft,
//!    not marked prerelease and has no `-` in its tag; if it is newer than this build, download the
//!    installer that matches how PhoeCord was installed ([`Kind`]), the release's `SHA256SUMS` and
//!    its minisign signature `SHA256SUMS.minisig`; the signature must verify with the public key
//!    compiled into this build ([`UPDATE_KEY`]) and its trusted comment must name the release's
//!    version ([`trusted_comment`]); the installer is kept in `<data>/updates/` only when its hash
//!    is the one listed. A build without a key, a missing or bad signature, a missing asset, a
//!    missing sums file, a file that is not listed in it or any mismatch is refused.
//! 2. **Apply** ([`Updater::ready`] then [`plan`], at the next start of the window or of
//!    `phoecord run`): the downloaded file is hashed again, then handed to the OS's own installer
//!    (Windows: the NSIS setup, silent; macOS: the `.app` is replaced from the `.dmg`; Linux: `dpkg`
//!    or `rpm` through `pkexec`, from the window only, since `phoecord run` has nobody to ask for the
//!    password).
//! 3. **Tell** ([`Updater::settle`], at every start): once the running build is the pending
//!    version, the update is recorded as applied, its file removed, and the window shows it once.
//!
//! The binaries have no publisher certificate, but `SHA256SUMS` is signed by the owner with an
//! offline minisign key (`scripts/sign-release.ps1`, audit finding A1 of 2.4.6): a compromised
//! GitHub account or release cannot ship an update without that key.
//!
//! Network access goes through [`Fetch`], so tests use a fake and never touch the network.

use crate::system::Os;
use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The release list of this repository (`[workspace.package] repository` in `Cargo.toml`).
/// `PHOECORD_UPDATE_URL` replaces it (a local mirror for manual tests; `http://` only to this
/// computer).
pub fn releases_url() -> String {
    if let Some(u) = std::env::var_os("PHOECORD_UPDATE_URL") {
        return u.to_string_lossy().into_owned();
    }
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let path = repo
        .strip_prefix("https://github.com/")
        .unwrap_or("nanda070/phoecord");
    format!("https://api.github.com/repos/{path}/releases?per_page=20")
}

/// The version of this build.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// The checksum file every release carries (`release.yml`, job `checksums`).
pub const SUMS: &str = "SHA256SUMS";
/// The owner's minisign signature of [`SUMS`], added to the draft release by
/// `scripts/sign-release.ps1` before it is published.
pub const SUMS_SIG: &str = "SHA256SUMS.minisig";
/// The minisign public key updates are checked with (`minisign.pub` format: a comment line, then
/// the key). A file with no key line makes every update refused ([`Refused::NoKey`]);
/// `release.yml` refuses to build a release from such a tree.
pub const UPDATE_KEY: &str = include_str!("../update-key.pub");
/// The trusted comment the owner signs `SHA256SUMS` of `version` with (`minisign -t`). Tied to the
/// version, so a signed list of one release cannot be reused under another tag.
pub fn trusted_comment(version: &Version) -> String {
    format!("phoecord {version}")
}
/// Folder in the data folder for downloads and [`State`].
pub const DIR: &str = "updates";
const STATE_FILE: &str = "state.json";
/// Seconds between two automatic checks.
pub const CHECK_EVERY: u64 = 24 * 60 * 60;
/// Size caps: the release list, the sums file, an installer.
const LIST_LIMIT: u64 = 4 << 20;
const SUMS_LIMIT: u64 = 1 << 20;
const SIG_LIMIT: u64 = 4 << 10;
const ASSET_LIMIT: u64 = 400 << 20;

/// A published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// The release page, for "what changed".
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// The releases in a GitHub `releases` API answer that may be offered: no drafts, nothing marked
/// prerelease, no tag with `-` (GOVERNANCE.md: such tags are prereleases), only `v<semver>` tags.
pub fn parse_releases(json: &[u8]) -> Result<Vec<Release>> {
    let list: Vec<ApiRelease> =
        serde_json::from_slice(json).context("the release list is not what GitHub sends")?;
    Ok(list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease && !r.tag_name.contains('-'))
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix('v')?).ok()?;
            Some(Release {
                version,
                tag: r.tag_name,
                page: r.html_url,
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| Asset {
                        name: a.name,
                        url: a.browser_download_url,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// The newest release newer than `current`, if any.
pub fn newest_after<'a>(releases: &'a [Release], current: &Version) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| r.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// How this copy of PhoeCord was installed, which decides the file an update needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Windows, from the NSIS setup: `uninstall.exe` next to the binaries.
    Nsis,
    /// macOS, the binaries inside `app` (`…/PhoeCord.app`).
    MacApp { app: PathBuf },
    /// Linux, from the `.deb` (dpkg knows the package `phoecord`).
    Deb,
    /// Linux, from the `.rpm` (in `/usr`, not known to dpkg).
    Rpm,
    /// A CLI archive, a build from source or anything else: told about updates, never updated.
    Portable,
}

/// The Debian and RPM package name (`Cargo.toml`, `[package.metadata.packager.deb]` and
/// `[package.metadata.generate-rpm]`).
pub const PACKAGE: &str = "phoecord";

/// Which [`Kind`] the binary at `exe` is. `exists` is the file system (a parameter for tests).
pub fn detect(os: Os, exe: &Path, exists: &dyn Fn(&Path) -> bool) -> Kind {
    let Some(dir) = exe.parent() else {
        return Kind::Portable;
    };
    match os {
        Os::Windows if exists(&dir.join("uninstall.exe")) => Kind::Nsis,
        Os::MacOs => {
            // `<app>/Contents/MacOS/<binary>`
            let app = dir.parent().and_then(Path::parent);
            match app {
                Some(a)
                    if dir.file_name().is_some_and(|n| n == "MacOS")
                        && a.extension().is_some_and(|e| e == "app") =>
                {
                    Kind::MacApp {
                        app: a.to_path_buf(),
                    }
                }
                _ => Kind::Portable,
            }
        }
        Os::Linux if dir == Path::new("/usr/bin") => {
            if exists(Path::new(&format!("/var/lib/dpkg/info/{PACKAGE}.list"))) {
                Kind::Deb
            } else {
                Kind::Rpm
            }
        }
        _ => Kind::Portable,
    }
}

/// The release file `kind` updates from (names from `scripts/package/name-files.sh`); `arch` is
/// `std::env::consts::ARCH`. `None`: this kind is not updated automatically.
pub fn asset_name(kind: &Kind, version: &Version, arch: &str) -> Option<String> {
    Some(match kind {
        Kind::Nsis => format!("PhoeCord_{version}_x64-setup.exe"),
        Kind::MacApp { .. } => {
            let a = if arch == "aarch64" { "aarch64" } else { "x64" };
            format!("PhoeCord_{version}_{a}.dmg")
        }
        Kind::Deb => format!("PhoeCord_{version}_amd64.deb"),
        Kind::Rpm => format!("PhoeCord_{version}_x86_64.rpm"),
        Kind::Portable => return None,
    })
}

/// `name → lower-case hex` from a `sha256sum` listing (`<hex>  <name>` or `<hex> *<name>`).
pub fn parse_sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (hash, name) = l.trim().split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            let ok = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
            (ok && !name.is_empty()).then(|| (name.to_string(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// Checks the owner's signature `sig` (a `.minisig` file) of `sums` with `key`, and that its trusted
/// comment is [`trusted_comment`] of `version`. Only prehashed (current minisign) signatures count.
pub fn verify_sums(
    key: &minisign_verify::PublicKey,
    sums: &[u8],
    sig: &[u8],
    version: &Version,
) -> Result<(), Refused> {
    let sig = std::str::from_utf8(sig).map_err(|_| Refused::BadSignature)?;
    let sig = minisign_verify::Signature::decode(sig).map_err(|_| Refused::BadSignature)?;
    key.verify(sums, &sig, false)
        .map_err(|_| Refused::BadSignature)?;
    let comment = sig.trusted_comment().trim_end();
    if comment != trusted_comment(version) {
        return Err(Refused::WrongRelease(comment.to_string()));
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    phoecord_proxy::trust::sha256_hex(bytes)
}

/// One HTTP GET of at most `limit` bytes.
pub trait Fetch {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>>;
}

/// The real network: HTTPS only (plain `http://` only to this computer, for a local mirror).
pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let local = is_loopback_http(url);
        if !local && !url.starts_with("https://") {
            bail!("not an https:// link: {url}");
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(!local)
            .max_redirects(if local { 0 } else { 5 })
            .timeout_global(Some(std::time::Duration::from_secs(300)))
            .user_agent(concat!("PhoeCord/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut resp = agent
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| anyhow!("{url}: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| anyhow!("{url}: {e}"))
    }
}

fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.contains('@')
        && (host.starts_with("127.0.0.1")
            || host.starts_with("[::1]")
            || host == "localhost"
            || host.starts_with("localhost:"))
}

/// What [`Updater`] remembers between runs (`<data>/updates/state.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Unix seconds of the last check that reached GitHub (or tried to).
    #[serde(default)]
    pub last_check: Option<u64>,
    /// A verified installer waiting for the next start.
    #[serde(default)]
    pub pending: Option<Pending>,
    /// The last update that was installed; `shown` once the window told the user.
    #[serde(default)]
    pub applied: Option<Applied>,
    /// Why the last check or install failed, for the window and `phoecord upgrade`.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub version: String,
    /// The version that downloaded it.
    #[serde(default)]
    pub from: String,
    /// File name inside `<data>/updates/`.
    pub file: String,
    pub sha256: String,
    pub page: String,
    /// Starts that tried to install it. After [`MAX_ATTEMPTS`] it is no longer tried on its own,
    /// so a setup that keeps failing cannot keep the window from opening; `phoecord upgrade`
    /// resets it.
    #[serde(default)]
    pub attempts: u32,
}

/// How often one downloaded update is tried before it waits for `phoecord upgrade`.
pub const MAX_ATTEMPTS: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub from: String,
    pub to: String,
    pub page: String,
    #[serde(default)]
    pub shown: bool,
}

/// The result of [`Updater::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Checked less than [`CHECK_EVERY`] ago.
    NotDue,
    UpToDate,
    /// Downloaded and verified; installed at the next start.
    Ready(Pending),
    /// A newer release exists, but this kind of install is not updated automatically.
    Manual {
        version: String,
        page: String,
    },
}

/// Why a release was refused. Kept apart from network errors so the text can say "refused".
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// This build carries no usable [`UPDATE_KEY`].
    NoKey,
    NoAsset(String),
    NoSums,
    NoSignature,
    /// The signature is malformed, made with another key, or does not match `SHA256SUMS`.
    BadSignature,
    /// The signature is valid but its trusted comment names another release.
    WrongRelease(String),
    NotListed(String),
    Mismatch(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NoKey => write!(f, "this build has no update signing key"),
            Refused::NoSignature => write!(f, "the release has no {SUMS_SIG}"),
            Refused::BadSignature => write!(f, "{SUMS_SIG} does not verify with PhoeCord's key"),
            Refused::WrongRelease(c) => {
                write!(f, "{SUMS_SIG} was made for another release ({c:?})")
            }
            Refused::NoAsset(n) => write!(f, "the release has no {n}"),
            Refused::NoSums => write!(f, "the release has no {SUMS}"),
            Refused::NotListed(n) => write!(f, "{n} is not listed in {SUMS}"),
            Refused::Mismatch(n) => write!(f, "{n} does not match its line in {SUMS}"),
        }
    }
}

impl std::error::Error for Refused {}

/// Checks, downloads and remembers updates for one install.
pub struct Updater<'a> {
    /// `<data>/updates`.
    pub dir: PathBuf,
    pub current: Version,
    pub kind: Kind,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    pub url: String,
    /// `minisign.pub` text; [`UPDATE_KEY`] outside tests.
    pub key: String,
    pub fetch: &'a dyn Fetch,
}

impl<'a> Updater<'a> {
    /// The updater of this build, with its data in `data_dir`.
    pub fn system(data_dir: &Path, exe: &Path, fetch: &'a dyn Fetch) -> Updater<'a> {
        Updater {
            dir: data_dir.join(DIR),
            current: current_version(),
            kind: detect(Os::current(), exe, &|p| p.exists()),
            arch: std::env::consts::ARCH.to_string(),
            url: releases_url(),
            key: UPDATE_KEY.to_string(),
            fetch,
        }
    }

    pub fn state(&self) -> State {
        std::fs::read(self.dir.join(STATE_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, s: &State) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let tmp = self.dir.join(format!("{STATE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
        std::fs::rename(&tmp, self.dir.join(STATE_FILE))?;
        Ok(())
    }

    /// Look for a newer release and download it. `now` is Unix seconds; `force` ignores
    /// [`CHECK_EVERY`]. The error text is also kept in [`State::error`].
    pub fn check(&self, now: u64, force: bool) -> Result<Checked> {
        let mut s = self.state();
        if !force
            && s.last_check
                .is_some_and(|t| now.saturating_sub(t) < CHECK_EVERY)
        {
            return Ok(match s.pending {
                Some(p) if self.is_newer(&p.version) => Checked::Ready(p),
                _ => Checked::NotDue,
            });
        }
        // Recorded before the request, so a failing network is not asked again on every start.
        s.last_check = Some(now);
        self.save(&s)?;
        if force && let Some(p) = &mut s.pending {
            // Asked for by hand: a download that failed to install gets its tries back.
            p.attempts = 0;
        }
        let r = self.check_now(&mut s);
        s.error = match &r {
            Err(e) => Some(format!("{e:#}")),
            // "did not install after N tries" stays until `phoecord upgrade` resets the tries.
            Ok(_)
                if s.pending
                    .as_ref()
                    .is_some_and(|p| p.attempts >= MAX_ATTEMPTS) =>
            {
                s.error.take()
            }
            Ok(_) => None,
        };
        self.save(&s)?;
        r
    }

    fn check_now(&self, s: &mut State) -> Result<Checked> {
        let list = self.fetch.get(&self.url, LIST_LIMIT)?;
        let releases = parse_releases(&list)?;
        let Some(rel) = newest_after(&releases, &self.current) else {
            return Ok(Checked::UpToDate);
        };
        let version = rel.version.to_string();
        let Some(name) = asset_name(&self.kind, &rel.version, &self.arch) else {
            return Ok(Checked::Manual {
                version,
                page: rel.page.clone(),
            });
        };
        if let Some(p) = &s.pending
            && p.version == version
            && self.file_ok(p)
        {
            return Ok(Checked::Ready(p.clone()));
        }
        let asset = rel
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| Refused::NoAsset(name.clone()))?;
        let sums = rel
            .assets
            .iter()
            .find(|a| a.name == SUMS)
            .ok_or(Refused::NoSums)?;
        let sig = rel
            .assets
            .iter()
            .find(|a| a.name == SUMS_SIG)
            .ok_or(Refused::NoSignature)?;
        let key = minisign_verify::PublicKey::decode(&self.key).map_err(|_| Refused::NoKey)?;
        let sums = self.fetch.get(&sums.url, SUMS_LIMIT)?;
        let sig = self.fetch.get(&sig.url, SIG_LIMIT)?;
        verify_sums(&key, &sums, &sig, &rel.version)?;
        let sums = String::from_utf8_lossy(&sums).into_owned();
        let expected = parse_sums(&sums)
            .remove(&name)
            .ok_or_else(|| Refused::NotListed(name.clone()))?;
        let bytes = self.fetch.get(&asset.url, ASSET_LIMIT)?;
        if sha256_hex(&bytes) != expected {
            return Err(Refused::Mismatch(name).into());
        }
        // An older download is replaced by this one.
        if let Some(old) = s.pending.take() {
            let _ = std::fs::remove_file(self.dir.join(&old.file));
        }
        let tmp = self.dir.join(format!("{name}.part"));
        std::fs::write(&tmp, &bytes).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, self.dir.join(&name))?;
        let p = Pending {
            version,
            from: self.current.to_string(),
            file: name,
            sha256: expected,
            page: rel.page.clone(),
            attempts: 0,
        };
        s.pending = Some(p.clone());
        Ok(Checked::Ready(p))
    }

    fn is_newer(&self, version: &str) -> bool {
        Version::parse(version).is_ok_and(|v| v > self.current)
    }

    fn file_ok(&self, p: &Pending) -> bool {
        std::fs::read(self.dir.join(&p.file)).is_ok_and(|b| sha256_hex(&b) == p.sha256)
    }

    /// The downloaded installer to apply now, hashed again; a file that changed on disk is
    /// deleted and forgotten.
    pub fn ready(&self) -> Result<Option<(Pending, PathBuf)>> {
        let mut s = self.state();
        let Some(p) = s.pending.clone() else {
            return Ok(None);
        };
        if !self.is_newer(&p.version) || p.attempts >= MAX_ATTEMPTS {
            return Ok(None); // settled by [`Self::settle`], or waiting for `phoecord upgrade`
        }
        if self.file_ok(&p) {
            let path = self.dir.join(&p.file);
            return Ok(Some((p, path)));
        }
        let _ = std::fs::remove_file(self.dir.join(&p.file));
        s.pending = None;
        s.error = Some(Refused::Mismatch(p.file).to_string());
        self.save(&s)?;
        Ok(None)
    }

    /// At every start: when this build is (at least) the pending version, the update went in:
    /// remember it as applied and remove the installer. Returns an applied update not yet shown.
    pub fn settle(&self) -> Result<Option<Applied>> {
        let mut s = self.state();
        if let Some(p) = s.pending.clone()
            && !self.is_newer(&p.version)
        {
            let _ = std::fs::remove_file(self.dir.join(&p.file));
            s.pending = None;
            s.error = None;
            s.applied = Some(Applied {
                from: p.from.clone(),
                to: p.version.clone(),
                page: p.page,
                shown: false,
            });
            self.save(&s)?;
        }
        Ok(s.applied.filter(|a| !a.shown))
    }

    /// Count one try to install the pending update; call it right before [`execute`].
    pub fn begin_apply(&self) -> Result<()> {
        let mut s = self.state();
        if let Some(p) = &mut s.pending {
            p.attempts += 1;
            if p.attempts >= MAX_ATTEMPTS {
                s.error = Some(format!(
                    "PhoeCord {} did not install after {} tries; run `phoecord upgrade` to try again, or install it by hand",
                    p.version, p.attempts
                ));
            }
        }
        self.save(&s)
    }

    /// The window told the user about the applied update.
    pub fn mark_shown(&self) -> Result<()> {
        let mut s = self.state();
        if let Some(a) = &mut s.applied {
            a.shown = true;
        }
        self.save(&s)
    }

    /// Keep why an install did not go through, for the window and `phoecord upgrade`.
    pub fn record_error(&self, why: &str) -> Result<()> {
        let mut s = self.state();
        s.error = Some(why.to_string());
        self.save(&s)
    }
}

/// Who applies the update: the installer window or `phoecord run` (autostart).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Window,
    Proxy,
}

/// One step of applying an update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start `program` and let it run on after this process exits; the caller exits next.
    Spawn { program: PathBuf, args: Vec<String> },
    /// Run `program` and wait; a non-zero exit fails the update.
    Run { program: PathBuf, args: Vec<String> },
    /// macOS: replace the bundle `app` by the one in `dmg`.
    ReplaceApp { dmg: PathBuf, app: PathBuf },
    /// Stop PhoeCord's proxies so the autostart service starts the new binary.
    RestartProxy,
    /// Start this window again (the new build) and exit.
    Relaunch { program: PathBuf },
}

/// The NSIS switches PhoeCord's template understands (`assets/installer/nsis/installer.nsi`):
/// `/S` silent, `/R` start the window afterwards, `/LAUNCHPROXY` start `phoecord run` afterwards
/// (no flag of its own may start with `P`, `R` or `S`: NSIS `GetOptions` matches prefixes).
pub const NSIS_SILENT: &str = "/S";
pub const NSIS_RELAUNCH: &str = "/R";
pub const NSIS_RUN_PROXY: &str = "/LAUNCHPROXY";

/// How to install `file` from `who`. `window` is the installer window binary; `autostart` says
/// whether the proxy starts at login (so it is started again after the update). `None`: this
/// install cannot be updated from here (Linux packages from `phoecord run`: no one to type the
/// password), the window does it.
pub fn plan(
    kind: &Kind,
    file: &Path,
    who: Who,
    window: &Path,
    autostart: bool,
) -> Option<Vec<Action>> {
    let spawn = |args: &[&str]| Action::Spawn {
        program: file.to_path_buf(),
        args: args.iter().map(|a| a.to_string()).collect(),
    };
    Some(match (kind, who) {
        // The setup stops PhoeCord's proxies (`phoecord stop`) and the window before it copies
        // anything (ruling Q7): the files of a running program are locked on Windows.
        (Kind::Nsis, Who::Window) if autostart => {
            vec![spawn(&[NSIS_SILENT, NSIS_RELAUNCH, NSIS_RUN_PROXY])]
        }
        (Kind::Nsis, Who::Window) => vec![spawn(&[NSIS_SILENT, NSIS_RELAUNCH])],
        (Kind::Nsis, Who::Proxy) => vec![spawn(&[NSIS_SILENT, NSIS_RUN_PROXY])],
        // macOS and Linux replace the files of a running program without trouble; the proxy is
        // stopped after the new files are in place, so launchd / systemd start the new binary.
        (Kind::MacApp { app }, Who::Window) => vec![
            Action::ReplaceApp {
                dmg: file.to_path_buf(),
                app: app.clone(),
            },
            Action::RestartProxy,
            Action::Relaunch {
                program: window.to_path_buf(),
            },
        ],
        // `phoecord run` is the LaunchAgent with KeepAlive: it exits after this and launchd
        // starts the new binary.
        (Kind::MacApp { app }, Who::Proxy) => vec![Action::ReplaceApp {
            dmg: file.to_path_buf(),
            app: app.clone(),
        }],
        (Kind::Deb | Kind::Rpm, Who::Window) => {
            let file = file.display().to_string();
            let args = if *kind == Kind::Deb {
                vec!["dpkg".to_string(), "-i".into(), file]
            } else {
                vec!["rpm".to_string(), "-U".into(), "--replacepkgs".into(), file]
            };
            vec![
                Action::Run {
                    program: PathBuf::from("pkexec"),
                    args,
                },
                Action::RestartProxy,
                Action::Relaunch {
                    program: window.to_path_buf(),
                },
            ]
        }
        (Kind::Deb | Kind::Rpm, Who::Proxy) | (Kind::Portable, _) => return None,
    })
}

/// Carry out [`plan`]'s actions. `restart_proxy` stops PhoeCord's proxies (the autostart service
/// starts them again). Returns `true` when the caller must exit now (a spawned installer or a
/// relaunched window takes over).
pub fn execute(actions: &[Action], restart_proxy: &dyn Fn() -> Result<()>) -> Result<bool> {
    let mut exit = false;
    for a in actions {
        match a {
            Action::Spawn { program, args } => {
                Command::new(program)
                    .args(args)
                    .spawn()
                    .with_context(|| format!("starting {}", program.display()))?;
                exit = true;
            }
            Action::Run { program, args } => {
                let st = Command::new(program)
                    .args(args)
                    .status()
                    .with_context(|| format!("starting {}", program.display()))?;
                if !st.success() {
                    bail!("{} {} failed: {st}", program.display(), args.join(" "));
                }
            }
            Action::ReplaceApp { dmg, app } => replace_app(dmg, app)?,
            Action::RestartProxy => {
                if let Err(e) = restart_proxy() {
                    tracing::warn!("the proxy was not restarted after the update: {e:#}");
                }
            }
            Action::Relaunch { program } => {
                Command::new(program)
                    .spawn()
                    .with_context(|| format!("starting {}", program.display()))?;
                exit = true;
            }
        }
    }
    Ok(exit)
}

/// macOS: mount `dmg` read-only, copy its `.app` next to `app`, swap the two, unmount.
fn replace_app(dmg: &Path, app: &Path) -> Result<()> {
    let parent = app.parent().context("the app has no parent folder")?;
    let name = app.file_name().context("the app has no name")?;
    let mount = std::env::temp_dir().join(format!("phoecord-update-{}", std::process::id()));
    std::fs::create_dir_all(&mount)?;
    let ok = |c: &mut Command| -> Result<()> {
        let st = c.status().context("running a macOS tool")?;
        if !st.success() {
            bail!("{c:?} failed: {st}");
        }
        Ok(())
    };
    ok(Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(dmg))?;
    let new = parent.join(format!("{}.new", name.to_string_lossy()));
    let old = parent.join(format!("{}.old", name.to_string_lossy()));
    let _ = std::fs::remove_dir_all(&new);
    let _ = std::fs::remove_dir_all(&old);
    let copied = ok(Command::new("ditto").arg(mount.join(name)).arg(&new));
    let _ = Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mount)
        .status();
    let _ = std::fs::remove_dir(&mount);
    copied?;
    std::fs::rename(app, &old).with_context(|| format!("moving {} aside", app.display()))?;
    if let Err(e) = std::fs::rename(&new, app) {
        let _ = std::fs::rename(&old, app);
        return Err(e).with_context(|| format!("putting the new {} in place", app.display()));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Is automatic updating on? `[update] auto` in `config.toml`, default on (ruling Q5).
pub fn auto_enabled(extra: &toml::Table) -> bool {
    extra
        .get("update")
        .and_then(|u| u.get("auto"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true)
}

/// Set `[update] auto` in the config's extra keys.
pub fn set_auto(extra: &mut toml::Table, on: bool) {
    let t = extra
        .entry("update")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(t) = t.as_table_mut() {
        t.insert("auto".into(), on.into());
    }
}

/// Unix seconds now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Answers from a map of URL → body; records every URL asked for.
    #[derive(Default)]
    struct FakeFetch {
        bodies: HashMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Fetch for FakeFetch {
        fn get(&self, url: &str, _limit: u64) -> Result<Vec<u8>> {
            self.asked.borrow_mut().push(url.to_string());
            self.bodies
                .get(url)
                .cloned()
                .ok_or_else(|| anyhow!("no route to {url}"))
        }
    }

    const LIST: &str = "https://api.test/releases";

    fn release(tag: &str, draft: bool, pre: bool, assets: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": pre,
            "html_url": format!("https://example.test/releases/{tag}"),
            "assets": assets.iter().map(|a| serde_json::json!({
                "name": a,
                "browser_download_url": format!("https://dl.test/{tag}/{a}"),
            })).collect::<Vec<_>>(),
        })
    }

    /// A minisign key made the way `minisign -G` makes one, from a fixed seed.
    struct TestKey {
        sk: ed25519_dalek::SigningKey,
        id: [u8; 8],
    }

    impl TestKey {
        fn new(seed: u8) -> TestKey {
            TestKey {
                sk: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]),
                id: [seed; 8],
            }
        }

        fn b64(bytes: &[u8]) -> String {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }

        /// `minisign.pub`.
        fn public(&self) -> String {
            let mut bin = b"Ed".to_vec();
            bin.extend_from_slice(&self.id);
            bin.extend_from_slice(self.sk.verifying_key().as_bytes());
            format!(
                "untrusted comment: minisign public key\n{}\n",
                Self::b64(&bin)
            )
        }

        /// `minisign -S -m <file> -t <comment>`: a prehashed signature (BLAKE2b-512 of the file).
        fn sign(&self, data: &[u8], comment: &str) -> Vec<u8> {
            use blake2::Digest as _;
            use ed25519_dalek::Signer as _;
            let hash = blake2::Blake2b512::digest(data);
            let sig = self.sk.sign(&hash).to_bytes();
            let mut bin = b"ED".to_vec();
            bin.extend_from_slice(&self.id);
            bin.extend_from_slice(&sig);
            let mut global = sig.to_vec();
            global.extend_from_slice(comment.as_bytes());
            let global = self.sk.sign(&global).to_bytes();
            format!(
                "untrusted comment: signature from minisign secret key\n{}\ntrusted comment: {comment}\n{}\n",
                Self::b64(&bin),
                Self::b64(&global)
            )
            .into_bytes()
        }
    }

    /// The key the fake releases are signed with.
    fn owner() -> TestKey {
        TestKey::new(7)
    }

    /// Puts `text` as release 0.6.0's SHA256SUMS, signed by `owner()` for 0.6.0.
    fn put_sums(f: &mut FakeFetch, text: &str) {
        let sig = owner().sign(text.as_bytes(), "phoecord 0.6.0");
        f.bodies
            .insert(format!("https://dl.test/v0.6.0/{SUMS}"), text.into());
        f.bodies
            .insert(format!("https://dl.test/v0.6.0/{SUMS_SIG}"), sig);
    }

    /// A fake GitHub with release 0.6.0 carrying the Windows setup, SHA256SUMS and its signature.
    fn github(setup: &[u8], listed: Option<&[u8]>) -> FakeFetch {
        let name = "PhoeCord_0.6.0_x64-setup.exe";
        let list = serde_json::json!([
            release("v0.7.0-rc.1", false, true, &[name]),
            release("v0.6.0", false, false, &[name, SUMS, SUMS_SIG]),
            release("v0.5.0", false, false, &[]),
        ]);
        let mut f = FakeFetch::default();
        f.bodies.insert(LIST.into(), list.to_string().into_bytes());
        f.bodies
            .insert(format!("https://dl.test/v0.6.0/{name}"), setup.to_vec());
        let sums = format!(
            "{}  {name}\n{}  PhoeCord_0.6.0_amd64.deb\n",
            sha256_hex(listed.unwrap_or(setup)),
            "0".repeat(64)
        );
        put_sums(&mut f, &sums);
        f
    }

    fn updater<'a>(dir: &Path, kind: Kind, fetch: &'a FakeFetch) -> Updater<'a> {
        Updater {
            dir: dir.join(DIR),
            current: Version::new(0, 5, 0),
            kind,
            arch: "x86_64".into(),
            url: LIST.into(),
            key: owner().public(),
            fetch,
        }
    }

    fn refused(e: anyhow::Error) -> Refused {
        e.downcast::<Refused>().expect("a refusal")
    }

    #[test]
    fn only_a_release_signed_with_the_compiled_key_is_downloaded() {
        let t = tempfile::tempdir().unwrap();
        let setup_url = "https://dl.test/v0.6.0/PhoeCord_0.6.0_x64-setup.exe";

        // No signature file in the release.
        let mut f = github(b"setup", None);
        let name = "PhoeCord_0.6.0_x64-setup.exe";
        let list = serde_json::json!([release("v0.6.0", false, false, &[name, SUMS])]);
        f.bodies.insert(LIST.into(), list.to_string().into_bytes());
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(refused(e), Refused::NoSignature);

        // Signed with another key (a stolen GitHub account with its own key).
        let mut f = github(b"setup", None);
        let sums = f.bodies[&format!("https://dl.test/v0.6.0/{SUMS}")].clone();
        f.bodies.insert(
            format!("https://dl.test/v0.6.0/{SUMS_SIG}"),
            TestKey::new(9).sign(&sums, "phoecord 0.6.0"),
        );
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(refused(e), Refused::BadSignature);
        assert!(!f.asked.borrow().iter().any(|u| u == setup_url));

        // SHA256SUMS changed after signing (an installer swapped together with its line).
        let mut f = github(b"evil", None);
        let good = format!("{}  {name}\n", sha256_hex(b"setup"));
        let sig = owner().sign(good.as_bytes(), "phoecord 0.6.0");
        f.bodies
            .insert(format!("https://dl.test/v0.6.0/{SUMS_SIG}"), sig);
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(refused(e), Refused::BadSignature);

        // A valid signature of another release's list.
        let mut f = github(b"setup", None);
        let sums = f.bodies[&format!("https://dl.test/v0.6.0/{SUMS}")].clone();
        f.bodies.insert(
            format!("https://dl.test/v0.6.0/{SUMS_SIG}"),
            owner().sign(&sums, "phoecord 0.5.0"),
        );
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(refused(e), Refused::WrongRelease("phoecord 0.5.0".into()));

        // Garbage in place of the signature.
        let mut f = github(b"setup", None);
        f.bodies.insert(
            format!("https://dl.test/v0.6.0/{SUMS_SIG}"),
            b"not a signature".to_vec(),
        );
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(refused(e), Refused::BadSignature);

        // A build without a key refuses every update.
        let f = github(b"setup", None);
        let mut u = updater(t.path(), Kind::Nsis, &f);
        u.key = UPDATE_KEY.lines().next().unwrap_or_default().to_string();
        let e = u.check(1, true).unwrap_err();
        assert_eq!(refused(e), Refused::NoKey);

        // Nothing was kept by any of them.
        assert!(updater(t.path(), Kind::Nsis, &f).state().pending.is_none());
        assert!(!t.path().join(DIR).join(name).exists());

        // The owner's signature for this release passes.
        let f = github(b"setup", None);
        assert!(matches!(
            updater(t.path(), Kind::Nsis, &f).check(1, true).unwrap(),
            Checked::Ready(_)
        ));
    }

    #[test]
    fn the_compiled_key_file_is_a_key_or_a_comment_only() {
        // `release.yml` refuses to release with the comment-only file; until then it must parse
        // as "no key", not panic.
        let mut lines = UPDATE_KEY.lines().filter(|l| !l.trim().is_empty());
        assert!(lines.next().unwrap().starts_with("untrusted comment:"));
        if lines.next().is_some() {
            minisign_verify::PublicKey::decode(UPDATE_KEY)
                .expect("update-key.pub is a minisign key");
        }
    }

    #[test]
    fn drafts_prereleases_and_dash_tags_are_never_offered() {
        let list = serde_json::json!([
            release("v9.0.0", true, false, &[]),
            release("v8.0.0", false, true, &[]),
            release("v7.0.0-beta", false, false, &[]),
            release("not-a-version", false, false, &[]),
            release("v0.6.0", false, false, &[]),
        ]);
        let r = parse_releases(list.to_string().as_bytes()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].version, Version::new(0, 6, 0));
        assert_eq!(
            newest_after(&r, &Version::new(0, 5, 0)).unwrap().tag,
            "v0.6.0"
        );
        assert!(newest_after(&r, &Version::new(0, 6, 0)).is_none());
    }

    #[test]
    fn the_install_kind_follows_where_the_binary_is() {
        let none = |_: &Path| false;
        let all = |_: &Path| true;
        let w = Path::new(r"C:\Users\u\AppData\Local\PhoeCord\phoecord.exe");
        assert_eq!(detect(Os::Windows, w, &all), Kind::Nsis);
        assert_eq!(detect(Os::Windows, w, &none), Kind::Portable);
        let m = Path::new("/Applications/PhoeCord.app/Contents/MacOS/phoecord");
        assert_eq!(
            detect(Os::MacOs, m, &none),
            Kind::MacApp {
                app: PathBuf::from("/Applications/PhoeCord.app")
            }
        );
        assert_eq!(
            detect(Os::MacOs, Path::new("/usr/local/bin/phoecord"), &none),
            Kind::Portable
        );
        let l = Path::new("/usr/bin/phoecord");
        let dpkg = |p: &Path| p == Path::new("/var/lib/dpkg/info/phoecord.list");
        assert_eq!(detect(Os::Linux, l, &dpkg), Kind::Deb);
        assert_eq!(detect(Os::Linux, l, &none), Kind::Rpm);
        assert_eq!(
            detect(Os::Linux, Path::new("/home/u/phoecord/phoecord"), &all),
            Kind::Portable
        );
    }

    #[test]
    fn asset_names_match_the_release_files() {
        let v = Version::new(1, 0, 0);
        let app = Kind::MacApp {
            app: PathBuf::from("/Applications/PhoeCord.app"),
        };
        assert_eq!(
            asset_name(&Kind::Nsis, &v, "x86_64").unwrap(),
            "PhoeCord_1.0.0_x64-setup.exe"
        );
        assert_eq!(
            asset_name(&app, &v, "aarch64").unwrap(),
            "PhoeCord_1.0.0_aarch64.dmg"
        );
        assert_eq!(
            asset_name(&app, &v, "x86_64").unwrap(),
            "PhoeCord_1.0.0_x64.dmg"
        );
        assert_eq!(
            asset_name(&Kind::Deb, &v, "x86_64").unwrap(),
            "PhoeCord_1.0.0_amd64.deb"
        );
        assert_eq!(
            asset_name(&Kind::Rpm, &v, "x86_64").unwrap(),
            "PhoeCord_1.0.0_x86_64.rpm"
        );
        assert_eq!(asset_name(&Kind::Portable, &v, "x86_64"), None);
    }

    #[test]
    fn sums_are_read_in_both_sha256sum_styles() {
        let h = "A".repeat(64);
        let s = parse_sums(&format!(
            "{h}  one.exe\n{h} *two.dmg\nbroken line\nabc  short\n"
        ));
        assert_eq!(s.len(), 2);
        assert_eq!(s["one.exe"], "a".repeat(64));
        assert!(s.contains_key("two.dmg"));
    }

    #[test]
    fn a_verified_download_is_kept_and_offered_at_the_next_start() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"new setup", None);
        let u = updater(t.path(), Kind::Nsis, &f);
        let Checked::Ready(p) = u.check(1_000, false).unwrap() else {
            panic!("expected a ready update")
        };
        assert_eq!(p.version, "0.6.0");
        let (again, path) = u.ready().unwrap().unwrap();
        assert_eq!(again, p);
        assert_eq!(std::fs::read(path).unwrap(), b"new setup");
        // The prerelease 0.7.0-rc.1 was never downloaded.
        assert!(f.asked.borrow().iter().all(|u| !u.contains("rc")));
    }

    #[test]
    fn a_mismatch_is_refused_and_nothing_is_kept() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"tampered", Some(b"what was published"));
        let u = updater(t.path(), Kind::Nsis, &f);
        let e = u.check(1_000, false).unwrap_err();
        assert_eq!(
            e.downcast_ref::<Refused>(),
            Some(&Refused::Mismatch("PhoeCord_0.6.0_x64-setup.exe".into()))
        );
        assert!(u.ready().unwrap().is_none());
        assert!(
            !t.path()
                .join(DIR)
                .join("PhoeCord_0.6.0_x64-setup.exe")
                .exists()
        );
        assert!(u.state().error.unwrap().contains("does not match"));
    }

    #[test]
    fn a_release_without_the_asset_or_the_sums_is_refused() {
        let t = tempfile::tempdir().unwrap();
        // The .dmg is not in the release.
        let f = github(b"x", None);
        let mac = Kind::MacApp {
            app: PathBuf::from("/Applications/PhoeCord.app"),
        };
        let e = updater(t.path(), mac, &f).check(1, true).unwrap_err();
        assert!(matches!(
            e.downcast_ref::<Refused>(),
            Some(Refused::NoAsset(_))
        ));
        // The .deb is listed in SHA256SUMS but not attached.
        let e = updater(t.path(), Kind::Deb, &f).check(1, true).unwrap_err();
        assert!(matches!(
            e.downcast_ref::<Refused>(),
            Some(Refused::NoAsset(_))
        ));

        // No SHA256SUMS at all.
        let mut f = github(b"x", None);
        let list = serde_json::json!([release(
            "v0.6.0",
            false,
            false,
            &["PhoeCord_0.6.0_x64-setup.exe"]
        )]);
        f.bodies.insert(LIST.into(), list.to_string().into_bytes());
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert_eq!(e.downcast_ref::<Refused>(), Some(&Refused::NoSums));

        // SHA256SUMS that does not list the file.
        let mut f = github(b"x", None);
        put_sums(&mut f, &format!("{}  other.exe\n", "0".repeat(64)));
        let e = updater(t.path(), Kind::Nsis, &f)
            .check(1, true)
            .unwrap_err();
        assert!(matches!(
            e.downcast_ref::<Refused>(),
            Some(Refused::NotListed(_))
        ));
    }

    #[test]
    fn checks_run_once_a_day_unless_forced() {
        let t = tempfile::tempdir().unwrap();
        let f = FakeFetch::default(); // every request fails
        let u = updater(t.path(), Kind::Nsis, &f);
        assert!(u.check(10_000, false).is_err());
        assert_eq!(
            u.check(10_000 + CHECK_EVERY - 1, false).unwrap(),
            Checked::NotDue
        );
        assert_eq!(f.asked.borrow().len(), 1);
        assert!(u.check(10_000 + CHECK_EVERY, false).is_err());
        assert!(u.check(10_000 + CHECK_EVERY, true).is_err());
        assert_eq!(f.asked.borrow().len(), 3);
    }

    #[test]
    fn up_to_date_and_manual_installs() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"x", None);
        let mut u = updater(t.path(), Kind::Nsis, &f);
        u.current = Version::new(0, 6, 0);
        assert_eq!(u.check(1, true).unwrap(), Checked::UpToDate);
        let u = updater(t.path(), Kind::Portable, &f);
        assert_eq!(
            u.check(1, true).unwrap(),
            Checked::Manual {
                version: "0.6.0".into(),
                page: "https://example.test/releases/v0.6.0".into()
            }
        );
        // Nothing but the list was fetched for a portable copy.
        assert_eq!(f.asked.borrow().len(), 2);
    }

    #[test]
    fn a_file_changed_on_disk_is_not_applied() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"setup", None);
        let u = updater(t.path(), Kind::Nsis, &f);
        u.check(1, false).unwrap();
        std::fs::write(
            t.path().join(DIR).join("PhoeCord_0.6.0_x64-setup.exe"),
            b"evil",
        )
        .unwrap();
        assert!(u.ready().unwrap().is_none());
        assert!(u.state().pending.is_none());
        assert!(
            !t.path()
                .join(DIR)
                .join("PhoeCord_0.6.0_x64-setup.exe")
                .exists()
        );
    }

    #[test]
    fn a_failing_install_is_tried_twice_then_waits_for_upgrade() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"setup", None);
        let u = updater(t.path(), Kind::Nsis, &f);
        u.check(1, false).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            assert!(u.ready().unwrap().is_some());
            u.begin_apply().unwrap(); // ...and the setup did not install it
        }
        assert!(u.ready().unwrap().is_none(), "the window must open");
        assert!(u.state().error.unwrap().contains("phoecord upgrade"));
        // `phoecord upgrade` (a forced check) gives it its tries back, without a new download.
        assert!(matches!(u.check(2, true).unwrap(), Checked::Ready(_)));
        assert!(u.ready().unwrap().is_some());
    }

    #[test]
    fn after_the_update_the_user_is_told_once() {
        let t = tempfile::tempdir().unwrap();
        let f = github(b"setup", None);
        let u = updater(t.path(), Kind::Nsis, &f);
        u.check(1, false).unwrap();
        assert_eq!(
            u.settle().unwrap(),
            None,
            "0.5.0 still runs: nothing applied yet"
        );
        // The next start runs 0.6.0.
        let mut u = updater(t.path(), Kind::Nsis, &f);
        u.current = Version::new(0, 6, 0);
        assert!(u.ready().unwrap().is_none());
        let a = u.settle().unwrap().unwrap();
        assert_eq!((a.from.as_str(), a.to.as_str()), ("0.5.0", "0.6.0"));
        assert!(
            !t.path()
                .join(DIR)
                .join("PhoeCord_0.6.0_x64-setup.exe")
                .exists()
        );
        assert_eq!(u.settle().unwrap(), Some(a), "still unshown");
        u.mark_shown().unwrap();
        assert_eq!(u.settle().unwrap(), None);
    }

    #[test]
    fn plans_per_os_and_caller() {
        let file = Path::new("/d/updates/f");
        let win = Path::new("/app/phoecord-installer");
        let args = |a: &[Action]| match &a[0] {
            Action::Spawn { args, .. } => args.clone(),
            other => panic!("{other:?}"),
        };
        let w = plan(&Kind::Nsis, file, Who::Window, win, true).unwrap();
        assert_eq!(args(&w), ["/S", "/R", "/LAUNCHPROXY"]);
        let w = plan(&Kind::Nsis, file, Who::Window, win, false).unwrap();
        assert_eq!(args(&w), ["/S", "/R"]);
        let p = plan(&Kind::Nsis, file, Who::Proxy, win, true).unwrap();
        assert_eq!(args(&p), ["/S", "/LAUNCHPROXY"]);

        let app = Kind::MacApp {
            app: PathBuf::from("/Applications/PhoeCord.app"),
        };
        let m = plan(&app, file, Who::Window, win, true).unwrap();
        assert!(matches!(m[0], Action::ReplaceApp { .. }));
        assert_eq!(
            m[1],
            Action::RestartProxy,
            "after the files, so launchd starts the new one"
        );
        let m = plan(&app, file, Who::Proxy, win, true).unwrap();
        assert_eq!(m.len(), 1);

        let d = plan(&Kind::Deb, file, Who::Window, win, true).unwrap();
        assert_eq!(
            d[0],
            Action::Run {
                program: "pkexec".into(),
                args: vec!["dpkg".into(), "-i".into(), file.display().to_string()]
            }
        );
        let r = plan(&Kind::Rpm, file, Who::Window, win, true).unwrap();
        assert!(matches!(&r[0], Action::Run { args, .. } if args[0] == "rpm"));
        assert_eq!(plan(&Kind::Deb, file, Who::Proxy, win, true), None);
        assert_eq!(plan(&Kind::Portable, file, Who::Window, win, true), None);
    }

    #[test]
    fn the_setting_defaults_to_on() {
        let mut t = toml::Table::new();
        assert!(auto_enabled(&t));
        set_auto(&mut t, false);
        assert!(!auto_enabled(&t));
        assert_eq!(t.to_string().trim(), "[update]\nauto = false");
        set_auto(&mut t, true);
        assert!(auto_enabled(&t));
    }

    #[test]
    fn the_default_list_is_this_repository() {
        if std::env::var_os("PHOECORD_UPDATE_URL").is_none() {
            assert_eq!(
                releases_url(),
                "https://api.github.com/repos/nanda070/phoecord/releases?per_page=20"
            );
        }
        assert!(is_loopback_http("http://127.0.0.1:8080/x"));
        assert!(!is_loopback_http("http://127.0.0.1@evil.test/"));
        assert!(!is_loopback_http("http://example.test/"));
    }
}
