//! The in-process transport (Phase 2, task 2.2.7), first part: what stands between the detours
//! and `Engine`.
//!
//! [`InProcessTransport`] is what a detour calls. It asks the degradation plan ([`crate::plan`])
//! whether the entry point runs in-process, runs it through [`HookRuntime`] (locator gate, panic
//! guard, `Engine`) and takes care of what the plugins queued ([`Delivery`]):
//!
//! - server → client payloads can only be handed to the client from inside a registry handler
//!   (`docs/research/phoenix-desktop.md` §3.2), so they wait in a queue that the dispatch detour
//!   empties; without a dispatch hook they are dropped. The inflate hook empties it but cannot
//!   hand them over yet (`docs/research/inprocess-dispatch.md`);
//! - client → server frames leave through a [`FrameSink`]. No send function is known (2.1.6,
//!   2.2.4), so for now they are dropped;
//! - plugin timers need a clock: [`Clock`], one thread.
//!
//! The transport is all or nothing per launch (ruling 2026-11-26, task 2.2.8 item 2): it runs only
//! when every entry point's locator resolved ([`Plan::in_process`]). With any one missing the whole
//! launch goes through the proxy and every entry point here passes the traffic untouched, so the
//! same traffic never meets the plugin chain twice and nothing is handed over to the proxy.
//!
//! The `Network` behind `Host::rest` and `Host::fetch` in-process is [`crate::net::LoaderNet`];
//! the transport only tells the `send` detour when to hand it the client's request headers
//! ([`InProcessTransport::wants_session`], [`InProcessTransport::session`]).

use crate::hooks::{
    HookRuntime,
    abi::Header,
    found::HookPlan,
    gateway::GatewayAction,
    guard::{Breaker, LogFn},
    http::HttpAction,
};
use crate::net::LoaderNet;
use crate::plan::{EntryPoint, Plan, Reason, Route};
use phoecord_core::Val;
use phoecord_transport::{Effects, Engine};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Wake, Waker};
use std::thread::{self, JoinHandle, Thread};
use std::time::Instant;

/// The most payloads that wait for the dispatch detour; the oldest are dropped beyond it.
pub const MAX_QUEUED: usize = 1024;

/// Writes one client → server frame to the live gateway connection; `false` when there is none.
pub type FrameSink = Box<dyn Fn(Val) -> bool + Send + Sync>;

/// How much of what plugins queued could not be delivered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dropped {
    pub locals: u64,
    pub frames: u64,
}

/// Where the effects of the plugin chain go in-process.
pub struct Delivery {
    engine: Arc<Engine>,
    locals: Mutex<VecDeque<Val>>,
    /// Whether a dispatch detour will ever take the queued payloads.
    to_client: bool,
    to_server: Option<FrameSink>,
    log: LogFn,
    dropped_locals: AtomicU64,
    dropped_frames: AtomicU64,
}

impl Delivery {
    pub fn new(
        engine: Arc<Engine>,
        to_client: bool,
        to_server: Option<FrameSink>,
        log: LogFn,
    ) -> Delivery {
        Delivery {
            engine,
            locals: Mutex::new(VecDeque::new()),
            to_client,
            to_server,
            log,
            dropped_locals: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
        }
    }

    /// What plugins queued outside a gateway frame (a REST event, a timer). Its frames have
    /// already passed the Send chain.
    pub fn effects(&self, effects: Effects) {
        self.to_client(effects.locals);
        self.to_server(effects.frames);
    }

    /// What plugins queued while handling a dispatch event: `sends` still have to pass the Send
    /// chain.
    pub fn gateway(&self, extra: Vec<Val>, sends: Vec<(i64, Val)>) {
        self.to_client(extra);
        self.effects(self.engine.chain_queued(sends));
    }

    /// The payloads waiting for the client, oldest first. Only the dispatch detour calls this: it
    /// feeds them to the client's own handlers, without running the plugin chain on them again.
    pub fn take_locals(&self) -> Vec<Val> {
        self.queue().drain(..).collect()
    }

    pub fn dropped(&self) -> Dropped {
        Dropped {
            locals: self.dropped_locals.load(Ordering::Relaxed),
            frames: self.dropped_frames.load(Ordering::Relaxed),
        }
    }

