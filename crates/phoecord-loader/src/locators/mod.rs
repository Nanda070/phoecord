//! Typed locators (Phase 2, task 2.2.5), first part: the framework.
//!
//! A locator is code that finds one thing in the client binary (a function, a vtable, a field
//! offset) and returns a typed result. The rule (`docs/research/phoenix-desktop.md` §5, written
//! here independently): **exactly one match is success; zero or several is a failure**, and a
//! failure turns off only the feature that needed that locator. The runtime never falls back to an
//! address or offset seen on another build.
//!
//! Building blocks:
//!
//! - [`Image`]: the code (`.text`), read-only data (`.rdata`) and function bounds (`.pdata`) of a
//!   loaded or mock image, by RVA;
//! - [`Image::find_bytes`] / [`Image::find_str`]: string anchors (panic messages, format
//!   templates, `type_name`s);
//! - [`CallIndex`]: built once per image and shared by every locator: who calls which address
//!   (`call rel32`) and who loads which address (RIP-relative `lea` / `mov`);
//! - [`exactly_one`] and [`winner`]: turn candidates into a result or a typed error.
//!
//! Part 2 adds the locators for the hook targets themselves ([`allocator`], [`http_transport`],
//! [`gateway`]), each a masked AOB anchor derived from the private `phoecord-recon` notes (never
//! raw bytes of the dump, per `docs/AGENTS.md`, "Never commit … disassembly"). Every one is
//! checked against this one build of Phoenix (`0.1.0.2208`) only; cross-build verification is
//! task 2.2.9.
//!
//! [`Image::find_ptr`] (8-byte pointer scan of `.rdata`, needs [`Image::image_base`]) is built for
//! the one target this part does not close: `http_poll`, the `.rdata` vtable slot of the response
//! future's `poll`. The recon notes have the `poll` function's own address but not a `.text` site
//! that refers to the vtable slot itself, so there is nothing yet to anchor a locator on without
//! guessing — exactly what this framework exists to avoid. `find_ptr` is tested on its own and
//! waits for that anchor. Tests use mock sections, never Discord bytes.

pub mod allocator;
pub mod gateway;
pub mod http_transport;
mod index;

pub use index::CallIndex;

use crate::pe::Pe;

/// A located value and how it was found (for logs and `phoecord check`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found<T> {
    pub value: T,
    /// A short description of the path: "callers of X ∩ refs to Y".
    pub how: String,
}

impl<T> Found<T> {
    pub fn new(value: T, how: impl Into<String>) -> Found<T> {
        Found {
            value,
            how: how.into(),
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Found<U> {
        Found {
            value: f(self.value),
            how: self.how,
        }
    }
}

/// Why a locator failed. Every variant names what was being looked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocatorError {
    /// No candidate at all: the anchor is gone or the code changed shape.
    Missing { what: String },
    /// Several candidates and no rule to pick one.
    Ambiguous { what: String, count: usize },
    /// A candidate was found but does not decode as expected.
    Bad { what: String, why: String },
}

impl std::fmt::Display for LocatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocatorError::Missing { what } => write!(f, "{what}: not found"),
            LocatorError::Ambiguous { what, count } => {
                write!(f, "{what}: {count} candidates, expected one")
            }
            LocatorError::Bad { what, why } => write!(f, "{what}: {why}"),
        }
    }
}

impl std::error::Error for LocatorError {}

/// A located value or why not.
pub type Located<T> = Result<Found<T>, LocatorError>;

/// The single item, else `Missing` or `Ambiguous`.
pub fn exactly_one<T>(what: &str, items: impl IntoIterator<Item = T>) -> Result<T, LocatorError> {
    let mut it = items.into_iter();
    let Some(first) = it.next() else {
        return Err(LocatorError::Missing { what: what.into() });
    };
    let rest = it.count();
    if rest == 0 {
        Ok(first)
    } else {
        Err(LocatorError::Ambiguous {
            what: what.into(),
            count: rest + 1,
        })
    }
}

