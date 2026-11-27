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
                // `full_ready` (2.2.7): `IDENTIFY` leaves before the worker could hook it, so this
                // one hook goes in now, before the client's code runs (no logging here).
                #[cfg(target_arch = "x86_64")]
                crate::hooks::client_state::early_install();
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
            #[cfg(target_arch = "x86_64")]
            crate::hooks::detour::uninstall_poll();
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

/// Detects the build from the exe mapped in this process. `.text`'s hash is then taken from the
/// exe file on disk ([`version::file_text_hash`]) when it can be read, because another DLL loaded
/// before this worker runs (a mod such as Sunbird) may have patched code in memory already.
fn detect(log: &Logger, exe: &std::path::Path) -> Result<version::ClientVersion, String> {
    let (mut v, image) = detect_in_memory()?;
    match version::file_text_hash(image, exe) {
        Ok(h) if h == v.text_hash => {}
        Ok(h) => {
            log.log(&format!(
                "in-memory .text differs from {} (sha256 {}): another DLL changed code before the \
                 loader; the build is identified by the file",
                exe.display(),
                v.text_hash
            ));
            v.text_hash = h;
        }
        Err(e) => log.log(&format!(
            "cannot hash .text of {}: {e}; using the in-memory hash",
            exe.display()
        )),
    }
    Ok(v)
}

/// Detects the build from the exe already mapped in this process: no file read.
fn detect_in_memory() -> Result<(version::ClientVersion, &'static [u8]), String> {
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
    let v = version::detect_mapped(image).map_err(|e| e.to_string())?;
    Ok((v, image))
}

unsafe extern "system" fn worker(_: *mut c_void) -> u32 {
    // A panic must never unwind into the client's process.
    let _ = panic::catch_unwind(run);
    0
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
    #[cfg(target_arch = "x86_64")]
    configure_full_ready(&log);
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
    // Set by `phoecord launch --channel phoenix` (2.2.8): in `in-process` the loader starts the
    // transport itself (2.2.7, after the build is known).
    let launched = crate::choice::Launched::from_env();
    match &launched {
        Some(l) => log.log(&format!("launched with transport {}", l.mode.name())),
        None => log.log("no transport from phoecord launch: proxy-era behaviour"),
    }

    let started = Instant::now();
    let v = match detect(log, &exe) {
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

    // The compiled-in public tables, with a hot-fix folder on top when there is one (2.2.10).
    let dir = signatures::default_dir(std::env::var_os("LOCALAPPDATA"));
    let (set, errors) = signatures::SignatureSet::load_with_builtin(dir.as_deref());
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
        // Hooked from `DllMain` already: its first bytes are the hook's jump now, so a rescan
        // cannot match; `report_full_ready` checks it instead.
        #[cfg(target_arch = "x86_64")]
        if name == crate::found::CLIENT_STATE && crate::hooks::client_state::installed() {
            log.log(&format!(
                "signature {name}: hooked before the client started"
            ));
            continue;
        }
        match r {
            Ok(rva) => log.log(&format!("signature {name}: rva {rva:#x}")),
            Err(e) => {
                missed += 1;
                log.log(&format!("signature {name}: {e}; its feature stays off"));
            }
        }
    }
    let kind = version::ClientKind::of(&exe.to_string_lossy(), v.file_version.as_deref());
    let mut menu = false;
    let mut patched = false;
    #[cfg(target_arch = "x86_64")]
    report_full_ready(log, sigs);
    if kind == version::ClientKind::Phoenix {
        let found = report_network_plan(log, sigs, text, &v, image_len());
        if let Some(l) = launched
            .as_ref()
            .filter(|l| l.mode == crate::choice::Mode::InProcess)
        {
            start_in_process(l, found, &v.text_hash);
        }
        menu = install_context_menus(log, sigs, text, &v);
        patched = install_behaviour_patches(log, sigs, text, &v);
    } else {
        log.log("Electron client: the in-process network hooks do not apply");
    }
    if menu || patched {
        return Some(Report {
            version: v,
            detect_ms,
            ui: true,
            reason: None,
        });
    }
    let reason = if missed > 0 {
        format!("{missed} signature(s) did not match")
    } else {
        "signatures resolved, no UI hook installed".to_string()
    };
    off(v, &reason)
}

