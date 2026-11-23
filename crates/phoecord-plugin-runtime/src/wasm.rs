//! The WASM plugin backend: a [`WasmPlugin`] runs a `wasm32-wasip2` component built against
//! `wit/phoecord.wit` and implements [`Plugin`], so the [`crate::Chain`] treats it like a native one.
//!
//! Each plugin owns one wasmtime `Store` on its own worker thread. [`Plugin::on_event`] borrows
//! the host only for the duration of the call, which a `Store` (all of its data is `'static`) cannot
//! hold without `unsafe`; so the caller and the worker talk over channels instead. The worker sends
//! every `host` import the guest makes to the caller, who answers it with the real [`Host`].

use crate::package::{EventFilter, PackageManifest};
use phoecord_core::{
    Event, EventKind, Host, HostResponse, Layer, Manifest, Plugin, Val, Verdict,
    plugin::API_VERSION,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap, UpdateDeadline};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "plugin",
    });
}
use bindings::phoecord::plugin::{host as wit_host, types as wit};

type Error = wasmtime::Error;

/// Deepest `Val` nesting accepted from a guest. Discord payloads are far shallower.
const MAX_DEPTH: usize = 256;

// ---------------------------------------------------------------------------------------------
// `Val` <-> `tree`
// ---------------------------------------------------------------------------------------------

pub(crate) fn val_to_tree(val: &Val) -> wit::Tree {
    fn push(nodes: &mut Vec<wit::Node>, val: &Val) -> u32 {
        let idx = nodes.len() as u32;
        nodes.push(wit::Node::Null);
        let node = match val {
            Val::Null => wit::Node::Null,
            Val::Bool(b) => wit::Node::Bool(*b),
            Val::Int(n) => wit::Node::Int(*n),
            Val::Big(n) => wit::Node::Big(((*n >> 64) as i64, *n as u64)),
            Val::Float(f) => wit::Node::Float(*f),
            Val::Str(s) => wit::Node::String(s.clone()),
            Val::Atom(s) => wit::Node::Atom(s.clone()),
            Val::Bin(b) => wit::Node::Bin(b.clone()),
            Val::Charlist(b) => wit::Node::Charlist(b.clone()),
            Val::List(items) => wit::Node::List(items.iter().map(|v| push(nodes, v)).collect()),
            Val::Tuple(items) => wit::Node::Tuple(items.iter().map(|v| push(nodes, v)).collect()),
            Val::Map(pairs) => wit::Node::Map(
                pairs
                    .iter()
                    .map(|(k, v)| {
                        let k = push(nodes, k);
                        (k, push(nodes, v))
                    })
                    .collect(),
            ),
        };
        nodes[idx as usize] = node;
        idx
    }
    let mut nodes = Vec::new();
    push(&mut nodes, val);
    wit::Tree { nodes }
}

/// Rebuild a [`Val`] from a guest's tree. The arena is untrusted: indices are bounds-checked, every
/// node may be used once (no sharing, so no exponential blow-up) and depth is capped.
pub(crate) fn tree_to_val(tree: &wit::Tree) -> Result<Val, String> {
    struct Walk<'a> {
        nodes: &'a [wit::Node],
        used: Vec<bool>,
    }
    impl Walk<'_> {
        fn take(&mut self, idx: u32, depth: usize) -> Result<Val, String> {
            if depth > MAX_DEPTH {
                return Err("value tree is nested too deeply".into());
            }
            let i = idx as usize;
            let node = self.nodes.get(i).ok_or("value tree index out of range")?;
            if std::mem::replace(&mut self.used[i], true) {
                return Err("value tree node is used twice".into());
            }
            Ok(match node {
                wit::Node::Null => Val::Null,
                wit::Node::Bool(b) => Val::Bool(*b),
                wit::Node::Int(n) => Val::Int(*n),
                wit::Node::Big((hi, lo)) => Val::Big((i128::from(*hi) << 64) | i128::from(*lo)),
                wit::Node::Float(f) => Val::Float(*f),
                wit::Node::String(s) => Val::Str(s.clone()),
                wit::Node::Atom(s) => Val::Atom(s.clone()),
                wit::Node::Bin(b) => Val::Bin(b.clone()),
                wit::Node::Charlist(b) => Val::Charlist(b.clone()),
                wit::Node::List(ids) => Val::List(self.take_all(ids, depth)?),
                wit::Node::Tuple(ids) => Val::Tuple(self.take_all(ids, depth)?),
                wit::Node::Map(pairs) => Val::Map(
                    pairs
                        .iter()
                        .map(|(k, v)| Ok((self.take(*k, depth + 1)?, self.take(*v, depth + 1)?)))
                        .collect::<Result<_, String>>()?,
                ),
            })
        }
        fn take_all(&mut self, ids: &[u32], depth: usize) -> Result<Vec<Val>, String> {
            ids.iter().map(|id| self.take(*id, depth + 1)).collect()
        }
    }
    if tree.nodes.is_empty() {
        return Err("value tree is empty".into());
    }
    let mut walk = Walk {
        nodes: &tree.nodes,
        used: vec![false; tree.nodes.len()],
    };
    walk.take(0, 0)
}

