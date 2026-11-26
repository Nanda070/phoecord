//! Locator for the HTTP transport's `send` entry (task 2.2.5, part 2).
//!
//! `phoecord-recon`'s second pass (task 2.1.5) pins `reqwest_transport::send` by its own prologue
//! — a verified anchor, matching exactly once in `.text` of the checked build — which is what
//! [`crate::hooks::found`]'s `http_send` inline hook needs: the entry address to patch, not the
//! vtable. The vtable itself (two identical copies in the checked build, at `.rdata` offsets the
//! recon notes give as raw addresses only) is not re-derived here: picking which copy an inline
//! entry hook should care about needs no vtable lookup at all, and resolving the `.rdata` address
//! from scratch by scanning for a pointer equal to `send`'s address would, on this build, come
//! back with two candidates — not a guess the "exactly one or off" rule allows to pick from
//! blindly. [`Transport`]'s `struct_size`/`align` are instead carried as the fixed facts the
//! recon notes read out of either copy, not located by scanning for them.
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// `reqwest_transport::send(&self, &Request) -> BoxFuture<..>`, RVA `0x07923400` in the checked
/// build. 19 leading bytes with no RIP-relative operand (pushes and `sub rsp, imm32`): enough for
/// a 5-byte relative jump, so this also satisfies the "hook targets are checked, not assumed"
/// rule (`docs/phase-2/AGENTS.md`) for an inline entry hook.
const SEND_AOB: &str = "55 41 57 41 56 41 55 41 54 56 57 53 48 81 EC 58 0C 00 00 48 8D AC 24 80 00 00 00 \
     48 C7 85 D0 0B 00 00 FE FF FF FF 48 89 CE 4C 8B BA B0 00 00 00";

/// `dyn Transport`'s `send`. The struct-layout facts are the vtable's `+0x08`/`+0x10` slots, read
/// once from the recon notes, not located per build (they describe the type, not an address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transport {
    /// Entry RVA of `send`, for an inline hook.
    pub send_rva: u32,
    /// Size of `self` behind the vtable (vtable `+0x08`).
    pub struct_size: u32,
    /// Alignment of `self` (vtable `+0x10`).
    pub align: u32,
}

/// Locates `send`'s entry.
pub fn find(image: &Image<'_>) -> Result<Found<Transport>, LocatorError> {
    let pattern = Pattern::parse(SEND_AOB).expect("locator AOB is a fixed, valid pattern");
    let matches = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32);
    let send_rva = exactly_one("ReqwestTransport::send", matches)?;
    Ok(Found::new(
        Transport {
            send_rva,
            struct_size: 0x50,
            align: 8,
        },
        "masked AOB on send's own prologue (phoecord-recon, HTTP transport §Transport vtable)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    fn pattern_bytes() -> Vec<u8> {
        SEND_AOB
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
    fn finds_send_and_carries_the_fixed_layout() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x80];
        text[0x30..0x30 + bytes.len()].copy_from_slice(&bytes);
        let found = find(&image(text)).unwrap();
        assert_eq!(
            found.value,
            Transport {
                send_rva: 0x1030,
                struct_size: 0x50,
                align: 8,
            }
        );
    }

    #[test]
    fn two_copies_are_ambiguous_not_a_guess() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x100];
        text[0x10..0x10 + bytes.len()].copy_from_slice(&bytes);
        text[0x90..0x90 + bytes.len()].copy_from_slice(&bytes);
        assert!(matches!(
            find(&image(text)),
            Err(LocatorError::Ambiguous { count: 2, .. })
        ));
    }

    #[test]
    fn missing_is_not_guessed() {
        let text = vec![0xCCu8; 0x80];
        assert_eq!(
            find(&image(text)),
            Err(LocatorError::Missing {
                what: "ReqwestTransport::send".into()
            })
        );
    }
}
