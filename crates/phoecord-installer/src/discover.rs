//! Find installed Discord clients (Stable / PTB / Canary) on Windows, macOS and Linux.
//!
//! All path logic lives in [`discover_in`], which takes the folders to look in as [`Roots`], so tests
//! run it over a fake directory tree on any OS. [`discover`] fills `Roots` from the real machine.
//! Nothing here writes anything.

use crate::Channel;
use std::path::{Path, PathBuf};

/// How a client is packaged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Windows `%LOCALAPPDATA%\Discord*`, macOS `.app`, or a native Linux package found on `PATH`.
    Native,
    Flatpak,
    Snap,
}

impl Source {
    fn rank(self) -> u8 {
        match self {
            Source::Native => 0,
            Source::Flatpak => 1,
            Source::Snap => 2,
        }
    }
}

/// One installed client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub channel: Channel,
    pub source: Source,
    /// The folder the client lives in (`app-x.y.z`, the `.app` bundle, the package folder).
    pub root: PathBuf,
    /// The executable inside `root` (it may not exist for a Flatpak whose files are not exported).
    pub exe: PathBuf,
    pub version: Option<String>,
}

/// The folders discovery looks in. Every field may be empty.
#[derive(Clone, Debug, Default)]
pub struct Roots {
    /// Windows: `%LOCALAPPDATA%`, holding `Discord`, `DiscordPTB`, `DiscordCanary`.
    pub local_app_data: Option<PathBuf>,
    /// macOS: folders holding `Discord.app` and friends (`/Applications`, `~/Applications`).
    pub applications: Vec<PathBuf>,
    /// Linux: the directories of `PATH`.
    pub path_dirs: Vec<PathBuf>,
    /// Linux: Flatpak installation roots (`~/.local/share/flatpak`, `/var/lib/flatpak`).
    pub flatpak_roots: Vec<PathBuf>,
    /// Linux: Snap mount roots (`/snap`).
    pub snap_roots: Vec<PathBuf>,
}

impl Roots {
    /// The real machine's folders.
    pub fn system() -> Roots {
        let home = dirs::home_dir();
        let mut applications = vec![PathBuf::from("/Applications")];
        let mut flatpak_roots = vec![PathBuf::from("/var/lib/flatpak")];
        if let Some(h) = &home {
            applications.push(h.join("Applications"));
            flatpak_roots.insert(0, h.join(".local/share/flatpak"));
        }
        Roots {
            local_app_data: dirs::data_local_dir(),
            applications,
            path_dirs: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            flatpak_roots,
            snap_roots: vec![PathBuf::from("/snap")],
        }
    }
}

/// Every installed client on this machine.
pub fn discover() -> Vec<Client> {
    discover_in(&Roots::system())
}

/// Every client found under `roots`, in a stable order (channel, then source).
pub fn discover_in(roots: &Roots) -> Vec<Client> {
    let mut found = Vec::new();
    windows(roots, &mut found);
    macos(roots, &mut found);
    linux_path(roots, &mut found);
    flatpak(roots, &mut found);
    snap(roots, &mut found);
    found.sort_by_key(|c| (c.channel.name(), c.source.rank(), c.root.clone()));
    found.dedup_by(|a, b| a.channel == b.channel && a.root == b.root);
    found
}

/// Every installed client: the default folders plus `extra`, folders the user picked for a client
/// installed somewhere else (`clients` in `config.toml`).
pub fn discover_with(extra: &[PathBuf]) -> Vec<Client> {
    merge(discover(), extra)
}

/// `found` plus the clients in `extra`, one per real folder: a junction or symlink to a folder
/// that is already listed (say `%LOCALAPPDATA%\DiscordCanary` pointing at another drive, which is
/// also in `clients`) does not add a second tile.
fn merge(found: Vec<Client>, extra: &[PathBuf]) -> Vec<Client> {
    let mut out: Vec<Client> = Vec::new();
    let picked = extra.iter().flat_map(|dir| clients_at(dir));
    for c in found.into_iter().chain(picked) {
        if !is_listed(&out, &c) {
            out.push(c);
        }
    }
    out.sort_by_key(|c| (c.channel.name(), c.source.rank(), c.root.clone()));
    out
}

/// Whether `listed` already holds `client`: the same channel in the same real folder, after
/// links and junctions are followed.
pub fn is_listed(listed: &[Client], client: &Client) -> bool {
    let real = real_path(&client.root);
    listed
        .iter()
        .any(|c| c.channel == client.channel && real_path(&c.root) == real)
}

