mod common;

use common::guest_wasm;
use phoecord_core::{Plugin, Val};
use phoecord_plugin_runtime::wasm::{WasmPlugin, WasmRuntime};
use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

fn chain_of(plugin: WasmPlugin) -> Chain {
    Chain::new(vec![Box::new(plugin) as Box<dyn Plugin>])
}

fn guest() -> WasmPlugin {
    let rt = WasmRuntime::new().unwrap();
    WasmPlugin::from_bytes(&rt, guest_wasm()).unwrap()
}

fn touched() -> Val {
    Val::Map(vec![(Val::Str("touched".into()), Val::Bool(true))])
}

#[test]
fn well_behaved_guest_works_in_the_chain_like_a_native_plugin() {
    let plugin = guest();
    assert_eq!(plugin.manifest().id, "test_guest");
    let chain = chain_of(plugin);
    let mut host = NullHost::default();

    // Dispatch: payload replaced, one extra payload, a host log reached the real host.
    let mut payload = Val::map();
    let mut extra = vec![];
    let out = chain.dispatch(&mut host, "MESSAGE_CREATE", &mut payload, &mut extra);
    assert_eq!(out, Outcome::Changed);
    assert_eq!(payload, touched());
    assert_eq!(extra, vec![touched()]);
    assert_eq!(host.logs, vec!["saw MESSAGE_CREATE".to_string()]);

    // REST request: a body is replaced, `/drop` is swallowed.
    let mut body = Val::map();
    assert_eq!(
        chain.rest_request(&mut host, "POST", "/channels/1/messages", Some(&mut body)),
        Outcome::Changed
    );
    assert_eq!(body, touched());
    assert_eq!(
        chain.rest_request(&mut host, "GET", "/drop", None),
        Outcome::Dropped {
            by: "test_guest".into()
        }
    );

    // The guest's own `wants` can say no.
    let mut payload = Val::map();
    assert_eq!(
        chain.dispatch(&mut host, "UNWANTED", &mut payload, &mut vec![]),
        Outcome::Untouched
    );
    assert_eq!(payload, Val::map());

    // Send and a REST answer reach the guest and pass.
    assert_eq!(
        chain.send(&mut host, 3, &mut Val::map()),
        Outcome::Untouched
    );
    assert_eq!(
        chain.rest_response(&mut host, "GET", "/x", 200, &Val::Null),
        Outcome::Untouched
    );
}
