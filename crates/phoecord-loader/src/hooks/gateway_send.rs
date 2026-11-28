//! The outgoing gateway hook (task 2.2.4): an inline detour on the client's frame encoder.
//!
//! The encoder (`gateway_send` locator) takes the output in `rcx` and the typed command in `rdx`,
//! builds `{"op": …, "d": …}` and serialises it into a `String` it writes through `rcx` (capacity,
//! pointer, length); private recon notes, 2.1.6 second pass, read again on 2026-11-27. So the hook
//! runs **after** the original: the naked [`encode_entry`] calls it, then [`on_encoded`] reads the
//! frame text and hands it to the [`FrameSink`] (`Engine::on_send` through the in-process
//! transport).
//!
//! Verdicts:
//! - **changed:** the `String` is replaced. The client frees it, so the new text is allocated and the
//!   old freed with the client's `__rust_alloc` / `__rust_dealloc`; without them it is not applied;
//! - **dropped:** logged and not applied. The caller sends whatever the encoder returned, and an
//!   empty or invalid frame would close the connection.
//!
//! Heartbeat, identify and resume are not built here (recon notes), so plugins see presence, voice
//! state, member requests, guild subscriptions and the stream commands, not op 2, 6 or 40. The
//! `send_text` hook ([`super::gateway_send_text`]) sees all of them and can drop a frame; this
//! detour is the fallback for a build where that one is not located, and is not placed alongside
//! it.

use super::abi::Memory;
use super::detour::ProcessMemory;
use super::gateway::GatewayAction;
use super::guard::{Breaker, LogFn};
use crate::hook::{HookError, HookSet};
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// The longest frame read; Discord's limit for a client frame is 4096 bytes, so anything past
/// this is a wrong reading.
pub const MAX_FRAME: usize = 64 * 1024;

/// Where the hook hands a frame: the in-process transport.
pub trait FrameSink: Send + Sync {
    /// The plugin chain's verdict on one outgoing frame (JSON text).
    fn frame(&self, json: &[u8]) -> GatewayAction;
}

struct State {
    sink: Box<dyn FrameSink>,
    log: LogFn,
    breaker: Breaker,
    allocator: Option<(usize, usize)>,
}

static STATE: OnceLock<State> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Prepares the detour on the encoder at `target` (not active until the set is installed).
/// `allocator` is the client's `__rust_alloc` / `__rust_dealloc`, needed to change a frame.
///
/// # Safety
/// `target` must be the entry of the client's outgoing frame encoder, located by its signature,
/// whose first bytes are whole instructions with no RIP-relative operand; `allocator` must be the
/// client's allocator shims.
pub unsafe fn prepare(
    set: &mut HookSet,
    target: usize,
    allocator: Option<(usize, usize)>,
    sink: Box<dyn FrameSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        sink,
        log,
        breaker: Breaker::new("gateway_send"),
        allocator,
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the gateway send detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `encode_entry` passes every argument register
    // to the original and returns its return registers unchanged.
    let hook = unsafe { set.add(target as *const (), encode_entry as *const ()) }?;
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    Ok(())
}

/// The detour. `rsi` (callee-saved) keeps the output pointer across the original; `rdx`, `r8`,
/// `r9` reach it untouched. Alignment as in the `send` entry: two pushes and `0x38`.
#[unsafe(naked)]
unsafe extern "C" fn encode_entry() {
    std::arch::naked_asm!(
        "push rsi",
        "push rdi",
        "sub rsp, 0x38",
        "mov rsi, rcx",
        "call qword ptr [rip + {original}]",
        "mov [rsp + 0x20], rax",
        "mov [rsp + 0x28], rdx",
        "mov rcx, rsi",
        "call {hook}",
        "mov rax, [rsp + 0x20]",
        "mov rdx, [rsp + 0x28]",
        "add rsp, 0x38",
        "pop rdi",
        "pop rsi",
        "ret",
        hook = sym on_encoded,
        original = sym ORIGINAL,
    );
}

const LOG_READ: u32 = 1;
const LOG_DROP: u32 = 2;
const LOG_NO_ALLOC: u32 = 4;
const LOG_ALLOC: u32 = 8;
const LOG_FIRST: u32 = 16;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

/// After the original encoder, on the client's thread: `out` holds the frame `String`.
extern "C" fn on_encoded(out: usize) {
    let Some(state) = STATE.get() else { return };
    if INSIDE.with(|c| c.replace(true)) {
        return;
    }
    state
        .breaker
        .guard(&state.log, || (), || observe(state, out));
    INSIDE.with(|c| c.set(false));
}