// ---------------------------------------------------------------------------------------------
// Other type conversions
// ---------------------------------------------------------------------------------------------

fn manifest_from_wit(m: wit::Manifest) -> Manifest {
    Manifest {
        id: m.id,
        name: m.name,
        version: m.version,
        description: m.description,
        authors: m.authors,
        vencord: m.vencord,
        layers: m
            .layers
            .into_iter()
            .map(|l| match l {
                wit::Layer::Network => Layer::Network,
                wit::Layer::Theme => Layer::Theme,
                wit::Layer::Ui => Layer::Ui,
            })
            .collect(),
        api: m.api,
    }
}

fn kind_to_wit(kind: &EventKind) -> wit::EventKind {
    let rest = |method: &str, route: &str| wit::RestKind {
        method: method.into(),
        route: route.into(),
    };
    match kind {
        EventKind::RestRequest { method, route } => {
            wit::EventKind::RestRequest(rest(method, route))
        }
        EventKind::RestResponse { method, route } => {
            wit::EventKind::RestResponse(rest(method, route))
        }
        EventKind::Dispatch { name } => wit::EventKind::Dispatch((*name).into()),
        EventKind::Send { op } => wit::EventKind::Send(*op),
    }
}

fn event_to_wit(event: &Event<'_>) -> wit::Event {
    match event {
        Event::RestRequest {
            method,
            route,
            body,
        } => wit::Event::RestRequest(wit::RestRequest {
            method: (*method).into(),
            route: (*route).into(),
            body: body.as_deref().map(val_to_tree),
        }),
        Event::RestResponse {
            method,
            route,
            status,
            body,
        } => wit::Event::RestResponse(wit::RestResponse {
            method: (*method).into(),
            route: (*route).into(),
            status: *status,
            body: val_to_tree(body),
        }),
        Event::Dispatch { name, payload, .. } => wit::Event::Dispatch(wit::Dispatch {
            name: (*name).into(),
            payload: val_to_tree(payload),
        }),
        Event::Send { op, payload } => wit::Event::Send(wit::Send {
            op: *op,
            payload: val_to_tree(payload),
        }),
    }
}

/// What a guest's [`wit::Outcome`] means, with its trees already decoded.
struct Decoded {
    verdict: Verdict,
    payload: Option<Val>,
    extra: Vec<Val>,
}

fn decode_outcome(o: wit::Outcome) -> Result<Decoded, String> {
    let verdict = match o.verdict {
        wit::Verdict::Pass => Verdict::Pass,
        wit::Verdict::Changed => Verdict::Changed,
        wit::Verdict::Drop => Verdict::Drop,
        wit::Verdict::Reply(r) => Verdict::Reply {
            status: r.status,
            body: r.body.as_ref().map(tree_to_val).transpose()?,
        },
    };
    Ok(Decoded {
        verdict,
        payload: o.payload.as_ref().map(tree_to_val).transpose()?,
        extra: o.extra.iter().map(tree_to_val).collect::<Result<_, _>>()?,
    })
}

