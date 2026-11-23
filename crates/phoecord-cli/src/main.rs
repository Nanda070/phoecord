//! `phoecord`: command line front end.
//!
//! Status: scaffold. Commands are ported from the v0 CLI as the proxy lands (ROADMAP.md, 1.1):
//! `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `log`, `selftest`.

use phoecord_plugin_runtime::Chain;

fn main() -> anyhow::Result<()> {
    let chain = Chain::new(phoecord_plugins::builtin());
    println!("PhoeCord {} (scaffold)", env!("CARGO_PKG_VERSION"));
    println!(
        "built-in plugins: {}",
        chain.ids().collect::<Vec<_>>().join(", ")
    );
    println!("local CA name: {}", phoecord_proxy::ca::CA_NAME);
    println!("The proxy is not wired up yet; see ROADMAP.md, milestone 1.1.");
    Ok(())
}
