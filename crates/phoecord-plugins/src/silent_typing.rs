//! SilentTyping: your typing indicator is never sent (Vencord: SilentTyping).

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Verdict};

pub struct SilentTyping {
    manifest: Manifest,
}

impl SilentTyping {
    pub fn new() -> SilentTyping {
        SilentTyping {
            manifest: {
                let mut m = Manifest::new(
                    "silent_typing",
                    "SilentTyping",
                    env!("CARGO_PKG_VERSION"),
                    "Your typing indicator is never sent",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("SilentTyping".into());
                m
            },
        }
    }
}

impl Default for SilentTyping {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/typing` and nothing else.
fn matches(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "POST" && matches!(seg.as_slice(), ["channels", _, "typing"])
}

impl Plugin for SilentTyping {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if matches(method, route))
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest { method, route, .. } if matches(method, route) => Verdict::Reply {
                status: 204,
                body: None,
            },
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    #[test]
    fn matches_only_the_typing_route() {
        assert!(matches("POST", "/channels/1/typing"));
        assert!(matches("POST", "/channels/1/typing/"));
        assert!(!matches("GET", "/channels/1/typing"));
        assert!(!matches("POST", "/channels/1/messages"));
        assert!(!matches("POST", "/channels/typing"));
        assert!(!matches("POST", "/channels/1/messages/2/typing"));
    }

    #[test]
    fn typing_is_answered_locally() {
        let chain = Chain::new(vec![Box::new(SilentTyping::new())]);
        let mut host = NullHost::default();
        let out = chain.rest_request(&mut host, "POST", "/channels/1/typing", None);
        assert!(matches!(out, Outcome::Replied { status: 204, .. }));
    }

    #[test]
    fn other_requests_pass() {
        let chain = Chain::new(vec![Box::new(SilentTyping::new())]);
        let mut host = NullHost::default();
        let out = chain.rest_request(&mut host, "POST", "/channels/1/messages", None);
        assert_eq!(out, Outcome::Untouched);
    }
}
