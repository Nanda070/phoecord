//! FakeDeafenDetector: flag members caught speaking while shown as muted or deafened.
//!
//! Ported from the owner's userplugin `FakeDeafenDetector`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/FakeDeafenDetector,
//! GPL-3.0-or-later per the monorepo README; the files carry no header), modified for PhoeCord
//! 2026-11-25. The script subscribes to the client's `SPEAKING` action, and when a user's voice
//! activity bit is set while their voice state says `selfMute || selfDeaf`, it records that user
//! as a suspect (count, first and last time) for good, logs it, and shows a flag in the member
//! list. Traffic is never changed here.
//!
//! Switching it on: `[plugins.fake_deafen_detector] enabled = true`, or
//! `/phoecord toggle fake_deafen_detector`. `/phoecord flagged` lists the suspects.
//!
//! Ported: the claim tracking (`self_mute || self_deaf` from `READY`/`GUILD_CREATE` voice states and
//! `VOICE_STATE_UPDATE`), the detection rule (context other than `default` ignored, bit 0 of the
//! speaking flags required, user must be claiming silence), the suspect table with
//! count/first/last, the log line, the setting `rememberAcrossRestarts` (default `true`: the table
//! is loaded from the plugin's store on first use; as in the script it is saved after every flag
//! whatever the setting says).
//!
//! GAP, no 1:1 here: `SPEAKING` is not a main-gateway dispatch. Discord sends it on the voice
//! server's own connection, which this proxy does not carry; the script gets it because the
//! client turns it into a Flux action. The detection code below therefore runs only if a host ever
//! delivers a `SPEAKING` dispatch (fields `userId`/`user_id`, `speakingFlags`/`speaking`,
//! `context`); with the current proxy nobody gets flagged. Nothing is faked to cover it.
//! Also not ported: the flag next to the name in the member list and its tooltip (Phase 2), and
//! the script's "forget everything on stop" (a plugin has no stop hook; the saved table stays).

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::{Shared, Users, is_one_of};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::{BTreeMap, HashMap};

const PLUGIN_ID: &str = "fake_deafen_detector";
/// The store key; the script's `STORE_KEY` is `FakeDeafenDetector_suspects`.
const STORE_KEY: &str = "suspects";
const EVENTS: &[&str] = &["SPEAKING", "READY", "GUILD_CREATE", "VOICE_STATE_UPDATE"];

#[derive(Clone, Debug, PartialEq)]
struct Suspect {
    count: i64,
    first_seen: i64,
    last_seen: i64,
}