// ---------------------------------------------------------------------------------------------
// The worker thread and its messages
// ---------------------------------------------------------------------------------------------

/// A `host` import the guest made, with its arguments decoded.
enum HostCall {
    Setting(String),
    KvGet(String),
    KvSet(String, Val),
    SelfId,
    Notify(String, String),
    Log(String),
    EmitSend(i64, Val),
    GuildEmojis(u64),
    ChannelGuild(u64),
    Fetch(String, String, Option<Val>),
    Rest(String, String, Option<Val>),
}

enum HostReply {
    Val(Option<Val>),
    Unit,
    Id(Option<u64>),
    Ids(Vec<u64>),
    Response(Result<HostResponse, String>),
}

/// Caller to worker.
enum Down {
    Job(Box<Job>),
    Reply(HostReply),
}

/// Worker to caller.
enum Up {
    Host(HostCall),
    Done(Result<wit::Outcome, String>),
}

struct Job {
    kind: wit::EventKind,
    event: wit::Event,
}

/// The data of one plugin's `Store`.
struct State {
    up: Sender<Up>,
    down: Receiver<Down>,
    limits: StoreLimits,
    /// When the current job started, and how long it has spent waiting for the host since.
    started: Instant,
    host_wait: Duration,
    timeout: Duration,
    rights: Vec<Right>,
    /// Rights a call was refused for, so each is logged once.
    refused: Vec<Right>,
}

impl State {
    /// Ask the caller to run a host call and wait for its answer.
    fn call(&mut self, call: HostCall) -> Result<HostReply, String> {
        const GONE: &str = "the host went away";
        let asked = Instant::now();
        self.up.send(Up::Host(call)).map_err(|_| GONE)?;
        let reply = self.down.recv();
        // Time the host spends answering is not the guest's.
        self.host_wait += asked.elapsed();
        match reply {
            Ok(Down::Reply(r)) => Ok(r),
            _ => Err(GONE.into()),
        }
    }

    /// Check a right. A refusal is an error for the guest and a log line (once per right).
    fn allow(&mut self, right: Right) -> Result<(), String> {
        if self.rights.contains(&right) {
            return Ok(());
        }
        let msg = format!(
            "this plugin has no `{}` right (add it to `rights` in plugin.toml)",
            right.name()
        );
        if !self.refused.contains(&right) {
            self.refused.push(right);
            let _ = self.call_unit(HostCall::Log(msg.clone()));
        }
        Err(msg)
    }

    fn call_val(&mut self, call: HostCall) -> Option<Val> {
        match self.call(call) {
            Ok(HostReply::Val(v)) => v,
            _ => None,
        }
    }

    fn call_unit(&mut self, call: HostCall) -> Result<(), String> {
        self.call(call).map(|_| ())
    }

    fn call_response(&mut self, call: HostCall) -> Result<wit::HostResponse, String> {
        match self.call(call)? {
            HostReply::Response(r) => r.map(|r| wit::HostResponse {
                status: r.status,
                body: r.body.as_ref().map(val_to_tree),
            }),
            _ => Err("unexpected host reply".into()),
        }
    }
}

impl wit::Host for State {}

