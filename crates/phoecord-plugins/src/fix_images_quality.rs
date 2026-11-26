//! FixImagesQuality: load attached images at their original quality (Vencord: FixImagesQuality).
//!
//! Ported from Vencord `src/plugins/fixImagesQuality/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2024 Vendicated and contributors; authors Nuckyz, Ven). Vencord patches the function that builds
//! an image's address in the client. The network layer only sees the address the server sends, so
//! this covers Vencord's `originalImagesInChat` mode: an image attachment whose `proxy_url` points
//! at `media.discordapp.net/attachments/…` (which resizes and re-encodes) gets the same path and
//! signature on `cdn.discordapp.com`, which serves the file as it was uploaded.
//!
//! Setting, with Vencord's name and default: `originalImagesInChat` (default `false`). With
//! Vencord's caveats: animated images always animate and large ones may lag.
//!
//! Not ported (Phase 2, needs a UI hook): the default mode, where chat shows a full resolution
//! but optimised image and only the image viewer loads the original, and images inside embeds.
//! Unverified against a live client.

use crate::state::dispatch_data_mut;
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PROXY: &str = "https://media.discordapp.net/attachments/";
const CDN: &str = "https://cdn.discordapp.com/attachments/";

pub struct FixImagesQuality {
    manifest: Manifest,
}

impl FixImagesQuality {
    pub fn new() -> FixImagesQuality {
        FixImagesQuality {
            manifest: {
                let mut m = Manifest::new(
                    "fix_images_quality",
                    "FixImagesQuality",
                    env!("CARGO_PKG_VERSION"),
                    "Load attached images at their original quality",
                );
                m.authors = vec!["Nuckyz".into(), "Ven".into()];
                m.vencord = Some("FixImagesQuality".into());
                m
            },
        }
    }
}

impl Default for FixImagesQuality {
    fn default() -> Self {
        Self::new()
    }
}

fn is_history(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "GET" && matches!(seg.as_slice(), ["channels", _, "messages"])
}

fn is_message_event(name: &str) -> bool {
    matches!(name, "MESSAGE_CREATE" | "MESSAGE_UPDATE")
}

fn bytes(v: &Val) -> Option<&[u8]> {
    match v {
        Val::Bin(b) | Val::Charlist(b) => Some(b),
        v => v.as_str().map(str::as_bytes),
    }
}

/// Point one image attachment at the CDN; `false` if it is not one or needs no change.
fn fix(attachment: &mut Val) -> bool {
    let is_image = attachment
        .get("content_type")
        .and_then(bytes)
        .is_some_and(|t| t.starts_with(b"image/"));
    if !is_image {
        return false;
    }
    let Some(slot) = attachment.get_mut("proxy_url") else {
        return false;
    };
    let Some(url) = bytes(slot).and_then(|b| std::str::from_utf8(b).ok()) else {
        return false;
    };
    let Some(path) = url.strip_prefix(PROXY) else {
        return false;
    };
    let new = format!("{CDN}{path}");
    *slot = match &*slot {
        Val::Bin(_) => Val::Bin(new.into_bytes()),
        v => v.str_like(&new),
    };
    true
}

/// The attachments of a message, and of the messages it forwards.
fn fix_message(message: &mut Val) -> bool {
    let mut changed = false;
    if let Some(list) = message.get_mut("attachments").and_then(Val::as_list_mut) {
        for a in list {
            changed |= fix(a);
        }
    }
    if let Some(list) = message
        .get_mut("message_snapshots")
        .and_then(Val::as_list_mut)
    {
        for snapshot in list {
            if let Some(m) = snapshot.get_mut("message") {
                changed |= fix_message(m);
            }
        }
    }
    changed
}

fn on(host: &dyn Host) -> bool {
    host.setting("originalImagesInChat")
        .and_then(|v| v.as_bool())
        == Some(true)
}

impl Plugin for FixImagesQuality {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Dispatch { name } => is_message_event(name),
            EventKind::RestResponse { method, route } => is_history(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        let changed = match event {
            Event::Dispatch { name, payload, .. } if is_message_event(name) && on(host) => {
                fix_message(dispatch_data_mut(payload))
            }
            Event::RestResponse {
                method,
                route,
                status: 200,
                body,
            } if is_history(method, route) && on(host) => {
                let mut any = false;
                for message in body.as_list_mut().into_iter().flatten() {
                    any |= fix_message(message);
                }
                any
            }
            _ => false,
        };
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

    const HISTORY: &str = r#"[{"attachments":[
        {"content_type":"image/png","proxy_url":"https://media.discordapp.net/attachments/1/2/a.png?ex=1&hm=2"},
        {"content_type":"video/mp4","proxy_url":"https://media.discordapp.net/attachments/1/2/v.mp4"},
        {"content_type":"image/png","proxy_url":"https://example.com/attachments/a.png"},
        {"proxy_url":"https://media.discordapp.net/attachments/1/2/unknown"}],
        "message_snapshots":[{"message":{"attachments":[
        {"content_type":"image/gif","proxy_url":"https://media.discordapp.net/attachments/3/4/b.gif"}]}}]}]"#;

    fn urls(body: &Val) -> Vec<String> {
        let m = &body.as_list()[0];
        let mut all: Vec<&Val> = m.get("attachments").unwrap().as_list().iter().collect();
        let snap = &m.get("message_snapshots").unwrap().as_list()[0];
        all.extend(snap.at(&["message", "attachments"]).unwrap().as_list());
        all.iter()
            .map(|a| a.str_at(&["proxy_url"]).unwrap().to_string())
            .collect()
    }

    fn run(host: &mut Cfg, body: &mut Val) -> Outcome {
        let chain = Chain::new(vec![Box::new(FixImagesQuality::new())]);
        chain.rest_response(host, "GET", "/channels/1/messages", 200, body)
    }

    #[test]
    fn matching_rules() {
        let p = FixImagesQuality::new();
        assert!(p.wants(&EventKind::Dispatch {
            name: "MESSAGE_CREATE"
        }));
        assert!(!p.wants(&EventKind::Dispatch { name: "READY" }));
        assert!(!is_history("GET", "/channels/1/pins"));
    }

    #[test]
    fn off_by_default() {
        let mut body = j(HISTORY);
        assert_eq!(run(&mut Cfg::default(), &mut body), Outcome::Untouched);
        assert_eq!(body, j(HISTORY));
    }

    #[test]
    fn only_image_attachments_move_to_the_cdn() {
        let mut h = Cfg::default().with("originalImagesInChat", Val::Bool(true));
        let mut body = j(HISTORY);
        assert_eq!(run(&mut h, &mut body), Outcome::Changed);
        let got = urls(&body);
        assert_eq!(
            got[0],
            "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1&hm=2"
        );
        assert!(got[1].starts_with(PROXY) && got[3].starts_with(PROXY));
        assert!(got[2].starts_with("https://example.com/"));
        assert_eq!(got[4], "https://cdn.discordapp.com/attachments/3/4/b.gif");
        assert_eq!(run(&mut h, &mut body), Outcome::Untouched);
    }

    #[test]
    fn etf_attachments_stay_binary() {
        let k = |s: &str| Val::Atom(s.into());
        let mut a = Val::Map(vec![
            (k("content_type"), Val::Bin(b"image/png".to_vec())),
            (k("proxy_url"), Val::Bin(format!("{PROXY}x").into_bytes())),
        ]);
        assert!(fix(&mut a));
        assert_eq!(
            a.get("proxy_url"),
            Some(&Val::Bin(format!("{CDN}x").into_bytes()))
        );
    }
}
