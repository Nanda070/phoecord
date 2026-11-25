//! `Host::rest` and `Host::fetch` against fake servers on loopback. No network, no token.

use hyper::header::{HeaderMap, HeaderValue};
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};
use phoecord_plugin_runtime::Chain;
use phoecord_proxy::config::Config;
use phoecord_proxy::host::{ProxyHost, Shared};
use phoecord_proxy::hostnet::{Net, Target};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

/// What the fake server saw: request line and headers (lower-cased names).
#[derive(Clone, Debug)]
struct Seen {
    line: String,
    headers: Vec<(String, String)>,
}

impl Seen {
    fn header(&self, n: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.as_str())
    }
}

type Reply = (u16, Vec<(String, String)>, Vec<u8>);
type Answer = Box<dyn Fn(&str) -> Reply + Send + Sync>;

/// A one-request-per-connection HTTP/1.1 server on a random loopback port.
fn fake(answer: Answer) -> (SocketAddr, Arc<Mutex<Vec<Seen>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut s) = conn else { continue };
            let mut buf = Vec::new();
            let mut b = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&b[..n]),
                }
            }
            let text = String::from_utf8_lossy(&buf).to_string();
            let mut lines = text.split("\r\n");
            let line = lines.next().unwrap_or("").to_string();
            let headers: Vec<(String, String)> = lines
                .take_while(|l| !l.is_empty())
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .collect();
            log.lock().unwrap().push(Seen {
                line: line.clone(),
                headers,
            });
            let (status, hdrs, body) = answer(&line);
            let mut out = format!(
                "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                body.len()
            );
            for (k, v) in hdrs {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str("\r\n");
            let _ = s.write_all(out.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    (addr, seen)
}

fn json(status: u16, body: &str) -> Reply {
    (
        status,
        vec![("content-type".into(), "application/json".into())],
        body.as_bytes().to_vec(),
    )
}

/// The host state plus the proxy's own [`Net`] behind it (what the proxy supplies to the engine).
struct Rig {
    shared: Shared,
    net: Arc<Net>,
}

impl std::ops::Deref for Rig {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.shared
    }
}

fn shared(addr: SocketAddr) -> Rig {
    let dir = std::env::temp_dir().join(format!("phoecord-test-hostnet-{}", addr.port()));
    let net = Arc::new(Net::new());
    net.set_test_target(Target::Test { addr, tls: None });
    Rig {
        shared: Shared::with_network(dir, net.clone()),
        net,
    }
}

fn client_headers(auth: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("authorization", HeaderValue::from_str(auth).unwrap());
    h.insert("user-agent", HeaderValue::from_static("TestClient/1.0"));
    h.insert("x-super-properties", HeaderValue::from_static("c3VwZXI="));
    h.insert("cookie", HeaderValue::from_static("__dcfduid=secret"));
    h
}

fn sign_in(s: &Rig, auth: &str) {
    s.net.observe(
        "discord.com",
        "/api/v9/channels/1/messages",
        &client_headers(auth),
    );
}

