//! Blocks Discord's analytics and crash reporting (Vencord: NoTrack).

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Verdict};

pub struct NoTrack {
    manifest: Manifest,
}

impl NoTrack {
    pub fn new() -> NoTrack {
        NoTrack {
            manifest: {
                let mut m = Manifest::new(
                    "no_track",
                    "NoTrack",
                    env!("CARGO_PKG_VERSION"),
                    "Block Discord's analytics and crash reports",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("NoTrack".into());
                m
            },
        }
    }
}

impl Default for NoTrack {
    fn default() -> Self {
        Self::new()
    }
}

fn is_tracking(method: &str, route: &str) -> bool {
    let r = route.trim_end_matches('/');
    (method == "POST" && matches!(r, "/science" | "/track" | "/metrics" | "/metrics/v2"))
        || r.starts_with("/error-reporting-proxy")
}

impl Plugin for NoTrack {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_tracking(method, route))
    }
    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest { method, route, .. } if is_tracking(method, route) => {
                Verdict::Reply {
                    status: 204,
                    body: None,
                }
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_tracking_only() {
        assert!(is_tracking("POST", "/science"));
        assert!(is_tracking("POST", "/metrics/v2"));
        assert!(is_tracking("POST", "/error-reporting-proxy/web"));
        assert!(!is_tracking("GET", "/science"));
        assert!(!is_tracking("POST", "/channels/1/messages"));
    }
}
