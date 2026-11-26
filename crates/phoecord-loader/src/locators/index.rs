//! [`CallIndex`]: who calls and who loads which address, built once per image.
//!
//! x64 code is not decoded instruction by instruction here. The index looks for the three
//! instruction forms locators need and checks that each one makes sense:
//!
//! - `E8 rel32` (`call`): kept when the target is a function start (from `.pdata`), or, in an
//!   image without a function table, inside `.text`;
//! - `[REX.W] 8D /r` with `mod = 00, r/m = 101` (`lea reg, [rip + disp32]`) and the same with
//!   `8B` (`mov reg, [rip + disp32]`): kept when the target is inside `.text` or `.rdata`.
//!
//! A byte scan can see such a shape inside another instruction's operands. Requiring a sensible
//! target removes most of those, and locators intersect several facts (callers of A that also load
//! B) rather than trusting one reference.

use super::Image;
use std::collections::HashMap;

/// Who calls and who loads which RVA.
#[derive(Debug, Default)]
pub struct CallIndex {
    /// Target RVA → RVAs of the `call` instructions.
    calls: HashMap<u32, Vec<u32>>,
    /// Target RVA → RVAs of the RIP-relative `lea` / `mov` instructions.
    refs: HashMap<u32, Vec<u32>>,
}

/// `rel` added to the RVA right after an instruction ending at `next`.
fn target(next: u32, rel: i32) -> Option<u32> {
    u32::try_from(i64::from(next) + i64::from(rel)).ok()
}

