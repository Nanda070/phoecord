//! CustomRPC: a fully custom Rich Presence on your profile (Vencord: CustomRPC).
//!
//! Ported from Vencord `src/plugins/customRPC/index.tsx` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors captain, AutumnVN, nin0dev, c0nnorgg). Vencord builds one
//! activity from its settings (`createActivity`) and hands it to the client as a local activity.
//! Here the same activity is added to every presence the client sends (identify and `op 3`), and
//! `/phoecord rpc` sends your presence again at once with the current settings (the client only
//! sends one when something changes; a reconnect, Ctrl+R, does it too). Image ids are asked
//! once and kept, a missing one too; `/phoecord rpc` asks again.
//!
//! Settings (`[plugins.custom_rpc]`, Vencord's names): `enablePresence` (default true), `appID`,
//! `appName` (required, else nothing is shown), `details`, `detailsURL`, `state`, `stateURL`,
//! `type` (0 playing, 1 streaming, 2 listening, 3 watching, 5 competing), `streamLink`,
//! `timestampMode` (0 none, 1 since the activity started, 2 since local midnight, 3 custom; or
//! `"none"`, `"now"`, `"time"`, `"custom"`), `startTime`, `endTime` (ms), `imageBig`,
//! `imageBigURL`, `imageBigTooltip`, `imageSmall`, `imageSmallURL`, `imageSmallTooltip` (an image
//! is an asset name of the app or a link), `buttonOneText`, `buttonOneURL`, `buttonTwoText`,
//! `buttonTwoURL`, `partySize`, `partyMaxSize`.
//!
//! Not ported: the settings page with its preview, and the patch that shows your own buttons.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer};
use crate::presence::{self, Owned, STREAMING};
use crate::state::Shared;
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use std::collections::HashMap;

const PLUGIN_ID: &str = "custom_rpc";
/// Activity flag `INSTANCE`, as Vencord sets.
const INSTANCE: i64 = 1;

#[derive(Default)]
struct State {
    owned: Owned,
    /// Asset ids by app and key.
    assets: HashMap<String, String>,
    /// When the activity with these settings started (timestamp mode "now").
    started: Option<(String, i64)>,
}

pub struct CustomRpc {
    manifest: Manifest,
    kept: Kept,
    state: Shared<State>,
}

impl CustomRpc {
    pub fn new() -> CustomRpc {
        CustomRpc {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "CustomRPC".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Add a fully custom Rich Presence to your profile".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("CustomRPC".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            kept: Kept::default(),
            state: Shared::default(),
        }
    }

    /// `/phoecord rpc`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            "rpc",
            "Show your custom Rich Presence now with the current settings",
            &self.kept,
        )
        .boxed()
    }
}

impl Default for CustomRpc {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A text setting, `None` when missing or empty.
fn s(host: &dyn Host, key: &str) -> Option<String> {
    match host.setting(key)? {
        Val::Int(n) => Some(n.to_string()),
        v => v.as_str().map(str::to_string),
    }
    .filter(|v| !v.trim().is_empty())
}

fn n(host: &dyn Host, key: &str) -> Option<i64> {
    match host.setting(key)? {
        Val::Float(f) => Some(f as i64),
        Val::Str(v) => v.trim().parse().ok(),
        v => v.as_i64(),
    }
}

fn str_val(v: &str) -> Val {
    Val::Str(v.to_string())
}

/// `timestampMode` as Vencord's number.
fn timestamp_mode(host: &dyn Host) -> i64 {
    match host.setting("timestampMode") {
        Some(Val::Str(m)) => match m.to_ascii_lowercase().as_str() {
            "now" => 1,
            "time" => 2,
            "custom" => 3,
            other => other.parse().unwrap_or(0),
        },
        Some(v) => v.as_i64().unwrap_or(0),
        None => 0,
    }
}

/// Milliseconds since local midnight (timestamp mode "time").
fn since_midnight_ms() -> i64 {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let (h, m, s) = now.time().as_hms();
    (i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s)) * 1000
}

