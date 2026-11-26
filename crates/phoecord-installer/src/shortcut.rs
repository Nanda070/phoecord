//! Install step: a menu shortcut that starts one Discord channel through the proxy.
//!
//! The client's own shortcut does not pass `--proxy-server`, so a proxy that runs at login is not
//! used by it. This shortcut runs `phoecord launch --channel <c>` instead (which reuses the running
//! proxy).
//!
//! * Windows: `<Start Menu>\Programs\Discord Canary (PhoeCord).lnk`, made through [`System`].
//! * Linux: `$XDG_DATA_HOME/applications/Discord Canary (PhoeCord).desktop`.
//! * macOS: skipped; `phoecord launch --channel <c>` is the way.
//!
//! The file is logged as a `File` step, so `uninstall` removes it like any other file.

use crate::Channel;
use crate::ca::create_dir_logged;
use crate::log::{InstallLog, Step};
use crate::system::{Env, Os, System};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// What `setup` did about the shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// No channel was chosen.
    NotRequested,
    /// The shortcut file now exists.
    Created { name: String, path: PathBuf },
    /// This OS has no simple shortcut; the CLI says what to run instead.
    Unsupported(Os),
}

/// `Discord Canary (PhoeCord)`.
pub fn name(channel: Channel) -> String {
    let client = match channel {
        Channel::Stable => "Discord",
        Channel::Ptb => "Discord PTB",
        Channel::Canary => "Discord Canary",
        // Its own name, so it is never taken for Discord's own Phoenix shortcut (2.2.8).
        Channel::Phoenix => "Discord Phoenix",
    };
    format!("{client} (PhoeCord)")
}

/// The arguments the shortcut passes: `launch --channel canary`.
pub fn arguments(channel: Channel) -> String {
    format!("launch --channel {}", channel.name())
}

/// Where the shortcut file goes, or `None` on an OS without one.
pub fn path(env: &Env, channel: Channel) -> Option<PathBuf> {
    let name = name(channel);
    match env.os {
        Os::Windows => Some(env.start_menu.join(format!("{name}.lnk"))),
        Os::Linux => Some(env.applications_dir.join(format!("{name}.desktop"))),
        Os::MacOs => None,
    }
}

pub(crate) fn desktop_text(exe: &Path, channel: Channel) -> String {
    let exec = exe
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "[Desktop Entry]\nType=Application\nName={}\n\
         Comment=Start Discord through the PhoeCord proxy\n\
         Exec=\"{exec}\" {}\nTerminal=false\nCategories=Network;InstantMessaging;\n",
        name(channel),
        arguments(channel)
    )
}

/// Create the shortcut for `channel` and log it. Safe to repeat: the file is rewritten, logged once.
pub fn install_shortcut(
    env: &Env,
    sys: &dyn System,
    log: &InstallLog,
    channel: Channel,
) -> Result<Outcome> {
    let Some(file) = path(env, channel) else {
        return Ok(Outcome::Unsupported(env.os));
    };
    let existed = file.exists();
    if let Some(dir) = file.parent() {
        create_dir_logged(log, dir)?;
    }
    match env.os {
        Os::Windows => sys.shortcut_create(
            &file,
            &env.proxy_exe,
            &arguments(channel),
            &format!("Start {} through PhoeCord", name(channel)),
        )?,
        _ => std::fs::write(&file, desktop_text(&env.proxy_exe, channel))
            .with_context(|| format!("writing {}", file.display()))?,
    }
    if !existed {
        log.record(Step::File { path: file.clone() })?;
    }
    Ok(Outcome::Created {
        name: name(channel),
        path: file,
    })
}

