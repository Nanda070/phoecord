//! Starting the in-process transport in a launch that chose it (task 2.2.7, with the launch mode
//! of 2.2.8).
//!
//! `phoecord launch` decides the transport before Phoenix starts and passes it in the environment
//! ([`crate::choice::Launched`]). In an `in-process` launch no proxy runs, so the loader builds its
//! own [`Engine`]: the built-in plugins and the WASM packages of `<data>/plugins/`, enabled from
//! `config.toml` (ruling 2026-11-26: the loader reads its settings from `config.toml` itself, no
//! pipe), with [`LoaderNet`] behind plugins' `rest` and `fetch`. [`engine_from`] is that part, on
//! any OS.
//!
//! The rest is Windows x64 ([`start`]): the [`InProcessTransport`] over the hooks found in this
//! build, the effects [`Clock`], and the `send` detour fed by a [`SendSink`]. All or nothing per
//! launch (2.2.8): when the transport is off (a locator missing in memory although the launcher's
//! check on disk passed), nothing is hooked, the build is marked failed in the launcher's cache so
//! the next launch uses the proxy, and this launch runs without network plugins.
//!
//! [`InProcessTransport`]: crate::transport::InProcessTransport
//! [`Clock`]: crate::transport::Clock
//! [`SendSink`]: crate::transport::SendSink

use crate::net::LoaderNet;
use phoecord_plugin_runtime::{Chain, package::PackageSet, wasm::WasmRuntime};
use phoecord_transport::{
    Engine,
    config::{Config, Paths},
    host::Shared,
};
use std::sync::Arc;