    fn fire(&self, plugin: &str, tag: &str) {
        self.effects(self.engine.fire_timer(plugin, tag));
    }

    fn queue(&self) -> MutexGuard<'_, VecDeque<Val>> {
        // The queue is whole even if a holder panicked: every change is one call.
        self.locals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn to_client(&self, payloads: Vec<Val>) {
        if payloads.is_empty() {
            return;
        }
        if !self.to_client {
            return self.drop_some(
                &self.dropped_locals,
                payloads.len(),
                "no dispatch hook: payloads plugins queued for the client are dropped",
            );
        }
        let over = {
            let mut queue = self.queue();
            queue.extend(payloads);
            let over = queue.len().saturating_sub(MAX_QUEUED);
            queue.drain(..over);
            over
        };
        self.drop_some(
            &self.dropped_locals,
            over,
            "the queue for the client is full: the oldest payloads are dropped",
        );
    }

    fn to_server(&self, frames: Vec<Val>) {
        let mut lost = 0;
        for frame in frames {
            if !self.to_server.as_ref().is_some_and(|send| send(frame)) {
                lost += 1;
            }
        }
        self.drop_some(
            &self.dropped_frames,
            lost,
            "no way to send: gateway frames plugins queued are dropped",
        );
    }

    /// Counts `n` drops; logs the first of a kind only, so a busy plugin cannot flood the log.
    fn drop_some(&self, counter: &AtomicU64, n: usize, why: &str) {
        if n > 0 && counter.fetch_add(n as u64, Ordering::Relaxed) == 0 {
            (self.log)(why);
        }
    }
}

/// What the dispatch detour does with one event, in this order.
#[derive(Debug, PartialEq)]
pub struct Dispatch {
    /// Payloads queued since the last event: apply them first.
    pub before: Vec<Val>,
    /// What to do with the event itself.
    pub action: GatewayAction,
    /// Payloads queued while handling it: apply them afterwards.
    pub after: Vec<Val>,
}

/// The entry points a detour calls.
pub struct InProcessTransport {
    plan: Plan,
    /// Whether this launch runs in-process at all ([`Plan::in_process`]).
    active: bool,
    hooks: HookRuntime,
    delivery: Arc<Delivery>,
    queued: Breaker,
    log: LogFn,
    /// The plugins' network, fed with the client's session headers by the `send` detour.
    net: Option<Arc<LoaderNet>>,
}

impl InProcessTransport {
    /// `engine` driven by the hooks `found` in this build. Logs each entry point that stays with
    /// the proxy only because another one does (the failed locators are logged by the runtime),
    /// and whether the launch runs in-process.
    pub fn new(engine: Arc<Engine>, found: HookPlan, log: LogFn) -> InProcessTransport {
        let plan = Plan::from_hooks(&found);
        for point in EntryPoint::ALL {
            if let Route::Proxy(why @ Reason::Needs(_)) = plan.route(point) {
                log(&format!("{}: through the proxy ({why})", point.name()));
            }
        }
        let active = plan.all_in_process();
        log(if active {
            "in-process transport on: every entry point was found"
        } else {
            "in-process transport off for this launch: the proxy carries all network traffic"
        });
        let delivery = Delivery::new(engine.clone(), active, None, log.clone());
        InProcessTransport {
            plan,
            active,
            hooks: HookRuntime::new(engine, found, log.clone()),
            delivery: Arc::new(delivery),
            queued: Breaker::new("delivery"),
            log,
            net: None,
        }
    }

    /// The same transport feeding `net` (the engine's `Network`) with the client's session.
    pub fn with_network(mut self, net: Arc<LoaderNet>) -> InProcessTransport {
        self.net = Some(net);
        self
    }

    /// Whether the `send` detour should read this request's headers for the plugins' session:
    /// only Discord API URLs, and only when the session is missing or due for a refresh.
    pub fn wants_session(&self, url: &str) -> bool {
        self.active && self.net.as_ref().is_some_and(|n| n.wants_session(url))
    }

    /// The headers of the client's request to `url`, for the plugins' session.
    pub fn session(&self, url: &str, headers: &[Header]) {
        if let Some(net) = self.net.as_ref().filter(|_| self.active) {
            net.observe(url, headers);
        }
    }

