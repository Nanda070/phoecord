//! RelationshipNotifier: a desktop notification when someone unfriends you, cancels or declines a
//! friend request, or removes you from a server or group DM (Vencord: RelationshipNotifier).
//! Things you did yourself are not reported.
//!
//! Ported from the v0 implementation. The plugin learns names from the gateway (`READY`, guild and
//! channel events) and remembers your own destructive REST calls for a short while.
//!
//! Settings (`[plugins.relationship_notifier]`, all default `true`): `friends`, `friend_requests`,
//! `servers`, `groups`.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an action of your own suppresses the matching notification.
const OWN_ACTION_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct State {
    user_id: Option<u64>,
    user_names: HashMap<u64, String>,
    guild_names: HashMap<u64, String>,
    group_names: HashMap<u64, String>,
    /// `friend:ID`, `guild:ID`, `group:ID` -> when you did it.
    own_actions: HashMap<String, Instant>,
}

pub struct RelationshipNotifier {
    manifest: Manifest,
    state: Mutex<State>,
}

impl RelationshipNotifier {
    pub fn new() -> RelationshipNotifier {
        RelationshipNotifier {
            manifest: Manifest {
                id: "relationship_notifier".into(),
                name: "RelationshipNotifier".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Notify you when you're unfriended or removed from a server or group"
                    .into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("RelationshipNotifier".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            state: Mutex::new(State::default()),
        }
    }
}

impl Default for RelationshipNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Gateway events the plugin reads: notification triggers and name sources.
fn dispatch_matches(name: &str) -> bool {
    matches!(
        name,
        "READY"
            | "GUILD_CREATE"
            | "GUILD_UPDATE"
            | "CHANNEL_CREATE"
            | "CHANNEL_UPDATE"
            | "RELATIONSHIP_ADD"
            | "RELATIONSHIP_REMOVE"
            | "GUILD_DELETE"
            | "CHANNEL_RECIPIENT_REMOVE"
    )
}

/// The key under which a REST call of yours is remembered, if it later produces a removal event:
/// unfriend, decline or cancel a request, block, leave or delete a server, leave a group.
fn own_action_key(method: &str, route: &str) -> Option<String> {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    match (method, seg.as_slice()) {
        // Blocking is a PUT on the same route that adds a friend; both are your own doing.
        ("DELETE" | "PUT", ["users", "@me", "relationships", id]) => Some(format!("friend:{id}")),
        ("DELETE", ["users", "@me", "guilds", id]) | ("DELETE", ["guilds", id]) => {
            Some(format!("guild:{id}"))
        }
        ("POST", ["guilds", id, "delete"]) => Some(format!("guild:{id}")),
        ("DELETE", ["channels", id]) => Some(format!("group:{id}")),
        _ => None,
    }
}

fn display_name(u: &Val) -> Option<String> {
    u.str_at(&["global_name"])
        .or_else(|| u.str_at(&["username"]))
        .map(str::to_string)
}

/// Which kinds of notification are on.
#[derive(Clone, Copy)]
struct Opts {
    friends: bool,
    friend_requests: bool,
    servers: bool,
    groups: bool,
}

impl Opts {
    fn read(host: &dyn Host) -> Opts {
        let on = |key: &str| host.setting(key).and_then(|v| v.as_bool()).unwrap_or(true);
        Opts {
            friends: on("friends"),
            friend_requests: on("friend_requests"),
            servers: on("servers"),
            groups: on("groups"),
        }
    }
}

impl State {
    fn did_myself(&self, key: &str) -> bool {
        self.own_actions
            .get(key)
            .is_some_and(|t| t.elapsed() < OWN_ACTION_TTL)
    }

    fn name_of(&self, user: u64) -> String {
        self.user_names
            .get(&user)
            .cloned()
            .unwrap_or_else(|| format!("user {user}"))
    }

    fn remember_user(&mut self, u: &Val) {
        if let (Some(id), Some(name)) = (u.id_at(&["id"]), display_name(u)) {
            self.user_names.insert(id, name);
        }
    }

    fn remember_guild(&mut self, g: &Val) {
        if let (Some(id), Some(name)) = (
            g.id_at(&["id"]),
            g.str_at(&["name"])
                .or_else(|| g.str_at(&["properties", "name"])),
        ) {
            self.guild_names.insert(id, name.to_string());
        }
    }

    fn remember_group(&mut self, c: &Val) {
        let Some(id) = c.id_at(&["id"]) else { return };
        if c.at(&["type"]).and_then(Val::as_i64) != Some(3) {
            return;
        }
        let recipients = c.get("recipients").map(Val::as_list).unwrap_or_default();
        for r in recipients {
            self.remember_user(r);
        }
        let name = match c.str_at(&["name"]).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => {
                let names: Vec<String> = recipients.iter().filter_map(display_name).collect();
                if names.is_empty() {
                    "a group DM".into()
                } else {
                    names.join(", ")
                }
            }
        };
        self.group_names.insert(id, name);
    }

    /// Learn names from a dispatch's `d`.
    fn track(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                if let Some(u) = d.get("user") {
                    self.user_id = u.id_at(&["id"]);
                    self.remember_user(u);
                }
                for u in d.get("users").map(Val::as_list).unwrap_or_default() {
                    self.remember_user(u);
                }
                for g in d.get("guilds").map(Val::as_list).unwrap_or_default() {
                    self.remember_guild(g);
                }
                for c in d
                    .get("private_channels")
                    .map(Val::as_list)
                    .unwrap_or_default()
                {
                    self.remember_group(c);
                }
                for r in d.get("relationships").map(Val::as_list).unwrap_or_default() {
                    if let Some(u) = r.get("user") {
                        self.remember_user(u);
                    }
                }
            }
            "GUILD_CREATE" | "GUILD_UPDATE" => self.remember_guild(d),
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" if d.id_at(&["guild_id"]).is_none() => {
                self.remember_group(d)
            }
            "RELATIONSHIP_ADD" => {
                if let Some(u) = d.get("user") {
                    self.remember_user(u);
                }
            }
            _ => {}
        }
    }

    /// The notification for a removal event, if any: `(title, body)`.
    fn notice(
        &self,
        name: &str,
        d: &Val,
        self_id: Option<u64>,
        opts: Opts,
    ) -> Option<(String, String)> {
        match name {
            "RELATIONSHIP_REMOVE" => {
                let id = d.id_at(&["id"])?;
                if self.did_myself(&format!("friend:{id}")) {
                    return None;
                }
                let who = self.name_of(id);
                match d.at(&["type"]).and_then(Val::as_i64) {
                    Some(1) if opts.friends => Some((
                        "Friend removed".into(),
                        format!("{who} is no longer your friend."),
                    )),
                    Some(3) if opts.friend_requests => Some((
                        "Friend request cancelled".into(),
                        format!("{who} cancelled their friend request."),
                    )),
                    Some(4) if opts.friend_requests => Some((
                        "Friend request declined".into(),
                        format!("{who} declined your friend request."),
                    )),
                    _ => None,
                }
            }
            "GUILD_DELETE" if opts.servers => {
                let id = d.id_at(&["id"])?;
                // `unavailable` means an outage, not a removal.
                if d.at(&["unavailable"]).and_then(Val::as_bool) == Some(true)
                    || self.did_myself(&format!("guild:{id}"))
                {
                    return None;
                }
                let name = self
                    .guild_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("server {id}"));
                Some((
                    "Removed from a server".into(),
                    format!(
                        "You're no longer in {name}. You were kicked or banned, or it was deleted."
                    ),
                ))
            }
            "CHANNEL_RECIPIENT_REMOVE" if opts.groups => {
                let ch = d.id_at(&["channel_id"])?;
                let removed = d.id_at(&["user", "id"]);
                let me = self.user_id.or(self_id);
                if removed.is_none() || removed != me || self.did_myself(&format!("group:{ch}")) {
                    return None;
                }
                let name = self
                    .group_names
                    .get(&ch)
                    .cloned()
                    .unwrap_or_else(|| "a group DM".into());
                Some((
                    "Removed from a group".into(),
                    format!("You were removed from {name}."),
                ))
            }
            _ => None,
        }
    }
}

