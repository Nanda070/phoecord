//! Locator for the allocator shims (task 2.2.5, part 2): `__rust_alloc` and `__rust_dealloc`.
//!
//! A hook that replaces a request or response body has to allocate and free it through the
//! host's own allocator, not ours — both shims keep an allocation counter the other side checks
//! (`phoecord-recon`, "HTTP transport (2.1.5, second pass)"), so a body freed the wrong way
//! corrupts that counter. Each shim's prologue is a short, clean run of whole instructions with
//! no RIP-relative operand, so the AOB resolves straight to the shim's own entry; no `rip` follow
//! is needed, unlike [`crate::locators::http_transport`].
//!
//! Checked against Phoenix `0.1.0.2208` only (one dump); not yet verified on a second build
//! (task 2.2.9, before this goes into a signature table used in anger).

use super::{Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// `__rust_alloc(size, align) -> *mut u8` (null on failure), RVA `0x0456d100` in the checked
/// build. 11,804 call sites in that dump — the allocator's whole fan-in, not one call shape.
const ALLOC_AOB: &str = "56 57 48 83 EC 28 48 89 D7 48 89 CE E8 ?? ?? ?? ?? 84 C0 74 26 48 8D 0D ?? ?? ?? ?? \
     48 89 FA 49 89 F0 E8 ?? ?? ?? ?? 48 85 C0 75 11 48 8D 0D ?? ?? ?? ?? 48 89 F2 E8 ?? ?? ?? ?? \
     31 C0 48 83 C4 28 5F 5E C3 CC";

/// `__rust_dealloc(ptr, size, align)`, RVA `0x0456d150`. 28,014 call sites.
const DEALLOC_AOB: &str = "56 48 83 EC 20 48 89 D6 48 89 CA 48 8D 0D ?? ?? ?? ??";

/// The host's allocator shims, both or neither: pairing one build's `alloc` with another
/// build's `dealloc` (if the signature table were ever mismatched) would corrupt the heap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocator {
    pub alloc_rva: u32,
    pub dealloc_rva: u32,
}

fn find_one(image: &Image<'_>, aob: &str, what: &str) -> Result<u32, LocatorError> {
    let pattern = Pattern::parse(aob).expect("locator AOB is a fixed, valid pattern");
    let matches = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32);
    exactly_one(what, matches)
}

/// Locates both shims. Each is checked on its own, so a miss on one names which.
pub fn find(image: &Image<'_>) -> Result<Found<Allocator>, LocatorError> {
    let alloc_rva = find_one(image, ALLOC_AOB, "__rust_alloc")?;
    let dealloc_rva = find_one(image, DEALLOC_AOB, "__rust_dealloc")?;
    Ok(Found::new(
        Allocator {
            alloc_rva,
            dealloc_rva,
        },
        "masked AOB on the shim's own prologue (phoecord-recon, HTTP transport §Allocator)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    /// Lays `bytes` into a `0x200`-byte `.text` at `at`, padded with `0xCC` (an invalid opcode,
    /// never a false match for these patterns).
    fn text_with(bytes: &[(usize, &[u8])]) -> Vec<u8> {
        let mut t = vec![0xCCu8; 0x200];
        for (at, b) in bytes {
            t[*at..*at + b.len()].copy_from_slice(b);
        }
        t
    }

    fn pattern_bytes(aob: &str) -> Vec<u8> {
        // Wildcards become 0x90 (nop): any byte that is not the 0xCC padding works for a match.
        aob.split_whitespace()
            .map(|t| {
                if t == "??" {
                    0x90
                } else {
                    u8::from_str_radix(t, 16).unwrap()
                }
            })
            .collect()
    }

    fn image(text: Vec<u8>) -> Image<'static> {
        Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region { rva: 0, bytes: &[] },
            functions: vec![],
            image_base: 0,
        }
    }

    #[test]
    fn finds_both_shims() {
        let alloc = pattern_bytes(ALLOC_AOB);
        let dealloc = pattern_bytes(DEALLOC_AOB);
        let text = text_with(&[(0x10, &alloc), (0x100, &dealloc)]);
        let img = image(text);
        let found = find(&img).unwrap();
        assert_eq!(found.value.alloc_rva, 0x1010);
        assert_eq!(found.value.dealloc_rva, 0x1100);
    }

    #[test]
    fn missing_dealloc_fails_even_with_alloc_found() {
        let alloc = pattern_bytes(ALLOC_AOB);
        let text = text_with(&[(0x10, &alloc)]);
        let img = image(text);
        assert_eq!(
            find(&img),
            Err(LocatorError::Missing {
                what: "__rust_dealloc".into()
            })
        );
    }

    #[test]
    fn two_copies_are_ambiguous_not_a_guess() {
        let alloc = pattern_bytes(ALLOC_AOB);
        let text = text_with(&[(0x10, &alloc), (0x80, &alloc)]);
        let img = image(text);
        assert!(matches!(
            find(&img),
            Err(LocatorError::Ambiguous { count: 2, .. })
        ));
    }
}