/// Vencord's `createActivity`, or `None` without `appName` or with `enablePresence = false`.
fn build(host: &mut dyn Host, st: &mut State) -> Option<Val> {
    if host.setting("enablePresence").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let name = s(host, "appName")?;
    let app = s(host, "appID").unwrap_or_else(|| "0".into());
    let kind = n(host, "type").unwrap_or(0);
    let mut a = Val::map();
    a.set("application_id", str_val(&app));
    a.set("name", str_val(&name));
    a.set("state", s(host, "state").map_or(Val::Null, |v| str_val(&v)));
    a.set(
        "details",
        s(host, "details").map_or(Val::Null, |v| str_val(&v)),
    );
    a.set("type", Val::Int(kind));
    a.set("flags", Val::Int(INSTANCE));
    if kind == STREAMING {
        a.set(
            "url",
            s(host, "streamLink").map_or(Val::Null, |v| str_val(&v)),
        );
    }
    let mut ts = Val::map();
    match timestamp_mode(host) {
        1 => {
            // Same settings, same start: the timer must not restart on every presence.
            let key = format!("{name}\u{0}{app}\u{0}{kind}");
            let start = match &st.started {
                Some((k, t)) if *k == key => *t,
                _ => {
                    let t = now_ms();
                    st.started = Some((key, t));
                    t
                }
            };
            ts.set("start", Val::Int(start));
        }
        2 => ts.set("start", Val::Int(now_ms() - since_midnight_ms())),
        3 => {
            if let Some(t) = n(host, "startTime").filter(|t| *t > 0) {
                ts.set("start", Val::Int(t));
            }
            if let Some(t) = n(host, "endTime").filter(|t| *t > 0) {
                ts.set("end", Val::Int(t));
            }
        }
        _ => {}
    }
    a.set("timestamps", ts);
    if let Some(u) = s(host, "detailsURL") {
        a.set("details_url", str_val(&u));
    }
    if let Some(u) = s(host, "stateURL") {
        a.set("state_url", str_val(&u));
    }
    if let Some(one) = s(host, "buttonOneText") {
        let labels: Vec<Val> = [Some(one), s(host, "buttonTwoText")]
            .into_iter()
            .flatten()
            .map(|t| str_val(&t))
            .collect();
        let urls: Vec<Val> = [s(host, "buttonOneURL"), s(host, "buttonTwoURL")]
            .into_iter()
            .flatten()
            .map(|t| str_val(&t))
            .collect();
        a.set("buttons", Val::List(labels));
        let mut meta = Val::map();
        meta.set("button_urls", Val::List(urls));
        a.set("metadata", meta);
    }
    let mut assets = Val::map();
    for (size, img, text, url) in [
        ("large", "imageBig", "imageBigTooltip", "imageBigURL"),
        ("small", "imageSmall", "imageSmallTooltip", "imageSmallURL"),
    ] {
        let Some(key) = s(host, img) else { continue };
        if let Some(id) = presence::asset_id(host, &mut st.assets, &app, &key) {
            assets.set(&format!("{size}_image"), str_val(&id));
        }
        if let Some(t) = s(host, text) {
            assets.set(&format!("{size}_text"), str_val(&t));
        }
        if let Some(u) = s(host, url) {
            assets.set(&format!("{size}_url"), str_val(&u));
        }
    }
    a.set("assets", assets);
    if let (Some(size), Some(max)) = (n(host, "partySize"), n(host, "partyMaxSize"))
        && size > 0
        && max > 0
    {
        let mut party = Val::map();
        party.set("size", Val::List(vec![Val::Int(size), Val::Int(max)]));
        a.set("party", party);
    }
    presence::drop_empty(&mut a);
    Some(a)
}

impl CustomRpc {
    fn on_presence(&self, host: &mut dyn Host, p: &mut Val) -> bool {
        let mut st = self.state.lock();
        st.owned.current = build(host, &mut st);
        st.owned.apply(p)
    }

    fn refresh(&self, host: &mut dyn Host) -> String {
        let mut st = self.state.lock();
        // Ask Discord for the images again: they may have been added to the app since.
        st.assets.clear();
        st.owned.current = build(host, &mut st);
        let shown = st.owned.current.is_some();
        let Some(frame) = st.owned.resend() else {
            return host.t("custom_rpc-no-presence", &[]);
        };
        drop(st);
        host.emit_send(presence::PRESENCE, frame);
        host.t(
            if shown {
                "custom_rpc-shown"
            } else {
                "custom_rpc-cleared"
            },
            &[],
        )
    }
}

