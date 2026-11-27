//! The rules behind `Host::rest` and `Host::fetch`, shared by every transport's [`Network`]
//! (the proxy's `hostnet::Net`, the loader's in-process `LoaderNet`). Nothing here does I/O: a
//! transport brings the HTTP client and feeds this module what it saw.
//!
//! * [`Sessions`]: the client's own Discord API headers (`Authorization`, `User-Agent`,
//!   `X-Super-Properties`, …), kept in memory only, refreshed from the client's requests, dropped
//!   after [`SESSION_TTL`] without client traffic, on a 401 from Discord, and on `clear`. They
//!   never reach a plugin: plugins get a [`HostResponse`] and nothing else.
//! * [`RateLimits`]: Discord's `X-RateLimit-*` and `Retry-After`, remembered per route bucket
//!   ([`bucket_key`]) and globally; a call into a blocked bucket is refused instead of waiting.
//! * [`NetState::begin_rest`] / [`NetState::end_rest`]: one Discord API call from the session's
//!   point of view (route checks, limits, the headers to send, what the answer teaches).
//! * `fetch`: [`fetch_target`] (HTTPS on port 443 to a host on the plugin's list only) and
//!   [`fetch_headers`] (the caller's extra headers, credential and hop-by-hop ones refused).
//! * Bodies and answers: [`encode_body`], [`multipart`], [`response`], and the size limits.
//!
//! [`Network`]: crate::host::Network

use phoecord_core::{FilePart, HostResponse, Val};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the session headers stay after the client's last authenticated API request.
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Largest response body a plugin gets (`rest` and `fetch`).
pub const MAX_BODY: usize = 5 * 1024 * 1024;
/// Largest request body a plugin may send.
pub const MAX_REQUEST: usize = 1024 * 1024;
/// Largest multipart upload `Host::rest_files` sends (Discord's own limit for most accounts).
pub const MAX_UPLOAD: usize = 25 * 1024 * 1024;
/// Whole-call timeout, connect to last byte.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// Client headers that make a call look like the client's own. Cookies are deliberately absent.
pub const KEPT: &[&str] = &[
    "user-agent",
    "x-super-properties",
    "x-discord-locale",
    "x-discord-timezone",
];
/// Response headers handed to plugins.
pub const SHOWN: &[&str] = &[
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

/// `User-Agent` of a plugin's `fetch` (never the client's).
pub const FETCH_USER_AGENT: &str = concat!("PhoeCord/", env!("CARGO_PKG_VERSION"));

/// The error of a `rest` call before the client's session is known.
pub const NOT_SIGNED_IN: &str =
    "client not signed in yet: Discord has not made an authenticated API request through PhoeCord";

/// The Discord hosts whose `/api/` requests carry the client's session.
pub fn is_api_host(h: &str) -> bool {
    matches!(
        h,
        "discord.com" | "ptb.discord.com" | "canary.discord.com" | "discordapp.com"
    )
}

/// A header value as text, by the rule of `http`'s `HeaderValue::to_str`: visible ASCII and tabs
/// only; anything else is `None`.
pub fn header_text(v: &[u8]) -> Option<&str> {
    if v.iter().all(|&b| b == b'\t' || (32..127).contains(&b)) {
        std::str::from_utf8(v).ok()
    } else {
        None
    }
}

/// The client's Discord API session. `Debug` never prints the values.
#[derive(Clone)]
pub struct Session {
    /// The Discord host the client talked to, lowercase.
    pub host: String,
    /// `/api/v9`, as the client used it.
    pub prefix: String,
    pub authorization: String,
    /// [`KEPT`] headers the client sent.
    pub headers: Vec<(&'static str, String)>,
    seen: Instant,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({} {} <redacted>)", self.host, self.prefix)
    }
}

/// The one session a transport knows.
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<Option<Session>>,
}

