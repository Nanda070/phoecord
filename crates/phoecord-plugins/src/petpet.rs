//! petpet: a headpat GIF of an avatar or picture (Vencord: petpet).
//!
//! Ported from Vencord `src/plugins/petpet/index.ts` (GPL-3.0-or-later, Copyright (c) 2022
//! Vendicated and contributors; authors Ven, u32). The ten hand frames are Vencord's,
//! `https://raw.githubusercontent.com/VenPlugs/petpet/main/frames/pet{0..9}.gif`, fetched once and
//! kept in memory. For frame `i` (`j = i` for the first half, `10 - i` after) the picture is drawn
//! at width `0.8 + 0.02 j`, height `0.8 - 0.05 j`, x `(1 - width) / 2 + 0.1`, y `1 - height -
//! 0.08` (all times the resolution), then the hand on top; Vencord's defaults and limits stay:
//! `delay` 20 ms (at least 20, GIF stores tens of ms), `resolution` 128.
//!
//! `/phoecord petpet [user:<id or mention>] [url:<link>] [delay] [resolution] [no-server-pfp]`:
//!
//! - `user`: the avatar from `GET /users/{id}/profile` (the server avatar unless
//!   `no-server-pfp:true`), loaded as PNG from `cdn.discordapp.com`;
//! - `url`: only Discord's own image hosts (`cdn.discordapp.com`, `media.discordapp.net`), because
//!   `Host::fetch` only reaches hosts the manifest lists; PNG, JPEG or GIF (its first frame);
//! - the result is uploaded to the channel as `petpet.gif` with `Host::rest_files`. Vencord puts
//!   it in the message box for you to send; here it is sent at once.
//!
//! Not ported: the `image` attachment option (commands take text options only). `resolution` is
//! capped at 512 px: the GIF is made inside the proxy, and a huge one would hold up your traffic.

use crate::commands::{
    Invocation, Kept, Subcommand, invocation, is_interaction, kept_answer, snowflake_arg,
};
use crate::state::Shared;
use image::codecs::gif::{GifEncoder, Repeat};
use image::imageops::{self, FilterType};
use image::{Delay, Frame, RgbaImage};
use phoecord_core::plugin::{FilePart, HostResponse};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "petpet";
const SUB: &str = "petpet";
const FRAMES: usize = 10;
const DEFAULT_DELAY: u32 = 20;
const DEFAULT_RESOLUTION: u32 = 128;
const MAX_RESOLUTION: u32 = 512;
const FRAME_HOST: &str = "raw.githubusercontent.com";
const CDN: &str = "cdn.discordapp.com";
const MEDIA: &str = "media.discordapp.net";

pub struct Petpet {
    manifest: Manifest,
    kept: Kept,
    frames: Shared<Option<Vec<RgbaImage>>>,
}