#[derive(Default)]
struct State {
    /// user -> claims to be self-muted or self-deafened.
    claiming: HashMap<u64, bool>,
    suspects: BTreeMap<u64, Suspect>,
    users: Users,
    loaded: bool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` (UTC) of a Unix time in milliseconds.
fn date_of(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

impl State {
    /// Load the saved table once; only when `rememberAcrossRestarts` is not `false`.
    fn ensure_loaded(&mut self, host: &dyn Host) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if host
            .setting("rememberAcrossRestarts")
            .and_then(|v| v.as_bool())
            == Some(false)
        {
            return;
        }
        let Some(Val::Map(entries)) = host.kv_get(STORE_KEY) else {
            return;
        };
        for (k, v) in entries {
            let id = match &k {
                Val::Str(s) | Val::Atom(s) => s.parse::<u64>().ok(),
                other => other.snowflake(),
            };
            let (Some(id), Some(count)) = (id, v.get("count").and_then(Val::as_i64)) else {
                continue;
            };
            let first_seen = v.get("firstSeen").and_then(Val::as_i64).unwrap_or(0);
            let last_seen = v
                .get("lastSeen")
                .and_then(Val::as_i64)
                .unwrap_or(first_seen);
            self.suspects.insert(
                id,
                Suspect {
                    count,
                    first_seen,
                    last_seen,
                },
            );
        }
    }

    fn save(&self, host: &mut dyn Host) {
        let mut map = Val::map();
        for (id, s) in &self.suspects {
            let mut e = Val::map();
            e.set("count", Val::Int(s.count));
            e.set("firstSeen", Val::Int(s.first_seen));
            e.set("lastSeen", Val::Int(s.last_seen));
            map.set(&id.to_string(), e);
        }
        host.kv_set(STORE_KEY, map);
    }

    fn set_claim(&mut self, d: &Val) {
        let Some(user) = d.id_at(&["user_id"]) else {
            return;
        };
        if d.id_at(&["channel_id"]).is_none() {
            self.claiming.remove(&user);
            return;
        }
        let b = |k: &str| d.get(k).and_then(Val::as_bool).unwrap_or(false);
        self.claiming.insert(user, b("self_mute") || b("self_deaf"));
    }

    fn observe(&mut self, name: &str, d: &Val) {
        if is_one_of(name, Users::EVENTS) {
            self.users.observe(name, d);
        }
        match name {
            "VOICE_STATE_UPDATE" => self.set_claim(d),
            "GUILD_CREATE" => self.load_guild(d),
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.load_guild(g);
                }
            }
            _ => {}
        }
    }

    fn load_guild(&mut self, g: &Val) {
        for vs in g.get("voice_states").map(Val::as_list).unwrap_or(&[]) {
            self.set_claim(vs);
        }
    }

    /// One `SPEAKING` event. `true` if it flagged someone for the first time.
    fn speaking(&mut self, d: &Val, now: i64) -> Option<(u64, bool)> {
        if let Some(ctx) = d.get("context").and_then(Val::as_str)
            && !ctx.is_empty()
            && ctx != "default"
        {
            return None; // "go live" stream audio context
        }
        let flags = d
            .get("speakingFlags")
            .or_else(|| d.get("speaking"))
            .and_then(Val::as_i64)
            .unwrap_or(0);
        if flags & 1 == 0 {
            return None; // bit 0 = actual voice activity
        }
        let user = d.id_at(&["userId"]).or_else(|| d.id_at(&["user_id"]))?;
        if user == 0 || self.claiming.get(&user) != Some(&true) {
            return None;
        }
        match self.suspects.get_mut(&user) {
            Some(s) => {
                s.count += 1;
                s.last_seen = now;
                Some((user, false))
            }
            None => {
                self.suspects.insert(
                    user,
                    Suspect {
                        count: 1,
                        first_seen: now,
                        last_seen: now,
                    },
                );
                Some((user, true))
            }
        }
    }

    fn report(&self, host: &dyn Host) -> String {
        if self.suspects.is_empty() {
            return host.t("fake_deafen_detector-none", &[]);
        }
        let mut out = format!(
            "**{}**",
            host.t(
                "fake_deafen_detector-header",
                &[("count", &self.suspects.len().to_string())]
            )
        );
        for (id, s) in &self.suspects {
            out.push('\n');
            out.push_str(&host.t(
                "fake_deafen_detector-line",
                &[
                    ("user", &self.users.display(*id)),
                    ("count", &s.count.to_string()),
                    ("first", &date_of(s.first_seen)),
                    ("last", &date_of(s.last_seen)),
                ],
            ));
        }
        out
    }
}

pub struct FakeDeafenDetector {
    manifest: Manifest,
    state: Shared<State>,
}

impl FakeDeafenDetector {
    pub fn new() -> FakeDeafenDetector {
        FakeDeafenDetector {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "FakeDeafenDetector",
                    env!("CARGO_PKG_VERSION"),
                    "Flag members caught speaking while shown as muted or deafened",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("FakeDeafenDetector".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord flagged` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "flagged",
            "List members caught speaking while shown as muted or deafened",
            move |host: &mut dyn Host, _args: &Args| {
                let mut st = state.lock();
                st.ensure_loaded(host);
                st.report(host)
            },
        )
        .boxed()
    }
}

impl Default for FakeDeafenDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn wanted(name: &str) -> bool {
    is_one_of(name, EVENTS) || is_one_of(name, Users::EVENTS)
}

