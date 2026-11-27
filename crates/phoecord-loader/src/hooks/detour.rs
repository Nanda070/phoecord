//! The REST detours (task 2.2.2): an inline detour on the transport's `send` and a swap of the
//! `poll` slot in the response future's vtable.
//!
//! **`send(self, request)`** takes the transport in `rcx` and the request in `rdx` and returns the
//! boxed response future (data pointer and vtable in `rax:rdx`). The naked [`send_entry`] calls
//! [`on_send`] with the arguments, then the original `send`, then [`on_sent`] with what it returned,
//! and hands the return registers back unchanged. `on_send` reads the request through
//! [`ProcessMemory`] (`ReadProcessMemory` on this process: a wrong offset is a refused read, not an
//! access violation) and asks the [`SendSink`]; `on_sent` remembers which future answers which
//! request, so the response side knows the method and URL.
//!
//! **`poll`** of that future writes `Poll<Result<Response, _>>` through its first argument. The
//! slot in the vtable (`.rdata`) is swapped for [`poll_entry`], which calls the original and then
//! [`on_poll`]: for a ready answer of a remembered future it reads status and body
//! ([`abi::read_response`]) and asks the sink.
//!
//! Verdicts (part 2):
//! - a changed **request** body becomes a `Bytes` with `bytes`'s static vtable over memory PhoeCord
//!   keeps for the life of the process (the old body is dropped through its own vtable), so no
//!   host allocation is needed;
//! - a changed **response** body is a `Vec<u8>` the client frees, so it is allocated and the old
//!   one freed through the host's `__rust_alloc` / `__rust_dealloc`; without them it is not applied;
//! - a **local reply** is logged and not applied: answering without the network needs a ready
//!   future of the client's own type (an open question for the owner).
//!
//! Every problem is logged once per kind and the call goes on untouched. Nothing here unwinds into
//! the client: the Rust parts run under the panic guard. The naked entries have no unwind data, so
//! a panic that unwinds out of the client's own `send` or `poll` would not walk through them
//! cleanly; the client's code does not unwind there in practice (it aborts on panic paths seen in
//! the recon notes), and the entries keep no state on the stack that matters after a fault.

use super::abi::{self, AbiError, Body, Memory};
use super::guard::{Breaker, LogFn};
use super::http::HttpAction;
use crate::hook::{HookError, HookSet};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ops::Range;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory,
    Memory::{PAGE_READWRITE, VirtualProtect},
    Threading::GetCurrentProcess,
};

/// Reads of this process's memory that fail instead of faulting.
pub struct ProcessMemory;

impl Memory for ProcessMemory {
    fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return buf.is_empty();
        }
        let mut done = 0usize;
        // SAFETY: `ReadProcessMemory` on our own process copies into `buf`, which is valid for
        // `buf.len()` bytes, and reports unreadable source pages as a failure instead of faulting.
        let ok = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                addr as *const c_void,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut done,
            )
        };
        ok != 0 && done == buf.len()
    }
}

/// Where the detours hand what they read: the in-process transport (2.2.7).
pub trait SendSink: Send + Sync {
    /// Whether any plugin looks at this request; when not, the body is never read.
    fn wants(&self, method: &str, url: &str) -> bool;
    /// The plugin chain's verdict on the request.
    fn request(&self, method: &str, url: &str, body: Option<&[u8]>) -> HttpAction;
    /// Whether any plugin looks at the answer to this request; when not, its future is not
    /// remembered and `poll` passes it untouched.
    fn wants_response(&self, method: &str, url: &str) -> bool;
    /// The plugin chain's verdict on the answer.
    fn response(&self, method: &str, url: &str, status: u16, body: &[u8]) -> HttpAction;
    /// Whether the headers of this request should be read for the plugins' network session
    /// (a Discord API URL, and no session yet or one due for a refresh); when not, they never are.
    fn wants_session(&self, _url: &str) -> bool {
        false
    }
    /// The headers of a request [`SendSink::wants_session`] asked for.
    fn session(&self, _url: &str, _headers: &[abi::Header]) {}
}

