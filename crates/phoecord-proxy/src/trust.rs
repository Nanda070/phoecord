//! Add the local CA to the certificate stores Discord reads (and remove it again).

use crate::ca::CA_NAME;
use anyhow::{Context, Result, bail};
use std::path::Path;
#[cfg(all(unix, not(target_os = "macos")))]
use std::path::PathBuf;
use std::process::Command;

/// The SHA-256 of a DER certificate as lower-case hex: what the install log stores.
pub fn sha256_hex(der: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, der).as_ref())
}

/// The SHA-1 thumbprint as lower-case hex (what Windows and the macOS keychain list).
pub fn sha1_hex(der: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, der).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The DER of the certificate in `ca_pem`.
fn read_der(ca_pem: &Path) -> Result<Vec<u8>> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    let pem = std::fs::read(ca_pem).with_context(|| format!("reading {}", ca_pem.display()))?;
    Ok(CertificateDer::from_pem_slice(&pem)
        .context("not a PEM certificate")?
        .to_vec())
}

/// The SHA-256 fingerprint (lower-case hex) of the certificate in `ca_pem`.
pub fn fingerprint(ca_pem: &Path) -> Result<String> {
    Ok(sha256_hex(&read_der(ca_pem)?))
}

/// Does the `security find-certificate -a -Z` listing hold a certificate with this SHA-1 or SHA-256?
pub fn macos_listing_has(listing: &str, sha1: &str, sha256: &str) -> bool {
    listing.lines().any(|l| {
        let l = l.trim();
        l.strip_prefix("SHA-1 hash:")
            .or_else(|| l.strip_prefix("SHA-256 hash:"))
            .map(|h| h.trim().to_lowercase())
            .is_some_and(|h| h == sha1 || h == sha256)
    })
}

/// Does the PEM that `certutil -L -a` printed hold exactly the certificate with this SHA-256?
pub fn pem_text_matches(pem: &str, sha256: &str) -> bool {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    CertificateDer::from_pem_slice(pem.as_bytes()).is_ok_and(|d| sha256_hex(&d) == sha256)
}

