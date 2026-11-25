//! Cost of the plugin chain per gateway frame: decode → `Proxy::on_dispatch` (account tracking and
//! every built-in plugin) → encode, plus one REST response. Task 1.1.10, goal: p99 ≤ 1 ms per
//! MESSAGE_CREATE. Run: `cargo bench -p phoecord-proxy --bench chain` (criterion means, then a
//! p50/p99 table). All payloads are generated here; nothing is real Discord data.
//!
//! `cargo bench -p phoecord-proxy --bench chain -- --test` runs every case once (smoke test).

use criterion::{Criterion, Throughput};
use phoecord_core::Val;
use phoecord_plugin_runtime::Chain;
use phoecord_proxy::Proxy;
use phoecord_proxy::codec::Codec;
use phoecord_proxy::config::{Config, Paths};
use serde_json::{Value, json};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

/// Tiny deterministic generator, so every run benches the same bytes.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn id(&mut self) -> String {
        (1_000_000_000_000_000_000u64 + self.next() * 1_000_003).to_string()
    }
    fn word(&mut self) -> String {
        const W: [&str; 12] = [
            "alpha", "bravo", "lounge", "dev", "music", "memes", "general", "voice", "help",
            "news", "games", "art",
        ];
        format!("{}-{}", W[self.next() as usize % 12], self.next() % 1000)
    }
}

fn user(r: &mut Rng) -> Value {
    json!({"id": r.id(), "username": r.word(), "discriminator": "0", "global_name": r.word(),
           "avatar": "a1b2c3d4e5f60718293a4b5c6d7e8f90", "bot": false, "public_flags": 0})
}

fn guild(r: &mut Rng, channels: usize, members: usize) -> Value {
    let chans: Vec<Value> = (0..channels)
        .map(|i| {
            json!({"id": r.id(), "type": if i % 5 == 0 { 2 } else { 0 }, "name": r.word(),
                   "position": i, "parent_id": r.id(), "topic": "synthetic channel topic text",
                   "permission_overwrites": [{"id": r.id(), "type": 0, "allow": "1024", "deny": "0"}],
                   "nsfw": false, "rate_limit_per_user": 0, "last_message_id": r.id()})
        })
        .collect();
    let roles: Vec<Value> = (0..12)
        .map(|_| {
            json!({"id": r.id(), "name": r.word(), "color": 0, "permissions": "1071698660929",
                   "position": 1, "hoist": false, "managed": false, "mentionable": false})
        })
        .collect();
    let emojis: Vec<Value> = (0..10)
        .map(|_| json!({"id": r.id(), "name": r.word(), "animated": false, "available": true}))
        .collect();
    let mem: Vec<Value> = (0..members)
        .map(|_| {
            json!({"user": user(r), "roles": [r.id()], "nick": null,
                   "joined_at": "2021-05-04T10:20:30.000000+00:00", "flags": 0})
        })
        .collect();
    json!({"id": r.id(), "name": r.word(), "icon": "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
           "owner_id": r.id(), "member_count": 1200, "channels": chans, "roles": roles,
           "emojis": emojis, "members": mem, "threads": [], "premium_tier": 1,
           "features": ["COMMUNITY", "NEWS"]})
}

/// A READY of about `target` bytes: user, many guilds with channels, roles, emojis and members,
/// DMs, relationships, read states.
fn ready(target: usize) -> Vec<u8> {
    let mut r = Rng(7);
    let mut guilds = Vec::new();
    let mut size = 0;
    let mut me = user(&mut r);
    me["id"] = json!("1999999999999999999");
    while size < target {
        let g = guild(&mut r, 40, 25);
        size += serde_json::to_vec(&g).unwrap().len();
        guilds.push(g);
    }
    let privates: Vec<Value> = (0..40)
        .map(|_| json!({"id": r.id(), "type": 1, "recipients": [user(&mut r)], "last_message_id": r.id()}))
        .collect();
    let rels: Vec<Value> = (0..150)
        .map(|_| json!({"id": r.id(), "type": 1, "user": user(&mut r), "since": "2022-01-01T00:00:00.000000+00:00"}))
        .collect();
    let v = json!({"op": 0, "s": 1, "t": "READY", "d": {
        "v": 10, "user": me, "session_id": "0123456789abcdef0123456789abcdef", "guilds": guilds,
        "private_channels": privates, "relationships": rels, "users": [],
        "read_state": {"entries": [], "version": 1}, "user_settings": {"theme": "dark"}}});
    serde_json::to_vec(&v).unwrap()
}