/// Absolute addresses of what the detours use, from the locators of this build.
pub struct Targets {
    /// Entry of `send`.
    pub send: usize,
    /// The response future's vtable; `None` leaves responses alone.
    pub poll_vtable: Option<usize>,
    /// `bytes`'s static vtable; `None` leaves request bodies unchanged.
    pub bytes_static: Option<usize>,
    /// `__rust_alloc` and `__rust_dealloc`; `None` leaves response bodies unchanged.
    pub allocator: Option<(usize, usize)>,
}

struct State {
    sink: Box<dyn SendSink>,
    image: Range<usize>,
    log: LogFn,
    send: Breaker,
    poll: Breaker,
    poll_vtable: Option<usize>,
    bytes_static: Option<usize>,
    allocator: Option<(usize, usize)>,
    /// Response future (data pointer) → the request it answers.
    futures: Mutex<HashMap<usize, (String, String)>>,
}

/// The most futures remembered at once; past it the map is cleared (futures dropped before their
/// answer would otherwise pile up).
const MAX_FUTURES: usize = 1024;

static STATE: OnceLock<State> = OnceLock::new();
/// Entry of the relocated original `send`.
static SEND_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// The original `poll` the swapped slot held, and the slot itself (for removal).
static POLL_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static POLL_SLOT: AtomicUsize = AtomicUsize::new(0);
/// Kinds of problems already logged (one bit each), so a busy client cannot flood the log.
static LOGGED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// Set while this thread runs plugin code from a detour: a request made from there must not
    /// re-enter the chain.
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    /// The request `on_send` read, for `on_sent` on the same thread.
    static PENDING: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Prepares the detour on `send` (not active until the set is installed) and remembers where the
/// `poll` slot is; [`install_poll`] swaps it after the set is installed. `image` is the client
/// module's address range.
///
/// # Safety
/// The addresses in `targets` must be what their locators found in this process: the entry of
/// the client's `send` (whose first bytes are whole instructions with no RIP-relative operand),
/// the response future's vtable, `bytes`'s static vtable and the allocator shims.
pub unsafe fn prepare(
    set: &mut HookSet,
    targets: Targets,
    image: Range<usize>,
    sink: Box<dyn SendSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        sink,
        image,
        log,
        send: Breaker::new("http_send"),
        poll: Breaker::new("http_poll"),
        poll_vtable: targets.poll_vtable,
        bytes_static: targets.bytes_static,
        allocator: targets.allocator,
        futures: Mutex::new(HashMap::new()),
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the send detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `send_entry` keeps the ABI of `send`: it
    // passes the argument registers to the original and returns its return registers unchanged.
    let hook = unsafe { set.add(targets.send as *const (), send_entry as *const ()) }?;
    // Stored before the set is installed, so the entry never calls through a null pointer.
    SEND_ORIGINAL.store(hook.original() as usize, Ordering::Release);
    Ok(())
}

/// Swaps the `poll` slot of the response future's vtable for [`poll_entry`]. Call after
/// [`prepare`]; a single aligned pointer write, so no thread needs to be suspended.
pub fn install_poll() -> Result<(), HookError> {
    let Some(vtable) = STATE.get().and_then(|s| s.poll_vtable) else {
        return Ok(());
    };
    let slot = vtable + crate::found::POLL_SLOT as usize;
    let original = ProcessMemory
        .word(slot)
        .ok_or_else(|| HookError::Detour(format!("cannot read the poll slot at {slot:#x}")))?;
    POLL_ORIGINAL.store(original, Ordering::Release);
    // SAFETY: `slot` is an aligned pointer-sized word inside the client's `.rdata` (the vtable
    // the locator found); it is made writable only for this one write.
    unsafe { write_slot(slot, poll_entry as *const () as usize) }?;
    POLL_SLOT.store(slot, Ordering::Release);
    Ok(())
}

/// Puts the original `poll` back (on unload).
pub fn uninstall_poll() {
    let slot = POLL_SLOT.swap(0, Ordering::AcqRel);
    if slot != 0 {
        // SAFETY: the same slot `install_poll` wrote, given back its original value.
        let _ = unsafe { write_slot(slot, POLL_ORIGINAL.load(Ordering::Acquire)) };
    }
}