impl wit_host::Host for State {
    fn setting(&mut self, key: String) -> Option<wit::Tree> {
        self.call_val(HostCall::Setting(key))
            .as_ref()
            .map(val_to_tree)
    }
    fn kv_get(&mut self, key: String) -> Result<Option<wit::Tree>, String> {
        self.allow(Right::Kv)?;
        Ok(self
            .call_val(HostCall::KvGet(key))
            .as_ref()
            .map(val_to_tree))
    }
    fn kv_set(&mut self, key: String, value: wit::Tree) -> Result<(), String> {
        self.allow(Right::Kv)?;
        self.call_unit(HostCall::KvSet(key, tree_to_val(&value)?))
    }
    fn self_id(&mut self) -> Option<u64> {
        match self.call(HostCall::SelfId) {
            Ok(HostReply::Id(id)) => id,
            _ => None,
        }
    }
    fn notify(&mut self, title: String, body: String) -> Result<(), String> {
        self.allow(Right::Notify)?;
        self.call_unit(HostCall::Notify(title, body))
    }
    fn log(&mut self, message: String) {
        let _ = self.call_unit(HostCall::Log(message));
    }
    fn emit_send(&mut self, op: i64, payload: wit::Tree) -> Result<(), String> {
        self.allow(Right::EmitSend)?;
        self.call_unit(HostCall::EmitSend(op, tree_to_val(&payload)?))
    }
    fn guild_emojis(&mut self, guild_id: u64) -> Result<Vec<u64>, String> {
        self.allow(Right::Guilds)?;
        match self.call(HostCall::GuildEmojis(guild_id))? {
            HostReply::Ids(ids) => Ok(ids),
            _ => Ok(Vec::new()),
        }
    }
    fn channel_guild(&mut self, channel_id: u64) -> Result<Option<u64>, String> {
        self.allow(Right::Guilds)?;
        match self.call(HostCall::ChannelGuild(channel_id))? {
            HostReply::Id(id) => Ok(id),
            _ => Ok(None),
        }
    }
    fn fetch(
        &mut self,
        method: String,
        url: String,
        body: Option<wit::Tree>,
    ) -> Result<wit::HostResponse, String> {
        self.allow(Right::Fetch)?;
        let body = body.as_ref().map(tree_to_val).transpose()?;
        self.call_response(HostCall::Fetch(method, url, body))
    }
    fn rest(
        &mut self,
        method: String,
        route: String,
        body: Option<wit::Tree>,
    ) -> Result<wit::HostResponse, String> {
        self.allow(Right::Rest)?;
        let body = body.as_ref().map(tree_to_val).transpose()?;
        self.call_response(HostCall::Rest(method, route, body))
    }
}

/// Start the clock for one guest call.
fn begin_job(store: &mut Store<State>) {
    let s = store.data_mut();
    s.started = Instant::now();
    s.host_wait = Duration::ZERO;
    store.set_epoch_deadline(1);
}

/// Answer one host call with the real [`Host`].
fn serve(host: &mut dyn Host, call: HostCall) -> HostReply {
    match call {
        HostCall::Setting(k) => HostReply::Val(host.setting(&k)),
        HostCall::KvGet(k) => HostReply::Val(host.kv_get(&k)),
        HostCall::KvSet(k, v) => {
            host.kv_set(&k, v);
            HostReply::Unit
        }
        HostCall::SelfId => HostReply::Id(host.self_id()),
        HostCall::Notify(t, b) => {
            host.notify(&t, &b);
            HostReply::Unit
        }
        HostCall::Log(m) => {
            host.log(&m);
            HostReply::Unit
        }
        HostCall::EmitSend(op, p) => {
            host.emit_send(op, p);
            HostReply::Unit
        }
        HostCall::GuildEmojis(g) => HostReply::Ids(host.guild_emojis(g)),
        HostCall::ChannelGuild(c) => HostReply::Id(host.channel_guild(c)),
        HostCall::Fetch(m, u, b) => HostReply::Response(host.fetch(&m, &u, b.as_ref())),
        HostCall::Rest(m, r, b) => HostReply::Response(host.rest(&m, &r, b.as_ref())),
    }
}

// ---------------------------------------------------------------------------------------------
// Runtime and plugin
// ---------------------------------------------------------------------------------------------

/// Limits applied to every WASM plugin.
#[derive(Clone, Copy, Debug)]
pub struct WasmLimits {
    /// CPU time one event may take in the guest (time spent in host calls does not count).
    /// Enforced with epoch interruption, so it is accurate to about a millisecond.
    pub event_timeout: Duration,
    /// Most linear memory a guest may grow to, in bytes.
    pub memory: usize,
}

impl Default for WasmLimits {
    fn default() -> Self {
        WasmLimits {
            event_timeout: Duration::from_millis(5),
            memory: 32 << 20,
        }
    }
}

/// How often the engine's epoch advances. The event timeout is checked on every tick.
const TICK: Duration = Duration::from_millis(1);

