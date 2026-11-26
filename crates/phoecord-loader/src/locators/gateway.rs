//! Locator for the Gateway event registry (task 2.2.5, part 2).
//!
//! `phoecord-recon`'s second pass on the registry (task 2.1.6) found it has **no fixed table
//! address**: it is a per-instance, tree-shaped structure built at `ctx + 0x18` by the function
//! this locator finds (`FUN_147250360` in the dump), guarded by a `Once`-style lock byte at
//! `ctx + 0x10`. So "locating the registry" means locating that builder/query function, which a
//! 2.2.3 detour can hook to see every dispatch; there is no separate static address to resolve.
//!
//! `phoecord check` run against the real Phoenix `0.1.0.2208` exe found the bare prologue AOB
//! ambiguous (2 candidates) — the recon notes' claim that it "already matches exactly once" held
//! only for the slice of `.text` the dump covered, not the whole section (ledger, 2026-11-26,
//! task 2.2.6 part 2). The registry builder's own decompilation (`dumps/transport_dump.txt`)
//! gives a second fact to intersect it with: the exact event-name seed blob it passes to
//! `FUN_1470fa2e0`, [`SEED_BLOB`]. The real builder is the one candidate whose body also
//! references that string — "intersect several facts, don't trust one reference"
//! ([`crate::locators::index`]).
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{CallIndex, Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// Prologue of the registry builder/query function, RVA `0x07250360` in the checked build. Ends
/// in the `lock cmpxchg` on the `ctx + 0x10` init flag (`F0 0F ...`). Ambiguous on its own (see
/// the module doc); paired with [`SEED_BLOB`] below.
const REGISTRY_AOB: &str = "55 56 57 48 83 EC 30 48 8D 6C 24 30 48 C7 45 F8 FE FF FF FF \
     48 89 D6 48 89 CF B1 01 31 C0 F0 0F";

/// The concatenated event-name blob `FUN_147250360` passes to `FUN_1470fa2e0(ctx+0x18, SEED_BLOB,
/// 0xc, ...)` to seed the registry (`dumps/transport_dump.txt`, verbatim). A `.text` site that
/// loads this string's address is the registry builder, whichever candidate the bare prologue
/// AOB landed on too.
const SEED_BLOB: &str = "GUILD_CREATEGUILD_UPDATECHANNEL_CREATERELATIONSHIP_ADDRELATIONSHIP_UPDATE\
     RELATIONSHIP_REMOVEENTITLEMENT_CREATEENTITLEMENT_DELETE";

/// The Gateway event registry's builder/query function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayRegistry {
    pub rva: u32,
}

/// Locates the registry builder/query function: the prologue match that also refers to the seed
/// blob. Either fact alone can be ambiguous (the prologue, confirmed on the real exe; the seed
/// string if it is ever referenced from more than one place) — only their intersection is taken
/// as the answer, never either one by itself.
pub fn find(image: &Image<'_>) -> Result<Found<GatewayRegistry>, LocatorError> {
    let pattern = Pattern::parse(REGISTRY_AOB).expect("locator AOB is a fixed, valid pattern");
    let prologue_matches: Vec<u32> = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32)
        .collect();
    let seed_rva = image.find_str(SEED_BLOB)?;
    let index = CallIndex::build(image);
    let loaders = index.loaders(image, seed_rva);
    let rva = exactly_one(
        "gateway registry builder",
        prologue_matches.into_iter().filter(|m| loaders.contains(m)),
    )?;
    Ok(Found::new(
        GatewayRegistry { rva },
        "masked AOB on the registry builder's prologue, confirmed by a reference to its \
         event-name seed blob (phoecord-recon, Gateway dispatcher ABI; dumps/transport_dump.txt)",
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

    /// `lea rdx, [rip + disp32]` at `at` (text RVA `text_rva + at`), pointing at `target_rva`.
    fn lea(text: &mut [u8], text_rva: u32, at: usize, target_rva: u32) {
        text[at..at + 3].copy_from_slice(&[0x48, 0x8D, 0x15]);
        let end = text_rva + at as u32 + 7;
        let rel = target_rva as i64 - end as i64;
        text[at + 3..at + 7].copy_from_slice(&(rel as i32).to_le_bytes());
    }

    /// Two copies of the bare prologue (the real ambiguity seen on the real exe), 0x100 apart;
    /// only the one at `with_ref` also loads [`SEED_BLOB`]'s address, if `with_ref` is `Some`.
    fn image_with_two_candidates(with_ref: Option<usize>) -> Image<'static> {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x200];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        text[0x120..0x120 + bytes.len()].copy_from_slice(&bytes);
        if let Some(at) = with_ref {
            lea(&mut text, 0x1000, at, 0x8000);
        }
        let rdata = SEED_BLOB.as_bytes().to_vec();
        Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region {
                rva: 0x8000,
                bytes: Box::leak(rdata.into_boxed_slice()),
            },
            functions: vec![(0x1020, 0x1120), (0x1120, 0x1220)],
            image_base: 0,
        }
    }

    #[test]
    fn two_bare_prologue_matches_are_disambiguated_by_the_seed_blob_reference() {
        // The second candidate (0x1120) is the real builder: it alone loads the seed blob,
        // reproducing what `phoecord check` saw on the real exe (prologue ambiguous, 2 candidates).
        let img = image_with_two_candidates(Some(0x150));
        let found = find(&img).unwrap();
        assert_eq!(found.value.rva, 0x1120);
    }

    #[test]
    fn the_prologue_alone_is_ambiguous_and_not_guessed() {
        let img = image_with_two_candidates(None);
        assert_eq!(
            find(&img),
            Err(LocatorError::Missing {
                what: "gateway registry builder".into()
            })
        );
    }

    #[test]
    fn missing_prologue_is_not_guessed() {
        let text = vec![0xCCu8; 0x80];
        let img = Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region {
                rva: 0x8000,
                bytes: SEED_BLOB.as_bytes(),
            },
            functions: vec![],
            image_base: 0,
        };
        assert_eq!(
            find(&img),
            Err(LocatorError::Missing {
                what: "gateway registry builder".into()
            })
        );
    }

    #[test]
    fn missing_seed_blob_is_not_guessed() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x80];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        let img = Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region {
                rva: 0x8000,
                bytes: b"no seed blob here",
            },
            functions: vec![(0x1020, 0x1120)],
            image_base: 0,
        };
        assert!(matches!(find(&img), Err(LocatorError::Missing { .. })));
    }
}
