//! Experiments: Discord experiments and developer features (Vencord: Experiments).
//!
//! Ported from Vencord `src/plugins/experiments/index.tsx` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors Megu, Ven, Nickyux, BanTheNons, Nuckyz). Vencord patches
//! the client so `isStaff` is true: that opens the hidden "Experiments" and "Developer options"
//! settings pages and the staff-only menus (setting `enableIsStaff`). The experiment assignments
//! themselves are ordinary data Discord sends.
//!
//! What this port does (data part):
//! - It reads the experiment assignments from `READY` (`experiments`, `guild_experiments`) and
//!   from `GET /experiments` (`assignments`, `guild_experiments`), in the array form Discord uses
//!   and in an object form. It keeps ids, buckets, revisions and names, at most [`MAX_EXPERIMENTS`]
//!   of each kind.
//! - `/phoecord experiments [filter]` shows the counts, whether your account has the staff flag,
//!   and the assignments (`filter` is part of an id or name).
//!
//! Switching it on:
//! - `[plugins.experiments] enabled = true` (off by default, like every plugin). Reading only:
//!   no traffic is changed.
//! - RISK, second switch, also off by default: `enableIsStaff = true` (the Vencord setting name)
//!   rewrites your user object in `READY` and in `USER_UPDATE` about you, setting the `STAFF`
//!   flag (`flags |= 1`), so the client unlocks its staff and developer UI. This is the most
//!   ban-prone thing a plugin here can do: the client then calls staff-only endpoints, which
//!   answer with errors, and an account that behaves like a staff client without being one is
//!   exactly what Discord's abuse detection looks for. Changing an experiment's bucket is not
//!   done (it would only affect the local client and has the same risk). Leave it off unless you
//!   accept that.
//!
//! Not ported: the toolbar developer menu and the Experiments settings page itself
//! (`toolbarDevMenu`, `forceStagingBanner`; Phase 2), and any editing of buckets.

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "experiments";
/// Most assignments of each kind kept.
pub const MAX_EXPERIMENTS: usize = 3000;
const LIST_LIMIT: usize = 30;
/// User flag `STAFF`.
const STAFF: i64 = 1;

const EVENTS: &[&str] = &["READY", "USER_UPDATE"];

#[derive(Clone, Debug, PartialEq)]
struct UserExp {
    hash: i64,
    revision: Option<i64>,
    bucket: Option<i64>,
    override_bucket: Option<i64>,
    population: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
struct GuildExp {
    key: String,
    revision: Option<i64>,
    populations: usize,
}

#[derive(Default)]
struct State {
    me: Option<u64>,
    user_flags: Option<i64>,
    /// The STAFF flag was added by us.
    forced: bool,
    user: Vec<UserExp>,
    guild: Vec<GuildExp>,
}

fn int_at(l: &[Val], i: usize) -> Option<i64> {
    l.get(i).and_then(Val::as_i64)
}

/// `[hash, revision, bucket, override, population, ...]` or `{hash|id, revision, bucket, ...}`.
fn user_exp(v: &Val) -> Option<UserExp> {
    if let Val::List(l) | Val::Tuple(l) = v {
        return Some(UserExp {
            hash: int_at(l, 0)?,
            revision: int_at(l, 1),
            bucket: int_at(l, 2),
            override_bucket: int_at(l, 3),
            population: int_at(l, 4),
        });
    }
    let get = |k: &str| v.get(k).and_then(Val::as_i64);
    Some(UserExp {
        hash: get("hash").or_else(|| get("id"))?,
        revision: get("revision"),
        bucket: get("bucket"),
        override_bucket: get("override").or_else(|| get("override_bucket")),
        population: get("population"),
    })
}

/// `[hash, hash_key, revision, populations, overrides, ...]` or an object.
fn guild_exp(v: &Val) -> Option<GuildExp> {
    if let Val::List(l) | Val::Tuple(l) = v {
        let key = match l.get(1) {
            Some(Val::Str(s)) if !s.is_empty() => s.clone(),
            _ => int_at(l, 0)?.to_string(),
        };
        return Some(GuildExp {
            key,
            revision: int_at(l, 2),
            populations: l.get(3).map(|p| p.as_list().len()).unwrap_or(0),
        });
    }
    let key = v
        .str_at(&["hash_key"])
        .or_else(|| v.str_at(&["name"]))
        .map(str::to_string)
        .or_else(|| v.get("hash").and_then(Val::as_i64).map(|h| h.to_string()))?;
    Some(GuildExp {
        key,
        revision: v.get("revision").and_then(Val::as_i64),
        populations: v.get("populations").map(|p| p.as_list().len()).unwrap_or(0),
    })
}

impl State {
    fn learn(&mut self, user: &[Val], guild: &[Val]) {
        if !user.is_empty() {
            self.user = user
                .iter()
                .filter_map(user_exp)
                .take(MAX_EXPERIMENTS)
                .collect();
        }
        if !guild.is_empty() {
            self.guild = guild
                .iter()
                .filter_map(guild_exp)
                .take(MAX_EXPERIMENTS)
                .collect();
        }
    }

