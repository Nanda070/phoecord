//! Installing and removing a WASM package from the settings tab (2.3.3) in an in-process launch:
//! a real package (the runtime's test guest) is staged, committed and loaded into a running
//! engine, switched on, then removed and unloaded, with no restart.

use phoecord_loader::ui::packages;
use phoecord_plugin_runtime::Chain;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::wasm::WasmRuntime;
use phoecord_transport::Engine;
use phoecord_transport::config::Config;
use phoecord_transport::host::Shared;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The runtime's test guest, built for `wasm32-wasip2` as its own tests do.
fn guest_wasm() -> Vec<u8> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../phoecord-plugin-runtime/tests/guest");
    let out = Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("cannot run cargo");
    assert!(
        out.status.success(),
        "guest build failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read(dir.join("target/wasm32-wasip2/release/phoecord_test_guest.wasm"))
        .expect("guest component")
}

fn write_package(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        "id = \"tab_guest\"\nname = \"Tab guest\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .unwrap();
    std::fs::write(dir.join("plugin.wasm"), guest_wasm()).unwrap();
}

#[test]
fn a_package_installed_from_the_tab_runs_at_once_and_goes_on_remove() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let config = t.path().join("config.toml");
    let mut shared = Shared::new(data.join("plugins"));
    shared.config_path = config.clone();
    let engine = Engine::new(Chain::new(Vec::new()), shared, Config::default());
    engine.watch_packages(PackageSet::new(
        WasmRuntime::new().unwrap(),
        data.join("plugins"),
    ));

    let src = t.path().join("picked");
    write_package(&src);
    let store = packages::store(&data, &config);
    let c = packages::stage(&store, &src).expect("the guest stages");
    let tr = phoecord_i18n::Localizer::new("en");
    let ui = packages::localized(&tr);
    assert_eq!(packages::confirm(&c, &ui).title, "Install Tab guest?");

    assert_eq!(
        packages::engine_install(&engine, &store, &c).unwrap(),
        "tab_guest"
    );
    assert!(
        engine.chain.is_dynamic("tab_guest"),
        "loaded without a restart"
    );
    assert!(
        engine
            .chain
            .enabled_ids()
            .contains(&"tab_guest".to_string()),
        "a first install is switched on"
    );
    assert!(
        engine.cfg().is_enabled("tab_guest"),
        "the engine has the new config"
    );

    packages::engine_remove(&engine, &store, "tab_guest").unwrap();
    assert!(
        !engine.chain.ids().contains(&"tab_guest".to_string()),
        "unloaded"
    );
    assert!(!data.join("plugins/tab_guest").exists());
    assert!(
        !Config::load(&config)
            .unwrap()
            .plugins
            .contains_key("tab_guest")
    );
}