/// The candidate with the highest score, if it beats the runner-up by `factor` (a score of `0`
/// never wins). A close race is `Ambiguous`: better off than guessing.
pub fn winner<T>(what: &str, scored: Vec<(T, u32)>, factor: u32) -> Result<T, LocatorError> {
    let mut scored: Vec<(T, u32)> = scored.into_iter().filter(|(_, s)| *s > 0).collect();
    scored.sort_by_key(|c| std::cmp::Reverse(c.1));
    let count = scored.len();
    let mut it = scored.into_iter();
    let Some((best, top)) = it.next() else {
        return Err(LocatorError::Missing { what: what.into() });
    };
    match it.next() {
        Some((_, second)) if u64::from(top) < u64::from(second) * u64::from(factor.max(1)) => {
            Err(LocatorError::Ambiguous {
                what: what.into(),
                count,
            })
        }
        _ => Ok(best),
    }
}

/// A run of bytes at an RVA.
#[derive(Clone, Copy, Debug)]
pub struct Region<'a> {
    pub rva: u32,
    pub bytes: &'a [u8],
}

impl<'a> Region<'a> {
    pub fn contains(&self, rva: u32) -> bool {
        rva >= self.rva && u64::from(rva) < u64::from(self.rva) + self.bytes.len() as u64
    }

    /// The bytes from `rva` to the end of the region.
    pub fn at(&self, rva: u32) -> Option<&'a [u8]> {
        self.contains(rva)
            .then(|| &self.bytes[(rva - self.rva) as usize..])
    }
}

/// What the locators read: code, read-only data and function bounds, all by RVA.
#[derive(Clone, Debug)]
pub struct Image<'a> {
    pub text: Region<'a>,
    pub rdata: Region<'a>,
    /// `(begin, end)` RVAs of every function, sorted by `begin` (`.pdata` on Windows x64).
    pub functions: Vec<(u32, u32)>,
    /// `ImageBase`: what an RVA is added to for comparison against a raw pointer value stored in
    /// `.rdata` (a vtable slot, for instance). `0` for a mock image with no such pointers to find.
    pub image_base: u64,
}

impl<'a> Image<'a> {
    /// From a loaded image (`Pe::parse_mapped`): `.text`, `.rdata` and `.pdata`.
    pub fn from_pe(pe: &Pe<'a>) -> Option<Image<'a>> {
        let region = |name: &str| {
            Some(Region {
                rva: pe.section(name)?.virtual_address,
                bytes: pe.section_bytes(name)?,
            })
        };
        let text = region(".text")?;
        let rdata = region(".rdata").unwrap_or(Region { rva: 0, bytes: &[] });
        let functions = pe
            .section_bytes(".pdata")
            .map(parse_pdata)
            .unwrap_or_default();
        Some(Image {
            text,
            rdata,
            functions,
            image_base: pe.image_base,
        })
    }

    /// Every RVA in `.rdata` holding an 8-byte little-endian pointer equal to `image_base + rva`
    /// (a vtable slot or similar static reference to it), 8-byte aligned to the start of the
    /// section the way the Rust/LLVM codegen that builds such tables does.
    pub fn find_ptr(&self, rva: u32) -> Vec<u32> {
        let va = self.image_base.wrapping_add(u64::from(rva)).to_le_bytes();
        self.rdata
            .bytes
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, w)| **w == va)
            .map(|(i, _)| self.rdata.rva + (i * 8) as u32)
            .collect()
    }

    /// Every RVA in `.rdata` where `needle` starts.
    pub fn find_bytes(&self, needle: &[u8]) -> Vec<u32> {
        if needle.is_empty() || needle.len() > self.rdata.bytes.len() {
            return Vec::new();
        }
        self.rdata
            .bytes
            .windows(needle.len())
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| self.rdata.rva + i as u32)
            .collect()
    }

    /// The single place a string anchor is stored. Rust does not end strings with a zero, so this
    /// is a byte match; an anchor that is also part of a longer string counts every copy.
    pub fn find_str(&self, anchor: &str) -> Result<u32, LocatorError> {
        exactly_one(
            &format!("string {anchor:?}"),
            self.find_bytes(anchor.as_bytes()),
        )
    }

    /// The function an RVA is in, from the function table: its `(begin, end)`.
    pub fn function_of(&self, rva: u32) -> Option<(u32, u32)> {
        let i = self.functions.partition_point(|(begin, _)| *begin <= rva);
        let (begin, end) = *self.functions.get(i.checked_sub(1)?)?;
        (rva < end).then_some((begin, end))
    }
}

