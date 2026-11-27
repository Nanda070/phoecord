//! Array-of-bytes signatures: `"48 8B 05 ?? ?? ?? ?? 48 85 C0"`, `??` (or `?`) matching any byte.
//!
//! Pure and platform-independent, so signatures can be unit-tested against dumped byte ranges.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<Option<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad AOB pattern {:?}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Pattern {
    pub fn parse(s: &str) -> Result<Pattern, ParseError> {
        let bytes = s
            .split_whitespace()
            .map(|t| match t {
                "?" | "??" => Ok(None),
                _ if t.len() == 2 => u8::from_str_radix(t, 16)
                    .map(Some)
                    .map_err(|_| ParseError(t.into())),
                _ => Err(ParseError(t.into())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // A pattern of only wildcards matches everywhere: always a mistake.
        if bytes.iter().all(Option::is_none) {
            return Err(ParseError(s.into()));
        }
        Ok(Pattern { bytes })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn matches_at(&self, hay: &[u8], at: usize) -> bool {
        self.bytes
            .iter()
            .zip(&hay[at..])
            .all(|(p, b)| p.is_none_or(|p| p == *b))
    }

    /// The longest run of fixed bytes (the first of equal ones): where it starts in the pattern,
    /// and its bytes. `parse` guarantees at least one fixed byte.
    fn anchor(&self) -> (usize, Vec<u8>) {
        let (mut best, mut start) = ((0, 0), 0);
        for (i, b) in self.bytes.iter().enumerate() {
            if b.is_none() {
                start = i + 1;
            } else if i + 1 - start > best.1 {
                best = (start, i + 1 - start);
            }
        }
        let run = self.bytes[best.0..best.0 + best.1].iter().flatten();
        (best.0, run.copied().collect())
    }

    /// Every offset in `hay` where the pattern matches, in increasing order.
    ///
    /// A client's `.text` is hundreds of MiB and the loader resolves its signatures while the
    /// client is already starting, so the scan looks for the pattern's longest fixed run with
    /// `memchr`'s SIMD substring search and checks the whole pattern only where that run occurs.
    /// Overlapping matches are all reported, as a byte-by-byte scan would.
    pub fn find_all<'a>(&'a self, hay: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
        let end = (hay.len() + 1).saturating_sub(self.len());
        let (off, run) = self.anchor();
        let finder = memchr::memmem::Finder::new(&run).into_owned();
        // `pos` is where the next search for the run starts; one past the last hit, so runs that
        // overlap each other are not skipped.
        let mut pos = off;
        std::iter::from_fn(move || {
            while pos < hay.len() {
                let hit = pos + finder.find(&hay[pos..])?;
                pos = hit + 1;
                let start = hit - off;
                if start >= end {
                    return None;
                }
                if self.matches_at(hay, start) {
                    return Some(start);
                }
            }
            None
        })
    }

    /// The single match, or `None` if there are zero or several (an ambiguous signature is a miss).
    pub fn find_unique(&self, hay: &[u8]) -> Option<usize> {
        let mut it = self.find_all(hay);
        let first = it.next()?;
        it.next().is_none().then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects() {
        assert_eq!(Pattern::parse("48 ?? 05").unwrap().len(), 3);
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("?? ??").is_err());
        assert!(Pattern::parse("4G").is_err());
        assert!(Pattern::parse("488B").is_err());
    }

    #[test]
    fn finds_with_wildcards() {
        let hay = [0x90, 0x48, 0x8B, 0x05, 0x11, 0x22, 0x48, 0x8B, 0x05, 0x33];
        let p = Pattern::parse("48 8B 05 ??").unwrap();
        assert_eq!(p.find_all(&hay).collect::<Vec<_>>(), vec![1, 6]);
        assert_eq!(p.find_unique(&hay), None);
        assert_eq!(Pattern::parse("05 11").unwrap().find_unique(&hay), Some(3));
        assert_eq!(Pattern::parse("05 33").unwrap().find_unique(&hay), Some(8));
        let too_long = Pattern::parse("90 48 8B 05 11 22 48 8B 05 33 00").unwrap();
        assert_eq!(too_long.find_unique(&hay), None);
    }

    /// The byte-by-byte scan `find_all` replaced: the reference for its results.
    fn naive(p: &Pattern, hay: &[u8]) -> Vec<usize> {
        let end = (hay.len() + 1).saturating_sub(p.len());
        (0..end).filter(|&i| p.matches_at(hay, i)).collect()
    }

    #[test]
    fn overlapping_and_edge_matches_are_all_found() {
        let hay = [0xAA; 6];
        let p = Pattern::parse("AA AA").unwrap();
        assert_eq!(p.find_all(&hay).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        // The anchor is the second run; a match must still start at the pattern's first byte.
        let p = Pattern::parse("AA ?? AA AA").unwrap();
        assert_eq!(p.find_all(&hay).collect::<Vec<_>>(), [0, 1, 2]);
        // A leading wildcard cannot start before the haystack.
        let p = Pattern::parse("?? AA").unwrap();
        assert_eq!(p.find_all(&hay).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        // The run occurs at the very end, but the pattern does not fit there.
        let p = Pattern::parse("AA ??").unwrap();
        assert_eq!(p.find_all(&hay[..1]).count(), 0);
    }

    #[test]
    fn the_anchor_is_the_longest_fixed_run() {
        let p = Pattern::parse("48 ?? 8B 05 0D ?? E8 E9").unwrap();
        assert_eq!(p.anchor(), (2, vec![0x8B, 0x05, 0x0D]));
        assert_eq!(Pattern::parse("?? 01").unwrap().anchor(), (1, vec![1]));
    }

    #[test]
    fn matches_the_byte_by_byte_scan() {
        // A small alphabet, so patterns occur often, overlap and cross wildcards.
        let mut x = 0x2545_f491_u32;
        let hay: Vec<u8> = (0..20_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x % 3) as u8
            })
            .collect();
        for pat in [
            "00",
            "01 02",
            "00 ?? 00",
            "?? 01 01 ??",
            "02 02 02",
            "00 ?? ?? 01 02 00",
            "01 00 02 01 00 02 01",
        ] {
            let p = Pattern::parse(pat).unwrap();
            assert_eq!(
                p.find_all(&hay).collect::<Vec<_>>(),
                naive(&p, &hay),
                "{pat}"
            );
        }
    }
}
