//! SendTimestamps: `` `12:34` `` in a message becomes a Discord timestamp (Vencord: SendTimestamps).
//!
//! Ported from Vencord `src/plugins/sendTimestamps/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2022 Vendicated and contributors; authors Ven, Tyler, Grzesiek11). `onBeforeMessageSend` and
//! `parseTime` are translated; the chat bar button and the picker modal are UI (Phase 2).
//!
//! A time in backticks, ``/`\d{1,2}:\d{2} ?(?:AM|PM)?`/i``, becomes `<t:UNIX:t>`: the next moment
//! the local clock shows that time (today, or tomorrow if it has passed). `24:00` is midnight.
//! Text that is not a valid time (`99:99`) is left as it is.
//!
//! Setting (`[plugins.send_timestamps]`): `replace_message_contents` (default `true`, as Vencord).
//! Extra setting: `utc_offset_minutes`, the local zone's offset from UTC, for systems where the
//! zone cannot be read (see below).
//!
//! Time zone without `unsafe`: the `time` crate's `UtcOffset::local_offset_at` (feature
//! `local-offset`) reads it; it works on Windows, and on Unix only while the process is
//! single-threaded, which the proxy is not, so there it answers "indeterminate". Then the plugin
//! uses `utc_offset_minutes` if set, otherwise UTC, and logs that once.

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Verdict};
use std::sync::atomic::{AtomicBool, Ordering};
use time::{Date, Duration, OffsetDateTime, Time, UtcOffset};

pub struct SendTimestamps {
    manifest: Manifest,
    warned: AtomicBool,
}

impl SendTimestamps {
    pub fn new() -> SendTimestamps {
        SendTimestamps {
            manifest: Manifest {
                id: "send_timestamps".into(),
                name: "SendTimestamps".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Turn `12:34` in your messages into a Discord timestamp".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("SendTimestamps".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
            },
            warned: AtomicBool::new(false),
        }
    }
}

