//! In-process hooks of the loader and their bridge to `phoecord-transport::Engine`
//! (tasks 2.2.2 HTTP transport, 2.2.3 Gateway dispatch, 2.2.4 Gateway send).
//!
//! * [`found`]: typed locators (`Found<T>`) over the `signatures/*.toml` entries; each one that
//!   fails turns off only its own hook.
//! * [`guard`]: the panic guard every hook entry runs under.
//! * [`http`], [`gateway`]: client data in, `Engine` call, verdict out.
//! * [`HookRuntime`]: the three entry points a detour calls ([`HookRuntime::request`],
//!   [`HookRuntime::response`], [`HookRuntime::dispatch`]). Each is guarded and gated by its
//!   locator, and each falls back to "pass the traffic through" on any failure.
//!
//! * [`abi`]: method, URL and body read out of the client's request, through reads that may fail.
//! * `detour` (Windows x64): the inline detour on `send`, read-only in part 1 of 2.2.2.
//!
//! What is not here yet: the `poll` vtable swap (needs the response layout), body replacement
//! (needs the `Bytes` vtable) and the Gateway detours (need the handler payload type); see
//! `docs/research/phoenix-recon.md`, "Open questions".

pub mod abi;
pub mod behaviour;
pub mod client_state;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod context_menu;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod detour;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod gateway_frame;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod gateway_recv;
#[cfg(all(windows, target_arch = "x86_64"))]
pub mod gateway_send;
pub use crate::found;
pub mod gateway;
pub mod guard;
pub mod http;

use found::HookPlan;
use gateway::{GatewayAction, GatewayOutcome};
use guard::{Breaker, LogFn};
use http::HttpOutcome;
use phoecord_transport::{Effects, Engine};
use std::sync::{Arc, OnceLock};

pub struct HookRuntime {
    engine: Arc<Engine>,
    plan: HookPlan,
    log: LogFn,
    http_request: Breaker,
    http_response: Breaker,
    gateway: Breaker,
    gateway_send: Breaker,
}

impl HookRuntime {
    /// Logs every locator that failed, so the log says which features are off.
    pub fn new(engine: Arc<Engine>, plan: HookPlan, log: LogFn) -> HookRuntime {
        for line in plan.failures() {
            log(&line);
        }
        HookRuntime {
            engine,
            plan,
            log,
            http_request: Breaker::new("http_request"),
            http_response: Breaker::new("http_response"),
            gateway: Breaker::new("gateway_dispatch"),
            gateway_send: Breaker::new("gateway_send"),
        }
    }

    pub fn plan(&self) -> &HookPlan {
        &self.plan
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Entry of the `send` hook: a REST request about to leave the client.
    pub fn request(&self, method: &str, url: &str, body: Option<&[u8]>) -> HttpOutcome {
        if !self.plan.requests() {
            return pass_http();
        }
        self.http_request.guard_result(&self.log, pass_http, || {
            http::request(
                &self.engine,
                method,
                url,
                body,
                self.plan.can_replace_request_body(),
            )
        })
    }

    /// Entry of the `poll` hook: a REST answer about to reach the client.
    pub fn response(&self, method: &str, url: &str, status: u16, body: &[u8]) -> HttpOutcome {
        if !self.plan.responses() {
            return pass_http();
        }
        self.http_response.guard_result(&self.log, pass_http, || {
            http::response(
                &self.engine,
                method,
                url,
                status,
                body,
                self.plan.can_replace_body(),
            )
        })
    }

    /// Entry of the incoming Gateway hook: one dispatch event, name and `d` as JSON (2.2.3).
    pub fn dispatch(&self, name: &str, data: &[u8]) -> GatewayOutcome {
        if !self.plan.gateway() {
            return pass_gateway();
        }
        if name == "READY" {
            self.gateway
                .guard(&self.log, || (), || self.log_ready(data));
        }
        self.gateway.guard_result(&self.log, pass_gateway, || {
            gateway::dispatch(&self.engine, name, data)
        })
    }

    /// One line per `READY`: how many guilds arrived partial, which says whether `full_ready`
    /// (2.2.7, [`client_state`]) took effect.
    fn log_ready(&self, data: &[u8]) {
        let Ok(d) = serde_json::from_slice::<serde_json::Value>(data) else {
            return;
        };
        let (partial, total) =
            phoecord_transport::full_ready::partial_guilds(&phoecord_core::Val::from_json(d));
        (self.log)(&format!(
            "READY: {total} guild(s), {partial} of them partial{}",
            if partial == 0 { " (full READY)" } else { "" }
        ));
    }
}

impl HookRuntime {
    /// Entry of the encoder hook (2.2.4): one outgoing frame as JSON text.
    pub fn send(&self, frame: &[u8]) -> GatewayOutcome {
        if self.plan.gateway_send.is_err() {
            return pass_gateway();
        }
        self.gateway_send.guard_result(&self.log, pass_gateway, || {
            gateway::send(&self.engine, frame)
        })
    }
}

fn pass_http() -> HttpOutcome {
    HttpOutcome {
        action: http::HttpAction::Pass,
        effects: Effects::default(),
    }
}

fn pass_gateway() -> GatewayOutcome {
    GatewayOutcome {
        action: GatewayAction::Pass,
        extra: Vec::new(),
        sends: Vec::new(),
    }
}

static RUNTIME: OnceLock<HookRuntime> = OnceLock::new();

/// Makes `runtime` the one the detours reach. The first call wins; returns whether this was it.
pub fn install(runtime: HookRuntime) -> bool {
    RUNTIME.set(runtime).is_ok()
}

/// The installed runtime, if any; a detour with none passes the call through.
pub fn runtime() -> Option<&'static HookRuntime> {
    RUNTIME.get()
}

