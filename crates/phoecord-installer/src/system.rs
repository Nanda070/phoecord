//! The part of the machine the installer cannot undo by deleting files: trust stores, the Windows
//! registry, launchd and systemd. Everything goes through [`System`] so tests use a fake and never
//! touch the real machine; [`RealSystem`] is the only implementation that does.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Which OS's rules to follow. A parameter (not a `cfg`) so every OS's code runs in tests everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Where the installer works. [`Env::system`] resolves the real folders.
#[derive(Clone, Debug)]
pub struct Env {
    pub os: Os,
    /// PhoeCord's data folder (`ca.pem`, `ca.key`, `install.log`).
    pub data_dir: PathBuf,
    /// The user's home folder (macOS LaunchAgents live under it).
    pub home: PathBuf,
    /// The user's config folder (`$XDG_CONFIG_HOME`; Linux systemd user units live under it).
    pub config_home: PathBuf,
    /// The `phoecord` binary that autostart runs as `phoecord run`.
    pub proxy_exe: PathBuf,
}

impl Env {
    pub fn system() -> Result<Env> {
        let home = dirs::home_dir().context("cannot find the home folder")?;
        Ok(Env {
            os: Os::current(),
            data_dir: phoecord_proxy::config::Paths::resolve().data,
            config_home: dirs::config_dir().unwrap_or_else(|| home.join(".config")),
            home,
            proxy_exe: std::env::current_exe().context("cannot find the phoecord binary")?,
        })
    }
}

/// Everything that changes state outside plain files.
pub trait System {
    /// Add the CA in `ca_pem` to the OS trust stores.
    fn trust_ca(&self, ca_pem: &Path) -> Result<()>;
    fn untrust_ca(&self) -> Result<()>;
    /// Windows: set `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\<name>`.
    fn run_key_set(&self, name: &str, value: &str) -> Result<()>;
    fn run_key_delete(&self, name: &str) -> Result<()>;
    /// macOS: `launchctl load -w <plist>`.
    fn launchctl_load(&self, plist: &Path) -> Result<()>;
    fn launchctl_unload(&self, plist: &Path) -> Result<()>;
    /// Linux: `systemctl --user enable --now <unit>`.
    fn systemd_enable(&self, unit: &str) -> Result<()>;
    fn systemd_disable(&self, unit: &str) -> Result<()>;
    /// Stop the PhoeCord proxy processes (`<exe> run` or `<exe> launch ...`) of the binaries in
    /// `exes`, except the calling process. Returns the process ids stopped; none running is `Ok([])`.
    fn stop_proxies(&self, exes: &[PathBuf]) -> Result<Vec<u32>>;
}

