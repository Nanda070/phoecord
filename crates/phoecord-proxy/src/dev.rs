//! The local endpoint of the plugin dev console (task 1.3.9).
//!
//! It lives on the proxy's own listener under [`PREFIX`]. Two rules guard it, both checked here
//! even though the listener already serves loopback peers only:
//!
//! 1. the peer must be a loopback address;
//! 2. the request must carry `Authorization: Bearer <token>`, where the token is made at every
//!    start of the proxy and written to `dev-token` in the data folder. Only someone who can read
//!    that file (the user who runs the proxy) can attach; a web page cannot guess it.
//!
//! Endpoints:
//!
//! | Request | Answer |
//! |:---|:---|
//! | `GET /__phoecord/dev/stream` | never-ending NDJSON: a `hello` line, then `log`, `run` and `ping` lines |
//! | `POST /__phoecord/dev/command` | JSON `{"cmd": …}` → `{"ok": true, "result": …}` or `{"ok": false, "error": …}` |
//!
//! Commands: `plugins`, `toggle` (`plugin`), `reload` (`plugin`), `settings` (`plugin`),
//! `kv` (`plugin`), `replay` (`event`, `plugin`), `events` (what can be replayed).

use crate::Proxy;
use crate::rest::{Body, full};
use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Limited, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use phoecord_transport::dev::DevEntry;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Every dev endpoint is under this path.
pub const PREFIX: &str = "/__phoecord/dev";
/// The token file in the data folder.
pub const TOKEN_FILE: &str = "dev-token";
/// Largest command body accepted.
const MAX_BODY: usize = 64 * 1024;
/// Entries a console may be behind before the newest are dropped (and counted).
const QUEUE: usize = 2048;
/// A `ping` line this often on a quiet stream, so a gone console is noticed.
const PING: Duration = Duration::from_secs(15);

/// Where the token of the proxy that uses `data` lives.
pub fn token_path(data: &Path) -> PathBuf {
    data.join(TOKEN_FILE)
}

/// Make a new random token (kept in memory; [`write_token`] publishes it).
pub fn new_token() -> Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("no random numbers for the dev token"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write `token` to the data folder (owner-only on Unix; on Windows the user's profile folder is
/// already private to the user). Only the proxy that serves the user's client does this: a
/// short-lived proxy on the same data folder (`phoecord selftest`) must not replace the token of
/// the running one.
pub fn write_token(data: &Path, token: &str) -> Result<()> {
    std::fs::create_dir_all(data)?;
    write_private(&token_path(data), token.as_bytes()).context("writing dev-token")
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        // `mode` applies only when the file is created; an older file keeps its bits otherwise.
        f.set_permissions(
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600),
        )?;
        f.write_all(data)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)
    }
}

fn token_ok(expected: &str, header: Option<&str>) -> bool {
    let Some(given) = header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
    else {
        return false;
    };
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    // No early exit on the first differing byte.
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What the endpoint answers.
pub enum Reply {
    Status(StatusCode),
    Json(Value),
    /// NDJSON lines, until the console goes away.
    Stream(mpsc::Receiver<String>),
}

/// A request to the endpoint, already read.
pub struct DevRequest<'a> {
    pub peer: SocketAddr,
    pub method: &'a Method,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
    pub body: &'a [u8],
}

/// Whether `path` belongs to the dev endpoint.
pub fn is_dev_path(path: &str) -> bool {
    path == PREFIX || path.starts_with(&format!("{PREFIX}/"))
}

/// Check the peer and the token, then serve the request.
pub async fn route(app: &Arc<Proxy>, req: DevRequest<'_>) -> Reply {
    if !req.peer.ip().is_loopback() {
        tracing::warn!("dev console refused for {}: not a loopback peer", req.peer);
        return Reply::Status(StatusCode::FORBIDDEN);
    }
    if !token_ok(&app.dev_token, req.authorization) {
        tracing::warn!(
            "dev console refused for {}: wrong or missing token",
            req.peer
        );
        return Reply::Status(StatusCode::UNAUTHORIZED);
    }
    match (req.method, req.path.strip_prefix(PREFIX).unwrap_or("")) {
        (&Method::GET, "/stream") => Reply::Stream(stream(app)),
        (&Method::POST, "/command") => Reply::Json(command(app, req.body).await),
        (_, "/stream" | "/command") => Reply::Status(StatusCode::METHOD_NOT_ALLOWED),
        _ => Reply::Status(StatusCode::NOT_FOUND),
    }
}

