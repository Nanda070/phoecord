//! The incoming gateway hook (task 2.2.3): an inline detour on the function that inflates one
//! gateway frame (`gateway_inflate` locator).
//!
//! The registry handlers the plan named first receive typed actions, not JSON (live, 2026-11-26),
//! so the hook sits one step earlier, where every frame is still its JSON text: the inflater
//! (`zstd-stream`) writes the frame through its first argument as a `Vec<u8>` (capacity, pointer,
//! length at `+0x8`; read live on Phoenix `0.1.0.2208`, private recon notes), taken from the
//! decompressor and owned by the caller from then on. The naked [`inflate_entry`] calls the
//! original, then [`on_inflated`] reads that text, and for a dispatch (`op` 0) asks the
//! [`DispatchSink`] (`Engine::on_dispatch` through the in-process transport).
//!
//! Verdicts are applied in the same buffer, so nothing is allocated: the buffer's capacity is the
//! decompressor's working size (256 KiB on this build), far above an ordinary event.
//! - **changed:** the frame is rebuilt with the new `d` (and the new `t` when a plugin renamed the
//!   event) and written over the old text, when it fits the capacity; a bigger one is logged and
//!   passes unchanged;
//! - **dropped:** the frame becomes a dispatch of [`DROPPED_EVENT`] with the same sequence
//!   number, which the client ignores while its session's `s` still advances.
//!
//! Server → client payloads plugins queue (`Host::emit_dispatch`) cannot be handed to the client
//! from here: the inflater returns one frame per call and this buffer holds one frame, so there
//! is no slot for an extra one. They are counted and logged once. Delivering them needs the
//! client's per-frame handler (RE, `docs/research/inprocess-dispatch.md`).

use super::abi::Memory;
use super::detour::ProcessMemory;
use super::gateway::{DROPPED_EVENT, GatewayAction, dropped, rebuild, split_dispatch};
use super::guard::{Breaker, LogFn};
use crate::hook::{HookError, HookSet};
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Offset of the frame `Vec<u8>` in the inflater's output.
pub const FRAME_VEC: usize = 0x8;
/// The biggest frame read: Discord's READY can reach a few MiB.
pub const MAX_FRAME: usize = 64 << 20;

/// Where the hook hands a dispatch: the in-process transport.
pub trait DispatchSink: Send + Sync {
    /// Whether any plugin reads the gateway; when not, frames are not even parsed.
    fn wants(&self) -> bool;
    /// The plugin chain's verdict on one dispatch (`d` as JSON), and how many server → client
    /// payloads plugins queued that could not be delivered.
    fn dispatch(&self, name: &str, d: &[u8]) -> (GatewayAction, usize);
}

struct State {
    sink: Box<dyn DispatchSink>,
    log: LogFn,
    breaker: Breaker,
}

static STATE: OnceLock<State> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Prepares the detour on the inflater at `target` (not active until the set is installed).
///
/// # Safety
/// `target` must be the entry of the client's gateway frame inflater, located by its signature,
/// whose first bytes are whole instructions with no RIP-relative operand.
pub unsafe fn prepare(
    set: &mut HookSet,
    target: usize,
    sink: Box<dyn DispatchSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        sink,
        log,
        breaker: Breaker::new("gateway_dispatch"),
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the gateway inflate detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `inflate_entry` passes every argument register
    // to the original and returns its return registers unchanged.
    let hook = unsafe { set.add(target as *const (), inflate_entry as *const ()) }?;
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    Ok(())
}

/// The detour: the original first (`rdx`, `r8`, `r9` untouched), then [`on_inflated`] with the
/// output pointer kept in `rsi` (callee-saved).
#[unsafe(naked)]
unsafe extern "C" fn inflate_entry() {
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
        hook = sym on_inflated,
        original = sym ORIGINAL,
    );
}

const LOG_TOO_BIG: u32 = 1;
const LOG_LOCALS: u32 = 2;
const LOG_ENCODE: u32 = 4;
const LOG_FIRST: u32 = 8;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

