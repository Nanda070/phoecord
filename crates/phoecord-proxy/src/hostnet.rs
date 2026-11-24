//! The network side of `Host::rest` and `Host::fetch`.
//!
//! * [`Session`]: the client's own Discord API headers (`Authorization`, `User-Agent`,
//!   `X-Super-Properties`, …), kept in memory only, refreshed from each client request, dropped after
//!   [`SESSION_TTL`] without client traffic, on a 401 from Discord, and on `clear`. They never leave
//!   this module: plugins get a [`HostResponse`] and nothing else.
//! * [`Net`]: runs one HTTPS request at a time for a plugin. The plugin chain is synchronous, so
//!   the request runs on a small runtime of its own and the calling thread waits (with
//!   `block_in_place` when it is a worker of a multi-threaded runtime, so the proxy keeps serving).
//! * Discord's rate limits ([`Limits`]): `X-RateLimit-*` and `Retry-After` are remembered per route
//!   bucket and globally; a call into a blocked bucket returns an error instead of waiting.

use crate::egress;
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use phoecord_core::{HostResponse, Val};
use rustls::pki_types::ServerName;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long the session headers stay after the client's last authenticated API request.
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Largest response body a plugin gets (`rest` and `fetch`).
pub const MAX_BODY: usize = 5 * 1024 * 1024;
/// Largest request body a plugin may send.
pub const MAX_REQUEST: usize = 1024 * 1024;
/// Whole-call timeout, connect to last byte.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// At most this many Discord API calls a second across all plugins.
pub const GLOBAL_PER_SECOND: usize = 5;

/// Client headers that make a call look like the client's own. Cookies are deliberately absent.
const KEPT: &[&str] = &[
    "user-agent",
    "x-super-properties",
    "x-discord-locale",
    "x-discord-timezone",
];
/// Response headers handed to plugins.
const SHOWN: &[&str] = &[
    "content-type",
    "retry-after",
    "link",
    "location",
    "x-ratelimit-limit",
    "x-ratelimit-remaining",
    "x-ratelimit-reset",
    "x-ratelimit-reset-after",
    "x-ratelimit-bucket",
    "x-ratelimit-global",
    "x-ratelimit-scope",
];

fn is_api_host(h: &str) -> bool {
    matches!(
        h,
        "discord.com" | "ptb.discord.com" | "canary.discord.com" | "discordapp.com"
    )
}

/// The client's Discord API session. `Debug` never prints the values.
#[derive(Clone)]
struct Session {
    host: String,
    /// `/api/v9`, as the client used it.
    prefix: String,
    authorization: String,
    headers: Vec<(&'static str, String)>,
    seen: Instant,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({} {} <redacted>)", self.host, self.prefix)
    }
}

/// Where a request really goes. `Test` points at a loopback fake server (tests only).
#[derive(Clone)]
pub enum Target {
    Live,
    Test {
        addr: SocketAddr,
        /// `None`: plain HTTP; `Some`: TLS with this client config.
        tls: Option<Arc<rustls::ClientConfig>>,
    },
}

/// Discord's rate-limit state, from response headers.
#[derive(Default)]
struct Limits {
    global_until: Option<Instant>,
    buckets: HashMap<String, Instant>,
}

/// `GET /channels/123/messages/456` → `GET /channels/123/messages/:id`: channel, guild and webhook
/// ids are Discord's major parameters (own bucket each), other ids share one.
pub fn bucket_key(method: &str, path: &str) -> String {
    let mut out = String::from(method);
    let mut prev = "";
    for seg in path.split('?').next().unwrap_or("").split('/') {
        if seg.is_empty() {
            continue;
        }
        out.push('/');
        let numeric = seg.bytes().all(|b| b.is_ascii_digit());
        if numeric && !matches!(prev, "channels" | "guilds" | "webhooks") {
            out.push_str(":id");
        } else {
            out.push_str(seg);
        }
        prev = seg;
    }
    out
}

pub struct Net {
    session: Mutex<Option<Session>>,
    limits: Mutex<Limits>,
    tls: Arc<rustls::ClientConfig>,
    target: Mutex<Target>,
    rt: OnceLock<tokio::runtime::Runtime>,
}

impl Net {
    pub fn new() -> Net {
        Net {
            session: Mutex::new(None),
            limits: Mutex::new(Limits::default()),
            tls: crate::upstream_tls(None),
            target: Mutex::new(Target::Live),
            rt: OnceLock::new(),
        }
    }

    /// Send calls to a local fake server instead of the internet. **Tests only.**
    pub fn set_test_target(&self, target: Target) {
        *self.target.lock().unwrap() = target;
    }

