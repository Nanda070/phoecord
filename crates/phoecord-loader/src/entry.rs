//! `DllMain` and the worker thread.
//!
//! `DllMain` runs under the loader lock, so it does the bare minimum: disable thread
//! notifications and start the worker with a raw `CreateThread`. Everything else (log file, client
//! version, signatures, hooks) happens on the worker once the lock is released.

use crate::{log::Logger, signatures, version};
use std::{ffi::c_void, panic, path::PathBuf};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HMODULE, TRUE},
        System::{
            LibraryLoader::{DisableThreadLibraryCalls, GetModuleHandleW},
            SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH},
            Threading::CreateThread,
        },
    },
    core::BOOL,
};

/// `DllMain`. Exported by name so the loader finds it; not part of the forwarded API.
///
/// # Safety
/// Called by the Windows loader only.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, reserved: *mut c_void) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // SAFETY: `module` is this DLL's handle; `worker` has the thread-start signature and
            // needs no argument. The thread handle is closed straight away (we never join).
            unsafe {
                DisableThreadLibraryCalls(module);
                let h = CreateThread(
                    std::ptr::null(),
                    0,
                    Some(worker),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                );
                if !h.is_null() {
                    CloseHandle(h);
                }
            }
        }
        // FreeLibrary (reserved == null), not process exit: hooks point into this image, so they
        // must be removed before it is unmapped. On process exit nothing needs undoing.
        DLL_PROCESS_DETACH if reserved.is_null() => {
            if let Ok(set) = crate::hook::global().try_lock() {
                let _ = set.uninstall_all();
            }
        }
        _ => {}
    }
    TRUE
}

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    // A panic must never unwind into the client's process.
    let _ = panic::catch_unwind(run);
    0
}

fn signatures_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("PhoeCord").join("signatures"))
}

fn run() {
    let log = Logger::from_env();
    let exe = std::env::current_exe().unwrap_or_default();
    log.log(&format!(
        "phoecord-loader {} started in {}",
        env!("CARGO_PKG_VERSION"),
        exe.display()
    ));

    let bytes = match std::fs::read(&exe) {
        Ok(b) => b,
        Err(e) => return log.log(&format!("cannot read host exe: {e}")),
    };
    let v = match version::detect(&bytes) {
        Ok(v) => v,
        Err(e) => return log.log(&format!("cannot identify client build: {e}")),
    };
    log.log(&format!(
        "client build: version {} .text sha256 {}",
        v.file_version.as_deref().unwrap_or("unknown"),
        v.text_hash
    ));

    let Some(dir) = signatures_dir() else {
        return log.log("no LOCALAPPDATA: UI layer stays off");
    };
    let (set, errors) = match signatures::SignatureSet::load_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            return log.log(&format!(
                "no signatures in {}: {e}; UI layer stays off",
                dir.display()
            ));
        }
    };
    for (file, e) in errors {
        log.log(&format!("signature file {file} skipped: {e}"));
    }
    let Some((key, sigs)) = set.select(&v) else {
        return log.log("unknown client build: UI layer stays off, network plugins unaffected");
    };

    // SAFETY: a null module name returns the handle of the host exe, which is mapped for the
    // life of the process; `.text` is readable memory of `text_len` bytes at `base + text_rva`
    // (the section's virtual size from the exe's own headers).
    let text = unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return log.log("cannot get host module handle");
        }
        std::slice::from_raw_parts(base.add(v.text_rva as usize), v.text_len as usize)
    };
    log.log(&format!("using signature table {key}"));
    for (name, r) in signatures::resolve_all(sigs, text, v.text_rva) {
        match r {
            Ok(rva) => log.log(&format!("signature {name}: rva {rva:#x}")),
            Err(e) => log.log(&format!("signature {name}: {e}; its feature stays off")),
        }
    }
    // Hooks on the resolved functions arrive with the features that need them (1.8, phase 2).
}