impl Petpet {
    pub fn new() -> Petpet {
        Petpet {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "petpet",
                    env!("CARGO_PKG_VERSION"),
                    "Make a headpat GIF of an avatar or picture with /phoecord petpet",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("petpet".into());
                m.fetch = vec![FRAME_HOST.into(), CDN.into(), MEDIA.into()];
                m
            },
            kept: Kept::default(),
            frames: Shared::default(),
        }
    }

    /// `/phoecord petpet`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            SUB,
            "Create a petpet gif. You can only specify one of the image options",
            &self.kept,
        )
        .option("user", "User whose avatar to use as image", false)
        .option("url", "Discord image link (cdn.discordapp.com, media.discordapp.net)", false)
        .option(
            "delay",
            "The delay between each frame in ms. Rounded to nearest 10ms. Defaults to the minimum value of 20.",
            false,
        )
        .option("resolution", "Resolution for the gif. Defaults to 128, at most 512.", false)
        .option(
            "no-server-pfp",
            "true: use the normal avatar instead of the server specific one",
            false,
        )
        .boxed()
    }

    /// The hand frames, fetched on first use.
    fn frames(&self, host: &mut dyn Host) -> Result<Vec<RgbaImage>, String> {
        if let Some(f) = self.frames.lock().clone() {
            return Ok(f);
        }
        let mut frames = Vec::with_capacity(FRAMES);
        for i in 0..FRAMES {
            let url = format!("https://{FRAME_HOST}/VenPlugs/petpet/main/frames/pet{i}.gif");
            frames.push(decode(&load(host, &url)?)?);
        }
        *self.frames.lock() = Some(frames.clone());
        Ok(frames)
    }

    fn run(&self, host: &mut dyn Host, inv: &Invocation) -> String {
        match self.make(host, inv) {
            Ok(answer) => answer,
            Err(message) => message,
        }
    }

    /// The answer when it worked, else the error text; both translated.
    fn make(&self, host: &mut dyn Host, inv: &Invocation) -> Result<String, String> {
        let delay = match inv.get("delay").map(str::parse::<u32>) {
            None => DEFAULT_DELAY,
            Some(Ok(d)) if d >= 20 => d,
            Some(_) => return Err(host.t("petpet-delay", &[])),
        };
        let resolution = match inv.get("resolution").map(str::parse::<u32>) {
            None => DEFAULT_RESOLUTION,
            Some(Ok(r)) if (1..=MAX_RESOLUTION).contains(&r) => r,
            Some(_) => return Err(host.t("petpet-resolution", &[])),
        };
        let Some(channel) = inv.channel_id else {
            return Err(host.t("petpet-failed", &[("error", "no channel")]));
        };
        let url = image_url(host, inv)?;
        let fail = |host: &dyn Host, e: String| host.t("petpet-failed", &[("error", &e)]);
        let picture = load(host, &url)
            .and_then(|b| decode(&b))
            .map_err(|e| fail(host, e))?;
        let frames = self.frames(host).map_err(|e| fail(host, e))?;
        let gif = render(&picture, &frames, resolution, delay).map_err(|e| fail(host, e))?;
        let mut payload = Val::map();
        payload.set("content", Val::Str(String::new()));
        let file = FilePart {
            filename: "petpet.gif".into(),
            content_type: "image/gif".into(),
            data: gif,
        };
        match host.rest_files(
            "POST",
            &format!("/channels/{channel}/messages"),
            Some(&payload),
            &[file],
        ) {
            Ok(HostResponse {
                status: 200..=299, ..
            }) => Ok(String::new()),
            Ok(HostResponse { status, .. }) => Err(fail(host, format!("HTTP {status}"))),
            Err(e) => Err(fail(host, e)),
        }
    }
}

