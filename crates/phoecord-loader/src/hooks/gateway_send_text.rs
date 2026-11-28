//! The outgoing gateway hook on the last step before the socket (task 2.2.4): an inline detour on
//! the entry of the connection loop's `send_text` (the `gateway_send_text` locator).
//!
//! Every client → server text frame ends in that one function: the encoder's frames (presence,
//! voice state, requests) and the frames built elsewhere, `IDENTIFY` (op 2), `RESUME` (op 6) and the
//! heartbeat (op 40 on 0.1.0.2208). It takes the output in `rcx` (32 bytes, sret), the session in
//! `rdx` and the frame `String` record (capacity, pointer, length) in `r8`, and hands the `String`
//! to the websocket layer, which frees it (private recon notes, 2.2.4 send path, 2026-11-27). So
//! the hook runs **before** the original:
//!
//! - **pass:** the original runs;
//! - **changed:** the record gets a new `String` from the client's `__rust_alloc`, the old buffer
//!   is freed with `__rust_dealloc`, then the original runs;
//! - **dropped:** the `String` is freed with `__rust_dealloc`, word 0 of the output is set to
//!   `-1`, and the function returns without the original. The loop takes word 0 `== -1` as
//!   "nothing to send"; the send budget lives in the loop, so it is not touched.
//!
//! **Timing.** `IDENTIFY` leaves about 0.35 s after start; the worker has its hooks in after about
//! 1.1 s and the plugin engine later. So in an in-process launch this hook goes in from `DllMain`
//! ([`early_install`], like `client_state`), as a pass-through. The worker attaches the plugin
//! chain ([`attach`]) once the engine runs. An `IDENTIFY` that comes first waits for it on the
//! client's thread, at most [`HOLD`]; every other frame before that passes untouched. When the
//! early install did not happen, the worker places the hook itself ([`prepare`]); `IDENTIFY` is
//! then gone already. The encoder detour (`gateway_send`) is not placed while this one is on, so
//! `Engine::on_send` runs once per frame.

use std::time::Duration;

/// How long an `IDENTIFY` that arrives before the plugin engine waits for it.
pub const HOLD: Duration = Duration::from_secs(3);

