//! PauseInvitesForever: pause invites to a server indefinitely (data part).
//!
//! Ported from Vencord `src/plugins/pauseInvitesForever/index.tsx` (GPL-3.0-or-later, Copyright
//! (c) 2023 Vendicated and contributors; authors Dolfies, amia). Vencord adds a "Pause
//! Indefinitely" link to the invites dialog. It reads the guild's `features`, adds
//! `INVITES_DISABLED`, and sends `PATCH /guilds/{id}` with the whole list. Here the same two calls
//! are made by a command, because the dialog is client UI (Phase 2).
//!
//! `/phoecord pauseinvites guild:<id or name> [action:pause|resume|status]`:
//! - `pause` (the default): `GET /guilds/{id}`, then `PATCH /guilds/{id}` with `INVITES_DISABLED`
//!   added to the features.
//! - `resume`: the same, with `INVITES_DISABLED` removed (Vencord only pauses; the Discord dialog
//!   has the switch for turning it back on, so the command gives the same choice).
//! - `status`: only reads.
//!
//! `guild` is a server id or a part of the server name (taken from the gateway). Nothing is sent
//! unless you run the command; Discord refuses with 403 when you lack the Manage Server
//! permission, and the reply says so. Only the plugin being on enables the command.
//!
//! Not ported: the link in the dialog and its permission check (Phase 2; here Discord decides).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Guilds, Shared, dispatch_data, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "pause_invites_forever";
const FEATURE: &str = "INVITES_DISABLED";

pub struct PauseInvitesForever {
    manifest: Manifest,
    guilds: Shared<Guilds>,
}

impl PauseInvitesForever {
    pub fn new() -> PauseInvitesForever {
        PauseInvitesForever {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "PauseInvitesForever",
                    env!("CARGO_PKG_VERSION"),
                    "Pause invites to a server indefinitely",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("PauseInvitesForever".into());
                m
            },
            guilds: Shared::default(),
        }
    }

    /// `/phoecord pauseinvites`. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let guilds = self.guilds.clone();
        FnCmd::new(
            PLUGIN_ID,
            "pauseinvites",
            "Pause, resume or check invites of a server",
            move |host, args| {
                let query = args.get("guild").unwrap_or("").trim().to_string();
                let action = args
                    .get("action")
                    .unwrap_or("pause")
                    .trim()
                    .to_ascii_lowercase();
                let id = match pick_guild(&guilds, &query) {
                    Ok(id) => id,
                    Err(e) => return e,
                };
                run(host, id, &action)
            },
        )
        .option("guild", "Server id or part of its name", true)
        .option("action", "pause (default), resume or status", false)
        .boxed()
    }
}

impl Default for PauseInvitesForever {
    fn default() -> Self {
        Self::new()
    }
}