/// # Safety
/// `slot` must be an aligned, pointer-sized word of this process's image.
unsafe fn write_slot(slot: usize, value: usize) -> Result<(), HookError> {
    let mut old = 0u32;
    // SAFETY: forwarded; the protection is restored right after the write.
    unsafe {
        if VirtualProtect(slot as *const c_void, 8, PAGE_READWRITE, &mut old) == 0 {
            return Err(HookError::Detour(format!("cannot unprotect {slot:#x}")));
        }
        (*(slot as *const AtomicUsize)).store(value, Ordering::Release);
        let mut tmp = 0u32;
        VirtualProtect(slot as *const c_void, 8, old, &mut tmp);
    }
    Ok(())
}

/// The `send` detour. On entry `rsp` is 8 mod 16; two pushes and `0x38` realign it and leave the
/// 32-byte shadow space plus two saved words. `rsi` / `rdi` (callee-saved) keep the arguments
/// across `on_send`; `r8` / `r9` are saved too, though `send` takes two arguments.
#[unsafe(naked)]
unsafe extern "C" fn send_entry() {
    std::arch::naked_asm!(
        "push rsi",
        "push rdi",
        "sub rsp, 0x38",
        "mov rsi, rcx",
        "mov rdi, rdx",
        "mov [rsp + 0x20], r8",
        "mov [rsp + 0x28], r9",
        "call {on_send}",
        "mov rcx, rsi",
        "mov rdx, rdi",
        "mov r8, [rsp + 0x20]",
        "mov r9, [rsp + 0x28]",
        "call qword ptr [rip + {original}]",
        "mov [rsp + 0x20], rax",
        "mov [rsp + 0x28], rdx",
        "mov rcx, rax",
        "call {on_sent}",
        "mov rax, [rsp + 0x20]",
        "mov rdx, [rsp + 0x28]",
        "add rsp, 0x38",
        "pop rdi",
        "pop rsi",
        "ret",
        on_send = sym on_send,
        on_sent = sym on_sent,
        original = sym SEND_ORIGINAL,
    );
}

/// The `poll` replacement: the original first (output pointer in `rcx`, the future in `rdx`, the
/// context in `r8`, untouched), then [`on_poll`] with the output and the future.
#[unsafe(naked)]
unsafe extern "C" fn poll_entry() {
    std::arch::naked_asm!(
        "push rsi",
        "push rdi",
        "sub rsp, 0x38",
        "mov rsi, rcx",
        "mov rdi, rdx",
        "call qword ptr [rip + {original}]",
        "mov [rsp + 0x20], rax",
        "mov [rsp + 0x28], rdx",
        "mov rcx, rsi",
        "mov rdx, rdi",
        "call {on_poll}",
        "mov rax, [rsp + 0x20]",
        "mov rdx, [rsp + 0x28]",
        "add rsp, 0x38",
        "pop rdi",
        "pop rsi",
        "ret",
        on_poll = sym on_poll,
        original = sym POLL_ORIGINAL,
    );
}

/// Runs `f` once per thread at a time, under `breaker`; never unwinds.
fn guarded(state: &State, breaker: &Breaker, f: impl FnOnce()) {
    if INSIDE.with(|c| c.replace(true)) {
        return;
    }
    breaker.guard(&state.log, || (), f);
    INSIDE.with(|c| c.set(false));
}

/// Before the original `send`, on Discord's thread.
extern "C" fn on_send(_transport: usize, request: usize) {
    let Some(state) = STATE.get() else { return };
    PENDING.with(|p| p.borrow_mut().take());
    guarded(state, &state.send, || observe_request(state, request));
}

/// After the original `send`: `future` / `vtable` are the boxed future it returned.
extern "C" fn on_sent(future: usize, vtable: usize) {
    let Some(state) = STATE.get() else { return };
    let Some(pending) = PENDING.with(|p| p.borrow_mut().take()) else {
        return;
    };
    if Some(vtable) != state.poll_vtable {
        return once(state, LOG_FUTURE, || {
            "http_poll: send did not return the response future the locator found; answers pass untouched".into()
        });
    }
    let Ok(mut map) = state.futures.lock() else {
        return;
    };
    if map.len() >= MAX_FUTURES {
        map.clear();
        once(state, LOG_FUTURES_FULL, || {
            format!("http_poll: more than {MAX_FUTURES} unanswered requests; forgot them")
        });
    }
    map.insert(future, pending);
}

