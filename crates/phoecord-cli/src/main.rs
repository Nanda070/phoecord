//! `phoecord`: command line front end. Thin: every command calls library code.

mod log;
mod selftest;
mod text;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use phoecord_plugin_runtime::Chain;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::wasm::WasmRuntime;
use phoecord_proxy::config::{Config, Paths};
use phoecord_proxy::{Proxy, ca, server, trust};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "phoecord", version, about = text::ABOUT)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = text::CMD_RUN)]
    Run,
    #[command(about = text::CMD_LAUNCH)]
    Launch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    #[command(about = text::CMD_SETUP)]
    Setup,
    #[command(about = text::CMD_TRUST)]
    Trust,
    #[command(about = text::CMD_UNTRUST)]
    Untrust,
    #[command(about = text::CMD_PLUGINS)]
    Plugins,
    #[command(about = text::CMD_ENABLE)]
    Enable { name: String },
    #[command(about = text::CMD_DISABLE)]
    Disable { name: String },
    #[command(about = text::CMD_TOGGLE)]
    Toggle { name: String },
    #[command(about = text::CMD_LOG)]
    Log {
        /// How many entries to show
        #[arg(short, default_value_t = 20)]
        n: usize,
    },
    #[command(about = text::CMD_SELFTEST)]
    Selftest,
    #[command(about = text::CMD_PATHS)]
    Paths,
}

fn chain() -> Chain {
    Chain::new(phoecord_plugins::builtin())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("PHOECORD_LOG")
                .unwrap_or_else(|_| "phoecord_proxy=info,phoecord=info".into()),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let paths = Paths::resolve();
    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => runtime()?.block_on(run(paths, None)),
        Cmd::Launch { command } => runtime()?.block_on(run(paths, Some(command))),
        Cmd::Setup => setup(&paths),
        Cmd::Trust => {
            let ca = ca::Authority::load_or_create(&paths.data)?;
            trust::install(&ca.ca_pem_path)
        }
        Cmd::Untrust => trust::uninstall(),
        Cmd::Plugins => plugins(&paths),
        Cmd::Enable { name } => switch(&paths, &name, Some(true)),
        Cmd::Disable { name } => switch(&paths, &name, Some(false)),
        Cmd::Toggle { name } => switch(&paths, &name, None),
        Cmd::Log { n } => log::run(&paths, n),
        Cmd::Selftest => runtime()?.block_on(selftest::run(paths)),
        Cmd::Paths => {
            for (label, p) in [
                ("config", paths.config.clone()),
                ("CA", paths.data.join("ca.pem")),
                ("plugins", paths.data.join("plugins")),
            ] {
                println!("{}", text::paths_line(label, &p.display().to_string()));
            }
            Ok(())
        }
    }
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Runtime::new()?)
}

async fn run(paths: Paths, launch: Option<Vec<String>>) -> Result<()> {
    let cfg = Config::load_or_init(&paths.config)?;
    let listen = cfg.listen.clone();
    let proxy = Proxy::new(paths, cfg, chain(), false)?;
    // WASM plugin packages in `<data>/plugins/<id>/`: loaded now, and again on every folder scan.
    match WasmRuntime::new() {
        Ok(rt) => proxy.watch_packages(PackageSet::new(rt, proxy.paths.data.join("plugins"))),
        Err(e) => tracing::warn!("WASM plugins are off: {e:#}"),
    }
    proxy.spawn_reloader();
    let listener = server::bind(&listen)
        .await
        .with_context(|| format!("listening on {listen}"))?;
    tracing::info!(
        "{}",
        text::listening(&listen, &proxy.chain.enabled_ids().join(", "))
    );
    let server = tokio::spawn(server::serve(proxy.clone(), listener));

    if let Some(args) = launch {
        let cmd = phoecord_installer::launch::command(&args, &listen)?;
        tracing::info!("{}", text::starting(&format!("{:?}", cmd.get_program())));
        let started = Instant::now();
        let mut child = tokio::process::Command::from(cmd)
            .spawn()
            .context("starting Discord")?;
        tokio::select! {
            status = child.wait() => {
                let status = status?;
                // Launchers (Update.exe, flatpak) hand off and exit at once: keep serving then.
                if started.elapsed() < Duration::from_secs(15) {
                    tracing::info!("{}", text::launcher_exited(&status.to_string()));
                    let _ = tokio::signal::ctrl_c().await;
                } else {
                    tracing::info!("{}", text::client_exited(&status.to_string()));
                }
            }
            _ = tokio::signal::ctrl_c() => {}
        }
        return Ok(());
    }
    tokio::select! {
        r = server => r??,
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

fn setup(paths: &Paths) -> Result<()> {
    Config::load_or_init(&paths.config)?;
    println!("{}", text::config_line(&paths.config.display().to_string()));
    let ca = ca::Authority::load_or_create(&paths.data)?;
    println!(
        "{}",
        text::ca_line(
            &ca.ca_pem_path.display().to_string(),
            &ca::CONSTRAINED_TO.join(", ")
        )
    );
    trust::install(&ca.ca_pem_path)?;
    println!("{}", text::SETUP_DONE);
    Ok(())
}

fn plugins(paths: &Paths) -> Result<()> {
    let cfg = Config::load_or_init(&paths.config)?;
    for m in chain().manifests() {
        let on = if cfg.is_enabled(&m.id) { "on " } else { "off" };
        let vencord = m
            .vencord
            .as_deref()
            .map(|v| format!("  (Vencord: {v})"))
            .unwrap_or_default();
        println!("{on}  {:<22} {}{vencord}", m.id, m.description);
    }
    println!(
        "\n{}",
        text::config_line(&paths.config.display().to_string())
    );
    Ok(())
}

fn switch(paths: &Paths, name: &str, to: Option<bool>) -> Result<()> {
    let chain = chain();
    if !chain.ids().iter().any(|id| id == name) {
        anyhow::bail!(text::no_such_plugin(name));
    }
    let mut cfg = Config::load_or_init(&paths.config)?;
    let was = cfg.is_enabled(name);
    let on = to.unwrap_or(!was);
    cfg.set_enabled(name, on)?;
    cfg.save(&paths.config)?;
    println!("{}", text::plugin_state(name, on));
    if on && !was && chain.uses_gateway(name) {
        // Gateway plugins only see a connection that started after they were switched on.
        println!("{}", text::GATEWAY_NOTE);
    }
    Ok(())
}