    /// Remember the client's session headers from one of its own API requests (`host` is the
    /// Discord host it connected to). Requests without `Authorization` or for other hosts or paths
    /// change nothing.
    pub fn observe(&self, host: &str, path: &str, headers: &HeaderMap) {
        let host = host.to_ascii_lowercase();
        let Some(rest) = path.strip_prefix("/api/") else {
            return;
        };
        if !is_api_host(&host) {
            return;
        }
        let version = rest.split('/').next().unwrap_or("");
        let Some(auth) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
        else {
            return;
        };
        let prefix = if version.starts_with('v') && version[1..].bytes().all(|b| b.is_ascii_digit())
        {
            format!("/api/{version}")
        } else {
            "/api/v9".to_string()
        };
        let mut kept = Vec::new();
        for name in KEPT {
            if let Some(v) = headers.get(*name).and_then(|v| v.to_str().ok()) {
                kept.push((*name, v.to_string()));
            }
        }
        let mut s = self.session.lock().unwrap();
        // Keep what an earlier request had when this one omits a header.
        if let Some(old) = s.as_ref().filter(|o| o.authorization == auth) {
            for (name, v) in &old.headers {
                if !kept.iter().any(|(n, _)| n == name) {
                    kept.push((name, v.clone()));
                }
            }
        }
        *s = Some(Session {
            host,
            prefix,
            authorization: auth.to_string(),
            headers: kept,
            seen: Instant::now(),
        });
    }

    /// Forget the session (the client signed out, Discord said 401, …).
    pub fn clear(&self) {
        *self.session.lock().unwrap() = None;
    }

    pub fn signed_in(&self) -> bool {
        self.session_now().is_some()
    }

    fn session_now(&self) -> Option<Session> {
        let mut s = self.session.lock().unwrap();
        if s.as_ref().is_some_and(|x| x.seen.elapsed() > SESSION_TTL) {
            *s = None;
        }
        s.clone()
    }

    fn check_limits(&self, key: &str) -> Result<(), String> {
        let now = Instant::now();
        let mut l = self.limits.lock().unwrap();
        l.buckets.retain(|_, until| *until > now);
        if l.global_until.is_some_and(|u| u <= now) {
            l.global_until = None;
        }
        let wait = l
            .global_until
            .into_iter()
            .chain(l.buckets.get(key).copied())
            .max()
            .map(|u| u.saturating_duration_since(now));
        match wait {
            Some(w) => Err(format!(
                "rate limited by Discord: retry in {:.1}s",
                w.as_secs_f32()
            )),
            None => Ok(()),
        }
    }

    fn learn_limits(&self, key: &str, status: u16, h: &HeaderMap, body: &[u8]) {
        let secs = |name: &str| {
            h.get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let now = Instant::now();
        let mut l = self.limits.lock().unwrap();
        if status == 429 {
            let retry = secs("retry-after")
                .or_else(|| {
                    serde_json::from_slice::<serde_json::Value>(body)
                        .ok()?
                        .get("retry_after")?
                        .as_f64()
                })
                .unwrap_or(1.0)
                .clamp(0.0, 3600.0);
            let until = now + Duration::from_secs_f64(retry);
            if h.get("x-ratelimit-global").is_some() {
                l.global_until = Some(until);
            } else {
                l.buckets.insert(key.to_string(), until);
            }
        } else if h
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim() == "0")
            && let Some(after) = secs("x-ratelimit-reset-after")
        {
            let until = now + Duration::from_secs_f64(after.min(3600.0));
            l.buckets.insert(key.to_string(), until);
        }
    }

    /// A Discord API call as the client's own session.
    pub fn rest(
        &self,
        method: &str,
        path: &str,
        body: Option<&Val>,
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let session = self
            .session_now()
            .ok_or("client not signed in yet: Discord has not made an authenticated API request through PhoeCord")?;
        let method = parse_method(method)?;
        let path = clean_route(path)?;
        let key = bucket_key(method.as_str(), &path);
        self.check_limits(&key)?;
        let json = encode_body(body)?;
        let mut headers = HeaderMap::new();
        for (n, v) in &session.headers {
            headers.insert(
                HeaderName::from_static(n),
                HeaderValue::from_str(v).map_err(|e| e.to_string())?,
            );
        }
        let mut auth = HeaderValue::from_str(&session.authorization).map_err(|e| e.to_string())?;
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        let url = Url {
            host: session.host.clone(),
            port: 443,
            path: format!("{}{}", session.prefix, path),
        };
        let r = self.run(method, url, headers, json, upstream)?;
        self.learn_limits(&key, r.status, &r.raw_headers, &r.raw_body);
        if r.status == 401 {
            // The token is no good any more: stop using it until the client sends a new one.
            self.clear();
        }
        Ok(r.into_response())
    }

