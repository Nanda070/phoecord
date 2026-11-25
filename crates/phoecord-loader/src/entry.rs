//! `DllMain` and the worker thread.
//!
//! `DllMain` runs under the loader lock, so it does the bare minimum: disable thread
//! notifications and, in the main client process only, start the worker with a raw
//! `CreateThread`. Electron child processes (`--type=` on the command line) load the DLL too, for
//! export forwarding, and then do nothing else. Everything else (log file, client
//! version, signatures, hooks) happens on the worker once the lock is released.

use crate::{
    ipc::{self, Msg},
    log::Logger,
    pipe, signatures, version,
};
use std::{
    ffi::c_void,
    panic,
    path::PathBuf,
    time::{Duration, Instant},
};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HMODULE, TRUE},
        System::{
            Environment::GetCommandLineW,
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
                if is_child() {
                    return TRUE;
                }
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

/// Whether this process is an Electron child (command line has `--type=`).
fn is_child() -> bool {
    // SAFETY: `GetCommandLineW` returns a pointer to the process's own NUL-terminated command
    // line, valid for the life of the process; it takes no locks, so it is fine under the loader
    // lock. The length is found by scanning to the terminator.
    let cmd = unsafe {
        let p = GetCommandLineW();
        if p.is_null() {
            return false;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    };
    version::is_child_process(&cmd)
}

/// Detects the build from the exe already mapped in this process: no file read.
fn detect_in_memory() -> Result<version::ClientVersion, String> {
    // SAFETY: a null module name returns the host exe's base, mapped for the life of the process.
    // The first page holds the PE headers (the loader maps whole pages); `SizeOfImage` bytes from
    // the base are the mapped image, and we read only the headers, `.text` and the version
    // resource, all of which are mapped.
    let image = unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return Err("cannot get host module handle".into());
        }
        let size = crate::pe::Pe::image_size(std::slice::from_raw_parts(base, 0x1000))
            .map_err(|e| e.to_string())?;
        std::slice::from_raw_parts(base, size)
    };
    version::detect_mapped(image).map_err(|e| e.to_string())
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

/// What the worker tells the proxy about the UI layer.
struct Report {
    version: version::ClientVersion,
    detect_ms: u64,
    ui: bool,
    reason: Option<String>,
}

fn run() {
    let log = Logger::from_env();
    if let Some(r) = analyze(&log) {
        report_to_proxy(&log, r);
    }
}

/// Retries in the background (this is our own thread, never the client's), a few times, then
/// gives up quietly: no proxy means a client started without `phoecord run`, which is fine.
fn report_to_proxy(log: &Logger, r: Report) {
    let name = match pipe::current_pipe_name() {
        Ok(n) => n,
        Err(e) => return log.log(&format!("ipc: cannot name the pipe: {e}")),
    };
    let msgs = [
        Msg::Hello {
            loader_version: env!("CARGO_PKG_VERSION").to_string(),
            client_version: r.version.file_version.clone(),
            text_hash: r.version.text_hash.clone(),
            pid: std::process::id(),
            detect_ms: r.detect_ms,
        },
        Msg::Status {
            ui: r.ui,
            reason: r.reason,
        },
    ];
    let mut last = None;
    for (i, delay) in ipc::CONNECT_DELAYS_MS.iter().enumerate() {
        std::thread::sleep(Duration::from_millis(*delay));
        match pipe::send_all(&name, &msgs) {
            Ok(()) => {
                return log.log(&format!("ipc: reported to the proxy (attempt {})", i + 1));
            }
            Err(e) => last = Some(e),
        }
    }
    if let Some(e) = last {
        log.log(&format!(
            "ipc: no proxy reachable on {name} ({e}); giving up, the client runs without it"
        ));
    }
}

/// Identifies the build and resolves signatures; `None` when the build is unknown.
fn analyze(log: &Logger) -> Option<Report> {
    let exe = std::env::current_exe().unwrap_or_default();
    log.log(&format!(
        "phoecord-loader {} started in {}",
        env!("CARGO_PKG_VERSION"),
        exe.display()
    ));

    let started = Instant::now();
    let v = match detect_in_memory() {
        Ok(v) => v,
        Err(e) => {
            log.log(&format!("cannot identify client build: {e}"));
            return None;
        }
    };
    let detect_ms = started.elapsed().as_millis() as u64;
    log.log(&format!(
        "client build: version {} .text sha256 {} (detected in {detect_ms} ms)",
        v.file_version.as_deref().unwrap_or("unknown"),
        v.text_hash,
    ));
    let off = |v: version::ClientVersion, reason: &str| {
        Some(Report {
            version: v,
            detect_ms,
            ui: false,
            reason: Some(reason.to_string()),
        })
    };

    let Some(dir) = signatures_dir() else {
        log.log("no LOCALAPPDATA: UI layer stays off");
        return off(v, "no LOCALAPPDATA");
    };
    let (set, errors) = match signatures::SignatureSet::load_dir(&dir) {
        Ok(r) => r,
        Err(e) => {
            log.log(&format!(
                "no signatures in {}: {e}; UI layer stays off",
                dir.display()
            ));
            return off(v, "no signatures");
        }
    };
    for (file, e) in errors {
        log.log(&format!("signature file {file} skipped: {e}"));
    }
    let Some((key, sigs)) = set.select(&v) else {
        log.log("unknown client build: UI layer stays off, network plugins unaffected");
        return off(v, "unknown client build");
    };

    // SAFETY: a null module name returns the handle of the host exe, which is mapped for the
    // life of the process; `.text` is readable memory of `text_len` bytes at `base + text_rva`
    // (the section's virtual size from the exe's own headers).
    let text = unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            log.log("cannot get host module handle");
            return off(v, "cannot get host module handle");
        }
        std::slice::from_raw_parts(base.add(v.text_rva as usize), v.text_len as usize)
    };
    log.log(&format!("using signature table {key}"));
    let mut missed = 0;
    for (name, r) in signatures::resolve_all(sigs, text, v.text_rva) {
        match r {
            Ok(rva) => log.log(&format!("signature {name}: rva {rva:#x}")),
            Err(e) => {
                missed += 1;
                log.log(&format!("signature {name}: {e}; its feature stays off"));
            }
        }
    }
    // Hooks on the resolved functions arrive with the features that need them (1.8, phase 2).
    let reason = if missed > 0 {
        format!("{missed} signature(s) did not match")
    } else {
        "signatures resolved, no UI hooks installed yet".to_string()
    };
    off(v, &reason)
}
