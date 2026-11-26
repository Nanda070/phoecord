//! `phoecord signatures install <file or folder>` and `phoecord signatures list` (task 2.2.10):
//! the signature hot-fix path. The work is in `phoecord_installer::signatures`.

use crate::tf;
use anyhow::{Context, Result};
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_installer::log::InstallLog;
use phoecord_installer::signatures::{Installed, Present, install, list};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum SignaturesCmd {
    /// Check signature files and put them where the loader reads them. Windows only.
    Install { source: PathBuf },
    /// Show the signature files the loader would read. Windows only.
    List,
}

pub fn run(cmd: SignaturesCmd, l10n: &Localizer) -> Result<()> {
    // Only the Windows loader reads signatures from this folder.
    let target = cfg!(windows)
        .then(|| std::env::var_os("LOCALAPPDATA"))
        .and_then(phoecord_loader::signatures::default_dir)
        .with_context(|| l10n.t("cli-signatures-unsupported"))?;
    let folder = target.display().to_string();
    match cmd {
        SignaturesCmd::Install { source } => {
            let env = phoecord_installer::system::Env::system()?;
            let log = InstallLog::in_dir(&env.data_dir);
            let done = install(&log, &source, &target)?;
            for line in installed_lines(&done, l10n) {
                println!("{line}");
            }
            println!("{}", l10n.t("cli-signatures-restart"));
        }
        SignaturesCmd::List => {
            println!(
                "{}",
                tf(l10n, "cli-signatures-folder", &[("path", &folder)])
            );
            for line in listed_lines(&list(&target)?, l10n) {
                println!("{line}");
            }
        }
    }
    Ok(())
}

fn installed_lines(done: &[Installed], l10n: &Localizer) -> Vec<String> {
    let mut out = Vec::new();
    for file in done {
        let key = if file.replaced {
            "cli-signatures-replaced"
        } else {
            "cli-signatures-added"
        };
        out.push(tf(
            l10n,
            key,
            &[
                ("path", &file.path.display().to_string()),
                ("builds", &file.builds.join(", ")),
            ],
        ));
        for (build, by) in &file.overridden {
            out.push(tf(
                l10n,
                "cli-signatures-overridden",
                &[("build", build), ("file", by)],
            ));
        }
    }
    out
}

fn listed_lines(files: &[Present], l10n: &Localizer) -> Vec<String> {
    if files.is_empty() {
        return vec![l10n.t("cli-signatures-none")];
    }
    files
        .iter()
        .map(|f| match &f.builds {
            Ok(builds) => format!("  {}: {}", f.name(), builds.join(", ")),
            // Fluent drops leading spaces of a value, so the indent is added here.
            Err(why) => format!(
                "  {}",
                tf(
                    l10n,
                    "cli-signatures-broken",
                    &[("file", &f.name()), ("why", why)],
                )
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_is_said_and_an_empty_folder_too() {
        let l10n = Localizer::new("en");
        let done = [Installed {
            path: "a.toml".into(),
            replaced: true,
            builds: vec!["1.0.0.1".into()],
            overridden: vec![("1.0.0.1".into(), "z.toml".into())],
        }];
        let text = installed_lines(&done, &l10n).join("\n");
        for part in ["a.toml", "1.0.0.1", "z.toml"] {
            assert!(text.contains(part), "{part} not in {text}");
        }
        assert_eq!(listed_lines(&[], &l10n), [l10n.t("cli-signatures-none")]);
        let broken = Present {
            path: "b.toml".into(),
            builds: Err("bad".into()),
        };
        let line = &listed_lines(&[broken], &l10n)[0];
        assert!(line.starts_with("  b.toml"), "{line:?}");
    }
}