/// `[phoenix] full_ready` for the early `client_state` hook, read first thing (the hook may run
/// about 0.35 s after start). A missing or unreadable config leaves the default, on.
#[cfg(target_arch = "x86_64")]
fn configure_full_ready(log: &Logger) {
    use phoecord_transport::config::{Config, Paths};
    let paths = Paths::resolve();
    if !paths.config.is_file() {
        return;
    }
    match Config::load(&paths.config) {
        Ok(cfg) => crate::hooks::client_state::configure(cfg.phoenix.full_ready),
        Err(e) => log.log(&format!(
            "full_ready: {} not readable ({e:#}); on by default",
            paths.config.display()
        )),
    }
}

/// How long the worker waits to report the first emptied `IDENTIFY`.
#[cfg(target_arch = "x86_64")]
const FULL_READY_WAIT: Duration = Duration::from_secs(30);

/// Logs what the early `client_state` hook did and checks it against the hashed build's table.
#[cfg(target_arch = "x86_64")]
fn report_full_ready(log: &Logger, sigs: &[signatures::Signature]) {
    use crate::hooks::client_state as cs;
    let Some(status) = cs::status() else { return };
    log.log(&format!("full_ready: {status}"));
    match cs::confirm(sigs) {
        // `IDENTIFY` may still be ahead (the client connects the gateway after its first REST
        // calls): wait for it on a thread of our own, never the client's.
        Ok(_) => {
            std::thread::spawn(|| {
                let log = Logger::from_env();
                let started = Instant::now();
                while started.elapsed() < FULL_READY_WAIT {
                    let (n, len) = cs::emptied();
                    if n > 0 {
                        return log.log(&format!(
                            "full_ready: IDENTIFY's client_state sent its {len} guild versions as {{}}"
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                log.log(&format!(
                    "full_ready: no IDENTIFY emptied in {} s (full_ready off, or no guild versions)",
                    FULL_READY_WAIT.as_secs()
                ));
            });
        }
        Err(e) if e == "not installed" => {}
        Err(e) => log.log(&format!("full_ready: {e}")),
    }
}

/// The context menu hooks (2.3.5, 2.3.11): plugin entries in Phoenix's own menus. Its entries and
/// clicks go to the engine of an in-process launch, or to the proxy over the loader IPC. Returns
/// whether it is on.
fn install_context_menus(
    log: &Logger,
    sigs: &[signatures::Signature],
    text: &[u8],
    v: &version::ClientVersion,
) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use crate::hooks::context_menu::{self, IdSource, MenuSink, MenuTarget};
        use crate::ui::context_menu as layout;
        use phoecord_core::MenuKind;
        let scan = crate::hooks::found::Scan {
            sigs,
            text,
            text_rva: v.text_rva,
            image_len: image_len(),
        };
        // SAFETY: a null module name returns the host exe's base, mapped for the life of the
        // process.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let mut targets = Vec::new();
        match scan.message_menu() {
            Ok(f) => targets.push(MenuTarget {
                kind: MenuKind::Message,
                push: base + f.rva as usize,
                after_anchor: base + f.target.after_anchor_rva as usize,
                ids: IdSource::BuildCall(base + f.target.build_call_rva as usize),
            }),
            Err(e) => log.log(&format!("context menu: message menu off ({e})")),
        }
        for (kind, at) in [
            (MenuKind::User, layout::USER_ANCHOR),
            (MenuKind::Channel, layout::CHANNEL_ANCHOR),
            (MenuKind::Server, layout::SERVER_ANCHOR),
        ] {
            match scan.context_menu(kind) {
                Ok(f) => targets.push(MenuTarget {
                    kind,
                    push: base + f.rva as usize,
                    after_anchor: base + f.target.after_anchor_rva as usize,
                    ids: IdSource::Closure(at),
                }),
                Err(e) => log.log(&format!("context menu: {} menu off ({e})", kind.name())),
            }
        }
        if targets.is_empty() {
            return false;
        }
        let names: Vec<&str> = targets.iter().map(|t| t.kind.name()).collect();
        let names = names.join(", ");
        let sink: std::sync::Arc<dyn MenuSink> = match crate::start::running() {
            Some(t) => std::sync::Arc::new(InProcessMenu(t)),
            None => match pipe::current_pipe_name() {
                Ok(name) => std::sync::Arc::new(IpcMenu(name)),
                Err(e) => {
                    log.log(&format!("context menu: off (cannot name the pipe: {e})"));
                    return false;
                }
            },
        };
        let hook_log: crate::hooks::guard::LogFn =
            std::sync::Arc::new(|l: &str| Logger::from_env().log(l));
        let installed = match crate::hook::global().lock() {
            // SAFETY: every target is the unique match of its locator inside this image.
            Ok(mut set) => unsafe { context_menu::prepare(&mut set, targets, sink, hook_log) }
                .and_then(|()| set.install_all()),
            Err(_) => {
                log.log("context menu: off (hook set poisoned)");
                return false;
            }
        };
        match installed {
            Ok(()) => {
                log.log(&format!(
                    "context menu: hooked ({names}); plugin entries show there"
                ));
                true
            }
            Err(e) => {
                log.log(&format!("context menu: not installed ({e})"));
                false
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (sigs, text, v);
        log.log("context menu: not on this architecture");
        false
    }
}

/// The client behaviour patches the user switched on (2.3.20, AlwaysTrust). Each is located and
/// hooked on its own. Returns whether at least one is on.
fn install_behaviour_patches(
    log: &Logger,
    sigs: &[signatures::Signature],
    text: &[u8],
    v: &version::ClientVersion,
) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use crate::hooks::behaviour;
        use phoecord_transport::config::{Config, Paths};
        let paths = Paths::resolve();
        if !paths.config.is_file() {
            return false;
        }
        let wanted = match Config::load(&paths.config) {
            Ok(cfg) => behaviour::wanted(&cfg),
            Err(e) => {
                log.log(&format!(
                    "{}: off ({} not readable: {e:#})",
                    behaviour::ID,
                    paths.config.display()
                ));
                return false;
            }
        };
        if wanted.is_empty() {
            return false;
        }
        let scan = crate::hooks::found::Scan {
            sigs,
            text,
            text_rva: v.text_rva,
            image_len: image_len(),
        };
        // SAFETY: a null module name returns the host exe's base, mapped for the life of the
        // process.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let Ok(mut set) = crate::hook::global().lock() else {
            log.log(&format!("{}: off (hook set poisoned)", behaviour::ID));
            return false;
        };
        let mut on = Vec::new();
        for patch in wanted {
            let name = patch.setting();
            let prepared = match patch.locate(&scan) {
                // SAFETY: the target is the unique match of this patch's locator in this image.
                Ok(rva) => unsafe { behaviour::prepare(&mut set, patch, base + rva as usize) }
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            match prepared {
                Ok(()) => on.push(name),
                Err(e) => log.log(&format!("{}: {name} off ({e})", behaviour::ID)),
            }
        }
        if on.is_empty() {
            return false;
        }
        match set.install_all() {
            Ok(()) => {
                log.log(&format!("{}: patched ({})", behaviour::ID, on.join(", ")));
                true
            }
            Err(e) => {
                log.log(&format!("{}: not installed ({e})", behaviour::ID));
                false
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (log, sigs, text, v);
        false
    }
}

/// The menu's entries and clicks through the proxy (loader IPC, one connection per request).
#[cfg(target_arch = "x86_64")]
struct IpcMenu(String);

#[cfg(target_arch = "x86_64")]
impl crate::hooks::context_menu::MenuSink for IpcMenu {
    fn items(&self) -> Vec<crate::hooks::context_menu::Item> {
        match pipe::request(&self.0, &Msg::MenuGet { id: 1 }) {
            Ok(Msg::MenuItems { items, .. }) => items
                .into_iter()
                .map(|i| crate::hooks::context_menu::Item {
                    plugin: i.plugin,
                    entry: i.entry,
                    label: i.label,
                    menu: i.menu,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn click(&self, plugin: &str, entry: &str, click: phoecord_transport::menu::MenuClick) {
        let msg = ipc::menu_action(plugin, entry, &ipc_click(click));
        if let Err(e) = pipe::send_all(&self.0, &[msg]) {
            Logger::from_env().log(&format!("message menu: click not sent to the proxy ({e})"));
        }
    }
}

/// A click in the IPC's form.
#[cfg(target_arch = "x86_64")]
fn ipc_click(click: phoecord_transport::menu::MenuClick) -> ipc::Click {
    use phoecord_transport::menu::MenuClick as C;
    let menu = click.kind().name().to_string();
    match click {
        C::Message {
            channel_id,
            message_id,
        } => ipc::Click {
            menu,
            channel_id: Some(channel_id),
            message_id: Some(message_id),
            ..ipc::Click::default()
        },
        C::User {
            user_id,
            guild_id,
            channel_id,
        } => ipc::Click {
            menu,
            user_id: Some(user_id),
            guild_id,
            channel_id,
            ..ipc::Click::default()
        },
        C::Channel {
            channel_id,
            guild_id,
        } => ipc::Click {
            menu,
            channel_id: Some(channel_id),
            guild_id,
            ..ipc::Click::default()
        },
        C::Server { guild_id } => ipc::Click {
            menu,
            guild_id: Some(guild_id),
            ..ipc::Click::default()
        },
    }
}

/// The menu's entries and clicks through the engine of an in-process launch.
#[cfg(target_arch = "x86_64")]
struct InProcessMenu(std::sync::Arc<crate::transport::InProcessTransport>);

#[cfg(target_arch = "x86_64")]
impl crate::hooks::context_menu::MenuSink for InProcessMenu {
    fn items(&self) -> Vec<crate::hooks::context_menu::Item> {
        self.0
            .menu_items()
            .into_iter()
            .map(|i| crate::hooks::context_menu::Item {
                plugin: i.plugin,
                entry: i.entry,
                label: i.label,
                menu: i.menu,
            })
            .collect()
    }

    fn click(&self, plugin: &str, entry: &str, click: phoecord_transport::menu::MenuClick) {
        if let Err(e) = self.0.menu_action(plugin, entry, click) {
            Logger::from_env().log(&format!("message menu: {e}"));
        }
    }
}

/// `SizeOfImage` of the host exe, 0 when unreadable (every locator then fails as out of image).
fn image_len() -> u32 {
    // SAFETY: a null module name returns the host exe's base, mapped for the life of the process;
    // its first page holds the PE headers.
    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return 0;
        }
        crate::pe::Pe::image_size(std::slice::from_raw_parts(base, 0x1000))
            .map_or(0, |n| u32::try_from(n).unwrap_or(0))
    }
}

/// Logs which network entry points this Phoenix build could run in-process, and whether the
/// launch would (all or nothing, 2.2.8); returns what the locators found. The transport is
/// started only by an `in-process` launch ([`start_in_process`]).
fn report_network_plan(
    log: &Logger,
    sigs: &[signatures::Signature],
    text: &[u8],
    v: &version::ClientVersion,
    image_len: u32,
) -> crate::hooks::found::HookPlan {
    let found = crate::hooks::found::HookPlan::locate(&crate::hooks::found::Scan {
        sigs,
        text,
        text_rva: v.text_rva,
        image_len,
    });
    for line in found.failures() {
        log.log(&line);
    }
    let plan = crate::plan::Plan::from_hooks(&found);
    for line in plan.report() {
        log.log(&line);
    }
    log.log(if plan.all_in_process() {
        "network: every entry point found; an in-process launch runs here without the proxy"
    } else {
        "network: through the proxy for this launch"
    });
    found
}

/// An `in-process` launch (2.2.8): the loader runs the plugin chain (2.2.7). On other targets
/// than Windows x64 there is no detour, so nothing starts.
fn start_in_process(
    launched: &crate::choice::Launched,
    found: crate::hooks::found::HookPlan,
    text_hash: &str,
) {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: a null module name returns the host exe's base, mapped for the life of the
        // process.
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        let log: crate::hooks::guard::LogFn =
            std::sync::Arc::new(|l: &str| Logger::from_env().log(l));
        crate::start::start(
            launched,
            found,
            base..base + image_len() as usize,
            text_hash,
            log,
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (launched, found, text_hash);
        Logger::from_env().log("in-process: not on this architecture; nothing hooked");
    }
}
