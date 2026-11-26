//! NewGuildSettings: mute and tune servers as soon as you join them (Vencord: NewGuildSettings).
//!
//! Ported from Vencord `src/plugins/newGuildSettings/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors Glitch, Nuckyz, carince, Mopi, GabiRP). Vencord hooks
//! the "invite accepted" and "join guild" code and runs `applyDefaultSettings`. Here the join is
//! seen on the gateway: a `GUILD_CREATE` for a guild that was not in `READY`, or a
//! `GUILD_JOIN_REQUEST_UPDATE` with status `APPROVED` for you (a server with membership screening
//! or applications). Servers you created yourself and lurked servers are skipped.
//!
//! What it writes: one `PATCH /users/@me/guilds/{id}/settings` per new server, only while the plugin
//! is on, with the fields Vencord sets: `muted`, `suppress_everyone`, `suppress_roles`,
//! `mute_scheduled_events`, `notify_highlights`, and `message_notifications` unless it is the
//! "server default" choice. Vencord sends two calls; they are merged here to stay within the host's
//! per-plugin Discord call budget (5 per 5 s). A server is handled once (remembered in the plugin's
//! store); a failed call is logged and not repeated by itself.
//!
//! Settings (`[plugins.new_guild_settings]`), names and defaults as in Vencord: `guild` (true,
//! mute the server), `messages` (0 all, 1 only mentions, 2 nothing, 3 server default = default),
//! `everyone` (true), `role` (true), `highlights` (true), `events` (true).
//!
//! `/phoecord newguildsettings <guild_id>` applies the same settings to a server you are already
//! in (Vencord's "Apply NewGuildSettings" menu item).
//!
//! Not ported: `showAllChannels` (the "show all channels" toggle of community servers goes through
//! the client's onboarding module; there is no stable REST call for it).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashSet;

const PLUGIN_ID: &str = "new_guild_settings";
const KV_APPLIED: &str = "applied";
const KV_CAP: usize = 200;
const EVENTS: &[&str] = &["READY", "GUILD_CREATE", "GUILD_JOIN_REQUEST_UPDATE"];

#[derive(Default)]
struct State {
    /// Guild ids from `READY` and later `GUILD_CREATE`s.
    known: HashSet<u64>,
    ready: bool,
}

pub struct NewGuildSettings {
    manifest: Manifest,
    state: Shared<State>,
}

