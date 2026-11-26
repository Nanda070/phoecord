//! ReadAllNotificationsButton: mark every unread server channel as read (data part).
//!
//! Ported from Vencord `src/plugins/readAllNotificationsButton/index.tsx` (GPL-3.0-or-later,
//! Copyright (c) 2023 Vendicated and contributors; author kemo). Vencord puts a "Read All" button
//! above the server list; it collects every channel and joined thread of every server that has
//! unread messages and dispatches `BULK_ACK` to the client. Here the button is the command
//! `/phoecord readall`, and the ack goes to Discord as `POST /read-states/ack-bulk` (what the
//! client's own `BULK_ACK` sends), so every device is marked read.
//!
//! How it knows what is unread: the plugin follows `READY` (read states, channels), `GUILD_CREATE`,
//! channel updates, `MESSAGE_CREATE` and `MESSAGE_ACK`, and keeps only ids: for each text, voice,
//! announcement, stage channel and joined thread the last message id and the last read id. A
//! channel counts as unread when its last message is newer than the last read one. Channels
//! Discord never sent a read state for are not touched. Direct messages are not included, as in
//! Vencord.
//!
//! Limits: nothing is sent automatically. One command run sends one request with at most
//! `max_channels` channels (default 100, the most the client sends at once, never more than 100);
//! if more are unread, the reply says how many remain and you run it again. The host allows 5
//! Discord calls per 5 s per plugin, so rapid repeats beyond that are refused politely.
//!
//! Not ported: the button itself (Phase 2).

use crate::commands::{FnCmd, Subcommand};
use crate::state::{Shared, dispatch_data};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;

const PLUGIN_ID: &str = "read_all_notifications_button";
const BATCH: i64 = 100;
const EVENTS: &[&str] = &[
    "READY",
    "GUILD_CREATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
    "THREAD_CREATE",
    "THREAD_UPDATE",
    "THREAD_DELETE",
    "MESSAGE_CREATE",
    "MESSAGE_ACK",
];

/// Text, voice, announcement and stage channels and the three thread kinds.
fn readable_kind(kind: i64) -> bool {
    matches!(kind, 0 | 2 | 5 | 13 | 10 | 11 | 12)
}

#[derive(Clone, Copy, Default)]
struct Chan {
    guild: u64,
    last: u64,
    /// `None` until Discord (or an ack) told us.
    read: Option<u64>,
}

/// Channel id -> last and read message ids.
#[derive(Default)]
struct Reads {
    chans: HashMap<u64, Chan>,
}

impl Reads {
    fn add_channel(&mut self, guild: u64, c: &Val) {
        let Some(id) = c.id_at(&["id"]) else { return };
        if !c
            .get("type")
            .and_then(Val::as_i64)
            .is_some_and(readable_kind)
        {
            self.chans.remove(&id);
            return;
        }
        let last = c.id_at(&["last_message_id"]).unwrap_or(0);
        let e = self.chans.entry(id).or_default();
        e.guild = guild;
        e.last = e.last.max(last);
    }

    fn load_read_states(&mut self, d: &Val) {
        let rs = d.get("read_state");
        let entries = match rs {
            Some(Val::Map(_)) => rs.and_then(|m| m.get("entries")),
            other => other,
        };
        for e in entries.map(Val::as_list).unwrap_or(&[]) {
            if let (Some(id), Some(read)) = (e.id_at(&["id"]), e.id_at(&["last_message_id"]))
                && let Some(c) = self.chans.get_mut(&id)
            {
                c.read = Some(read);
            }
        }
    }

