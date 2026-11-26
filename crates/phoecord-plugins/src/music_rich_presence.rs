//! MusicRichPresence: Rich Presence for what you scrobble to Last.fm or ListenBrainz (Vencord:
//! MusicRichPresence, formerly LastFMRichPresence).
//!
//! Ported from Vencord `src/plugins/musicRichPresence/` (GPL-3.0-or-later, Copyright (c) 2022
//! Sofia Lima, and Vendicated and contributors; authors Rini, Ven, angelcube, RuiNtD, blahajZip,
//! archeruwu). Every 16 seconds Vencord asks the scrobbler what is playing now and shows it as an
//! activity of the app `1108588077900898414`. The same here, with a plugin timer and
//! `Host::fetch_with`; the activity is added to the presence the client sends and sent again when
//! the track changes:
//!
//! - Last.fm: `user.getrecenttracks` on `ws.audioscrobbler.com` (Vencord's shared key, or
//!   `apiKey`), the track only while `nowplaying`;
//! - ListenBrainz: `/1/user/{name}/playing-now` on `api.listenbrainz.org`; links and cover from
//!   the listen's MusicBrainz ids, else a MusicBrainz search (`musicbrainz.org`, kept 15
//!   minutes), else a YouTube thumbnail of `origin_url`. The cover is Cover Art Archive's
//!   `front-500` image link (Vencord reads the first image of the release's list; Discord fetches
//!   the link itself, so no extra host is needed).
//!
//! Settings (`[plugins.music_rich_presence]`, Vencord's names and defaults): `scrobblerBackend`
//! (`lastfm`, `listenbrainz`), `apiKey`, `username` (required), `shareUsername` (false),
//! `clickableLinks` (true), `hideWithSpotify` (true), `hideWithActivity` (false), `statusName`
//! ("some music", `{artist}`, `{album}`, `{title}`), `statusDisplayType` (`off`, `artist`,
//! `track`), `nameFormat` (`status-name`, `service-name`, `artist-first`, `song-first`, `artist`,
//! `song`, `album`), `useListeningStatus` (false), `missingArt` (`logo`, `placeholder`),
//! `showLogo` (true), `showAlbumCover` (true).
//!
//! A self-hosted ListenBrainz-compatible instance (Vencord's `listenbrainz-compatible`): set
//! `listenbrainzHost = "lb.example.org"` and allow it with `fetch_hosts = ["lb.example.org"]` in
//! the same `[plugins.music_rich_presence]` section; `fetch` only reaches the manifest's hosts and
//! the ones the user lists there (task 2.4.2 c).

use crate::presence::{self, LISTENING, Owned};
use crate::state::{Shared, dispatch_data};
use phoecord_core::plugin::{HostResponse, TIMER_EVENT};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PLUGIN_ID: &str = "music_rich_presence";
const TAG: &str = "poll";
const POLL_MS: u64 = 16_000;
const DISCORD_APP_ID: &str = "1108588077900898414";
/// Vencord's shared Last.fm key ("essentially public information").
const LASTFM_API_KEY: &str = "790c37d90400163a5a5fe00d6ca32ef0";
const LASTFM_PLACEHOLDER_IMAGE_HASH: &str = "2a96cbd8b46e442fc41c2b86b821562f";
const LASTFM_HOST: &str = "ws.audioscrobbler.com";
const LISTENBRAINZ_HOST: &str = "api.listenbrainz.org";
const MUSICBRAINZ_HOST: &str = "musicbrainz.org";
const USER_AGENT: &str = "PhoeCord/0.5 (https://github.com/Nanda070/phoecord)";
const CACHE_FOR: Duration = Duration::from_secs(15 * 60);

/// Vencord's `TrackData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub name: String,
    pub album: String,
    pub artist: String,
    pub track_url: Option<String>,
    pub artist_url: Option<String>,
    pub album_url: Option<String>,
    pub image_url: Option<String>,
    pub service_name: Option<String>,
}

