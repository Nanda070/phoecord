//! Plugin API v1 is frozen (task 2.4.3). These tests fail when a change to `wit/phoecord.wit`
//! would stop plugins that are already built from loading:
//! - `interface types` and the world must stay exactly as frozen in `wit/frozen/`, because
//!   wasmtime refuses a component whose WIT `variant`, `enum` or `record` differs from the host's;
//! - `interface host` may only gain functions;
//! - a guest built against the frozen WIT must load and see every event, `ui` included.

mod common;

use common::compat_guest_wasm;
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Plugin, UiEvent, Val};
use phoecord_plugin_runtime::wasm::WasmPlugin;
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};
use std::path::PathBuf;

fn wit(path: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
}

/// The text of the block that starts with `head` (up to its matching `}`), with comments and
/// whitespace runs removed, so only the contract is compared.
fn block(text: &str, head: &str) -> String {
    let code: String = text
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let start = code.find(head).unwrap_or_else(|| panic!("no {head:?}"));
    let mut depth = 0;
    let mut end = start;
    for (i, c) in code[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    code[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `name: func(...) -> ...;` items of `interface host`.
fn host_functions(text: &str) -> Vec<String> {
    block(text, "interface host {")
        .split(';')
        .map(str::trim)
        .filter(|item| item.contains(": func("))
        .map(|item| item.rsplit('{').next().unwrap().trim().to_string())
        .collect()
}

#[test]
fn the_sdk_targets_the_frozen_api_version() {
    assert_eq!(API_VERSION, 1);
    assert_eq!(phoecord_plugin_sdk::API_VERSION, API_VERSION);
}

#[test]
fn types_and_world_did_not_change_since_the_freeze() {
    let (now, frozen) = (
        wit("wit/phoecord.wit"),
        wit("wit/frozen/phoecord-1.0.0.wit"),
    );
    assert!(frozen.contains("package phoecord:plugin@1.0.0;"));
    assert!(now.contains("package phoecord:plugin@1.0.0;"));
    for head in ["interface types {", "world plugin {"] {
        assert_eq!(
            block(&now, head),
            block(&frozen, head),
            "`{head}` of wit/phoecord.wit differs from wit/frozen/: plugins built against v1 \
             would stop loading. Only new host functions and new UI event names are additive."
        );
    }
}

#[test]
fn host_functions_only_grow() {
    let now = host_functions(&wit("wit/phoecord.wit"));
    let frozen = host_functions(&wit("wit/frozen/phoecord-1.0.0.wit"));
    assert!(frozen.len() >= 21, "{frozen:?}");
    for f in &frozen {
        assert!(
            now.contains(f),
            "host function changed or removed since the freeze: {f}"
        );
    }
}

#[test]
fn a_plugin_built_against_the_frozen_wit_still_works() {
    let rt = common::runtime();
    let plugin = WasmPlugin::from_bytes(&rt, compat_guest_wasm()).expect("the v1 guest loads");
    assert_eq!(plugin.manifest().id, "compat_guest");
    assert_eq!(plugin.manifest().api, API_VERSION);
    let chain = Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>]);
    let mut host = NullHost::default();

    let mut payload = Val::map();
    let mut extra = vec![];
    assert_eq!(
        chain.dispatch(&mut host, "MESSAGE_CREATE", &mut payload, &mut extra),
        Outcome::Changed
    );
    assert_eq!(payload, Val::Str("seen".into()));
    assert_eq!(
        chain.dispatch(&mut host, "UNWANTED", &mut Val::map(), &mut vec![]),
        Outcome::Untouched
    );
    assert!(matches!(
        chain.rest_request(&mut host, "GET", "/users/@me", None),
        Outcome::Replied { status: 200, body: Some(Val::Str(ref r)), .. } if r == "/users/@me"
    ));
    assert!(matches!(
        chain.send(&mut host, 3, &mut Val::map()),
        Outcome::Dropped { .. }
    ));

    let click = UiEvent::MenuAction {
        entry: "copy_raw",
        channel_id: 10,
        message_id: 1_234_567_890_123_456_789,
    };
    assert_eq!(
        chain.ui_to(&mut host, "compat_guest", click),
        Some(Outcome::Untouched)
    );
    assert_eq!(
        host.logs,
        vec![
            "dispatch MESSAGE_CREATE".to_string(),
            "ui menu_action copy_raw 1234567890123456789".to_string()
        ]
    );
}