/// After the original inflater, on the client's thread.
extern "C" fn on_inflated(out: usize) {
    let Some(state) = STATE.get() else { return };
    if INSIDE.with(|c| c.replace(true)) {
        return;
    }
    state
        .breaker
        .guard(&state.log, || (), || observe(state, out));
    INSIDE.with(|c| c.set(false));
}

/// The frame `Vec<u8>` at `out`: (capacity, pointer, text), or `None` when it does not hold JSON
/// text.
pub fn read_frame(mem: &dyn Memory, out: usize) -> Option<(usize, usize, Vec<u8>)> {
    let at = out + FRAME_VEC;
    let (cap, ptr, len) = (mem.word(at)?, mem.word(at + 8)?, mem.word(at + 16)?);
    if ptr == 0 || len == 0 || len > cap || len > MAX_FRAME {
        return None;
    }
    let mut text = vec![0u8; len];
    (mem.read(ptr, &mut text) && text[0] == b'{').then_some((cap, ptr, text))
}

fn observe(state: &State, out: usize) {
    once(state, LOG_FIRST, || {
        "gateway_dispatch: first frame seen".into()
    });
    if !state.sink.wants() {
        return;
    }
    let Some((cap, ptr, text)) = read_frame(&ProcessMemory, out) else {
        return;
    };
    let Some((name, s, d)) = split_dispatch(&text) else {
        return;
    };
    if name == DROPPED_EVENT {
        return;
    }
    let (action, undelivered) = state.sink.dispatch(&name, &d);
    if undelivered > 0 {
        once(state, LOG_LOCALS, || {
            format!(
                "gateway_dispatch: {undelivered} payload(s) plugins queued for the client cannot be delivered in-process yet; dropped"
            )
        });
    }
    let new = match action {
        GatewayAction::Pass => return,
        GatewayAction::Drop => dropped(s),
        GatewayAction::Replace(d) => match rebuild(&name, s, &d) {
            Ok(f) => f,
            Err(e) => {
                return once(state, LOG_ENCODE, || {
                    format!("gateway_dispatch: {e}; event passes unchanged")
                });
            }
        },
        GatewayAction::Rename { name: to, data } => match rebuild(&to, s, &data) {
            Ok(f) => f,
            Err(e) => {
                return once(state, LOG_ENCODE, || {
                    format!("gateway_dispatch: {name} as {to}: {e}; event passes unchanged")
                });
            }
        },
    };
    if new.len() > cap {
        return once(state, LOG_TOO_BIG, || {
            format!(
                "gateway_dispatch: {name}: the changed frame ({} bytes) does not fit the buffer ({cap}); event passes unchanged",
                new.len()
            )
        });
    }
    // SAFETY: `ptr` / `cap` are the frame buffer the inflater just handed to its caller (read and
    // checked above: `len <= cap`); `new` fits the capacity, so writing it and the new length keeps
    // the `Vec` valid, and the caller frees it with the same capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(new.as_ptr(), ptr as *mut u8, new.len());
        ((out + FRAME_VEC + 16) as *mut usize).write(new.len());
    }
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

    fn out(cap: usize, ptr: usize, len: usize) -> Vec<u8> {
        [0usize, cap, ptr, len]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    #[test]
    fn reads_the_frame_vec() {
        let text = br#"{"t":"READY","s":1,"op":0,"d":{}}"#.to_vec();
        let mem = Fake(vec![
            (0x100, out(262144, 0x900, text.len())),
            (0x900, text.clone()),
        ]);
        assert_eq!(read_frame(&mem, 0x100), Some((262144, 0x900, text)));
    }

    #[test]
    fn not_json_or_wrong_words_are_none() {
        let mem = Fake(vec![
            (0x100, out(64, 0x900, 4)),
            (0x900, b"\x28\xb5\x2f\xfd".to_vec()),
        ]);
        assert_eq!(read_frame(&mem, 0x100), None, "still compressed bytes");
        let mem = Fake(vec![(0x100, out(2, 0x900, 4)), (0x900, b"{}{}".to_vec())]);
        assert_eq!(read_frame(&mem, 0x100), None, "length over capacity");
    }
}
