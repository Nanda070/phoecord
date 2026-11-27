//! Client behaviour patches (task 2.3.20). So far AlwaysTrust (Vencord: AlwaysTrust, by zt and
//! Trwy, GPL-3.0-or-later): no "untrusted link" and "suspicious download" prompts.
//!
//! Phoenix classifies every link click in one function, which asks three checks in turn (private
//! recon notes, 2.3.20). Each is a plain function, replaced as a whole; the original is not
//! called:
//!
//! | Switch | Check | Replacement | Default |
//! |:---|:---|:---|:---|
//! | `domain` | is this website trusted? | yes | on |
//! | `file` | is this a suspicious download? | no (`None`) | on |
//! | `protocol` | is this app protocol trusted? | yes | **off** |
//!
//! `protocol` is not in Vencord: with it any custom scheme opens another program without a
//! question, which is more than "untrusted link" suggests. The blocked-domain check is a safety
//! list, not a trust prompt, and is left alone.
//!
//! Switches live in `config.toml`, off until the user turns the plugin on:
//!
//! ```toml
//! [plugins.always_trust]
//! enabled = true
//! domain = true
//! file = true
//! protocol = false
//! ```
//!
//! Each patch has its own locator: a miss turns off only that patch. The replacements are a few
//! instructions that cannot panic or allocate, so they run without a panic guard. They are read
//! once, when the loader starts; a change needs a client restart (as in Vencord).

use crate::found::{Found, LocatorError, Scan};
use phoecord_transport::config::Config;

/// The plugin's table in `config.toml`.
pub const ID: &str = "always_trust";

/// One patch of AlwaysTrust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Patch {
    Domain,
    File,
    Protocol,
}

impl Patch {
    pub const ALL: [Patch; 3] = [Patch::Domain, Patch::File, Patch::Protocol];

    /// The setting's name, Vencord's where Vencord has it.
    pub fn setting(self) -> &'static str {
        match self {
            Patch::Domain => "domain",
            Patch::File => "file",
            Patch::Protocol => "protocol",
        }
    }

    fn default_on(self) -> bool {
        self != Patch::Protocol
    }

    /// The entry of the function this patch replaces, in this build.
    pub fn locate(self, scan: &Scan<'_>) -> Result<u32, LocatorError> {
        fn rva<T>(found: Result<Found<T>, LocatorError>) -> Result<u32, LocatorError> {
            found.map(|f| f.rva)
        }
        match self {
            Patch::Domain => rva(scan.link_website_trusted()),
            Patch::File => rva(scan.link_suspicious_download()),
            Patch::Protocol => rva(scan.link_protocol_trusted()),
        }
    }
}

/// The patches the user switched on: none unless the plugin is enabled.
pub fn wanted(cfg: &Config) -> Vec<Patch> {
    if !cfg.is_enabled(ID) {
        return Vec::new();
    }
    let table = cfg.plugins.get(ID);
    Patch::ALL
        .into_iter()
        .filter(|p| {
            table
                .and_then(|t| t.get(p.setting()))
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| p.default_on())
        })
        .collect()
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub use install::prepare;

#[cfg(all(windows, target_arch = "x86_64"))]
mod install {
    use super::Patch;
    use crate::hook::{HookError, HookSet};

    /// Windows x64: `is_website_trusted(ctx, url_ptr, url_len) -> bool` and
    /// `is_protocol_trusted` with the same arguments.
    pub(super) extern "C" fn trusted(_ctx: *mut u8, _url: *const u8, _len: usize) -> bool {
        true
    }

    /// `Option<Sus>`'s `None`, in the first word of the output.
    pub(super) const NOT_SUSPICIOUS: u64 = u64::MAX;

    /// Windows x64: `suspicious_download(out: *mut Option<Sus>, url_ptr, url_len)`, which returns
    /// `out`.
    pub(super) unsafe extern "C" fn not_suspicious(
        out: *mut u64,
        _url: *const u8,
        _len: usize,
    ) -> *mut u64 {
        // SAFETY: the client passes its own output buffer, which the original always writes; its
        // first word alone says `None`.
        unsafe { out.write(NOT_SUSPICIOUS) };
        out
    }