impl Plugin for RelationshipNotifier {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => dispatch_matches(name),
            EventKind::RestRequest { method, route } => own_action_key(method, route).is_some(),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest { method, route, .. } => {
                if let Some(key) = own_action_key(method, route) {
                    let mut st = self.state.lock().unwrap();
                    st.own_actions.insert(key, Instant::now());
                    st.own_actions
                        .retain(|_, t| t.elapsed() < OWN_ACTION_TTL * 2);
                }
            }
            Event::Dispatch { name, payload, .. } if dispatch_matches(name) => {
                let Some(d) = payload.get("d") else {
                    return Verdict::Pass;
                };
                let opts = Opts::read(host);
                let notice = {
                    let mut st = self.state.lock().unwrap();
                    let notice = st.notice(name, d, host.self_id(), opts);
                    st.track(name, d);
                    notice
                };
                if let Some((title, body)) = notice {
                    host.notify(&title, &body);
                }
            }
            _ => {}
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoecord_plugin_runtime::{Chain, NullHost, Outcome};

    fn json(s: &str) -> Val {
        Val::from_json(serde_json::from_str(s).unwrap())
    }

    /// A host that answers a few settings and records notifications.
    #[derive(Default)]
    struct TestHost {
        settings: Vec<(&'static str, bool)>,
        notices: Vec<(String, String)>,
    }

