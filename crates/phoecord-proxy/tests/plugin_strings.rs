//! `Host::t` through the real proxy host: a built-in plugin's notification in the user's language.

use phoecord_core::Val;
use phoecord_plugin_runtime::Chain;
use phoecord_proxy::config::Config;
use phoecord_proxy::host::{ProxyHost, Shared};
use std::sync::{Arc, Mutex};

fn json(s: &str) -> Val {
    Val::from_json(serde_json::from_str(s).unwrap())
}

/// Run READY and an unfriend event through the built-in plugins; the notifications shown.
fn notifications(language: Option<&str>, flag: Option<&str>) -> Vec<(String, String)> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let dir = std::env::temp_dir().join(format!("phoecord-strings-{}", std::process::id()));
    let shared = Shared::with_notifier(
        dir,
        Box::new(move |t, b| sink.lock().unwrap().push((t.to_string(), b.to_string()))),
    );
    // Not the machine's language: the test must give the same answer everywhere.
    shared.set_os_language(Some("en".into()));
    shared.set_language_flag(flag.map(str::to_string));
    phoecord_plugins::register_strings(&shared.strings);
    let cfg = Config {
        language: language.map(str::to_string),
        ..Config::default()
    };

    let chain = Chain::new(phoecord_plugins::builtin());
    let mut host = ProxyHost::new(&shared, &cfg);
    for (name, frame) in [
        (
            "READY",
            r#"{"op":0,"t":"READY","d":{"user":{"id":"1"},
                "relationships":[{"id":"3","type":1,"user":{"id":"3","username":"bo"}}]}}"#,
        ),
        (
            "RELATIONSHIP_REMOVE",
            r#"{"op":0,"t":"RELATIONSHIP_REMOVE","d":{"id":"3","type":1}}"#,
        ),
    ] {
        let mut payload = json(frame);
        chain.dispatch(&mut host, name, &mut payload, &mut Vec::new());
    }
    seen.lock().unwrap().clone()
}

#[test]
fn a_builtin_notification_appears_in_russian_with_language_ru() {
    assert_eq!(
        notifications(Some("ru"), None),
        [(
            "Друг удалён".to_string(),
            "bo больше не ваш друг.".to_string()
        )]
    );
}

#[test]
fn english_is_the_default_and_the_flag_beats_the_config() {
    let english = [(
        "Friend removed".to_string(),
        "bo is no longer your friend.".to_string(),
    )];
    assert_eq!(notifications(None, None), english);
    assert_eq!(notifications(Some("ru"), Some("en")), english);
    assert_eq!(notifications(Some("xx"), None), english);
}