/// The engine of an in-process launch: every built-in plugin, the WASM packages in
/// `<data>/plugins/`, enabled as `paths.config` says. A missing `config.toml` means the defaults
/// (the loader never writes one); an unreadable one is an error, so a broken file does not
/// silently turn every plugin off. A WASM runtime that does not start only turns packages off.
pub fn engine_from(
    paths: &Paths,
    net: Arc<LoaderNet>,
    log: &dyn Fn(&str),
) -> Result<Arc<Engine>, String> {
    let cfg = if paths.config.is_file() {
        Config::load(&paths.config).map_err(|e| format!("{}: {e:#}", paths.config.display()))?
    } else {
        log(&format!(
            "{}: not found, the plugins run with their defaults",
            paths.config.display()
        ));
        Config::default()
    };
    let mut shared = Shared::with_network(paths.data.join("plugins"), net);
    shared.config_path = paths.config.clone();
    let engine = Arc::new(Engine::new(
        Chain::new(phoecord_plugins::builtin()),
        shared,
        cfg,
    ));
    phoecord_plugins::register_strings(&engine.shared.strings);
    match WasmRuntime::new() {
        Ok(rt) => {
            engine.watch_packages(
                PackageSet::new(rt, paths.data.join("plugins"))
                    .with_strings(engine.shared.strings.clone()),
            );
        }
        Err(e) => log(&format!("WASM plugins are off: {e:#}")),
    }
    let on = engine.chain.enabled_ids();
    log(&format!(
        "engine: {} plugin(s) on from {} ({})",
        on.len(),
        paths.config.display(),
        on.join(", ")
    ));
    Ok(engine)
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub use windows::{running, start};

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::engine_from;
    use crate::{
        choice::Launched,
        found::HookPlan,
        hook,
        hooks::guard::LogFn,
        net::LoaderNet,
        transport::{Clock, DecodeSink, EncodeSink, FrameFeed, InProcessTransport, SendSink},
    };
    use phoecord_transport::config::Paths;
    use std::{ops::Range, sync::Arc, sync::OnceLock};

    /// What stays alive for the life of the process once started.
    struct Running {
        _transport: Arc<InProcessTransport>,
        _clock: Option<Clock>,
    }

    static RUNNING: OnceLock<Running> = OnceLock::new();

    /// The transport of this launch once it runs (the message menu hook sends its clicks there).
    pub fn running() -> Option<Arc<InProcessTransport>> {
        RUNNING.get().map(|r| r._transport.clone())
    }

    /// Starts the in-process transport for this launch. `image` is the client module's range,
    /// `text_hash` the running build's (for the "failed" mark).
    pub fn start(
        launched: &Launched,
        found: HookPlan,
        image: Range<usize>,
        text_hash: &str,
        log: LogFn,
    ) {
        // Plugins' `rest` and `fetch`: the session comes from the `send` detour below.
        let net = Arc::new(LoaderNet::new());
        let engine = match engine_from(&Paths::resolve(), net.clone(), &*log) {
            Ok(e) => e,
            Err(e) => {
                log(&format!(
                    "in-process: cannot build the engine ({e}); no network plugins this launch"
                ));
                return mark_failed(launched, text_hash, &log);
            }
        };
        let base = image.start;
        let abs = |rva: u32| base + rva as usize;
        let send = found.send.as_ref().ok().map(|f| abs(f.rva));
        let poll_vtable = found.poll.as_ref().ok().map(|f| abs(f.rva));
        let bytes_static = found.bytes_static.as_ref().ok().map(|f| abs(f.rva));
        let gateway_send = found.gateway_send.as_ref().ok().map(|f| abs(f.rva));
        let send_text = found.send_text.as_ref().ok().map(|f| (abs(f.rva), f.rva));
        let inflate = found.inflate.as_ref().ok().map(|f| abs(f.rva));
        let frame = found.frame.as_ref().ok().map(|f| abs(f.rva));
        let allocator = found
            .allocator
            .as_ref()
            .ok()
            .map(|f| (abs(f.rva), abs(f.target.dealloc_rva)));
        let transport =
            Arc::new(InProcessTransport::new(engine, found, log.clone()).with_network(net));
        if !transport.is_active() {
            log(
                "in-process: the launcher chose in-process but this build lacks an entry point in memory; nothing hooked",
            );
            return mark_failed(launched, text_hash, &log);
        }
        let clock = match Clock::start(transport.delivery().clone()) {
            Ok(c) => Some(c),
            Err(e) => {
                log(&format!(
                    "in-process: timer clock did not start ({e}); plugin timers are off"
                ));
                None
            }
        };
        let Some(send) = send else {
            // `is_active` needs every locator, so this cannot happen; refuse rather than guess.
            log("in-process: no send locator although the transport is on; nothing hooked");
            return mark_failed(launched, text_hash, &log);
        };
        let installed = {
            let mut set = match hook::global().lock() {
                Ok(s) => s,
                Err(_) => {
                    log("in-process: hook set poisoned; nothing hooked");
                    return mark_failed(launched, text_hash, &log);
                }
            };
            let targets = crate::hooks::detour::Targets {
                send,
                poll_vtable,
                bytes_static,
                allocator,
            };
            // SAFETY: every address is the unique match of its locator inside this image.
            unsafe {
                crate::hooks::detour::prepare(
                    &mut set,
                    targets,
                    image,
                    Box::new(SendSink(transport.clone())),
                    log.clone(),
                )
            }
            .and_then(|()| {
                // The outgoing frames: `send_text` (placed from `DllMain`, or now), else the
                // encoder. Never both, or `on_send` runs twice per frame.
                use crate::hooks::gateway_send_text as text;
                let sink = Box::new(EncodeSink(transport.clone()));
                if text::installed() {
                    text::attach(allocator, sink, log.clone());
                    return Ok(());
                }
                match (send_text, gateway_send) {
                    // SAFETY: the unique match of the `gateway_send_text` locator inside this
                    // image, at RVA `rva`, and the allocator shims of the same build.
                    (Some((target, rva)), _) => unsafe {
                        text::prepare(&mut set, target, rva, allocator, sink, log.clone())
                    },
                    // SAFETY: the unique match of the `gateway_send` locator inside this image.
                    (None, Some(target)) => unsafe {
                        crate::hooks::gateway_send::prepare(
                            &mut set,
                            target,
                            allocator,
                            sink,
                            log.clone(),
                        )
                    },
                    (None, None) => Ok(()),
                }
            })
            .and_then(|()| match inflate {
                // SAFETY: the unique match of the `gateway_inflate` locator inside this image.
                Some(target) => unsafe {
                    crate::hooks::gateway_recv::prepare(
                        &mut set,
                        target,
                        Box::new(DecodeSink(transport.clone())),
                        log.clone(),
                    )
                },
                None => Ok(()),
            })
            .and_then(|()| match (inflate, frame, allocator) {
                // SAFETY: the unique match of the `gateway_frame` locator inside this image and
                // the allocator shims of the same build.
                (Some(_), Some(target), Some(alloc)) => unsafe {
                    crate::hooks::gateway_frame::prepare(
                        &mut set,
                        target,
                        alloc,
                        Box::new(FrameFeed(transport.clone())),
                        log.clone(),
                    )
                },
                _ => Ok(()),
            })
            .and_then(|()| set.install_all())
            .and_then(|()| crate::hooks::detour::install_poll())
        };
        match installed {
            Ok(()) => log(if crate::hooks::gateway_send_text::installed() {
                "in-process: send, poll, gateway inflate and gateway send_text hooks installed; plugins see REST and the gateway in-process"
            } else {
                "in-process: send, poll, gateway inflate and gateway encoder hooks installed; plugins see REST and the gateway in-process"
            }),
            Err(e) => {
                log(&format!("in-process: send detour not installed ({e})"));
                return mark_failed(launched, text_hash, &log);
            }
        }
        let _ = RUNNING.set(Running {
            _transport: transport,
            _clock: clock,
        });
    }

    /// The seam of 2.2.8 item 4: the next launch of this build uses the proxy until the signature
    /// folder changes. `phoecord launch` passes the cache in every in-process launch, forced or
    /// not; a launch without one (`phoecord dev inject`) marks nothing.
    fn mark_failed(launched: &Launched, text_hash: &str, log: &LogFn) {
        // No plugin chain will come: an `IDENTIFY` held by the early `send_text` hook goes on.
        crate::hooks::gateway_send_text::give_up();
        let (Some(cache), Some(sigs)) = (&launched.cache, &launched.signatures_hash) else {
            return;
        };
        match crate::choice::mark_failed(cache, text_hash, sigs) {
            Ok(()) => {
                log("in-process: this build is marked failed; the next launch uses the proxy")
            }
            Err(e) => log(&format!(
                "in-process: cannot mark the build failed in {}: {e}",
                cache.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn paths(tag: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("phoecord-start-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            config: dir.join("config.toml"),
            data: dir.join("data"),
        }
    }

    fn lines() -> (Arc<Mutex<Vec<String>>>, impl Fn(&str)) {
        let v = Arc::new(Mutex::new(Vec::new()));
        let sink = v.clone();
        (v, move |l: &str| sink.lock().unwrap().push(l.to_string()))
    }

    #[test]
    fn no_config_means_defaults_and_is_logged() {
        let p = paths("none");
        let (got, log) = lines();
        let engine = engine_from(&p, Arc::new(LoaderNet::new()), &log).unwrap();
        assert!(!p.config.exists(), "the loader never writes config.toml");
        assert!(
            !engine.chain.enabled_ids().is_empty(),
            "default plugins are on"
        );
        let got = got.lock().unwrap();
        assert!(got.iter().any(|l| l.contains("not found")));
        assert!(got.iter().any(|l| l.starts_with("engine: ")));
    }

    #[test]
    fn the_config_decides_which_plugins_run() {
        let p = paths("cfg");
        std::fs::write(&p.config, "[plugins.no_track]\nenabled = false\n").unwrap();
        let (_, log) = lines();
        let engine = engine_from(&p, Arc::new(LoaderNet::new()), &log).unwrap();
        assert!(!engine.chain.enabled_ids().iter().any(|id| id == "no_track"));
        let p = paths("cfg-on");
        std::fs::write(&p.config, "[plugins.no_track]\nenabled = true\n").unwrap();
        let engine = engine_from(&p, Arc::new(LoaderNet::new()), &log).unwrap();
        assert!(engine.chain.enabled_ids().iter().any(|id| id == "no_track"));
    }

    #[test]
    fn a_broken_config_is_an_error_not_silence() {
        let p = paths("broken");
        std::fs::write(&p.config, "this is = = not toml").unwrap();
        let (_, log) = lines();
        assert!(engine_from(&p, Arc::new(LoaderNet::new()), &log).is_err());
    }
}