    /// What the locators found, entry point by entry point.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Whether this launch runs in-process; the detour installer hooks nothing when it does not.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether a plugin looks at this REST request; the `send` detour skips reading the body
    /// otherwise.
    pub fn wants_request(&self, method: &str, url: &str) -> bool {
        self.active
            && self
                .hooks
                .engine()
                .wants_request(method, &crate::hooks::http::route_of(url))
    }

    /// Whether a plugin looks at the answer to this REST request; the `poll` side remembers only
    /// those requests.
    pub fn wants_response(&self, method: &str, url: &str) -> bool {
        self.active
            && self
                .hooks
                .engine()
                .wants_response(method, &crate::hooks::http::route_of(url))
    }

    /// For the [`Clock`].
    /// The plugin entries of the message context menu (2.3.5).
    pub fn menu_items(&self) -> Vec<phoecord_transport::menu::MenuItem> {
        self.hooks.engine().menu_items()
    }

    /// A click on a plugin's menu entry: runs it and delivers what it queues.
    pub fn menu_action(
        &self,
        plugin: &str,
        entry: &str,
        click: phoecord_transport::menu::MenuClick,
    ) -> Result<(), String> {
        let effects = self.hooks.engine().menu_action(plugin, entry, click)?;
        self.deliver(|d| d.effects(effects));
        Ok(())
    }

    pub fn delivery(&self) -> &Arc<Delivery> {
        &self.delivery
    }

    /// A REST request about to leave the client.
    pub fn request(&self, method: &str, url: &str, body: Option<&[u8]>) -> HttpAction {
        if !self.runs(EntryPoint::Request) {
            return HttpAction::Pass;
        }
        let out = self.hooks.request(method, url, body);
        self.deliver(|d| d.effects(out.effects));
        out.action
    }

    /// A REST answer about to reach the client.
    pub fn response(&self, method: &str, url: &str, status: u16, body: &[u8]) -> HttpAction {
        if !self.runs(EntryPoint::Response) {
            return HttpAction::Pass;
        }
        let out = self.hooks.response(method, url, status, body);
        self.deliver(|d| d.effects(out.effects));
        out.action
    }

    /// One decoded gateway event.
    pub fn dispatch(&self, name: &str, data: &[u8]) -> Dispatch {
        if !self.runs(EntryPoint::Dispatch) {
            return Dispatch {
                before: Vec::new(),
                action: GatewayAction::Pass,
                after: Vec::new(),
            };
        }
        let before = self.delivery.take_locals();
        let out = self.hooks.dispatch(name, data);
        self.deliver(|d| d.gateway(out.extra, out.sends));
        Dispatch {
            before,
            action: out.action,
            after: self.delivery.take_locals(),
        }
    }

    /// Whether any plugin reads the gateway; the inflate hook skips parsing frames otherwise.
    pub fn wants_gateway(&self) -> bool {
        self.active && self.hooks.engine().wants_gateway()
    }

    /// One outgoing gateway frame (JSON text) from the encoder hook. What plugins queued is
    /// delivered like a dispatch's effects.
    pub fn send_frame(&self, frame: &[u8]) -> GatewayAction {
        if !self.runs(EntryPoint::Send) {
            return GatewayAction::Pass;
        }
        let out = self.hooks.send(frame);
        self.deliver(|d| d.gateway(out.extra, out.sends));
        out.action
    }

    /// All or nothing per launch: an entry point runs only when they all do.
    fn runs(&self, _point: EntryPoint) -> bool {
        self.active
    }

    /// Delivery runs plugin code too (the Send chain), so it gets a panic guard of its own.
    fn deliver(&self, f: impl FnOnce(&Delivery)) {
        self.queued.guard(&self.log, || (), || f(&self.delivery));
    }
}

/// What the `send` detour calls: the transport behind a shared handle.
#[cfg(all(windows, target_arch = "x86_64"))]
pub struct SendSink(pub Arc<InProcessTransport>);

#[cfg(all(windows, target_arch = "x86_64"))]
impl crate::hooks::detour::SendSink for SendSink {
    fn wants(&self, method: &str, url: &str) -> bool {
        self.0.wants_request(method, url)
    }

    fn request(&self, method: &str, url: &str, body: Option<&[u8]>) -> HttpAction {
        self.0.request(method, url, body)
    }

    fn wants_response(&self, method: &str, url: &str) -> bool {
        self.0.wants_response(method, url)
    }

