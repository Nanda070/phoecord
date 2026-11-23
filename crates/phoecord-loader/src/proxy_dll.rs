//! Export forwarding for the proxy `version.dll`.
//!
//! Every export of the system `version.dll` (`exports.txt`, expanded by `build.rs`) is a tiny
//! assembly stub that jumps through [`FORWARD_TABLE`]. A slot is empty until the first call; the
//! stub then calls [`resolve_export`], which loads the **real** DLL by full path from
//! `GetSystemDirectoryW` (never by name: the name would resolve to this very DLL), looks the
//! function up and fills the slot. Nothing is loaded in `DllMain`, so there is no loader-lock work.
//!
//! Ordinals are not preserved: all 17 exports of `version.dll` are imported by name.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};
use windows_sys::{
    Win32::{
        Foundation::HMODULE,
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            SystemInformation::GetSystemDirectoryW,
        },
    },
    core::PCWSTR,
};

include!(concat!(env!("OUT_DIR"), "/forwarders.rs"));

/// Resolved addresses of the real functions, indexed like [`EXPORT_NAMES`].
pub static FORWARD_TABLE: [AtomicUsize; EXPORT_NAMES.len()] =
    [const { AtomicUsize::new(0) }; EXPORT_NAMES.len()];

static REAL_DLL: AtomicUsize = AtomicUsize::new(0);

/// Full path of the system `version.dll`, UTF-16, NUL-terminated.
pub fn system_dll_path(name: &str) -> Option<Vec<u16>> {
    let mut buf = [0u16; 520];
    // SAFETY: `buf` is valid for `buf.len()` u16 writes.
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n + name.len() + 2 > buf.len() {
        return None;
    }
    let mut path: Vec<u16> = buf[..n].to_vec();
    path.push(u16::from(b'\\'));
    path.extend(name.encode_utf16());
    path.push(0);
    Some(path)
}

/// The real `version.dll`, loaded on first use.
fn real_dll() -> Option<HMODULE> {
    let cached = REAL_DLL.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached as HMODULE);
    }
    let path = system_dll_path("version.dll")?;
    // SAFETY: `path` is a NUL-terminated wide string that outlives the call.
    let module = unsafe { LoadLibraryW(path.as_ptr() as PCWSTR) };
    if module.is_null() {
        return None;
    }
    // Two threads may race here; both get the same module (the loader ref-counts), so either
    // store is correct.
    REAL_DLL.store(module as usize, Ordering::Release);
    Some(module)
}

/// Address of a function in the real DLL (follows its own forwarders, e.g. to `kernel32`).
pub fn real_function(name: &str) -> Option<usize> {
    let module = real_dll()?;
    let mut c = name.as_bytes().to_vec();
    c.push(0);
    // SAFETY: `module` is a live module handle and `c` is a NUL-terminated ANSI name.
    let p = unsafe { GetProcAddress(module, c.as_ptr()) }?;
    Some(p as usize)
}

/// Returned in place of a function that cannot be resolved: the call fails with 0/FALSE instead
/// of crashing the client.
extern "system" fn unresolved() -> usize {
    0
}

/// Called by an export stub whose table slot is empty. Returns the address to jump to.
extern "C" fn resolve_export(index: u32) -> usize {
    let Some(slot) = FORWARD_TABLE.get(index as usize) else {
        return unresolved as *const c_void as usize;
    };
    let addr =
        real_function(EXPORT_NAMES[index as usize]).unwrap_or(unresolved as *const c_void as usize);
    slot.store(addr, Ordering::Release);
    addr
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::Pe;

    /// The checked-in export list must be exactly the export table of the system DLL.
    #[test]
    fn exports_match_system_dll() {
        let path = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("version.dll");
        let bytes = std::fs::read(path).unwrap();
        let real = Pe::parse(&bytes).unwrap().export_names().unwrap();
        let mut ours: Vec<&str> = EXPORT_NAMES.to_vec();
        let mut real: Vec<&str> = real.iter().map(String::as_str).collect();
        ours.sort_unstable();
        real.sort_unstable();
        assert_eq!(
            ours, real,
            "exports.txt is out of date: regenerate it from the system version.dll"
        );
    }

    #[test]
    fn every_export_resolves_in_the_real_dll() {
        for (i, name) in EXPORT_NAMES.iter().enumerate() {
            let a = resolve_export(i as u32);
            assert_ne!(a, unresolved as *const c_void as usize, "{name}");
            assert_eq!(FORWARD_TABLE[i].load(Ordering::Acquire), a);
        }
    }

    #[test]
    fn unknown_index_is_harmless() {
        assert_eq!(resolve_export(9999), unresolved as *const c_void as usize);
    }

    #[test]
    fn resolved_function_works() {
        // GetFileVersionInfoSizeW(path, &mut handle) -> size of the version block.
        let i = EXPORT_NAMES
            .iter()
            .position(|n| *n == "GetFileVersionInfoSizeW")
            .unwrap();
        let f: extern "system" fn(*const u16, *mut u32) -> u32 =
            // SAFETY: the resolved address is the real function with exactly this signature.
            unsafe { std::mem::transmute(resolve_export(i as u32)) };
        let path = system_dll_path("kernel32.dll").unwrap();
        let mut handle = 0u32;
        assert!(f(path.as_ptr(), &mut handle) > 0);
    }
}