impl Sessions {
    /// Remember the client's session headers from one of its own API requests (`host` is the
    /// Discord host it went to, `path` the request's path). `header` looks a header up by its
    /// lowercase name and gives its text (see [`header_text`]). Requests without
    /// `Authorization`, or for other hosts or paths, change nothing; the result says whether
    /// the session was stored.
    pub fn observe(&self, host: &str, path: &str, header: &dyn Fn(&str) -> Option<String>) -> bool {
        let host = host.to_ascii_lowercase();
        let Some(rest) = path.strip_prefix("/api/") else {
            return false;
        };
        if !is_api_host(&host) {
            return false;
        }
        let version = rest.split('/').next().unwrap_or("");
        let Some(auth) = header("authorization").filter(|v| !v.is_empty()) else {
            return false;
        };
        let prefix = if version.starts_with('v') && version[1..].bytes().all(|b| b.is_ascii_digit())
        {
            format!("/api/{version}")
        } else {
            "/api/v9".to_string()
        };
        let mut kept = Vec::new();
        for name in KEPT {
            if let Some(v) = header(name) {
                kept.push((*name, v));
            }
        }
        let mut s = self.lock();
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
            authorization: auth,
            headers: kept,
            seen: Instant::now(),
        });
        true
    }

    /// Forget the session (the client signed out, Discord said 401, …).
    pub fn clear(&self) {
        *self.lock() = None;
    }

    pub fn signed_in(&self) -> bool {
        self.current().is_some()
    }

    /// The session, unless it expired.
    pub fn current(&self) -> Option<Session> {
        self.current_at(Instant::now())
    }

    /// [`Sessions::current`] as of `now` (a session older than [`SESSION_TTL`] is dropped).
    pub fn current_at(&self, now: Instant) -> Option<Session> {
        let mut s = self.lock();
        if s.as_ref()
            .is_some_and(|x| now.saturating_duration_since(x.seen) > SESSION_TTL)
        {
            *s = None;
        }
        s.clone()
    }

    /// How long ago the session was last refreshed, as of `now`; `None` without one.
    pub fn age_at(&self, now: Instant) -> Option<Duration> {
        self.current_at(now)
            .map(|s| now.saturating_duration_since(s.seen))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Discord's rate-limit state, from response headers.
#[derive(Default)]
pub struct RateLimits {
    inner: Mutex<Limits>,
}

#[derive(Default)]
struct Limits {
    global_until: Option<Instant>,
    buckets: HashMap<String, Instant>,
}

impl RateLimits {
    /// `Err` with the wait when the bucket `key` (or everything) is blocked.
    pub fn check(&self, key: &str) -> Result<(), String> {
        let now = Instant::now();
        let mut l = self.lock();
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

    /// What one answer of bucket `key` says about the limits (`header` as in
    /// [`Sessions::observe`]).
    pub fn learn(
        &self,
        key: &str,
        status: u16,
        header: &dyn Fn(&str) -> Option<String>,
        body: &[u8],
    ) {
        let secs = |name: &str| {
            header(name)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let now = Instant::now();
        let mut l = self.lock();
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
            if header("x-ratelimit-global").is_some() {
                l.global_until = Some(until);
            } else {
                l.buckets.insert(key.to_string(), until);
            }
        } else if header("x-ratelimit-remaining").is_some_and(|v| v.trim() == "0")
            && let Some(after) = secs("x-ratelimit-reset-after")
        {
            let until = now + Duration::from_secs_f64(after.min(3600.0));
            l.buckets.insert(key.to_string(), until);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Limits> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The session and the limits of one transport.
#[derive(Default)]
pub struct NetState {
    pub sessions: Sessions,
    pub limits: RateLimits,
}

/// One Discord API call, checked and ready to send. `Debug` never prints the header values.
pub struct RestCall {
    /// Uppercase.
    pub method: &'static str,
    /// The rate-limit bucket.
    pub key: String,
    pub host: String,
    /// The session's `/api/vN` prefix plus the route.
    pub path: String,
    /// The session's [`KEPT`] headers.
    pub headers: Vec<(&'static str, String)>,
    /// Sent as a sensitive header.
    pub authorization: String,
}

impl std::fmt::Debug for RestCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RestCall({} {}{} <redacted>)",
            self.method, self.host, self.path
        )
    }
}

impl NetState {
    /// Everything before a Discord API call leaves: a session, an allowed method, a plain route
    /// ([`clean_route`]) and an open bucket.
    pub fn begin_rest(&self, method: &str, route: &str) -> Result<RestCall, String> {
        let session = self.sessions.current().ok_or(NOT_SIGNED_IN)?;
        let method = check_method(method)?;
        let route = clean_route(route)?;
        let key = bucket_key(method, &route);
        self.limits.check(&key)?;
        Ok(RestCall {
            method,
            key,
            path: format!("{}{}", session.prefix, route),
            host: session.host,
            headers: session.headers,
            authorization: session.authorization,
        })
    }

    /// What the answer to `call` teaches: rate limits, and a 401 drops the session until the
    /// client sends a new one.
    pub fn end_rest(
        &self,
        call: &RestCall,
        status: u16,
        header: &dyn Fn(&str) -> Option<String>,
        body: &[u8],
    ) {
        self.limits.learn(&call.key, status, header, body);
        if status == 401 {
            self.sessions.clear();
        }
    }
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

/// The methods a plugin may use, uppercase.
pub fn check_method(m: &str) -> Result<&'static str, String> {
    match m.to_ascii_uppercase().as_str() {
        "GET" => Ok("GET"),
        "POST" => Ok("POST"),
        "PUT" => Ok("PUT"),
        "PATCH" => Ok("PATCH"),
        "DELETE" => Ok("DELETE"),
        other => Err(format!("method {other:?} is not allowed")),
    }
}

/// A plugin's JSON body, at most [`MAX_REQUEST`] bytes.
pub fn encode_body(body: Option<&Val>) -> Result<Option<Vec<u8>>, String> {
    let Some(v) = body else { return Ok(None) };
    let b = serde_json::to_vec(&v.to_json()).map_err(|e| e.to_string())?;
    if b.len() > MAX_REQUEST {
        return Err(format!("request body over {MAX_REQUEST} bytes"));
    }
    Ok(Some(b))
}

/// A random multipart boundary (no `rand` dependency: time and a counter are enough for a
/// boundary, which only has to not occur in the parts).
pub fn new_boundary() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("phoecord{t:x}{:x}", N.fetch_add(1, Ordering::Relaxed))
}

/// Quote a form-data header value: no CR, LF or `"` may reach the wire.
fn form_value(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(c, '"' | '\r' | '\n') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// The `multipart/form-data` body Discord's client sends for uploads, and its content type.
pub fn multipart(
    payload: Option<&Val>,
    files: &[FilePart],
    boundary: &str,
) -> Result<(Vec<u8>, String), String> {
    if files.is_empty() {
        return Err("rest_files needs at least one file".into());
    }
    if files.len() > 10 {
        return Err("Discord takes at most 10 files per message".into());
    }
    let mut out = Vec::new();
    if let Some(p) = payload {
        let json = serde_json::to_vec(&p.to_json()).map_err(|e| e.to_string())?;
        out.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(&json);
        out.extend_from_slice(b"\r\n");
    }
    for (i, f) in files.iter().enumerate() {
        if f.content_type.contains(['\r', '\n']) {
            return Err("bad content type".into());
        }
        out.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"files[{i}]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                form_value(&f.filename),
                f.content_type
            )
            .as_bytes(),
        );
        out.extend_from_slice(&f.data);
        out.extend_from_slice(b"\r\n");
        if out.len() > MAX_UPLOAD {
            return Err(format!("upload over {MAX_UPLOAD} bytes"));
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((out, format!("multipart/form-data; boundary={boundary}")))
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
    // A dot segment in any spelling: `.`, `..`, or with `%2e` / `%2E` for any of its dots.
    let dot_segment = |s: &str| {
        let plain = s.to_ascii_lowercase().replace("%2e", ".");
        plain == "." || plain == ".."
    };
    if path.split('/').any(dot_segment) {
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

/// Where a `fetch` goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchUrl {
    /// Lowercase.
    pub host: String,
    pub port: u16,
    /// Path and query, starting with `/`.
    pub path: String,
}

/// An `https://` URL on port 443, without credentials or a fragment.
pub fn parse_fetch_url(url: &str) -> Result<FetchUrl, String> {
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
    Ok(FetchUrl {
        host: host.to_ascii_lowercase(),
        port,
        path,
    })
}

/// [`parse_fetch_url`], then the plugin's `fetch` list (`allowed`).
pub fn fetch_target(allowed: &[String], url: &str) -> Result<FetchUrl, String> {
    let url = parse_fetch_url(url)?;
    if !allowed
        .iter()
        .any(|h| phoecord_core::plugin::fetch_host_matches(h, &url.host))
    {
        return Err(format!(
            "host {} is not in this plugin's `fetch` list",
            url.host
        ));
    }
    Ok(url)
}

/// The headers of a `fetch`: [`FETCH_USER_AGENT`], then the caller's `extra` (lowercase names).
/// Hop-by-hop and credential headers (`phoecord_core::plugin::fetch_header_refused`), names that
/// are not HTTP tokens and values with control characters are refused before anything is sent.
pub fn fetch_headers(extra: &[(&str, &str)]) -> Result<Vec<(String, String)>, String> {
    let mut out = vec![("user-agent".to_string(), FETCH_USER_AGENT.to_string())];
    for (name, value) in extra {
        if phoecord_core::plugin::fetch_header_refused(name) {
            return Err(format!("header {name:?} may not be set by a plugin"));
        }
        let n = name.trim().to_ascii_lowercase();
        if n.is_empty() || !n.bytes().all(is_token_byte) {
            return Err(format!("bad header name {name:?}"));
        }
        if header_text(value.as_bytes()).is_none() {
            return Err(format!("bad value for header {name:?}"));
        }
        // A later value replaces an earlier one, as a `HeaderMap::insert` would.
        out.retain(|(k, _)| *k != n);
        out.push((n, value.to_string()));
    }
    Ok(out)
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// What a plugin gets from an answer: the [`SHOWN`] headers, and the body as JSON, text or bytes.
pub fn response(status: u16, header: &dyn Fn(&str) -> Option<String>, body: &[u8]) -> HostResponse {
    let headers = SHOWN
        .iter()
        .filter_map(|n| Some((n.to_string(), header(n)?)))
        .collect();
    let (json, text, bytes) = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(j) => (Some(Val::from_json(j)), None, None),
        Err(_) if body.is_empty() => (None, None, None),
        Err(_) => (
            None,
            Some(String::from_utf8_lossy(body).into_owned()),
            Some(body.to_vec()),
        ),
    };
    let mut r = HostResponse::new(status);
    r.body = json;
    r.text = text;
    r.headers = headers;
    r.bytes = bytes;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let pairs = pairs.to_vec();
        move |n: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn routes_are_plain_paths() {
        assert_eq!(clean_route("/users/@me").unwrap(), "/users/@me");
        assert_eq!(clean_route("/api/v9/users/@me").unwrap(), "/users/@me");
        for bad in [
            "https://evil.example/x",
            "//evil.example/x",
            "users/@me",
            "/a/../b",
            "/a/%2e%2e/b",
            "/a/.%2E/b",
            "/a/%2E./b",
            "/a/%2e/b",
            "/a\\b",
            "/a b",
            "/a#b",
            "/a://b",
        ] {
            assert!(clean_route(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn multipart_has_payload_and_files() {
        let payload = Val::from_json(serde_json::json!({"content": "hi"}));
        let files = [FilePart {
            filename: "pet\"pet.gif".into(),
            content_type: "image/gif".into(),
            data: b"GIF89a".to_vec(),
        }];
        let (body, ct) = multipart(Some(&payload), &files, "B").unwrap();
        assert_eq!(ct, "multipart/form-data; boundary=B");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"payload_json\""));
        assert!(text.contains("{\"content\":\"hi\"}"));
        assert!(text.contains("name=\"files[0]\"; filename=\"pet_pet.gif\"\r\nContent-Type: image/gif\r\n\r\nGIF89a\r\n"));
        assert!(text.ends_with("--B--\r\n"));
        assert!(multipart(None, &[], "B").is_err());
        let bad = [FilePart {
            filename: "a".into(),
            content_type: "x\r\ny".into(),
            data: vec![],
        }];
        assert!(multipart(None, &bad, "B").is_err());
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

    #[test]
    fn fetch_targets_need_the_list() {
        let list = [
            "api.example.com".to_string(),
            "*.discordsays.com".to_string(),
        ];
        assert!(fetch_target(&list, "https://api.example.com/x").is_ok());
        assert!(fetch_target(&list, "https://1.discordsays.com/x").is_ok());
        for bad in [
            "https://discord.com/api/v9/users/@me",
            "https://api.example.com.evil.example/x",
            "http://api.example.com/x",
            "https://a.b.discordsays.com/x",
        ] {
            assert!(fetch_target(&list, bad).is_err(), "{bad}");
        }
        assert!(fetch_target(&[], "https://api.example.com/x").is_err());
    }

    #[test]
    fn fetch_headers_refuse_credentials_and_bad_values() {
        let h = fetch_headers(&[("X-Quest", "1")]).unwrap();
        assert_eq!(h[0], ("user-agent".into(), FETCH_USER_AGENT.into()));
        assert_eq!(h[1], ("x-quest".into(), "1".into()));
        for name in [
            "Authorization",
            "cookie",
            "Host",
            "Proxy-Authorization",
            "TE",
        ] {
            let e = fetch_headers(&[(name, "x")]).unwrap_err();
            assert!(e.contains("may not be set"), "{name}: {e}");
        }
        assert!(
            fetch_headers(&[("X-Bad", "a\r\nb")])
                .unwrap_err()
                .contains("bad value")
        );
        assert!(
            fetch_headers(&[("a b", "x")])
                .unwrap_err()
                .contains("bad header name")
        );
    }

    #[test]
    fn sessions_follow_the_clients_api_requests_only() {
        let s = Sessions::default();
        let full = headers(&[
            ("authorization", "tok"),
            ("user-agent", "UA"),
            ("x-super-properties", "SP"),
            ("cookie", "c"),
        ]);
        assert!(!s.observe("evil.example", "/api/v9/x", &full));
        assert!(!s.observe("discord.com", "/other", &full));
        assert!(!s.observe(
            "discord.com",
            "/api/v9/x",
            &headers(&[("user-agent", "UA")])
        ));
        assert!(!s.signed_in());
        assert!(s.observe("Discord.com", "/api/v10/x", &full));
        let got = s.current().unwrap();
        assert_eq!(
            (got.host.as_str(), got.prefix.as_str()),
            ("discord.com", "/api/v10")
        );
        assert!(!got.headers.iter().any(|(n, _)| *n == "cookie"));
        // A later request without some headers keeps the earlier values for the same token.
        assert!(s.observe(
            "discord.com",
            "/api/v9/y",
            &headers(&[("authorization", "tok")])
        ));
        let got = s.current().unwrap();
        assert!(got.headers.contains(&("user-agent", "UA".into())));
        assert!(got.headers.contains(&("x-super-properties", "SP".into())));
        // Another token starts over.
        assert!(s.observe(
            "discord.com",
            "/api/v9/y",
            &headers(&[("authorization", "other")])
        ));
        assert!(s.current().unwrap().headers.is_empty());
        assert!(!format!("{:?}", s.current().unwrap()).contains("other"));
    }

    #[test]
    fn sessions_expire() {
        let s = Sessions::default();
        s.observe(
            "discord.com",
            "/api/v9/x",
            &headers(&[("authorization", "tok")]),
        );
        let now = Instant::now();
        assert!(s.age_at(now).is_some());
        assert!(
            s.current_at(now + SESSION_TTL + Duration::from_secs(1))
                .is_none()
        );
        assert!(!s.signed_in(), "an expired session is gone");
    }

    #[test]
    fn rest_calls_need_a_session_and_a_401_clears_it() {
        let n = NetState::default();
        assert_eq!(
            n.begin_rest("GET", "/users/@me").unwrap_err(),
            NOT_SIGNED_IN
        );
        n.sessions.observe(
            "discord.com",
            "/api/v9/x",
            &headers(&[("authorization", "tok")]),
        );
        let c = n.begin_rest("get", "/api/v9/channels/1/messages").unwrap();
        assert_eq!(
            (c.method, c.path.as_str()),
            ("GET", "/api/v9/channels/1/messages")
        );
        assert!(!format!("{c:?}").contains("tok"));
        assert!(
            n.begin_rest("TRACE", "/x")
                .unwrap_err()
                .contains("not allowed")
        );
        n.end_rest(&c, 429, &headers(&[("retry-after", "30")]), b"");
        assert!(
            n.begin_rest("GET", "/channels/1/messages")
                .unwrap_err()
                .contains("rate limited")
        );
        assert!(n.begin_rest("GET", "/channels/2/messages").is_ok());
        n.end_rest(&c, 401, &headers(&[]), b"");
        assert!(!n.sessions.signed_in());
    }

    #[test]
    fn responses_show_only_listed_headers() {
        let r = response(
            200,
            &headers(&[("content-type", "text/plain"), ("set-cookie", "x")]),
            b"hello",
        );
        assert_eq!(
            r.headers,
            vec![("content-type".into(), "text/plain".into())]
        );
        assert_eq!(r.text.as_deref(), Some("hello"));
        assert!(response(204, &headers(&[]), b"").text.is_none());
        assert!(response(200, &headers(&[]), b"{\"a\":1}").body.is_some());
    }
}
