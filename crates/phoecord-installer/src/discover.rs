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

/// `1.2.10` > `1.2.9`; anything that is not a number sorts lowest.
fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn windows(roots: &Roots, out: &mut Vec<Client>) {
    let Some(local) = &roots.local_app_data else {
        return;
    };
    for (dir, exe, channel) in [
        ("Discord", "Discord.exe", Channel::Stable),
        ("DiscordPTB", "DiscordPTB.exe", Channel::Ptb),
        ("DiscordCanary", "DiscordCanary.exe", Channel::Canary),
    ] {
        let Ok(rd) = std::fs::read_dir(local.join(dir)) else {
            continue;
        };
        let newest = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let ver = name.strip_prefix("app-")?.to_string();
                e.path().join(exe).is_file().then(|| (ver, e.path()))
            })
            .max_by_key(|(v, _)| version_key(v));
        if let Some((version, root)) = newest {
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
