//! Shared setup for the WASM tests: builds the guest in `tests/guest` once per test binary.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// The bytes of the test guest component (built with `cargo build --target wasm32-wasip2`).
pub fn guest_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/guest");
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
            "building the test guest failed (is the wasm32-wasip2 target installed? \
             `rustup target add wasm32-wasip2`)\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read(dir.join("target/wasm32-wasip2/release/phoecord_test_guest.wasm"))
            .expect("guest wasm missing")
    })
}