impl NewGuildSettings {
    pub fn new() -> NewGuildSettings {
        NewGuildSettings {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "NewGuildSettings",
                    env!("CARGO_PKG_VERSION"),
                    "Automatically mute new servers and change other settings on joining",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("NewGuildSettings".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// `/phoecord newguildsettings <guild_id>`. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        FnCmd::new(
            PLUGIN_ID,
            "newguildsettings",
            "Apply your new-server notification settings to a server now",
            |host, args| {
                let Some(id) = args.get("guild").and_then(|g| g.trim().parse::<u64>().ok()) else {
                    return "Give the server id: `/phoecord newguildsettings guild:<id>`.".into();
                };
                match apply(host, id) {
                    Ok(()) => format!("Applied your new-server settings to {id}."),
                    Err(e) => format!("Could not apply the settings to {id}: {e}"),
                }
            },
        )
        .option("guild", "Server id", true)
        .boxed()
    }
}

impl Default for NewGuildSettings {
    fn default() -> Self {
        Self::new()
    }
}

fn flag(host: &dyn Host, key: &str) -> bool {
    host.setting(key).and_then(|v| v.as_bool()).unwrap_or(true)
}

/// The `PATCH` body for the current settings.
fn settings_body(host: &dyn Host) -> Val {
    let mut b = Val::map();
    b.set("muted", Val::Bool(flag(host, "guild")));
    b.set("suppress_everyone", Val::Bool(flag(host, "everyone")));
    b.set("suppress_roles", Val::Bool(flag(host, "role")));
    b.set("mute_scheduled_events", Val::Bool(flag(host, "events")));
    b.set(
        "notify_highlights",
        Val::Int(i64::from(flag(host, "highlights"))),
    );
    // 3 is "server default": leave `message_notifications` alone.
    if let Some(m) = host
        .setting("messages")
        .and_then(|v| v.as_i64())
        .filter(|m| (0..=2).contains(m))
    {
        b.set("message_notifications", Val::Int(m));
    }
    b
}

fn apply(host: &mut dyn Host, guild: u64) -> Result<(), String> {
    let body = settings_body(host);
    let r = host.rest(
        "PATCH",
        &format!("/users/@me/guilds/{guild}/settings"),
        Some(&body),
    )?;
    if (200..300).contains(&r.status) {
        Ok(())
    } else {
        Err(format!("Discord answered {}", r.status))
    }
}

fn applied(host: &dyn Host) -> Vec<u64> {
    host.kv_get(KV_APPLIED)
        .map(|v| v.as_list().iter().filter_map(Val::snowflake).collect())
        .unwrap_or_default()
}

fn remember(host: &mut dyn Host, mut ids: Vec<u64>, id: u64) {
    ids.push(id);
    let skip = ids.len().saturating_sub(KV_CAP);
    let list = ids[skip..]
        .iter()
        .map(|i| Val::Str(i.to_string()))
        .collect();
    host.kv_set(KV_APPLIED, Val::List(list));
}

/// Applies the settings once to a joined guild.
fn handle_join(host: &mut dyn Host, guild: u64) {
    let done = applied(host);
    if done.contains(&guild) {
        return;
    }
    match apply(host, guild) {
        Ok(()) => {
            host.log(&format!("applied new-server settings to {guild}"));
            remember(host, done, guild);
        }
        Err(e) => host.log(&format!("new-server settings for {guild} failed: {e}")),
    }
}

impl Plugin for NewGuildSettings {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if is_one_of(name, EVENTS))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch { name, payload, .. } = event else {
            return Verdict::Pass;
        };
        let payload = dispatch_data(payload);
        match name {
            "READY" => {
                let mut st = self.state.lock();
                for g in payload.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    if let Some(id) = g.id_at(&["id"]) {
                        st.known.insert(id);
                    }
                }
                st.ready = true;
            }
            "GUILD_CREATE" => {
                let Some(id) = payload.id_at(&["id"]) else {
                    return Verdict::Pass;
                };
                let fresh = {
                    let mut st = self.state.lock();
                    let new = st.ready && st.known.insert(id);
                    // An outage, a guild you made and a lurked guild are no joins.
                    new && payload.get("unavailable").and_then(Val::as_bool) != Some(true)
                        && payload.get("lurker").and_then(Val::as_bool) != Some(true)
                        && payload
                            .id_at(&["owner_id"])
                            .is_none_or(|o| Some(o) != host.self_id())
                };
                if fresh {
                    handle_join(host, id);
                }
            }
            "GUILD_JOIN_REQUEST_UPDATE" => {
                let approved = payload.str_at(&["status"]) == Some("APPROVED");
                let mine = payload
                    .id_at(&["request", "user_id"])
                    .is_some_and(|u| Some(u) == host.self_id());
                if let (true, true, Some(id)) = (approved, mine, payload.id_at(&["guild_id"])) {
                    handle_join(host, id);
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
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn dispatch(p: &NewGuildSettings, h: &mut Rec, name: &str, payload: &str) {
        let mut payload = j(payload);
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name,
                payload: &mut payload,
                extra: &mut extra,
            },
        );
    }

    fn rec() -> Rec {
        let mut h = Rec::default().answer("PATCH", "/users/@me/guilds/9/settings", 200, "{}");
        h.self_id = Some(1);
        h
    }

