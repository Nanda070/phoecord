//! ImplicitRelationships: people you talk to a lot but are not friends with (Vencord:
//! ImplicitRelationships).
//!
//! Ported from Vencord `src/plugins/implicitRelationships/index.ts` (GPL-3.0-or-later, Copyright
//! (c) 2023 Vendicated and contributors; author Dolfies). Vencord reads the user's affinities
//! (`GET /users/@me/affinities/users`), keeps those the user has no relationship with, and shows
//! them in a new "Implicit" section of the Friends tab, sorted by communication rank. This is the
//! data part: `/phoecord implicit` does the same call through `Host::rest` and lists the result.
//!
//! "No relationship" is decided from what the gateway showed: the plugin remembers the user ids
//! of `READY.relationships` and follows `RELATIONSHIP_ADD` / `RELATIONSHIP_REMOVE` (friends,
//! pending requests and blocked users all count as a relationship). Names come from the shared
//! `state::Users` cache; people it has not seen show as `<@id>`. Vencord looks the missing ones
//! up with gateway op 8; there is no such call in the Plugin API.
//!
//! Switching it on: `[plugins.implicit_relationships] enabled = true`, or
//! `/phoecord toggle implicit_relationships`. Setting `sortByAffinity` (default `true`, as
//! Vencord) lists the strongest affinity first.
//!
//! Not ported: the "Implicit" tab, its counter and the Friends empty state (Phase 2 UI hooks).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, Users, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::BTreeSet;

const PLUGIN_ID: &str = "implicit_relationships";
/// People shown by one command.
const SHOW: usize = 40;
/// Bound on the remembered relationship ids (Discord caps friends at a few thousand).
const MAX_RELATIONSHIPS: usize = 20_000;

#[derive(Default)]
struct State {
    users: Users,
    /// User ids with any relationship to the signed-in user.
    related: BTreeSet<u64>,
    /// Whether a `READY` was seen, so `related` is meaningful.
    ready: bool,
}

const OWN_EVENTS: &[&str] = &["READY", "RELATIONSHIP_ADD", "RELATIONSHIP_REMOVE"];

fn interesting(name: &str) -> bool {
    is_one_of(name, OWN_EVENTS) || is_one_of(name, Users::EVENTS)
}

pub struct ImplicitRelationships {
    manifest: Manifest,
    state: Shared<State>,
}

impl ImplicitRelationships {
    pub fn new() -> ImplicitRelationships {
        ImplicitRelationships {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ImplicitRelationships",
                    env!("CARGO_PKG_VERSION"),
                    "Lists people you talk to a lot but are not friends with",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ImplicitRelationships".into());
                m
            },
            state: Shared::new(State::default()),
        }
    }

    /// The `/phoecord implicit` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "implicit",
            "List people you talk to a lot but are not friends with",
            move |host, _| run(host, &state),
        )
        .boxed()
    }
}

impl Default for ImplicitRelationships {
    fn default() -> Self {
        Self::new()
    }
}