    /// HTTPS to an outside host from the plugin's `fetch` list. No Discord credentials, no cookies,
    /// no redirects.
    pub fn fetch(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        body: Option<&Val>,
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let method = parse_method(method)?;
        let url = parse_fetch_url(url)?;
        if !allowed.iter().any(|h| h.eq_ignore_ascii_case(&url.host)) {
            return Err(format!(
                "host {} is not in this plugin's `fetch` list",
                url.host
            ));
        }
        let json = encode_body(body)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_static(concat!("PhoeCord/", env!("CARGO_PKG_VERSION"))),
        );
        Ok(self
            .run(method, url, headers, json, upstream)?
            .into_response())
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, String> {
        if let Some(rt) = self.rt.get() {
            return Ok(rt);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("phoecord-hostnet")
            .enable_all()
            .build()
            .map_err(|e| format!("cannot start the network runtime: {e}"))?;
        Ok(self.rt.get_or_init(|| rt))
    }

    fn run(
        &self,
        method: Method,
        url: Url,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        upstream: Option<egress::Upstream>,
    ) -> Result<Raw, String> {
        let target = self.target.lock().unwrap().clone();
        let tls = self.tls.clone();
        let rt = self.runtime()?;
        let (tx, rx) = std::sync::mpsc::channel();
        rt.spawn(async move {
            let r = tokio::time::timeout(
                TIMEOUT,
                request(target, tls, upstream, method, url, headers, body),
            )
            .await
            .map_err(|_| "the request timed out".to_string())
            .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = tx.send(r);
        });
        let wait = || {
            rx.recv()
                .unwrap_or_else(|_| Err("the request was cancelled".into()))
        };
        // Let the proxy's own runtime keep serving while this thread waits.
        match tokio::runtime::Handle::try_current() {
            Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(wait)
            }
            _ => wait(),
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        // A runtime must not be dropped inside async code; this may be.
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl Default for Net {
    fn default() -> Net {
        Net::new()
    }
}

struct Url {
    host: String,
    port: u16,
    /// Path and query, starting with `/`.
    path: String,
}

struct Raw {
    status: u16,
    raw_headers: HeaderMap,
    raw_body: Bytes,
}

impl Raw {
    fn into_response(self) -> HostResponse {
        let headers = SHOWN
            .iter()
            .filter_map(|n| {
                let v = self.raw_headers.get(*n)?.to_str().ok()?;
                Some((n.to_string(), v.to_string()))
            })
            .collect();
        let (body, text) = match serde_json::from_slice::<serde_json::Value>(&self.raw_body) {
            Ok(j) => (Some(Val::from_json(j)), None),
            Err(_) if self.raw_body.is_empty() => (None, None),
            Err(_) => (
                None,
                Some(String::from_utf8_lossy(&self.raw_body).into_owned()),
            ),
        };
        HostResponse {
            status: self.status,
            body,
            text,
            headers,
        }
    }
}

fn parse_method(m: &str) -> Result<Method, String> {
    match m.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        other => Err(format!("method {other:?} is not allowed")),
    }
}

fn encode_body(body: Option<&Val>) -> Result<Option<Vec<u8>>, String> {
    let Some(v) = body else { return Ok(None) };
    let b = serde_json::to_vec(&v.to_json()).map_err(|e| e.to_string())?;
    if b.len() > MAX_REQUEST {
        return Err(format!("request body over {MAX_REQUEST} bytes"));
    }
    Ok(Some(b))
}

/// A route a plugin gives: `/channels/1/messages?limit=5`, optionally with the `/api/vN` prefix.
/// Absolute URLs, other hosts, `..` and anything that is not a plain path are refused.
pub fn clean_route(route: &str) -> Result<String, String> {
    let bad = |why: &str| Err(format!("bad route {route:?}: {why}"));
    if !route.starts_with('/') || route.starts_with("//") {
        return bad("must be a path starting with a single `/`");
    }
    if route.contains("://")
        || route.contains('\\')
        || route.contains('#')
        || route.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return bad("not a plain path");
    }
    let path = route.split('?').next().unwrap_or("");
    if path
        .split('/')
        .any(|s| s == ".." || s == "." || s.eq_ignore_ascii_case("%2e%2e"))
    {
        return bad("`..` is not allowed in the path");
    }
    let mut r = route;
    if let Some(rest) = route.strip_prefix("/api/v") {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with('/') {
            r = &rest[digits..];
        } else {
            return bad("`/api/` prefix must be `/api/v<N>/`");
        }
    }
    Ok(r.to_string())
}

fn parse_fetch_url(url: &str) -> Result<Url, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("only https:// URLs are allowed")?;
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) || url.contains('\\') {
        return Err("bad URL".into());
    }
    let (auth, path) = match rest.find(['/', '?', '#']) {
        Some(i) => rest.split_at(i),
        None => (rest, "/"),
    };
    if auth.contains('@') {
        return Err("credentials in a URL are not allowed".into());
    }
    let (host, port) = match auth.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| "bad port")?),
        None => (auth, 443),
    };
    if port != 443 {
        return Err("only port 443 is allowed".into());
    }
    if host.is_empty() {
        return Err("no host in the URL".into());
    }
    let path = path.split('#').next().unwrap_or("/");
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(Url {
        host: host.to_ascii_lowercase(),
        port,
        path,
    })
}

