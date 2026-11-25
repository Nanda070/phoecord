//! SpotifyShareCommands: share the Spotify track, album or artist you are listening to (Vencord:
//! SpotifyShareCommands).
//!
//! Ported from Vencord `src/plugins/spotifyShareCommands/index.ts` (GPL-3.0-or-later, Copyright
//! (c) 2022 Vendicated and contributors; author katlyn). Vencord reads the client's
//! `SpotifyStore` and sends a normal message with the link (and an optional text before it):
//!
//! - `/phoecord track` → `https://open.spotify.com/track/<track id>`
//! - `/phoecord album` → `https://open.spotify.com/album/<album id>`
//! - `/phoecord artist` → `https://open.spotify.com/artist/<first artist id>`
//!
//! The proxy has no `SpotifyStore`, so the plugin remembers your last Spotify activity from what
//! passes through it: the presence the client sends (gateway `op 3`, the activity with
//! `name: "Spotify"`, `sync_id` = track id and `metadata.album_id` / `metadata.artist_ids`) and
//! your sessions in `SESSIONS_REPLACE`. A presence without a Spotify activity means you stopped.
//! The message goes out with `POST /channels/{id}/messages` through `Host::rest`; nothing else is
//! shown when it worked. Not ported: answering the reply you were writing (`PendingReplyStore`).
//!
//! Unverified against a live client: the field names of the Spotify activity (taken from the
//! activities other users' presences carry).

use crate::commands::{Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer};
use crate::state::{Shared, dispatch_data};
use phoecord_core::plugin::{API_VERSION, HostResponse};
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "spotify_share_commands";
const SUBS: [&str; 3] = ["track", "album", "artist"];
/// Gateway opcode of a presence update.
const PRESENCE: i64 = 3;
/// Activity type "Listening to".
const LISTENING: i64 = 2;

/// What the links are made of.
#[derive(Clone, Debug, Default, PartialEq)]
struct Track {
    id: Option<String>,
    album: Option<String>,
    artist: Option<String>,
}

pub struct SpotifyShareCommands {
    manifest: Manifest,
    kept: Kept,
    /// `None`: not listening (or not seen yet).
    track: Shared<Option<Track>>,
}

impl SpotifyShareCommands {
    pub fn new() -> SpotifyShareCommands {
        SpotifyShareCommands {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "SpotifyShareCommands".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Share your current Spotify track, album or artist with a command"
                    .into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("SpotifyShareCommands".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            kept: Kept::default(),
            track: Shared::default(),
        }
    }

    /// `/phoecord track`, `album` and `artist`.
    pub fn commands(&self) -> Vec<Box<dyn Subcommand>> {
        let make = |name: &'static str, description: &'static str| {
            kept_answer(PLUGIN_ID, name, description, &self.kept)
                .option("message", "Text to send before the link", false)
                .boxed()
        };
        vec![
            make("track", "Share your current Spotify track in chat"),
            make("album", "Share your current Spotify album in chat"),
            make("artist", "Share your current Spotify artist in chat"),
        ]
    }

    fn share(&self, host: &mut dyn Host, inv: &Invocation) -> String {
        let Some(track) = self.track.lock().clone() else {
            return host.t("spotify_share_commands-not-listening", &[]);
        };
        // Local files have no Spotify id (Vencord: "local tracks have an id of null").
        let Some(id) = track.id.as_deref() else {
            return host.t("spotify_share_commands-not-on-spotify", &[]);
        };
        let url = match inv.sub.as_str() {
            "track" => Some(format!("https://open.spotify.com/track/{id}")),
            "album" => track
                .album
                .map(|a| format!("https://open.spotify.com/album/{a}")),
            _ => track
                .artist
                .map(|a| format!("https://open.spotify.com/artist/{a}")),
        };
        let Some(url) = url else {
            return host.t("spotify_share_commands-not-on-spotify", &[]);
        };
        let Some(channel) = inv.channel_id else {
            return host.t("spotify_share_commands-failed", &[("error", "no channel")]);
        };
        let content = match inv.get("message") {
            Some(m) => format!("{m} {url}"),
            None => url,
        };
        let mut body = Val::map();
        body.set("content", Val::Str(content));
        match host.rest(
            "POST",
            &format!("/channels/{channel}/messages"),
            Some(&body),
        ) {
            Ok(HostResponse {
                status: 200..=299, ..
            }) => String::new(),
            Ok(HostResponse { status, .. }) => host.t(
                "spotify_share_commands-failed",
                &[("error", &format!("HTTP {status}"))],
            ),
            Err(e) => host.t("spotify_share_commands-failed", &[("error", &e)]),
        }
    }
}

impl Default for SpotifyShareCommands {
    fn default() -> Self {
        Self::new()
    }
}

/// A Spotify "Listening to" activity in a list of activities, as a [`Track`].
fn spotify(activities: &Val) -> Option<Track> {
    let a = activities.as_list().iter().find(|a| {
        a.get("type").and_then(Val::as_i64) == Some(LISTENING)
            && a.str_at(&["name"]) == Some("Spotify")
    })?;
    let text = |path: &[&str]| a.str_at(path).filter(|s| !s.is_empty()).map(str::to_string);
    Some(Track {
        id: text(&["sync_id"]),
        album: text(&["metadata", "album_id"]),
        artist: a
            .at(&["metadata", "artist_ids"])
            .and_then(|ids| ids.as_list().first())
            .and_then(Val::as_str)
            .map(str::to_string),
    })
}

impl Plugin for SpotifyShareCommands {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            EventKind::Send { op } => *op == PRESENCE,
            EventKind::Dispatch { name } => *name == "SESSIONS_REPLACE",
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if op == PRESENCE => {
                if let Some(activities) = payload.at(&["d", "activities"]) {
                    *self.track.lock() = spotify(activities);
                }
            }
            Event::Dispatch {
                name: "SESSIONS_REPLACE",
                payload,
                ..
            } => {
                let found = dispatch_data(payload)
                    .as_list()
                    .iter()
                    .find_map(|s| s.get("activities").and_then(spotify));
                *self.track.lock() = found;
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if let Some(inv) = invocation(body)
                    && SUBS.contains(&inv.sub.as_str())
                {
                    let answer = self.share(host, &inv);
                    *self.kept.lock() = Some(answer);
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

    const LISTENING_TO: &str = r#"{"op":3,"d":{"status":"online","activities":[
        {"type":0,"name":"Game"},
        {"type":2,"name":"Spotify","sync_id":"T1","details":"Song",
         "metadata":{"album_id":"A1","artist_ids":["R1","R2"]}}]}}"#;