#[derive(Default)]
struct State {
    owned: Owned,
    polling: bool,
    assets: HashMap<String, String>,
    /// MusicBrainz answers by query.
    lookups: HashMap<String, (Instant, Track)>,
}

pub struct MusicRichPresence {
    manifest: Manifest,
    state: Shared<State>,
}

impl MusicRichPresence {
    pub fn new() -> MusicRichPresence {
        MusicRichPresence {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "MusicRichPresence",
                    env!("CARGO_PKG_VERSION"),
                    "Rich Presence for what you scrobble to Last.fm or ListenBrainz",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("MusicRichPresence".into());
                m.fetch = vec![
                    LASTFM_HOST.into(),
                    LISTENBRAINZ_HOST.into(),
                    MUSICBRAINZ_HOST.into(),
                ];
                m
            },
            state: Shared::default(),
        }
    }
}

impl Default for MusicRichPresence {
    fn default() -> Self {
        Self::new()
    }
}

fn setting_str(host: &dyn Host, key: &str, default: &str) -> String {
    host.setting(key)
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn setting_bool(host: &dyn Host, key: &str, default: bool) -> bool {
    host.setting(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// JavaScript's `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn get_json(host: &mut dyn Host, url: &str, headers: &[(&str, &str)]) -> Option<Val> {
    match host.fetch_with("GET", url, headers, None) {
        Ok(HostResponse {
            status: 200..=299,
            body: Some(b),
            ..
        }) => Some(b),
        Ok(r) => {
            host.log(&format!("{url}: HTTP {}", r.status));
            None
        }
        Err(e) => {
            host.log(&format!("{url}: {e}"));
            None
        }
    }
}

fn lastfm_url(path: &str) -> String {
    format!("https://www.last.fm{path}")
}

/// Vencord's `LastFMScrobbler.fetchTrackData`.
fn lastfm(host: &mut dyn Host, user: &str) -> Option<Track> {
    let key = setting_str(host, "apiKey", LASTFM_API_KEY);
    let url = format!(
        "https://{LASTFM_HOST}/2.0/?method=user.getrecenttracks&api_key={}&user={}&limit=1&format=json",
        encode_component(&key),
        encode_component(user)
    );
    let json = get_json(host, &url, &[])?;
    if json.get("error").is_some() {
        host.log(&format!(
            "Last.fm: {}",
            json.str_at(&["message"]).unwrap_or("error")
        ));
        return None;
    }
    let t = json
        .at(&["recenttracks", "track"])?
        .as_list()
        .first()?
        .clone();
    t.at(&["@attr", "nowplaying"])?;
    let artist = t.str_at(&["artist", "#text"]).unwrap_or("").to_string();
    let album = t.str_at(&["album", "#text"]).unwrap_or("").to_string();
    Some(Track {
        name: t
            .str_at(&["name"])
            .filter(|s| !s.is_empty())
            .unwrap_or("Unknown")
            .to_string(),
        track_url: t.str_at(&["url"]).map(str::to_string),
        artist_url: (!artist.is_empty())
            .then(|| lastfm_url(&format!("/music/{}", encode_component(&artist)))),
        album_url: Some(lastfm_url(&format!(
            "/music/{}/{}",
            encode_component(&artist),
            encode_component(&album)
        ))),
        image_url: t
            .get("image")
            .map(Val::as_list)
            .unwrap_or_default()
            .iter()
            .find(|i| i.str_at(&["size"]) == Some("large"))
            .and_then(|i| i.str_at(&["#text"]))
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        artist: if artist.is_empty() {
            "Unknown".into()
        } else {
            artist
        },
        album,
        service_name: None,
    })
}

fn lb_url(path: &str) -> String {
    format!("https://listenbrainz.org{path}")
}

fn cover(release: Option<&str>, group: Option<&str>, origin: Option<&str>) -> Option<String> {
    release
        .filter(|r| !r.is_empty())
        .map(|r| format!("https://coverartarchive.org/release/{r}/front-500"))
        .or_else(|| {
            group
                .filter(|g| !g.is_empty())
                .map(|g| format!("https://coverartarchive.org/release-group/{g}/front-500"))
        })
        .or_else(|| origin.and_then(youtube_thumbnail))
}

/// Vencord's `fallbackToYoutubeThumbnail`.
pub fn youtube_thumbnail(url: &str) -> Option<String> {
    let rest = url
        .trim_start_matches("https:")
        .trim_start_matches("http:")
        .trim_start_matches("//");
    let rest = rest
        .strip_prefix("www.")
        .or_else(|| rest.strip_prefix("m."))
        .unwrap_or(rest);
    let path = ["youtube.com", "youtube-nocookie.com", "youtu.be"]
        .iter()
        .find_map(|h| rest.strip_prefix(h))?;
    let path = path.strip_prefix('/')?;
    let id_part = if let Some(q) = path.split_once("?v=") {
        q.1
    } else if let Some(p) = ["embed/", "live/", "v/"]
        .iter()
        .find_map(|p| path.strip_prefix(p))
    {
        p
    } else {
        path
    };
    let id: String = id_part
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    (!id.is_empty()).then(|| format!("https://i.ytimg.com/vi/{id}/maxresdefault.jpg"))
}

/// The ListenBrainz API host: `listenbrainzHost` for a self-hosted instance (task 2.4.2 c), else
/// `api.listenbrainz.org`. A self-hosted host is reached only when the user also lists it in
/// `fetch_hosts` of this plugin's `config.toml` section; otherwise `fetch` refuses it and the
/// error is logged like any failed check.
fn listenbrainz_host(host: &dyn Host) -> String {
    host.setting("listenbrainzHost")
        .and_then(|v| v.as_str().map(|h| h.trim().to_ascii_lowercase()))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| LISTENBRAINZ_HOST.into())
}

/// Vencord's `ListenBrainzScrobbler.fetchTrackData` and `getUrls`.
fn listenbrainz(host: &mut dyn Host, st: &mut State, user: &str) -> Option<Track> {
    let url = format!(
        "https://{}/1/user/{}/playing-now",
        listenbrainz_host(host),
        encode_component(user)
    );
    let json = get_json(host, &url, &[])?;
    let listen = json.at(&["payload", "listens"])?.as_list().first()?.clone();
    if listen.get("playing_now").and_then(Val::as_bool) != Some(true) {
        return None;
    }
    let m = listen.get("track_metadata")?;
    let info = m.get("additional_info");
    let ai = |k: &str| info.and_then(|i| i.str_at(&[k])).filter(|s| !s.is_empty());
    let track_name = m.str_at(&["track_name"]).unwrap_or("").to_string();
    let artist = m.str_at(&["artist_name"]).unwrap_or("").to_string();
    let release = m.str_at(&["release_name"]).unwrap_or("").to_string();
    let mut track = Track {
        name: if track_name.is_empty() {
            "Unknown".into()
        } else {
            track_name.clone()
        },
        artist: artist.clone(),
        album: if release.is_empty() {
            "Unknown".into()
        } else {
            release.clone()
        },
        service_name: ai("music_service_name")
            .or_else(|| ai("submission_client"))
            .map(str::to_string),
        ..Track::default()
    };
    if let Some(recording) = ai("recording_mbid") {
        let (release_id, group) = (ai("release_mbid"), ai("release_group_mbid"));
        track.image_url = cover(release_id, group, ai("origin_url"));
        track.track_url = Some(lb_url(&format!("/track/{recording}")));
        track.album_url = release_id
            .map(|r| lb_url(&format!("/release/{r}")))
            .or_else(|| group.map(|g| lb_url(&format!("/release-group/{g}"))));
        track.artist_url = info
            .and_then(|i| i.get("artist_mbids"))
            .and_then(|a| a.as_list().first())
            .and_then(Val::as_str)
            .map(|a| lb_url(&format!("/artist/{a}")));
        return Some(track);
    }
    // No MusicBrainz ids: search by ISRC, then by name.
    let mut q = format!("artist:\"{artist}\" AND recording:\"{track_name}\"");
    if !release.is_empty() {
        q.push_str(&format!(" AND release:\"{release}\""));
    }
    let escape = |s: String| {
        s.chars()
            .flat_map(|c| {
                if "!()*-~".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect::<String>()
    };
    let name_query = escape(encode_component(&q));
    let mut queries = Vec::new();
    if let Some(isrc) = ai("isrc") {
        queries.push(encode_component(&format!("isrc:{isrc}")));
    }
    queries.push(name_query);
    for query in &queries {
        if let Some((at, found)) = st.lookups.get(query)
            && at.elapsed() < CACHE_FOR
        {
            return Some(merge(track, found, ai("release_name")));
        }
        let url =
            format!("https://{MUSICBRAINZ_HOST}/ws/2/recording/?fmt=json&limit=1&query={query}");
        let Some(json) = get_json(host, &url, &[("User-Agent", USER_AGENT)]) else {
            continue;
        };
        let Some(rec) = json.get("recordings").and_then(|r| r.as_list().first()) else {
            continue;
        };
        let releases = rec.get("releases").map(Val::as_list).unwrap_or_default();
        let rel = releases
            .iter()
            .find(|r| r.str_at(&["title"]) == Some(release.as_str()))
            .or_else(|| releases.first());
        let rel_id = rel.and_then(|r| r.str_at(&["id"]));
        let group = rel.and_then(|r| r.str_at(&["release-group", "id"]));
        let found = Track {
            image_url: cover(rel_id, group, ai("origin_url")),
            track_url: rec
                .str_at(&["id"])
                .map(|id| lb_url(&format!("/track/{id}/"))),
            album_url: rel_id
                .map(|r| lb_url(&format!("/release/{r}/")))
                .or_else(|| group.map(|g| lb_url(&format!("/release-group/{g}/")))),
            artist_url: rec
                .get("artist-credit")
                .and_then(|a| a.as_list().first())
                .and_then(|a| a.str_at(&["artist", "id"]))
                .map(|a| lb_url(&format!("/artist/{a}/"))),
            album: rel
                .and_then(|r| r.str_at(&["title"]))
                .unwrap_or("Unknown")
                .to_string(),
            ..Track::default()
        };
        st.lookups
            .insert(query.clone(), (Instant::now(), found.clone()));
        return Some(merge(track, &found, ai("release_name")));
    }
    track.image_url = ai("origin_url").and_then(youtube_thumbnail);
    Some(track)
}

/// The searched links on top of the listen; the album from the listen when it named one.
fn merge(mut track: Track, found: &Track, release_name: Option<&str>) -> Track {
    track.image_url = found.image_url.clone();
    track.track_url = found.track_url.clone();
    track.album_url = found.album_url.clone();
    track.artist_url = found.artist_url.clone();
    track.album = release_name.map_or_else(|| found.album.clone(), str::to_string);
    track
}

/// Vencord's `getActivity` for a track, given the client's other activities.
fn activity(host: &mut dyn Host, st: &mut State, track: &Track, backend: &str) -> Val {
    let (service, id) = if backend == "listenbrainz" {
        ("ListenBrainz", "listenbrainz")
    } else {
        ("Last.FM", "lastfm")
    };
    let large = if setting_bool(host, "showAlbumCover", true)
        && track
            .image_url
            .as_deref()
            .is_some_and(|u| !u.contains(LASTFM_PLACEHOLDER_IMAGE_HASH))
    {
        track.image_url.clone()
    } else if setting_str(host, "missingArt", "logo") == "placeholder" {
        Some("placeholder".into())
    } else {
        None
    };
    let mut asset = |host: &mut dyn Host, key: &str| {
        presence::asset_id(host, &mut st.assets, DISCORD_APP_ID, key).map_or(Val::Null, Val::Str)
    };
    let album = if track.album.is_empty() {
        Val::Null
    } else {
        Val::Str(track.album.clone())
    };
    let mut assets = Val::map();
    match large {
        Some(large) => {
            assets.set("large_image", asset(host, &large));
            assets.set("large_text", album);
            if setting_bool(host, "showLogo", true) {
                assets.set("small_image", asset(host, &format!("{id}-small")));
                assets.set("small_text", Val::Str(id.into()));
            }
        }
        None => {
            assets.set("large_image", asset(host, &format!("{id}-large")));
            assets.set("large_text", album);
        }
    }
    let user = setting_str(host, "username", "");
    let (labels, urls): (Vec<Val>, Vec<Val>) = if setting_bool(host, "shareUsername", false) {
        let profile = if id == "listenbrainz" {
            lb_url(&format!("/user/{user}"))
        } else {
            lastfm_url(&format!("/user/{user}"))
        };
        (
            vec![Val::Str(format!("{service} Profile"))],
            vec![Val::Str(profile)],
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let custom = setting_str(host, "statusName", "some music")
        .replace("{artist}", &track.artist)
        .replace("{album}", &track.album)
        .replace("{title}", &track.name);
    let name = match setting_str(host, "nameFormat", "status-name").as_str() {
        "artist-first" => format!("{} - {}", track.artist, track.name),
        "song-first" => format!("{} - {}", track.name, track.artist),
        "artist" => track.artist.clone(),
        "song" => track.name.clone(),
        "album" if !track.album.is_empty() => track.album.clone(),
        "service-name" => track.service_name.clone().unwrap_or(custom),
        _ => custom,
    };
    let display = match setting_str(host, "statusDisplayType", "artist").as_str() {
        "off" => 0,
        "track" => 2,
        _ => 1,
    };
    let mut a = Val::map();
    a.set("application_id", Val::Str(DISCORD_APP_ID.into()));
    a.set("name", Val::Str(name));
    a.set("details", Val::Str(track.name.clone()));
    a.set("state", Val::Str(track.artist.clone()));
    a.set("status_display_type", Val::Int(display));
    let mut meta = Val::map();
    meta.set("button_urls", Val::List(urls));
    a.set("buttons", Val::List(labels));
    a.set("metadata", meta);
    a.set(
        "type",
        Val::Int(if setting_bool(host, "useListeningStatus", false) {
            LISTENING
        } else {
            presence::PLAYING
        }),
    );
    a.set("flags", Val::Int(1));
    if setting_bool(host, "clickableLinks", true) {
        if let Some(u) = &track.track_url {
            a.set("details_url", Val::Str(u.clone()));
        }
        if let Some(u) = &track.artist_url {
            a.set("state_url", Val::Str(u.clone()));
        }
        if !track.album.is_empty()
            && let Some(u) = &track.album_url
        {
            assets.set("large_url", Val::Str(u.clone()));
        }
    }
    presence::drop_empty(&mut assets);
    a.set("assets", assets);
    if let Some(m) = a.get_mut("metadata") {
        presence::drop_empty(m);
    }
    presence::drop_empty(&mut a);
    a
}

/// Whether the client's own activities hide ours (`hideWithActivity`, `hideWithSpotify`).
fn hidden_by_others(host: &dyn Host, base: Option<&Val>) -> bool {
    let others: Vec<&Val> = base
        .and_then(|b| b.get("activities"))
        .map(Val::as_list)
        .unwrap_or_default()
        .iter()
        .filter(|a| presence::text(a, "application_id").as_deref() != Some(DISCORD_APP_ID))
        .collect();
    let any = others
        .iter()
        .any(|a| presence::activity_type(a) != Some(presence::CUSTOM_STATUS));
    let music = others
        .iter()
        .any(|a| presence::activity_type(a) == Some(LISTENING));
    (setting_bool(host, "hideWithActivity", false) && any)
        || (setting_bool(host, "hideWithSpotify", true) && music)
}

impl MusicRichPresence {
    /// One poll: ask the scrobbler, update the activity, schedule the next poll.
    fn poll(&self, host: &mut dyn Host) {
        let mut st = self.state.lock();
        let user = setting_str(host, "username", "");
        if user.is_empty() {
            st.polling = false;
            return;
        }
        let backend = setting_str(host, "scrobblerBackend", "lastfm");
        let next = if hidden_by_others(host, st.owned.base.as_ref()) {
            None
        } else {
            let track = match backend.as_str() {
                "listenbrainz" => listenbrainz(host, &mut st, &user),
                "lastfm" => lastfm(host, &user),
                other => {
                    host.log(&format!("scrobblerBackend `{other}` is not supported"));
                    None
                }
            };
            track.map(|t| activity(host, &mut st, &t, &backend))
        };
        st.polling = host.schedule(POLL_MS, TAG).is_ok();
        if next != st.owned.current {
            st.owned.current = next;
            if let Some(frame) = st.owned.resend() {
                drop(st);
                host.emit_send(presence::PRESENCE, frame);
            }
        }
    }
}

impl Plugin for MusicRichPresence {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Send { op } => presence::carries_presence(*op),
            EventKind::Dispatch { name } => *name == TIMER_EVENT,
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if presence::carries_presence(op) => {
                let mut st = self.state.lock();
                let changed = presence::presence_mut(payload).is_some_and(|p| st.owned.apply(p));
                let start = !st.polling;
                drop(st);
                if start {
                    // The first presence starts the loop (Vencord: on start, then every 16 s).
                    self.state.lock().polling = host.schedule(0, TAG).is_ok();
                }
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::Dispatch { name, payload, .. } if name == TIMER_EVENT => {
                if dispatch_data(payload).str_at(&["tag"]) == Some(TAG) {
                    self.poll(host);
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
    use crate::testutil::{Rec, j};

    const LFM: &str = "https://ws.audioscrobbler.com/2.0/?method=user.getrecenttracks&api_key=790c37d90400163a5a5fe00d6ca32ef0&user=ann&limit=1&format=json";
    const NOW_PLAYING: &str = r##"{"recenttracks":{"track":[{"name":"Song","url":"https://www.last.fm/music/Band/_/Song",
        "artist":{"#text":"Band"},"album":{"#text":"Disc"},"@attr":{"nowplaying":"true"},
        "image":[{"size":"small","#text":"s.png"},{"size":"large","#text":"https://img/l.png"}]}]}}"##;

    fn str(v: &str) -> Val {
        Val::Str(v.into())
    }

    fn host() -> Rec {
        Rec::default()
            .with("username", str("ann"))
            .answer_fetch("GET", LFM, 200, NOW_PLAYING)
            .answer(
                "POST",
                &format!("/applications/{DISCORD_APP_ID}/external-assets"),
                200,
                r#"[{"external_asset_path":"external/cover"}]"#,
            )
            .answer(
                "GET",
                &format!("/oauth2/applications/{DISCORD_APP_ID}/assets"),
                200,
                r#"[{"id":"11","name":"lastfm-small"},{"id":"12","name":"lastfm-large"}]"#,
            )
    }

    fn presence(p: &MusicRichPresence, h: &mut Rec, frame: &str) -> Val {
        let mut f = j(frame);
        let op = f.get("op").and_then(Val::as_i64).unwrap();
        p.on_event(
            h,
            Event::Send {
                op,
                payload: &mut f,
            },
        );
        f
    }

    fn tick(p: &MusicRichPresence, h: &mut Rec) {
        let mut f = Rec::timer_frame(TAG);
        let mut extra = Vec::new();
        p.on_event(
            h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut extra,
            },
        );
    }

    /// The presence the plugin sent, after it passed the plugin chain (as the proxy does).
    fn sent_through(p: &MusicRichPresence, h: &mut Rec) -> Val {
        let (op, mut frame) = h.sent.pop().expect("a presence was sent");
        assert_eq!(op, 3);
        p.on_event(
            h,
            Event::Send {
                op,
                payload: &mut frame,
            },
        );
        frame
    }

    #[test]
    fn last_fm_now_playing_becomes_vencords_activity() {
        let p = MusicRichPresence::new();
        let mut h = host();
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        assert_eq!(h.scheduled, vec![(0, TAG.to_string())]);
        tick(&p, &mut h);
        assert_eq!(h.scheduled[1], (POLL_MS, TAG.to_string()));
        let sent = sent_through(&p, &mut h);
        let list = sent.at(&["d", "activities"]).unwrap().as_list();
        assert_eq!(list.len(), 1, "one copy only");
        let a = list[0].clone();
        assert_eq!(
            a,
            j(
                r#"{"application_id":"1108588077900898414","name":"some music","details":"Song",
                "state":"Band","status_display_type":1,"type":0,"flags":1,
                "details_url":"https://www.last.fm/music/Band/_/Song",
                "state_url":"https://www.last.fm/music/Band",
                "assets":{"large_image":"mp:external/cover","large_text":"Disc",
                          "small_image":"11","small_text":"lastfm",
                          "large_url":"https://www.last.fm/music/Band/Disc"}}"#
            )
        );
        // Same track on the next poll: nothing is sent again.
        tick(&p, &mut h);
        assert!(h.sent.is_empty());
    }

    #[test]
    fn spotify_hides_it_and_a_stopped_track_removes_it() {
        let p = MusicRichPresence::new();
        let mut h = host();
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[{"type":2,"name":"Spotify"}]}}"#,
        );
        tick(&p, &mut h);
        assert!(h.sent.is_empty(), "hideWithSpotify");
        let mut h = host().answer_fetch(
            "GET",
            LFM,
            200,
            r#"{"recenttracks":{"track":[{"name":"Old"}]}}"#,
        );
        let p = MusicRichPresence::new();
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        tick(&p, &mut h);
        assert!(h.sent.is_empty(), "not playing now");
    }

    #[test]
    fn name_formats_and_listening_status() {
        let p = MusicRichPresence::new();
        let mut h = host()
            .with("nameFormat", str("artist-first"))
            .with("useListeningStatus", Val::Bool(true))
            .with("showLogo", Val::Bool(false))
            .with("clickableLinks", Val::Bool(false))
            .with("shareUsername", Val::Bool(true));
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        tick(&p, &mut h);
        let sent = sent_through(&p, &mut h);
        let a = &sent.at(&["d", "activities"]).unwrap().as_list()[0];
        assert_eq!(a.str_at(&["name"]), Some("Band - Song"));
        assert_eq!(a.get("type"), Some(&Val::Int(2)));
        assert!(a.at(&["assets", "small_image"]).is_none());
        assert!(a.get("details_url").is_none());
        assert_eq!(a.at(&["buttons"]), Some(&j(r#"["Last.FM Profile"]"#)));
        assert_eq!(
            a.at(&["metadata", "button_urls"]),
            Some(&j(r#"["https://www.last.fm/user/ann"]"#))
        );
    }

    #[test]
    fn a_self_hosted_listenbrainz_host_is_asked_instead() {
        let mut h = host()
            .with("listenbrainzHost", str(" LB.example.org "))
            .answer_fetch(
                "GET",
                "https://lb.example.org/1/user/ann/playing-now",
                200,
                r#"{"payload":{"listens":[]}}"#,
            );
        let mut st = State::default();
        assert!(listenbrainz(&mut h, &mut st, "ann").is_none());
        assert_eq!(
            h.fetch_calls[0].0,
            "GET https://lb.example.org/1/user/ann/playing-now"
        );
    }

    #[test]
    fn listenbrainz_with_ids_and_without() {
        let p = MusicRichPresence::new();
        let mut h = host()
            .with("scrobblerBackend", str("listenbrainz"))
            .answer_fetch(
                "GET",
                "https://api.listenbrainz.org/1/user/ann/playing-now",
                200,
                r#"{"payload":{"listens":[{"playing_now":true,"track_metadata":{
                    "track_name":"Song","artist_name":"Band","release_name":"Disc",
                    "additional_info":{"recording_mbid":"r1","release_mbid":"rel1","artist_mbids":["a1"],
                    "music_service_name":"spotify.com"}}}]}}"#,
            );
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        tick(&p, &mut h);
        let sent = sent_through(&p, &mut h);
        let a = &sent.at(&["d", "activities"]).unwrap().as_list()[0];
        assert_eq!(
            a.str_at(&["details_url"]),
            Some("https://listenbrainz.org/track/r1")
        );
        assert_eq!(
            a.str_at(&["state_url"]),
            Some("https://listenbrainz.org/artist/a1")
        );
        let posted = &h
            .rest_calls
            .iter()
            .find(|(r, _)| r.starts_with("POST"))
            .unwrap()
            .1;
        assert_eq!(
            posted.as_ref().unwrap().at(&["urls"]),
            Some(&j(
                r#"["https://coverartarchive.org/release/rel1/front-500"]"#
            ))
        );

        // Without ids: one MusicBrainz search with a User-Agent, then cached.
        let mut st = State::default();
        let q = escape_check();
        let mut h = Rec::default()
            .answer_fetch(
                "GET",
                "https://api.listenbrainz.org/1/user/bob/playing-now",
                200,
                r#"{"payload":{"listens":[{"playing_now":true,"track_metadata":{
                    "track_name":"Hi-Fi","artist_name":"X","additional_info":{}}}]}}"#,
            )
            .answer_fetch(
                "GET",
                &format!("https://musicbrainz.org/ws/2/recording/?fmt=json&limit=1&query={q}"),
                200,
                r#"{"recordings":[{"id":"rec","artist-credit":[{"artist":{"id":"art"}}],
                    "releases":[{"id":"rel","title":"Album","release-group":{"id":"grp"}}]}]}"#,
            );
        let t = listenbrainz(&mut h, &mut st, "bob").unwrap();
        assert_eq!(t.album, "Album");
        assert_eq!(
            t.track_url.as_deref(),
            Some("https://listenbrainz.org/track/rec/")
        );
        assert_eq!(
            h.fetch_calls[1].1,
            vec![("User-Agent".to_string(), USER_AGENT.to_string())]
        );
        listenbrainz(&mut h, &mut st, "bob").unwrap();
        assert_eq!(h.fetch_calls.len(), 3, "the search is cached");
    }

    /// The query Vencord builds for artist `X`, track `Hi-Fi`, no release.
    fn escape_check() -> String {
        encode_component(r#"artist:"X" AND recording:"Hi-Fi""#)
            .replace('-', "\\-")
            .replace('(', "\\(")
    }

    #[test]
    fn helpers_match_javascript() {
        assert_eq!(encode_component("a b/ü!"), "a%20b%2F%C3%BC!");
        assert_eq!(
            youtube_thumbnail("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1"),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg".into())
        );
        assert_eq!(
            youtube_thumbnail("https://youtu.be/abc_DEF-1"),
            Some("https://i.ytimg.com/vi/abc_DEF-1/maxresdefault.jpg".into())
        );
        assert_eq!(youtube_thumbnail("https://example.org/watch?v=x"), None);
    }

    #[test]
    fn no_username_no_polling() {
        let p = MusicRichPresence::new();
        let mut h = Rec::default();
        presence(
            &p,
            &mut h,
            r#"{"op":3,"d":{"status":"online","activities":[]}}"#,
        );
        tick(&p, &mut h);
        assert!(h.fetch_calls.is_empty());
        assert_eq!(h.scheduled.len(), 1, "stops until the next presence");
        assert_eq!(p.manifest().fetch.len(), 3);
    }
}
