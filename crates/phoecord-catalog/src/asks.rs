//! How a package's asks read in a confirm dialog, shared by the installer window's Plugins tab and
//! the PhoeCord settings tab in Phoenix (2.3.3), so the two never disagree.

/// Rights that act as the user on Discord: the dialogs mark them as a warning.
pub const STRONG_RIGHTS: &[&str] = &["rest", "emit_send"];

/// Whether right `name` acts as the user.
pub fn is_strong(name: &str) -> bool {
    STRONG_RIGHTS.contains(&name)
}

/// The string key for right `name` (`emit_send` → `gui-right-emit-send`).
pub fn right_key(name: &str) -> String {
    format!("gui-right-{}", name.replace('_', "-"))
}

/// One event a package reads, in words: `rest_request:POST /channels/*/messages` →
/// "Requests your Discord sends: POST /channels/*/messages". `t` translates a `gui-*` key.
pub fn event_text(event: &str, t: impl Fn(&str) -> String) -> String {
    let (kind, detail) = event.split_once(':').unwrap_or((event, ""));
    let key = match kind {
        "rest_request" => "gui-event-rest-request",
        "rest_response" => "gui-event-rest-response",
        "dispatch" => "gui-event-dispatch",
        "send" => "gui-event-send",
        _ => "",
    };
    let what = if key.is_empty() {
        kind.to_string()
    } else {
        t(key)
    };
    let which = if detail.is_empty() || detail == "*" {
        t("gui-event-all")
    } else {
        detail.to_string()
    };
    format!("{what}: {which}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_and_rights_read_as_words() {
        let t = |k: &str| format!("<{k}>");
        assert_eq!(
            event_text("dispatch:MESSAGE_CREATE", t),
            "<gui-event-dispatch>: MESSAGE_CREATE"
        );
        assert_eq!(
            event_text("rest_request:*", t),
            "<gui-event-rest-request>: <gui-event-all>"
        );
        assert_eq!(event_text("ui", t), "ui: <gui-event-all>");
        assert_eq!(right_key("emit_send"), "gui-right-emit-send");
        assert!(is_strong("rest") && is_strong("emit_send") && !is_strong("kv"));
    }
}
