//! Inline detours (`retour`), installed and removed with every other thread of the process
//! suspended.
//!
//! Creating a detour (trampoline allocation, instruction decoding) happens first, with the
//! process running. Only the final patch (`enable` / `disable`) runs inside the suspended window,
//! and nothing in that window allocates or logs: a suspended thread may hold the heap lock.
//! If a suspended thread's instruction pointer sits inside a function about to be patched, the
//! threads are resumed and the attempt is retried a few times instead of patching under it.

use retour::RawDetour;
use std::{
    fmt,
    sync::{Arc, Mutex, OnceLock},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        Threading::{
            GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, Sleep,
            SuspendThread, THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME,
        },
    },
};

#[derive(Debug)]
pub enum HookError {
    Detour(String),
    /// Another thread kept executing inside a patch site; nothing was patched.
    Busy,
    /// A thread of the process could not be suspended.
    Suspend,
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::Detour(e) => write!(f, "detour error: {e}"),
            HookError::Busy => f.write_str("a thread was executing inside the patch site"),
            HookError::Suspend => f.write_str("could not suspend all threads"),
        }
    }
}

impl std::error::Error for HookError {}

impl From<retour::Error> for HookError {
    fn from(e: retour::Error) -> Self {
        HookError::Detour(e.to_string())
    }
}

/// Bytes at a patch site that must not be executing while it is rewritten. A 64-bit jump
/// sequence is at most 14 bytes; 16 leaves some slack.
const PATCH_SPAN: usize = 16;

/// One prepared detour.
pub struct Hook {
    raw: RawDetour,
    target: usize,
}

impl Hook {
    /// Entry of the relocated original function; call it from the detour to run the real code.
    pub fn original(&self) -> *const () {
        std::ptr::from_ref(self.raw.trampoline())
    }

    pub fn is_enabled(&self) -> bool {
        self.raw.is_enabled()
    }

    /// # Safety
    /// Every other thread must be suspended and none may execute inside the patch site.
    unsafe fn set_enabled(&self, on: bool) -> Result<(), HookError> {
        // SAFETY: forwarded from the caller's contract.
        unsafe {
            if on {
                self.raw.enable()
            } else {
                self.raw.disable()
            }
            .map_err(HookError::from)
        }
    }
}

/// All suspended threads of this process except the caller, resumed on drop.
struct Suspended {
    handles: Vec<HANDLE>,
}

impl Suspended {
    fn thread_ids() -> Option<Vec<u32>> {
        // SAFETY: plain Win32 calls; the snapshot handle is closed below and `entry.dwSize` is set
        // as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let (pid, me) = (GetCurrentProcessId(), GetCurrentThreadId());
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut ids = Vec::new();
            let mut ok = Thread32First(snap, &mut entry) != 0;
            while ok {
                if entry.th32OwnerProcessID == pid && entry.th32ThreadID != me {
                    ids.push(entry.th32ThreadID);
                }
                ok = Thread32Next(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            Some(ids)
        }
    }

    /// Suspends every other thread. All allocation happens before the first `SuspendThread`.
    fn all_other_threads() -> Result<Suspended, HookError> {
        let ids = Self::thread_ids().ok_or(HookError::Suspend)?;
        let mut open: Vec<HANDLE> = Vec::with_capacity(ids.len());
        for id in ids {
            // SAFETY: plain Win32 call; a null result (thread already gone) is skipped.
            let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT, 0, id) };
            if !h.is_null() {
                open.push(h);
            }
        }
        let mut guard = Suspended {
            handles: Vec::with_capacity(open.len()),
        };
        for h in open {
            // SAFETY: `h` is a thread handle we own. A thread that exited between the snapshot
            // and now fails with u32::MAX and is simply closed.
            if unsafe { SuspendThread(h) } == u32::MAX {
                // SAFETY: closing the handle we opened.
                unsafe { CloseHandle(h) };
            } else {
                guard.handles.push(h);
            }
        }
        Ok(guard)
    }

    /// True if any suspended thread is executing within `PATCH_SPAN` bytes after one of `sites`.
    #[cfg(target_arch = "x86_64")]
    fn any_inside(&self, sites: &[usize]) -> bool {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext,
        };
        self.handles.iter().any(|&h| {
            // SAFETY: CONTEXT is plain data; `ContextFlags` is set before the call, and `h` is a
            // suspended thread handle opened with THREAD_GET_CONTEXT.
            let rip = unsafe {
                // GetThreadContext requires a 16-byte aligned CONTEXT (fails with 998 otherwise).
                #[repr(align(16))]
                struct Aligned(CONTEXT);
                let mut ctx: Aligned = std::mem::zeroed();
                ctx.0.ContextFlags = CONTEXT_CONTROL_AMD64;
                if GetThreadContext(h, &mut ctx.0) == 0 {
                    return true; // cannot tell: assume the worst
                }
                ctx.0.Rip as usize
            };
            sites.iter().any(|&s| rip >= s && rip < s + PATCH_SPAN)
        })
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn any_inside(&self, _sites: &[usize]) -> bool {
        false
    }
}

