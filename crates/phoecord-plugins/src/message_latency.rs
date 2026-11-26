//! MessageLatency: mark messages that took a while to send (Vencord: MessageLatency).
//!
//! Ported from Vencord `src/plugins/messageLatency/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2024 Vendicated and contributors; author arHSM). The client builds a message's `nonce` from
//! the clock when the user pressed send, and Discord's message id is made when the server accepted
//! it. The difference of the two snowflake timestamps is the send delay. Vencord shows a coloured
//! indicator with a tooltip when it reaches the threshold. This is the data part: every
//! `MESSAGE_CREATE` is checked the same way and the slow ones go into a bounded log
//! (the last 200), which `/phoecord latency` lists, newest first.
//!
//! Same rules as Vencord: no nonce, a bot author (bots rarely send one), or a nonce that is not a
//! snowflake means no verdict. A negative delta means the sender's clock is ahead; two minutes or
//! more is treated as a clock problem or a troll nonce. A delay of about 17 days is how old
//! Discord Android clients (Kotlin) look, and is corrected by that amount.
//!
//! Switching it on: `[plugins.message_latency] enabled = true`, or
//! `/phoecord toggle message_latency`. Settings, with Vencord's names and defaults: `latency`
//! (threshold in seconds, 2), `detectDiscordKotlin` (true), `showMillis` (false), `ignoreSelf`
//! (false).
//!
//! Not ported: the indicator and tooltip next to the username (Phase 2 UI hooks).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Ring, Shared, Users};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "message_latency";
const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;
/// About 17 days: the delay old Discord Android clients show (Vencord's `DISCORD_KT_DELAY`).
const DISCORD_KT_DELAY: i64 = 1_471_228_928;
const DAY_MS: i64 = 86_400_000;
const TROLL_LIMIT_MS: i64 = 2 * 60 * 1000;
const LOG_CAP: usize = 200;
/// Entries one command shows.
const SHOW: usize = 15;

/// How bad one slow message looks, Vencord's three colours plus the grey cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Old Android client (green in Vencord).
    Kotlin,
    /// The sender's clock is ahead (grey).
    ClockAhead,
    /// Two minutes or more (grey): probably a clock problem, not the network.
    Implausible,
    /// At least twice the threshold (red).
    Slow,
    /// Over the threshold (yellow).
    Mild,
}

#[derive(Clone, Debug, PartialEq)]
struct Latency {
    kind: Kind,
    /// Text of the delay, `None` for the pure Kotlin case.
    text: Option<String>,
}

#[derive(Clone, Copy)]
struct Opts {
    latency_ms: i64,
    detect_kotlin: bool,
    show_millis: bool,
    ignore_self: bool,
}

impl Opts {
    fn read(host: &dyn Host) -> Opts {
        let num = |k: &str| match host.setting(k) {
            Some(Val::Int(n)) => Some(n as f64),
            Some(Val::Float(f)) => Some(f),
            Some(Val::Big(n)) => Some(n as f64),
            _ => None,
        };
        let flag =
            |k: &str, default: bool| host.setting(k).and_then(|v| v.as_bool()).unwrap_or(default);
        Opts {
            latency_ms: (num("latency").unwrap_or(2.0).max(0.0) * 1000.0) as i64,
            detect_kotlin: flag("detectDiscordKotlin", true),
            show_millis: flag("showMillis", false),
            ignore_self: flag("ignoreSelf", false),
        }
    }
}

struct Entry {
    channel: u64,
    message: u64,
    author: u64,
    text: String,
}

struct State {
    users: Users,
    log: Ring<Entry>,
}

impl Default for State {
    fn default() -> Self {
        State {
            users: Users::default(),
            log: Ring::new(LOG_CAP),
        }
    }
}

fn snowflake_ms(id: u64) -> i64 {
    ((id >> 22) as i64).saturating_add(DISCORD_EPOCH_MS)
}

/// Vencord's `stringDelta`: `1 minute and 5 seconds`, `2 hours 3 minutes`.
fn string_delta(delta: i64, show_millis: bool) -> String {
    let parts = [
        ("day", delta / DAY_MS),
        ("hour", (delta / 3_600_000) % 24),
        ("minute", (delta / 60_000) % 60),
        ("second", (delta / 1000) % 60),
        ("millisecond", delta % 1000),
    ];
    let mut out = String::new();
    for (unit, n) in parts {
        if n <= 0 {
            continue;
        }
        if !out.is_empty() {
            let last = if show_millis {
                unit == "millisecond"
            } else {
                unit == "second"
            };
            out.push_str(if last { " and " } else { " " });
        }
        out.push_str(&format!("{n} {unit}{}", if n > 1 { "s" } else { "" }));
    }
    if out.is_empty() {
        "0 seconds".into()
    } else {
        out
    }
}