/// One wasmtime engine shared by all WASM plugins, and the clock that drives epoch interruption.
pub struct WasmRuntime {
    engine: Engine,
    limits: WasmLimits,
    stop: Arc<AtomicBool>,
}

impl WasmRuntime {
    pub fn new() -> Result<Arc<WasmRuntime>, Error> {
        Self::with_limits(WasmLimits::default())
    }

    pub fn with_limits(limits: WasmLimits) -> Result<Arc<WasmRuntime>, Error> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.epoch_interruption(true);
        let engine = Engine::new(&config)?;
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (engine, stop) = (engine.clone(), stop.clone());
            std::thread::Builder::new()
                .name("phoecord-wasm-clock".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(TICK);
                        engine.increment_epoch();
                    }
                })?;
        }
        Ok(Arc::new(WasmRuntime {
            engine,
            limits,
            stop,
        }))
    }

    pub fn limits(&self) -> WasmLimits {
        self.limits
    }
}

impl Drop for WasmRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Worker {
    down: Option<Sender<Down>>,
    up: Receiver<Up>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the channel ends the worker's loop.
        self.down = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Worker {
    fn send(&self, msg: Down) -> Result<(), String> {
        self.down
            .as_ref()
            .and_then(|d| d.send(msg).ok())
            .ok_or_else(|| "the plugin worker is gone".to_string())
    }
}

/// A group of `Host` methods a plugin must be granted in its `plugin.toml` (`rights = [...]`).
/// `setting`, `self_id` and `log` need no right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Right {
    /// `kv_get`, `kv_set`.
    Kv,
    /// `notify`.
    Notify,
    /// `emit_send`.
    EmitSend,
    /// `guild_emojis`, `channel_guild`.
    Guilds,
    /// `fetch`.
    Fetch,
    /// `rest`.
    Rest,
}

impl Right {
    pub const ALL: [Right; 6] = [
        Right::Kv,
        Right::Notify,
        Right::EmitSend,
        Right::Guilds,
        Right::Fetch,
        Right::Rest,
    ];