    fn observe(&mut self, name: &str, d: &Val, self_id: Option<u64>) {
        match name {
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.add_guild(g);
                }
                self.load_read_states(d);
            }
            "GUILD_CREATE" => self.add_guild(d),
            "GUILD_DELETE" => {
                if d.get("unavailable").and_then(Val::as_bool) != Some(true)
                    && let Some(g) = d.id_at(&["id"])
                {
                    self.chans.retain(|_, c| c.guild != g);
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
                if let Some(g) = d.id_at(&["guild_id"]) {
                    self.add_channel(g, d);
                }
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    self.chans.remove(&id);
                }
            }
            "MESSAGE_CREATE" => {
                let (Some(ch), Some(mid)) = (d.id_at(&["channel_id"]), d.id_at(&["id"])) else {
                    return;
                };
                if let Some(c) = self.chans.get_mut(&ch) {
                    // A channel we had no read state for was caught up before this message.
                    if c.read.is_none() && c.last > 0 {
                        c.read = Some(c.last);
                    }
                    c.last = c.last.max(mid);
                    // The client acks your own messages itself.
                    if d.id_at(&["author", "id"]).is_some() && d.id_at(&["author", "id"]) == self_id
                    {
                        c.read = Some(c.last);
                    }
                }
            }
            "MESSAGE_ACK" => {
                if let (Some(ch), Some(mid)) = (d.id_at(&["channel_id"]), d.id_at(&["message_id"]))
                    && let Some(c) = self.chans.get_mut(&ch)
                {
                    c.read = Some(mid);
                }
            }
            _ => {}
        }
    }

    fn add_guild(&mut self, g: &Val) {
        let Some(gid) = g.id_at(&["id"]) else { return };
        for list in ["channels", "threads"] {
            for c in g.get(list).map(Val::as_list).unwrap_or(&[]) {
                self.add_channel(gid, c);
            }
        }
    }

    /// `(channel, last message)` of every unread channel, sorted by channel id.
    fn unread(&self) -> Vec<(u64, u64)> {
        let mut v: Vec<_> = self
            .chans
            .iter()
            .filter(|(_, c)| c.last > 0 && c.read.is_some_and(|r| c.last > r))
            .map(|(id, c)| (*id, c.last))
            .collect();
        v.sort_unstable();
        v
    }
}

pub struct ReadAllNotificationsButton {
    manifest: Manifest,
    reads: Shared<Reads>,
}

impl ReadAllNotificationsButton {
    pub fn new() -> ReadAllNotificationsButton {
        ReadAllNotificationsButton {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "ReadAllNotificationsButton",
                    env!("CARGO_PKG_VERSION"),
                    "Read all server notifications at once",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("ReadAllNotificationsButton".into());
                m
            },
            reads: Shared::default(),
        }
    }

    /// `/phoecord readall`. Register it with `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let reads = self.reads.clone();
        FnCmd::new(
            PLUGIN_ID,
            "readall",
            "Mark all unread server channels as read",
            move |host, _args| {
                let max = host
                    .setting("max_channels")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(BATCH)
                    .clamp(1, BATCH) as usize;
                let unread = reads.lock().unread();
                if unread.is_empty() {
                    return "Nothing unread (as far as PhoeCord has seen since it started)."
                        .into();
                }
                let batch = &unread[..unread.len().min(max)];
                let mut states = Vec::with_capacity(batch.len());
                for (ch, last) in batch {
                    let mut s = Val::map();
                    s.set("channel_id", Val::Str(ch.to_string()));
                    s.set("message_id", Val::Str(last.to_string()));
                    s.set("read_state_type", Val::Int(0));
                    states.push(s);
                }
                let mut body = Val::map();
                body.set("read_states", Val::List(states));
                match host.rest("POST", "/read-states/ack-bulk", Some(&body)) {
                    Ok(r) if (200..300).contains(&r.status) => {
                        let mut st = reads.lock();
                        for (ch, last) in batch {
                            if let Some(c) = st.chans.get_mut(ch) {
                                c.read = Some(*last);
                            }
                        }
                        let left = unread.len() - batch.len();
                        if left == 0 {
                            format!("Marked {} channels as read.", batch.len())
                        } else {
                            format!(
                                "Marked {} channels as read, {left} more are unread: run `/phoecord readall` again.",
                                batch.len()
                            )
                        }
                    }
                    Ok(r) => format!("Discord answered {}; nothing was marked.", r.status),
                    Err(e) => format!("Could not mark channels as read: {e}"),
                }
            },
        )
        .boxed()
    }
}

