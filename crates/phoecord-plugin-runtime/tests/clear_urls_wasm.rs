//! The example plugin `examples/plugins/clear_urls` (ClearURLs as a WASM package) behaves like the
//! built-in `clear_urls` and loads into a running chain without a restart.

use phoecord_core::{Host, Val};
use phoecord_plugin_runtime::package::PackageSet;
use phoecord_plugin_runtime::wasm::WasmRuntime;
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

mod common;
use common::TempDir;

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/clear_urls")
}

/// Builds the example once per test binary; returns its `plugin.wasm`.
fn example_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let dir = example_dir();
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
            "building examples/plugins/clear_urls failed (`rustup target add wasm32-wasip2`)\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read(dir.join("target/wasm32-wasip2/release/clear_urls_wasm.wasm")).unwrap()
    })
}

/// Installs the example the way a user does: its `plugin.toml` and `plugin.wasm` in `root/<id>/`.
fn install(root: &Path) {
    let dest = root.join("clear_urls_wasm");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(example_dir().join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::write(dest.join("plugin.wasm"), example_wasm()).unwrap();
}

/// A host that serves `extra_rules` and nothing else.
#[derive(Default)]
struct SettingsHost {
    inner: NullHost,
    extra_rules: Vec<String>,
}

impl Host for SettingsHost {
    fn setting(&self, key: &str) -> Option<Val> {
        (key == "extra_rules").then(|| {
            Val::List(
                self.extra_rules
                    .iter()
                    .map(|s| Val::Str(s.clone()))
                    .collect(),
            )
        })
    }
    fn kv_get(&self, key: &str) -> Option<Val> {
        self.inner.kv_get(key)
    }
    fn kv_set(&mut self, key: &str, value: Val) {
        self.inner.kv_set(key, value)
    }
    fn self_id(&self) -> Option<u64> {
        None
    }
    fn notify(&mut self, title: &str, body: &str) {
        self.inner.notify(title, body)
    }
    fn log(&mut self, message: &str) {
        self.inner.log(message)
    }
    fn emit_send(&mut self, op: i64, payload: Val) {
        self.inner.emit_send(op, payload)
    }
    fn guild_emojis(&self, _guild_id: u64) -> Vec<u64> {
        Vec::new()
    }
    fn channel_guild(&self, _channel_id: u64) -> Option<u64> {
        None
    }
}

impl phoecord_core::ChainHost for SettingsHost {}

fn message(content: &str) -> Val {
    let mut body = Val::map();
    body.set("content", Val::Str(content.into()));
    body.set("tts", Val::Bool(false));
    body
}

fn loaded_chain(root: &Path) -> (Chain, PackageSet) {
    let chain = Chain::default();
    let mut set = PackageSet::new(WasmRuntime::new().unwrap(), root.to_path_buf());
    let logs = set.sync(&chain);
    assert_eq!(chain.ids(), vec!["clear_urls_wasm"], "{logs:?}");
    (chain, set)
}

#[test]
fn the_package_cleans_sent_and_edited_messages_like_the_native_plugin() {
    let root = TempDir::new();
    install(&root.0);
    let (chain, _set) = loaded_chain(&root.0);
    let mut host = NullHost::default();

    // Tracking parameters go; other parameters, the fragment and the text stay byte for byte.
    let mut body = message("look https://example.com/a?utm_source=x&id=5&utm_medium=y#top ok, два");
    let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
    assert_eq!(out, Outcome::Changed);
    assert_eq!(
        body.str_at(&["content"]),
        Some("look https://example.com/a?id=5#top ok, два")
    );
    assert_eq!(body.get("tts"), Some(&Val::Bool(false)));

    // Site rules apply only on their site; Markdown and punctuation are handled.
    let mut body =
        message("[v](https://www.youtube.com/watch?v=abc&si=1&t=30) https://e.com/?si=1.");
    let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
    assert_eq!(out, Outcome::Changed);
    assert_eq!(
        body.str_at(&["content"]),
        Some("[v](https://www.youtube.com/watch?v=abc&t=30) https://e.com/?si=1.")
    );

    // An edit.
    let mut body = message("https://e.com/?fbclid=1");
    let out = chain.rest_request(
        &mut host,
        "PATCH",
        "/channels/1/messages/2",
        Some(&mut body),
    );
    assert_eq!(out, Outcome::Changed);
    assert_eq!(body.str_at(&["content"]), Some("https://e.com/"));
}

#[test]
fn the_package_leaves_everything_else_untouched() {
    let root = TempDir::new();
    install(&root.0);
    let (chain, _set) = loaded_chain(&root.0);
    let mut host = NullHost::default();

    // Nothing to strip.
    let text = "https://example.com/search?q=rust&page=2 no links here";
    let mut body = message(text);
    let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
    assert_eq!(out, Outcome::Untouched);
    assert_eq!(body.str_at(&["content"]), Some(text));

    // Routes that are not a message send or edit never reach the plugin.
    for (method, route) in [
        ("GET", "/channels/1/messages"),
        ("POST", "/channels/1/typing"),
        ("POST", "/channels/1/messages/2"),
        ("DELETE", "/channels/1/messages/2"),
    ] {
        let mut body = message("https://e.com/?utm_source=a");
        let out = chain.rest_request(&mut host, method, route, Some(&mut body));
        assert_eq!(out, Outcome::Untouched, "{method} {route}");
        assert_eq!(
            body.str_at(&["content"]),
            Some("https://e.com/?utm_source=a")
        );
    }

    // No body, or a body without `content`.
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", None),
        Outcome::Untouched
    );
    let mut body = Val::map();
    body.set("embeds", Val::List(vec![]));
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body)),
        Outcome::Untouched
    );
}

#[test]
fn extra_rules_come_from_the_host_setting() {
    let root = TempDir::new();
    install(&root.0);
    let (chain, _set) = loaded_chain(&root.0);
    let mut host = SettingsHost {
        extra_rules: vec!["REF".into(), "shop.com:tag".into()],
        ..Default::default()
    };

    let mut body =
        message("https://e.com/?ref=a&id=1 https://www.shop.com/p?tag=x&id=1 https://e.com/?tag=x");
    let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body));
    assert_eq!(out, Outcome::Changed);
    assert_eq!(
        body.str_at(&["content"]),
        Some("https://e.com/?id=1 https://www.shop.com/p?id=1 https://e.com/?tag=x")
    );
}

#[test]
fn the_package_can_be_added_to_a_running_chain() {
    let root = TempDir::new();
    let chain = Chain::default();
    let mut set = PackageSet::new(WasmRuntime::new().unwrap(), root.0.clone());
    let mut host = NullHost::default();

    // The chain runs with no plugins: the link is sent as it is.
    assert!(set.sync(&chain).is_empty());
    let mut body = message("https://e.com/?utm_source=a");
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body)),
        Outcome::Untouched
    );

    // The user drops the two files into `plugins/clear_urls_wasm/`: the next scan loads them.
    install(&root.0);
    let logs = set.sync(&chain);
    assert_eq!(chain.ids(), vec!["clear_urls_wasm"], "{logs:?}");
    let mut body = message("https://e.com/?utm_source=a");
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body)),
        Outcome::Changed
    );
    assert_eq!(body.str_at(&["content"]), Some("https://e.com/"));
}
