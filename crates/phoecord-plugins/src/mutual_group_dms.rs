//! MutualGroupDMs: the group DMs you share with a user (Vencord: MutualGroupDMs).
//!
//! Ported from Vencord `src/plugins/mutualGroupDMs/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; author amia). Vencord filters the client's private channels
//! for group DMs (type 3) whose recipients include the profile's user and lists them in a new
//! profile tab. This is the data part: the plugin keeps its own copy of your group DMs from
//! `READY`, `CHANNEL_CREATE`, `CHANNEL_UPDATE`, `CHANNEL_DELETE`, `CHANNEL_RECIPIENT_ADD` and
//! `CHANNEL_RECIPIENT_REMOVE` (ids, names and recipient ids only), and
//! `/phoecord mutualgdms <user>` answers the question.
//!
//! Switching it on: `[plugins.mutual_group_dms] enabled = true`, or
//! `/phoecord toggle mutual_group_dms`. It only knows group DMs it saw arrive, so switch it on
//! and reload the client (so `READY` passes through PhoeCord) before asking.
//!
//! `<user>` is a user id or a mention (`<@123>`): the Plugin API has no way to search users by
//! name. A group DM with no name of its own is shown by its members' names (from `state::Users`,
//! or `<@id>` for people not seen yet), as Vencord does.
//!
//! Not ported: the profile tab, the "N Mutual Groups" label and click-to-open (Phase 2 UI hooks).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, Users, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::BTreeMap;

const PLUGIN_ID: &str = "mutual_group_dms";
/// Channel type of a group DM.
const GROUP_DM: i64 = 3;
/// Bound on tracked group DMs (Discord limits how many a user can be in).
const MAX_GROUPS: usize = 2000;
/// Lines shown by one command.
const SHOW: usize = 25;

const OWN_EVENTS: &[&str] = &[
    "READY",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
    "CHANNEL_RECIPIENT_ADD",
    "CHANNEL_RECIPIENT_REMOVE",
];

fn interesting(name: &str) -> bool {
    is_one_of(name, OWN_EVENTS) || is_one_of(name, Users::EVENTS)
}

struct Group {
    name: Option<String>,
    /// Everyone but the signed-in user.
    recipients: Vec<u64>,
}

#[derive(Default)]
struct State {
    users: Users,
    groups: BTreeMap<u64, Group>,
    ready: bool,
}

/// Recipient ids of a channel object: `recipient_ids` (list of ids) or `recipients` (user objects).
fn recipients_of(ch: &Val) -> Option<Vec<u64>> {
    if let Some(ids) = ch.get("recipient_ids") {
        return Some(ids.as_list().iter().filter_map(Val::snowflake).collect());
    }
    ch.get("recipients").map(|l| {
        l.as_list()
            .iter()
            .filter_map(|u| u.id_at(&["id"]).or_else(|| u.snowflake()))
            .collect()
    })
}

fn group_name(ch: &Val) -> Option<String> {
    ch.str_at(&["name"])
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl State {
    fn learn_recipients(&mut self, ch: &Val) {
        for u in ch.get("recipients").map(Val::as_list).unwrap_or(&[]) {
            self.users.learn(u);
        }
    }

    fn put(&mut self, ch: &Val) {
        if ch.get("type").and_then(Val::as_i64) != Some(GROUP_DM) {
            return;
        }
        let Some(id) = ch.id_at(&["id"]) else { return };
        self.learn_recipients(ch);
        let known = self.groups.contains_key(&id);
        if !known && self.groups.len() >= MAX_GROUPS {
            return;
        }
        let name = group_name(ch);
        let recipients = recipients_of(ch);
        match self.groups.get_mut(&id) {
            // An update may carry only some fields: keep what it does not mention.
            Some(g) => {
                g.name = name;
                if let Some(r) = recipients {
                    g.recipients = r;
                }
            }
            None => {
                self.groups.insert(
                    id,
                    Group {
                        name,
                        recipients: recipients.unwrap_or_default(),
                    },
                );
            }
        }
    }

    fn observe(&mut self, name: &str, d: &Val) {
        self.users.observe(name, d);
        match name {
            "READY" => {
                self.groups.clear();
                for ch in d.get("private_channels").map(Val::as_list).unwrap_or(&[]) {
                    self.put(ch);
                }
                self.ready = true;
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => self.put(d),
            "CHANNEL_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.groups.remove(&id);
                }
            }
            "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => {
                let (Some(ch), Some(user)) = (d.id_at(&["channel_id"]), d.id_at(&["user", "id"]))
                else {
                    return;
                };
                if let Some(u) = d.get("user") {
                    self.users.learn(u);
                }
                if let Some(g) = self.groups.get_mut(&ch) {
                    g.recipients.retain(|r| *r != user);
                    if name == "CHANNEL_RECIPIENT_ADD" {
                        g.recipients.push(user);
                    }
                }
            }
            _ => {}
        }
    }

    fn title(&self, g: &Group) -> String {
        match &g.name {
            Some(n) => n.clone(),
            None => g
                .recipients
                .iter()
                .map(|r| self.users.display(*r))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// `123`, `<@123>` and `<@!123>`.
fn parse_user(arg: &str) -> Option<u64> {
    let t = arg.trim();
    let t = t
        .strip_prefix("<@")
        .and_then(|s| s.strip_suffix('>'))
        .map(|s| s.trim_start_matches('!'))
        .unwrap_or(t);
    t.parse().ok()
}

pub struct MutualGroupDms {
    manifest: Manifest,
    state: Shared<State>,
}

impl MutualGroupDms {
    pub fn new() -> MutualGroupDms {
        MutualGroupDms {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "MutualGroupDMs",
                    env!("CARGO_PKG_VERSION"),
                    "Lists the group DMs you share with a user",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("MutualGroupDMs".into());
                m
            },
            state: Shared::new(State::default()),
        }
    }

    /// The `/phoecord mutualgdms` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "mutualgdms",
            "List the group DMs you share with a user",
            move |_, args| {
                let Some(user) = args.get("user").and_then(parse_user) else {
                    return "Give a user id or mention, e.g. `/phoecord mutualgdms 123456789`.".into();
                };
                let st = state.lock();
                let mut out = String::new();
                let mutual: Vec<(&u64, &Group)> = st
                    .groups
                    .iter()
                    .filter(|(_, g)| g.recipients.contains(&user))
                    .collect();
                out.push_str(&format!(
                    "{} Mutual Group{} with {}",
                    if mutual.is_empty() { "No".to_string() } else { mutual.len().to_string() },
                    if mutual.len() == 1 { "" } else { "s" },
                    st.users.display(user)
                ));
                for (id, g) in mutual.iter().take(SHOW) {
                    out.push_str(&format!(
                        "\n- {} ({} members, channel {id})",
                        st.title(g),
                        g.recipients.len() + 1
                    ));
                }
                if mutual.len() > SHOW {
                    out.push_str(&format!("\n...and {} more", mutual.len() - SHOW));
                }
                if !st.ready {
                    out.push_str("\nNote: no READY seen yet; reload the client so your group DMs are tracked.");
                }
                out
            },
        )
        .option("user", "User id or mention", true)
        .boxed()
    }
}

