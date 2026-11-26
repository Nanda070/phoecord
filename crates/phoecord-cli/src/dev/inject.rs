//! `phoecord dev inject <exe>` (task 2.1.7 prototype): launcher injection into a client that does
//! not import a DLL this loader could sideload — Phoenix, as of `docs/research/phoenix-recon.md`.
//!
//! Starts `exe` suspended, queues an APC that calls `LoadLibraryW` on the loader DLL on the
//! target's own (still suspended) main thread, then resumes it: the DLL's `DllMain` runs before
//! the target executes a single instruction of its own code. The actual syscalls live in
//! `phoecord_loader::inject` (`docs/AGENTS.md`: `unsafe` only in `phoecord-loader`); this is only
//! argument handling and the result message.
//!
//! This is a developer tool for proving the mechanism and measuring it (2.1.7's "done when").
//! `phoecord launch --channel phoenix` injects the same way, with the transport it picked
//! (`phoecord_installer::phoenix`, 2.2.8).

use crate::tf;
use anyhow::Result;
use clap::Args;
use phoecord_i18n::Localizer;
use std::path::PathBuf;

#[derive(Args)]
pub struct InjectArgs {
    /// The executable to start suspended and inject into.
    exe: PathBuf,
    /// The loader DLL to load into it. Defaults to `phoecord_loader.dll` next to this `phoecord`
    /// executable.
    #[arg(long)]
    dll: Option<PathBuf>,
    /// Arguments passed to `exe` (each quoted for you; no shell is involved).
    #[arg(last = true)]
    args: Vec<String>,
}

/// `phoecord_loader.dll` next to the running `phoecord` executable, if that can be worked out.
#[cfg(windows)]
fn default_dll_path() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", exe.display()))?;
    Ok(dir.join("phoecord_loader.dll"))
}

#[cfg(windows)]
pub fn run(args: InjectArgs, l10n: &Localizer) -> Result<()> {
    use anyhow::{Context, bail};
    use std::time::Instant;

    let dll = match args.dll {
        Some(d) => d,
        None => default_dll_path()?,
    };
    if !dll.is_file() {
        bail!(tf(
            l10n,
            "dev-inject-err-no-dll",
            &[("path", &dll.display().to_string())]
        ));
    }
    if !args.exe.is_file() {
        bail!(tf(
            l10n,
            "dev-inject-err-no-exe",
            &[("path", &args.exe.display().to_string())]
        ));
    }
    let arg_refs: Vec<&str> = args.args.iter().map(String::as_str).collect();

    let started = Instant::now();
    let suspended =
        phoecord_loader::inject::spawn_suspended(&args.exe, &arg_refs).with_context(|| {
            tf(
                l10n,
                "dev-inject-err-spawn",
                &[("exe", &args.exe.display().to_string())],
            )
        })?;
    phoecord_loader::inject::queue_load_library(&suspended, &dll).with_context(|| {
        tf(
            l10n,
            "dev-inject-err-queue",
            &[("dll", &dll.display().to_string())],
        )
    })?;
    let pid = phoecord_loader::inject::resume(suspended)
        .with_context(|| tf(l10n, "dev-inject-err-resume", &[]))?;
    let ms = started.elapsed().as_millis();
    println!(
        "{}",
        tf(
            l10n,
            "dev-inject-ok",
            &[("pid", &pid.to_string()), ("ms", &ms.to_string())]
        )
    );
    Ok(())
}

#[cfg(not(windows))]
pub fn run(_args: InjectArgs, l10n: &Localizer) -> Result<()> {
    anyhow::bail!(tf(l10n, "dev-inject-err-windows-only", &[]))
}
