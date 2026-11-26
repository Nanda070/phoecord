//! Starting the in-process transport in a launch that chose it (task 2.2.7, with the launch mode
//! of 2.2.8).
//!
//! `phoecord launch` decides the transport before Phoenix starts and passes it in the environment
//! ([`crate::choice::Launched`]). In an `in-process` launch no proxy runs, so the loader builds its
//! own [`Engine`]: the built-in plugins and the WASM packages of `<data>/plugins/`, enabled from
//! `config.toml` (ruling 2026-11-26: the loader reads its settings from `config.toml` itself, no
//! pipe). [`engine_from`] is that part, on any OS.
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
pub fn engine_from(paths: &Paths, log: &dyn Fn(&str)) -> Result<Arc<Engine>, String> {
    let cfg = if paths.config.is_file() {
        Config::load(&paths.config).map_err(|e| format!("{}: {e:#}", paths.config.display()))?
    } else {
        log(&format!(
            "{}: not found, the plugins run with their defaults",
            paths.config.display()
        ));
        Config::default()
    };
    let mut shared = Shared::new(paths.data.join("plugins"));
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
pub use windows::start;

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::engine_from;
    use crate::{
        choice::Launched,
        found::HookPlan,
        hook,
        hooks::guard::LogFn,
        transport::{Clock, InProcessTransport, SendSink},
    };
    use phoecord_transport::config::Paths;
    use std::{ops::Range, sync::Arc, sync::OnceLock};

    /// What stays alive for the life of the process once started.
    struct Running {
        _transport: Arc<InProcessTransport>,
        _clock: Option<Clock>,
    }

    static RUNNING: OnceLock<Running> = OnceLock::new();

    /// Starts the in-process transport for this launch. `image` is the client module's range,
    /// `text_hash` the running build's (for the "failed" mark).
    pub fn start(
        launched: &Launched,
        found: HookPlan,
        image: Range<usize>,
        text_hash: &str,
        log: LogFn,
    ) {
        let engine = match engine_from(&Paths::resolve(), &*log) {
            Ok(e) => e,
            Err(e) => {
                log(&format!(
                    "in-process: cannot build the engine ({e}); no network plugins this launch"
                ));
                return mark_failed(launched, text_hash, &log);
            }
        };
        let send = found.send.as_ref().ok().map(|f| f.rva);
        let transport = Arc::new(InProcessTransport::new(engine, found, log.clone()));
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
        let Some(rva) = send else {
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
            let target = image.start + rva as usize;
            // SAFETY: `target` is the unique match of the `http_send` locator inside this image.
            unsafe {
                crate::hooks::detour::prepare(
                    &mut set,
                    target,
                    image,
                    Box::new(SendSink(transport.clone())),
                    log.clone(),
                )
            }
            .and_then(|()| set.install_all())
        };
        match installed {
            Ok(()) => {
                log("in-process: send detour installed; plugins see REST requests in-process")
            }
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
        let engine = engine_from(&p, &log).unwrap();
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
        let engine = engine_from(&p, &log).unwrap();
        assert!(!engine.chain.enabled_ids().iter().any(|id| id == "no_track"));
        let p = paths("cfg-on");
        std::fs::write(&p.config, "[plugins.no_track]\nenabled = true\n").unwrap();
        let engine = engine_from(&p, &log).unwrap();
        assert!(engine.chain.enabled_ids().iter().any(|id| id == "no_track"));
    }

    #[test]
    fn a_broken_config_is_an_error_not_silence() {
        let p = paths("broken");
        std::fs::write(&p.config, "this is = = not toml").unwrap();
        let (_, log) = lines();
        assert!(engine_from(&p, &log).is_err());
    }
}
