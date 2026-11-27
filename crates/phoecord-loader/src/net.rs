//! The in-process [`Network`] behind `Host::rest`, `Host::rest_files`, `Host::fetch` and
//! `Host::fetch_with` when PhoeCord runs inside the client (task 2.2.7).
//!
//! The rules are the proxy's, from `phoecord_transport::net_rules`: the session from the client's
//! own Discord API requests (`authorization` required, the `KEPT` headers, merged with an earlier
//! session of the same token, dropped after `SESSION_TTL` and on a 401 from Discord), Discord's
//! rate limits, plain routes only, `fetch` over HTTPS to a host on the plugin's list without any
//! credentials, the size limits and the timeout. Only the HTTP client differs: a blocking `ureq`
//! agent (rustls, no redirects, no environment proxy, no compression).
//!
//! The `send` detour feeds the session ([`LoaderNet::observe`]) with the headers of a client
//! request to the Discord API, but only when [`LoaderNet::wants_session`] says so (no session yet,
//! or the last refresh is older than [`REFRESH`]), so headers are not read on every request.
//!
//! An upstream proxy (`[network] upstream` in `config.toml`) is not supported in-process yet: a
//! call with one is refused instead of going out directly.
//!
//! [`Network`]: phoecord_transport::host::Network

use crate::hooks::abi::Header;
use phoecord_core::{FilePart, HostResponse, Val};
use phoecord_transport::host::Network;
use phoecord_transport::net_rules::{self, MAX_BODY, NetState, TIMEOUT, header_text};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use ureq::http;

/// How old the session may get before the detour reads the client's headers again.
pub const REFRESH: Duration = Duration::from_secs(60);

/// The refusal of a call that names an upstream proxy.
pub const NO_UPSTREAM: &str = "an upstream proxy is not supported in-process yet";

/// Where calls really go.
enum Target {
    Live,
    /// Plain HTTP to a loopback fake server (tests only).
    #[cfg_attr(not(test), allow(dead_code))]
    Test(SocketAddr),
}

pub struct LoaderNet {
    state: NetState,
    agent: ureq::Agent,
    target: Target,
}

impl Default for LoaderNet {
    fn default() -> LoaderNet {
        LoaderNet::new()
    }
}

/// `https://discord.com/api/v9/x?y` → (`discord.com`, `/api/v9/x?y`) for a Discord API URL.
fn api_url(url: &str) -> Option<(String, &str)> {
    let rest = url.strip_prefix("https://")?;
    let i = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(i);
    let host = match authority.rsplit_once(':') {
        Some((h, "443")) => h,
        Some(_) => return None,
        None => authority,
    }
    .to_ascii_lowercase();
    (net_rules::is_api_host(&host) && path.starts_with("/api/")).then_some((host, path))
}

impl LoaderNet {
    pub fn new() -> LoaderNet {
        LoaderNet::with_target(Target::Live)
    }

    #[cfg(test)]
    fn for_test(addr: SocketAddr) -> LoaderNet {
        LoaderNet::with_target(Target::Test(addr))
    }

    fn with_target(target: Target) -> LoaderNet {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .user_agent(ureq::config::AutoHeaderValue::None)
            .accept(ureq::config::AutoHeaderValue::None)
            .accept_encoding("identity")
            .build();
        LoaderNet {
            state: NetState::default(),
            agent: config.into(),
            target,
        }
    }

    /// Whether the client request to `url` should be read for its headers now: a Discord API URL,
    /// and no session yet or the last refresh older than [`REFRESH`].
    pub fn wants_session(&self, url: &str) -> bool {
        self.wants_session_at(url, Instant::now())
    }

    fn wants_session_at(&self, url: &str, now: Instant) -> bool {
        api_url(url).is_some() && self.state.sessions.age_at(now).is_none_or(|a| a > REFRESH)
    }

    /// The session from one of the client's own requests: `url` as the client sent it, `headers`
    /// as `abi::read_headers` read them. Other hosts and paths, and requests without
    /// `authorization`, change nothing.
    pub fn observe(&self, url: &str, headers: &[Header]) {
        let Some((host, path)) = api_url(url) else {
            return;
        };
        let lookup = |n: &str| {
            headers
                .iter()
                .find(|h| h.name == n)
                .and_then(|h| header_text(&h.value))
                .map(str::to_string)
        };
        self.state.sessions.observe(&host, path, &lookup);
    }