impl Default for Petpet {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the picture comes from: the `url` option or the `user`'s avatar (Vencord checks the
/// options in this order too, first one wins).
fn image_url(host: &mut dyn Host, inv: &Invocation) -> Result<String, String> {
    if let Some(url) = inv.get("url") {
        let host_name = url
            .strip_prefix("https://")
            .and_then(|r| r.split(['/', '?']).next())
            .unwrap_or("");
        if host_name != CDN && host_name != MEDIA {
            return Err(host.t("petpet-bad-url", &[]));
        }
        return Ok(url.to_string());
    }
    let Some(user) = inv.get("user") else {
        return Err(host.t("petpet-no-image", &[]));
    };
    let Some(user) = snowflake_arg(user) else {
        return Err(host.t("petpet-bad-user", &[]));
    };
    let server_pfp = !matches!(inv.get("no-server-pfp"), Some("true" | "yes" | "1"));
    let guild = inv.guild_id.filter(|_| server_pfp);
    let route = match guild {
        Some(g) => format!("/users/{user}/profile?guild_id={g}"),
        None => format!("/users/{user}/profile"),
    };
    let profile = match host.rest("GET", &route, None) {
        Ok(HostResponse {
            status: 200..=299,
            body: Some(b),
            ..
        }) => b,
        Ok(HostResponse { status, .. }) => {
            let e = format!("HTTP {status}");
            return Err(host.t("petpet-user-failed", &[("error", &e)]));
        }
        Err(e) => return Err(host.t("petpet-user-failed", &[("error", &e)])),
    };
    Ok(avatar_url(&profile, user, guild))
}

/// The PNG avatar URL of a `GET /users/{id}/profile` answer.
fn avatar_url(profile: &Val, user: u64, guild: Option<u64>) -> String {
    let member = profile
        .str_at(&["guild_member", "avatar"])
        .filter(|h| !h.is_empty());
    match (
        guild,
        member,
        profile
            .str_at(&["user", "avatar"])
            .filter(|h| !h.is_empty()),
    ) {
        (Some(g), Some(hash), _) => {
            format!("https://{CDN}/guilds/{g}/users/{user}/avatars/{hash}.png?size=512")
        }
        (_, _, Some(hash)) => format!("https://{CDN}/avatars/{user}/{hash}.png?size=512"),
        _ => format!("https://{CDN}/embed/avatars/{}.png", (user >> 22) % 6),
    }
}

/// The bytes at `url`, through `Host::fetch`.
fn load(host: &mut dyn Host, url: &str) -> Result<Vec<u8>, String> {
    match host.fetch("GET", url, None)? {
        HostResponse {
            status: 200..=299,
            bytes: Some(b),
            ..
        } if !b.is_empty() => Ok(b),
        HostResponse {
            status: 200..=299, ..
        } => Err("empty answer".into()),
        HostResponse { status, .. } => Err(format!("HTTP {status}")),
    }
}

/// Largest picture petpet decodes, per side: a bigger one (a huge attachment) is refused instead of
/// taking hundreds of MB inside the proxy.
const MAX_INPUT_SIDE: u32 = 4096;
/// Most memory the decoder may take for one picture.
const MAX_INPUT_ALLOC: u64 = 64 * 1024 * 1024;

/// A picture (its first frame for a GIF) as RGBA, within [`MAX_INPUT_SIDE`] and [`MAX_INPUT_ALLOC`].
fn decode(bytes: &[u8]) -> Result<RgbaImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_INPUT_SIDE);
    limits.max_image_height = Some(MAX_INPUT_SIDE);
    limits.max_alloc = Some(MAX_INPUT_ALLOC);
    reader.limits(limits);
    reader
        .decode()
        .map(|i| i.to_rgba8())
        .map_err(|e| e.to_string())
}