/// The process ids in `listing` (lines of pid, a tab or space, command line) that run one of `exes` as a proxy:
/// the command is `<exe> run` or `<exe> launch ...`, possibly after global options. `own` is skipped.
pub fn select_proxy_pids(listing: &str, exes: &[PathBuf], os: Os, own: u32) -> Vec<u32> {
    let norm = |s: &str| {
        let s = s.trim().trim_matches('"').to_string();
        if os == Os::Windows {
            s.to_lowercase().replace('/', "\\")
        } else {
            s
        }
    };
    let exes: Vec<String> = exes
        .iter()
        .map(|e| norm(&e.display().to_string()))
        .collect();
    let mut pids = Vec::new();
    for line in listing.lines() {
        let Some((pid, cmd)) = line.trim().split_once(|c: char| c.is_whitespace()) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let cmd = cmd.trim_start();
        // The command line starts with the exe, quoted or not; paths may hold spaces.
        let (prog, rest) = if let Some(q) = cmd.strip_prefix('"') {
            match q.split_once('"') {
                Some((p, r)) => (norm(p), r.to_string()),
                None => continue,
            }
        } else {
            let n = norm(cmd);
            match exes.iter().find(|e| n.starts_with(e.as_str())) {
                Some(e) => (e.clone(), n[e.len()..].to_string()),
                None => continue,
            }
        };
        if pid == own || !exes.contains(&prog) {
            continue;
        }
        let mut words = rest.split_whitespace();
        let mut first = words.next();
        while first.is_some_and(|w| w == "--lang") {
            words.next();
            first = words.next();
        }
        if matches!(first, Some("run" | "launch")) {
            pids.push(pid);
        }
    }
    pids
}

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn run(cmd: &mut Command) -> Result<()> {
    let what = format!("{cmd:?}");
    let out = cmd.output().with_context(|| format!("running {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// The real machine. Never constructed by tests.
pub struct RealSystem;

impl System for RealSystem {
    fn trust_ca(&self, ca_pem: &Path) -> Result<()> {
        phoecord_proxy::trust::install(ca_pem)
    }

    fn untrust_ca(&self) -> Result<()> {
        phoecord_proxy::trust::uninstall()
    }

    fn run_key_set(&self, name: &str, value: &str) -> Result<()> {
        run(Command::new("reg").args([
            "add", RUN_KEY, "/v", name, "/t", "REG_SZ", "/d", value, "/f",
        ]))
    }

    fn run_key_delete(&self, name: &str) -> Result<()> {
        run(Command::new("reg").args(["delete", RUN_KEY, "/v", name, "/f"]))
    }

    fn launchctl_load(&self, plist: &Path) -> Result<()> {
        run(Command::new("launchctl").args(["load", "-w"]).arg(plist))
    }

    fn launchctl_unload(&self, plist: &Path) -> Result<()> {
        run(Command::new("launchctl").args(["unload", "-w"]).arg(plist))
    }

    fn systemd_enable(&self, unit: &str) -> Result<()> {
        run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
        run(Command::new("systemctl").args(["--user", "enable", "--now", unit]))
    }

    fn systemd_disable(&self, unit: &str) -> Result<()> {
        run(Command::new("systemctl").args(["--user", "disable", "--now", unit]))
    }

    fn stop_proxies(&self, exes: &[PathBuf]) -> Result<Vec<u32>> {
        let os = Os::current();
        let out = if os == Os::Windows {
            Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "Get-CimInstance Win32_Process -Filter \"Name='phoecord.exe'\" |                      ForEach-Object { \"$($_.ProcessId)`t$($_.CommandLine)\" }",
                ])
                .output()
        } else {
            Command::new("ps").args(["-eo", "pid=,args="]).output()
        }
        .context("listing processes")?;
        let listing = String::from_utf8_lossy(&out.stdout);
        let pids = select_proxy_pids(&listing, exes, os, std::process::id());
        for pid in &pids {
            if os == Os::Windows {
                run(Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]))?;
            } else {
                run(Command::new("kill").arg(pid.to_string()))?;
            }
        }
        Ok(pids)
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::cell::RefCell;

    /// Records calls instead of changing anything; can be told to fail one operation.
    #[derive(Default)]
    pub struct FakeSystem {
        pub calls: RefCell<Vec<String>>,
        pub fail_on: RefCell<Option<String>>,
        /// Process ids `stop_proxies` finds running (and stops).
        pub running: RefCell<Vec<u32>>,
    }

    impl FakeSystem {
        fn hit(&self, call: String) -> Result<()> {
            if self.fail_on.borrow().as_deref() == Some(call.split(' ').next().unwrap()) {
                bail!("fake failure: {call}");
            }
            self.calls.borrow_mut().push(call);
            Ok(())
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl System for FakeSystem {
        fn trust_ca(&self, ca_pem: &Path) -> Result<()> {
            self.hit(format!("trust_ca {}", ca_pem.display()))
        }
        fn untrust_ca(&self) -> Result<()> {
            self.hit("untrust_ca".into())
        }
        fn run_key_set(&self, name: &str, value: &str) -> Result<()> {
            self.hit(format!("run_key_set {name} {value}"))
        }
        fn run_key_delete(&self, name: &str) -> Result<()> {
            self.hit(format!("run_key_delete {name}"))
        }
        fn launchctl_load(&self, plist: &Path) -> Result<()> {
            self.hit(format!("launchctl_load {}", plist.display()))
        }
        fn launchctl_unload(&self, plist: &Path) -> Result<()> {
            self.hit(format!("launchctl_unload {}", plist.display()))
        }
        fn systemd_enable(&self, unit: &str) -> Result<()> {
            self.hit(format!("systemd_enable {unit}"))
        }
        fn systemd_disable(&self, unit: &str) -> Result<()> {
            self.hit(format!("systemd_disable {unit}"))
        }
        fn stop_proxies(&self, exes: &[PathBuf]) -> Result<Vec<u32>> {
            let list: Vec<_> = exes.iter().map(|e| e.display().to_string()).collect();
            self.hit(format!("stop_proxies {}", list.join(",")))?;
            Ok(std::mem::take(&mut *self.running.borrow_mut()))
        }
    }

    /// An `Env` rooted in a temporary folder.
    pub fn env(os: Os, root: &Path) -> Env {
        Env {
            os,
            data_dir: root.join("data/phoecord"),
            home: root.join("home"),
            config_home: root.join("home/.config"),
            proxy_exe: root.join("bin/phoecord"),
        }
    }

    #[test]
    fn selects_only_proxy_processes_of_our_binaries() {
        let exes = [
            PathBuf::from("C:/Users/a b/phoecord.exe"),
            PathBuf::from("D:/x/phoecord.exe"),
        ];
        let listing = [
            r#"10	"C:\Users\a b\phoecord.exe" run"#,
            r"11	C:\Users\a b\phoecord.exe launch --channel canary",
            r#"12	"c:\users\a b\PHOECORD.EXE" --lang ru run"#,
            r#"13	"C:\Users\a b\phoecord.exe" uninstall"#,
            r#"14	"E:\other\phoecord.exe" run"#,
            r#"15	"D:\x\phoecord.exe" run"#,
            r#"99	"D:\x\phoecord.exe" run"#,
        ]
        .join("\n");
        assert_eq!(
            select_proxy_pids(&listing, &exes, Os::Windows, 99),
            vec![10, 11, 12, 15]
        );
        let ps = " 7 /home/u/bin/phoecord run\n 8 /home/u/bin/phoecord plugins\n 9 /usr/bin/phoecord run\n";
        assert_eq!(
            select_proxy_pids(ps, &[PathBuf::from("/home/u/bin/phoecord")], Os::Linux, 1),
            vec![7]
        );
    }
}