/// Vencord's `latencyTooltipData` on two snowflakes: `None` when the message was not slow.
fn assess(id: u64, nonce: u64, o: Opts) -> Option<Latency> {
    let mut delta = snowflake_ms(id) - snowflake_ms(nonce);
    if !o.show_millis {
        // JavaScript's Math.round: halves go up.
        delta = ((delta as f64 / 1000.0 + 0.5).floor() as i64) * 1000;
    }
    let mut kotlin = false;
    // One day of padding for good measure, like Vencord.
    if delta.saturating_neg() >= DISCORD_KT_DELAY - DAY_MS {
        kotlin = o.detect_kotlin;
        delta += DISCORD_KT_DELAY;
    }
    let abs = delta.abs();
    let ahead = abs != delta;
    let text = (abs >= o.latency_ms).then(|| string_delta(abs, o.show_millis));
    if text.is_none() && !kotlin {
        return None;
    }
    let kind = if kotlin {
        Kind::Kotlin
    } else if delta >= TROLL_LIMIT_MS {
        Kind::Implausible
    } else if ahead {
        Kind::ClockAhead
    } else if delta >= o.latency_ms * 2 {
        Kind::Slow
    } else {
        Kind::Mild
    };
    Some(Latency { kind, text })
}

fn describe(v: &Latency) -> String {
    let t = v.text.as_deref();
    match (v.kind, t) {
        (Kind::Kotlin, None) => "suspected old Discord Android client".into(),
        (Kind::Kotlin, Some(t)) => format!("{t} (suspected old Discord Android client)"),
        (Kind::ClockAhead, Some(t)) => format!("sender's clock is {t} ahead"),
        (Kind::Implausible, Some(t)) => format!("{t} (probably a clock problem)"),
        (_, Some(t)) => format!("sent with a delay of {t}"),
        (_, None) => "slow".into(),
    }
}

pub struct MessageLatency {
    manifest: Manifest,
    state: Shared<State>,
}