    impl Host for TestHost {
        fn setting(&self, key: &str) -> Option<Val> {
            self.settings
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| Val::Bool(*v))
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            None
        }
        fn notify(&mut self, title: &str, body: &str) {
            self.notices.push((title.into(), body.into()));
        }
        fn log(&mut self, _: &str) {}
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, _: u64) -> Vec<u64> {
            Vec::new()
        }
        fn channel_guild(&self, _: u64) -> Option<u64> {
            None
        }
    }

    const READY: &str = r#"{"op":0,"t":"READY","d":{"user":{"id":"1"},
        "guilds":[{"id":"10","name":"Ember"}],
        "private_channels":[{"id":"20","type":3,"name":"","recipients":[{"id":"4","username":"cy"},{"id":"5","username":"di"}]}],
        "relationships":[{"id":"3","type":1,"user":{"id":"3","username":"bo"}}]}}"#;

    const FRIEND_GONE: &str = r#"{"op":0,"t":"RELATIONSHIP_REMOVE","d":{"id":"3","type":1}}"#;
    const GUILD_GONE: &str = r#"{"op":0,"t":"GUILD_DELETE","d":{"id":"10"}}"#;
    const GROUP_KICK: &str =
        r#"{"op":0,"t":"CHANNEL_RECIPIENT_REMOVE","d":{"channel_id":"20","user":{"id":"1"}}}"#;

    fn dispatch(chain: &Chain, host: &mut dyn Host, name: &str, frame: &str) {
        let mut p = json(frame);
        let mut extra = Vec::new();
        assert_eq!(
            chain.dispatch(host, name, &mut p, &mut extra),
            Outcome::Untouched
        );
    }

    fn ready_chain() -> (Chain, TestHost) {
        let chain = Chain::new(vec![Box::new(RelationshipNotifier::new())]);
        let mut host = TestHost::default();
        dispatch(&chain, &mut host, "READY", READY);
        (chain, host)
    }

    #[test]
    fn matching_rule() {
        let p = RelationshipNotifier::new();
        assert!(p.wants(&EventKind::Dispatch {
            name: "RELATIONSHIP_REMOVE"
        }));
        assert!(p.wants(&EventKind::Dispatch {
            name: "GUILD_DELETE"
        }));
        assert!(!p.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
        assert!(p.wants(&EventKind::RestRequest {
            method: "DELETE",
            route: "/users/@me/guilds/10"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "GET",
            route: "/users/@me/guilds/10"
        }));
        assert!(!p.wants(&EventKind::Send { op: 3 }));
        assert_eq!(
            own_action_key("DELETE", "/channels/20"),
            Some("group:20".into())
        );
        assert_eq!(own_action_key("POST", "/channels/20/messages"), None);
    }

    #[test]
    fn reports_removals() {
        let (chain, mut host) = ready_chain();
        dispatch(&chain, &mut host, "RELATIONSHIP_REMOVE", FRIEND_GONE);
        dispatch(&chain, &mut host, "GUILD_DELETE", GUILD_GONE);
        dispatch(&chain, &mut host, "CHANNEL_RECIPIENT_REMOVE", GROUP_KICK);
        let bodies: Vec<&str> = host.notices.iter().map(|n| n.1.as_str()).collect();
        assert_eq!(bodies.len(), 3, "{bodies:?}");
        assert_eq!(bodies[0], "bo is no longer your friend.");
        assert!(bodies[1].contains("Ember"));
        assert_eq!(bodies[2], "You were removed from cy, di.");
    }

    #[test]
    fn ignores_unavailable_guilds_and_other_recipients() {
        let (chain, mut host) = ready_chain();
        dispatch(
            &chain,
            &mut host,
            "GUILD_DELETE",
            r#"{"op":0,"t":"GUILD_DELETE","d":{"id":"10","unavailable":true}}"#,
        );
        dispatch(
            &chain,
            &mut host,
            "CHANNEL_RECIPIENT_REMOVE",
            r#"{"op":0,"t":"CHANNEL_RECIPIENT_REMOVE","d":{"channel_id":"20","user":{"id":"4"}}}"#,
        );
        assert!(host.notices.is_empty());
    }

    #[test]
    fn own_actions_are_not_reported() {
        let (chain, mut host) = ready_chain();
        for (method, route) in [
            ("DELETE", "/users/@me/guilds/10"),
            ("DELETE", "/users/@me/relationships/3"),
            ("DELETE", "/channels/20"),
        ] {
            assert_eq!(
                chain.rest_request(&mut host, method, route, None),
                Outcome::Untouched
            );
        }
        dispatch(&chain, &mut host, "GUILD_DELETE", GUILD_GONE);
        dispatch(&chain, &mut host, "RELATIONSHIP_REMOVE", FRIEND_GONE);
        dispatch(&chain, &mut host, "CHANNEL_RECIPIENT_REMOVE", GROUP_KICK);
        assert!(host.notices.is_empty(), "{:?}", host.notices);
    }

    #[test]
    fn settings_switch_kinds_off() {
        let (chain, mut host) = ready_chain();
        host.settings = vec![("friends", false), ("servers", false)];
        dispatch(&chain, &mut host, "RELATIONSHIP_REMOVE", FRIEND_GONE);
        dispatch(&chain, &mut host, "GUILD_DELETE", GUILD_GONE);
        assert!(host.notices.is_empty());
        dispatch(
            &chain,
            &mut host,
            "RELATIONSHIP_REMOVE",
            r#"{"op":0,"t":"RELATIONSHIP_REMOVE","d":{"id":"3","type":3}}"#,
        );
        assert_eq!(host.notices.len(), 1);
        assert_eq!(host.notices[0].0, "Friend request cancelled");
    }

    #[test]
    fn defaults_apply_with_null_host() {
        let chain = Chain::new(vec![Box::new(RelationshipNotifier::new())]);
        let mut host = NullHost::default();
        let mut p = json(r#"{"op":0,"t":"GUILD_DELETE","d":{"id":"77"}}"#);
        let mut extra = Vec::new();
        chain.dispatch(&mut host, "GUILD_DELETE", &mut p, &mut extra);
        assert_eq!(host.logs.len(), 1);
        assert!(host.logs[0].contains("server 77"));
    }
}