/// The folder with every link and junction followed, or the path as written when it cannot be
/// resolved (it was removed, or access is denied).
fn real_path(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The clients in a folder the user picked, empty when it holds none. Accepted on every OS, so a
/// folder copied from another machine is still recognised:
/// - Windows: the `Discord` / `DiscordPTB` / `DiscordCanary` folder, one `app-x.y.z` inside it, or a
///   folder holding them;
/// - macOS: a `Discord*.app` bundle or a folder holding one;
/// - Linux: the folder with the `discord` / `Discord` / `DiscordCanary` executable.
pub fn clients_at(dir: &Path) -> Vec<Client> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // One exact Windows build: `<channel folder>/app-x.y.z`.
    if let Some(version) = name.strip_prefix("app-") {
        let channel_dir = dir.parent().and_then(Path::file_name);
        let channel_dir = channel_dir.map(|n| n.to_string_lossy().into_owned());
        let Some((exe, channel)) = channel_dir.and_then(|d| windows_channel(&d)) else {
            return Vec::new();
        };
        if !dir.join(exe).is_file() {
            return Vec::new();
        }
        return vec![Client {
            channel,
            source: Source::Native,
            root: dir.to_path_buf(),
            exe: dir.join(exe),
            version: Some(version.to_string()),
        }];
    }
    // The folder holds the channel folders, the `.app` bundles or the Linux executable.
    let mut found = discover_in(&Roots {
        local_app_data: Some(dir.to_path_buf()),
        applications: vec![dir.to_path_buf()],
        path_dirs: vec![dir.to_path_buf()],
        ..Roots::default()
    });
    // The folder is a channel folder or a `.app` bundle itself: look from the folder above and
    // keep only what lives in it.
    if found.is_empty()
        && let Some(parent) = dir.parent()
    {
        found = discover_in(&Roots {
            local_app_data: Some(parent.to_path_buf()),
            applications: vec![parent.to_path_buf()],
            ..Roots::default()
        })
        .into_iter()
        .filter(|c| c.root.starts_with(dir))
        .collect();
    }
    found
}

/// The executable and channel of a Windows channel folder name.
fn windows_channel(dir: &str) -> Option<(&'static str, Channel)> {
    WINDOWS_CHANNELS
        .iter()
        .find(|(d, _, _)| *d == dir)
        .map(|(_, exe, ch)| (*exe, *ch))
}

const WINDOWS_CHANNELS: [(&str, &str, Channel); 3] = [
    ("Discord", "Discord.exe", Channel::Stable),
    ("DiscordPTB", "DiscordPTB.exe", Channel::Ptb),
    ("DiscordCanary", "DiscordCanary.exe", Channel::Canary),
];

/// `1.2.10` > `1.2.9`; anything that is not a number sorts lowest.
fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The newest `app-x.y.z` folder (with the client's executable in it) inside a Windows channel
/// folder such as `%LOCALAPPDATA%\DiscordCanary`, and its version. `None` when the folder name is
/// not a known channel folder or holds no complete build.
pub fn newest_app_dir(channel_dir: &Path) -> Option<(String, PathBuf)> {
    let name = channel_dir.file_name()?.to_string_lossy().into_owned();
    let (exe, _) = windows_channel(&name)?;
    newest_build(channel_dir, exe)
}

fn newest_build(channel_dir: &Path, exe: &str) -> Option<(String, PathBuf)> {
    std::fs::read_dir(channel_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ver = name.strip_prefix("app-")?.to_string();
            e.path().join(exe).is_file().then(|| (ver, e.path()))
        })
        .max_by_key(|(v, _)| version_key(v))
}

fn windows(roots: &Roots, out: &mut Vec<Client>) {
    let Some(local) = &roots.local_app_data else {
        return;
    };
    for (dir, exe, channel) in WINDOWS_CHANNELS {
        if let Some((version, root)) = newest_build(&local.join(dir), exe) {
            out.push(Client {
                channel,
                source: Source::Native,
                exe: root.join(exe),
                root,
                version: Some(version),
            });
        }
    }
}

fn macos(roots: &Roots, out: &mut Vec<Client>) {
    for base in &roots.applications {
        for (app, channel) in [
            ("Discord", Channel::Stable),
            ("Discord PTB", Channel::Ptb),
            ("Discord Canary", Channel::Canary),
        ] {
            let root = base.join(format!("{app}.app"));
            let exe = root.join("Contents/MacOS").join(app);
            if !exe.is_file() {
                continue;
            }
            let version = std::fs::read_to_string(root.join("Contents/Info.plist"))
                .ok()
                .and_then(|t| plist_version(&t));
            out.push(Client {
                channel,
                source: Source::Native,
                root,
                exe,
                version,
            });
        }
    }
}

/// `CFBundleShortVersionString` from an XML `Info.plist`.
fn plist_version(text: &str) -> Option<String> {
    let after = text.split("<key>CFBundleShortVersionString</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")?;
    Some(after[start..start + end].trim().to_string())
}

