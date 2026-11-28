//! What the hooks installed from `DllMain` share: the host image and the signature table of its
//! build, chosen by the version resource alone (hashing `.text` takes about 0.5 s, too long before
//! the client's first frames). The worker checks each placement later against the table it selects
//! with the hash ([`placed_by`] / [`same_signature`]).

use crate::aob::Pattern;
use crate::found::Scan;
use crate::signatures::{self, Signature, SignatureSet};
use crate::version::ClientVersion;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

/// Runs `f` with the host's base address and a [`Scan`] over its `.text` with the table of its
/// build. Errors are one line for the worker's log.
pub fn with_scan<R>(f: impl FnOnce(usize, &Scan<'_>) -> Result<R, String>) -> Result<R, String> {
    // SAFETY: a null module name returns the host exe's base, mapped for the life of the
    // process; its first page holds the PE headers and `SizeOfImage` bytes are mapped.
    let (base, image) = unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as *const u8;
        if base.is_null() {
            return Err("cannot get the host module".into());
        }
        let size = crate::pe::Pe::image_size(std::slice::from_raw_parts(base, 0x1000))
            .map_err(|e| e.to_string())?;
        (base as usize, std::slice::from_raw_parts(base, size))
    };
    let pe = crate::pe::Pe::parse_mapped(image).map_err(|e| e.to_string())?;
    let version = pe.file_version().ok_or("the exe has no version resource")?;
    let text = pe.section(".text").ok_or("the exe has no .text")?.clone();
    let (start, len) = (text.virtual_address as usize, text.virtual_size as usize);
    let bytes = image
        .get(start..start + len)
        .ok_or(".text is outside the image")?;
    let dir = signatures::default_dir(std::env::var_os("LOCALAPPDATA"));
    let (set, _) = SignatureSet::load_with_builtin(dir.as_deref());
    // No hash yet: `select` falls back to the plain version key.
    let v = ClientVersion {
        file_version: Some(version.clone()),
        text_hash: String::new(),
        text_rva: text.virtual_address,
        text_len: text.virtual_size,
    };
    let (_, sigs) = set
        .select(&v)
        .ok_or_else(|| format!("no signature table for build {version}"))?;
    f(
        base,
        &Scan {
            sigs,
            text: bytes,
            text_rva: text.virtual_address,
            image_len: image.len() as u32,
        },
    )
}

/// The pattern and `rip` of the entry `name` in `sigs`, to remember what a hook was placed by.
pub fn placed_by(sigs: &[Signature], name: &str) -> Option<(Pattern, Option<usize>)> {
    sigs.iter()
        .find(|s| s.name == name)
        .map(|s| (s.pattern.clone(), s.rip))
}

/// Whether `sigs` (the worker's table, selected with the hash) has the same entry `name` a hook
/// was placed by. Compared, not rescanned: the hook's jump has changed the first bytes by now.
pub fn same_signature(
    sigs: &[Signature],
    name: &str,
    placed: Option<&(Pattern, Option<usize>)>,
) -> bool {
    sigs.iter()
        .find(|s| s.name == name)
        .zip(placed)
        .is_some_and(|(s, (pattern, rip))| s.pattern == *pattern && s.rip == *rip)
}
