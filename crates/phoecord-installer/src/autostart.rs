//! Install step: start the proxy at login, the way each OS does it for one user (no admin rights).
//!
//! * Windows: a value in `HKCU\...\CurrentVersion\Run`.
//! * macOS: a LaunchAgent plist in `~/Library/LaunchAgents`.
//! * Linux: a systemd user unit in `$XDG_CONFIG_HOME/systemd/user`.

use crate::ca::create_dir_logged;
use crate::log::{InstallLog, Step};
use crate::system::{Env, Os, System};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const RUN_KEY_NAME: &str = "PhoeCord";
pub const LAUNCH_AGENT_LABEL: &str = "dev.phoecord.proxy";
pub const SYSTEMD_UNIT: &str = "phoecord.service";

/// Where the LaunchAgent plist goes.
pub fn plist_path(env: &Env) -> PathBuf {
    env.home
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// Where the systemd user unit goes.
pub fn unit_path(env: &Env) -> PathBuf {
    env.config_home.join("systemd/user").join(SYSTEMD_UNIT)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn plist_text(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{LAUNCH_AGENT_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>run</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<true/>\n\
         </dict>\n</plist>\n",
        xml_escape(&exe.display().to_string())
    )
}

pub(crate) fn unit_text(exe: &Path) -> String {
    format!(
        "[Unit]\nDescription=PhoeCord local proxy\n\n\
         [Service]\nExecStart=\"{}\" run\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n",
        exe.display()
    )
}

/// Write `text` to `path` and log the file if it did not exist before.
fn write_logged(log: &InstallLog, path: &Path, text: &str) -> Result<()> {
    let existed = path.exists();
    if let Some(dir) = path.parent() {
        create_dir_logged(log, dir)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    if !existed {
        log.record(Step::File {
            path: path.to_path_buf(),
        })?;
    }
    Ok(())
}

/// Register autostart for `env.os`. Safe to repeat: files are rewritten, nothing is logged twice, and
/// the OS registration is only made the first time.
pub fn install_autostart(env: &Env, sys: &dyn System, log: &InstallLog) -> Result<()> {
    match env.os {
        Os::Windows => {
            let value = format!("\"{}\" run", env.proxy_exe.display());
            let step = Step::RunKey {
                name: RUN_KEY_NAME.into(),
                value: value.clone(),
            };
            if !log.contains(&step)? {
                sys.run_key_set(RUN_KEY_NAME, &value)?;
                log.record(step)?;
            }
        }
        Os::MacOs => {
            let plist = plist_path(env);
            write_logged(log, &plist, &plist_text(&env.proxy_exe))?;
            let step = Step::LaunchAgent {
                label: LAUNCH_AGENT_LABEL.into(),
                plist: plist.clone(),
            };
            if !log.contains(&step)? {
                sys.launchctl_load(&plist)?;
                log.record(step)?;
            }
        }
        Os::Linux => {
            write_logged(log, &unit_path(env), &unit_text(&env.proxy_exe))?;
            let step = Step::SystemdUnit {
                unit: SYSTEMD_UNIT.into(),
            };
            if !log.contains(&step)? {
                sys.systemd_enable(SYSTEMD_UNIT)?;
                log.record(step)?;
            }
        }
    }
    Ok(())
}