#[test]
fn rest_is_refused_before_the_client_signed_in() {
    let (addr, seen) = fake(Box::new(|_| json(200, "{}")));
    let s = shared(addr);
    let cfg = Config::default();
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("p");
    let e = h.rest("GET", "/users/@me", None).unwrap_err();
    assert!(e.contains("not signed in yet"), "{e}");
    // A request without Authorization, or for another host or path, does not sign anyone in.
    let mut hm = client_headers("tok");
    hm.remove("authorization");
    s.net.observe("discord.com", "/api/v9/x", &hm);
    s.net
        .observe("evil.example", "/api/v9/x", &client_headers("tok"));
    s.net
        .observe("discord.com", "/other", &client_headers("tok"));
    assert!(!s.net.signed_in());
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn rest_reuses_the_clients_headers_and_follows_changes() {
    let (addr, seen) = fake(Box::new(|_| json(200, r#"{"id":"1","username":"me"}"#)));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok-one");
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("a");
    let r = h.rest("get", "/users/@me?with=1", None).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body.as_ref().unwrap().str_at(&["username"]), Some("me"));
    assert!(r.headers.iter().any(|(k, _)| k == "content-type"));
    {
        let v = seen.lock().unwrap();
        let q = &v[0];
        assert_eq!(q.line, "GET /api/v9/users/@me?with=1 HTTP/1.1");
        assert_eq!(q.header("authorization"), Some("tok-one"));
        assert_eq!(q.header("user-agent"), Some("TestClient/1.0"));
        assert_eq!(q.header("x-super-properties"), Some("c3VwZXI="));
        assert_eq!(q.header("host"), Some("discord.com"));
        assert_eq!(q.header("cookie"), None, "cookies are never forwarded");
    }
    // The client signs in as someone else: the next call uses the new value.
    sign_in(&s, "tok-two");
    h.begin_plugin("b");
    h.rest("POST", "/channels/9/messages", Some(&Val::map()))
        .unwrap();
    {
        let v = seen.lock().unwrap();
        assert_eq!(v[1].header("authorization"), Some("tok-two"));
        assert_eq!(v[1].header("content-type"), Some("application/json"));
    }
    // A cleared session is gone.
    s.net.clear();
    h.begin_plugin("c");
    let e = h.rest("GET", "/users/@me", None).unwrap_err();
    assert!(e.contains("not signed in"), "{e}");
}

#[test]
fn rest_only_talks_to_the_clients_discord_host() {
    let (addr, seen) = fake(Box::new(|_| json(200, "{}")));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    for (i, bad) in [
        "https://evil.example/api/v9/users/@me",
        "//evil.example/x",
        "users/@me",
        "/channels/../../x",
    ]
    .iter()
    .enumerate()
    {
        h.begin_plugin(&format!("p{i}"));
        let e = h.rest("GET", bad, None).unwrap_err();
        assert!(e.contains("bad route"), "{bad}: {e}");
    }
    h.begin_plugin("m");
    let e = h.rest("TRACE", "/x", None).unwrap_err();
    assert!(e.contains("not allowed"), "{e}");
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing reached the server"
    );
}

#[test]
fn discord_rate_limits_are_respected() {
    let (addr, seen) = fake(Box::new(|line| {
        if line.contains("/channels/1/") {
            (
                429,
                vec![
                    ("retry-after".into(), "30".into()),
                    ("content-type".into(), "application/json".into()),
                ],
                br#"{"message":"slow down","retry_after":30.0}"#.to_vec(),
            )
        } else if line.contains("/guilds/2/") {
            (
                200,
                vec![
                    ("x-ratelimit-remaining".into(), "0".into()),
                    ("x-ratelimit-reset-after".into(), "30".into()),
                ],
                b"{}".to_vec(),
            )
        } else {
            json(200, "{}")
        }
    }));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    // A 429 is handed to the plugin; the bucket is then closed for everyone.
    h.begin_plugin("a");
    let r = h.rest("GET", "/channels/1/messages", None).unwrap();
    assert_eq!(r.status, 429);
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "retry-after" && v == "30")
    );
    h.begin_plugin("b");
    let e = h.rest("GET", "/channels/1/messages", None).unwrap_err();
    assert!(e.contains("rate limited by Discord"), "{e}");
    // Another channel is another bucket.
    h.begin_plugin("c");
    assert_eq!(
        h.rest("GET", "/channels/5/messages", None).unwrap().status,
        200
    );
    // `remaining: 0` closes the bucket without a 429.
    h.begin_plugin("d");
    assert_eq!(
        h.rest("GET", "/guilds/2/members", None).unwrap().status,
        200
    );
    h.begin_plugin("e");
    let e = h.rest("GET", "/guilds/2/members", None).unwrap_err();
    assert!(e.contains("rate limited by Discord"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 3, "blocked calls never left");
}

#[test]
fn per_plugin_and_global_caps_apply() {
    let (addr, _) = fake(Box::new(|_| json(200, "{}")));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("solo");
    // A burst of five is fine; the sixth within 5 s is refused with the wait.
    for _ in 0..5 {
        h.rest("GET", "/users/@me", None).unwrap();
    }
    let e = h.rest("GET", "/users/@me", None).unwrap_err();
    assert!(e.contains("per plugin") && e.contains("retry in"), "{e}");
    // Six more plugins in the same second run into the global cap of five.
    let mut errs = Vec::new();
    for i in 0..6 {
        h.begin_plugin(&format!("g{i}"));
        if let Err(e) = h.rest("GET", "/users/@me", None) {
            errs.push(e);
        }
    }
    assert!(
        !errs.is_empty() && errs.iter().all(|e| e.contains("in total")),
        "{errs:?}"
    );
}

#[test]
fn fetch_goes_only_to_listed_hosts_and_never_carries_discord_credentials() {
    let (addr, seen) = fake(Box::new(|_| json(200, r#"{"ok":true}"#)));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("p");
    // No list: nothing is allowed.
    let e = h
        .fetch("GET", "https://api.example.com/x", None)
        .unwrap_err();
    assert!(e.contains("not in this plugin's `fetch` list"), "{e}");
    h.set_fetch_hosts(&["api.example.com".to_string()]);
    let r = h
        .fetch("GET", "https://API.example.com/x?q=1", None)
        .unwrap();
    assert_eq!(r.body.unwrap().get("ok"), Some(&Val::Bool(true)));
    for bad in [
        "https://discord.com/api/v9/users/@me",
        "https://api.example.com.evil.example/x",
        "http://api.example.com/x",
        "https://api.example.com@evil.example/x",
        "https://api.example.com:8443/x",
    ] {
        assert!(h.fetch("GET", bad, None).is_err(), "{bad}");
    }
    {
        let v = seen.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, "GET /x?q=1 HTTP/1.1");
        assert_eq!(v[0].header("host"), Some("api.example.com"));
        for n in ["authorization", "cookie", "x-super-properties"] {
            assert_eq!(v[0].header(n), None, "{n} must not reach outside hosts");
        }
    }
    // The list belongs to the plugin: the next one starts empty.
    h.begin_plugin("q");
    assert!(h.fetch("GET", "https://api.example.com/x", None).is_err());
}

#[test]
fn fetch_with_sends_the_callers_headers_to_pattern_hosts_and_refuses_sensitive_ones() {
    let (addr, seen) = fake(Box::new(|_| json(200, r#"{"token":"t"}"#)));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("p");
    h.set_fetch_hosts(&["*.discordsays.com".to_string()]);
    let headers = [
        ("Content-Type", "application/json"),
        ("X-Auth-Token", ""),
        ("X-Discord-Quest-ID", "42"),
        ("Referer", "https://123.discordsays.com/?instance_id=x"),
    ];
    let body = Val::from_json(serde_json::json!({"code": "abc"}));
    let r = h
        .fetch_with(
            "POST",
            "https://123.discordsays.com/.proxy/acf/authorize",
            &headers,
            Some(&body),
        )
        .unwrap();
    assert_eq!(r.body.unwrap().str_at(&["token"]), Some("t"));
    {
        let v = seen.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, "POST /.proxy/acf/authorize HTTP/1.1");
        assert_eq!(v[0].header("host"), Some("123.discordsays.com"));
        assert_eq!(v[0].header("x-discord-quest-id"), Some("42"));
        assert_eq!(v[0].header("x-auth-token"), Some(""));
        assert_eq!(
            v[0].header("referer"),
            Some("https://123.discordsays.com/?instance_id=x")
        );
        assert_eq!(v[0].header("content-type"), Some("application/json"));
        for n in ["authorization", "cookie", "x-super-properties"] {
            assert_eq!(v[0].header(n), None, "{n} must not reach outside hosts");
        }
    }
    // The pattern is one label deep only.
    for bad in [
        "https://discordsays.com/x",
        "https://a.b.discordsays.com/x",
        "https://123.discordsays.com.evil.example/x",
    ] {
        let e = h.fetch_with("GET", bad, &[], None).unwrap_err();
        assert!(e.contains("fetch` list"), "{bad}: {e}");
    }
    // Credential and hop-by-hop headers never leave, and nothing is sent.
    for name in [
        "Authorization",
        "cookie",
        "Host",
        "Proxy-Authorization",
        "Connection",
        "Content-Length",
        "Transfer-Encoding",
        "TE",
        "Upgrade",
    ] {
        let e = h
            .fetch_with("GET", "https://1.discordsays.com/x", &[(name, "x")], None)
            .unwrap_err();
        assert!(e.contains("may not be set"), "{name}: {e}");
    }
    let e = h
        .fetch_with(
            "GET",
            "https://1.discordsays.com/x",
            &[("X-Bad", "a\r\nb")],
            None,
        )
        .unwrap_err();
    assert!(e.contains("bad value"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    // Plain `fetch` still works on the pattern.
    assert!(h.fetch("GET", "https://9.discordsays.com/x", None).is_ok());
}

#[test]
fn fetch_text_size_cap_and_redirects() {
    let (addr, _) = fake(Box::new(|line| {
        if line.contains("/big") {
            (200, vec![], vec![b'a'; 5 * 1024 * 1024 + 1])
        } else if line.contains("/redirect") {
            (
                302,
                vec![("location".into(), "https://evil.example/".into())],
                vec![],
            )
        } else {
            (
                200,
                vec![("content-type".into(), "text/plain".into())],
                b"hello".to_vec(),
            )
        }
    }));
    let s = shared(addr);
    let cfg = Config::default();
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("p");
    h.set_fetch_hosts(&["api.example.com".to_string()]);
    let r = h.fetch("GET", "https://api.example.com/t", None).unwrap();
    assert_eq!((r.body, r.text.as_deref()), (None, Some("hello")));
    let e = h
        .fetch("GET", "https://api.example.com/big", None)
        .unwrap_err();
    assert!(e.contains("larger than"), "{e}");
    let r = h
        .fetch("GET", "https://api.example.com/redirect", None)
        .unwrap();
    assert_eq!(r.status, 302, "redirects are not followed");
    assert!(r.headers.iter().any(|(k, _)| k == "location"));
}

#[test]
fn fetch_returns_binary_bodies_as_bytes() {
    let gif: Vec<u8> = vec![
        b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0xff, 0,
    ];
    let body = gif.clone();
    let (addr, _) = fake(Box::new(move |_| {
        (
            200,
            vec![("content-type".into(), "image/gif".into())],
            body.clone(),
        )
    }));
    let s = shared(addr);
    let cfg = Config::default();
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("p");
    h.set_fetch_hosts(&["cdn.example.com".to_string()]);
    let r = h
        .fetch("GET", "https://cdn.example.com/frame.gif", None)
        .unwrap();
    assert_eq!(r.body, None);
    assert_eq!(r.bytes.as_deref(), Some(&gif[..]), "raw bytes survive");
}

#[test]
fn rest_files_sends_multipart_with_the_clients_session() {
    let (addr, seen) = fake(Box::new(|_| json(200, r#"{"id":"5"}"#)));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let mut h = ProxyHost::new(&s, &cfg);
    h.begin_plugin("petpet");
    let files = [phoecord_core::FilePart {
        filename: "petpet.gif".into(),
        content_type: "image/gif".into(),
        data: b"GIF89a".to_vec(),
    }];
    let r = h
        .rest_files("POST", "/channels/9/messages", Some(&Val::map()), &files)
        .unwrap();
    assert_eq!(r.status, 200);
    let v = seen.lock().unwrap();
    assert_eq!(v[0].line, "POST /api/v9/channels/9/messages HTTP/1.1");
    assert_eq!(v[0].header("authorization"), Some("tok"));
    assert!(
        v[0].header("content-type")
            .is_some_and(|c| c.starts_with("multipart/form-data; boundary=")),
        "{:?}",
        v[0].header("content-type")
    );
    drop(v);
    // Same caps as rest: the sixth call from the same plugin within 5 s is refused.
    for _ in 0..4 {
        h.rest_files("POST", "/channels/9/messages", None, &files)
            .unwrap();
    }
    let e = h
        .rest_files("POST", "/channels/9/messages", None, &files)
        .unwrap_err();
    assert!(e.contains("rate limited") && e.contains("retry in"), "{e}");
}

/// A plugin that calls both from inside the chain.
struct Probe {
    m: Manifest,
    out: Arc<Mutex<Vec<Result<u16, String>>>>,
}

impl Plugin for Probe {
    fn manifest(&self) -> &Manifest {
        &self.m
    }
    fn wants(&self, e: &EventKind) -> bool {
        matches!(
            e,
            EventKind::RestRequest {
                route: "/probe",
                ..
            }
        )
    }
    fn on_event(&self, host: &mut dyn Host, _e: Event<'_>) -> Verdict {
        let mut o = self.out.lock().unwrap();
        o.push(host.rest("GET", "/users/@me", None).map(|r| r.status));
        o.push(
            host.fetch("GET", "https://api.example.com/x", None)
                .map(|r| r.status),
        );
        o.push(
            host.fetch("GET", "https://other.example/x", None)
                .map(|r| r.status),
        );
        Verdict::Pass
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plugin_in_the_chain_can_call_rest_and_fetch() {
    let (addr, seen) = fake(Box::new(|_| json(200, "{}")));
    let s = shared(addr);
    let cfg = Config::default();
    sign_in(&s, "tok");
    let out = Arc::new(Mutex::new(Vec::new()));
    let chain = Chain::new(vec![Box::new(Probe {
        m: Manifest {
            id: "probe".into(),
            name: "Probe".into(),
            version: "0".into(),
            description: String::new(),
            authors: vec![],
            vencord: None,
            layers: vec![Layer::Network],
            api: API_VERSION,
            fetch: vec!["api.example.com".into()],
        },
        out: out.clone(),
    })]);
    chain.set_enabled(["probe".to_string()]);
    let mut host = ProxyHost::new(&s, &cfg);
    // The chain runs on a runtime worker, as in the proxy; the call must not stall the runtime.
    chain.rest_request(&mut host, "GET", "/probe", None);
    let o = out.lock().unwrap();
    assert_eq!(o[0], Ok(200));
    assert_eq!(o[1], Ok(200));
    assert!(o[2].as_ref().unwrap_err().contains("fetch"), "{:?}", o[2]);
    assert_eq!(seen.lock().unwrap().len(), 2);
}