    fn url(&self, host: &str, path: &str) -> String {
        match &self.target {
            Target::Live => format!("https://{host}{path}"),
            Target::Test(addr) => format!("http://{addr}{path}"),
        }
    }

    fn rest_raw(
        &self,
        method: &str,
        route: &str,
        body: Option<(Vec<u8>, String)>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        no_upstream(upstream)?;
        let call = self.state.begin_rest(method, route)?;
        let mut headers: Vec<(String, String)> = call
            .headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect();
        headers.push(("authorization".into(), call.authorization.clone()));
        let (status, h, b) = self.run(
            call.method,
            &self.url(&call.host, &call.path),
            headers,
            body,
        )?;
        let lookup = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
        self.state.end_rest(&call, status, &lookup, &b);
        Ok(net_rules::response(status, &lookup, &b))
    }

    fn run(
        &self,
        method: &'static str,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<(Vec<u8>, String)>,
    ) -> Result<(u16, http::HeaderMap, Vec<u8>), String> {
        let mut req = http::Request::builder().method(method).uri(url);
        for (n, v) in &headers {
            let mut value = http::HeaderValue::from_str(v)
                .map_err(|_| format!("bad value for header {n:?}"))?;
            if n == "authorization" {
                value.set_sensitive(true);
            }
            req = req.header(n.as_str(), value);
        }
        let sent = match body {
            Some((bytes, content_type)) => {
                let req = req
                    .header("content-type", content_type)
                    .body(bytes)
                    .map_err(|e| format!("bad request: {e}"))?;
                self.agent.run(req)
            }
            None => {
                let req = req.body(()).map_err(|e| format!("bad request: {e}"))?;
                self.agent.run(req)
            }
        };
        let mut resp = sent.map_err(describe)?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY as u64)
            .read_to_vec()
            .map_err(describe)?;
        Ok((status, resp.headers().clone(), body))
    }
}

/// A `ureq` error for the plugin, without anything from the request's headers.
fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "the request timed out".into(),
        ureq::Error::BodyExceedsLimit(_) => {
            format!("the response is larger than {MAX_BODY} bytes")
        }
        other => format!("the request failed: {other}"),
    }
}

fn no_upstream(upstream: Option<&str>) -> Result<(), String> {
    match upstream.map(str::trim) {
        Some(u) if !u.is_empty() => Err(NO_UPSTREAM.into()),
        _ => Ok(()),
    }
}

impl Network for LoaderNet {
    fn signed_in(&self) -> bool {
        self.state.sessions.signed_in()
    }

    fn rest(
        &self,
        method: &str,
        route: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        let json = net_rules::encode_body(body)?;
        self.rest_raw(
            method,
            route,
            json.map(|b| (b, "application/json".to_string())),
            upstream,
        )
    }

    fn rest_files(
        &self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        let body = net_rules::multipart(payload, files, &net_rules::new_boundary())?;
        self.rest_raw(method, route, Some(body), upstream)
    }

    fn fetch(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        self.fetch_with(allowed, method, url, &[], body, upstream)
    }