    /// READY / USER_UPDATE. `true` if the payload was rewritten.
    fn dispatch(&mut self, name: &str, d: &mut Val, me: Option<u64>, staff: bool) -> bool {
        if me.is_some() {
            self.me = me;
        }
        let user_obj = match name {
            "READY" => {
                let user = d
                    .get("experiments")
                    .map(Val::as_list)
                    .unwrap_or(&[])
                    .to_vec();
                let guild = d
                    .get("guild_experiments")
                    .map(Val::as_list)
                    .unwrap_or(&[])
                    .to_vec();
                self.learn(&user, &guild);
                if let Some(id) = d.id_at(&["user", "id"]) {
                    self.me = Some(id);
                }
                d.get_mut("user")
            }
            // Only about yourself.
            "USER_UPDATE" if d.id_at(&["id"]).is_some() && d.id_at(&["id"]) == self.me => Some(d),
            _ => None,
        };
        let Some(u) = user_obj.filter(|u| matches!(u, Val::Map(_))) else {
            return false;
        };
        let flags = u.get("flags").and_then(Val::as_i64).unwrap_or(0);
        self.user_flags = Some(flags);
        self.forced = false;
        if staff && flags & STAFF == 0 {
            u.set("flags", Val::Int(flags | STAFF));
            self.user_flags = Some(flags | STAFF);
            self.forced = true;
            return true;
        }
        false
    }