fn message_create(r: &mut Rng, n: usize) -> Vec<u8> {
    let other = r.id();
    let content = format!(
        "hey <@{other}>, have you seen https://example.com/some/long/path?utm_source=x&utm_medium=y yet? message {n}"
    );
    let v = json!({"op": 0, "s": n + 2, "t": "MESSAGE_CREATE", "d": {
        "id": r.id(), "channel_id": "1100000000000000002", "guild_id": "1000000000000000003",
        "author": {"id": "1300000000000000004", "username": "someone", "discriminator": "0",
                   "avatar": "a1b2c3d4e5f60718293a4b5c6d7e8f90", "bot": false, "public_flags": 0},
        "member": {"roles": ["1", "2", "3"], "joined_at": "2021-05-04T10:20:30.000000+00:00", "nick": null},
        "content": content,
        "timestamp": "2025-01-02T03:04:05.678000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": [{"id": other, "username": "other", "discriminator": "0"}],
        "mention_roles": [], "attachments": [{"id": r.id(), "filename": "image.png", "size": 1234,
            "url": "https://cdn.example.com/a/image.png", "width": 64, "height": 64}],
        "embeds": [], "pinned": false, "type": 0, "flags": 0, "nonce": r.id()}});
    serde_json::to_vec(&v).unwrap()
}

fn rest_messages() -> Val {
    let mut r = Rng(11);
    let msgs: Vec<Value> = (0..50)
        .map(|n| {
            let content = format!("see https://example.com/p?utm_source=x message {n}");
            json!({"id": r.id(), "channel_id": "1100000000000000002", "type": 0,
                   "author": user(&mut r), "content": content,
                   "timestamp": "2025-01-02T03:04:05.678000+00:00", "attachments": [], "embeds": [],
                   "mentions": [], "mention_roles": [], "pinned": false, "flags": 0})
        })
        .collect();
    Val::from_json(Value::Array(msgs))
}

/// A proxy over a scratch data dir; `all` switches every built-in plugin on, else only the default.
fn proxy(all: bool, dir: &std::path::Path) -> Arc<Proxy> {
    let paths = Paths {
        config: dir.join("config.toml"),
        data: dir.to_path_buf(),
    };
    let chain = Chain::new(phoecord_plugins::builtin());
    let mut cfg = Config::default();
    if all {
        for id in chain.ids() {
            cfg.set_enabled(&id, true).unwrap();
        }
    }
    Proxy::new(paths, cfg, chain, false).unwrap()
}

/// Decode, run the chain, encode (always, which is the worst case: the proxy re-encodes only
/// when a plugin changed the frame).
fn frame(app: &Proxy, codec: &Codec, bytes: &[u8]) -> usize {
    let v = codec.decode(bytes).unwrap();
    let d = app.on_dispatch(v);
    codec.encode(&d.payload).len()
}

/// (p50, p99, max) in milliseconds over `rounds` passes of `frames`.
fn percentiles(app: &Proxy, codec: &Codec, frames: &[Vec<u8>], rounds: usize) -> (f64, f64, f64) {
    let mut t = Vec::with_capacity(frames.len() * rounds);
    for _ in 0..rounds {
        for f in frames {
            let s = Instant::now();
            black_box(frame(app, codec, black_box(f)));
            t.push(s.elapsed().as_secs_f64() * 1e3);
        }
    }
    t.sort_by(f64::total_cmp);
    let at = |q: f64| t[((t.len() - 1) as f64 * q) as usize];
    (at(0.5), at(0.99), t[t.len() - 1])
}

