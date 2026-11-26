//! The dev console endpoint (1.3.9): token and loopback rules, the event stream, and the commands.
//! A real listener on loopback, raw HTTP, a small probe plugin; no network, no Discord.

use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, Val, Verdict};
use phoecord_plugin_runtime::Chain;
use phoecord_proxy::config::{Config, Paths};
use phoecord_proxy::dev::{self, DevRequest, Reply};
use phoecord_proxy::{Proxy, server};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Wants `MESSAGE_CREATE`, logs it, passes; `DROPME` is dropped.
struct Probe(Manifest);

impl Probe {
    fn new() -> Probe {
        Probe(Manifest::new("probe", "Probe", "1.0.0", String::new()))
    }
}

impl Plugin for Probe {
    fn manifest(&self) -> &Manifest {
        &self.0
    }
    fn wants(&self, k: &EventKind) -> bool {
        matches!(k, EventKind::Dispatch { name } if *name == "MESSAGE_CREATE" || *name == "DROPME")
    }
    fn on_event(&self, h: &mut dyn Host, e: Event<'_>) -> Verdict {
        h.kv_set("seen", Val::Int(1));
        if let Event::Dispatch { name, .. } = e {
            h.log(&format!("got {name}"));
            if name == "DROPME" {
                return Verdict::Drop;
            }
        }
        Verdict::Pass
    }
}

struct Rig {
    proxy: Arc<Proxy>,
    addr: SocketAddr,
    token: String,
    config: std::path::PathBuf,
}

async fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("phoecord-dev-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let paths = Paths {
        config: dir.join("config.toml"),
        data: dir.join("data"),
    };
    let mut cfg = Config::default();
    cfg.set_enabled("probe", true).unwrap();
    cfg.save(&paths.config).unwrap();
    let config = paths.config.clone();
    let chain = Chain::new(vec![Box::new(Probe::new())]);
    let proxy = Proxy::new(paths, cfg, chain, false).unwrap();
    proxy.publish_dev_token().unwrap();
    let listener = server::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(server::serve(proxy.clone(), listener));
    let token = proxy.dev_token.clone();
    Rig {
        proxy,
        addr,
        token,
        config,
    }
}

fn message(id: i64) -> Val {
    let mut d = Val::map();
    d.set("id", Val::Int(id));
    let mut p = Val::map();
    p.set("op", Val::Int(0));
    p.set("t", Val::Str("MESSAGE_CREATE".into()));
    p.set("d", d);
    p
}

