//! `full_ready` in an in-process launch (task 2.2.7): the gateway answers `IDENTIFY` with every
//! guild in full instead of `data_mode: "partial"`, as the proxy does (ruling 2026-11-26).
//!
//! The proxy empties `client_state.guild_versions` in the `IDENTIFY` JSON. In-process that JSON
//! never passes the encoder hook (2.2.4): `IDENTIFY` has its own builder, which serialises the
//! session's `ClientState` through one function (the `client_state` locator; private recon notes,
//! live probes of 2026-11-27). `guild_versions` is a `BTreeMap` (`root`, `height`, `len`) at
//! [`GUILD_VERSIONS`]; with `len` set to `0` for the call it iterates nothing and serialises as
//! `{}`. So the detour on that function zeroes `len` on entry, calls the original and puts `len`
//! back: the client's own state is unchanged afterwards.
//!
//! **Timing.** `IDENTIFY` leaves about 0.35 s after the process starts; the worker thread has the
//! other hooks in after about 1.1 s. So this one hook is installed from `DllMain`
//! ([`early_install`]), before the client's own code runs: the process is still suspended (the
//! launcher's APC) or still loading its imports (`version.dll`), so no other thread can be inside
//! the function. The build is told by its version resource only (hashing `.text` takes about
//! 0.5 s); the worker checks later, with the hash, that the table it selects puts `client_state`
//! at the same address ([`confirm`]), and switches the hook to a plain pass-through otherwise.
//! `[phoenix] full_ready = false` does the same ([`configure`]).

/// Offset of `guild_versions` (a `BTreeMap`: root pointer, height, length) in `ClientState`.
pub const GUILD_VERSIONS: usize = 0x88;
/// The most guild versions believed; anything larger is a wrong reading, left alone.
pub const MAX_GUILDS: usize = 100_000;

