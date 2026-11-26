//! VoiceMessages: send an audio file as a voice message (Vencord: VoiceMessages), data part.
//!
//! Ported from Vencord `src/plugins/voiceMessages/index.tsx` (GPL-3.0-or-later, Copyright (c)
//! 2023 Vendicated and contributors; authors Ven, Vap, Nickyux). Vencord records (or takes) an
//! Ogg Opus file, works out its duration and a waveform (at most 256 bins, base64), uploads it as
//! `voice-message.ogg` and posts a message with flag `1 << 13` (`IS_VOICE_MESSAGE`) and the
//! attachment's `waveform` and `duration_secs`.
//!
//! The proxy has no microphone, so recording waits for Phase 2. The data part:
//! `/phoecord voicemessage url:<link to an .ogg>` loads an Ogg Opus file from Discord's own hosts
//! (`cdn.discordapp.com`, `media.discordapp.net`, the only ones `Host::fetch` may reach, for
//! example a file you uploaded before), and sends it with `Host::rest_files` as a voice message
//! to the channel the command was typed in.
//!
//! The duration comes from the Ogg pages (last granule position minus the Opus pre-skip, at
//! 48 kHz). Vencord's waveform needs the decoded audio; here each bin is the Opus bytes per
//! millisecond of its packets (louder parts take more bytes at a variable bitrate), with
//! Vencord's bin count and normalization. That is an approximation of the shape, not the RMS.

use crate::commands::{Kept, Subcommand, invocation, is_interaction, kept_answer};
use phoecord_core::plugin::{FilePart, HostResponse};
use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "voice_messages";
const SUB: &str = "voicemessage";
const CDN: &str = "cdn.discordapp.com";
const MEDIA: &str = "media.discordapp.net";
/// Message flag `IS_VOICE_MESSAGE`.
const VOICE_MESSAGE: i64 = 1 << 13;
const FILENAME: &str = "voice-message.ogg";

pub struct VoiceMessages {
    manifest: Manifest,
    kept: Kept,
}

impl VoiceMessages {
    pub fn new() -> VoiceMessages {
        VoiceMessages {
            manifest: {
                let mut m = Manifest::new(
                    PLUGIN_ID,
                    "VoiceMessages",
                    env!("CARGO_PKG_VERSION"),
                    "Send an .ogg file as a voice message with /phoecord voicemessage",
                );
                m.authors = vec!["Cheterin Group".into()];
                m.vencord = Some("VoiceMessages".into());
                m.fetch = vec![CDN.into(), MEDIA.into()];
                m
            },
            kept: Kept::default(),
        }
    }

    /// `/phoecord voicemessage`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        kept_answer(
            PLUGIN_ID,
            SUB,
            "Send an .ogg file as a voice message",
            &self.kept,
        )
        .option(
            "url",
            "Link to an Ogg Opus file on cdn.discordapp.com or media.discordapp.net",
            true,
        )
        .boxed()
    }
}

impl Default for VoiceMessages {
    fn default() -> Self {
        Self::new()
    }
}

/// What an Ogg Opus file says about itself.
#[derive(Debug, PartialEq)]
pub struct Audio {
    pub duration_secs: f64,
    /// `(duration in ms, bytes)` of every audio packet, in order.
    pub packets: Vec<(f64, usize)>,
}

/// Milliseconds of audio in one Opus packet (RFC 6716 §3.1: the TOC byte).
fn packet_ms(packet: &[u8]) -> f64 {
    let Some(&toc) = packet.first() else {
        return 0.0;
    };
    let config = toc >> 3;
    let frame = match config {
        0..=11 => [10.0, 20.0, 40.0, 60.0][usize::from(config % 4)],
        12..=15 => [10.0, 20.0][usize::from(config % 2)],
        _ => [2.5, 5.0, 10.0, 20.0][usize::from(config % 4)],
    };
    let frames = match toc & 0b11 {
        0 => 1.0,
        1 | 2 => 2.0,
        _ => packet.get(1).map_or(0.0, |b| f64::from(b & 0x3f)),
    };
    frame * frames
}