impl Default for MutualGroupDms {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for MutualGroupDms {
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

    fn feed(p: &MutualGroupDms, name: &str, json: &str) -> Verdict {
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

    fn ask(p: &MutualGroupDms, user: &str) -> String {
        p.command().run(
            &mut Rec::default(),
            &Args {
                options: vec![("user".into(), user.into())],
            },
        )
    }

    const READY: &str = r#"{"d":{"private_channels":[
        {"id":"100","type":3,"name":"Raid night","recipient_ids":["5","6"]},
        {"id":"101","type":3,"name":null,"recipients":[{"id":"5","username":"ann"},{"id":"7","username":"bob","global_name":"Bob"}]},
        {"id":"102","type":1,"recipient_ids":["5"]}]}}"#;

    #[test]
    fn finds_mutual_groups_from_ready_and_ignores_plain_dms() {
        let p = MutualGroupDms::new();
        feed(&p, "READY", READY);
        let out = ask(&p, "5");
        assert!(out.starts_with("2 Mutual Groups with ann"), "{out}");
        assert!(out.contains("Raid night (3 members, channel 100)"));
        assert!(out.contains("ann, Bob (3 members, channel 101)"));
        assert!(!out.contains("channel 102"));
        assert!(ask(&p, "<@!7>").starts_with("1 Mutual Group with Bob"));
        assert!(ask(&p, "999").starts_with("No Mutual Groups"));
    }

    #[test]
    fn follows_create_recipient_changes_and_delete() {
        let p = MutualGroupDms::new();
        feed(&p, "READY", READY);
        feed(
            &p,
            "CHANNEL_CREATE",
            r#"{"d":{"id":"200","type":3,"recipients":[{"id":"8","username":"cy"}]}}"#,
        );
        assert!(ask(&p, "8").contains("channel 200"));
        feed(
            &p,
            "CHANNEL_RECIPIENT_ADD",
            r#"{"d":{"channel_id":"100","user":{"id":"8","username":"cy"}}}"#,
        );
        assert!(ask(&p, "8").starts_with("2 Mutual Groups"));
        feed(
            &p,
            "CHANNEL_RECIPIENT_REMOVE",
            r#"{"d":{"channel_id":"100","user":{"id":"5","username":"ann"}}}"#,
        );
        assert!(!ask(&p, "5").contains("channel 100"));
        feed(
            &p,
            "CHANNEL_UPDATE",
            r#"{"d":{"id":"101","type":3,"name":"Renamed"}}"#,
        );
        let out = ask(&p, "7");
        assert!(
            out.contains("Renamed (3 members"),
            "recipients kept on update: {out}"
        );
        feed(&p, "CHANNEL_DELETE", r#"{"d":{"id":"101","type":3}}"#);
        assert!(ask(&p, "7").starts_with("No Mutual Groups"));
    }

    #[test]
    fn bad_input_and_disabled_plugin() {
        let p = MutualGroupDms::new();
        feed(&p, "READY", r#"{"d":{"private_channels":"x"}}"#);
        feed(&p, "CHANNEL_CREATE", r#"{"d":{"type":3}}"#);
        feed(&p, "CHANNEL_RECIPIENT_ADD", r#"{"d":{}}"#);
        feed(&p, "CHANNEL_DELETE", r#"{"nod":1}"#);
        assert!(ask(&p, "not-a-user").starts_with("Give a user id"));
        let mut h = Rec::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        let out = p.command().run(
            &mut h,
            &Args {
                options: vec![("user".into(), "5".into())],
            },
        );
        assert!(out.contains("is off"));
    }

    #[test]
    fn group_count_is_bounded() {
        let mut st = State::default();
        for i in 0..(MAX_GROUPS as u64 + 50) {
            st.put(&j(&format!(
                r#"{{"id":"{}","type":3,"recipient_ids":["1"]}}"#,
                i + 1
            )));
        }
        assert_eq!(st.groups.len(), MAX_GROUPS);
    }
}