async fn open(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> TcpStream {
    let mut c = TcpStream::connect(addr).await.unwrap();
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    c.write_all(
        format!(
            "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    c
}

/// One whole answer: (status code, body).
async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    let mut c = open(addr, method, path, token, body).await;
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(5), c.read_to_string(&mut out))
        .await
        .expect("answered")
        .unwrap();
    let code = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (code, body)
}

async fn command(r: &Rig, body: Value) -> Value {
    let (code, text) = request(
        r.addr,
        "POST",
        "/__phoecord/dev/command",
        Some(&r.token),
        &body.to_string(),
    )
    .await;
    assert_eq!(code, 200, "{text}");
    serde_json::from_str(text.trim()).unwrap()
}

/// A chunked NDJSON answer read as it comes; `until(n)` returns all lines so far once there are `n`.
struct Feed {
    c: TcpStream,
    seen: String,
}

impl Feed {
    async fn open(r: &Rig) -> Feed {
        Feed {
            c: open(r.addr, "GET", "/__phoecord/dev/stream", Some(&r.token), "").await,
            seen: String::new(),
        }
    }

    async fn until(&mut self, n: usize) -> Vec<Value> {
        let mut buf = [0u8; 4096];
        loop {
            let found: Vec<Value> = self
                .seen
                .lines()
                .filter(|l| l.starts_with('{'))
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            if found.len() >= n {
                return found;
            }
            let got = tokio::time::timeout(Duration::from_secs(5), self.c.read(&mut buf))
                .await
                .unwrap_or_else(|_| panic!("{n} lines expected; got: {:?}", self.seen))
                .unwrap();
            assert!(got > 0, "stream closed after: {}", self.seen);
            self.seen.push_str(&String::from_utf8_lossy(&buf[..got]));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_token_is_written_to_the_data_folder_and_required() {
    let r = rig("token").await;
    let file = std::fs::read_to_string(dev::token_path(&r.proxy.paths.data)).unwrap();
    assert_eq!(file, r.token);
    assert_eq!(r.token.len(), 64);

    for (token, what) in [(None, "none"), (Some("0".repeat(64)), "wrong")] {
        for (m, p) in [
            ("GET", "/__phoecord/dev/stream"),
            ("POST", "/__phoecord/dev/command"),
        ] {
            let (code, _) = request(r.addr, m, p, token.as_deref(), "{}").await;
            assert_eq!(code, 401, "{m} {p} with {what} token");
        }
    }
    // A token of another proxy start is no good either.
    let (code, _) = request(
        r.addr,
        "POST",
        "/__phoecord/dev/command",
        Some(&format!("{}x", r.token)),
        "{}",
    )
    .await;
    assert_eq!(code, 401);
    // A wrong token does not attach: nothing is copied for it.
    assert!(!r.proxy.shared.dev.active());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_proxy_on_the_same_data_folder_keeps_the_token_file() {
    // `phoecord selftest` builds its own Proxy on the user's data folder while `phoecord run`
    // may be serving: the running proxy's token must stay in `dev-token`.
    let r = rig("second").await;
    let other = Proxy::new(
        Paths {
            config: r.config.clone(),
            data: r.proxy.paths.data.clone(),
        },
        Config::default(),
        Chain::default(),
        true,
    )
    .unwrap();
    assert_ne!(other.dev_token, r.token);
    let file = std::fs::read_to_string(dev::token_path(&r.proxy.paths.data)).unwrap();
    assert_eq!(file, r.token);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_that_is_not_loopback_is_refused_even_with_the_token() {
    let r = rig("peer").await;
    for peer in ["192.168.1.20:5000", "8.8.8.8:443", "[2001:db8::1]:1"] {
        let reply = dev::route(
            &r.proxy,
            DevRequest {
                peer: peer.parse().unwrap(),
                method: &hyper::Method::POST,
                path: "/__phoecord/dev/command",
                authorization: Some(&format!("Bearer {}", r.token)),
                body: br#"{"cmd":"plugins"}"#,
            },
        )
        .await;
        assert!(
            matches!(reply, Reply::Status(s) if s == hyper::StatusCode::FORBIDDEN),
            "{peer}"
        );
    }
    let loopback = dev::route(
        &r.proxy,
        DevRequest {
            peer: "127.0.0.1:5000".parse().unwrap(),
            method: &hyper::Method::POST,
            path: "/__phoecord/dev/command",
            authorization: Some(&format!("Bearer {}", r.token)),
            body: br#"{"cmd":"plugins"}"#,
        },
    )
    .await;
    assert!(matches!(loopback, Reply::Json(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_stream_shows_logs_events_verdicts_and_timings() {
    let r = rig("stream").await;
    let mut c = Feed::open(&r).await;
    let hello = c.until(1).await;
    assert_eq!(hello[0]["type"], "hello");
    assert_eq!(hello[0]["plugins"][0]["id"], "probe");
    assert_eq!(hello[0]["plugins"][0]["enabled"], true);
    assert!(r.proxy.shared.dev.active());

    r.proxy.on_dispatch(message(1));
    let mut drop_frame = message(2);
    drop_frame.set("t", Val::Str("DROPME".into()));
    assert!(r.proxy.on_dispatch(drop_frame).drop);

    let got = c.until(5).await;
    let log = &got[1];
    assert_eq!(log["type"], "log");
    assert_eq!(log["plugin"], "probe");
    assert_eq!(log["message"], "got MESSAGE_CREATE");
    let run = &got[2];
    assert_eq!(run["type"], "run");
    assert_eq!(run["kind"], "dispatch");
    assert_eq!(run["event"], "MESSAGE_CREATE");
    assert_eq!(run["plugin"], "probe");
    assert_eq!(run["verdict"], "pass");
    assert!(run["micros"].is_u64());
    assert_eq!(got[4]["event"], "DROPME");
    assert_eq!(got[4]["verdict"], "drop");

    // The console going away detaches it.
    drop(c);
    for _ in 0..50 {
        r.proxy.on_dispatch(message(3));
        if !r.proxy.shared.dev.active() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!r.proxy.shared.dev.active(), "a gone console must detach");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn toggle_flips_the_plugin_in_the_config_and_in_the_chain() {
    let r = rig("toggle").await;
    let list = command(&r, json!({"cmd": "plugins"})).await;
    assert_eq!(list["result"][0]["enabled"], true);
    assert_eq!(list["result"][0]["source"], "builtin");

    let off = command(&r, json!({"cmd": "toggle", "plugin": "probe"})).await;
    assert_eq!(off["result"]["enabled"], false);
    assert!(!Config::load(&r.config).unwrap().is_enabled("probe"));
    assert!(r.proxy.chain.enabled_ids().is_empty(), "applied at once");
    assert!(!r.proxy.on_dispatch(message(1)).changed);

    let on = command(&r, json!({"cmd": "toggle", "plugin": "probe"})).await;
    assert_eq!(on["result"]["enabled"], true);
    assert_eq!(r.proxy.chain.enabled_ids(), vec!["probe".to_string()]);

    let bad = command(&r, json!({"cmd": "toggle", "plugin": "nope"})).await;
    assert_eq!(bad["ok"], false);
    let none = command(&r, json!({"cmd": "toggle"})).await;
    assert_eq!(none["ok"], false);
    let unknown = command(&r, json!({"cmd": "explode"})).await;
    assert_eq!(unknown["ok"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reload_says_why_a_built_in_cannot_be_reloaded() {
    let r = rig("reload").await;
    let out = command(&r, json!({"cmd": "reload", "plugin": "probe"})).await;
    assert_eq!(out["ok"], false);
    assert!(out["error"].as_str().unwrap().contains("built in"), "{out}");
    let out = command(&r, json!({"cmd": "reload", "plugin": "nope"})).await;
    assert_eq!(out["ok"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_and_kv_are_shown_and_replay_runs_one_plugin_dry() {
    let r = rig("replay").await;
    // An event from before anything was attached cannot be replayed.
    r.proxy.on_dispatch(message(1));
    let none = command(
        &r,
        json!({"cmd": "replay", "event": "MESSAGE_CREATE", "plugin": "probe"}),
    )
    .await;
    assert_eq!(none["ok"], false);

    // Attach (a stream), then the next event is remembered.
    let mut c = Feed::open(&r).await;
    c.until(1).await;
    r.proxy.on_dispatch(message(7));
    c.until(3).await;

    let events = command(&r, json!({"cmd": "events"})).await;
    assert_eq!(events["result"][0], "dispatch MESSAGE_CREATE");

    let rep = command(
        &r,
        json!({"cmd": "replay", "event": "message_create", "plugin": "probe"}),
    )
    .await;
    assert_eq!(rep["ok"], true, "{rep}");
    assert_eq!(rep["result"]["verdict"], "pass");
    assert_eq!(rep["result"]["event"], "MESSAGE_CREATE");
    // The replay shows in the stream like any run.
    let more = c.until(5).await;
    assert_eq!(more[3]["message"], "got MESSAGE_CREATE", "{more:?}");
    assert_eq!(more[4]["verdict"], "pass");

    let settings = command(&r, json!({"cmd": "settings", "plugin": "probe"})).await;
    assert_eq!(settings["result"]["enabled"], true);
    let kv = command(&r, json!({"cmd": "kv", "plugin": "probe"})).await;
    assert_eq!(kv["result"]["seen"], 1);

    // A plugin that is off is not replayed into.
    command(&r, json!({"cmd": "toggle", "plugin": "probe"})).await;
    let off = command(
        &r,
        json!({"cmd": "replay", "event": "MESSAGE_CREATE", "plugin": "probe"}),
    )
    .await;
    assert_eq!(off["ok"], false);
    assert!(
        off["error"].as_str().unwrap().contains("did not run"),
        "{off}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_paths_and_methods_are_not_served() {
    let r = rig("paths").await;
    for (m, p, want) in [
        ("GET", "/__phoecord/dev/nothing", 404),
        ("POST", "/__phoecord/dev/stream", 405),
        ("GET", "/__phoecord/dev/command", 405),
    ] {
        let (code, _) = request(r.addr, m, p, Some(&r.token), "").await;
        assert_eq!(code, want, "{m} {p}");
    }
}