/// Attach a console: the `hello` line, then everything the bus carries.
fn stream(app: &Arc<Proxy>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel::<String>(QUEUE);
    let hello = json!({
        "type": "hello",
        "version": env!("CARGO_PKG_VERSION"),
        "plugins": app.plugin_list(),
    });
    let _ = tx.try_send(line(&hello));
    let bus = app.shared.dev.clone();
    let counter = bus.clone();
    bus.subscribe(Box::new(move |entry: &DevEntry| {
        let Ok(text) = serde_json::to_string(entry) else {
            return true;
        };
        match tx.try_send(text + "\n") {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                counter.count_dropped(1);
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }));
    rx
}

fn line(v: &Value) -> String {
    format!("{v}\n")
}

fn ok(result: Value) -> Value {
    json!({ "ok": true, "result": result })
}

fn fail(error: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": error.to_string() })
}

async fn command(app: &Arc<Proxy>, body: &[u8]) -> Value {
    let Ok(cmd) = serde_json::from_slice::<Value>(body) else {
        return fail("the body is not JSON");
    };
    let text = |key: &str| cmd.get(key).and_then(Value::as_str).map(str::to_string);
    let need = |key: &str| text(key).ok_or_else(|| format!("{key} is missing"));
    let Some(name) = text("cmd") else {
        return fail("cmd is missing");
    };
    let result: Result<Value, String> = match name.as_str() {
        "plugins" => Ok(json!(app.plugin_list())),
        "events" => Ok(json!(app.shared.dev.remembered())),
        "settings" => match need("plugin") {
            Ok(id) => app.plugin_settings(&id),
            Err(e) => Err(e),
        },
        "kv" => match need("plugin") {
            Ok(id) => app.plugin_kv(&id),
            Err(e) => Err(e),
        },
        "toggle" => match need("plugin") {
            Ok(id) => app.toggle_plugin(&id).map(|on| {
                // Gateway tasks replay their state through the chain when this changes.
                app.reloads.send_modify(|n| *n += 1);
                json!({ "plugin": id, "enabled": on })
            }),
            Err(e) => Err(e),
        },
        "reload" => match need("plugin") {
            Ok(id) => {
                let (a, i) = (app.clone(), id.clone());
                match tokio::task::spawn_blocking(move || a.reload_plugin(&i)).await {
                    Ok(Ok(())) => {
                        app.reloads.send_modify(|n| *n += 1);
                        Ok(json!({ "plugin": id }))
                    }
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(format!("the reload task failed: {e}")),
                }
            }
            Err(e) => Err(e),
        },
        "replay" => match (need("event"), need("plugin")) {
            (Ok(event), Ok(plugin)) => {
                let (a, ev, pl) = (app.clone(), event, plugin);
                match tokio::task::spawn_blocking(move || a.replay(&ev, &pl)).await {
                    Ok(r) => r.map(|r| json!(r)),
                    Err(e) => Err(format!("the replay task failed: {e}")),
                }
            }
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        other => Err(format!("unknown command {other:?}")),
    };
    match result {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}

/// Serve one HTTP request for the dev endpoint (`front` in `server.rs` calls this for [`PREFIX`]).
pub async fn handle(app: Arc<Proxy>, req: Request<Incoming>, peer: SocketAddr) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let auth = parts
        .headers
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    // Refuse before reading anything from a peer that may not talk to us.
    let body = if peer.ip().is_loopback() {
        match Limited::new(body, MAX_BODY).collect().await {
            Ok(b) => b.to_bytes(),
            Err(_) => return status(StatusCode::PAYLOAD_TOO_LARGE),
        }
    } else {
        Bytes::new()
    };
    let reply = route(
        &app,
        DevRequest {
            peer,
            method: &parts.method,
            path: parts.uri.path(),
            authorization: auth.as_deref(),
            body: &body,
        },
    )
    .await;
    match reply {
        Reply::Status(s) => status(s),
        Reply::Json(v) => {
            let mut r = Response::new(full(v.to_string()));
            r.headers_mut().insert(
                hyper::header::CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/json"),
            );
            r
        }
        Reply::Stream(rx) => {
            let lines = futures_util::stream::unfold(rx, |mut rx| async move {
                let next = match tokio::time::timeout(PING, rx.recv()).await {
                    Ok(Some(line)) => line,
                    Ok(None) => return None,
                    Err(_) => line(&json!({ "type": "ping" })),
                };
                Some((next, rx))
            });
            let body = StreamBody::new(
                lines.map(|l| Ok::<_, crate::rest::BoxError>(Frame::data(Bytes::from(l)))),
            );
            let mut r = Response::new(BodyExt::boxed(body));
            r.headers_mut().insert(
                hyper::header::CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/x-ndjson"),
            );
            r
        }
    }
}

fn status(s: StatusCode) -> Response<Body> {
    let mut r = Response::new(full(""));
    *r.status_mut() = s;
    r
}
