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
    from_pe(&Pe::parse(exe)?)
}

fn from_pe(pe: &Pe<'_>) -> Result<ClientVersion, VersionError> {
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

/// Identifies a client build from its exe as mapped in memory (`image` starts at the module base
/// and spans `SizeOfImage`). The `.text` hash equals the one [`detect`] gives for the file.
pub fn detect_mapped(image: &[u8]) -> Result<ClientVersion, VersionError> {
    from_pe(&Pe::parse_mapped(image)?)
}

/// SHA-256 of `.text` as stored in the exe file at `exe`, with the span taken from the headers of
/// `image` (the same exe as mapped in this process). Equal to [`detect`]'s `text_hash` for that
/// file. The loader identifies the build with it, not with the mapped bytes: another DLL loaded
/// into the client before the loader's worker runs (a mod such as Sunbird) may already have
/// patched code in memory, which would change the hash of an unchanged build. Streams the file,
/// so the whole exe (hundreds of MB) is never in memory at once.
pub fn file_text_hash(image: &[u8], exe: &std::path::Path) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let bad =
        |e: &dyn fmt::Display| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string());
    let pe = Pe::parse_mapped(image).map_err(|e| bad(&e))?;
    let sect = pe
        .section(".text")
        .ok_or_else(|| bad(&VersionError::NoTextSection))?;
    let mut left = u64::from(sect.raw_size.min(sect.virtual_size));
    let mut f = std::fs::File::open(exe)?;
    f.seek(SeekFrom::Start(u64::from(sect.raw_offset)))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    while left > 0 {
        let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        let n = f.read(&mut buf[..want])?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Electron's child processes (renderer, GPU, utility, crashpad) carry `--type=` on their command
/// line; the main client process does not. Only the main process needs the loader's work.
pub fn is_child_process(command_line: &str) -> bool {
    command_line.contains("--type=")
}

/// Which Discord desktop client the loader runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientKind {
    /// The native Rust client (Desktop Phoenix): the in-process hooks of Phase 2 apply.
    Phoenix,
    /// The Electron client (Stable, PTB, Canary): no Phoenix hook applies, the client must run as
    /// it would without the loader.
    Electron,
}

impl ClientKind {
    /// Tells the clients apart by where the executable lives: Phoenix always installs into one
    /// fixed folder, `%LOCALAPPDATA%\DiscordPhoenix\app\` (2.1.1), and no Electron client uses a
    /// folder of that name. The match is on a whole path component, case-insensitive, with either
    /// separator. `file_version` is not used to decide today (both clients carry one); it is taken
    /// so a later rule can use it without changing callers.
    pub fn of(exe_path: &str, file_version: Option<&str>) -> ClientKind {
        let _ = file_version;
        let phoenix = exe_path
            .split(['\\', '/'])
            .any(|part| part.eq_ignore_ascii_case("DiscordPhoenix"));
        if phoenix {
            ClientKind::Phoenix
        } else {
            ClientKind::Electron
        }
    }
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
    fn the_file_hash_ignores_code_patched_in_memory() {
        let code: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let exe = build_pe(&code, &[], Some((0, 1, 0, 2208)));
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("Discord.exe");
        std::fs::write(&path, &exe).unwrap();
        // build_pe lays sections out with RVA == file offset, so the bytes double as the image.
        let mut image = exe.clone();
        let from_file = detect(&exe).unwrap().text_hash;
        assert_eq!(file_text_hash(&image, &path).unwrap(), from_file);
        // Another DLL patches a function in memory: the mapped hash moves, the file hash does not.
        image[0x200 + 100] ^= 0xFF;
        assert_ne!(detect_mapped(&image).unwrap().text_hash, from_file);
        assert_eq!(file_text_hash(&image, &path).unwrap(), from_file);
        // A file shorter than its headers say is an error, not a wrong hash.
        std::fs::write(&path, &exe[..0x300]).unwrap();
        assert!(file_text_hash(&image, &path).is_err());
    }

    #[test]
    fn same_version_different_code_differs_in_hash() {
        let a = detect(&build_pe(&[1, 2, 3], &[], Some((1, 0, 0, 1)))).unwrap();
        let b = detect(&build_pe(&[1, 2, 4], &[], Some((1, 0, 0, 1)))).unwrap();
        assert_eq!(a.file_version, b.file_version);
        assert_ne!(a.text_hash, b.text_hash);
    }

    #[test]
    fn child_process_check() {
        assert!(is_child_process(
            r#""C:\D\Discord.exe" --type=renderer --user-data-dir="C:\x" --lang=en"#
        ));
        assert!(is_child_process("Discord.exe --type=gpu-process"));
        assert!(!is_child_process(
            r#""C:\D\Discord.exe" --processStart Discord.exe"#
        ));
        assert!(!is_child_process(""));
    }

    #[test]
    fn not_a_pe() {
        assert!(detect(b"nope").is_err());
    }

    #[test]
    fn phoenix_is_told_apart_from_electron_by_its_folder() {
        let phoenix = r"C:\Users\a\AppData\Local\DiscordPhoenix\app\Discord.exe";
        assert_eq!(
            ClientKind::of(phoenix, Some("0.1.0.2208")),
            ClientKind::Phoenix
        );
        assert_eq!(
            ClientKind::of(&phoenix.to_lowercase(), None),
            ClientKind::Phoenix
        );
        assert_eq!(
            ClientKind::of("/home/a/.local/share/DiscordPhoenix/app/Discord", None),
            ClientKind::Phoenix
        );
        for electron in [
            r"C:\Users\a\AppData\Local\Discord\app-1.0.9200\Discord.exe",
            r"C:\Users\a\AppData\Local\DiscordCanary\app-1.0.1364\DiscordCanary.exe",
            r"C:\Users\a\AppData\Local\DiscordPTB\app-1.0.1200\DiscordPTB.exe",
            // A component that only contains the name is not the Phoenix folder.
            r"C:\x\DiscordPhoenixBackup\Discord.exe",
            "",
        ] {
            assert_eq!(
                ClientKind::of(electron, Some("1.0.9200.0")),
                ClientKind::Electron,
                "{electron}"
            );
        }
    }
}