/// After the original `poll`: `out` holds its output, `future` is the future polled.
extern "C" fn on_poll(out: usize, future: usize) {
    let Some(state) = STATE.get() else { return };
    guarded(state, &state.poll, || observe_response(state, out, future));
}

const LOG_ABI: u32 = 1;
const LOG_OPAQUE: u32 = 2;
const LOG_REPLY: u32 = 4;
const LOG_FUTURE: u32 = 8;
const LOG_FUTURES_FULL: u32 = 16;
const LOG_RESP_ABI: u32 = 32;
const LOG_NO_REQ_BODY: u32 = 64;
const LOG_NO_RESP_BODY: u32 = 128;
const LOG_ALLOC: u32 = 256;
const LOG_FIRST_SEND: u32 = 512;
const LOG_FIRST_POLL: u32 = 1024;
const LOG_HEADERS: u32 = 2048;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

fn observe_request(state: &State, request: usize) {
    once(state, LOG_FIRST_SEND, || {
        "http_send: first request seen".into()
    });
    let mem = ProcessMemory;
    let head = match abi::read_head(&mem, request) {
        Ok(h) => h,
        Err(e) => return abi_error(state, &e),
    };
    let route = super::http::route_of(&head.url);
    if state.sink.wants_session(&head.url) {
        match abi::read_headers(&mem, request, state.image.clone()) {
            Ok(headers) => state.sink.session(&head.url, &headers),
            Err(e) => once(state, LOG_HEADERS, || {
                format!(
                    "http_send: request headers not readable ({e}); plugins' rest stays signed out"
                )
            }),
        }
    }
    if state.poll_vtable.is_some() && state.sink.wants_response(&head.method, &head.url) {
        PENDING.with(|p| *p.borrow_mut() = Some((head.method.clone(), head.url.clone())));
    }
    if !state.sink.wants(&head.method, &head.url) {
        return;
    }
    let body = match abi::read_body(&mem, request, state.image.clone()) {
        Body::None => None,
        Body::Bytes(b) => Some(b),
        Body::Opaque => {
            once(state, LOG_OPAQUE, || {
                format!(
                    "http_send: {} {route}: body not readable in-process, plugins see none",
                    head.method
                )
            });
            None
        }
    };
    match state.sink.request(&head.method, &head.url, body.as_deref()) {
        HttpAction::Pass => {}
        HttpAction::ReplaceBody(new) => {
            if body.is_none() {
                return once(state, LOG_NO_REQ_BODY, || {
                    format!(
                        "http_send: {} {route}: a body can only be changed, not added; request sent unchanged",
                        head.method
                    )
                });
            }
            replace_request_body(state, request, new, &head.method, &route);
        }
        HttpAction::Reply { .. } => once(state, LOG_REPLY, || {
            format!(
                "http_send: {} {route}: a local reply is not applied in-process yet; request sent unchanged",
                head.method
            )
        }),
    }
}

/// Swaps the request's `Bytes` for one over `new` with the static vtable. The new buffer is kept
/// for the life of the process (a static `Bytes` never frees, and the client may clone it); the old
/// body is dropped through its own vtable once nothing points at it any more.
fn replace_request_body(state: &State, request: usize, new: Vec<u8>, method: &str, route: &str) {
    let Some(static_vt) = state.bytes_static else {
        return once(state, LOG_NO_REQ_BODY, || {
            format!(
                "http_send: {method} {route}: no static Bytes vtable for this build; request sent unchanged"
            )
        });
    };
    let mem = ProcessMemory;
    let at = |i: usize| request + abi::BODY_BYTES + 8 * i;
    let (Some(old_vt), Some(old_ptr), Some(old_len), Some(old_data)) = (
        mem.word(at(abi::BYTES_VTABLE)),
        mem.word(at(abi::BYTES_PTR)),
        mem.word(at(abi::BYTES_LEN)),
        mem.word(at(abi::BYTES_DATA)),
    ) else {
        return;
    };
    let Some(old_drop) = mem
        .word(old_vt + DROP_SLOT)
        .filter(|f| state.image.contains(f))
    else {
        return;
    };
    let new: &'static [u8] = Box::leak(new.into_boxed_slice());
    // SAFETY: `request` is the request `send` was called with, alive and owned by the caller for
    // the duration of this call; its `Bytes` words were just read from these addresses. The new
    // words describe `new`, which lives for the rest of the process, with `bytes`'s static vtable
    // (which never frees). The old body is then dropped exactly once through its own vtable's
    // `drop(&mut data, ptr, len)` (three scalar arguments: the same registers in the Rust and C
    // ABIs on Windows x64), with a copy of its `data` word, as `Bytes::drop` would.
    unsafe {
        let w = |i: usize| at(i) as *mut usize;
        w(abi::BYTES_VTABLE).write(static_vt);
        w(abi::BYTES_PTR).write(new.as_ptr() as usize);
        w(abi::BYTES_LEN).write(new.len());
        w(abi::BYTES_DATA).write(0);
        let drop: unsafe extern "C" fn(*mut usize, *const u8, usize) =
            std::mem::transmute(old_drop);
        let mut data = old_data;
        drop(&mut data, old_ptr as *const u8, old_len);
    }
}

