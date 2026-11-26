//! AnonymiseFileNames: uploaded files get a random name, the extension stays (Vencord:
//! AnonymiseFileNames).
//!
//! Ported from Vencord `src/plugins/anonymiseFileNames/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! Vendicated and contributors; author fawn): the `anonymise` function (extension rule including
//! `.tar.*`, the three methods, the character set) and the settings. Vencord patches the client's
//! `uploadFiles`; here the same rename is done on the two requests that carry the name:
//!
//! 1. `POST /channels/{id}/attachments` (ask for upload URLs): `files[].filename` is renamed, so
//!    the cloud upload path (`upload_filename` in the answer) is built from the new name. The
//!    new name is remembered per `(channel, files[].id)`.
//! 2. `POST /channels/{id}/messages` (send): every `attachments[]` entry that has an
//!    `uploaded_filename` gets `filename` set to the remembered name, or, when none was
//!    remembered (the plugin was switched on in between), to the last path part of
//!    `uploaded_filename`. So both places agree.
//!
//! Settings (`[plugins.anonymise_file_names]`), Vencord's names in snake_case:
//! - `anonymise_by_default` (default `true`): `false` turns the plugin into a no-op. Vencord's
//!   per-upload toggle button is UI (Phase 2); until then this is all-or-nothing.
//! - `method`: `"random"` (default), `"consistent"` or `"timestamp"` (Vencord stores 0, 1, 2; those
//!   work too).
//! - `randomised_length` (default 7, at least 1): characters of a random name.
//! - `consistent` (default `"image"`): the name for `method = "consistent"`.
//!
//! Not on the wire: files sent as a multipart message body (the proxy never rewrites multipart
//! bodies) keep their names; the web client uses the two-step cloud upload above for normal
//! attachments. Unverified against a live client: that Discord builds `upload_filename` from the
//! requested name and that the client echoes the id it sent.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// How many renames are remembered between the two requests.
const REMEMBERED: usize = 500;
/// Vencord's character set for random names.
const CHARS: &[u8] = b"0123456789bdfhjkmnpqrstvwxz";

pub struct AnonymiseFileNames {
    manifest: Manifest,
    /// `(channel, upload id)` -> new file name.
    names: Mutex<HashMap<(String, String), String>>,
    random: RandomState,
}