async fn request(
    target: Target,
    tls_live: Arc<rustls::ClientConfig>,
    upstream: Option<egress::Upstream>,
    method: Method,
    url: Url,
    mut headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<Raw> {
    let (tcp, tls) = match &target {
        Target::Live => (
            egress::connect(upstream.as_ref(), &url.host, url.port)
                .await
                .with_context(|| format!("connecting to {}", url.host))?,
            Some(tls_live),
        ),
        Target::Test { addr, tls } => (tokio::net::TcpStream::connect(addr).await?, tls.clone()),
    };
    tcp.set_nodelay(true).ok();
    let name = ServerName::try_from(url.host.clone()).map_err(|e| anyhow!("{e}"))?;
    headers.insert(
        header::HOST,
        HeaderValue::from_str(&url.host).map_err(|e| anyhow!("{e}"))?,
    );
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    if body.is_some() {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    let len = body.as_ref().map_or(0, Vec::len);
    let mut b = Request::builder().method(method).uri(&url.path);
    *b.headers_mut().unwrap() = headers;
    if len > 0 || b.method_ref().is_some_and(|m| *m != Method::GET) {
        b = b.header(header::CONTENT_LENGTH, len);
    }
    let req = match body {
        Some(v) => b.body(Full::new(Bytes::from(v)).map_err(|e| match e {}).boxed())?,
        None => b.body(Empty::<Bytes>::new().map_err(|e| match e {}).boxed())?,
    };
    match tls {
        Some(cfg) => {
            let io = tokio_rustls::TlsConnector::from(cfg)
                .connect(name, tcp)
                .await?;
            send(TokioIo::new(io), req).await
        }
        None => send(TokioIo::new(tcp), req).await,
    }
}

type ReqBody = http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>;

async fn send<T>(io: TokioIo<T>, req: Request<ReqBody>) -> Result<Raw>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let resp = sender.send_request(req).await?;
    let (parts, mut body) = resp.into_parts();
    let mut raw = Vec::new();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame?.into_data() {
            if raw.len() + data.len() > MAX_BODY {
                bail!("the response is larger than {MAX_BODY} bytes");
            }
            raw.extend_from_slice(&data);
        }
    }
    Ok(Raw {
        status: parts.status.as_u16(),
        raw_headers: parts.headers,
        raw_body: Bytes::from(raw),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_plain_paths() {
        assert_eq!(clean_route("/users/@me").unwrap(), "/users/@me");
        assert_eq!(clean_route("/api/v9/users/@me").unwrap(), "/users/@me");
        for bad in [
            "https://evil.example/x",
            "//evil.example/x",
            "users/@me",
            "/a/../b",
            "/a\\b",
            "/a b",
            "/a#b",
            "/a://b",
        ] {
            assert!(clean_route(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bucket_keys_keep_major_ids_only() {
        assert_eq!(
            bucket_key("GET", "/channels/5/messages/6?x=1"),
            "GET/channels/5/messages/:id"
        );
        assert_eq!(bucket_key("GET", "/users/7"), "GET/users/:id");
    }

    #[test]
    fn fetch_urls() {
        let u = parse_fetch_url("https://Api.Example.com/a/b?c=1").unwrap();
        assert_eq!(
            (u.host.as_str(), u.path.as_str()),
            ("api.example.com", "/a/b?c=1")
        );
        assert_eq!(parse_fetch_url("https://x.com").unwrap().path, "/");
        assert_eq!(parse_fetch_url("https://x.com?q=1").unwrap().path, "/?q=1");
        for bad in [
            "http://x.com/",
            "https://u:p@x.com/",
            "https://x.com:8443/",
            "https:///a",
            "ftp://x.com",
            "https://x.com/a b",
        ] {
            assert!(parse_fetch_url(bad).is_err(), "{bad}");
        }
    }
}