/// `.pdata`: 12-byte `RUNTIME_FUNCTION` entries (begin, end, unwind info), sorted by begin.
fn parse_pdata(bytes: &[u8]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|e| {
            let u = |o: usize| u32::from_le_bytes([e[o], e[o + 1], e[o + 2], e[o + 3]]);
            (u(0), u(4))
        })
        .filter(|(b, e)| *b != 0 && e > b)
        .collect();
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image<'a>(text: &'a [u8], rdata: &'a [u8], functions: Vec<(u32, u32)>) -> Image<'a> {
        Image {
            text: Region {
                rva: 0x1000,
                bytes: text,
            },
            rdata: Region {
                rva: 0x8000,
                bytes: rdata,
            },
            functions,
            image_base: 0,
        }
    }

    #[test]
    fn finds_rdata_pointers_to_an_rva() {
        let mut rdata = vec![0u8; 24];
        // A fake vtable: drop(8) + the pointer to text RVA 0x1005 at offset 8, under base 0x9000_0000.
        rdata[8..16].copy_from_slice(&(0x9000_0000u64 + 0x1005).to_le_bytes());
        let mut img = image(&[0; 16], &rdata, vec![]);
        img.image_base = 0x9000_0000;
        assert_eq!(img.find_ptr(0x1005), vec![0x8008]);
        assert_eq!(img.find_ptr(0x1006), Vec::<u32>::new());
    }

    #[test]
    fn exactly_one_and_winner_never_guess() {
        assert_eq!(exactly_one("x", [7]), Ok(7));
        assert_eq!(
            exactly_one::<u8>("x", []),
            Err(LocatorError::Missing { what: "x".into() })
        );
        assert_eq!(
            exactly_one("x", [1, 2, 3]),
            Err(LocatorError::Ambiguous {
                what: "x".into(),
                count: 3
            })
        );
        assert_eq!(winner("w", vec![("a", 9), ("b", 3)], 3), Ok("a"));
        assert!(matches!(
            winner("w", vec![("a", 8), ("b", 3)], 3),
            Err(LocatorError::Ambiguous { count: 2, .. })
        ));
        assert_eq!(winner("w", vec![("a", 1), ("b", 0)], 3), Ok("a"));
        assert!(matches!(
            winner::<&str>("w", vec![("b", 0)], 3),
            Err(LocatorError::Missing { .. })
        ));
        assert_eq!(
            LocatorError::Ambiguous {
                what: "vtable".into(),
                count: 2
            }
            .to_string(),
            "vtable: 2 candidates, expected one"
        );
        assert_eq!(Found::new(5, "how").map(|v| v * 2).value, 10);
    }

    #[test]
    fn string_anchors_and_function_bounds() {
        let rdata = b"..called `Option::unwrap()`..message-content-..message-content-";
        let img = image(&[0x90; 64], rdata, vec![(0x1000, 0x1010), (0x1020, 0x1040)]);
        assert_eq!(img.find_str("called `Option::unwrap()`"), Ok(0x8002));
        assert!(matches!(
            img.find_str("message-content-"),
            Err(LocatorError::Ambiguous { count: 2, .. })
        ));
        assert!(matches!(
            img.find_str("absent"),
            Err(LocatorError::Missing { .. })
        ));
        assert_eq!(img.function_of(0x1005), Some((0x1000, 0x1010)));
        assert_eq!(img.function_of(0x1015), None, "between functions");
        assert_eq!(img.function_of(0x103f), Some((0x1020, 0x1040)));
        assert_eq!(img.function_of(0x0fff), None);
        assert_eq!(img.text.at(0x1002).map(<[u8]>::len), Some(62));
    }

    #[test]
    fn pdata_is_read_and_sorted() {
        let mut pdata = Vec::new();
        for (b, e) in [(0x2000u32, 0x2010u32), (0x1000, 0x1080), (0, 0)] {
            pdata.extend(b.to_le_bytes());
            pdata.extend(e.to_le_bytes());
            pdata.extend(0u32.to_le_bytes());
        }
        assert_eq!(
            parse_pdata(&pdata),
            vec![(0x1000, 0x1080), (0x2000, 0x2010)]
        );
    }
}