#[cfg(test)]
mod tests {
    use super::found::*;
    use super::*;
    use crate::signatures::{ResolveError, SignatureSet};
    use phoecord_plugin_runtime::Chain;
    use phoecord_transport::{config::Config, host::Shared};
    use std::sync::Mutex;

    const TEXT_RVA: u32 = 0x1000;
    const IMAGE_LEN: u32 = 0x2000;

    /// Distinct one-byte markers at known offsets of a fake `.text`.
    fn text() -> Vec<u8> {
        let mut t = vec![0xCCu8; 0x100];
        for (i, b) in [
            (0x10, 0xA1),
            (0x20, 0xA2),
            (0x30, 0xA3),
            (0x40, 0xA4),
            (0x50, 0xA5),
            (0x58, 0xA6),
            (0x68, 0xA7),
            (0x78, 0xA8),
            (0x88, 0xA9),
        ] {
            t[i] = b;
        }
        // Two copies of one marker: ambiguous.
        t[0x60] = 0xB1;
        t[0x70] = 0xB1;
        t
    }

    fn sigs(toml: &str) -> Vec<crate::signatures::Signature> {
        let set = SignatureSet::parse(&format!("[build.\"1\"]\n{toml}")).unwrap();
        let v = crate::version::ClientVersion {
            file_version: Some("1".into()),
            text_hash: "00".into(),
            text_rva: TEXT_RVA,
            text_len: 0x100,
        };
        set.select(&v).unwrap().1.to_vec()
    }

    fn plan(toml: &str) -> HookPlan {
        let sigs = sigs(toml);
        HookPlan::locate(&Scan {
            sigs: &sigs,
            text: &text(),
            text_rva: TEXT_RVA,
            image_len: IMAGE_LEN,
        })
    }

    const ALL: &str = r#"
        http_send = { aob = "A1" }
        http_poll = { aob = "A2" }
        rust_alloc = { aob = "A3" }
        rust_dealloc = { aob = "A4" }
        gateway_registry = { aob = "A5" }
        gateway_send = { aob = "A6" }
        bytes_static_vtable = { aob = "A7" }
        gateway_inflate = { aob = "A8" }
        gateway_frame = { aob = "A9" }
    "#;

    fn runtime_with(plan: HookPlan) -> (HookRuntime, Arc<Mutex<Vec<String>>>) {
        let dir = std::env::temp_dir().join(format!("phoecord-test-hooks-{}", std::process::id()));
        let chain = Chain::new(vec![Box::new(phoecord_plugins::no_track::NoTrack::new())]);
        let engine = Arc::new(Engine::new(chain, Shared::new(dir), Config::default()));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        let log: LogFn = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        (HookRuntime::new(engine, plan, log), lines)
    }

    #[test]
    fn every_locator_resolves_on_a_good_table() {
        let p = plan(ALL);
        assert_eq!(p.send.as_ref().unwrap().rva, TEXT_RVA + 0x10);
        assert_eq!(p.poll.as_ref().unwrap().rva, TEXT_RVA + 0x20);
        let a = p.allocator.as_ref().unwrap();
        assert_eq!(
            (a.rva, a.target.dealloc_rva),
            (TEXT_RVA + 0x30, TEXT_RVA + 0x40)
        );
        assert_eq!(p.registry.as_ref().unwrap().rva, TEXT_RVA + 0x50);
        assert_eq!(p.gateway_send.as_ref().unwrap().rva, TEXT_RVA + 0x58);
        assert!(p.failures().is_empty());
    }

    #[test]
    fn a_missing_signature_disables_only_its_hook() {
        let p = plan(&ALL.replace("http_poll = { aob = \"A2\" }", ""));
        assert_eq!(
            p.poll.as_ref().unwrap_err(),
            &LocatorError::Missing(HTTP_POLL)
        );
        assert!(p.requests() && p.gateway() && p.can_replace_body());
        assert!(!p.responses());
        assert_eq!(p.failures().len(), 1);
    }

