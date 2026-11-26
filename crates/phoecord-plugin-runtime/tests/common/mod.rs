#![allow(dead_code)] // each test binary uses a different part
//! Shared setup for the WASM tests: builds the guest in `tests/guest` once per test binary.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// The bytes of the test guest component (built with `cargo build --target wasm32-wasip2`).
pub fn guest_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| build_guest("guest", "phoecord_test_guest"))
}

/// The bytes of the compatibility guest, built against the frozen v1 WIT (`wit/frozen/`).
pub fn compat_guest_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| build_guest("compat-guest", "phoecord_compat_guest"))
}

/// Builds the guest crate in `tests/<folder>` and reads its component.
fn build_guest(folder: &str, artifact: &str) -> Vec<u8> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(folder);
    let out = Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        // Do not inherit the outer build's target dir or flags: the guest is its own workspace.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("cannot run cargo");
    assert!(
        out.status.success(),
        "building tests/{folder} failed (is the wasm32-wasip2 target installed?          `rustup target add wasm32-wasip2`)
{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read(dir.join(format!("target/wasm32-wasip2/release/{artifact}.wasm")))
        .expect("guest wasm missing")
}

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// A scratch folder under the system temp dir, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "phoecord-wasm-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `root/<id>/plugin.toml` (header + `extra` lines) and `plugin.wasm` (the test guest).
pub fn write_package(root: &Path, id: &str, api: u32, extra: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let toml =
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\napi = {api}\n{extra}\n");
    std::fs::write(dir.join("plugin.toml"), toml).unwrap();
    std::fs::write(dir.join("plugin.wasm"), guest_wasm()).unwrap();
    dir
}

/// A runtime with the production limits, for tests that are not about timing.
pub fn runtime() -> std::sync::Arc<phoecord_plugin_runtime::wasm::WasmRuntime> {
    phoecord_plugin_runtime::wasm::WasmRuntime::new().unwrap()
}

/// A runtime whose hard ceiling is short, so a test of an endless loop is quick.
pub fn quick_runtime() -> std::sync::Arc<phoecord_plugin_runtime::wasm::WasmRuntime> {
    phoecord_plugin_runtime::wasm::WasmRuntime::with_limits(
        phoecord_plugin_runtime::wasm::WasmLimits {
            hard_timeout: std::time::Duration::from_millis(200),
            ..Default::default()
        },
    )
    .unwrap()
}
