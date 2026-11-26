//! `phoecord theme import <vencord-theme.css> [--out <file>] [--name <name>]` (task 2.3.2): a
//! Vencord CSS theme to a PhoeCord `theme.toml`. The work is in `phoecord_loader::theme::import`.

use crate::tf;
use anyhow::{Context, Result};
use clap::Subcommand;
use phoecord_i18n::Localizer;
use phoecord_loader::theme::import::import;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ThemeCmd {
    /// Turn a Vencord CSS theme into a theme.toml.
    Import {
        css: PathBuf,
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
}

pub fn run(cmd: ThemeCmd, l10n: &Localizer) -> Result<()> {
    match cmd {
        ThemeCmd::Import { css, out, name } => {
            let text = std::fs::read_to_string(&css).with_context(|| {
                tf(
                    l10n,
                    "cli-theme-err-read",
                    &[("path", &css.display().to_string())],
                )
            })?;
            let r = import(&text);
            let name = name.unwrap_or_else(|| {
                css.file_stem()
                    .map(|s| s.to_string_lossy().trim_end_matches(".theme").to_string())
                    .unwrap_or_else(|| "Imported".into())
            });
            for line in &r.bad {
                eprintln!("{}", tf(l10n, "cli-theme-bad", &[("what", line)]));
            }
            if !r.unsupported.is_empty() {
                eprintln!(
                    "{}",
                    tf(
                        l10n,
                        "cli-theme-unsupported",
                        &[("list", &r.unsupported.join(", "))]
                    )
                );
            }
            if r.colors.is_empty() {
                anyhow::bail!(l10n.t("cli-theme-none"));
            }
            let toml = r.to_toml(&name);
            match out {
                Some(path) => {
                    std::fs::write(&path, &toml).with_context(|| {
                        tf(
                            l10n,
                            "cli-theme-err-write",
                            &[("path", &path.display().to_string())],
                        )
                    })?;
                    println!(
                        "{}",
                        tf(
                            l10n,
                            "cli-theme-written",
                            &[
                                ("path", &path.display().to_string()),
                                ("count", &r.colors.len().to_string()),
                            ],
                        )
                    );
                }
                None => print!("{toml}"),
            }
            Ok(())
        }
    }
}