/// Offset of `drop` in a `bytes` vtable: clone, to_vec, to_mut, is_unique, drop.
const DROP_SLOT: usize = 0x20;

fn observe_response(state: &State, out: usize, future: usize) {
    once(state, LOG_FIRST_POLL, || {
        "http_poll: first poll seen".into()
    });
    let mem = ProcessMemory;
    let Some(read) = abi::read_response(&mem, out) else {
        return;
    };
    let Some((method, url)) = state
        .futures
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&future))
    else {
        return;
    };
    let route = super::http::route_of(&url);
    let resp = match read {
        Ok(r) => r,
        Err(e) => {
            return once(state, LOG_RESP_ABI, || {
                format!("http_poll: {method} {route}: {e}; answers pass untouched")
            });
        }
    };
    match state.sink.response(&method, &url, resp.status, &resp.body) {
        HttpAction::Pass => {}
        HttpAction::ReplaceBody(new) => {
            replace_response_body(state, out, &resp, new, &method, &route)
        }
        HttpAction::Reply { .. } => once(state, LOG_REPLY, || {
            format!(
                "http_poll: {method} {route}: a reply to an answer is not supported; answer unchanged"
            )
        }),
    }
}

/// Swaps the answer's `Vec<u8>` for one with `new`, allocated by the client's allocator (the
/// client frees it), and frees the old one the same way.
fn replace_response_body(
    state: &State,
    out: usize,
    old: &abi::Response,
    new: Vec<u8>,
    method: &str,
    route: &str,
) {
    let Some((alloc, dealloc)) = state.allocator else {
        return once(state, LOG_NO_RESP_BODY, || {
            format!("http_poll: {method} {route}: no allocator for this build; answer unchanged")
        });
    };
    // SAFETY: `alloc` / `dealloc` are the client's `__rust_alloc(size, align) -> *mut u8` and
    // `__rust_dealloc(ptr, size, align)` (scalar arguments: the same registers in the Rust and C
    // ABIs on Windows x64). The new buffer is filled before the output's words point at it; the
    // old one is freed with the capacity and alignment (1) it was allocated with, after nothing in
    // the output refers to it. `out` is the output `poll` just wrote, owned by its caller.
    unsafe {
        let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = std::mem::transmute(alloc);
        let dealloc: unsafe extern "C" fn(*mut u8, usize, usize) = std::mem::transmute(dealloc);
        let (cap, ptr) = if new.is_empty() {
            (0, 1usize)
        } else {
            let p = alloc(new.len(), 1);
            if p.is_null() {
                return once(state, LOG_ALLOC, || {
                    format!(
                        "http_poll: {method} {route}: the client's allocator refused {} bytes; answer unchanged",
                        new.len()
                    )
                });
            }
            std::ptr::copy_nonoverlapping(new.as_ptr(), p, new.len());
            (new.len(), p as usize)
        };
        let w = |i: usize| (out + abi::RESP_BODY + 8 * i) as *mut usize;
        w(0).write(cap);
        w(1).write(ptr);
        w(2).write(new.len());
        if old.cap > 0 {
            dealloc(old.ptr as *mut u8, old.cap, 1);
        }
    }
}

fn abi_error(state: &State, e: &AbiError) {
    once(state, LOG_ABI, || {
        format!("http_send: {e}; requests pass untouched (layout of this build differs?)")
    });
}