impl CallIndex {
    /// Scan `.text` once.
    pub fn build(image: &Image<'_>) -> CallIndex {
        let mut index = CallIndex::default();
        let code = image.text.bytes;
        let base = image.text.rva;
        let is_function_start = |rva: u32| {
            if image.functions.is_empty() {
                image.text.contains(rva)
            } else {
                image
                    .functions
                    .binary_search_by_key(&rva, |(begin, _)| *begin)
                    .is_ok()
            }
        };
        let in_image = |rva: u32| image.text.contains(rva) || image.rdata.contains(rva);
        let disp = |at: usize| {
            code.get(at..at + 4)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        for i in 0..code.len() {
            let here = base + i as u32;
            // call rel32
            if code[i] == 0xE8
                && let Some(rel) = disp(i + 1)
                && let Some(t) = target(here + 5, rel)
                && is_function_start(t)
            {
                index.calls.entry(t).or_default().push(here);
            }
            // [REX] 8D/8B modrm(mod=00, rm=101) disp32
            let (op_at, start) = match code[i] {
                0x40..=0x4F => (i + 1, i),
                _ => (i, i),
            };
            if matches!(code.get(op_at), Some(0x8D | 0x8B))
                && code
                    .get(op_at + 1)
                    .is_some_and(|m| m & 0b1100_0111 == 0b0000_0101)
                && let Some(rel) = disp(op_at + 2)
            {
                let len = (op_at + 6 - start) as u32;
                if let Some(t) = target(base + start as u32 + len, rel)
                    && in_image(t)
                {
                    let list = index.refs.entry(t).or_default();
                    // A REX-prefixed match is also seen without the prefix one byte later.
                    if list.last() != Some(&(base + start as u32 - 1)) {
                        list.push(base + start as u32);
                    }
                }
            }
        }
        index
    }

    /// RVAs of the `call`s to `target`.
    pub fn calls_to(&self, target: u32) -> &[u32] {
        self.calls.get(&target).map_or(&[], Vec::as_slice)
    }

    /// RVAs of the instructions that load `target`'s address or value.
    pub fn refs_to(&self, target: u32) -> &[u32] {
        self.refs.get(&target).map_or(&[], Vec::as_slice)
    }

    /// The functions (`begin` RVAs) that call `target`, each once, in order.
    pub fn callers(&self, image: &Image<'_>, target: u32) -> Vec<u32> {
        functions_of(image, self.calls_to(target))
    }

    /// The functions (`begin` RVAs) that load `target`, each once, in order.
    pub fn loaders(&self, image: &Image<'_>, target: u32) -> Vec<u32> {
        functions_of(image, self.refs_to(target))
    }
}

fn functions_of(image: &Image<'_>, sites: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = sites
        .iter()
        .filter_map(|s| image.function_of(*s).map(|(begin, _)| begin))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::super::{Image, Region, exactly_one};
    use super::*;

    /// Mock code: three functions; f1 at 0x1000 loads the string and calls f3, f2 at 0x1020
    /// calls f3 twice, f3 at 0x1040 is a leaf.
    fn code() -> Vec<u8> {
        let mut c = vec![0xCC; 0x60];
        // f1 @0x1000: lea rcx, [rip + disp] → 0x8004 ; call f3
        let lea = 0x1000u32 + 7;
        c[0..3].copy_from_slice(&[0x48, 0x8D, 0x0D]);
        c[3..7].copy_from_slice(&((0x8004i64 - i64::from(lea)) as i32).to_le_bytes());
        c[7] = 0xE8;
        c[8..12].copy_from_slice(&((0x1040i64 - 0x100C) as i32).to_le_bytes());
        c[12] = 0xC3;
        // f2 @0x1020: call f3 ; call f3 ; ret
        for (at, next) in [(0x20usize, 0x1025i64), (0x25, 0x102A)] {
            c[at] = 0xE8;
            c[at + 1..at + 5].copy_from_slice(&((0x1040 - next) as i32).to_le_bytes());
        }
        c[0x2A] = 0xC3;
        // f3 @0x1040: mov rax, [rip + disp] → 0x8000 ; ret
        c[0x40..0x43].copy_from_slice(&[0x48, 0x8B, 0x05]);
        c[0x43..0x47].copy_from_slice(&((0x8000i64 - 0x1047) as i32).to_le_bytes());
        c[0x47] = 0xC3;
        c
    }

    #[test]
    fn indexes_calls_and_rip_relative_loads() {
        let code = code();
        let img = Image {
            text: Region {
                rva: 0x1000,
                bytes: &code,
            },
            rdata: Region {
                rva: 0x8000,
                bytes: b"....anchor",
            },
            functions: vec![(0x1000, 0x1010), (0x1020, 0x1030), (0x1040, 0x1050)],
            image_base: 0,
        };
        let idx = CallIndex::build(&img);
        assert_eq!(idx.calls_to(0x1040), &[0x1007, 0x1020, 0x1025]);
        assert_eq!(idx.callers(&img, 0x1040), vec![0x1000, 0x1020]);
        assert_eq!(idx.refs_to(0x8004), &[0x1000], "one entry for the REX form");
        assert_eq!(idx.loaders(&img, 0x8000), vec![0x1040]);

        // A locator: the single caller of f3 that also loads the "anchor" string.
        let anchor = img.find_str("anchor").unwrap();
        let callers = idx.callers(&img, 0x1040);
        let found = exactly_one(
            "caller of f3 loading \"anchor\"",
            callers
                .into_iter()
                .filter(|f| idx.loaders(&img, anchor).contains(f)),
        );
        assert_eq!(found, Ok(0x1000));
    }

    #[test]
    fn targets_that_make_no_sense_are_dropped() {
        // A call into the middle of a function, and a lea far outside the image.
        let mut code = vec![0x90; 0x20];
        code[0] = 0xE8;
        code[1..5].copy_from_slice(&3i32.to_le_bytes());
        code[5..8].copy_from_slice(&[0x48, 0x8D, 0x05]);
        code[8..12].copy_from_slice(&0x7000_0000i32.to_le_bytes());
        let img = Image {
            text: Region {
                rva: 0x1000,
                bytes: &code,
            },
            rdata: Region {
                rva: 0x8000,
                bytes: b"",
            },
            functions: vec![(0x1000, 0x1020)],
            image_base: 0,
        };
        let idx = CallIndex::build(&img);
        assert!(idx.calls_to(0x1008).is_empty());
        assert!(idx.refs.is_empty());
    }
}
