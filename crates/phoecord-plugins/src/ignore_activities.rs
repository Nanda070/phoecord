//! IgnoreActivities: keep chosen activities out of your status, for others only (Vencord:
//! IgnoreActivities).
//!
//! Ported from Vencord `src/plugins/ignoreActivities/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; authors Nuckyz, Kylie). Vencord filters the client's own
//! activity list before it becomes your presence; here the same filter runs on the presence the
//! client sends (identify, `op 2`, and updates, `op 3`), so you still see your game but others do
//! not. The rule is Vencord's `isActivityNotIgnored`, 1:1:
//!
//! 1. an activity whose application id is in `idsList` is hidden in `listMode` blacklist; in
//!    whitelist mode it goes on to the next steps;
//! 2. otherwise by type: `ignorePlaying` (0), `ignoreStreaming` (1), `ignoreListening` (2),
//!    `ignoreWatching` (3), `ignoreCompeting` (5), all off by default; a custom status is never
//!    hidden;
//! 3. an activity in `ignoredActivities` (`{ id, name, type }`, matched by application id, or by
//!    name for games without one, where Vencord matches the exe path) is hidden, unless whitelist
//!    mode lists its id in `idsList`.
//!
//! Settings (`[plugins.ignore_activities]`, Vencord's names): `listMode` (`"whitelist"`, the
//! default, or `"blacklist"`; Vencord's 0 and 1 work too), `idsList` (`"id, id"`), the five
//! `ignore*` switches, `ignoredActivities`. Vencord toggles activities from the Registered Games
//! tab; until Phase 2 that is `/phoecord ignoreactivity id:<application id or game name>` (kept in
//! the plugin's kv store, added to the setting's list), and `/phoecord activities` lists what your
//! last presence carried and what was hidden.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer};
use crate::presence::{self, activity_type, text};
use crate::state::Shared;
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "ignore_activities";
const KV_IGNORED: &str = "ignoredActivities";

pub struct IgnoreActivities {
    manifest: Manifest,
    kept: Kept,
    /// The activities of the last presence: `(activity, hidden)`.
    last: Shared<Vec<(Val, bool)>>,
}

impl IgnoreActivities {
    pub fn new() -> IgnoreActivities {
        IgnoreActivities {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "IgnoreActivities".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Keep chosen games and activities out of your status for others"
                    .into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("IgnoreActivities".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            kept: Kept::default(),
            last: Shared::default(),
        }
    }

    /// `/phoecord activities` and `/phoecord ignoreactivity`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        vec![
            kept_answer(
                PLUGIN_ID,
                "activities",
                "List the activities of your status and which ones are hidden",
                &self.kept,
            )
            .boxed(),
            kept_answer(
                PLUGIN_ID,
                "ignoreactivity",
                "Hide an activity from your status, or show it again",
                &self.kept,
            )
            .option(
                "id",
                "Application id or game name (see /phoecord activities)",
                true,
            )
            .boxed(),
        ]
    }
}

impl Default for IgnoreActivities {
    fn default() -> Self {
        Self::new()
    }
}

/// The settings, read once per presence.
pub struct Rules {
    blacklist: bool,
    ids: Vec<String>,
    by_type: [bool; 6],
    /// `(id, name)` of each ignored activity.
    ignored: Vec<(String, String)>,
}

fn flag(host: &dyn Host, key: &str) -> bool {
    host.setting(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn ignored_list(v: Option<Val>) -> Vec<(String, String)> {
    v.map(|v| v.as_list().to_vec())
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            let id = text(a, "id").unwrap_or_default();
            let name = text(a, "name").unwrap_or_default();
            (!id.is_empty() || !name.is_empty()).then_some((id, name))
        })
        .collect()
}

impl Rules {
    pub fn read(host: &dyn Host) -> Rules {
        let blacklist = match host.setting("listMode") {
            Some(Val::Str(s)) => s.eq_ignore_ascii_case("blacklist"),
            Some(v) => v.as_i64() == Some(1),
            None => false,
        };
        let ids = host
            .setting("idsList")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mut ignored = ignored_list(host.setting("ignoredActivities"));
        ignored.extend(ignored_list(host.kv_get(KV_IGNORED)));
        Rules {
            blacklist,
            ids,
            by_type: [
                flag(host, "ignorePlaying"),
                flag(host, "ignoreStreaming"),
                flag(host, "ignoreListening"),
                flag(host, "ignoreWatching"),
                false,
                flag(host, "ignoreCompeting"),
            ],
            ignored,
        }
    }