/// Whether the three words at [`GUILD_VERSIONS`] look like a non-empty map worth emptying: a
/// length in range, a root node behind it and a sane height. An empty map is left alone.
pub fn should_empty(root: usize, height: usize, len: usize) -> bool {
    len > 0 && len <= MAX_GUILDS && root != 0 && height < 32
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub use imp::*;

#[cfg(all(windows, target_arch = "x86_64"))]
mod imp {
    use super::{GUILD_VERSIONS, should_empty};
    use crate::aob::Pattern;
    use crate::found::CLIENT_STATE;
    use crate::hooks::abi::Memory;
    use crate::hooks::detour::ProcessMemory;
    use crate::hooks::early;
    use crate::signatures::Signature;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::time::Instant;

    /// Whether the detour empties the map; `false` makes it a pass-through.
    static ENABLED: AtomicBool = AtomicBool::new(true);
    /// Entry of the relocated original.
    static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
    /// RVA the early hook went in at; `0` when it did not.
    static INSTALLED_RVA: AtomicU32 = AtomicU32::new(0);
    /// What `early_install` did, for the worker to log (`DllMain` does not log).
    static STATUS: OnceLock<String> = OnceLock::new();
    /// The signature the early hook was placed by (pattern, `rip`), for [`confirm`].
    static PLACED_BY: OnceLock<(Pattern, Option<usize>)> = OnceLock::new();
    /// Calls that emptied the map, and the length the last one found.
    static EMPTIED: AtomicU32 = AtomicU32::new(0);
    static LAST_LEN: AtomicUsize = AtomicUsize::new(0);

    /// Installs the detour from `DllMain`, before the client runs; never panics out. The result is
    /// kept for [`status`].
    pub fn early_install() {
        let started = Instant::now();
        let line = match std::panic::catch_unwind(try_install) {
            Ok(Ok(rva)) => {
                INSTALLED_RVA.store(rva, Ordering::Release);
                format!(
                    "client_state hook in at rva {rva:#x} before the client started ({} ms)",
                    started.elapsed().as_millis()
                )
            }
            Ok(Err(e)) => format!("client_state hook off: {e}"),
            Err(_) => "client_state hook off: the early install panicked".into(),
        };
        let _ = STATUS.set(line);
    }

    fn try_install() -> Result<u32, String> {
        early::with_scan(|base, scan| {
            let found = scan.client_state().map_err(|e| e.to_string())?;
            if let Some(p) = early::placed_by(scan.sigs, CLIENT_STATE) {
                let _ = PLACED_BY.set(p);
            }
            let target = base + found.rva as usize;
            let mut set = crate::hook::global()
                .lock()
                .map_err(|_| "the hook set is poisoned")?;
            // SAFETY: `target` is the entry of the `ClientState` serializer the locator found (its
            // first bytes are pushes, no RIP-relative operand); `entry` keeps its ABI: it passes
            // the argument registers to the original and returns its `rax`.
            let hook = unsafe { set.add(target as *const (), entry as *const ()) }
                .map_err(|e| e.to_string())?;
            ORIGINAL.store(hook.original() as usize, Ordering::Release);
            set.install_all().map_err(|e| e.to_string())?;
            Ok(found.rva)
        })
    }

    /// `[phoenix] full_ready` from the config.
    pub fn configure(on: bool) {
        ENABLED.store(on, Ordering::Release);
    }

    /// Whether the early hook is in.
    pub fn installed() -> bool {
        INSTALLED_RVA.load(Ordering::Acquire) != 0
    }

    /// The worker's check with the hashed build: `Ok` when the hook is in and the table the worker
    /// selected has the same `client_state` signature the hook was placed by, else the hook is
    /// switched to a pass-through and `Err` says why. The signature is compared, not rescanned:
    /// the hook's own jump has changed the function's first bytes in memory by now.
    pub fn confirm(sigs: &[Signature]) -> Result<u32, String> {
        let rva = INSTALLED_RVA.load(Ordering::Acquire);
        if rva == 0 {
            return Err("not installed".into());
        }
        let same = early::same_signature(sigs, CLIENT_STATE, PLACED_BY.get());
        if same {
            return Ok(rva);
        }
        configure(false);
        Err(
            "the hashed build's table has another client_state signature; hook passes through"
                .into(),
        )
    }

    /// What [`early_install`] did.
    pub fn status() -> Option<&'static str> {
        STATUS.get().map(String::as_str)
    }

    /// How many serialisations had their guild versions emptied, and the last length found.
    pub fn emptied() -> (u32, usize) {
        (
            EMPTIED.load(Ordering::Relaxed),
            LAST_LEN.load(Ordering::Relaxed),
        )
    }

    /// The detour: `rcx` is the output (sret), `rdx` the `ClientState`. On entry `rsp` is 8 mod
    /// 16; two pushes and `0x48` realign it and leave the shadow space plus four saved words
    /// (`r8`, `r9`, the saved length, `rax`). `rbx` / `rsi` (callee-saved) keep `rcx` / `rdx`.
    #[unsafe(naked)]
    unsafe extern "C" fn entry() {
        std::arch::naked_asm!(
            "push rbx",
            "push rsi",
            "sub rsp, 0x48",
            "mov rbx, rcx",
            "mov rsi, rdx",
            "mov [rsp + 0x20], r8",
            "mov [rsp + 0x28], r9",
            "mov rcx, rsi",
            "call {before}",
            "mov [rsp + 0x30], rax",
            "mov rcx, rbx",
            "mov rdx, rsi",
            "mov r8, [rsp + 0x20]",
            "mov r9, [rsp + 0x28]",
            "call qword ptr [rip + {original}]",
            "mov [rsp + 0x38], rax",
            "mov rcx, rsi",
            "mov rdx, [rsp + 0x30]",
            "call {after}",
            "mov rax, [rsp + 0x38]",
            "add rsp, 0x48",
            "pop rsi",
            "pop rbx",
            "ret",
            before = sym before,
            after = sym after,
            original = sym ORIGINAL,
        );
    }

    /// Nothing to put back.
    const UNTOUCHED: usize = usize::MAX;

    /// Before the original: empties `guild_versions` when on and it looks right; returns the
    /// length to put back, or [`UNTOUCHED`]. No allocation, no panic.
    extern "C" fn before(state: usize) -> usize {
        if !ENABLED.load(Ordering::Acquire) || state == 0 {
            return UNTOUCHED;
        }
        let mem = ProcessMemory;
        let at = state + GUILD_VERSIONS;
        let (Some(root), Some(height), Some(len)) =
            (mem.word(at), mem.word(at + 8), mem.word(at + 16))
        else {
            return UNTOUCHED;
        };
        if !should_empty(root, height, len) {
            return UNTOUCHED;
        }
        // SAFETY: `at + 16` is the map's length word in the `ClientState` the client handed this
        // function, just read; the client's own thread is inside the call, and the word is put
        // back by `after` before the call returns to the client.
        unsafe { ((at + 16) as *mut usize).write_volatile(0) };
        EMPTIED.fetch_add(1, Ordering::Relaxed);
        LAST_LEN.store(len, Ordering::Relaxed);
        len
    }

    /// After the original: puts the length back.
    extern "C" fn after(state: usize, saved: usize) {
        if saved != UNTOUCHED {
            // SAFETY: the same word `before` zeroed, in the same `ClientState`.
            unsafe { ((state + GUILD_VERSIONS + 16) as *mut usize).write_volatile(saved) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plausible_non_empty_map_is_emptied() {
        assert!(should_empty(0x2737c79c390, 0, 7), "the live reading");
        assert!(!should_empty(0x2737c79c390, 0, 0), "already empty");
        assert!(!should_empty(0, 0, 7), "no root");
        assert!(!should_empty(0x1000, 40, 7), "height");
        assert!(!should_empty(0x1000, 0, MAX_GUILDS + 1), "length");
    }
}
