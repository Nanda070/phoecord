//! Locator for the Gateway event registry (task 2.2.5, part 2).
//!
//! `phoecord-recon`'s second pass on the registry (task 2.1.6) found it has **no fixed table
//! address**: it is a per-instance, tree-shaped structure built at `ctx + 0x18` by the function
//! this locator finds (`FUN_147250360` in the dump), guarded by a `Once`-style lock byte at
//! `ctx + 0x10`. So "locating the registry" means locating that builder/query function, which a
//! 2.2.3 detour can hook to see every dispatch; there is no separate static address to resolve.
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// Prologue of the registry builder/query function, RVA `0x07250360` in the checked build. Ends
/// in the `lock cmpxchg` on the `ctx + 0x10` init flag (`F0 0F ...`); the registry-offset and
/// seed-blob cross-references `phoecord-recon` uses to confirm the match are not needed here —
/// masked, this prologue alone already matches exactly once in `.text`.
const REGISTRY_AOB: &str = "55 56 57 48 83 EC 30 48 8D 6C 24 30 48 C7 45 F8 FE FF FF FF \
     48 89 D6 48 89 CF B1 01 31 C0 F0 0F";

/// The Gateway event registry's builder/query function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayRegistry {
    pub rva: u32,
}

/// Locates the registry builder/query function.
pub fn find(image: &Image<'_>) -> Result<Found<GatewayRegistry>, LocatorError> {
    let pattern = Pattern::parse(REGISTRY_AOB).expect("locator AOB is a fixed, valid pattern");
    let matches = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32);
    let rva = exactly_one("gateway registry builder", matches)?;
    Ok(Found::new(
        GatewayRegistry { rva },
        "masked AOB on the registry builder's prologue (phoecord-recon, Gateway dispatcher ABI)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    fn pattern_bytes() -> Vec<u8> {
        REGISTRY_AOB
            .split_whitespace()
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
    fn finds_the_registry_builder() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x80];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        let found = find(&image(text)).unwrap();
        assert_eq!(found.value.rva, 0x1020);
    }

    #[test]
    fn missing_is_not_guessed() {
        let text = vec![0xCCu8; 0x80];
        assert_eq!(
            find(&image(text)),
            Err(LocatorError::Missing {
                what: "gateway registry builder".into()
            })
        );
    }
}