/// A guild id from a number or a unique name part.
fn pick_guild(guilds: &Shared<Guilds>, query: &str) -> Result<u64, String> {
    if query.is_empty() {
        return Err(
            "Give a server id or part of its name: `/phoecord pauseinvites guild:<id>`.".into(),
        );
    }
    if let Ok(id) = query.parse::<u64>() {
        return Ok(id);
    }
    let q = query.to_lowercase();
    let g = guilds.lock();
    let hits: Vec<_> = g
        .guilds()
        .into_iter()
        .filter(|x| x.name.to_lowercase().contains(&q))
        .map(|x| (x.id, x.name.clone()))
        .collect();
    match hits.as_slice() {
        [] => Err(format!("No server matches `{query}`. Use the server id.")),
        [(id, _)] => Ok(*id),
        many => Err(format!(
            "Several servers match `{query}`: {}. Use the id.",
            many.iter()
                .take(5)
                .map(|(i, n)| format!("{n} ({i})"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn describe(status: u16) -> String {
    match status {
        403 => "Discord refused (403): you need the Manage Server permission.".into(),
        404 => "Discord does not know that server (404).".into(),
        s => format!("Discord answered {s}."),
    }
}

fn run(host: &mut dyn Host, id: u64, action: &str) -> String {
    if !matches!(action, "pause" | "resume" | "status") {
        return format!("Unknown action `{action}`. Use `pause`, `resume` or `status`.");
    }
    let route = format!("/guilds/{id}");
    let got = match host.rest("GET", &route, None) {
        Ok(r) if r.status == 200 => r,
        Ok(r) => return describe(r.status),
        Err(e) => return format!("Could not read the server: {e}"),
    };
    let mut features: Vec<Val> = got
        .body
        .as_ref()
        .and_then(|b| b.get("features"))
        .map(|f| f.as_list().to_vec())
        .unwrap_or_default();
    let paused = features.iter().any(|f| f.as_str() == Some(FEATURE));
    let name = got
        .body
        .as_ref()
        .and_then(|b| b.str_at(&["name"]))
        .unwrap_or("the server")
        .to_string();
    let want = match action {
        "status" => {
            return if paused {
                format!("Invites to {name} are paused.")
            } else {
                format!("Invites to {name} are open.")
            };
        }
        "pause" => true,
        _ => false,
    };
    if want == paused {
        return if paused {
            format!("Invites to {name} were already paused.")
        } else {
            format!("Invites to {name} were already open.")
        };
    }
    if want {
        features.push(Val::Str(FEATURE.into()));
    } else {
        features.retain(|f| f.as_str() != Some(FEATURE));
    }
    let mut body = Val::map();
    body.set("features", Val::List(features));
    match host.rest("PATCH", &route, Some(&body)) {
        Ok(r) if (200..300).contains(&r.status) => {
            if want {
                format!("Invites to {name} are now paused indefinitely.")
            } else {
                format!("Invites to {name} are open again.")
            }
        }
        Ok(r) => describe(r.status),
        Err(e) => format!("Could not change the server: {e}"),
    }
}

impl Plugin for PauseInvitesForever {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if is_one_of(name, Guilds::EVENTS))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event {
            self.guilds.lock().observe(name, dispatch_data(payload));
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn args(guild: &str, action: Option<&str>) -> Args {
        let mut options = vec![("guild".to_string(), guild.to_string())];
        if let Some(a) = action {
            options.push(("action".into(), a.into()));
        }
        Args { options }
    }

    const OPEN: &str = r#"{"id":"7","name":"Club","features":["COMMUNITY","NEWS"]}"#;
    const PAUSED: &str = r#"{"id":"7","name":"Club","features":["COMMUNITY","INVITES_DISABLED"]}"#;

    #[test]
    fn pause_adds_the_feature_and_keeps_the_rest() {
        let p = PauseInvitesForever::new();
        let mut h = Rec::default().answer("GET", "/guilds/7", 200, OPEN).answer(
            "PATCH",
            "/guilds/7",
            200,
            "{}",
        );
        let out = p.command().run(&mut h, &args("7", None));
        assert!(out.contains("paused indefinitely"), "{out}");
        let body = h.rest_calls[1].1.clone().unwrap();
        let f: Vec<_> = body
            .get("features")
            .unwrap()
            .as_list()
            .iter()
            .filter_map(Val::as_str)
            .collect();
        assert_eq!(f, ["COMMUNITY", "NEWS", "INVITES_DISABLED"]);
    }

    #[test]
    fn resume_and_status_and_noops() {
        let p = PauseInvitesForever::new();
        let mut h = Rec::default()
            .answer("GET", "/guilds/7", 200, PAUSED)
            .answer("PATCH", "/guilds/7", 200, "{}");
        let out = p.command().run(&mut h, &args("7", Some("status")));
        assert!(out.contains("are paused"));
        assert_eq!(h.rest_calls.len(), 1, "status does not write");
        let out = p.command().run(&mut h, &args("7", Some("pause")));
        assert!(out.contains("already paused"));
        let out = p.command().run(&mut h, &args("7", Some("resume")));
        assert!(out.contains("open again"), "{out}");
        let body = h.rest_calls.last().unwrap().1.clone().unwrap();
        assert_eq!(body.get("features").unwrap().as_list().len(), 1);
    }

    #[test]
    fn names_ids_errors_and_bad_input() {
        let p = PauseInvitesForever::new();
        let mut h = Rec::default().answer("GET", "/guilds/7", 403, "{}");
        let mut g = j(r#"{"id":"7","name":"Club House","channels":[]}"#);
        let mut extra = Vec::new();
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "GUILD_CREATE",
                payload: &mut g,
                extra: &mut extra,
            },
        );
        let cmd = p.command();
        assert!(
            cmd.run(&mut h, &args("club", None))
                .contains("Manage Server")
        );
        assert!(
            cmd.run(&mut h, &args("nothing", None))
                .contains("No server")
        );
        assert!(cmd.run(&mut h, &args("", None)).contains("Give a server"));
        assert!(
            cmd.run(&mut h, &args("7", Some("zap")))
                .contains("Unknown action")
        );
        assert_eq!(h.rest_calls.len(), 1, "only the first GET went out");
    }

    #[test]
    fn guild_names_come_from_a_real_gateway_frame() {
        use phoecord_plugin_runtime::Chain;
        let p = PauseInvitesForever::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default().answer("GET", "/guilds/7", 403, "{}");
        let mut frame = j(
            r#"{"op":0,"t":"GUILD_CREATE","s":1,"d":{"id":"7","name":"Club House","channels":[]}}"#,
        );
        chain.dispatch(&mut h, "GUILD_CREATE", &mut frame, &mut Vec::new());
        assert!(
            cmd.run(&mut h, &args("club", None))
                .contains("Manage Server")
        );
    }
}