impl Drop for Suspended {
    fn drop(&mut self) {
        for &h in &self.handles {
            // SAFETY: `h` is a suspended thread handle we own.
            unsafe {
                ResumeThread(h);
                CloseHandle(h);
            }
        }
    }
}

/// Runs `patch` with the other threads suspended and none of them inside `sites`.
fn while_suspended<R>(sites: &[usize], patch: impl FnOnce() -> R) -> Result<R, HookError> {
    for _ in 0..20 {
        let guard = Suspended::all_other_threads()?;
        if guard.any_inside(sites) {
            drop(guard);
            // SAFETY: plain Win32 call; gives the busy thread time to leave.
            unsafe { Sleep(1) };
            continue;
        }
        let r = patch();
        drop(guard);
        return Ok(r);
    }
    Err(HookError::Busy)
}

/// A set of detours installed and removed together.
#[derive(Default)]
pub struct HookSet {
    hooks: Vec<Arc<Hook>>,
}

impl HookSet {
    pub fn new() -> HookSet {
        HookSet::default()
    }

    /// Prepares a detour from `target` to `detour` (not active until [`install_all`]). The
    /// returned handle gives access to the original through [`Hook::original`].
    ///
    /// # Safety
    /// `target` must be the entry of a function with enough relocatable bytes at its start, and
    /// `detour` must be a function with the same signature and ABI.
    ///
    /// [`install_all`]: HookSet::install_all
    pub unsafe fn add(
        &mut self,
        target: *const (),
        detour: *const (),
    ) -> Result<Arc<Hook>, HookError> {
        // SAFETY: forwarded from the caller's contract.
        let raw = unsafe { RawDetour::new(target, detour) }?;
        let hook = Arc::new(Hook {
            raw,
            target: target as usize,
        });
        self.hooks.push(hook.clone());
        Ok(hook)
    }

    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    fn set_all(&self, on: bool) -> Result<(), HookError> {
        let sites: Vec<usize> = self
            .hooks
            .iter()
            .filter(|h| h.is_enabled() != on)
            .map(|h| h.target)
            .collect();
        if sites.is_empty() {
            return Ok(());
        }
        while_suspended(&sites, || {
            for h in self.hooks.iter().filter(|h| h.is_enabled() != on) {
                // SAFETY: other threads are suspended and none is inside a patch site.
                unsafe { h.set_enabled(on)? };
            }
            Ok::<(), HookError>(())
        })?
    }

    /// Activates every hook.
    pub fn install_all(&self) -> Result<(), HookError> {
        self.set_all(true)
    }

    /// Restores the original code of every hook.
    pub fn uninstall_all(&self) -> Result<(), HookError> {
        self.set_all(false)
    }
}

/// The hooks of this process, so that unloading the DLL can remove them.
pub fn global() -> &'static Mutex<HookSet> {
    static SET: OnceLock<Mutex<HookSet>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HookSet::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    extern "C" fn target(x: i32) -> i32 {
        std::hint::black_box(x) * 2 + 1
    }

    extern "C" fn replacement(x: i32) -> i32 {
        std::hint::black_box(x) * 2 + 1000
    }

    /// A no-op style hook (calls through to the original) installs and uninstalls 100 times.
    #[test]
    fn install_uninstall_100_times() {
        let mut set = HookSet::new();
        // SAFETY: `target` and `replacement` share the signature `extern "C" fn(i32) -> i32`.
        let d = unsafe {
            set.add(
                target as extern "C" fn(i32) -> i32 as *const (),
                replacement as extern "C" fn(i32) -> i32 as *const (),
            )
        }
        .unwrap();
        for _ in 0..100 {
            set.install_all().unwrap();
            assert_eq!(target(3), 1006); // detour active
            // SAFETY: the trampoline is the relocated original with the same signature.
            let orig: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(d.original()) };
            assert_eq!(orig(3), 7);
            set.uninstall_all().unwrap();
            assert_eq!(target(3), 7); // restored
        }
    }

    #[test]
    fn empty_set_is_a_no_op() {
        let set = HookSet::new();
        assert!(set.is_empty());
        set.install_all().unwrap();
        set.uninstall_all().unwrap();
    }
}