/// The PowerShell that creates a `.lnk` through the WScript.Shell COM object. Every value is a
/// single-quoted PowerShell string, so nothing in a path or argument is interpreted.
pub fn lnk_script(lnk: &Path, target: &Path, args: &str, description: &str) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); \
         $s.TargetPath = {}; $s.Arguments = {}; $s.Description = {}; \
         $s.WindowStyle = 7; $s.Save()",
        q(&lnk.display().to_string()),
        q(&target.display().to_string()),
        q(args),
        q(description),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::install_with;
    use crate::system::fake::{FakeSystem, env};
    use crate::uninstall::uninstall_all;

    #[test]
    fn names_follow_the_channel() {
        assert_eq!(name(Channel::Canary), "Discord Canary (PhoeCord)");
        assert_eq!(name(Channel::Stable), "Discord (PhoeCord)");
        assert_eq!(name(Channel::Ptb), "Discord PTB (PhoeCord)");
        assert_eq!(arguments(Channel::Ptb), "launch --channel ptb");
        assert_eq!(name(Channel::Phoenix), "Discord Phoenix (PhoeCord)");
        assert_eq!(arguments(Channel::Phoenix), "launch --channel phoenix");
    }

    #[test]
    fn phoenix_gets_its_own_shortcut_and_uninstall_removes_it() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        let r = install_with(&e, &sys, Some(Channel::Phoenix)).unwrap();
        let lnk = e.start_menu.join("Discord Phoenix (PhoeCord).lnk");
        assert_eq!(
            r.shortcut,
            Outcome::Created {
                name: "Discord Phoenix (PhoeCord)".into(),
                path: lnk.clone()
            }
        );
        assert!(
            sys.calls().contains(&format!(
                "shortcut_create {} {} launch --channel phoenix",
                lnk.display(),
                e.proxy_exe.display()
            )),
            "{:?}",
            sys.calls()
        );
        // Discord's own shortcuts are never touched: the only one made is ours.
        let made: Vec<_> = sys
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("shortcut_create"))
            .collect();
        assert_eq!(made.len(), 1);
        let u = uninstall_all(&e, &sys).unwrap();
        assert!(u.is_clean(), "{:?}", u.failed);
        assert!(!lnk.exists());
    }

    #[test]
    fn windows_asks_the_system_for_a_lnk_in_the_start_menu_and_logs_it() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        let r = install_with(&e, &sys, Some(Channel::Canary)).unwrap();
        let lnk = e.start_menu.join("Discord Canary (PhoeCord).lnk");
        assert_eq!(
            r.shortcut,
            Outcome::Created {
                name: "Discord Canary (PhoeCord)".into(),
                path: lnk.clone()
            }
        );
        assert!(
            sys.calls().contains(&format!(
                "shortcut_create {} {} launch --channel canary",
                lnk.display(),
                e.proxy_exe.display()
            )),
            "{:?}",
            sys.calls()
        );
        assert!(
            r.log
                .iter()
                .any(|x| x.step == Step::File { path: lnk.clone() })
        );
    }

    #[test]
    fn linux_writes_a_desktop_file_that_runs_launch() {
        let t = tempfile::tempdir().unwrap();
        let mut e = env(Os::Linux, t.path());
        e.proxy_exe = "/opt/phoecord/phoecord".into();
        let sys = FakeSystem::default();
        install_with(&e, &sys, Some(Channel::Stable)).unwrap();
        let f = e.applications_dir.join("Discord (PhoeCord).desktop");
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Name=Discord (PhoeCord)"), "{text}");
        assert!(
            text.contains("Exec=\"/opt/phoecord/phoecord\" launch --channel stable"),
            "{text}"
        );
        assert!(!sys.calls().iter().any(|c| c.starts_with("shortcut_create")));
    }

    #[test]
    fn macos_skips_it_and_says_so() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::MacOs, t.path());
        let r = install_with(&e, &FakeSystem::default(), Some(Channel::Canary)).unwrap();
        assert_eq!(r.shortcut, Outcome::Unsupported(Os::MacOs));
    }

    #[test]
    fn no_channel_means_no_shortcut() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        let r = install_with(&e, &sys, None).unwrap();
        assert_eq!(r.shortcut, Outcome::NotRequested);
        assert!(!sys.calls().iter().any(|c| c.starts_with("shortcut_create")));
    }

    #[test]
    fn repeating_setup_logs_the_shortcut_once() {
        let t = tempfile::tempdir().unwrap();
        let e = env(Os::Windows, t.path());
        let sys = FakeSystem::default();
        install_with(&e, &sys, Some(Channel::Canary)).unwrap();
        let again = install_with(&e, &sys, Some(Channel::Canary)).unwrap();
        assert_eq!(again.added, 0);
        let lnks = again
            .log
            .iter()
            .filter(|x| matches!(&x.step, Step::File { path } if path.extension().is_some_and(|e| e == "lnk")))
            .count();
        assert_eq!(lnks, 1);
    }

    #[test]
    fn uninstall_removes_the_shortcut_on_every_os() {
        for os in [Os::Windows, Os::Linux] {
            let t = tempfile::tempdir().unwrap();
            let e = env(os, t.path());
            let sys = FakeSystem::default();
            let r = install_with(&e, &sys, Some(Channel::Canary)).unwrap();
            let Outcome::Created { path, .. } = r.shortcut else {
                panic!("{os:?}")
            };
            assert!(path.is_file());
            let u = uninstall_all(&e, &sys).unwrap();
            assert!(u.is_clean(), "{os:?}: {:?}", u.failed);
            assert!(!path.exists(), "{os:?}: shortcut left");
            assert!(
                u.removed
                    .iter()
                    .any(|s| matches!(s, Step::File { path: p } if *p == path))
            );
        }
    }

    #[test]
    fn the_powershell_quotes_every_value() {
        let s = lnk_script(
            Path::new(r"C:\Users\O'Neil\Start Menu\Discord (PhoeCord).lnk"),
            Path::new(r"C:\Program Files\phoecord.exe"),
            "launch --channel canary",
            "Start $(evil) through PhoeCord",
        );
        assert!(s.contains(r"'C:\Users\O''Neil\Start Menu\Discord (PhoeCord).lnk'"));
        assert!(s.contains("$s.Arguments = 'launch --channel canary'"));
        assert!(s.contains("'Start $(evil) through PhoeCord'"));
    }
}