    /// Vencord's `isActivityTypeIgnored`.
    fn type_ignored(&self, kind: i64, app: Option<&str>) -> bool {
        if let Some(id) = app
            && self.ids.iter().any(|i| i == id)
        {
            return self.blacklist;
        }
        usize::try_from(kind)
            .ok()
            .and_then(|k| self.by_type.get(k).copied())
            .unwrap_or(false)
    }

    /// Vencord's `isActivityNotIgnored`, negated.
    pub fn hides(&self, activity: &Val) -> bool {
        let kind = activity_type(activity).unwrap_or(-1);
        if kind == presence::CUSTOM_STATUS {
            return false;
        }
        let app = text(activity, "application_id").filter(|a| !a.is_empty() && a != "0");
        if self.type_ignored(kind, app.as_deref()) {
            return true;
        }
        match app {
            Some(id) => {
                let listed = self.ignored.iter().any(|(i, _)| *i == id);
                let whitelisted = !self.blacklist && self.ids.contains(&id);
                listed && !whitelisted
            }
            None => {
                let name = text(activity, "name").unwrap_or_default();
                self.ignored
                    .iter()
                    .any(|(i, n)| !name.is_empty() && (*n == name || *i == name))
            }
        }
    }
}

impl IgnoreActivities {
    fn filter(&self, host: &dyn Host, presence: &mut Val) -> bool {
        let rules = Rules::read(host);
        let Some(list) = presence::activities_mut(presence) else {
            return false;
        };
        let seen: Vec<(Val, bool)> = list.iter().map(|a| (a.clone(), rules.hides(a))).collect();
        let before = list.len();
        list.retain(|a| !rules.hides(a));
        let changed = list.len() != before;
        *self.last.lock() = seen;
        changed
    }

