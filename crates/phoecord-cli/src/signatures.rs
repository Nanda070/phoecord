//! `phoecord signatures install <file or folder>` and `phoecord signatures list` (task 2.2.10):
//! the signature hot-fix path. The work is in `phoecord_installer::signatures`.

use crate::tf;
use anyhow::{Context, Result};
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_installer::log::InstallLog;
use phoecord_installer::signatures::{Installed, Present, install, list};
use phoecord_loader::signatures::{BUILTIN, SignatureSet};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum SignaturesCmd {
    /// Check signature files and put them where the loader reads them. Windows only.
    Install { source: PathBuf },
    /// Show the compiled-in tables and the hot-fix files the loader would read. Windows only.
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
            let builtin = SignatureSet::parse(BUILTIN).context("compiled-in signatures")?;
            for line in listed_lines(&builtin, &list(&target)?, l10n) {
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

/// The compiled-in tables first, each marked as replaced when a hot-fix file has the same build
/// key (the loader takes the hot-fix one), then the hot-fix files.
fn listed_lines(builtin: &SignatureSet, files: &[Present], l10n: &Localizer) -> Vec<String> {
    let mut out = Vec::new();
    for (build, count) in builtin.builds() {
        let count = count.to_string();
        // The loader reads the files in path order and the last table of a key wins.
        let winner = files.iter().rfind(|f| {
            f.builds
                .as_ref()
                .is_ok_and(|b| b.iter().any(|k| k == build))
        });
        let line = match winner {
            Some(w) => tf(
                l10n,
                "cli-signatures-builtin-replaced",
                &[("build", build), ("count", &count), ("file", &w.name())],
            ),
            None => tf(
                l10n,
                "cli-signatures-builtin",
                &[("build", build), ("count", &count)],
            ),
        };
        out.push(format!("  {line}"));
    }
    if files.is_empty() {
        out.push(format!("  {}", l10n.t("cli-signatures-none")));
    }
    for f in files {
        // Fluent drops leading spaces of a value, so the indent is added here.
        out.push(match &f.builds {
            Ok(builds) => format!(
                "  {}",
                tf(
                    l10n,
                    "cli-signatures-hotfix",
                    &[("file", &f.name()), ("builds", &builds.join(", "))],
                )
            ),
            Err(why) => format!(
                "  {}",
                tf(
                    l10n,
                    "cli-signatures-broken",
                    &[("file", &f.name()), ("why", why)],
                )
            ),
        });
    }
    out
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
        let builtin = SignatureSet::parse(
            "[build.\"1.0.0.1\"]
a = { aob = \"90\" }
[build.\"1.0.0.2\"]
a = { aob = \"90\" }
b = { aob = \"91\" }
",
        )
        .unwrap();
        let none = listed_lines(&builtin, &[], &l10n).join(
            "
",
        );
        assert!(
            none.contains("1.0.0.1") && none.contains("1.0.0.2"),
            "{none}"
        );
        assert!(none.contains(&l10n.t("cli-signatures-none")), "{none}");
        let broken = Present {
            path: "b.toml".into(),
            builds: Err("bad".into()),
        };
        let line = &listed_lines(&builtin, &[broken], &l10n)[2];
        assert!(line.starts_with("  b.toml"), "{line:?}");
    }

    #[test]
    fn a_hotfix_file_is_said_to_replace_the_builtin_build() {
        let l10n = Localizer::new("en");
        let builtin = SignatureSet::parse(
            "[build.\"1.0.0.1\"]
a = { aob = \"90\" }
[build.\"1.0.0.2\"]
a = { aob = \"90\" }
",
        )
        .unwrap();
        let fix = Present {
            path: "fix.toml".into(),
            builds: Ok(vec!["1.0.0.2".into()]),
        };
        let lines = listed_lines(&builtin, &[fix], &l10n);
        assert!(!lines[0].contains("fix.toml"), "{lines:?}");
        assert!(
            lines[1].contains("1.0.0.2") && lines[1].contains("fix.toml"),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("fix.toml") && lines[2].contains("1.0.0.2"),
            "{lines:?}"
        );
    }
}