impl AnonymiseFileNames {
    pub fn new() -> AnonymiseFileNames {
        AnonymiseFileNames {
            manifest: {
                let mut m = Manifest::new(
                    "anonymise_file_names",
                    "AnonymiseFileNames",
                    env!("CARGO_PKG_VERSION"),
                    "Anonymise uploaded file names",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("AnonymiseFileNames".into());
                m
            },
            names: Mutex::new(HashMap::new()),
            random: RandomState::new(),
        }
    }
}

impl Default for AnonymiseFileNames {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/attachments`: the channel id.
fn upload_route(method: &str, route: &str) -> Option<String> {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    match (method, seg.as_slice()) {
        ("POST", ["channels", id, "attachments"]) => Some((*id).to_string()),
        _ => None,
    }
}

/// `POST /channels/{id}/messages`: the channel id.
fn send_route(method: &str, route: &str) -> Option<String> {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    match (method, seg.as_slice()) {
        ("POST", ["channels", id, "messages"]) => Some((*id).to_string()),
        _ => None,
    }
}

/// Vencord: the extension is `.tar.xxx` if the name ends so, else from the last dot (dot
/// included), else none.
fn extension(name: &str) -> &str {
    let last = name.rfind('.');
    if let Some(dot) = last
        && let Some(stem_end) = name[..dot].strip_suffix(".tar").map(str::len)
        && name.len() > dot + 1
        && name[dot + 1..]
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
    {
        return &name[stem_end..];
    }
    last.map_or("", |dot| &name[dot..])
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Method {
    Random,
    Consistent,
    Timestamp,
}

struct Settings {
    method: Method,
    length: usize,
    consistent: String,
}

impl Settings {
    /// `None` when `anonymise_by_default` is off.
    fn read(host: &dyn Host) -> Option<Settings> {
        if host
            .setting("anonymise_by_default")
            .and_then(|v| v.as_bool())
            == Some(false)
        {
            return None;
        }
        let method = match host.setting("method") {
            Some(Val::Str(s)) if s.eq_ignore_ascii_case("consistent") => Method::Consistent,
            Some(Val::Str(s)) if s.eq_ignore_ascii_case("timestamp") => Method::Timestamp,
            Some(v) => match v.as_i64() {
                Some(1) => Method::Consistent,
                Some(2) => Method::Timestamp,
                _ => Method::Random,
            },
            None => Method::Random,
        };
        let length = host
            .setting("randomised_length")
            .and_then(|v| v.as_i64())
            .map_or(7, |n| n.clamp(1, 200) as usize);
        let consistent = host
            .setting("consistent")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "image".into());
        Some(Settings {
            method,
            length,
            consistent,
        })
    }
}

impl AnonymiseFileNames {
    fn random_chars(&self, n: usize) -> String {
        let mut out = String::with_capacity(n);
        let mut counter = 0u64;
        while out.len() < n {
            let mut h = self.random.build_hasher();
            h.write_u64(counter);
            if let Ok(t) = SystemTime::now().duration_since(UNIX_EPOCH) {
                h.write_u128(t.as_nanos());
            }
            counter += 1;
            let mut x = h.finish();
            // 64 bits give several characters; the set is 27 long, so use 5 bits and skip the rest.
            for _ in 0..12 {
                let i = (x & 31) as usize;
                x >>= 5;
                if i < CHARS.len() && out.len() < n {
                    out.push(CHARS[i] as char);
                }
            }
        }
        out
    }

    fn new_name(&self, original: &str, s: &Settings) -> String {
        let ext = extension(original);
        match s.method {
            Method::Random => format!("{}{ext}", self.random_chars(s.length)),
            Method::Consistent => format!("{}{ext}", s.consistent),
            Method::Timestamp => {
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                format!("{ms}{ext}")
            }
        }
    }
}

/// An upload id as text (a string in JSON, a number in some clients).
fn id_text(v: &Val) -> Option<String> {
    match v {
        Val::Str(s) => Some(s.clone()),
        Val::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Plugin for AnonymiseFileNames {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route }
            if upload_route(method, route).is_some() || send_route(method, route).is_some())
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
        if let Some(channel) = upload_route(method, route) {
            let Some(settings) = Settings::read(host) else {
                return Verdict::Pass;
            };
            let Some(files) = body.get_mut("files").and_then(Val::as_list_mut) else {
                return Verdict::Pass;
            };
            let mut names = self.names.lock().unwrap();
            let mut changed = false;
            for file in files {
                let Some(original) = file
                    .get("filename")
                    .and_then(Val::as_str)
                    .map(str::to_string)
                else {
                    continue;
                };
                let new = self.new_name(&original, &settings);
                if let Some(id) = file.get("id").and_then(id_text) {
                    if names.len() >= REMEMBERED {
                        names.clear();
                    }
                    names.insert((channel.clone(), id), new.clone());
                }
                if let Some(slot) = file.get_mut("filename") {
                    *slot = slot.str_like(&new);
                    changed = true;
                }
            }
            return if changed {
                Verdict::Changed
            } else {
                Verdict::Pass
            };
        }
        let Some(channel) = send_route(method, route) else {
            return Verdict::Pass;
        };
        if Settings::read(host).is_none() {
            return Verdict::Pass;
        }
        let Some(attachments) = body.get_mut("attachments").and_then(Val::as_list_mut) else {
            return Verdict::Pass;
        };
        let mut names = self.names.lock().unwrap();
        let mut changed = false;
        for a in attachments {
            let Some(uploaded) = a.get("uploaded_filename").and_then(Val::as_str) else {
                continue;
            };
            let from_path = uploaded.rsplit('/').next().unwrap_or(uploaded).to_string();
            let remembered = a
                .get("id")
                .and_then(id_text)
                .and_then(|id| names.remove(&(channel.clone(), id)));
            let new = remembered.unwrap_or(from_path);
            if new.is_empty() {
                continue;
            }
            if let Some(slot) = a.get_mut("filename")
                && slot.as_str() != Some(new.as_str())
            {
                *slot = slot.str_like(&new);
                changed = true;
            }
        }
        if changed {
            Verdict::Changed
        } else {
            Verdict::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Cfg, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    #[test]
    fn matching_rules() {
        assert_eq!(
            upload_route("POST", "/channels/5/attachments").as_deref(),
            Some("5")
        );
        assert_eq!(upload_route("GET", "/channels/5/attachments"), None);
        assert_eq!(
            send_route("POST", "/channels/5/messages").as_deref(),
            Some("5")
        );
        assert_eq!(send_route("PATCH", "/channels/5/messages/1"), None);
        let p = AnonymiseFileNames::new();
        assert!(p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/channels/5/attachments"
        }));
        assert!(!p.wants(&EventKind::RestRequest {
            method: "POST",
            route: "/channels/5/typing"
        }));
    }

    #[test]
    fn extension_follows_vencord() {
        assert_eq!(extension("cat.png"), ".png");
        assert_eq!(extension("backup.tar.gz"), ".tar.gz");
        assert_eq!(extension("a.b.tar.xz"), ".tar.xz");
        assert_eq!(extension("noext"), "");
        assert_eq!(extension("archive.tar"), ".tar");
        assert_eq!(extension(".bashrc"), ".bashrc");
    }

    #[test]
    fn names_by_method() {
        let p = AnonymiseFileNames::new();
        let s = |method, length| Settings {
            method,
            length,
            consistent: "image".into(),
        };
        let r = p.new_name("holiday.jpg", &s(Method::Random, 9));
        assert!(r.ends_with(".jpg") && r.len() == 9 + 4, "{r}");
        assert!(r[..9].bytes().all(|c| CHARS.contains(&c)), "{r}");
        assert_ne!(r, p.new_name("holiday.jpg", &s(Method::Random, 9)));
        assert_eq!(
            p.new_name("x.tar.gz", &s(Method::Consistent, 7)),
            "image.tar.gz"
        );
        let t = p.new_name("a.png", &s(Method::Timestamp, 7));
        assert!(
            t.ends_with(".png") && t[..t.len() - 4].parse::<u128>().is_ok(),
            "{t}"
        );
    }

    #[test]
    fn settings_accept_vencords_numbers_and_the_off_switch() {
        let h = Cfg::default().with("method", Val::Int(1));
        assert_eq!(Settings::read(&h).unwrap().method, Method::Consistent);
        let h = Cfg::default().with("method", Val::Str("timestamp".into()));
        assert_eq!(Settings::read(&h).unwrap().method, Method::Timestamp);
        let h = Cfg::default().with("randomised_length", Val::Int(0));
        assert_eq!(Settings::read(&h).unwrap().length, 1);
        let h = Cfg::default().with("anonymise_by_default", Val::Bool(false));
        assert!(Settings::read(&h).is_none());
    }

    #[test]
    fn chain_renames_the_request_and_the_message_consistently() {
        let chain = Chain::new(vec![Box::new(AnonymiseFileNames::new())]);
        let mut h = Cfg::default()
            .with("method", Val::Str("consistent".into()))
            .with("consistent", Val::Str("pic".into()));
        let mut req = j(
            r#"{"files":[{"filename":"me at home.PNG","file_size":10,"id":"0"},{"filename":"notes.tar.gz","file_size":5,"id":"1"}]}"#,
        );
        let out = chain.rest_request(&mut h, "POST", "/channels/9/attachments", Some(&mut req));
        assert_eq!(out, Outcome::Changed);
        assert_eq!(
            req.at(&["files"]).unwrap().as_list()[0].str_at(&["filename"]),
            Some("pic.PNG")
        );
        assert_eq!(
            req.at(&["files"]).unwrap().as_list()[1].str_at(&["filename"]),
            Some("pic.tar.gz")
        );

        let mut msg = j(
            r#"{"content":"x","attachments":[{"id":"0","filename":"me at home.PNG","uploaded_filename":"abc-123/pic.PNG"},{"id":"1","filename":"notes.tar.gz","uploaded_filename":"def/pic.tar.gz"}]}"#,
        );
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut msg));
        assert_eq!(out, Outcome::Changed);
        let a = msg.at(&["attachments"]).unwrap().as_list();
        assert_eq!(a[0].str_at(&["filename"]), Some("pic.PNG"));
        assert_eq!(a[1].str_at(&["filename"]), Some("pic.tar.gz"));
        assert_eq!(a[0].str_at(&["uploaded_filename"]), Some("abc-123/pic.PNG"));

        // Without a remembered rename the last path part of `uploaded_filename` is used.
        let mut late = j(
            r#"{"attachments":[{"id":"7","filename":"orig.txt","uploaded_filename":"u/zzz.txt"}]}"#,
        );
        chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut late));
        assert_eq!(
            late.at(&["attachments"]).unwrap().as_list()[0].str_at(&["filename"]),
            Some("zzz.txt")
        );
        // Messages without uploads and the off switch are left alone.
        let mut plain = j(r#"{"content":"hi"}"#);
        let out = chain.rest_request(&mut h, "POST", "/channels/9/messages", Some(&mut plain));
        assert_eq!(out, Outcome::Untouched);
        let mut off = Cfg::default().with("anonymise_by_default", Val::Bool(false));
        let mut req = j(r#"{"files":[{"filename":"a.png","id":"0"}]}"#);
        let out = chain.rest_request(&mut off, "POST", "/channels/9/attachments", Some(&mut req));
        assert_eq!(out, Outcome::Untouched);
    }
}