    /// Prepares the detour of `patch` over the function at `target` (not active until the set is
    /// installed).
    ///
    /// # Safety
    /// `target` must be the entry of the function `patch` replaces, located by its signature,
    /// with enough relocatable bytes at its start.
    pub unsafe fn prepare(set: &mut HookSet, patch: Patch, target: usize) -> Result<(), HookError> {
        let detour = match patch {
            Patch::Domain | Patch::Protocol => trusted as *const (),
            Patch::File => not_suspicious as *const (),
        };
        // SAFETY: forwarded from the caller's contract; each replacement has the signature and
        // ABI of the function it replaces.
        unsafe { set.add(target as *const (), detour) }.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signatures::SignatureSet;

    fn config(text: &str) -> Config {
        let dir = std::env::temp_dir().join(format!(
            "phoecord-behaviour-{}-{}",
            std::process::id(),
            text.len()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, text).unwrap();
        let cfg = Config::load(&file).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        cfg
    }

    #[test]
    fn off_until_enabled_and_protocol_only_when_asked_for() {
        assert!(wanted(&Config::default()).is_empty());
        assert!(wanted(&config("[plugins.always_trust]\ndomain = true")).is_empty());
        assert_eq!(
            wanted(&config("[plugins.always_trust]\nenabled = true")),
            [Patch::Domain, Patch::File]
        );
        assert_eq!(
            wanted(&config(
                "[plugins.always_trust]\nenabled = true\ndomain = false\nprotocol = true"
            )),
            [Patch::File, Patch::Protocol]
        );
    }

    #[test]
    fn a_missing_signature_turns_off_only_its_patch() {
        // Made-up code; each name finds its own bytes, `protocol` has no entry.
        let set = SignatureSet::parse(
            "[build.\"1\"]\n\
             link_website_trusted = { aob = \"11 22 33\" }\n\
             link_suspicious_download = { aob = \"44 55 66\" }",
        )
        .unwrap();
        let version = crate::version::ClientVersion {
            file_version: Some("1".into()),
            text_hash: "00".into(),
            text_rva: 0x1000,
            text_len: 0,
        };
        let (_, sigs) = set.select(&version).unwrap();
        let text = [0u8, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let scan = Scan {
            sigs,
            text: &text,
            text_rva: 0x1000,
            image_len: 0x2000,
        };
        assert_eq!(Patch::Domain.locate(&scan), Ok(0x1001));
        assert_eq!(Patch::File.locate(&scan), Ok(0x1004));
        assert_eq!(
            Patch::Protocol.locate(&scan),
            Err(LocatorError::Missing("link_protocol_trusted"))
        );
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn the_detours_replace_the_checks_and_are_removed() {
        use crate::hook::HookSet;
        use std::hint::black_box;

        // Stand-ins with the client functions' signatures.
        #[inline(never)]
        extern "C" fn untrusted(_ctx: *mut u8, _url: *const u8, len: usize) -> bool {
            black_box(len) == usize::MAX
        }
        #[inline(never)]
        unsafe extern "C" fn suspicious(out: *mut u64, _url: *const u8, len: usize) -> *mut u64 {
            // SAFETY: the test passes a valid buffer.
            unsafe { out.write(black_box(len) as u64) };
            out
        }
        type Trusted = extern "C" fn(*mut u8, *const u8, usize) -> bool;
        type Suspicious = unsafe extern "C" fn(*mut u64, *const u8, usize) -> *mut u64;
        let trusted_fn = black_box(untrusted as Trusted);
        let suspicious_fn = black_box(suspicious as Suspicious);
        let call = || {
            let mut out = 0u64;
            let null = std::ptr::null_mut();
            // SAFETY: `out` is a valid buffer; neither stand-in reads the url.
            let back = unsafe { suspicious_fn(&mut out, null, 7) };
            assert_eq!(back, &raw mut out);
            (trusted_fn(null, null, 7), out)
        };
        assert_eq!(call(), (false, 7));

        let mut set = HookSet::new();
        // SAFETY: both targets are functions of this test with the replacements' signatures.
        unsafe {
            prepare(&mut set, Patch::Domain, trusted_fn as usize).unwrap();
            prepare(&mut set, Patch::File, suspicious_fn as usize).unwrap();
        }
        set.install_all().unwrap();
        assert_eq!(call(), (true, u64::MAX));
        set.uninstall_all().unwrap();
        assert_eq!(call(), (false, 7));
    }
}