/// Vencord's ten frames as a looping GIF.
pub fn render(
    picture: &RgbaImage,
    hands: &[RgbaImage],
    resolution: u32,
    delay_ms: u32,
) -> Result<Vec<u8>, String> {
    let res = resolution as f64;
    let delay = Delay::from_numer_denom_ms(delay_ms.saturating_add(5) / 10 * 10, 1);
    let mut frames = Vec::with_capacity(hands.len());
    for (i, hand) in hands.iter().enumerate() {
        let j = if i < FRAMES / 2 { i } else { FRAMES - i } as f64;
        let width = 0.8 + j * 0.02;
        let height = 0.8 - j * 0.05;
        let x = (1.0 - width) * 0.5 + 0.1;
        let y = 1.0 - height - 0.08;
        let mut canvas = RgbaImage::new(resolution, resolution);
        let pic = imageops::resize(
            picture,
            ((width * res).round() as u32).max(1),
            ((height * res).round() as u32).max(1),
            FilterType::Triangle,
        );
        imageops::overlay(
            &mut canvas,
            &pic,
            (x * res).round() as i64,
            (y * res).round() as i64,
        );
        let hand = imageops::resize(hand, resolution, resolution, FilterType::Triangle);
        imageops::overlay(&mut canvas, &hand, 0, 0);
        frames.push(Frame::from_parts(canvas, 0, 0, delay));
    }
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        encoder.encode_frames(frames).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

impl Plugin for Petpet {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if is_interaction(method, route))
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if let Event::RestRequest {
            method,
            route,
            body: Some(body),
        } = event
            && is_interaction(method, route)
            && let Some(inv) = invocation(body)
            && inv.sub == SUB
        {
            let answer = self.run(host, &inv);
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_huge_picture_is_refused() {
        let big = RgbaImage::new(MAX_INPUT_SIDE + 1, 1);
        let mut png = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(decode(&png).is_err());
        let ok = RgbaImage::new(8, 8);
        let mut png = Vec::new();
        ok.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode(&png).unwrap().dimensions(), (8, 8));
    }

    use super::*;
    use crate::commands::Args;
    use crate::testutil::j;
    use image::{AnimationDecoder, ImageFormat, Rgba};
    use std::collections::HashMap;
    use std::io::Cursor;

    /// A host that answers `fetch` with bytes, `rest` with JSON and records uploads.
    #[derive(Default)]
    struct Pics {
        files: HashMap<String, Vec<u8>>,
        rest: HashMap<String, Val>,
        fetched: Vec<String>,
        uploads: Vec<(String, Vec<FilePart>)>,
    }

    impl Host for Pics {
        fn setting(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_get(&self, _: &str) -> Option<Val> {
            None
        }
        fn kv_set(&mut self, _: &str, _: Val) {}
        fn self_id(&self) -> Option<u64> {
            None
        }
        fn notify(&mut self, _: &str, _: &str) {}
        fn log(&mut self, _: &str) {}
        fn t(&self, key: &str, args: &[(&str, &str)]) -> String {
            crate::testutil::tr("", key, args)
        }
        fn emit_send(&mut self, _: i64, _: Val) {}
        fn guild_emojis(&self, _: u64) -> Vec<u64> {
            Vec::new()
        }
        fn channel_guild(&self, _: u64) -> Option<u64> {
            None
        }
        fn fetch(&mut self, _: &str, url: &str, _: Option<&Val>) -> Result<HostResponse, String> {
            self.fetched.push(url.into());
            let bytes = self.files.get(url).cloned();
            Ok({
                let mut r = HostResponse::new(if bytes.is_some() { 200 } else { 404 });
                r.bytes = bytes;
                r
            })
        }
        fn rest(&mut self, _: &str, route: &str, _: Option<&Val>) -> Result<HostResponse, String> {
            let body = self.rest.get(route).cloned();
            Ok({
                let mut r = HostResponse::new(if body.is_some() { 200 } else { 404 });
                r.body = body;
                r
            })
        }
        fn rest_files(
            &mut self,
            _: &str,
            route: &str,
            _: Option<&Val>,
            files: &[FilePart],
        ) -> Result<HostResponse, String> {
            self.uploads.push((route.into(), files.to_vec()));
            Ok({
                let mut r = HostResponse::new(200);
                r.body = Some(Val::map());
                r
            })
        }
    }

    impl phoecord_core::ChainHost for Pics {}

    fn png(color: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(8, 8, Rgba(color));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn gif_frame() -> Vec<u8> {
        // A hand: the top half opaque, the rest transparent.
        let img = RgbaImage::from_fn(8, 8, |_, y| {
            if y < 4 {
                Rgba([200, 150, 100, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Gif).unwrap();
        out.into_inner()
    }

    fn host() -> Pics {
        let mut h = Pics::default();
        for i in 0..FRAMES {
            h.files.insert(
                format!("https://raw.githubusercontent.com/VenPlugs/petpet/main/frames/pet{i}.gif"),
                gif_frame(),
            );
        }
        h.files.insert(
            "https://cdn.discordapp.com/avatars/5/abc.png?size=512".into(),
            png([0, 0, 255, 255]),
        );
        h.rest.insert(
            "/users/5/profile?guild_id=7".into(),
            j(r#"{"user":{"id":"5","avatar":"abc"},"guild_member":{"avatar":null}}"#),
        );
        h
    }

    fn run(p: &Petpet, h: &mut Pics, opts: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","guild_id":"7","data":{{"name":"phoecord","options":[{{"type":1,"name":"petpet","options":[{opts}]}}]}}}}"#
        ));
        p.on_event(
            h,
            Event::RestRequest {
                method: "POST",
                route: "/interactions",
                body: Some(&mut body),
            },
        );
        p.command().run(h, &Args { options: vec![] })
    }

    #[test]
    fn pets_a_users_avatar_and_uploads_a_looping_gif() {
        let p = Petpet::new();
        let mut h = host();
        let out = run(
            &p,
            &mut h,
            r#"{"type":3,"name":"user","value":"<@5>"},{"type":3,"name":"resolution","value":"32"}"#,
        );
        assert_eq!(out, "");
        let (route, files) = &h.uploads[0];
        assert_eq!(route, "/channels/9/messages");
        assert_eq!(files[0].filename, "petpet.gif");
        assert_eq!(files[0].content_type, "image/gif");
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&files[0].data)).unwrap();
        let frames = decoder.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), FRAMES);
        assert_eq!(frames[0].buffer().dimensions(), (32, 32));
        assert_eq!(frames[0].delay().numer_denom_ms(), (20, 1));
        // Frame 0: the avatar (blue) under the hand's empty half, the hand on top.
        let f = frames[0].buffer();
        assert_eq!(f.get_pixel(16, 2)[3], 255, "hand");
        let blue = f.get_pixel(16, 24);
        assert!(blue[2] > 200 && blue[0] < 50, "{blue:?}");
        assert_eq!(
            f.get_pixel(1, 31)[3],
            0,
            "outside the avatar stays transparent"
        );

        // Frames are fetched once.
        assert_eq!(h.fetched.len(), FRAMES + 1);
        assert_eq!(
            run(&p, &mut h, r#"{"type":3,"name":"user","value":"5"}"#),
            ""
        );
        assert_eq!(h.fetched.len(), FRAMES + 2);
    }

    #[test]
    fn avatar_urls_prefer_the_server_avatar_and_fall_back_to_the_default() {
        let both = j(r#"{"user":{"avatar":"u"},"guild_member":{"avatar":"g"}}"#);
        assert_eq!(
            avatar_url(&both, 5, Some(7)),
            "https://cdn.discordapp.com/guilds/7/users/5/avatars/g.png?size=512"
        );
        assert_eq!(
            avatar_url(&both, 5, None),
            "https://cdn.discordapp.com/avatars/5/u.png?size=512"
        );
        assert_eq!(
            avatar_url(&j("{}"), 3 << 22, None),
            "https://cdn.discordapp.com/embed/avatars/3.png"
        );
    }

    #[test]
    fn checks_options_like_vencord() {
        let p = Petpet::new();
        let mut h = host();
        assert_eq!(run(&p, &mut h, ""), "No Image specified!");
        assert_eq!(
            run(
                &p,
                &mut h,
                r#"{"type":3,"name":"user","value":"5"},{"type":3,"name":"delay","value":"10"}"#
            ),
            "Delay must be at least 20."
        );
        assert!(
            run(&p, &mut h, r#"{"type":3,"name":"user","value":"5"},{"type":3,"name":"resolution","value":"4000"}"#)
                .contains("512")
        );
        assert!(
            run(
                &p,
                &mut h,
                r#"{"type":3,"name":"url","value":"https://evil.example/a.png"}"#
            )
            .contains("cdn.discordapp.com")
        );
        assert!(
            run(
                &p,
                &mut h,
                r#"{"type":3,"name":"url","value":"https://cdn.discordapp.com/x.png"}"#
            )
            .contains("HTTP 404")
        );
        assert!(run(&p, &mut h, r#"{"type":3,"name":"user","value":"bob"}"#).contains("user id"));
        assert!(h.uploads.is_empty());
        assert!(
            p.manifest()
                .fetch
                .contains(&"raw.githubusercontent.com".to_string())
        );
    }
}