/// Read an Ogg Opus file: duration and packets; `Err` for anything else.
pub fn parse_ogg_opus(data: &[u8]) -> Result<Audio, &'static str> {
    let mut pos = 0;
    let mut packet = Vec::new();
    let mut packets = Vec::new();
    let mut index = 0usize;
    let mut pre_skip = 0u64;
    let mut last_granule = 0u64;
    while pos < data.len() {
        let header = data.get(pos..pos + 27).ok_or("cut Ogg page")?;
        if &header[..4] != b"OggS" {
            return Err("not an Ogg file");
        }
        let granule = u64::from_le_bytes(header[6..14].try_into().expect("8 bytes"));
        let segments = usize::from(header[26]);
        let table = data
            .get(pos + 27..pos + 27 + segments)
            .ok_or("cut Ogg page")?;
        let mut body = pos + 27 + segments;
        for &lace in table {
            let len = usize::from(lace);
            packet.extend_from_slice(data.get(body..body + len).ok_or("cut Ogg page")?);
            body += len;
            if lace < 255 {
                match index {
                    0 if packet.starts_with(b"OpusHead") && packet.len() >= 12 => {
                        pre_skip = u64::from(u16::from_le_bytes([packet[10], packet[11]]));
                    }
                    0 => return Err("not an Opus file"),
                    1 => {}
                    _ => packets.push((packet_ms(&packet), packet.len())),
                }
                index += 1;
                packet.clear();
            }
        }
        if granule != u64::MAX {
            last_granule = last_granule.max(granule);
        }
        pos = body;
    }
    if packets.is_empty() {
        return Err("no audio in the file");
    }
    Ok(Audio {
        duration_secs: last_granule.saturating_sub(pre_skip) as f64 / 48_000.0,
        packets,
    })
}