    /// The name used in `plugin.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Right::Kv => "kv",
            Right::Notify => "notify",
            Right::EmitSend => "emit_send",
            Right::Guilds => "guilds",
            Right::Fetch => "fetch",
            Right::Rest => "rest",
        }
    }

    pub fn parse(name: &str) -> Option<Right> {
        Right::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// What a plugin is allowed and expected to be, beyond the component itself.
#[derive(Clone, Debug)]
pub struct Setup {
    /// Overrides the guest's own manifest (a package's `plugin.toml` wins).
    pub manifest: Option<Manifest>,
    /// Events delivered to the plugin; the rest never reach it.
    pub events: EventFilter,
    /// `Host` methods the plugin may call.
    pub rights: Vec<Right>,
}

impl Default for Setup {
    /// The guest's own manifest, every event and every right.
    fn default() -> Self {
        Setup {
            manifest: None,
            events: EventFilter::all(),
            rights: Right::ALL.to_vec(),
        }
    }
}

impl Setup {
    pub fn from_package(package: PackageManifest) -> Result<Setup, String> {
        let rights = package
            .rights
            .iter()
            .map(|name| {
                Right::parse(name).ok_or_else(|| {
                    let known: Vec<_> = Right::ALL.iter().map(|r| r.name()).collect();
                    format!("unknown right {name:?} (known: {})", known.join(", "))
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Setup {
            manifest: Some(package.manifest),
            events: package.events,
            rights,
        })
    }
}

/// A plugin that is a WASM component.
pub struct WasmPlugin {
    /// Keeps the epoch clock running for as long as the plugin lives.
    _runtime: Arc<WasmRuntime>,
    manifest: Manifest,
    events: EventFilter,
    worker: Mutex<Worker>,
    disabled: AtomicBool,
}

impl WasmPlugin {
    /// [`WasmPlugin::new`] with the guest's own manifest and every event.
    pub fn from_bytes(runtime: &Arc<WasmRuntime>, wasm: &[u8]) -> Result<WasmPlugin, Error> {
        WasmPlugin::new(runtime, wasm, Setup::default())
    }

    /// Compile `wasm` (a component) and start its worker.
    pub fn new(runtime: &Arc<WasmRuntime>, wasm: &[u8], setup: Setup) -> Result<WasmPlugin, Error> {
        let component = Component::new(&runtime.engine, wasm)?;
        let mut linker: Linker<State> = Linker::new(&runtime.engine);
        // The guest links `std`, which imports WASI. No WASI is provided: any call traps. These
        // stubs go in first and the real `host` interface shadows its own entry.
        linker.allow_shadowing(true);
        linker.define_unknown_imports_as_traps(&component)?;
        wit_host::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)?;
        let pre = bindings::PluginPre::new(linker.instantiate_pre(&component)?)?;
        let engine = runtime.engine.clone();
        let limits = runtime.limits;
        let rights = setup.rights.clone();

        let (down_tx, down_rx) = channel();
        let (up_tx, up_rx) = channel();
        let (ready_tx, ready_rx) = channel::<Result<wit::Manifest, String>>();
        let thread = std::thread::Builder::new()
            .name("phoecord-wasm".into())
            .spawn(move || {
                let mut store = Store::new(
                    &engine,
                    State {
                        up: up_tx,
                        down: down_rx,
                        limits: StoreLimitsBuilder::new()
                            .memory_size(limits.memory)
                            .trap_on_grow_failure(true)
                            .build(),
                        started: Instant::now(),
                        host_wait: Duration::ZERO,
                        timeout: limits.event_timeout,
                        rights,
                        refused: Vec::new(),
                    },
                );
                store.limiter(|s| &mut s.limits);
                // Runs on every epoch tick the guest is executing through: trap once the guest
                // itself has used up its time.
                store.epoch_deadline_callback(|ctx| {
                    let s = ctx.data();
                    if s.started.elapsed().saturating_sub(s.host_wait) > s.timeout {
                        Err(Trap::Interrupt.into())
                    } else {
                        Ok(UpdateDeadline::Continue(1))
                    }
                });
                begin_job(&mut store);
                let instance = match pre.instantiate(&mut store) {
                    Ok(i) => i,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("{e:#}")));
                        return;
                    }
                };
                begin_job(&mut store);
                match instance.call_manifest(&mut store) {
                    Ok(m) => {
                        let _ = ready_tx.send(Ok(m));
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("{e:#}")));
                        return;
                    }
                }
                while let Ok(Down::Job(job)) = store.data().down.recv() {
                    begin_job(&mut store);
                    let result = instance
                        .call_wants(&mut store, &job.kind)
                        .and_then(|wanted| {
                            if wanted {
                                instance.call_on_event(&mut store, &job.event)
                            } else {
                                Ok(wit::Outcome {
                                    verdict: wit::Verdict::Pass,
                                    payload: None,
                                    extra: Vec::new(),
                                })
                            }
                        })
                        .map_err(|e| format!("{e:#}"));
                    let failed = result.is_err();
                    if store.data().up.send(Up::Done(result)).is_err() || failed {
                        break;
                    }
                }
            })?;
        let guest_manifest = match ready_rx.recv() {
            Ok(Ok(m)) => manifest_from_wit(m),
            Ok(Err(e)) => return Err(Error::msg(e)),
            Err(_) => return Err(Error::msg("the plugin worker failed to start")),
        };
        let manifest = setup.manifest.unwrap_or(guest_manifest);
        if manifest.api != API_VERSION {
            return Err(Error::msg(format!(
                "plugin {} targets Plugin API v{}, this host speaks v{API_VERSION}",
                manifest.id, manifest.api
            )));
        }
        Ok(WasmPlugin {
            _runtime: runtime.clone(),
            manifest,
            events: setup.events,
            worker: Mutex::new(Worker {
                down: Some(down_tx),
                up: up_rx,
                thread: Some(thread),
            }),
            disabled: AtomicBool::new(false),
        })
    }

    /// Whether a fault (trap, limit, bad answer) switched this plugin off.
    pub fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    /// Send one job to the worker and serve its host calls until it answers.
    fn run_job(&self, host: &mut dyn Host, job: Job) -> Result<wit::Outcome, String> {
        let worker = self
            .worker
            .lock()
            .map_err(|_| "worker lock poisoned".to_string())?;
        worker.send(Down::Job(Box::new(job)))?;
        loop {
            match worker.up.recv() {
                Ok(Up::Host(call)) => worker.send(Down::Reply(serve(host, call)))?,
                Ok(Up::Done(result)) => return result,
                Err(_) => return Err("the plugin worker is gone".into()),
            }
        }
    }

    fn disable(&self, host: &mut dyn Host, why: &str) {
        self.disabled.store(true, Ordering::Relaxed);
        host.log(&format!(
            "plugin {} is disabled until restart: {why}",
            self.manifest.id
        ));
    }
}