    fn list(&self, host: &dyn Host) -> String {
        let last = self.last.lock();
        if last.is_empty() {
            return host.t("ignore_activities-none", &[]);
        }
        last.iter()
            .map(|(a, hidden)| {
                let name = text(a, "name").unwrap_or_else(|| "?".into());
                let id = text(a, "application_id").unwrap_or_else(|| "-".into());
                let kind = activity_type(a).unwrap_or(-1).to_string();
                host.t(
                    if *hidden {
                        "ignore_activities-line-hidden"
                    } else {
                        "ignore_activities-line-shown"
                    },
                    &[("name", &name), ("id", &id), ("type", &kind)],
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn toggle(&self, host: &mut dyn Host, id: &str) -> String {
        let mut list = host
            .kv_get(KV_IGNORED)
            .map_or_else(Vec::new, |v| v.as_list().to_vec());
        let before = list.len();
        list.retain(|a| {
            text(a, "id").as_deref() != Some(id) && text(a, "name").as_deref() != Some(id)
        });
        let removed = list.len() != before;
        if !removed {
            let name = self
                .last
                .lock()
                .iter()
                .map(|(a, _)| a)
                .find(|a| text(a, "application_id").as_deref() == Some(id))
                .and_then(|a| text(a, "name"))
                .unwrap_or_else(|| id.to_string());
            let mut entry = Val::map();
            entry.set("id", Val::Str(id.to_string()));
            entry.set("name", Val::Str(name));
            entry.set("type", Val::Int(0));
            list.push(entry);
        }
        host.kv_set(KV_IGNORED, Val::List(list));
        host.t(
            if removed {
                "ignore_activities-shown-again"
            } else {
                "ignore_activities-hidden"
            },
            &[("id", id)],
        )
    }
}

impl Plugin for IgnoreActivities {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Send { op } => presence::carries_presence(*op),
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if presence::carries_presence(op) => {
                let changed = presence::presence_mut(payload).is_some_and(|p| self.filter(host, p));
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if let Some(inv) = invocation(body) {
                    let answer = match inv.sub.as_str() {
                        "activities" => self.list(host),
                        "ignoreactivity" => match inv.get("id") {
                            Some(id) => self.toggle(host, id),
                            None => host.t("ignore_activities-need-id", &[]),
                        },
                        _ => return Verdict::Pass,
                    };
                    *self.kept.lock() = Some(answer);
                }
                Verdict::Pass
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    const PRESENCE: &str = r#"{"op":3,"d":{"status":"online","activities":[
        {"type":4,"name":"Custom Status","state":"hi"},
        {"type":0,"name":"Game","application_id":"111"},
        {"type":0,"name":"Local Game"},
        {"type":2,"name":"Spotify"},
        {"type":3,"name":"Show","application_id":"333"}]}}"#;

    fn names(h: &mut Rec) -> Vec<String> {
        let p = IgnoreActivities::new();
        let mut f = j(PRESENCE);
        p.on_event(
            h,
            Event::Send {
                op: 3,
                payload: &mut f,
            },
        );
        f.at(&["d", "activities"])
            .unwrap()
            .as_list()
            .iter()
            .map(|a| text(a, "name").unwrap())
            .collect()
    }

    fn s(v: &str) -> Val {
        Val::Str(v.into())
    }

    #[test]
    fn nothing_is_hidden_by_default() {
        assert_eq!(names(&mut Rec::default()).len(), 5);
    }

    #[test]
    fn type_switches_never_touch_the_custom_status() {
        let mut h = Rec::default()
            .with("ignorePlaying", Val::Bool(true))
            .with("ignoreListening", Val::Bool(true));
        assert_eq!(names(&mut h), vec!["Custom Status", "Show"]);
    }

    #[test]
    fn ids_list_in_both_modes_and_ignored_activities() {
        // Blacklist: an id in the list is hidden.
        let mut h = Rec::default()
            .with("listMode", s("blacklist"))
            .with("idsList", s("333, 999"));
        assert_eq!(
            names(&mut h),
            vec!["Custom Status", "Game", "Local Game", "Spotify"]
        );
        // Whitelist: a listed id survives even `ignorePlaying` and `ignoredActivities`.
        let mut h = Rec::default()
            .with("idsList", s("111"))
            .with("ignorePlaying", Val::Bool(true))
            .with(
                "ignoredActivities",
                j(r#"[{"id":"111","name":"Game","type":0},{"id":"333","name":"Show","type":1}]"#),
            );
        assert_eq!(names(&mut h), vec!["Custom Status", "Game", "Spotify"]);
        // A game without an application id matches by name.
        let mut h = Rec::default().with(
            "ignoredActivities",
            j(r#"[{"id":"C:\\Games\\local.exe","name":"Local Game","type":0}]"#),
        );
        assert_eq!(names(&mut h).len(), 4);
        // Vencord's numeric listMode: 1 is blacklist.
        let mut h = Rec::default()
            .with("listMode", Val::Int(1))
            .with("idsList", s("111"));
        assert!(!names(&mut h).contains(&"Game".to_string()));
    }

    #[test]
    fn commands_list_and_toggle() {
        let p = IgnoreActivities::new();
        let mut h = Rec::default();
        let run = |h: &mut Rec, sub: &str, opts: &str| {
            let mut body = j(&format!(
                r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"{sub}","options":[{opts}]}}]}}}}"#
            ));
            p.on_event(
                h,
                Event::RestRequest {
                    method: "POST",
                    route: "/interactions",
                    body: Some(&mut body),
                },
            );
            let cmds = p.commands();
            cmds.iter()
                .find(|c| c.name() == sub)
                .unwrap()
                .run(h, &Args { options: vec![] })
        };
        assert!(run(&mut h, "activities", "").contains("No presence"));
        let mut f = j(PRESENCE);
        p.on_event(
            &mut h,
            Event::Send {
                op: 3,
                payload: &mut f,
            },
        );
        let list = run(&mut h, "activities", "");
        assert!(list.contains("Game (`111`, type 0): shown"), "{list}");
        let out = run(
            &mut h,
            "ignoreactivity",
            r#"{"type":3,"name":"id","value":"111"}"#,
        );
        assert!(out.contains("hidden"), "{out}");
        let mut f = j(PRESENCE);
        assert_eq!(
            p.on_event(
                &mut h,
                Event::Send {
                    op: 3,
                    payload: &mut f
                }
            ),
            Verdict::Changed
        );
        assert_eq!(f.at(&["d", "activities"]).unwrap().as_list().len(), 4);
        assert!(run(&mut h, "activities", "").contains("Game (`111`, type 0): hidden"));
        let out = run(
            &mut h,
            "ignoreactivity",
            r#"{"type":3,"name":"id","value":"111"}"#,
        );
        assert!(out.contains("shown again"), "{out}");
    }
}