    fn presence(p: &SpotifyShareCommands, h: &mut Rec, frame: &str) {
        let mut payload = j(frame);
        let v = p.on_event(
            h,
            Event::Send {
                op: 3,
                payload: &mut payload,
            },
        );
        assert_eq!(v, Verdict::Pass);
        assert_eq!(payload, j(frame), "the presence is never changed");
    }

    fn run(p: &SpotifyShareCommands, h: &mut Rec, sub: &str, message: Option<&str>) -> String {
        let opts = message.map_or(String::new(), |m| {
            format!(r#"{{"type":3,"name":"message","value":"{m}"}}"#)
        });
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
    }

    fn sent(h: &Rec, i: usize) -> &str {
        assert_eq!(h.rest_calls[i].0, "POST /channels/9/messages");
        h.rest_calls[i]
            .1
            .as_ref()
            .unwrap()
            .str_at(&["content"])
            .unwrap()
    }

    #[test]
    fn sends_track_album_and_artist_links_like_vencord() {
        let p = SpotifyShareCommands::new();
        let mut h = Rec::default().answer("POST", "/channels/9/messages", 200, "{}");
        presence(&p, &mut h, LISTENING_TO);
        assert_eq!(run(&p, &mut h, "track", None), "");
        assert_eq!(sent(&h, 0), "https://open.spotify.com/track/T1");
        assert_eq!(run(&p, &mut h, "album", Some("listen")), "");
        assert_eq!(sent(&h, 1), "listen https://open.spotify.com/album/A1");
        assert_eq!(run(&p, &mut h, "artist", None), "");
        assert_eq!(sent(&h, 2), "https://open.spotify.com/artist/R1");
    }

    #[test]
    fn stopping_local_files_and_failures_are_explained() {
        let p = SpotifyShareCommands::new();
        let mut h = Rec::default();
        assert_eq!(
            run(&p, &mut h, "track", None),
            "You're not listening to any music."
        );
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"activities":[{"type":2,"name":"Spotify","details":"local.mp3"}]}}"#,
        );
        assert_eq!(
            run(&p, &mut h, "track", None),
            "Failed to find the track on spotify."
        );
        presence(&p, &mut h, LISTENING_TO);
        assert!(run(&p, &mut h, "track", None).contains("HTTP 404"));
        presence(&p, &mut h, r#"{"op":3,"d":{"activities":[]}}"#);
        assert_eq!(
            run(&p, &mut h, "album", None),
            "You're not listening to any music."
        );
        assert_eq!(h.rest_calls.len(), 1);
    }

    #[test]
    fn learns_the_track_from_sessions_replace() {
        let p = SpotifyShareCommands::new();
        let mut h = Rec::default().answer("POST", "/channels/9/messages", 200, "{}");
        let mut frame = j(r#"{"op":0,"t":"SESSIONS_REPLACE","d":[
            {"session_id":"a","activities":[]},
            {"session_id":"b","activities":[{"type":2,"name":"Spotify","sync_id":"T2"}]}]}"#);
        let mut extra = Vec::new();
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: "SESSIONS_REPLACE",
                payload: &mut frame,
                extra: &mut extra,
            },
        );
        assert_eq!(run(&p, &mut h, "track", None), "");
        assert_eq!(sent(&h, 0), "https://open.spotify.com/track/T2");
        assert_eq!(
            run(&p, &mut h, "album", None),
            "Failed to find the track on spotify."
        );
    }

    #[test]
    fn wants_only_its_traffic() {
        let p = SpotifyShareCommands::new();
        assert!(p.wants(&EventKind::Send { op: 3 }));
        assert!(!p.wants(&EventKind::Send { op: 4 }));
        assert!(p.wants(&EventKind::Dispatch {
            name: "SESSIONS_REPLACE"
        }));
        assert!(!p.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/interactions"
        }));
    }
}
