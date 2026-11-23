//! REST route helpers shared by the proxy and plugins.

/// `/api/v9/channels/1/messages` → `/channels/1/messages`; other paths stay as they are.
pub fn route_of(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/api/") {
        if let Some((ver, tail)) = rest.split_once('/')
            && ver.len() > 1
            && ver.starts_with('v')
            && ver[1..].chars().all(|c| c.is_ascii_digit())
        {
            return format!("/{tail}");
        }
        return format!("/{rest}");
    }
    path.to_string()
}

/// Channel id from `/channels/{id}/messages` (and `/channels/{id}/messages/{mid}`).
pub fn message_route_channel(route: &str) -> Option<u64> {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    match seg.as_slice() {
        ["channels", id, "messages"] | ["channels", id, "messages", _] => id.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(
            route_of("/api/v9/channels/1/messages"),
            "/channels/1/messages"
        );
        assert_eq!(route_of("/api/science"), "/science");
        assert_eq!(route_of("/assets/app.js"), "/assets/app.js");
        assert_eq!(message_route_channel("/channels/42/messages"), Some(42));
        assert_eq!(message_route_channel("/channels/42/messages/7"), Some(42));
        assert_eq!(message_route_channel("/channels/42/typing"), None);
    }
}