    #[test]
    fn invalid_signatures_are_reported_by_kind() {
        let p = plan(
            r#"
            http_send = { aob = "9E" }
            http_poll = { aob = "B1" }
            rust_alloc = { aob = "A3" }
            gateway_registry = { aob = "A5", rip = 0x2000 }
        "#,
        );
        assert_eq!(
            p.send.as_ref().unwrap_err(),
            &LocatorError::Unresolved(HTTP_SEND, ResolveError::NotFound)
        );
        assert_eq!(
            p.poll.as_ref().unwrap_err(),
            &LocatorError::Unresolved(HTTP_POLL, ResolveError::Ambiguous(2))
        );
        // One shim alone is no allocator.
        assert_eq!(
            p.allocator.as_ref().unwrap_err(),
            &LocatorError::Missing(RUST_DEALLOC)
        );
        assert!(matches!(
            p.registry.as_ref().unwrap_err(),
            LocatorError::Unresolved(GATEWAY_REGISTRY, ResolveError::BadRip)
        ));
        assert!(!p.requests() && !p.responses() && !p.can_replace_body() && !p.gateway());
    }

    #[test]
    fn an_address_outside_the_image_is_refused() {
        let sigs = sigs(r#"http_send = { aob = "A1" }"#);
        let p = HookPlan::locate(&Scan {
            sigs: &sigs,
            text: &text(),
            text_rva: TEXT_RVA,
            image_len: TEXT_RVA + 0x10,
        });
        assert_eq!(
            p.send.unwrap_err(),
            LocatorError::OutOfImage(HTTP_SEND, TEXT_RVA + 0x10)
        );
    }

    #[test]
    fn a_hook_without_a_locator_never_reaches_the_engine() {
        // `/science` would be answered locally by the NoTrack plugin; with the hook off it passes.
        let (rt, lines) = runtime_with(plan(r#"gateway_registry = { aob = "A5" }"#));
        let out = rt.request("POST", "https://discord.com/api/v9/science", None);
        assert_eq!(out.action, http::HttpAction::Pass);
        assert!(rt.response("GET", "/users/@me", 200, b"{}").action == http::HttpAction::Pass);
        // The three misses were logged at start, once each.
        assert!(
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.contains("http_send"))
        );
    }

    #[test]
    fn requests_and_dispatch_reach_the_engine() {
        let (rt, _) = runtime_with(plan(ALL));
        let out = rt.request("POST", "https://discord.com/api/v9/science", Some(b"{}"));
        assert_eq!(
            out.action,
            http::HttpAction::Reply {
                status: 204,
                body: None
            }
        );
        let out = rt.request("GET", "https://discord.com/api/v9/users/@me", None);
        assert_eq!(out.action, http::HttpAction::Pass);
        // A body that is not JSON is left alone.
        let out = rt.request(
            "POST",
            "/api/v9/channels/1/attachments",
            Some(b"--boundary"),
        );
        assert_eq!(out.action, http::HttpAction::Pass);
        let out = rt.dispatch("MESSAGE_CREATE", br#"{"content":"hi"}"#);
        assert_eq!(out.action, GatewayAction::Pass);
    }

    #[test]
    fn dispatch_with_no_gateway_plugin_passes_without_parsing() {
        let (rt, lines) = runtime_with(plan(ALL));
        let out = rt.dispatch("MESSAGE_CREATE", b"not json");
        // No plugin of this chain listens to the gateway, so the data is not even parsed.
        assert_eq!(out.action, GatewayAction::Pass);
        assert!(!rt.gateway.is_tripped());
        let _ = lines;
    }

    #[test]
    fn a_panic_is_caught_logged_and_trips_the_hook() {
        let log_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = log_lines.clone();
        let log: LogFn = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        let b = Breaker::new("t");
        let mut ran_after = false;
        let v = b.guard(&log, || "fallback", || -> &str { panic!("boom") });
        assert_eq!(v, "fallback");
        assert!(b.is_tripped());
        // Once tripped the body no longer runs.
        let v = b.guard(
            &log,
            || "fallback",
            || {
                ran_after = true;
                "body"
            },
        );
        assert_eq!((v, ran_after), ("fallback", false));
        let lines = log_lines.lock().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("boom") && lines[0].contains("disabled"));
    }

    #[test]
    fn a_failed_call_is_logged_and_falls_back_but_keeps_the_hook() {
        let log_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = log_lines.clone();
        let log: LogFn = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        let b = Breaker::new("t");
        let v = b.guard_result(&log, || 0, || Err::<i32, _>("bad state".into()));
        assert_eq!(v, 0);
        assert!(!b.is_tripped());
        assert_eq!(b.guard_result(&log, || 0, || Ok(7)), 7);
        assert!(log_lines.lock().unwrap()[0].contains("bad state"));
    }

    #[test]
    fn a_logger_that_panics_does_not_escape() {
        let log: LogFn = Arc::new(|_| panic!("log broke"));
        let b = Breaker::new("t");
        assert_eq!(b.guard(&log, || 1, || -> i32 { panic!("x") }), 1);
    }
}
