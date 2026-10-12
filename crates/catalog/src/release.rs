//! The pinned Suwayomi release and the checksums we accept for it.
//!
//! The digests below were taken from GitHub's release metadata for the tag
//! and cross-checked against the project's own `Checksums.sha256`. Upgrading
//! Suwayomi means changing this file deliberately, never following "latest".

use crate::error::{CatalogError, Result};

pub const VERSION_TAG: &str = "v2.4.2366";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub sha256: &'static str,
    pub size: u64,
    pub kind: ArchiveKind,
}

impl Asset {
    pub fn url(&self) -> String {
        format!(
            "https://github.com/Suwayomi/Suwayomi-Server/releases/download/{VERSION_TAG}/{}",
            self.name
        )
    }
}

struct Entry {
    os: &'static str,
    arch: &'static str,
    platform: &'static str,
    ext: &'static str,
    kind: ArchiveKind,
    sha256: &'static str,
    size: u64,
}

const TABLE: &[Entry] = &[
    Entry {
        os: "windows",
        arch: "x86_64",
        platform: "windows-x64",
        ext: "zip",
        kind: ArchiveKind::Zip,
        sha256: "b955f374a8d94b8d1c2750ae28fa7eb2066b479957561b5ba9cf8ac993c8fc13",
        size: 343_828_201,
    },
    Entry {
        os: "linux",
        arch: "x86_64",
        platform: "linux-x64",
        ext: "tar.gz",
        kind: ArchiveKind::TarGz,
        sha256: "7a469f8c820e65305f795d61f1776c783919884531497b587bdff898a7e0fa73",
        size: 337_841_487,
    },
    Entry {
        os: "macos",
        arch: "aarch64",
        platform: "macOS-arm64",
        ext: "tar.gz",
        kind: ArchiveKind::TarGz,
        sha256: "3b269855928070253a4d9f7c4a57d9ff99d0272102410233c459b02a732e87e7",
        size: 329_964_641,
    },
    Entry {
        os: "macos",
        arch: "x86_64",
        platform: "macOS-x64",
        ext: "tar.gz",
        kind: ArchiveKind::TarGz,
        sha256: "18768b2e3b7fb25bf3eff9e4b4b14b6235b2dfea320701ab172ea7f902eeb97e",
        size: 336_352_325,
    },
];

/// The release archive for a platform, using Rust's `OS` / `ARCH` names.
pub fn asset_for(os: &str, arch: &str) -> Result<Asset> {
    TABLE
        .iter()
        .find(|e| e.os == os && e.arch == arch)
        .map(|e| Asset {
            name: format!("Suwayomi-Server-{VERSION_TAG}-{}.{}", e.platform, e.ext),
            sha256: e.sha256,
            size: e.size,
            kind: e.kind,
        })
        .ok_or_else(|| CatalogError::UnsupportedPlatform(format!("{os}/{arch}")))
}

/// The archive for the machine we are running on.
pub fn current_asset() -> Result<Asset> {
    asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_asset_matches_the_published_name_and_url() {
        let a = asset_for("windows", "x86_64").unwrap();
        assert_eq!(a.name, "Suwayomi-Server-v2.4.2366-windows-x64.zip");
        assert_eq!(a.kind, ArchiveKind::Zip);
        assert_eq!(
            a.url(),
            "https://github.com/Suwayomi/Suwayomi-Server/releases/download/v2.4.2366/Suwayomi-Server-v2.4.2366-windows-x64.zip"
        );
    }

    #[test]
    fn every_digest_is_64_lowercase_hex_chars() {
        for e in TABLE {
            assert_eq!(e.sha256.len(), 64);
            assert!(e
                .sha256
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        }
    }

    #[test]
    fn unknown_platforms_are_reported() {
        assert!(matches!(
            asset_for("windows", "aarch64"),
            Err(CatalogError::UnsupportedPlatform(_))
        ));
    }
}