impl Default for SendTimestamps {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/messages` (send) and `PATCH /channels/{id}/messages/{id}` (edit).
fn is_message_send(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    matches!(
        (method, seg.as_slice()),
        ("POST", ["channels", _, "messages"]) | ("PATCH", ["channels", _, "messages", _])
    )
}

/// Where the local zone comes from.
trait Zone {
    /// The offset of the local zone at this instant.
    fn offset_at(&self, at: OffsetDateTime) -> UtcOffset;
}

/// The system zone, else `fallback`.
struct System {
    fallback: UtcOffset,
}

impl Zone for System {
    fn offset_at(&self, at: OffsetDateTime) -> UtcOffset {
        UtcOffset::local_offset_at(at).unwrap_or(self.fallback)
    }
}

/// Vencord's `parseTime` on the inside of the backticks, `"6:30 PM"`: the unix time of the next
/// time the clock shows it, or `None` if it is not a time.
fn parse_time(inner: &str, now: OffsetDateTime, zone: &dyn Zone) -> Option<i64> {
    let (clock, pm) = match inner.trim_end().to_ascii_uppercase() {
        s if s.ends_with("AM") => (s[..s.len() - 2].trim_end().to_string(), Some(false)),
        s if s.ends_with("PM") => (s[..s.len() - 2].trim_end().to_string(), Some(true)),
        s => (s, None),
    };
    let (h, m) = clock.split_once(':')?;
    let (mut h, m): (u8, u8) = (h.parse().ok()?, m.parse().ok()?);
    if m > 59 {
        return None;
    }
    let mut extra_day = false;
    match pm {
        Some(pm) => {
            if !(1..=12).contains(&h) && h != 0 {
                return None;
            }
            h = h % 12 + if pm { 12 } else { 0 };
        }
        None if h == 24 && m == 0 => {
            h = 0;
            extra_day = true;
        }
        None if h > 23 => return None,
        None => {}
    }
    let today = now.to_offset(zone.offset_at(now)).date();
    let at = |date: Date| -> Option<OffsetDateTime> {
        let naive = date.with_time(Time::from_hms(h, m, 0).ok()?).assume_utc();
        // The offset can change across the day (DST): ask for it at the wanted moment.
        let guess = naive.to_offset(zone.offset_at(naive));
        let offset = zone.offset_at(guess);
        Some(
            date.with_time(Time::from_hms(h, m, 0).ok()?)
                .assume_offset(offset),
        )
    };
    let mut date = today;
    if extra_day {
        date = date.checked_add(Duration::days(1))?;
    }
    let mut when = at(date)?;
    // "tomorrow can be 23 or 25 hours away across dst changes": add a calendar day, not 24 hours.
    if now > when {
        when = at(date.checked_add(Duration::days(1))?)?;
    }
    Some(when.unix_timestamp())
}

/// Vencord's `` msg.content.replace(/`\d{1,2}:\d{2} ?(?:AM|PM)?`/gi, parseTime) ``; `None` if
/// nothing was replaced.
fn replace_times(text: &str, now: OffsetDateTime, zone: &dyn Zone) -> Option<String> {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut last, mut i, mut changed) = (0, 0, false);
    while i < b.len() {
        if b[i] == b'`'
            && let Some(len) = time_len(&b[i + 1..])
            && b.get(i + 1 + len) == Some(&b'`')
        {
            let inner = &text[i + 1..i + 1 + len];
            if let Some(unix) = parse_time(inner, now, zone) {
                out.push_str(&text[last..i]);
                out.push_str(&format!("<t:{unix}:t>"));
                last = i + len + 2;
                i = last;
                changed = true;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    changed.then_some(out)
}

/// Length of `\d{1,2}:\d{2} ?(?:AM|PM)?` (case-insensitive) at the start of `b`.
fn time_len(b: &[u8]) -> Option<usize> {
    let digits = |from: usize, max: usize| {
        b[from.min(b.len())..]
            .iter()
            .take(max)
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let hours = digits(0, 2);
    if hours == 0 || b.get(hours) != Some(&b':') || digits(hours + 1, 2) != 2 {
        return None;
    }
    let mut end = hours + 3;
    // The backtick must follow either directly or after an optional space and AM/PM; the regex
    // backtracks, so try the longest form first.
    let suffix = |at: usize| {
        b.get(at..at + 2)
            .filter(|s| s.eq_ignore_ascii_case(b"AM") || s.eq_ignore_ascii_case(b"PM"))
            .map(|_| at + 2)
    };
    let spaced = b.get(end) == Some(&b' ');
    if let Some(e) = suffix(end + usize::from(spaced)).or_else(|| suffix(end)) {
        end = e;
    } else if spaced {
        end += 1;
    }
    Some(end)
}

impl Plugin for SendTimestamps {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_message_send(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
        else {
            return Verdict::Pass;
        };
        if !is_message_send(method, route) {
            return Verdict::Pass;
        }
        if host
            .setting("replace_message_contents")
            .and_then(|v| v.as_bool())
            == Some(false)
        {
            return Verdict::Pass;
        }
        let Some(content) = body.get_mut("content") else {
            return Verdict::Pass;
        };
        let Some(text) = content.as_str() else {
            return Verdict::Pass;
        };
        if !text.contains('`') {
            return Verdict::Pass;
        }
        let configured = host
            .setting("utc_offset_minutes")
            .and_then(|v| v.as_i64())
            .and_then(|m| UtcOffset::from_whole_seconds(i32::try_from(m * 60).ok()?).ok());
        let now = OffsetDateTime::now_utc();
        if UtcOffset::local_offset_at(now).is_err()
            && configured.is_none()
            && !self.warned.swap(true, Ordering::Relaxed)
        {
            host.log(
                "SendTimestamps: the system time zone is unknown here; using UTC. \
                 Set utc_offset_minutes in [plugins.send_timestamps].",
            );
        }
        let zone = System {
            fallback: configured.unwrap_or(UtcOffset::UTC),
        };
        // The configured offset is used only when the system zone is unreadable.
        let Some(new) = replace_times(text, now, &zone) else {
            return Verdict::Pass;
        };
        *content = content.str_like(&new);
        Verdict::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_core::Val;
    use phoecord_plugin_runtime::{Chain, Outcome};
    use time::macros::{datetime, offset};

    struct Fixed(UtcOffset);

    impl Zone for Fixed {
        fn offset_at(&self, _: OffsetDateTime) -> UtcOffset {
            self.0
        }
    }

    fn at(now: OffsetDateTime, text: &str, zone: UtcOffset) -> Option<String> {
        replace_times(text, now, &Fixed(zone))
    }

    #[test]
    fn matches_send_and_edit_only() {
        assert!(is_message_send("POST", "/channels/1/messages"));
        assert!(is_message_send("PATCH", "/channels/1/messages/2"));
        assert!(!is_message_send("GET", "/channels/1/messages"));
        assert!(!is_message_send("POST", "/channels/1/messages/2"));
    }

    #[test]
    fn upcoming_time_today_and_passed_time_tomorrow() {
        // 2024-06-10 10:00 at UTC+2 is 08:00 UTC.
        let now = datetime!(2024-06-10 08:00 UTC);
        let z = offset!(+2);
        // 17:59 local today = 15:59 UTC.
        let t = datetime!(2024-06-10 15:59 UTC).unix_timestamp();
        assert_eq!(
            at(now, "meet `17:59` ok", z).as_deref(),
            Some(format!("meet <t:{t}:t> ok").as_str())
        );
        // 3:51 local has passed today: tomorrow 03:51 local = 01:51 UTC on the 11th.
        let t = datetime!(2024-06-11 01:51 UTC).unix_timestamp();
        assert_eq!(at(now, "`3:51`", z), Some(format!("<t:{t}:t>")));
    }

    #[test]
    fn twelve_hour_clock_and_midnight() {
        let now = datetime!(2024-06-10 00:00 UTC);
        let z = UtcOffset::UTC;
        let unix =
            |s: &str| datetime!(2024-06-10 00:00 UTC).unix_timestamp() + s.parse::<i64>().unwrap();
        // 12:00 AM is 00:00 (not after "now" equal), 12:00 PM is noon, 3:30pm without space.
        assert_eq!(
            at(now, "`12:00 AM`", z),
            Some(format!("<t:{}:t>", unix("0")))
        );
        assert_eq!(
            at(now, "`12:00 PM`", z),
            Some(format!("<t:{}:t>", unix("43200")))
        );
        assert_eq!(
            at(now, "`3:30pm`", z),
            Some(format!("<t:{}:t>", unix("55800")))
        );
        // 24:00 is the next midnight.
        assert_eq!(
            at(now, "`24:00`", z),
            Some(format!("<t:{}:t>", unix("86400")))
        );
    }

    #[test]
    fn leaves_everything_else_alone() {
        let now = datetime!(2024-06-10 08:00 UTC);
        let z = UtcOffset::UTC;
        for text in [
            "12:34 no backticks",
            "`99:99`",
            "`25:00`",
            "`13:00 PM`",
            "`1:2`",
            "``code`` and `abc`",
            "`12:34 extra`",
            "`12:345`",
        ] {
            assert_eq!(at(now, text, z), None, "{text}");
        }
        // Several in one message, other text untouched.
        assert!(at(now, "`12:34 `", z).is_some());
        let out = at(now, "`9:00` and `23:00` `x`", z).unwrap();
        assert!(out.starts_with("<t:") && out.ends_with(":t> `x`"), "{out}");
    }

    #[test]
    fn dst_changes_use_the_offset_at_the_target_moment() {
        struct Dst;
        impl Zone for Dst {
            // Offset +1 before 2024-03-31 01:00 UTC, +2 from then on.
            fn offset_at(&self, at: OffsetDateTime) -> UtcOffset {
                if at < datetime!(2024-03-31 01:00 UTC) {
                    offset!(+1)
                } else {
                    offset!(+2)
                }
            }
        }
        // 2024-03-30 12:00 UTC; `12:00` local passed, so tomorrow (the 31st) 12:00 at +2 = 10:00 UTC.
        let now = datetime!(2024-03-30 12:00 UTC);
        let got = replace_times("`12:00`", now, &Dst).unwrap();
        let want = datetime!(2024-03-31 10:00 UTC).unix_timestamp();
        assert_eq!(got, format!("<t:{want}:t>"));
    }

    #[test]
    fn chain_respects_the_setting_and_leaves_plain_messages() {
        let chain = Chain::new(vec![Box::new(SendTimestamps::new())]);
        let mut h = Cfg::default().with("utc_offset_minutes", Val::Int(0));
        let mut body = j(r#"{"content":"at `18:30` ok","nonce":"7"}"#);
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut body));
        assert_eq!(out, Outcome::Changed);
        let text = body.str_at(&["content"]).unwrap();
        assert!(
            text.starts_with("at <t:") && text.ends_with(":t> ok"),
            "{text}"
        );
        assert_eq!(body.str_at(&["nonce"]), Some("7"));

        let mut plain = j(r#"{"content":"hello"}"#);
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut plain));
        assert_eq!(out, Outcome::Untouched);

        let mut off = Cfg::default().with("replace_message_contents", Val::Bool(false));
        let mut body = j(r#"{"content":"at `18:30`"}"#);
        let out = chain.rest_request(&mut off, "POST", "/channels/9/messages", Some(&mut body));
        assert_eq!(out, Outcome::Untouched);
    }
}
