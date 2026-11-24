//! Per-event cost of the WASM path: channel round trip to the worker, `Val` to `tree` copies in
//! both directions and the guest call. Run: `cargo bench -p phoecord-plugin-runtime`.
//!
//! The guest answers `NOOP` with `pass` and makes no host call, so what is measured is the
//! overhead alone, on a realistic MESSAGE_CREATE payload.

#[path = "../tests/common/mod.rs"]
mod common;

use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use phoecord_plugin_runtime::wasm::WasmPlugin;
use phoecord_plugin_runtime::{Chain, NullHost};
use std::hint::black_box;
use std::time::Instant;

fn s(text: &str) -> Val {
    Val::Str(text.into())
}

fn map(pairs: Vec<(&str, Val)>) -> Val {
    Val::Map(pairs.into_iter().map(|(k, v)| (s(k), v)).collect())
}

/// About what Discord sends for one chat message: nested author, member, mentions, attachments.
fn message_create() -> Val {
    let user = |id: &str, name: &str| {
        map(vec![
            ("id", s(id)),
            ("username", s(name)),
            ("discriminator", s("0")),
            ("avatar", s("a1b2c3d4e5f60718293a4b5c6d7e8f90")),
            ("bot", Val::Bool(false)),
            ("public_flags", Val::Int(0)),
        ])
    };
    map(vec![
        ("id", s("1200000000000000001")),
        ("channel_id", s("1100000000000000002")),
        ("guild_id", s("1000000000000000003")),
        ("author", user("1300000000000000004", "someone")),
        (
            "member",
            map(vec![
                ("roles", Val::List(vec![s("1"), s("2"), s("3")])),
                ("joined_at", s("2021-05-04T10:20:30.000000+00:00")),
                ("nick", Val::Null),
            ]),
        ),
        (
            "content",
            s(
                "hey <@1400000000000000005>, have you seen https://example.com/some/long/path?utm_source=x&utm_medium=y yet? it is quite good",
            ),
        ),
        ("timestamp", s("2025-01-02T03:04:05.678000+00:00")),
        (
            "mentions",
            Val::List(vec![user("1400000000000000005", "other")]),
        ),
        (
            "attachments",
            Val::List(vec![map(vec![
                ("id", s("1500000000000000006")),
                ("filename", s("image.png")),
                ("size", Val::Int(123456)),
                (
                    "url",
                    s("https://cdn.discordapp.com/attachments/1/2/image.png"),
                ),
                ("width", Val::Int(800)),
                ("height", Val::Int(600)),
            ])]),
        ),
        ("embeds", Val::List(vec![])),
        ("pinned", Val::Bool(false)),
        ("type", Val::Int(0)),
        ("flags", Val::Int(0)),
        ("nonce", s("1600000000000000007")),
    ])
}

struct Nop(Manifest);

impl Plugin for Nop {
    fn manifest(&self) -> &Manifest {
        &self.0
    }
    fn wants(&self, _: &EventKind) -> bool {
        true
    }
    fn on_event(&self, _: &mut dyn Host, _: Event<'_>) -> Verdict {
        Verdict::Pass
    }
}

fn measure(label: &str, chain: &Chain, name: &str) {
    let mut host = NullHost::default();
    let mut payload = message_create();
    let mut extra = Vec::new();
    for _ in 0..500 {
        chain.dispatch(&mut host, name, &mut payload, &mut extra);
    }
    const N: usize = 5000;
    let mut times = Vec::with_capacity(N);
    for _ in 0..N {
        let t = Instant::now();
        black_box(chain.dispatch(&mut host, name, &mut payload, &mut extra));
        times.push(t.elapsed());
    }
    times.sort();
    let us = |d: std::time::Duration| d.as_secs_f64() * 1e6;
    let mean: f64 = times.iter().map(|d| us(*d)).sum::<f64>() / N as f64;
    println!(
        "{label:<28} mean {mean:7.1} us   p50 {:7.1} us   p99 {:7.1} us   max {:8.1} us",
        us(times[N / 2]),
        us(times[N * 99 / 100]),
        us(times[N - 1]),
    );
}

fn main() {
    let rt = common::runtime();
    let wasm = WasmPlugin::from_bytes(&rt, common::guest_wasm()).unwrap();
    let wasm = Chain::new(vec![Box::new(wasm) as Box<dyn Plugin>]);
    let native = Chain::new(vec![Box::new(Nop(Manifest {
        id: "nop".into(),
        name: "nop".into(),
        version: "0".into(),
        description: String::new(),
        authors: vec![],
        vencord: None,
        layers: vec![Layer::Network],
        api: 1,
        fetch: Vec::new(),
    })) as Box<dyn Plugin>]);
    println!("MESSAGE_CREATE, one plugin, 5000 events after 500 warm-up:");
    measure("native no-op plugin", &native, "MESSAGE_CREATE");
    measure("wasm guest, no-op (NOOP)", &wasm, "NOOP");
}