/// The frame `String` at `out`: (capacity, pointer, text), or `None` when it does not look like
/// one.
pub fn read_frame(mem: &dyn Memory, out: usize) -> Option<(usize, usize, Vec<u8>)> {
    let (cap, ptr, len) = (mem.word(out)?, mem.word(out + 8)?, mem.word(out + 16)?);
    if ptr == 0 || len == 0 || len > cap || len > MAX_FRAME {
        return None;
    }
    let mut text = vec![0u8; len];
    mem.read(ptr, &mut text).then_some((cap, ptr, text))
}

fn observe(state: &State, out: usize) {
    once(state, LOG_FIRST, || "gateway_send: first frame seen".into());
    let Some((cap, ptr, text)) = read_frame(&ProcessMemory, out) else {
        return once(state, LOG_READ, || {
            "gateway_send: the encoder's output does not hold a frame string; frames pass untouched"
                .into()
        });
    };
    match state.sink.frame(&text) {
        // `Rename` comes from incoming dispatches only, never from an outgoing frame.
        GatewayAction::Pass | GatewayAction::Rename { .. } => {}
        GatewayAction::Drop => once(state, LOG_DROP, || {
            "gateway_send: dropping an outgoing frame is not applied in-process (the caller sends what the encoder returns); frame sent unchanged".into()
        }),
        GatewayAction::Replace(new) => replace(state, out, cap, ptr, new),
    }
}

/// Swaps the frame `String` for `new`, allocated by the client's allocator (the client frees it),
/// and frees the old one the same way.
fn replace(state: &State, out: usize, old_cap: usize, old_ptr: usize, new: Vec<u8>) {
    let Some((alloc, dealloc)) = state.allocator else {
        return once(state, LOG_NO_ALLOC, || {
            "gateway_send: no allocator for this build; changed frames are sent unchanged".into()
        });
    };
    if new.is_empty() {
        return;
    }
    // SAFETY: the client's allocator shims; `out` is the `String` the encoder just wrote, owned
    // by its caller.
    if !unsafe { put_string(alloc, dealloc, out, old_cap, old_ptr, &new) } {
        once(state, LOG_ALLOC, || {
            format!(
                "gateway_send: the client's allocator refused {} bytes; frame sent unchanged",
                new.len()
            )
        });
    }
}

/// Points the `String` record at `out` at a copy of `new` in the client's allocator and frees the
/// old buffer (`old_cap`, `old_ptr`) the same way. `false` when the allocator refused; the record
/// is then unchanged. Shared with the `send_text` hook.
///
/// # Safety
/// `alloc` / `dealloc` must be the client's `__rust_alloc(size, align)` and
/// `__rust_dealloc(ptr, size, align)` (scalar arguments: the same registers in the Rust and C ABIs
/// on Windows x64); `out` must be a live `String` record holding `old_cap` / `old_ptr`, owned by
/// the client code the hook runs in; `new` must not be empty.
pub unsafe fn put_string(
    alloc: usize,
    dealloc: usize,
    out: usize,
    old_cap: usize,
    old_ptr: usize,
    new: &[u8],
) -> bool {
    // SAFETY: see the contract above. The new buffer is filled before the `String` points at it;
    // the old one is freed with its capacity and alignment 1 once nothing refers to it.
    unsafe {
        let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = std::mem::transmute(alloc);
        let dealloc: unsafe extern "C" fn(*mut u8, usize, usize) = std::mem::transmute(dealloc);
        let p = alloc(new.len(), 1);
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(new.as_ptr(), p, new.len());
        let w = |i: usize| (out + 8 * i) as *mut usize;
        w(0).write(new.len());
        w(1).write(p as usize);
        w(2).write(new.len());
        if old_cap > 0 {
            dealloc(old_ptr as *mut u8, old_cap, 1);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<(usize, Vec<u8>)>);
    impl Memory for Fake {
        fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
            self.0.iter().any(|(base, bytes)| {
                let Some(off) = addr.checked_sub(*base) else {
                    return false;
                };
                match bytes.get(off..off + buf.len()) {
                    Some(src) => {
                        buf.copy_from_slice(src);
                        true
                    }
                    None => false,
                }
            })
        }
    }

    fn string(cap: usize, ptr: usize, len: usize) -> Vec<u8> {
        [cap, ptr, len]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    #[test]
    fn reads_the_frame_string() {
        let text = br#"{"op":3,"d":{}}"#.to_vec();
        let mem = Fake(vec![
            (0x100, string(32, 0x900, text.len())),
            (0x900, text.clone()),
        ]);
        assert_eq!(read_frame(&mem, 0x100), Some((32, 0x900, text)));
    }

    #[test]
    fn a_wrong_string_is_none() {
        let mem = Fake(vec![(0x100, string(4, 0x900, 15)), (0x900, vec![b'x'; 15])]);
        assert_eq!(read_frame(&mem, 0x100), None, "length over capacity");
        let mem = Fake(vec![(0x100, string(15, 0, 15))]);
        assert_eq!(read_frame(&mem, 0x100), None, "null pointer");
    }
}