/// Vencord's bins: `clamp(floor(duration * 10), min(32, samples), 256)` bins, each the average of
/// its share, then the same easing normalization; base64.
pub fn waveform(audio: &Audio) -> String {
    let total_ms: f64 = audio.packets.iter().map(|(ms, _)| ms).sum();
    let n = ((audio.duration_secs * 10.0).floor() as usize).clamp(audio.packets.len().min(32), 256);
    let mut sums = vec![(0.0f64, 0.0f64); n.max(1)];
    let mut at = 0.0;
    for &(ms, bytes) in &audio.packets {
        let bin = if total_ms > 0.0 {
            ((at / total_ms) * sums.len() as f64) as usize
        } else {
            0
        };
        let last = sums.len() - 1;
        let slot = &mut sums[bin.min(last)];
        slot.0 += bytes as f64;
        slot.1 += ms.max(1.0);
        at += ms;
    }
    let rate: Vec<f64> = sums
        .iter()
        .map(|(b, ms)| if *ms > 0.0 { b / ms } else { 0.0 })
        .collect();
    let peak = rate.iter().cloned().fold(0.0, f64::max);
    let mut bins: Vec<u8> = rate
        .iter()
        .map(|r| {
            if peak > 0.0 {
                (r / peak * 255.0) as u8
            } else {
                0
            }
        })
        .collect();
    let max = f64::from(bins.iter().copied().max().unwrap_or(0));
    if max > 0.0 {
        let ratio = 1.0 + (255.0 / max - 1.0) * (100.0 * (max / 255.0).powi(3)).min(1.0);
        for b in &mut bins {
            *b = (f64::from(*b) * ratio).min(255.0) as u8;
        }
    }
    base64(&bins)
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn send(host: &mut dyn Host, channel: u64, url: &str) -> String {
    let name = url
        .strip_prefix("https://")
        .and_then(|r| r.split(['/', '?']).next())
        .unwrap_or("");
    if name != CDN && name != MEDIA {
        return host.t("voice_messages-bad-url", &[]);
    }
    let data = match host.fetch("GET", url, None) {
        Ok(HostResponse {
            status: 200..=299,
            bytes: Some(b),
            ..
        }) => b,
        Ok(HostResponse { status, .. }) => {
            return host.t(
                "voice_messages-failed",
                &[("error", &format!("HTTP {status}"))],
            );
        }
        Err(e) => return host.t("voice_messages-failed", &[("error", &e)]),
    };
    let audio = match parse_ogg_opus(&data) {
        Ok(a) => a,
        Err(e) => return host.t("voice_messages-not-opus", &[("error", e)]),
    };
    let mut attachment = Val::map();
    attachment.set("id", Val::Str("0".into()));
    attachment.set("filename", Val::Str(FILENAME.into()));
    attachment.set("waveform", Val::Str(waveform(&audio)));
    attachment.set("duration_secs", Val::Float(audio.duration_secs.max(0.1)));
    let mut payload = Val::map();
    payload.set("flags", Val::Int(VOICE_MESSAGE));
    payload.set("content", Val::Str(String::new()));
    payload.set("attachments", Val::List(vec![attachment]));
    let file = FilePart {
        filename: FILENAME.into(),
        content_type: "audio/ogg".into(),
        data,
    };
    match host.rest_files(
        "POST",
        &format!("/channels/{channel}/messages"),
        Some(&payload),
        &[file],
    ) {
        Ok(HostResponse {
            status: 200..=299, ..
        }) => String::new(),
        Ok(HostResponse { status, .. }) => host.t(
            "voice_messages-failed",
            &[("error", &format!("HTTP {status}"))],
        ),
        Err(e) => host.t("voice_messages-failed", &[("error", &e)]),
    }
}

impl Plugin for VoiceMessages {
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
            let answer = match (inv.channel_id, inv.get("url")) {
                (Some(channel), Some(url)) => send(host, channel, url),
                _ => host.t("voice_messages-bad-url", &[]),
            };
            *self.kept.lock() = Some(answer);
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Args;
    use crate::testutil::j;
    use std::collections::HashMap;

    /// One Ogg page holding `packets` (each under 255 bytes) with this granule position.
    fn page(granule: u64, packets: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"OggS".to_vec();
        out.extend([0, 0]);
        out.extend(granule.to_le_bytes());
        out.extend([0; 12]);
        out.push(packets.len() as u8);
        out.extend(packets.iter().map(|p| p.len() as u8));
        for p in packets {
            out.extend(p);
        }
        out
    }

    /// A 2-second Ogg Opus file: 100 packets of 20 ms, loud in the middle.
    fn ogg() -> Vec<u8> {
        let mut head = b"OpusHead".to_vec();
        head.extend([1, 1, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]); // pre-skip 312
        let mut file = page(0, &[head]);
        file.extend(page(0, &[b"OpusTags........".to_vec()]));
        let audio: Vec<Vec<u8>> = (0..100)
            .map(|i| {
                // TOC 0xf8: config 31 (CELT, 20 ms), one frame.
                let size = if (40..60).contains(&i) { 120 } else { 20 };
                let mut p = vec![0xf8];
                p.resize(size, 0);
                p
            })
            .collect();
        for (n, chunk) in audio.chunks(50).enumerate() {
            file.extend(page(312 + (n as u64 + 1) * 48_000, chunk));
        }
        file
    }

    #[test]
    fn reads_duration_and_packets_of_ogg_opus() {
        let a = parse_ogg_opus(&ogg()).unwrap();
        assert_eq!(a.duration_secs, 2.0);
        assert_eq!(a.packets.len(), 100);
        assert_eq!(a.packets[0], (20.0, 20));
        assert_eq!(parse_ogg_opus(b"RIFF....").unwrap_err(), "cut Ogg page");
        assert_eq!(
            parse_ogg_opus(&page(0, &[b"Vorbis....".to_vec()])),
            Err("not an Opus file")
        );
        assert_eq!(packet_ms(&[0x18]), 60.0, "SILK 60 ms");
        assert_eq!(packet_ms(&[0xfb, 0x03]), 60.0, "CELT 20 ms x 3 frames");
    }

    #[test]
    fn waveform_has_vencords_bin_count_and_a_loud_middle() {
        let w = waveform(&parse_ogg_opus(&ogg()).unwrap());
        // 2 s gives 20, but Vencord takes at least 32 bins → 44 base64 characters.
        assert_eq!(w.len(), 44);
        let bins = decode64(&w);
        assert_eq!(bins.len(), 32);
        assert_eq!(bins[16], 255);
        assert!(bins[0] < bins[16] / 3, "{bins:?}");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
    }

    fn decode64(s: &str) -> Vec<u8> {
        const ABC: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let vals: Vec<u32> = s
            .bytes()
            .filter(|b| *b != b'=')
            .map(|b| ABC.find(b as char).unwrap() as u32)
            .collect();
        let mut out = Vec::new();
        for c in vals.chunks(4) {
            let n = c
                .iter()
                .enumerate()
                .fold(0, |n, (i, v)| n | v << (18 - 6 * i));
            for i in 0..c.len() - 1 {
                out.push((n >> (16 - 8 * i)) as u8);
            }
        }
        out
    }

    #[derive(Default)]
    struct Files {
        files: HashMap<String, Vec<u8>>,
        uploads: Vec<(String, Option<Val>, Vec<FilePart>)>,
    }

    impl Host for Files {
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
            let bytes = self.files.get(url).cloned();
            Ok({
                let mut r = HostResponse::new(if bytes.is_some() { 200 } else { 404 });
                r.bytes = bytes;
                r
            })
        }
        fn rest_files(
            &mut self,
            _: &str,
            route: &str,
            payload: Option<&Val>,
            files: &[FilePart],
        ) -> Result<HostResponse, String> {
            self.uploads
                .push((route.into(), payload.cloned(), files.to_vec()));
            Ok({
                let mut r = HostResponse::new(200);
                r.body = Some(Val::map());
                r
            })
        }
    }

    impl phoecord_core::ChainHost for Files {}

    fn run(p: &VoiceMessages, h: &mut Files, url: &str) -> String {
        let mut body = j(&format!(
            r#"{{"type":2,"channel_id":"9","data":{{"name":"phoecord","options":[{{"type":1,"name":"voicemessage","options":[{{"type":3,"name":"url","value":"{url}"}}]}}]}}}}"#
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
    fn sends_a_voice_message_like_vencord() {
        let p = VoiceMessages::new();
        let url = "https://cdn.discordapp.com/attachments/1/2/hi.ogg";
        let mut h = Files::default();
        h.files.insert(url.into(), ogg());
        assert_eq!(run(&p, &mut h, url), "");
        let (route, payload, files) = &h.uploads[0];
        assert_eq!(route, "/channels/9/messages");
        let payload = payload.as_ref().unwrap();
        assert_eq!(payload.get("flags"), Some(&Val::Int(8192)));
        let a = &payload.get("attachments").unwrap().as_list()[0];
        assert_eq!(a.str_at(&["filename"]), Some("voice-message.ogg"));
        assert_eq!(a.get("duration_secs"), Some(&Val::Float(2.0)));
        assert_eq!(a.str_at(&["waveform"]).unwrap().len(), 44);
        assert_eq!(files[0].filename, "voice-message.ogg");
        assert_eq!(files[0].content_type, "audio/ogg");
    }

    #[test]
    fn refuses_other_hosts_and_other_files() {
        let p = VoiceMessages::new();
        let mut h = Files::default();
        assert!(run(&p, &mut h, "https://evil.example/a.ogg").contains("cdn.discordapp.com"));
        let url = "https://media.discordapp.net/a.mp3";
        h.files.insert(url.into(), b"ID3...".to_vec());
        assert!(run(&p, &mut h, url).contains("Ogg Opus"));
        assert!(run(&p, &mut h, "https://cdn.discordapp.com/none.ogg").contains("HTTP 404"));
        assert!(h.uploads.is_empty());
    }
}
