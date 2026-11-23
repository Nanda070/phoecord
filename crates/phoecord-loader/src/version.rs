//! Client build identification: the file version from the exe's version resource plus a SHA-256 of
//! its `.text` section. The version picks the signature table; the hash tells two builds with the
//! same version number apart (Discord ships hotfixes without bumping it).

use crate::pe::{Pe, PeError};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientVersion {
    /// `a.b.c.d` from the version resource, if the exe has one.
    pub file_version: Option<String>,
    /// Lowercase hex SHA-256 of the `.text` section as stored in the file.
    pub text_hash: String,
    /// RVA of `.text`, to turn scan offsets into addresses (`module base + rva`).
    pub text_rva: u32,
    /// In-memory size of `.text` (virtual size), the span to scan in the running process.
    pub text_len: u32,
}

#[derive(Debug)]
pub enum VersionError {
    Pe(PeError),
    NoTextSection,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::Pe(e) => e.fmt(f),
            VersionError::NoTextSection => f.write_str("executable has no .text section"),
        }
    }
}

impl std::error::Error for VersionError {}

impl From<PeError> for VersionError {
    fn from(e: PeError) -> Self {
        VersionError::Pe(e)
    }
}

/// Lowercase hex SHA-256 of a byte slice.
pub fn hash_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Identifies a client build from the bytes of its exe file.
pub fn detect(exe: &[u8]) -> Result<ClientVersion, VersionError> {
    let pe = Pe::parse(exe)?;
    let text = pe
        .section_bytes(".text")
        .ok_or(VersionError::NoTextSection)?;
    let sect = pe.section(".text").ok_or(VersionError::NoTextSection)?;
    Ok(ClientVersion {
        file_version: pe.file_version(),
        text_hash: hash_hex(text),
        text_rva: sect.virtual_address,
        text_len: sect.virtual_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::testutil::build_pe;

    #[test]
    fn known_hash() {
        assert_eq!(
            hash_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn detects_version_and_text_hash() {
        let exe = build_pe(&[0x90, 0x90, 0xC3], &[], Some((1, 0, 9, 123)));
        let v = detect(&exe).unwrap();
        assert_eq!(v.file_version.as_deref(), Some("1.0.9.123"));
        assert_eq!(v.text_hash, hash_hex(&[0x90, 0x90, 0xC3]));
        assert_eq!(v.text_rva, 0x200);
        assert_eq!(v.text_len, 3);
    }

    #[test]
    fn same_version_different_code_differs_in_hash() {
        let a = detect(&build_pe(&[1, 2, 3], &[], Some((1, 0, 0, 1)))).unwrap();
        let b = detect(&build_pe(&[1, 2, 4], &[], Some((1, 0, 0, 1)))).unwrap();
        assert_eq!(a.file_version, b.file_version);
        assert_ne!(a.text_hash, b.text_hash);
    }

    #[test]
    fn not_a_pe() {
        assert!(detect(b"nope").is_err());
    }
}