/// Is the certificate in `ca_pem` (this exact one, not just one with the same name) in the stores
/// `install` fills? Windows: Current User > Trusted Root, by SHA-1 thumbprint; macOS: the keychains,
/// by hash; Linux: every NSS database that exists must hold it.
pub fn is_trusted(ca_pem: &Path) -> Result<bool> {
    let der = read_der(ca_pem)?;
    let (sha1, sha256) = (sha1_hex(&der), sha256_hex(&der));
    #[cfg(windows)]
    {
        // `-store` only looks the thumbprint up (no chain or revocation check, so it works offline).
        let _ = &sha256;
        let out = Command::new("certutil")
            .args(["-user", "-store", "Root", &sha1])
            .output()
            .context("running certutil")?;
        Ok(out.status.success())
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("security")
            .args(["find-certificate", "-a", "-Z", "-c", CA_NAME])
            .output()
            .context("running security")?;
        Ok(out.status.success()
            && macos_listing_has(&String::from_utf8_lossy(&out.stdout), &sha1, &sha256))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = &sha1;
        let dbs: Vec<_> = nss_dbs()
            .into_iter()
            .filter(|d| d.join("cert9.db").exists())
            .collect();
        if dbs.is_empty() {
            return Ok(false);
        }
        for db in dbs {
            let out = Command::new("certutil")
                .arg("-d")
                .arg(format!("sql:{}", db.display()))
                .args(["-L", "-a", "-n", CA_NAME])
                .output()
                .context("running certutil")?;
            if !out.status.success()
                || !pem_text_matches(&String::from_utf8_lossy(&out.stdout), &sha256)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn run(cmd: &mut Command) -> Result<()> {
    let what = format!("{cmd:?}");
    let out = cmd.output().with_context(|| format!("running {what}"))?;
    if !out.status.success() {
        bail!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// NSS databases Chromium/Electron builds of Discord read on Linux (native, Flatpak, Snap).
#[cfg(all(unix, not(target_os = "macos")))]
fn nss_dbs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![home.join(".pki/nssdb")];
    for app in [
        "com.discordapp.Discord",
        "com.discordapp.DiscordCanary",
        "dev.vencord.Vesktop",
    ] {
        let base = home.join(".var/app").join(app);
        if base.exists() {
            v.push(base.join(".pki/nssdb"));
        }
    }
    let snap = home.join("snap/discord/current");
    if snap.exists() {
        v.push(snap.join(".pki/nssdb"));
    }
    v
}

pub fn install(ca: &Path) -> Result<()> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if Command::new("certutil").arg("-H").output().is_err() {
            bail!(
                "`certutil` is missing: install NSS tools (Arch: nss, Debian/Ubuntu: libnss3-tools, Fedora: nss-tools)"
            );
        }
        for db in nss_dbs() {
            if !db.join("cert9.db").exists() {
                std::fs::create_dir_all(&db)?;
                run(Command::new("certutil")
                    .arg("-d")
                    .arg(format!("sql:{}", db.display()))
                    .args(["-N", "--empty-password"]))?;
            }
            run(Command::new("certutil")
                .arg("-d")
                .arg(format!("sql:{}", db.display()))
                .args(["-A", "-t", "C,,", "-n", CA_NAME, "-i"])
                .arg(ca))?;
            println!("trusted in {}", db.display());
        }
        println!(
            "\nApps that use the system store instead (possibly Discord's native client) need it there too;\n\
             that takes root, so run it yourself:\n  Arch/Fedora:   sudo trust anchor --store {0}\n  \
             Debian/Ubuntu: sudo cp {0} /usr/local/share/ca-certificates/phoecord.crt && sudo update-ca-certificates",
            ca.display()
        );
    }
    #[cfg(windows)]
    {
        // Current-user store: no admin needed; Windows asks you to confirm once.
        run(Command::new("certutil")
            .args(["-user", "-addstore", "Root"])
            .arg(ca))?;
        println!("trusted in your Windows certificate store (Current User > Trusted Root)");
    }
    #[cfg(target_os = "macos")]
    {
        let kc = dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Keychains/login.keychain-db");
        run(Command::new("security")
            .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
            .arg(kc)
            .arg(ca))?;
        println!("trusted in your login keychain");
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    #[cfg(all(unix, not(target_os = "macos")))]
    for db in nss_dbs() {
        if db.join("cert9.db").exists() {
            let _ = run(Command::new("certutil")
                .arg("-d")
                .arg(format!("sql:{}", db.display()))
                .args(["-D", "-n", CA_NAME]));
            println!("removed from {}", db.display());
        }
    }
    #[cfg(windows)]
    run(Command::new("certutil").args(["-user", "-delstore", "Root", CA_NAME]))?;
    #[cfg(target_os = "macos")]
    run(Command::new("security").args(["delete-certificate", "-c", CA_NAME]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_known_vectors() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn macos_listing_matches_either_hash_case_insensitively() {
        let l = "keychain: \"x\"\nSHA-256 hash: AABB\nSHA-1 hash: CCDD\n";
        assert!(macos_listing_has(l, "ccdd", "zz"));
        assert!(macos_listing_has(l, "zz", "aabb"));
        assert!(!macos_listing_has(l, "ee", "ff"));
    }

    #[test]
    fn pem_text_must_be_the_same_certificate() {
        let dir = std::env::temp_dir().join(format!("phoecord-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ca = crate::ca::Authority::load_or_create(&dir).unwrap();
        let pem = std::fs::read_to_string(&ca.ca_pem_path).unwrap();
        let fp = fingerprint(&ca.ca_pem_path).unwrap();
        assert!(pem_text_matches(&pem, &fp));
        assert!(!pem_text_matches(&pem, "00"));
        assert!(!pem_text_matches("garbage", &fp));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