impl MessageLatency {
    pub fn new() -> MessageLatency {
        MessageLatency {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "MessageLatency",
                    env!("CARGO_PKG_VERSION"),
                    "Logs messages that took a while to send",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("MessageLatency".into());
                m
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord latency` command. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "latency",
            "Show recent messages that were slow to send",
            move |_, _| {
                let st = state.lock();
                if st.log.is_empty() {
                    return "No slow messages seen yet.".into();
                }
                let mut out = format!("**{} slow messages seen** (newest first)", st.log.len());
                for e in st.log.iter().rev().take(SHOW) {
                    out.push_str(&format!(
                        "\n- {} in <#{}> (message {}): {}",
                        st.users.display(e.author),
                        e.channel,
                        e.message,
                        e.text
                    ));
                }
                if st.log.len() > SHOW {
                    out.push_str(&format!("\n...and {} older", st.log.len() - SHOW));
                }
                out
            },
        )
        .boxed()
    }
}

impl Default for MessageLatency {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for MessageLatency {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(
            event,
            EventKind::Dispatch {
                name: "MESSAGE_CREATE"
            }
        )
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::Dispatch {
            name: "MESSAGE_CREATE",
            payload,
            ..
        } = event
        else {
            return Verdict::Pass;
        };
        let Some(d) = payload.get("d") else {
            return Verdict::Pass;
        };
        // A bot author or a missing / non-snowflake nonce gives no verdict.
        let author = d.id_at(&["author", "id"]);
        let (Some(id), Some(nonce), Some(author)) = (
            d.id_at(&["id"]),
            d.get("nonce")
                .and_then(Val::snowflake)
                .filter(|n| *n >= 1 << 22),
            author,
        ) else {
            return Verdict::Pass;
        };
        if d.at(&["author", "bot"]).and_then(Val::as_bool) == Some(true) {
            return Verdict::Pass;
        }
        let opts = Opts::read(host);
        if opts.ignore_self && host.self_id() == Some(author) {
            return Verdict::Pass;
        }
        let Some(v) = assess(id, nonce, opts) else {
            return Verdict::Pass;
        };
        let mut st = self.state.lock();
        st.users.observe("MESSAGE_CREATE", d);
        st.log.push(Entry {
            channel: d.id_at(&["channel_id"]).unwrap_or(0),
            message: id,
            author,
            text: describe(&v),
        });
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn opts() -> Opts {
        Opts {
            latency_ms: 2000,
            detect_kotlin: true,
            show_millis: false,
            ignore_self: false,
        }
    }

    /// A snowflake made at `ms` after the Discord epoch.
    fn flake(ms: i64) -> u64 {
        (ms as u64) << 22
    }

    const T: i64 = 400_000_000_000;

    fn create(p: &MessageLatency, h: &mut Rec, id: u64, nonce: u64, extra: &str) -> Verdict {
        let mut payload = j(&format!(
            r#"{{"d":{{"id":"{id}","nonce":"{nonce}","channel_id":"77","author":{{"id":"5","username":"ann"{extra}}}}}}}"#
        ));
        let mut ex = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name: "MESSAGE_CREATE",
                payload: &mut payload,
                extra: &mut ex,
            },
        )
    }

    #[test]
    fn assess_levels() {
        let n = flake(T);
        assert_eq!(assess(flake(T + 1000), n, opts()), None, "under threshold");
        let mild = assess(flake(T + 3000), n, opts()).unwrap();
        assert_eq!(
            (mild.kind, mild.text.as_deref()),
            (Kind::Mild, Some("3 seconds"))
        );
        let slow = assess(flake(T + 5000), n, opts()).unwrap();
        assert_eq!(slow.kind, Kind::Slow);
        let troll = assess(flake(T + 3 * 60_000 + 5000), n, opts()).unwrap();
        assert_eq!(troll.kind, Kind::Implausible);
        assert_eq!(troll.text.as_deref(), Some("3 minutes and 5 seconds"));
        let ahead = assess(flake(T), flake(T + 6000), opts()).unwrap();
        assert_eq!(ahead.kind, Kind::ClockAhead);
        // Old Android: about 17 days early after correction.
        let kt = assess(flake(T), flake(T + DISCORD_KT_DELAY), opts()).unwrap();
        assert_eq!((kt.kind, kt.text), (Kind::Kotlin, None));
        let off = Opts {
            detect_kotlin: false,
            ..opts()
        };
        assert_ne!(
            assess(flake(T), flake(T + DISCORD_KT_DELAY), off).map(|v| v.kind),
            Some(Kind::Kotlin)
        );
    }

    #[test]
    fn string_delta_matches_vencord() {
        assert_eq!(string_delta(1000, false), "1 second");
        assert_eq!(string_delta(65_000, false), "1 minute and 5 seconds");
        assert_eq!(
            string_delta(3_725_000, false),
            "1 hour 2 minutes and 5 seconds"
        );
        assert_eq!(string_delta(1500, true), "1 second and 500 milliseconds");
        assert_eq!(string_delta(0, false), "0 seconds");
    }

    #[test]
    fn logs_slow_messages_and_command_lists_them_newest_first() {
        let p = MessageLatency::new();
        let mut h = Rec::default();
        create(&p, &mut h, flake(T + 1000), flake(T), "");
        create(&p, &mut h, flake(T + 4000), flake(T), "");
        create(&p, &mut h, flake(T + 9000), flake(T), "");
        let out = p.command().run(&mut h, &Args { options: vec![] });
        assert!(out.starts_with("**2 slow messages seen**"), "{out}");
        assert!(out.find("9 seconds").unwrap() < out.find("4 seconds").unwrap());
        assert!(out.contains("ann in <#77>"));
    }

    #[test]
    fn respects_settings_bots_and_missing_nonce() {
        let p = MessageLatency::new();
        // Higher threshold: 4 s is fine now.
        let mut h = Rec::default().with("latency", Val::Int(10));
        create(&p, &mut h, flake(T + 4000), flake(T), "");
        // Bot author.
        create(
            &p,
            &mut Rec::default(),
            flake(T + 9000),
            flake(T),
            r#","bot":true"#,
        );
        // ignoreSelf with self id 5.
        let mut me = Rec::default().with("ignoreSelf", Val::Bool(true));
        me.self_id = Some(5);
        create(&p, &mut me, flake(T + 9000), flake(T), "");
        // No nonce, junk nonce, tiny nonce, no payload.
        let mut none = j(r#"{"d":{"id":"5","author":{"id":"1"}}}"#);
        let mut junk = j(r#"{"d":{"id":"9999999999","nonce":"abc","author":{"id":"1"}}}"#);
        let mut ex = Vec::new();
        for pl in [&mut none, &mut junk] {
            let v = p.on_event(
                &mut Rec::default(),
                Event::Dispatch {
                    name: "MESSAGE_CREATE",
                    payload: pl,
                    extra: &mut ex,
                },
            );
            assert_eq!(v, Verdict::Pass);
        }
        assert_eq!(
            create(&p, &mut Rec::default(), flake(T + 9000), 12345, ""),
            Verdict::Pass
        );
        assert!(p.state.lock().log.is_empty());
        assert!(
            p.command()
                .run(&mut Rec::default(), &Args { options: vec![] })
                .starts_with("No slow")
        );
    }

    #[test]
    fn disabled_plugin_command_says_off() {
        let p = MessageLatency::new();
        let mut h = Rec::default();
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(
            p.command()
                .run(&mut h, &Args { options: vec![] })
                .contains("is off")
        );
    }
}