impl Default for ReadAllNotificationsButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for ReadAllNotificationsButton {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::Dispatch { name } if EVENTS.contains(name))
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::Dispatch { name, payload, .. } = event {
            self.reads
                .lock()
                .observe(name, dispatch_data(payload), host.self_id());
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn feed(p: &ReadAllNotificationsButton, h: &mut Rec, name: &str, payload: &str) {
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

    const READY: &str = r#"{"guilds":[{"id":"1","channels":[
        {"id":"10","type":0,"last_message_id":"500"},
        {"id":"11","type":0,"last_message_id":"600"},
        {"id":"12","type":4},
        {"id":"13","type":15,"last_message_id":"700"}]}],
        "read_state":{"entries":[{"id":"10","last_message_id":"400"},{"id":"11","last_message_id":"600"},{"id":"13","last_message_id":"1"}]}}"#;

    #[test]
    fn unread_channels_are_acked_in_bulk() {
        let p = ReadAllNotificationsButton::new();
        let mut h = Rec::default().answer("POST", "/read-states/ack-bulk", 204, "{}");
        feed(&p, &mut h, "READY", READY);
        let out = p.command().run(&mut h, &Args { options: vec![] });
        assert_eq!(out, "Marked 1 channels as read.");
        let body = h.rest_calls[0].1.clone().unwrap();
        let s = &body.get("read_states").unwrap().as_list()[0];
        assert_eq!(s.str_at(&["channel_id"]), Some("10"));
        assert_eq!(s.str_at(&["message_id"]), Some("500"));
        // Now nothing is left, and no second request goes out.
        let out = p.command().run(&mut h, &Args { options: vec![] });
        assert!(out.starts_with("Nothing unread"));
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn unread_channels_are_acked_from_a_real_gateway_frame() {
        use phoecord_plugin_runtime::Chain;
        let p = ReadAllNotificationsButton::new();
        let cmd = p.command();
        let chain = Chain::new(vec![Box::new(p)]);
        let mut h = Rec::default().answer("POST", "/read-states/ack-bulk", 204, "{}");
        let mut frame = j(&format!(r#"{{"op":0,"t":"READY","s":1,"d":{READY}}}"#));
        chain.dispatch(&mut h, "READY", &mut frame, &mut Vec::new());
        let out = cmd.run(&mut h, &Args { options: vec![] });
        assert_eq!(out, "Marked 1 channels as read.");
    }

    #[test]
    fn messages_and_acks_move_the_state() {
        let p = ReadAllNotificationsButton::new();
        let mut h = Rec {
            self_id: Some(7),
            ..Rec::default()
        };
        feed(&p, &mut h, "READY", READY);
        feed(
            &p,
            &mut h,
            "MESSAGE_CREATE",
            r#"{"id":"650","channel_id":"11","author":{"id":"3"}}"#,
        );
        feed(
            &p,
            &mut h,
            "MESSAGE_CREATE",
            r#"{"id":"900","channel_id":"10","author":{"id":"7"}}"#,
        );
        feed(
            &p,
            &mut h,
            "MESSAGE_ACK",
            r#"{"channel_id":"10","message_id":"900"}"#,
        );
        feed(
            &p,
            &mut h,
            "MESSAGE_CREATE",
            r#"{"id":"1","channel_id":"99","author":{"id":"3"}}"#,
        );
        assert_eq!(p.reads.lock().unread(), [(11, 650)]);
        feed(&p, &mut h, "GUILD_DELETE", r#"{"id":"1"}"#);
        assert!(p.reads.lock().unread().is_empty());
    }

    #[test]
    fn batch_limit_failures_and_junk() {
        let p = ReadAllNotificationsButton::new();
        let mut h = Rec::default().with("max_channels", Val::Int(1));
        let many: Vec<String> = (1..=3)
            .map(|i| format!(r#"{{"id":"{i}0","type":0,"last_message_id":"90"}}"#))
            .collect();
        let rs: Vec<String> = (1..=3)
            .map(|i| format!(r#"{{"id":"{i}0","last_message_id":"5"}}"#))
            .collect();
        feed(
            &p,
            &mut h,
            "READY",
            &format!(
                r#"{{"guilds":[{{"id":"1","channels":[{}]}}],"read_state":[{}]}}"#,
                many.join(","),
                rs.join(",")
            ),
        );
        // No answer scripted: the 404 is reported and nothing is marked read.
        let out = p.command().run(&mut h, &Args { options: vec![] });
        assert!(out.contains("404"), "{out}");
        assert_eq!(p.reads.lock().unread().len(), 3);
        let mut h = Rec::default().with("max_channels", Val::Int(1)).answer(
            "POST",
            "/read-states/ack-bulk",
            204,
            "{}",
        );
        let out = p.command().run(&mut h, &Args { options: vec![] });
        assert!(out.contains("2 more are unread"), "{out}");
        assert_eq!(
            h.rest_calls[0]
                .1
                .as_ref()
                .unwrap()
                .get("read_states")
                .unwrap()
                .as_list()
                .len(),
            1
        );
        for junk in [r#"null"#, r#"{"guilds":"x","read_state":5}"#, r#"[1]"#] {
            feed(&p, &mut h, "READY", junk);
        }
    }
}