impl Plugin for WasmPlugin {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        !self.is_disabled() && self.events.matches(event)
    }

    fn wants_gateway(&self) -> bool {
        !self.is_disabled() && self.events.wants_gateway()
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        if self.is_disabled() {
            return Verdict::Pass;
        }
        let kind = match &event {
            Event::RestRequest { method, route, .. } => EventKind::RestRequest { method, route },
            Event::RestResponse { method, route, .. } => EventKind::RestResponse { method, route },
            Event::Dispatch { name, .. } => EventKind::Dispatch { name },
            Event::Send { op, .. } => EventKind::Send { op: *op },
        };
        if !self.events.matches(&kind) {
            return Verdict::Pass;
        }
        let job = Job {
            kind: kind_to_wit(&kind),
            event: event_to_wit(&event),
        };
        let decoded = match self.run_job(host, job).and_then(decode_outcome) {
            Ok(d) => d,
            Err(why) => {
                self.disable(host, &why);
                return Verdict::Pass;
            }
        };
        let changed = matches!(decoded.verdict, Verdict::Changed);
        match event {
            Event::RestRequest {
                body: Some(body), ..
            } => {
                if let (true, Some(new)) = (changed, decoded.payload) {
                    *body = new;
                }
            }
            Event::Dispatch { payload, extra, .. } => {
                if let (true, Some(new)) = (changed, decoded.payload) {
                    *payload = new;
                }
                extra.extend(decoded.extra);
            }
            Event::RestResponse { body, .. } | Event::Send { payload: body, .. } => {
                if let (true, Some(new)) = (changed, decoded.payload) {
                    *body = new;
                }
            }
            _ => {}
        }
        decoded.verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_roundtrip_keeps_every_variant() {
        let v = Val::Map(vec![
            (
                Val::Str("a".into()),
                Val::List(vec![Val::Int(-1), Val::Null]),
            ),
            (
                Val::Atom("b".into()),
                Val::Tuple(vec![Val::Bool(true), Val::Float(1.5)]),
            ),
            (Val::Int(7), Val::Big(-(1i128 << 100) - 5)),
            (Val::Bin(vec![1, 2]), Val::Charlist(vec![104, 105])),
        ]);
        assert_eq!(tree_to_val(&val_to_tree(&v)).unwrap(), v);
    }

    #[test]
    fn hostile_trees_are_rejected() {
        let empty = wit::Tree { nodes: vec![] };
        assert!(tree_to_val(&empty).is_err());
        let out_of_range = wit::Tree {
            nodes: vec![wit::Node::List(vec![5])],
        };
        assert!(tree_to_val(&out_of_range).is_err());
        let shared = wit::Tree {
            nodes: vec![wit::Node::List(vec![1, 1]), wit::Node::Null],
        };
        assert!(tree_to_val(&shared).is_err());
        let cycle = wit::Tree {
            nodes: vec![wit::Node::List(vec![0])],
        };
        assert!(tree_to_val(&cycle).is_err());
        let deep = wit::Tree {
            nodes: (0..1000)
                .map(|i| wit::Node::List(vec![i + 1]))
                .chain([wit::Node::Null])
                .collect(),
        };
        assert!(tree_to_val(&deep).is_err());
    }
}
