//! The SDK example `examples/hello-plugin` builds with `phoecord-plugin-sdk`, loads as a package and
//! logs every `MESSAGE_CREATE`.

use phoecord_core::Val;
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::wasm::WasmRuntime;
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::path::Path;
use std::process::Command;

mod common;
use common::TempDir;

fn map(fields: Vec<(&str, Val)>) -> Val {
    Val::Map(
        fields
            .into_iter()
            .map(|(k, v)| (Val::Str(k.into()), v))
            .collect(),
    )
}

fn message(id: &str) -> Val {
    let text = |s: &str| Val::Str(s.into());
    let d = map(vec![
        ("id", text(id)),
        ("channel_id", text("42")),
        ("content", text("secret text")),
    ]);
    map(vec![
        ("op", Val::Int(0)),
        ("t", text("MESSAGE_CREATE")),
        ("d", d),
    ])
}

#[test]
fn the_sdk_example_loads_and_logs_messages() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-plugin");
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
        "building examples/hello-plugin failed (`rustup target add wasm32-wasip2`)\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let root = TempDir::new();
    let dest = root.0.join("hello_plugin");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(dir.join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::copy(
        dir.join("target/wasm32-wasip2/release/hello_plugin.wasm"),
        dest.join("plugin.wasm"),
    )
    .unwrap();

    let chain = Chain::default();
    let mut set = PackageSet::new(WasmRuntime::new().unwrap(), root.0.clone());
    let logs = set.sync(&chain);
    assert_eq!(chain.ids(), vec!["hello_plugin"], "{logs:?}");

    let mut host = NullHost::default();
    let mut extra = Vec::new();
    for id in ["7", "8"] {
        let mut payload = message(id);
        let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut payload, &mut extra);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(payload, message(id));
    }
    let mine: Vec<&String> = host
        .logs
        .iter()
        .filter(|l| l.contains("MESSAGE_CREATE"))
        .collect();
    assert_eq!(mine.len(), 2, "{:?}", host.logs);
    assert!(mine[0].contains("#1: message 7 in channel 42"), "{mine:?}");
    assert!(mine[1].contains("#2: message 8"), "{mine:?}");
    assert!(!host.logs.iter().any(|l| l.contains("secret")));
}