impl Plugin for FakeDeafenDetector {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if wanted(name))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch { name, payload, .. } = event else {
            return Verdict::Pass;
        };
        if !wanted(name) {
            return Verdict::Pass;
        }
        // The proxy hands over the whole `{op, t, d}` frame.
        let payload = payload.get("d").unwrap_or(payload);
        let mut st = self.state.lock();
        st.ensure_loaded(host);
        if name == "SPEAKING" {
            if let Some((user, first)) = st.speaking(payload, now_ms()) {
                if first {
                    host.log(&format!(
                        "[FakeDeafenDetector] flagged {user} as a suspected fake mute/deafen user"
                    ));
                }
                st.save(host);
            }
        } else {
            st.observe(name, payload);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn feed(chain: &Chain, h: &mut Rec, name: &str, json: &str) -> Outcome {
        let mut d = j(json);
        chain.dispatch(h, name, &mut d, &mut Vec::new())
    }

    fn vs(user: u32, mute: bool, deaf: bool) -> String {
        format!(
            r#"{{"user_id":"{user}","channel_id":"5","guild_id":"1","self_mute":{mute},"self_deaf":{deaf},
               "member":{{"user":{{"id":"{user}","username":"bob"}}}}}}"#
        )
    }

    #[test]
    fn speaking_while_muted_flags_once_and_counts() {
        let p = FakeDeafenDetector::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default();
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs(7, true, false));
        let sp = r#"{"userId":"7","speakingFlags":1,"context":"default"}"#;
        assert_eq!(feed(&chain, &mut h, "SPEAKING", sp), Outcome::Untouched);
        feed(&chain, &mut h, "SPEAKING", sp);
        let logs: Vec<_> = h.logs.iter().filter(|l| l.contains("flagged 7")).collect();
        assert_eq!(logs.len(), 1, "logged only the first time");
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(out.contains("bob") && out.contains("2x"), "{out}");
        // Saved to the store.
        let saved = h.kv.get(STORE_KEY).unwrap();
        assert_eq!(saved.at(&["7", "count"]).and_then(Val::as_i64), Some(2));
    }

    #[test]
    fn real_gateway_frames_are_read_from_d() {
        let p = FakeDeafenDetector::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default();
        let frame = format!(
            r#"{{"op":0,"t":"VOICE_STATE_UPDATE","d":{}}}"#,
            vs(7, true, false)
        );
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &frame);
        let sp =
            r#"{"op":0,"t":"SPEAKING","d":{"userId":"7","speakingFlags":1,"context":"default"}}"#;
        feed(&chain, &mut h, "SPEAKING", sp);
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(out.contains("bob") && out.contains("1x"), "{out}");
    }

    #[test]
    fn deafened_also_counts_and_unmuted_does_not() {
        let chain = Chain::new(vec![Box::new(FakeDeafenDetector::new())]);
        let mut h = Rec::default();
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs(7, false, true));
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs(8, false, false));
        feed(
            &chain,
            &mut h,
            "SPEAKING",
            r#"{"userId":"7","speakingFlags":1}"#,
        );
        feed(
            &chain,
            &mut h,
            "SPEAKING",
            r#"{"userId":"8","speakingFlags":1}"#,
        );
        assert_eq!(h.logs.len(), 1);
        assert!(h.logs[0].contains("flagged 7"));
        // Leaving the channel drops the claim.
        feed(
            &chain,
            &mut h,
            "VOICE_STATE_UPDATE",
            r#"{"user_id":"8","channel_id":null}"#,
        );
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs(8, true, false));
        feed(
            &chain,
            &mut h,
            "SPEAKING",
            r#"{"user_id":"8","speaking":1}"#,
        );
        assert_eq!(h.logs.len(), 2);
    }

    #[test]
    fn ignored_speaking_events() {
        let chain = Chain::new(vec![Box::new(FakeDeafenDetector::new())]);
        let mut h = Rec::default();
        feed(&chain, &mut h, "VOICE_STATE_UPDATE", &vs(7, true, true));
        for sp in [
            r#"{"userId":"7","speakingFlags":2}"#,
            r#"{"userId":"7","speakingFlags":0}"#,
            r#"{"userId":"7","speakingFlags":1,"context":"stream"}"#,
            r#"{"speakingFlags":1}"#,
            r#"{"userId":"9","speakingFlags":1}"#,
            r#"{"userId":"7","speakingFlags":"x"}"#,
        ] {
            feed(&chain, &mut h, "SPEAKING", sp);
        }
        assert!(h.logs.is_empty());
        assert!(h.kv.is_empty());
    }

    #[test]
    fn claims_come_from_guild_create_and_ready() {
        let chain = Chain::new(vec![Box::new(FakeDeafenDetector::new())]);
        let mut h = Rec::default();
        feed(
            &chain,
            &mut h,
            "GUILD_CREATE",
            r#"{"id":"1","voice_states":[{"user_id":"7","channel_id":"5","self_mute":true}]}"#,
        );
        feed(
            &chain,
            &mut h,
            "READY",
            r#"{"guilds":[{"id":"2","voice_states":[{"user_id":"8","channel_id":"6","self_deaf":true}]}]}"#,
        );
        feed(
            &chain,
            &mut h,
            "SPEAKING",
            r#"{"userId":"7","speakingFlags":1}"#,
        );
        feed(
            &chain,
            &mut h,
            "SPEAKING",
            r#"{"userId":"8","speakingFlags":1}"#,
        );
        assert_eq!(h.logs.len(), 2);
    }

    #[test]
    fn remember_across_restarts_loads_the_saved_table() {
        let saved = j(r#"{"7":{"count":3,"firstSeen":1700000000000,"lastSeen":1700000100000}}"#);
        let mut h = Rec::default();
        h.kv.insert(STORE_KEY.into(), saved.clone());
        let p = FakeDeafenDetector::new();
        let cmd = p.command();
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(out.contains("3x") && out.contains("2023-11-14"), "{out}");

        // rememberAcrossRestarts = false: starts empty.
        let mut h = Rec::default().with("rememberAcrossRestarts", Val::Bool(false));
        h.kv.insert(STORE_KEY.into(), saved);
        let cmd = FakeDeafenDetector::new().command();
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert!(!out.contains("3x"), "{out}");
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        let chain = Chain::new(vec![Box::new(FakeDeafenDetector::new())]);
        let mut h = Rec::default();
        for (n, d) in [
            ("VOICE_STATE_UPDATE", "{}"),
            ("VOICE_STATE_UPDATE", r#"{"user_id":"x","channel_id":5}"#),
            ("GUILD_CREATE", r#"{"voice_states":7}"#),
            ("READY", "[]"),
            ("SPEAKING", "[]"),
            ("SPEAKING", r#"{"userId":{},"speakingFlags":[]}"#),
        ] {
            assert_eq!(feed(&chain, &mut h, n, d), Outcome::Untouched);
        }
        h.kv.insert(STORE_KEY.into(), j(r#"{"x":1,"7":"bad"}"#));
        let out = FakeDeafenDetector::new()
            .command()
            .run(&mut h, &Args { options: vec![] });
        assert!(!out.is_empty());
    }

    #[test]
    fn dates() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_700_000_000_000), "2023-11-14");
    }
}