/// The `op` of a gateway frame's JSON text, read from its first key without parsing the rest.
/// The client writes `{"op":N,…}` (serde, `op` first); anything else is `None`.
pub fn op_of(frame: &[u8]) -> Option<u8> {
    let rest = frame.strip_prefix(br#"{"op":"#)?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()
}

/// Whether a frame that arrives before the plugin engine waits for it: only `IDENTIFY`.
pub fn waits_for_engine(op: Option<u8>) -> bool {
    op == Some(2)
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub use imp::*;

#[cfg(all(windows, target_arch = "x86_64"))]
mod imp {
    use super::{HOLD, op_of, waits_for_engine};
    use crate::aob::Pattern;
    use crate::found::GATEWAY_SEND_TEXT;
    use crate::hook::{HookError, HookSet};
    use crate::hooks::detour::ProcessMemory;
    use crate::hooks::early;
    use crate::hooks::gateway::GatewayAction;
    use crate::hooks::gateway_send::{FrameSink, put_string, read_frame};
    use crate::hooks::guard::{Breaker, LogFn};
    use crate::signatures::Signature;
    use std::cell::Cell;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
    use std::time::Instant;

    /// The plugin chain, once the worker has it.
    struct Attached {
        sink: Box<dyn FrameSink>,
        log: LogFn,
        breaker: Breaker,
        allocator: Option<(usize, usize)>,
    }

    static ATTACHED: OnceLock<Attached> = OnceLock::new();
    /// Off: a plain pass-through (the hashed table disagrees, or the hook was never wanted).
    static ENABLED: AtomicBool = AtomicBool::new(true);
    /// The worker will not attach (the in-process start failed): nothing waits any more.
    static GAVE_UP: AtomicBool = AtomicBool::new(false);
    static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
    /// RVA the hook went in at (early or by the worker); `0` when it is not in.
    static INSTALLED_RVA: AtomicU32 = AtomicU32::new(0);
    static EARLY: AtomicBool = AtomicBool::new(false);
    static STATUS: OnceLock<String> = OnceLock::new();
    static PLACED_BY: OnceLock<(Pattern, Option<usize>)> = OnceLock::new();
    /// How long the first `IDENTIFY` waited for the engine, in ms (`u32::MAX`: it did not wait).
    static HELD_MS: AtomicU32 = AtomicU32::new(u32::MAX);
    /// Ops already logged once (bit `op % 64`), and the one-off log lines.
    static SEEN_OPS: AtomicU64 = AtomicU64::new(0);
    static LOGGED: AtomicU32 = AtomicU32::new(0);
    static FRAMES: AtomicU32 = AtomicU32::new(0);
    static DROPPED: AtomicU32 = AtomicU32::new(0);
    static REPLACED: AtomicU32 = AtomicU32::new(0);

    thread_local! {
        static INSIDE: Cell<bool> = const { Cell::new(false) };
    }

    /// Installs the detour from `DllMain` when `phoecord launch` chose in-process; never panics
    /// out. The result is kept for [`status`].
    pub fn early_install() {
        let in_process = crate::choice::Launched::from_env()
            .is_some_and(|l| l.mode == crate::choice::Mode::InProcess);
        if !in_process {
            return;
        }
        let started = Instant::now();
        let line = match std::panic::catch_unwind(try_install) {
            Ok(Ok(rva)) => {
                INSTALLED_RVA.store(rva, Ordering::Release);
                EARLY.store(true, Ordering::Release);
                format!(
                    "send_text hook in at rva {rva:#x} before the client started ({} ms)",
                    started.elapsed().as_millis()
                )
            }
            Ok(Err(e)) => format!("send_text hook not placed early: {e}"),
            Err(_) => "send_text hook not placed early: the install panicked".into(),
        };
        let _ = STATUS.set(line);
    }

    fn try_install() -> Result<u32, String> {
        early::with_scan(|base, scan| {
            let found = scan.gateway_send_text().map_err(|e| e.to_string())?;
            if let Some(p) = early::placed_by(scan.sigs, GATEWAY_SEND_TEXT) {
                let _ = PLACED_BY.set(p);
            }
            let mut set = crate::hook::global()
                .lock()
                .map_err(|_| "the hook set is poisoned")?;
            // SAFETY: the unique match of the `gateway_send_text` locator in this image: the
            // entry of `send_text`, whose first bytes are pushes, `sub rsp` and `lea rbp` (no
            // RIP-relative operand); `entry` passes the argument registers on unchanged.
            unsafe { place(&mut set, base + found.rva as usize) }.map_err(|e| e.to_string())?;
            set.install_all().map_err(|e| e.to_string())?;
            Ok(found.rva)
        })
    }

    /// Adds the detour at `target` to `set` (not active until the set is installed).
    ///
    /// # Safety
    /// `target` must be the entry of `send_text`, located by its signature.
    unsafe fn place(set: &mut HookSet, target: usize) -> Result<(), HookError> {
        // SAFETY: forwarded from the caller.
        let hook = unsafe { set.add(target as *const (), entry as *const ()) }?;
        ORIGINAL.store(hook.original() as usize, Ordering::Release);
        Ok(())
    }

    /// The worker's placement when the early one did not happen: adds the detour to `set` and
    /// attaches the plugin chain.
    ///
    /// # Safety
    /// `target` must be the entry of `send_text` in this image, located by its signature, with
    /// `rva` its RVA; `allocator` must be the client's allocator shims.
    pub unsafe fn prepare(
        set: &mut HookSet,
        target: usize,
        rva: u32,
        allocator: Option<(usize, usize)>,
        sink: Box<dyn FrameSink>,
        log: LogFn,
    ) -> Result<(), HookError> {
        if installed() {
            return Err(HookError::Detour(
                "the send_text detour is already placed".into(),
            ));
        }
        // SAFETY: forwarded from the caller.
        unsafe { place(set, target) }?;
        INSTALLED_RVA.store(rva, Ordering::Release);
        attach(allocator, sink, log);
        Ok(())
    }

    /// Hands the hook the plugin chain; frames from now on pass `Engine::on_send`.
    pub fn attach(allocator: Option<(usize, usize)>, sink: Box<dyn FrameSink>, log: LogFn) {
        let _ = ATTACHED.set(Attached {
            sink,
            log,
            breaker: Breaker::new("gateway_send_text"),
            allocator,
        });
    }

    /// The worker will not attach: a waiting `IDENTIFY` goes on at once, and so do later ones.
    pub fn give_up() {
        GAVE_UP.store(true, Ordering::Release);
    }

    /// Whether the hook is in (early or by the worker).
    pub fn installed() -> bool {
        INSTALLED_RVA.load(Ordering::Acquire) != 0
    }

    /// Whether it went in from `DllMain`.
    pub fn early() -> bool {
        EARLY.load(Ordering::Acquire)
    }

    /// RVA the hook is at.
    pub fn rva() -> u32 {
        INSTALLED_RVA.load(Ordering::Acquire)
    }

    /// What [`early_install`] did.
    pub fn status() -> Option<&'static str> {
        STATUS.get().map(String::as_str)
    }

    /// The worker's check of the early placement with the hashed build's table: `Ok` when it has
    /// the same `gateway_send_text` signature, else the hook passes everything through.
    pub fn confirm(sigs: &[Signature]) -> Result<u32, String> {
        if !early() {
            return Err("not installed".into());
        }
        if early::same_signature(sigs, GATEWAY_SEND_TEXT, PLACED_BY.get()) {
            return Ok(rva());
        }
        ENABLED.store(false, Ordering::Release);
        give_up();
        Err(
            "the hashed build's table has another gateway_send_text signature; hook passes through"
                .into(),
        )
    }

    /// Frames seen, dropped and replaced so far.
    pub fn counts() -> (u32, u32, u32) {
        (
            FRAMES.load(Ordering::Relaxed),
            DROPPED.load(Ordering::Relaxed),
            REPLACED.load(Ordering::Relaxed),
        )
    }

    /// The detour, on entry of `send_text`: `rcx` output, `rdx` session, `r8` the `String`
    /// record. On entry `rsp` is 8 mod 16; three pushes and `0x40` realign it and leave the
    /// shadow space plus a slot for `r9`. `rbx` / `rsi` / `rdi` (callee-saved) keep `rcx` / `rdx`
    /// / `r8`. Pass: restore everything and jump to the original, which returns to the client
    /// itself. Skip: return the output pointer, as the original does.
    #[unsafe(naked)]
    unsafe extern "C" fn entry() {
        std::arch::naked_asm!(
            "push rbx",
            "push rsi",
            "push rdi",
            "sub rsp, 0x40",
            "mov rbx, rcx",
            "mov rsi, rdx",
            "mov rdi, r8",
            "mov [rsp + 0x20], r9",
            "mov rdx, r8",
            "call {before}",
            "test rax, rax",
            "jnz 2f",
            "mov rcx, rbx",
            "mov rdx, rsi",
            "mov r8, rdi",
            "mov r9, [rsp + 0x20]",
            "add rsp, 0x40",
            "pop rdi",
            "pop rsi",
            "pop rbx",
            "jmp qword ptr [rip + {original}]",
            "2:",
            "mov rax, rbx",
            "add rsp, 0x40",
            "pop rdi",
            "pop rsi",
            "pop rbx",
            "ret",
            before = sym before,
            original = sym ORIGINAL,
        );
    }

    /// Call the original.
    const PASS: usize = 0;
    /// Skipped: the output says "nothing to send".
    const SKIPPED: usize = 1;

    const LOG_READ: u32 = 1;
    const LOG_NO_ALLOC: u32 = 2;
    const LOG_ALLOC: u32 = 4;
    const LOG_HELD: u32 = 8;
    const LOG_FIRST_DROP: u32 = 16;

    fn once(a: &Attached, kind: u32, line: impl FnOnce() -> String) {
        if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
            (a.log)(&line());
        }
    }

    /// Before the original, on the client's thread. Returns [`PASS`] or [`SKIPPED`].
    extern "C" fn before(out: usize, record: usize) -> usize {
        if !ENABLED.load(Ordering::Acquire) || out == 0 || record == 0 {
            return PASS;
        }
        if INSIDE.with(|c| c.replace(true)) {
            return PASS;
        }
        let verdict = std::panic::catch_unwind(|| decide(out, record)).unwrap_or(PASS);
        INSIDE.with(|c| c.set(false));
        verdict
    }

    fn decide(out: usize, record: usize) -> usize {
        let attached = match ATTACHED.get() {
            Some(a) => a,
            None => match wait_if_identify(record) {
                Some(a) => a,
                None => return PASS,
            },
        };
        attached
            .breaker
            .guard(&attached.log, || PASS, || observe(attached, out, record))
    }

    /// No plugin chain yet: an `IDENTIFY` waits for it, at most [`HOLD`]; anything else passes.
    fn wait_if_identify(record: usize) -> Option<&'static Attached> {
        if GAVE_UP.load(Ordering::Acquire) {
            return None;
        }
        let (_, _, text) = read_frame(&ProcessMemory, record)?;
        if !waits_for_engine(op_of(&text)) {
            return None;
        }
        let started = Instant::now();
        while started.elapsed() < HOLD && !GAVE_UP.load(Ordering::Acquire) {
            if let Some(a) = ATTACHED.get() {
                let ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
                let _ = HELD_MS.compare_exchange(u32::MAX, ms, Ordering::AcqRel, Ordering::Relaxed);
                return Some(a);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    fn observe(a: &Attached, out: usize, record: usize) -> usize {
        let Some((cap, ptr, text)) = read_frame(&ProcessMemory, record) else {
            once(a, LOG_READ, || {
                "send_text: the record does not hold a frame string; frames pass untouched".into()
            });
            return PASS;
        };
        FRAMES.fetch_add(1, Ordering::Relaxed);
        let op = op_of(&text);
        if let Some(op) = op {
            let bit = 1u64 << (op % 64);
            if SEEN_OPS.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
                (a.log)(&format!(
                    "send_text: first op {op} frame ({} bytes) passes on_send",
                    text.len()
                ));
            }
        }
        if op == Some(2) {
            let held = HELD_MS.load(Ordering::Acquire);
            if held != u32::MAX {
                once(a, LOG_HELD, || {
                    format!("send_text: IDENTIFY waited {held} ms for the plugin engine")
                });
            }
        }
        match a.sink.frame(&text) {
            // `Rename` comes from incoming dispatches only, never from an outgoing frame.
            GatewayAction::Pass | GatewayAction::Rename { .. } => PASS,
            GatewayAction::Drop => drop_frame(a, out, record, cap, ptr, op),
            GatewayAction::Replace(new) => {
                replace(a, record, cap, ptr, &new);
                PASS
            }
        }
    }

    /// Frees the frame and makes the output "nothing to send".
    fn drop_frame(
        a: &Attached,
        out: usize,
        record: usize,
        cap: usize,
        ptr: usize,
        op: Option<u8>,
    ) -> usize {
        let Some((_, dealloc)) = a.allocator else {
            once(a, LOG_NO_ALLOC, || {
                "send_text: no allocator for this build; dropped and changed frames are sent unchanged".into()
            });
            return PASS;
        };
        // SAFETY: `dealloc` is the client's `__rust_dealloc(ptr, size, align)`; `ptr` / `cap` are
        // the frame `String`'s buffer, which `send_text` would hand to the websocket layer to
        // free: skipping the call makes the hook its only owner, so it is freed once. The record
        // is cleared to an empty `String` in case anyone reads it again. `out` is the 32-byte
        // output the caller passed; word 0 `-1` is the loop's empty value.
        unsafe {
            let dealloc: unsafe extern "C" fn(*mut u8, usize, usize) = std::mem::transmute(dealloc);
            if cap > 0 {
                dealloc(ptr as *mut u8, cap, 1);
            }
            let w = |i: usize| (record + 8 * i) as *mut usize;
            w(0).write(0);
            w(1).write(1);
            w(2).write(0);
            (out as *mut usize).write(usize::MAX);
        }
        DROPPED.fetch_add(1, Ordering::Relaxed);
        once(a, LOG_FIRST_DROP, || match op {
            Some(op) => format!("send_text: first frame dropped (op {op}); not sent"),
            None => "send_text: first frame dropped; not sent".into(),
        });
        SKIPPED
    }

    fn replace(a: &Attached, record: usize, cap: usize, ptr: usize, new: &[u8]) {
        let Some((alloc, dealloc)) = a.allocator else {
            return once(a, LOG_NO_ALLOC, || {
                "send_text: no allocator for this build; dropped and changed frames are sent unchanged".into()
            });
        };
        if new.is_empty() {
            return;
        }
        // SAFETY: the client's allocator shims; `record` is the `String` the caller passed,
        // which the original reads after this returns.
        if unsafe { put_string(alloc, dealloc, record, cap, ptr, new) } {
            REPLACED.fetch_add(1, Ordering::Relaxed);
        } else {
            once(a, LOG_ALLOC, || {
                format!(
                    "send_text: the client's allocator refused {} bytes; frame sent unchanged",
                    new.len()
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_op_from_the_first_key() {
        assert_eq!(op_of(br#"{"op":2,"d":{"token":"t"}}"#), Some(2));
        assert_eq!(op_of(br#"{"op":40,"d":{"seq":3}}"#), Some(40));
        assert_eq!(op_of(br#"{"op":6,"d":{}}"#), Some(6));
    }

    #[test]
    fn no_op_when_it_is_not_first_or_not_a_number() {
        assert_eq!(op_of(br#"{"d":{},"op":2}"#), None);
        assert_eq!(op_of(br#"{"op":"2"}"#), None);
        assert_eq!(op_of(br#"{"op":1234}"#), None);
        assert_eq!(op_of(b""), None);
    }

    #[test]
    fn only_identify_waits_for_the_engine() {
        assert!(waits_for_engine(Some(2)));
        assert!(!waits_for_engine(Some(40)));
        assert!(!waits_for_engine(Some(3)));
        assert!(!waits_for_engine(None));
    }
}