    fn render(&self, filter: Option<&str>, staff_rewrite: bool) -> String {
        let f = filter.map(str::to_lowercase).filter(|s| !s.is_empty());
        let hit = |s: &str| f.as_ref().is_none_or(|f| s.to_lowercase().contains(f));
        let overridden = |e: &UserExp| e.override_bucket.is_some_and(|o| o >= 0);
        let mut out = format!(
            "Experiments: {} for you ({} overridden), {} for servers.",
            self.user.len(),
            self.user.iter().filter(|e| overridden(e)).count(),
            self.guild.len()
        );
        out.push_str(match self.user_flags {
            Some(fl) if fl & STAFF != 0 && self.forced => "\nStaff flag: set (by `enableIsStaff`).",
            Some(fl) if fl & STAFF != 0 => "\nStaff flag: set by Discord.",
            Some(_) => "\nStaff flag: not set.",
            None => "\nStaff flag: unknown (no READY seen).",
        });
        out.push_str(if staff_rewrite {
            "\n`enableIsStaff` is **on**: your user flags are rewritten."
        } else {
            "\n`enableIsStaff` is off: nothing is rewritten."
        });
        let mut lines: Vec<String> = Vec::new();
        for e in self.user.iter().filter(|e| hit(&e.hash.to_string())) {
            let mut l = format!("- user {}", e.hash);
            if let Some(b) = e.bucket {
                l.push_str(&format!(": bucket {b}"));
            }
            if let Some(p) = e.population {
                l.push_str(&format!(", population {p}"));
            }
            if overridden(e) {
                l.push_str(&format!(", override {}", e.override_bucket.unwrap_or(0)));
            }
            lines.push(l);
        }
        for e in self.guild.iter().filter(|e| hit(&e.key)) {
            let mut l = format!("- server {}", e.key);
            if let Some(r) = e.revision {
                l.push_str(&format!(" (revision {r}, {} populations)", e.populations));
            }
            lines.push(l);
        }
        let total = lines.len();
        for l in lines.iter().take(LIST_LIMIT) {
            out.push('\n');
            out.push_str(l);
        }
        if total > LIST_LIMIT {
            out.push_str(&format!(
                "\n... and {} more (use a filter)",
                total - LIST_LIMIT
            ));
        }
        out
    }
}

fn is_experiments_route(method: &str, route: &str) -> bool {
    method == "GET" && route.trim_matches('/').split('?').next() == Some("experiments")
}

fn staff_on(host: &dyn Host) -> bool {
    host.setting("enableIsStaff").and_then(|v| v.as_bool()) == Some(true)
}

pub struct Experiments {
    manifest: Manifest,
    state: Shared<State>,
}

impl Experiments {
    pub fn new() -> Experiments {
        Experiments {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "Experiments",
                    env!("CARGO_PKG_VERSION"),
                    "Discord experiments and developer features",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("Experiments".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord experiments` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let st = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "experiments",
            "Show your Discord experiment assignments",
            move |host, args| st.lock().render(args.get("filter"), staff_on(host)),
        )
        .option("filter", "Part of an experiment id or name", false)
        .boxed()
    }
}

impl Default for Experiments {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Experiments {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => is_one_of(name, EVENTS),
            EventKind::RestResponse { method, route } => is_experiments_route(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Dispatch { name, payload, .. } if is_one_of(name, EVENTS) => {
                let Some(d) = payload.get_mut("d") else {
                    return Verdict::Pass;
                };
                let staff = staff_on(host);
                let me = host.self_id();
                if self.state.lock().dispatch(name, d, me, staff) {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } if is_experiments_route(method, route) => {
                let user = body.get("assignments").map(Val::as_list).unwrap_or(&[]);
                let guild = body
                    .get("guild_experiments")
                    .map(Val::as_list)
                    .unwrap_or(&[]);
                self.state.lock().learn(user, guild);
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
    use phoecord_plugin_runtime::{Chain, Outcome};

    const READY: &str = r#"{"op":0,"t":"READY","d":{"user":{"id":"5","flags":0},
        "experiments":[[2354, 3, 1, -1, 0, 123, 0, 0],[777, 1, 2, 4, 1, 9, 0, 0],"junk"],
        "guild_experiments":[[111, "2021-03_thing", 7, [[[1,2]]], [], [], null, null, 0],[222, null, 1, [], [], [], null, null, 0]]}}"#;

    fn pair() -> (Chain, Experiments) {
        let p = Experiments::new();
        let probe = Experiments {
            manifest: p.manifest.clone(),
            state: p.state.clone(),
        };
        (Chain::new(vec![Box::new(p)]), probe)
    }

    fn ready(chain: &Chain, h: &mut Rec) -> (Outcome, Val) {
        let mut p = j(READY);
        let out = chain.dispatch(h, "READY", &mut p, &mut Vec::new());
        (out, p)
    }

    fn text(probe: &Experiments, h: &mut Rec, filter: Option<&str>) -> String {
        let args = Args {
            options: filter
                .map(|f| vec![("filter".to_string(), f.to_string())])
                .unwrap_or_default(),
        };
        probe.command().run(h, &args)
    }

    #[test]
    fn collects_assignments_without_touching_the_payload() {
        let (chain, probe) = pair();
        let mut h = Rec::default();
        let (out, p) = ready(&chain, &mut h);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(p.at(&["d", "user", "flags"]).and_then(Val::as_i64), Some(0));
        let t = text(&probe, &mut h, None);
        assert!(t.contains("2 for you (1 overridden), 2 for servers"), "{t}");
        assert!(t.contains("- user 777: bucket 2, population 1, override 4"));
        assert!(t.contains("- server 2021-03_thing (revision 7, 1 populations)"));
        assert!(t.contains("- server 222"));
        assert!(t.contains("Staff flag: not set."));
        assert!(t.contains("`enableIsStaff` is off"));
        let f = text(&probe, &mut h, Some("2021"));
        assert!(f.contains("2021-03_thing") && !f.contains("user 777"));
    }

    #[test]
    fn staff_rewrite_is_opt_in_and_only_about_yourself() {
        let (chain, probe) = pair();
        let mut h = Rec::default().with("enableIsStaff", Val::Bool(true));
        let (out, p) = ready(&chain, &mut h);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(p.at(&["d", "user", "flags"]).and_then(Val::as_i64), Some(1));
        assert!(text(&probe, &mut h, None).contains("Staff flag: set (by `enableIsStaff`)"));
        // Already staff: nothing to change. Other users: untouched.
        let mut own = j(r#"{"d":{"id":"5","flags":1}}"#);
        assert_eq!(
            chain.dispatch(&mut h, "USER_UPDATE", &mut own, &mut Vec::new()),
            Outcome::Untouched
        );
        let mut mine = j(r#"{"d":{"id":"5","flags":64}}"#);
        assert_eq!(
            chain.dispatch(&mut h, "USER_UPDATE", &mut mine, &mut Vec::new()),
            Outcome::Changed
        );
        assert_eq!(mine.at(&["d", "flags"]).and_then(Val::as_i64), Some(65));
        let mut other = j(r#"{"d":{"id":"6","flags":0}}"#);
        assert_eq!(
            chain.dispatch(&mut h, "USER_UPDATE", &mut other, &mut Vec::new()),
            Outcome::Untouched
        );
    }

    #[test]
    fn rest_experiments_answer_is_read() {
        let (chain, probe) = pair();
        let mut h = Rec::default();
        let mut body = j(r#"{"fingerprint":"1.x","assignments":[[10,1,3,-1,0,5,0,0]],
            "guild_experiments":[{"hash_key":"2022-01_x","revision":2,"populations":[[]]}]}"#);
        let out = chain.rest_response(
            &mut h,
            "GET",
            "/experiments?with_guild_experiments=true",
            200,
            &mut body,
        );
        assert_eq!(out, Outcome::Untouched);
        let t = text(&probe, &mut h, None);
        assert!(t.contains("- user 10: bucket 3"), "{t}");
        assert!(t.contains("- server 2022-01_x (revision 2, 1 populations)"));
        // An error answer or another route is ignored.
        assert!(!probe.wants(&EventKind::RestResponse {
            method: "POST",
            route: "/experiments"
        }));
    }

    #[test]
    fn malformed_payloads_and_off_plugin() {
        let (chain, probe) = pair();
        let mut h = Rec::default().with("enableIsStaff", Val::Bool(true));
        for (n, b) in [
            ("READY", "{}"),
            (
                "READY",
                r#"{"d":{"experiments":"x","guild_experiments":[[],[{}],5],"user":7}}"#,
            ),
            ("USER_UPDATE", r#"{"d":{}}"#),
        ] {
            let mut p = j(b);
            assert_eq!(
                chain.dispatch(&mut h, n, &mut p, &mut Vec::new()),
                Outcome::Untouched
            );
        }
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(text(&probe, &mut h, None).contains("is off"));
        assert!(!probe.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
    }
}