    fn response(&self, method: &str, url: &str, status: u16, body: &[u8]) -> HttpAction {
        self.0.response(method, url, status, body)
    }

    fn wants_session(&self, url: &str) -> bool {
        self.0.wants_session(url)
    }

    fn session(&self, url: &str, headers: &[Header]) {
        self.0.session(url, headers)
    }
}

/// What the inflate hook calls (2.2.3): the transport behind a shared handle.
#[cfg(all(windows, target_arch = "x86_64"))]
pub struct DecodeSink(pub Arc<InProcessTransport>);

#[cfg(all(windows, target_arch = "x86_64"))]
impl crate::hooks::gateway_recv::DispatchSink for DecodeSink {
    fn wants(&self) -> bool {
        self.0.wants_gateway()
    }

    fn dispatch(&self, name: &str, d: &[u8]) -> (GatewayAction, usize) {
        let out = self.0.dispatch(name, d);
        (out.action, out.before.len() + out.after.len())
    }
}

/// What the encoder hook calls (2.2.4): the transport behind a shared handle.
#[cfg(all(windows, target_arch = "x86_64"))]
pub struct EncodeSink(pub Arc<InProcessTransport>);

#[cfg(all(windows, target_arch = "x86_64"))]
impl crate::hooks::gateway_send::FrameSink for EncodeSink {
    fn frame(&self, json: &[u8]) -> GatewayAction {
        self.0.send_frame(json)
    }
}