    #[test]
    fn new_guild_is_muted_once() {
        let p = NewGuildSettings::new();
        let mut h = rec().with("messages", Val::Int(1));
        dispatch(&p, &mut h, "READY", r#"{"guilds":[{"id":"5"}]}"#);
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"5","name":"old"}"#);
        assert!(h.rest_calls.is_empty(), "known guild is not new");
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"9","owner_id":"2"}"#);
        assert_eq!(h.rest_calls.len(), 1);
        let (call, body) = &h.rest_calls[0];
        assert_eq!(call, "PATCH /users/@me/guilds/9/settings");
        let body = body.as_ref().unwrap();
        assert_eq!(body.get("muted"), Some(&Val::Bool(true)));
        assert_eq!(body.get("notify_highlights"), Some(&Val::Int(1)));
        assert_eq!(body.get("message_notifications"), Some(&Val::Int(1)));
        // The same guild again (reconnect): nothing more.
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"9"}"#);
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn new_guild_is_muted_from_real_gateway_frames() {
        use phoecord_plugin_runtime::Chain;
        let chain = Chain::new(vec![Box::new(NewGuildSettings::new())]);
        let mut h = rec().with("messages", Val::Int(1));
        let mut send = |name: &str, d: &str| {
            let mut frame = j(&format!(r#"{{"op":0,"t":"{name}","s":1,"d":{d}}}"#));
            chain.dispatch(&mut h, name, &mut frame, &mut Vec::new());
        };
        send("READY", r#"{"guilds":[{"id":"5"}]}"#);
        send("GUILD_CREATE", r#"{"id":"5","name":"old"}"#);
        send("GUILD_CREATE", r#"{"id":"9","owner_id":"2"}"#);
        assert_eq!(h.rest_calls.len(), 1);
        assert_eq!(h.rest_calls[0].0, "PATCH /users/@me/guilds/9/settings");
    }

    #[test]
    fn own_lurked_unavailable_and_early_guilds_are_skipped() {
        let p = NewGuildSettings::new();
        let mut h = rec();
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"9"}"#);
        assert!(h.rest_calls.is_empty(), "before READY nothing is new");
        dispatch(&p, &mut h, "READY", r#"{"guilds":[]}"#);
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"20","owner_id":"1"}"#);
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"21","lurker":true}"#);
        dispatch(
            &p,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"22","unavailable":true}"#,
        );
        assert!(h.rest_calls.is_empty());
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"bad":true}"#);
        assert!(h.rest_calls.is_empty());
    }

    #[test]
    fn approved_request_and_server_default_messages() {
        let p = NewGuildSettings::new();
        let mut h = rec().with("guild", Val::Bool(false));
        dispatch(
            &p,
            &mut h,
            "GUILD_JOIN_REQUEST_UPDATE",
            r#"{"guild_id":"9","status":"PENDING","request":{"user_id":"1"}}"#,
        );
        assert!(h.rest_calls.is_empty());
        dispatch(
            &p,
            &mut h,
            "GUILD_JOIN_REQUEST_UPDATE",
            r#"{"guild_id":"9","status":"APPROVED","request":{"user_id":"1"}}"#,
        );
        let body = h.rest_calls[0].1.clone().unwrap();
        assert_eq!(body.get("muted"), Some(&Val::Bool(false)));
        assert!(
            body.get("message_notifications").is_none(),
            "default 3 = untouched"
        );
    }

    #[test]
    fn failed_patch_is_not_remembered_and_command_reports() {
        let p = NewGuildSettings::new();
        let mut h = Rec::default().answer("PATCH", "/users/@me/guilds/9/settings", 403, "{}");
        dispatch(&p, &mut h, "READY", r#"{"guilds":[]}"#);
        dispatch(&p, &mut h, "GUILD_CREATE", r#"{"id":"9"}"#);
        assert!(h.kv.is_empty());
        let cmd = p.command();
        let a = Args {
            options: vec![("guild".into(), "9".into())],
        };
        assert!(cmd.run(&mut h, &a).contains("403"));
        assert!(
            cmd.run(&mut h, &Args { options: vec![] })
                .contains("server id")
        );
    }
}
