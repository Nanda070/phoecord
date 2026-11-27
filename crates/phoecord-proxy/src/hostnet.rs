//! The network side of `Host::rest` and `Host::fetch`.
//!
//! The rules (the client's session, Discord's rate limits, route and URL checks, size limits,
//! what a plugin gets back) are `phoecord_transport::net_rules`, shared with the loader's
//! in-process network. This module is the proxy's HTTP client for them:
//!
//! * [`Net::observe`] feeds the session from the client's own requests through the proxy.
//! * [`Net`] runs one HTTPS request at a time for a plugin. The plugin chain is synchronous, so
//!   the request runs on a small runtime of its own and the calling thread waits (with
//!   `block_in_place` when it is a worker of a multi-threaded runtime, so the proxy keeps serving),
//!   optionally through the configured upstream proxy.

use crate::egress;
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::header::{self, HeaderMap, HeaderName, HeaderValue};
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use phoecord_core::{FilePart, HostResponse, Val};
use phoecord_transport::net_rules::{self, FetchUrl as Url, NetState};
use rustls::pki_types::ServerName;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

pub use phoecord_transport::host::GLOBAL_PER_SECOND;
pub use phoecord_transport::net_rules::{
    MAX_BODY, MAX_REQUEST, MAX_UPLOAD, SESSION_TTL, TIMEOUT, bucket_key, clean_route, multipart,
};

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

/// A header of `h` as text (`None` when absent or not visible ASCII).
fn text(h: &HeaderMap) -> impl Fn(&str) -> Option<String> + '_ {
    |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string)
}

pub struct Net {
    state: NetState,
    tls: Arc<rustls::ClientConfig>,
    target: Mutex<Target>,
    rt: OnceLock<tokio::runtime::Runtime>,
}

impl Net {
    pub fn new() -> Net {
        Net {
            state: NetState::default(),
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
        self.state.sessions.observe(host, path, &text(headers));
    }

    /// Forget the session (the client signed out, Discord said 401, …).
    pub fn clear(&self) {
        self.state.sessions.clear();
    }

    pub fn signed_in(&self) -> bool {
        self.state.sessions.signed_in()
    }

    /// A Discord API call as the client's own session.
    pub fn rest(
        &self,
        method: &str,
        path: &str,
        body: Option<&Val>,
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let json = net_rules::encode_body(body)?;
        self.rest_raw(
            method,
            path,
            json.map(|b| (b, "application/json".to_string())),
            upstream,
        )
    }

    /// A Discord API call with files as `multipart/form-data` (`payload_json` + `files[n]`).
    pub fn rest_files(
        &self,
        method: &str,
        path: &str,
        payload: Option<&Val>,
        files: &[FilePart],
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let (body, content_type) = multipart(payload, files, &net_rules::new_boundary())?;
        self.rest_raw(method, path, Some((body, content_type)), upstream)
    }

    fn rest_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<(Vec<u8>, String)>,
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let call = self.state.begin_rest(method, path)?;
        let mut headers = HeaderMap::new();
        for (n, v) in &call.headers {
            headers.insert(
                HeaderName::from_static(n),
                HeaderValue::from_str(v).map_err(|e| e.to_string())?,
            );
        }
        let mut auth = HeaderValue::from_str(&call.authorization).map_err(|e| e.to_string())?;
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        let url = Url {
            host: call.host.clone(),
            port: 443,
            path: call.path.clone(),
        };
        let r = self.run(to_method(call.method), url, headers, body, upstream)?;
        self.state
            .end_rest(&call, r.status, &text(&r.raw_headers), &r.raw_body);
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
        self.fetch_with(allowed, method, url, &[], body, upstream)
    }

    /// [`Net::fetch`] with the caller's extra request headers. Hop-by-hop and credential
    /// headers are refused before anything is sent (`phoecord_core::plugin::fetch_header_refused`).
    pub fn fetch_with(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        extra: &[(&str, &str)],
        body: Option<&Val>,
        upstream: Option<egress::Upstream>,
    ) -> Result<HostResponse, String> {
        let method = to_method(net_rules::check_method(method)?);
        let url = net_rules::fetch_target(allowed, url)?;
        let mut headers = HeaderMap::new();
        for (n, v) in net_rules::fetch_headers(extra)? {
            let name = HeaderName::from_bytes(n.as_bytes())
                .map_err(|_| format!("bad header name {n:?}"))?;
            let value =
                HeaderValue::from_str(&v).map_err(|_| format!("bad value for header {n:?}"))?;
            headers.insert(name, value);
        }
        let json = net_rules::encode_body(body)?;
        Ok(self
            .run(
                method,
                url,
                headers,
                json.map(|b| (b, "application/json".to_string())),
                upstream,
            )?
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
        body: Option<(Vec<u8>, String)>,
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

/// `net_rules::check_method`'s uppercase name as `hyper`'s method.
fn to_method(m: &'static str) -> Method {
    Method::from_bytes(m.as_bytes()).unwrap_or(Method::GET)
}

/// The proxy's [`Network`] for the transport boundary: the client's session headers, rate limits
/// and the (optionally upstream-proxied) HTTPS above.
impl phoecord_transport::host::Network for Net {
    fn signed_in(&self) -> bool {
        Net::signed_in(self)
    }

    fn rest(
        &self,
        method: &str,
        route: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        Net::rest(self, method, route, body, parse_upstream(upstream)?)
    }

    fn rest_files(
        &self,
        method: &str,
        route: &str,
        payload: Option<&Val>,
        files: &[FilePart],
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        Net::rest_files(
            self,
            method,
            route,
            payload,
            files,
            parse_upstream(upstream)?,
        )
    }

    fn fetch(
        &self,
        allowed: &[String],
        method: &str,
        url: &str,
        body: Option<&Val>,
        upstream: Option<&str>,
    ) -> Result<HostResponse, String> {
        Net::fetch(self, allowed, method, url, body, parse_upstream(upstream)?)
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
        Net::fetch_with(
            self,
            allowed,
            method,
            url,
            headers,
            body,
            parse_upstream(upstream)?,
        )
    }
}

fn parse_upstream(raw: Option<&str>) -> Result<Option<egress::Upstream>, String> {
    raw.map(egress::Upstream::parse)
        .transpose()
        .map_err(|e| format!("{e:#}"))
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

struct Raw {
    status: u16,
    raw_headers: HeaderMap,
    raw_body: Bytes,
}

impl Raw {
    fn into_response(self) -> HostResponse {
        net_rules::response(self.status, &text(&self.raw_headers), &self.raw_body)
    }
}

async fn request(
    target: Target,
    tls_live: Arc<rustls::ClientConfig>,
    upstream: Option<egress::Upstream>,
    method: Method,
    url: Url,
    mut headers: HeaderMap,
    body: Option<(Vec<u8>, String)>,
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
    let body = match body {
        Some((bytes, content_type)) => {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&content_type).map_err(|e| anyhow!("{e}"))?,
            );
            Some(bytes)
        }
        None => None,
    };
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