/// `version` from the `resources/build_info.json` that Linux builds of Discord ship.
fn build_info_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("resources/build_info.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

fn linux_path(roots: &Roots, out: &mut Vec<Client>) {
    for (names, channel) in [
        (&["discord", "Discord"][..], Channel::Stable),
        (&["discord-ptb", "DiscordPTB"][..], Channel::Ptb),
        (&["discord-canary", "DiscordCanary"][..], Channel::Canary),
    ] {
        for dir in &roots.path_dirs {
            let Some(found) = names.iter().map(|n| dir.join(n)).find(|p| p.is_file()) else {
                continue;
            };
            // `/usr/bin/discord` is usually a symlink into the package folder.
            let exe = std::fs::canonicalize(&found).unwrap_or(found);
            let root = exe.parent().map(Path::to_path_buf).unwrap_or_default();
            let version = build_info_version(&root);
            out.push(Client {
                channel,
                source: Source::Native,
                root,
                exe,
                version,
            });
            break;
        }
    }
}

fn flatpak(roots: &Roots, out: &mut Vec<Client>) {
    for base in &roots.flatpak_roots {
        for (id, channel) in [
            ("com.discordapp.Discord", Channel::Stable),
            ("com.discordapp.DiscordCanary", Channel::Canary),
        ] {
            let app = base.join("app").join(id);
            if !app.is_dir() {
                continue;
            }
            let files = app.join("current/active/files");
            // The bundled Discord sits in `discord` (Stable) or `discord-canary`.
            let root = ["discord", "discord-canary"]
                .iter()
                .map(|d| files.join(d))
                .find(|d| d.is_dir())
                .unwrap_or(files);
            let version = build_info_version(&root);
            let exe = root.join(if channel == Channel::Canary {
                "DiscordCanary"
            } else {
                "Discord"
            });
            out.push(Client {
                channel,
                source: Source::Flatpak,
                root,
                exe,
                version,
            });
        }
    }
}

fn snap(roots: &Roots, out: &mut Vec<Client>) {
    for base in &roots.snap_roots {
        let current = base.join("discord/current");
        if !current.is_dir() {
            continue;
        }
        let root = current.join("usr/share/discord");
        let version = build_info_version(&root).or_else(|| {
            let yaml = std::fs::read_to_string(current.join("meta/snap.yaml")).ok()?;
            yaml.lines()
                .find_map(|l| l.strip_prefix("version:"))
                .map(|v| v.trim().trim_matches(['\'', '"']).to_string())
        });
        out.push(Client {
            channel: Channel::Stable,
            source: Source::Snap,
            exe: root.join("Discord"),
            root,
            version,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn touch(p: &Path) {
        write(p, "");
    }

    #[test]
    fn windows_picks_newest_app_folder_per_channel() {
        let t = tempfile::tempdir().unwrap();
        let l = t.path();
        touch(&l.join("Discord/app-1.0.9/Discord.exe"));
        touch(&l.join("Discord/app-1.0.10/Discord.exe"));
        touch(&l.join("Discord/app-1.0.11/NotDiscord.exe")); // half-installed update: ignored
        touch(&l.join("DiscordCanary/app-1.0.500/DiscordCanary.exe"));
        touch(&l.join("DiscordPTB/Update.exe")); // no app folder: not usable
        let got = discover_in(&Roots {
            local_app_data: Some(l.to_path_buf()),
            ..Roots::default()
        });
        assert_eq!(got.len(), 2);
        let stable = got.iter().find(|c| c.channel == Channel::Stable).unwrap();
        assert_eq!(stable.version.as_deref(), Some("1.0.10"));
        assert!(
            stable
                .exe
                .ends_with(Path::new("app-1.0.10").join("Discord.exe"))
        );
        let canary = got.iter().find(|c| c.channel == Channel::Canary).unwrap();
        assert_eq!(canary.version.as_deref(), Some("1.0.500"));
    }

    #[test]
    fn macos_reads_the_bundle_version() {
        let t = tempfile::tempdir().unwrap();
        let apps = t.path();
        touch(&apps.join("Discord.app/Contents/MacOS/Discord"));
        write(
            &apps.join("Discord.app/Contents/Info.plist"),
            "<dict><key>CFBundleName</key><string>Discord</string>\n<key>CFBundleShortVersionString</key>\n<string>0.0.320</string></dict>",
        );
        touch(&apps.join("Discord PTB.app/Contents/MacOS/Discord PTB")); // no plist: version unknown
        let got = discover_in(&Roots {
            applications: vec![apps.to_path_buf(), t.path().join("missing")],
            ..Roots::default()
        });
        assert_eq!(got.len(), 2);
        let stable = got.iter().find(|c| c.channel == Channel::Stable).unwrap();
        assert_eq!(stable.version.as_deref(), Some("0.0.320"));
        let ptb = got.iter().find(|c| c.channel == Channel::Ptb).unwrap();
        assert_eq!(ptb.version, None);
    }

    #[test]
    fn linux_path_flatpak_and_snap() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        // PATH: a plain binary in a package folder with build_info.json.
        touch(&r.join("bin/discord-canary"));
        write(
            &r.join("bin/resources/build_info.json"),
            r#"{"version":"0.0.999"}"#,
        );
        // Flatpak (user installation).
        let fp = r.join("flatpak/app/com.discordapp.Discord/current/active/files/discord");
        write(
            &fp.join("resources/build_info.json"),
            r#"{"version":"0.0.70"}"#,
        );
        // Snap: version only in snap.yaml.
        write(
            &r.join("snap/discord/current/meta/snap.yaml"),
            "name: discord\nversion: '0.0.65'\n",
        );
        let got = discover_in(&Roots {
            path_dirs: vec![r.join("nothing"), r.join("bin")],
            flatpak_roots: vec![r.join("flatpak")],
            snap_roots: vec![r.join("snap")],
            ..Roots::default()
        });
        let by = |s: Source| got.iter().find(|c| c.source == s).unwrap();
        assert_eq!(by(Source::Native).channel, Channel::Canary);
        assert_eq!(by(Source::Native).version.as_deref(), Some("0.0.999"));
        assert_eq!(by(Source::Flatpak).version.as_deref(), Some("0.0.70"));
        assert_eq!(by(Source::Snap).version.as_deref(), Some("0.0.65"));
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn a_picked_folder_finds_its_client() {
        let t = tempfile::tempdir().unwrap();
        // Windows layout in a custom place.
        let win = t.path().join("Games/DiscordCanary");
        touch(&win.join("app-1.0.1355/DiscordCanary.exe"));
        touch(&win.join("app-1.0.1360/DiscordCanary.exe"));
        let c = clients_at(&win);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].channel, Channel::Canary);
        assert_eq!(c[0].version.as_deref(), Some("1.0.1360"));
        // Its parent folder, and one exact build.
        assert_eq!(clients_at(&t.path().join("Games")).len(), 1);
        let one = clients_at(&win.join("app-1.0.1355"));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].version.as_deref(), Some("1.0.1355"));
        // A macOS bundle, picked directly.
        let app = t.path().join("Apps/Discord PTB.app");
        touch(&app.join("Contents/MacOS/Discord PTB"));
        let c = clients_at(&app);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].channel, Channel::Ptb);
        // A Linux folder with the executable.
        let lin = t.path().join("opt/discord");
        touch(&lin.join("Discord"));
        assert_eq!(clients_at(&lin)[0].channel, Channel::Stable);
        // Anything else holds no client.
        let empty = t.path().join("Documents");
        fs::create_dir_all(&empty).unwrap();
        assert!(clients_at(&empty).is_empty());
        assert!(clients_at(&t.path().join("missing")).is_empty());
    }

    #[test]
    fn a_linked_folder_is_listed_once() {
        let t = tempfile::tempdir().unwrap();
        // Join part by part: `mklink` below rejects a path with `/` in it.
        let real = t.path().join("D").join("DiscordCanary");
        touch(&real.join("app-1.0.1360/DiscordCanary.exe"));
        let local = t.path().join("Local");
        fs::create_dir_all(&local).unwrap();
        let link = local.join("DiscordCanary");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link)
            .or_else(|_| {
                // A symlink needs Developer Mode; a junction does not.
                std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J"])
                    .arg(&link)
                    .arg(&real)
                    .status()
                    .and_then(|s| {
                        if s.success() {
                            Ok(())
                        } else {
                            Err(std::io::Error::other(format!("mklink /J failed: {s}")))
                        }
                    })
            })
            .unwrap();
        let found = discover_in(&Roots {
            local_app_data: Some(local.clone()),
            ..Roots::default()
        });
        assert_eq!(found.len(), 1);
        // The real folder is also in `clients`, written two ways: still one tile.
        let got = merge(found, &[real.clone(), link.clone()]);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(is_listed(&got, &clients_at(&real)[0]));
        // A different folder with the same channel is a second client.
        let other = t.path().join("E/DiscordCanary");
        touch(&other.join("app-1.0.1361/DiscordCanary.exe"));
        assert!(!is_listed(&got, &clients_at(&other)[0]));
        assert_eq!(merge(got, &[other]).len(), 2);
    }

    #[test]
    fn empty_machine_finds_nothing() {
        assert!(discover_in(&Roots::default()).is_empty());
        let t = tempfile::tempdir().unwrap();
        let p = t.path().to_path_buf();
        let got = discover_in(&Roots {
            local_app_data: Some(p.clone()),
            applications: vec![p.clone()],
            path_dirs: vec![p.clone()],
            flatpak_roots: vec![p.clone()],
            snap_roots: vec![p],
        });
        assert!(got.is_empty());
    }
}