/// The user an affinity row is about, and how strong it is. Discord's field names differ between
/// the REST answer (`user_id`) and the client's store (`otherUserId`); both are accepted.
fn affinity_of(row: &Val) -> Option<(u64, f64)> {
    let id = ["user_id", "other_user_id", "otherUserId"]
        .iter()
        .find_map(|k| row.id_at(&[k]))?;
    let score = ["affinity", "communication_rank", "communicationRank"]
        .iter()
        .find_map(|k| match row.get(k)? {
            Val::Int(n) => Some(*n as f64),
            Val::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(0.0);
    Some((id, score))
}

fn relationship_id(r: &Val) -> Option<u64> {
    r.id_at(&["user_id"])
        .or_else(|| r.id_at(&["id"]))
        .or_else(|| r.id_at(&["user", "id"]))
}

impl State {
    fn observe(&mut self, name: &str, d: &Val) {
        self.users.observe(name, d);
        match name {
            "READY" => {
                self.related.clear();
                for r in d.get("relationships").map(Val::as_list).unwrap_or(&[]) {
                    if self.related.len() < MAX_RELATIONSHIPS
                        && let Some(id) = relationship_id(r)
                    {
                        self.related.insert(id);
                    }
                }
                self.ready = true;
            }
            "RELATIONSHIP_ADD" => {
                if self.related.len() < MAX_RELATIONSHIPS
                    && let Some(id) = relationship_id(d)
                {
                    self.related.insert(id);
                }
            }
            "RELATIONSHIP_REMOVE" => {
                if let Some(id) = relationship_id(d) {
                    self.related.remove(&id);
                }
            }
            _ => {}
        }
    }
}

fn run(host: &mut dyn Host, state: &Shared<State>) -> String {
    let r = match host.rest("GET", "/users/@me/affinities/users", None) {
        Ok(r) => r,
        Err(e) => return format!("Could not read your affinities: {e}"),
    };
    if r.status != 200 {
        return format!(
            "Could not read your affinities: Discord answered {}.",
            r.status
        );
    }
    let Some(body) = r.body else {
        return "Could not read your affinities: empty answer.".into();
    };
    let sort = host.setting("sortByAffinity").and_then(|v| v.as_bool()) != Some(false);
    let me = host.self_id();
    let st = state.lock();
    let mut rows: Vec<(u64, f64)> = body
        .get("user_affinities")
        .map(Val::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(affinity_of)
        .filter(|(id, _)| Some(*id) != me && !st.related.contains(id))
        .collect();
    if sort {
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    let mut out = if rows.is_empty() {
        "No implicit relationships: everyone you talk to a lot is already on your list.".to_string()
    } else {
        format!("**{} implicit relationships**", rows.len())
    };
    for (id, score) in rows.iter().take(SHOW) {
        out.push_str(&format!(
            "\n- {} (affinity {score:.2})",
            st.users.display(*id)
        ));
    }
    if rows.len() > SHOW {
        out.push_str(&format!("\n...and {} more", rows.len() - SHOW));
    }
    if !st.ready {
        out.push_str(
            "\nNote: your friends list has not been seen yet (reload the client so READY passes \
             through PhoeCord), so friends may be listed here too.",
        );
    }
    out
}

impl Plugin for ImplicitRelationships {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if interesting(name))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event
            && interesting(name)
            && let Some(d) = payload.get("d")
        {
            self.state.lock().observe(name, d);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn feed(p: &ImplicitRelationships, name: &str, json: &str) -> Verdict {
        let mut payload = j(json);
        let mut extra = Vec::new();
        p.on_event(
            &mut Rec::default(),
            Event::Dispatch {
                name,
                payload: &mut payload,
                extra: &mut extra,
            },
        )
    }

    const AFF: &str = r#"{"user_affinities":[
        {"user_id":"10","affinity":0.2},{"user_id":"20","affinity":0.9},{"user_id":"30","affinity":0.5}],
        "inverse_user_affinities":[]}"#;

    fn run_cmd(p: &ImplicitRelationships, h: &mut Rec) -> String {
        p.command().run(h, &Args { options: vec![] })
    }

    #[test]
    fn lists_non_friends_sorted_with_names() {
        let p = ImplicitRelationships::new();
        // 10 is a friend, 20 and 30 are strangers; 20's name is known from a message.
        feed(
            &p,
            "READY",
            r#"{"d":{"user":{"id":"1","username":"me"},"relationships":[{"id":"10","type":1,"user":{"id":"10","username":"pal"}}]}}"#,
        );
        feed(
            &p,
            "MESSAGE_CREATE",
            r#"{"d":{"author":{"id":"20","username":"zed","global_name":"Zed"}}}"#,
        );
        let mut h = Rec::default().answer("GET", "/users/@me/affinities/users", 200, AFF);
        let out = run_cmd(&p, &mut h);
        assert!(out.contains("2 implicit relationships"), "{out}");
        let (zed, other) = (out.find("Zed").unwrap(), out.find("<@30>").unwrap());
        assert!(zed < other, "strongest first: {out}");
        assert!(!out.contains("<@10>") && !out.contains("pal"));
        assert!(!out.contains("has not been seen"));
    }

    #[test]
    fn relationship_events_update_the_set_and_sort_can_be_off() {
        let p = ImplicitRelationships::new();
        feed(&p, "READY", r#"{"d":{"relationships":[]}}"#);
        feed(&p, "RELATIONSHIP_ADD", r#"{"d":{"id":"20","type":1}}"#);
        feed(&p, "RELATIONSHIP_ADD", r#"{"d":{"id":"30","type":3}}"#);
        feed(&p, "RELATIONSHIP_REMOVE", r#"{"d":{"id":"30","type":3}}"#);
        let mut h = Rec::default()
            .with("sortByAffinity", Val::Bool(false))
            .answer("GET", "/users/@me/affinities/users", 200, AFF);
        let out = run_cmd(&p, &mut h);
        assert!(out.contains("2 implicit"), "{out}");
        let (a, b) = (out.find("<@10>").unwrap(), out.find("<@30>").unwrap());
        assert!(a < b, "API order kept: {out}");
        assert!(!out.contains("<@20>"));
    }

    #[test]
    fn warns_before_ready_and_survives_bad_answers() {
        let p = ImplicitRelationships::new();
        let mut h = Rec::default().answer("GET", "/users/@me/affinities/users", 200, AFF);
        assert!(run_cmd(&p, &mut h).contains("has not been seen"));
        for (status, body) in [
            (200, "{}"),
            (200, r#"{"user_affinities":[1,{"x":2},null]}"#),
            (403, "{}"),
        ] {
            let mut h = Rec::default().answer("GET", "/users/@me/affinities/users", status, body);
            let _ = run_cmd(&p, &mut h);
        }
        // Malformed gateway payloads are ignored.
        assert_eq!(
            feed(&p, "READY", r#"{"d":{"relationships":"no"}}"#),
            Verdict::Pass
        );
        assert_eq!(feed(&p, "RELATIONSHIP_ADD", r#"{"x":1}"#), Verdict::Pass);
    }

    #[test]
    fn disabled_plugin_does_not_call_rest() {
        let p = ImplicitRelationships::new();
        let mut h = Rec::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(run_cmd(&p, &mut h).contains("is off"));
        assert!(h.rest_calls.is_empty());
    }
}