    fn fetch_with(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        no_upstream(upstream)?;
        let method = net_rules::check_method(method)?;
        let target = net_rules::fetch_target(allowed, url)?;
        let headers = net_rules::fetch_headers(headers)?;
        let json = net_rules::encode_body(body)?;
        let (status, h, b) = self.run(
            method,
            &self.url(&target.host, &target.path),
            headers,
            json.map(|b| (b, "application/json".to_string())),
        )?;
        let lookup = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
        Ok(net_rules::response(status, &lookup, &b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// Request line and headers (lowercase names) the fake server saw.
    type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

    /// A one-request-per-connection HTTP/1.1 server on loopback that answers `answer(line)`.
    fn fake(answer: fn(&str) -> (u16, &'static str, &'static str)) -> (SocketAddr, Seen) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let seen: Seen = Arc::default();
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
                // Read the whole body before answering: closing with unread data resets the
                // connection before the client sees the answer.
                let len: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let head = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map_or(0, |i| i + 4);
                let mut have = buf.len().saturating_sub(head);
                while have < len {
                    match s.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => have += n,
                    }
                }
                log.lock().unwrap().push((line.clone(), headers));
                let (status, extra, body) = answer(&line);
                let out = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(out.as_bytes());
            }
        });
        (addr, seen)
    }

    fn header(seen: &Seen, i: usize, n: &str) -> Option<String> {
        seen.lock().unwrap()[i]
            .1
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
    }

    fn h(name: &str, value: &str) -> Header {
        Header {
            name: name.into(),
            value: value.as_bytes().to_vec(),
            sensitive: name == "authorization" || name == "cookie",
        }
    }

    fn client(auth: &str) -> Vec<Header> {
        vec![
            h("authorization", auth),
            h("user-agent", "TestClient/1.0"),
            h("x-super-properties", "c3VwZXI="),
            h("cookie", "secret"),
        ]
    }

    const API: &str = "https://discord.com/api/v9/channels/1/messages";

    #[test]
    fn only_discord_api_requests_with_authorization_sign_in() {
        let net = LoaderNet::new();
        assert!(net.wants_session(API));
        assert!(!net.wants_session("https://cdn.discordapp.com/api/x"));
        assert!(!net.wants_session("https://discord.com/assets/x.js"));
        assert!(!net.wants_session("http://discord.com/api/v9/x"));
        net.observe("https://evil.example/api/v9/x", &client("tok"));
        net.observe("https://discord.com/other", &client("tok"));
        net.observe("https://discord.com:8443/api/v9/x", &client("tok"));
        net.observe(API, &client("tok")[1..]);
        assert!(!net.signed_in());
        let mut bad = client("tok");
        bad[0].value = vec![0xff, 0];
        net.observe(API, &bad);
        assert!(
            !net.signed_in(),
            "a value that is not header text is no token"
        );
        net.observe(API, &client("tok"));
        assert!(net.signed_in());
    }

    #[test]
    fn the_session_is_refreshed_only_now_and_then_and_expires() {
        let net = LoaderNet::new();
        net.observe(API, &client("tok"));
        let now = Instant::now();
        assert!(!net.wants_session_at(API, now));
        assert!(net.wants_session_at(API, now + REFRESH + Duration::from_secs(1)));
        let late = now + net_rules::SESSION_TTL + Duration::from_secs(1);
        assert!(net.wants_session_at(API, late));
        assert!(net.state.sessions.current_at(late).is_none());
        assert!(!net.signed_in());
    }

    #[test]
    fn calls_are_refused_without_a_session_or_with_an_upstream() {
        let (addr, seen) = fake(|_| (200, "", "{}"));
        let net = LoaderNet::for_test(addr);
        let e = net.rest("GET", "/users/@me", None, None).unwrap_err();
        assert_eq!(e, net_rules::NOT_SIGNED_IN);
        net.observe(API, &client("tok"));
        for up in ["socks5://127.0.0.1:1080", "http://proxy:8080"] {
            let e = net.rest("GET", "/users/@me", None, Some(up)).unwrap_err();
            assert_eq!(e, NO_UPSTREAM);
            let list = ["api.example.com".to_string()];
            let e = net
                .fetch(&list, "GET", "https://api.example.com/x", None, Some(up))
                .unwrap_err();
            assert_eq!(e, NO_UPSTREAM);
        }
        assert!(net.rest("GET", "/users/@me", None, Some("  ")).is_ok());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "only the call without upstream left"
        );
    }

    #[test]
    fn rest_goes_out_as_the_client_and_a_401_signs_out() {
        let (addr, seen) = fake(|line| {
            if line.contains("/gone") {
                (401, "", r#"{"message":"401: Unauthorized"}"#)
            } else {
                (200, "x-ratelimit-remaining: 4\r\n", r#"{"id":"1"}"#)
            }
        });
        let net = LoaderNet::for_test(addr);
        net.observe(API, &client("tok"));
        // A later request without the other headers keeps them for the same token.
        net.observe(API, &[h("authorization", "tok")]);
        let r = net.rest("get", "/users/@me?x=1", None, None).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body.as_ref().unwrap().str_at(&["id"]), Some("1"));
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "x-ratelimit-remaining" && v == "4")
        );
        assert_eq!(
            seen.lock().unwrap()[0].0,
            "GET /api/v9/users/@me?x=1 HTTP/1.1"
        );
        assert_eq!(header(&seen, 0, "authorization").as_deref(), Some("tok"));
        assert_eq!(
            header(&seen, 0, "user-agent").as_deref(),
            Some("TestClient/1.0")
        );
        assert_eq!(
            header(&seen, 0, "x-super-properties").as_deref(),
            Some("c3VwZXI=")
        );
        assert_eq!(
            header(&seen, 0, "cookie"),
            None,
            "cookies are never forwarded"
        );
        let files = [FilePart {
            filename: "a.gif".into(),
            content_type: "image/gif".into(),
            data: b"GIF89a".to_vec(),
        }];
        let r = net
            .rest_files(
                "POST",
                "/channels/9/messages",
                Some(&Val::map()),
                &files,
                None,
            )
            .unwrap();
        assert_eq!(r.status, 200);
        assert!(
            header(&seen, 1, "content-type")
                .is_some_and(|c| c.starts_with("multipart/form-data; boundary="))
        );
        assert!(
            net.rest("GET", "https://evil.example/x", None, None)
                .is_err()
        );
        assert_eq!(net.rest("GET", "/gone", None, None).unwrap().status, 401);
        assert!(!net.signed_in(), "a 401 from Discord drops the session");
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn rate_limits_close_the_bucket() {
        let (addr, seen) = fake(|_| (429, "retry-after: 30\r\n", r#"{"retry_after":30.0}"#));
        let net = LoaderNet::for_test(addr);
        net.observe(API, &client("tok"));
        assert_eq!(
            net.rest("GET", "/channels/1/messages", None, None)
                .unwrap()
                .status,
            429
        );
        let e = net
            .rest("GET", "/channels/1/messages", None, None)
            .unwrap_err();
        assert!(e.contains("rate limited by Discord"), "{e}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn fetch_goes_only_to_listed_https_hosts_without_credentials() {
        let (addr, seen) = fake(|_| (200, "", r#"{"ok":true}"#));
        let net = LoaderNet::for_test(addr);
        net.observe(API, &client("tok"));
        let list = ["api.example.com".to_string()];
        for bad in [
            "http://api.example.com/x",
            "https://api.example.com:8443/x",
            "https://other.example/x",
            "https://discord.com/api/v9/users/@me",
        ] {
            assert!(net.fetch(&list, "GET", bad, None, None).is_err(), "{bad}");
        }
        assert!(
            net.fetch(&[], "GET", "https://api.example.com/x", None, None)
                .is_err()
        );
        let e = net
            .fetch_with(
                &list,
                "GET",
                "https://api.example.com/x",
                &[("Cookie", "x")],
                None,
                None,
            )
            .unwrap_err();
        assert!(e.contains("may not be set"), "{e}");
        assert!(
            seen.lock().unwrap().is_empty(),
            "nothing refused reached the server"
        );
        let r = net
            .fetch_with(
                &list,
                "GET",
                "https://API.example.com/x?q=1",
                &[("X-Quest", "7")],
                None,
                None,
            )
            .unwrap();
        assert_eq!(r.body.unwrap().get("ok"), Some(&Val::Bool(true)));
        assert_eq!(seen.lock().unwrap()[0].0, "GET /x?q=1 HTTP/1.1");
        assert_eq!(header(&seen, 0, "x-quest").as_deref(), Some("7"));
        assert_eq!(
            header(&seen, 0, "user-agent").as_deref(),
            Some(net_rules::FETCH_USER_AGENT)
        );
        for n in ["authorization", "cookie", "x-super-properties"] {
            assert_eq!(
                header(&seen, 0, n),
                None,
                "{n} must not reach outside hosts"
            );
        }
    }
}