/// Wakes a parked thread: all the executor the timer store's change notification needs.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// The thread that fires plugin timers (`Host::schedule`).
pub struct Clock {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Clock {
    pub fn start(delivery: Arc<Delivery>) -> std::io::Result<Clock> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("phoecord-timers".into())
            .spawn(move || tick(&delivery, &flag))?;
        Ok(Clock {
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the thread and waits for it. Never call this under the loader lock (`DllMain`): the
    /// thread cannot exit while it is held. Dropping the clock only asks the thread to stop.
    pub fn stop(mut self) {
        self.signal();
        if let Some(thread) = self.thread.take() {
            // An error means the thread panicked; there is nothing left to stop.
            let _ = thread.join();
        }
    }

    fn signal(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Drop for Clock {
    fn drop(&mut self) {
        self.signal();
    }
}

/// Sleeps until the earliest timer is due or the timers change, until told to stop. A panic while
/// firing ends the clock, as a panic ends any hook.
fn tick(delivery: &Delivery, stop: &AtomicBool) {
    let timers = delivery.engine.shared.timers.clone();
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let breaker = Breaker::new("timers");
    while !stop.load(Ordering::Acquire) && !breaker.is_tripped() {
        // Registered before `next_due` is read, so a timer set in between still wakes us.
        let mut changed = pin!(timers.changed());
        if changed.as_mut().poll(&mut cx).is_ready() {
            continue;
        }
        let now = Instant::now();
        match timers.next_due() {
            Some(due) if due <= now => breaker.guard(
                &delivery.log,
                || (),
                || {
                    for (plugin, tag) in timers.take_due(now) {
                        delivery.fire(&plugin, &tag);
                    }
                },
            ),
            Some(due) => thread::park_timeout(due - now),
            None => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::found::{Found, GatewayInflate, GatewaySend, HttpPoll, HttpSend};

    use phoecord_core::{Event, EventKind, Host, Manifest, Plugin, TIMER_EVENT, Verdict};
    use phoecord_plugin_runtime::Chain;
    use phoecord_transport::{config::Config, host::Shared};
    use std::time::Duration;

    /// Queues something on every kind of event, named after what caused it.
    struct Queues(Manifest);

    fn local(name: &str) -> Val {
        let mut v = Val::map();
        v.set("op", Val::Int(0));
        v.set("t", Val::Str(name.into()));
        v.set("d", Val::map());
        v
    }

    fn names(payloads: &[Val]) -> Vec<&str> {
        payloads.iter().filter_map(|p| p.str_at(&["t"])).collect()
    }

    impl Plugin for Queues {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, _: &EventKind) -> bool {
            true
        }
        fn wants_gateway(&self) -> bool {
            true
        }
        fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
            match event {
                Event::RestRequest {
                    route: "/later", ..
                } => {
                    host.schedule(50, "t").unwrap();
                }
                Event::RestRequest { .. } => {
                    host.emit_dispatch(local("FROM_REQUEST"));
                    host.emit_send(3, Val::map());
                }
                Event::RestResponse { .. } => host.emit_dispatch(local("FROM_RESPONSE")),
                Event::Dispatch { name, .. } if name == TIMER_EVENT => {
                    host.emit_dispatch(local("FROM_TIMER"));
                }
                Event::Dispatch {
                    name: "MESSAGE_CREATE",
                    extra,
                    ..
                } => {
                    extra.push(local("FROM_DISPATCH"));
                }
                _ => {}
            }
            Verdict::Pass
        }
    }

    fn engine() -> Arc<Engine> {
        let dir =
            std::env::temp_dir().join(format!("phoecord-test-transport-{}", std::process::id()));
        let plugin = Queues(Manifest::new("queues", "Queues", "1.0.0", String::new()));
        let chain = Chain::new(vec![Box::new(plugin)]);
        let engine = Engine::new(chain, Shared::new(dir), Config::default());
        engine.chain.set_enabled(vec!["queues".to_string()]);
        Arc::new(engine)
    }

    fn logger() -> (LogFn, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        let log: LogFn = Arc::new(move |l| sink.lock().unwrap().push(l.to_string()));
        (log, lines)
    }

    fn all() -> HookPlan {
        found(true, true, true, true)
    }

    fn found(send: bool, poll: bool, registry: bool, gateway_send: bool) -> HookPlan {
        let mut plan = HookPlan::none();
        if gateway_send {
            plan.gateway_send = Ok(Found {
                rva: 4,
                target: GatewaySend,
            });
        }
        if send {
            plan.send = Ok(Found {
                rva: 1,
                target: HttpSend,
            });
        }
        if poll {
            plan.poll = Ok(Found {
                rva: 2,
                target: HttpPoll,
            });
        }
        if registry {
            plan.inflate = Ok(Found {
                rva: 3,
                target: GatewayInflate,
            });
        }
        plan
    }

    #[test]
    fn what_a_request_queues_waits_for_the_next_dispatch() {
        let (log, lines) = logger();
        let transport = InProcessTransport::new(engine(), all(), log);
        assert_eq!(
            transport.request("GET", "/api/v9/ping", None),
            HttpAction::Pass
        );
        transport.response("GET", "/api/v9/ping", 200, b"{}");
        let step = transport.dispatch("TYPING_START", b"{}");
        assert_eq!(names(&step.before), ["FROM_REQUEST", "FROM_RESPONSE"]);
        assert_eq!(step.action, GatewayAction::Pass);
        assert!(step.after.is_empty());
        // There is no send hook, so the queued frame had nowhere to go.
        assert_eq!(transport.delivery().dropped().frames, 1);
        assert!(
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("no way to send"))
        );
    }

    #[test]
    fn the_send_detour_feeds_the_session_only_when_it_is_due() {
        use phoecord_transport::host::Network;
        let api = "https://discord.com/api/v9/users/@me";
        let auth = [Header {
            name: "authorization".into(),
            value: b"tok".to_vec(),
            sensitive: true,
        }];
        // Without a network, or with the transport off, nothing is read.
        let bare = InProcessTransport::new(engine(), all(), logger().0);
        assert!(!bare.wants_session(api));
        let net = Arc::new(LoaderNet::new());
        let off = InProcessTransport::new(engine(), found(false, true, true, true), logger().0)
            .with_network(net.clone());
        assert!(!off.wants_session(api));
        off.session(api, &auth);
        assert!(!net.signed_in());
        let on = InProcessTransport::new(engine(), all(), logger().0).with_network(net.clone());
        assert!(on.wants_session(api));
        assert!(!on.wants_session("https://cdn.discordapp.com/x.png"));
        on.session(api, &auth);
        assert!(net.signed_in());
        assert!(!on.wants_session(api), "a fresh session is not read again");
    }

    #[test]
    fn what_an_event_queues_follows_it() {
        let transport = InProcessTransport::new(engine(), all(), logger().0);
        transport.request("GET", "/ping", None);
        let step = transport.dispatch("MESSAGE_CREATE", b"{}");
        assert_eq!(names(&step.before), ["FROM_REQUEST"]);
        assert_eq!(names(&step.after), ["FROM_DISPATCH"]);
        assert!(transport.delivery().take_locals().is_empty());
    }

    #[test]
    fn a_response_hook_alone_stays_with_the_proxy() {
        let (log, lines) = logger();
        let transport = InProcessTransport::new(engine(), found(false, true, true, true), log);
        assert_eq!(
            transport.response("GET", "/ping", 200, b"{}"),
            HttpAction::Pass
        );
        assert!(transport.delivery().take_locals().is_empty());
        assert!(
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|l| l == "response: through the proxy (needs the request hook)")
        );
    }

    #[test]
    fn one_missing_locator_sends_the_whole_launch_through_the_proxy() {
        for missing in 0..4 {
            let f = [0, 1, 2, 3].map(|i| i != missing);
            let (log, lines) = logger();
            let transport = InProcessTransport::new(engine(), found(f[0], f[1], f[2], f[3]), log);
            assert!(!transport.is_active());
            assert!(!transport.wants_request("GET", "https://discord.com/api/v9/ping"));
            // Nothing reaches the engine: no plugin queues anything.
            assert_eq!(transport.request("GET", "/ping", None), HttpAction::Pass);
            let step = transport.dispatch("MESSAGE_CREATE", b"{}");
            assert!(step.before.is_empty() && step.after.is_empty());
            assert_eq!(transport.delivery().dropped(), Dropped::default());
            assert!(
                lines
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|l| l.starts_with("in-process transport off"))
            );
        }
        let transport = InProcessTransport::new(engine(), all(), logger().0);
        assert!(transport.is_active());
        assert!(transport.wants_request("GET", "https://discord.com/api/v9/ping"));
    }

    #[test]
    fn without_a_dispatch_hook_nothing_piles_up() {
        let (log, lines) = logger();
        let delivery = Delivery::new(engine(), false, None, log);
        delivery.gateway(vec![local("A")], vec![(3, Val::map())]);
        delivery.gateway(vec![local("B")], vec![(3, Val::map())]);
        assert!(delivery.take_locals().is_empty());
        assert_eq!(
            delivery.dropped(),
            Dropped {
                locals: 2,
                frames: 2
            }
        );
        let lines = lines.lock().unwrap();
        let logged = lines.iter().filter(|l| l.starts_with("no dispatch hook"));
        assert_eq!(logged.count(), 1, "the first drop only");
    }

    #[test]
    fn a_full_queue_loses_the_oldest() {
        let delivery = Delivery::new(engine(), true, None, logger().0);
        let extra = (0..MAX_QUEUED + 6)
            .map(|i| local(&format!("E{i}")))
            .collect();
        delivery.gateway(extra, Vec::new());
        let kept = delivery.take_locals();
        assert_eq!(kept.len(), MAX_QUEUED);
        assert_eq!(names(&kept)[0], "E6");
        assert_eq!(delivery.dropped().locals, 6);
    }

    #[test]
    fn queued_sends_pass_the_chain_and_reach_the_sink() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let seen = sent.clone();
        let sink: FrameSink = Box::new(move |frame| {
            let first = seen.lock().unwrap().is_empty();
            seen.lock().unwrap().push(frame);
            first
        });
        let delivery = Delivery::new(engine(), true, Some(sink), logger().0);
        delivery.gateway(Vec::new(), vec![(3, Val::map()), (4, Val::map())]);
        let ops: Vec<_> = sent
            .lock()
            .unwrap()
            .iter()
            .filter_map(|f| f.get("op").and_then(Val::as_i64))
            .collect();
        assert_eq!(ops, [3, 4]);
        // The sink refused the second one.
        assert_eq!(delivery.dropped().frames, 1);
    }

    #[test]
    fn the_clock_wakes_for_a_timer_set_while_it_sleeps() {
        let transport = InProcessTransport::new(engine(), all(), logger().0);
        let clock = Clock::start(transport.delivery().clone()).unwrap();
        // With no timer pending it sleeps without a time limit: only the change can wake it.
        thread::sleep(Duration::from_millis(100));
        transport.request("GET", "/later", None);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut fired = Vec::new();
        while fired.is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
            fired = transport.delivery().take_locals();
        }
        clock.stop();
        assert_eq!(names(&fired), ["FROM_TIMER"]);
    }
}