fn main() {
    let quick = std::env::args().any(|a| a == "--test");
    let codec = Codec::from_query("v=10&encoding=json").unwrap();
    let dir = std::env::temp_dir().join(format!("phoecord-chain-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ready_bytes = ready(5 * 1024 * 1024);
    let mut rng = Rng(42);
    let burst: Vec<Vec<u8>> = (0..200).map(|n| message_create(&mut rng, n)).collect();
    let body = rest_messages();

    let mut c = Criterion::default().configure_from_args();
    let mut rows = Vec::new();
    for (label, all) in [("defaults", false), ("all_on", true)] {
        let app = proxy(all, &dir);
        // READY first, so account state (self id, guilds, channels) is filled like in a session.
        frame(&app, &codec, &ready_bytes);
        let mut g = c.benchmark_group(format!("chain/{label}"));
        g.sample_size(10);
        g.throughput(Throughput::Bytes(ready_bytes.len() as u64));
        g.bench_function("ready_5mb", |b| {
            b.iter(|| frame(&app, &codec, &ready_bytes))
        });
        g.sample_size(100);
        g.throughput(Throughput::Elements(burst.len() as u64));
        g.bench_function("message_create_burst200", |b| {
            b.iter(|| burst.iter().map(|f| frame(&app, &codec, f)).sum::<usize>())
        });
        g.bench_function("rest_messages50", |b| {
            b.iter(|| {
                let mut v = body.clone();
                app.on_response("GET", "/channels/1100000000000000002/messages", 200, &mut v)
            })
        });
        g.finish();
        if !quick {
            let (a, b, m) = percentiles(&app, &codec, &burst, 100);
            rows.push(format!(
                "{label:9} MESSAGE_CREATE  p50 {a:.3} ms  p99 {b:.3} ms  max {m:.3} ms"
            ));
            let (a, b, m) = percentiles(&app, &codec, std::slice::from_ref(&ready_bytes), 30);
            rows.push(format!(
                "{label:9} READY 5 MB      p50 {a:.1} ms  p99 {b:.1} ms  max {m:.1} ms"
            ));
        }
    }
    c.final_summary();
    if !quick {
        // Where a READY goes: decode, chain, encode (best of 5, all plugins on).
        let app = proxy(true, &dir);
        let best = |mut f: Box<dyn FnMut()>| {
            (0..5)
                .map(|_| {
                    let s = Instant::now();
                    f();
                    s.elapsed().as_secs_f64() * 1e3
                })
                .fold(f64::MAX, f64::min)
        };
        let dec = best(Box::new(|| {
            drop(black_box(codec.decode(&ready_bytes).unwrap()))
        }));
        let v = codec.decode(&ready_bytes).unwrap();
        let chain = best(Box::new(|| drop(black_box(app.on_dispatch(v.clone())))));
        let clone = best(Box::new(|| drop(black_box(v.clone()))));
        let enc = best(Box::new(|| drop(black_box(codec.encode(&v)))));
        rows.push(format!(
            "READY 5 MB split (best of 5, all on): decode {dec:.1} ms, chain {chain:.1} ms (incl. a {clone:.1} ms clone made by the bench), encode {enc:.1} ms"
        ));
        // The first MESSAGE_CREATE on a fresh proxy, before any lazy state is warm.
        let fresh = proxy(true, &dir);
        let s = Instant::now();
        black_box(frame(&fresh, &codec, &burst[0]));
        rows.push(format!(
            "first MESSAGE_CREATE on a fresh proxy: {:.3} ms",
            s.elapsed().as_secs_f64() * 1e3
        ));
    }
    for r in rows {
        println!("{r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
