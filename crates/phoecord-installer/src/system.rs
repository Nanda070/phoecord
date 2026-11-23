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
}