impl Plugin for CustomRpc {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Send { op } => presence::carries_presence(*op),
            EventKind::RestRequest { method, route } => is_interaction(method, route),
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send { op, payload } if presence::carries_presence(op) => {
                let changed =
                    presence::presence_mut(payload).is_some_and(|p| self.on_presence(host, p));
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::RestRequest {
                method,
                route,
                body: Some(body),
            } if is_interaction(method, route) => {
                if let Some(inv) = invocation(body)
                    && inv.sub == "rpc"
                {
                    let answer = self.refresh(host);
                    *self.kept.lock() = Some(answer);
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
    use crate::commands::Args;
    use crate::testutil::{Rec, j};

    fn str(v: &str) -> Val {
        Val::Str(v.into())
    }

    fn send(p: &CustomRpc, h: &mut Rec, frame: &str) -> (Verdict, Val) {
        let mut v = j(frame);
        let op = v.get("op").and_then(Val::as_i64).unwrap();
        let out = p.on_event(
            h,
            Event::Send {
                op,
                payload: &mut v,
            },
        );
        (out, v)
    }

    const PRESENCE: &str =
        r#"{"op":3,"d":{"status":"online","activities":[{"type":4,"name":"Custom Status"}]}}"#;

    #[test]
    fn builds_vencords_activity_and_adds_it_once() {
        let p = CustomRpc::new();
        let mut h = Rec::default()
            .with("appID", str("123"))
            .with("appName", str("My Game"))
            .with("details", str("In a match"))
            .with("state", str(""))
            .with("timestampMode", Val::Int(3))
            .with("startTime", Val::Int(1000))
            .with("imageBig", str("https://x/big.png"))
            .with("imageBigTooltip", str("Big"))
            .with("buttonOneText", str("Site"))
            .with("buttonOneURL", str("https://example.org"))
            .with("partySize", Val::Int(1))
            .with("partyMaxSize", Val::Int(4))
            .answer(
                "POST",
                "/applications/123/external-assets",
                200,
                r#"[{"external_asset_path":"external/big"}]"#,
            );
        let (v, f) = send(&p, &mut h, PRESENCE);
        assert_eq!(v, Verdict::Changed);
        let list = f.at(&["d", "activities"]).unwrap().as_list();
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[1],
            j(
                r#"{"application_id":"123","name":"My Game","details":"In a match","type":0,"flags":1,
                "timestamps":{"start":1000},"buttons":["Site"],
                "metadata":{"button_urls":["https://example.org"]},
                "assets":{"large_image":"mp:external/big","large_text":"Big"},
                "party":{"size":[1,4]}}"#
            )
        );
        // Resent by /phoecord rpc and passing again: still one copy.
        let mut body = j(
            r#"{"type":2,"channel_id":"9","data":{"name":"phoecord","options":[{"type":1,"name":"rpc"}]}}"#,
        );
        p.on_event(
            &mut h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        assert_eq!(
            p.command().run(&mut h, &Args { options: vec![] }),
            "Custom Rich Presence updated."
        );
        let (_, mut again) = h.sent.pop().unwrap();
        p.on_event(
            &mut h,
            Event::Send {
                op: 3,
                payload: &mut again,
            },
        );
        assert_eq!(again.at(&["d", "activities"]).unwrap().as_list().len(), 2);
        assert_eq!(
            h.rest_calls.len(),
            2,
            "asked once, again by /phoecord rpc, then cached"
        );
    }

    #[test]
    fn streaming_now_mode_keeps_its_start_and_identify_gets_it_too() {
        let p = CustomRpc::new();
        let mut h = Rec::default()
            .with("appName", str("Live"))
            .with("type", Val::Int(1))
            .with("streamLink", str("https://twitch.tv/x"))
            .with("timestampMode", str("now"));
        let (_, a) = send(
            &p,
            &mut h,
            r#"{"op":2,"d":{"token":"t","presence":{"status":"online","activities":[]}}}"#,
        );
        let act = &a.at(&["d", "presence", "activities"]).unwrap().as_list()[0];
        assert_eq!(act.str_at(&["url"]), Some("https://twitch.tv/x"));
        assert_eq!(act.str_at(&["application_id"]), Some("0"));
        let start = act.at(&["timestamps", "start"]).cloned();
        let (_, b) = send(&p, &mut h, PRESENCE);
        assert_eq!(
            b.at(&["d", "activities"]).unwrap().as_list()[1]
                .at(&["timestamps", "start"])
                .cloned(),
            start
        );
    }

    #[test]
    fn nothing_without_a_name_or_when_disabled() {
        let p = CustomRpc::new();
        let mut h = Rec::default();
        assert_eq!(send(&p, &mut h, PRESENCE).0, Verdict::Pass);
        let mut h = Rec::default()
            .with("appName", str("X"))
            .with("enablePresence", Val::Bool(false));
        assert_eq!(send(&p, &mut h, PRESENCE).0, Verdict::Pass);
        let fresh = CustomRpc::new();
        assert!(fresh.refresh(&mut h).contains("No presence"));
    }
}
