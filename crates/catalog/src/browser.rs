//! "Browser support": the optional Chromium that Suwayomi downloads for
//! sources which need a real browser (token or Cloudflare checks).
//!
//! It is off by default. When the user turns it on, the next server start
//! downloads about 270 MB (about 560 MB once unpacked) into the server's data
//! folder; turning it off and removing it deletes that folder again.
//! macOS is not supported by Suwayomi's embedded Chromium.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Shown before the user removes browser support.
pub const REMOVE_WARNING: &str = "Browser support is needed to download from some sources. \
We recommend you keep it. Without it, those sources will stop working until you turn it back on.";

/// Shown when asking the user to turn it on.
pub const ENABLE_PROMPT: &str = "This source needs browser support. \
It downloads about 270 MB once and uses about 560 MB of disk space. Turn it on?";

/// Suwayomi's embedded Chromium is not available on macOS.
pub fn supported() -> bool {
    !cfg!(target_os = "macos")
}

/// Where the Chromium runtime lives inside the server's data directory.
pub fn runtime_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("bin").join("kcef")
}

fn profile_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("kcef")
}

/// Is a Chromium runtime present on disk?
pub fn is_installed(data_dir: &Path) -> bool {
    fs::read_dir(runtime_dir(data_dir))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// Disk space used by the runtime and its browser profile, in bytes.
pub fn size_on_disk(data_dir: &Path) -> u64 {
    dir_size(&runtime_dir(data_dir)) + dir_size(&profile_dir(data_dir))
}

/// Delete the Chromium runtime and its browser profile.
///
/// The server must be stopped first: the files are in use while it runs.
pub fn remove(data_dir: &Path) -> io::Result<()> {
    for dir in [runtime_dir(data_dir), profile_dir(data_dir)] {
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadProgress {
    pub percent: u8,
    pub total_mb: f32,
}

/// The latest download progress mentioned in a server log, if any.
///
/// The server logs lines like `Downloading 87% of 268.2 MB` while it fetches
/// the runtime; the UI polls this to draw a progress bar.
pub fn download_progress(log_tail: &str) -> Option<DownloadProgress> {
    log_tail.lines().rev().find_map(parse_progress_line)
}

fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    let rest = line.split("Downloading ").nth(1)?;
    let (percent, rest) = rest.split_once("% of ")?;
    let total = rest.split_whitespace().next()?;
    Some(DownloadProgress {
        percent: percent.trim().parse().ok()?,
        total_mb: total.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_log_format() {
        let log = "23:00:12.274 [x] INFO suwayomi.tachidesk.server.util.CEFManager -- Downloading CEF from Github (jbr)\n\
                   23:00:13.230 [x] INFO suwayomi.tachidesk.server.util.CEFManager -- Downloading 1% of 268.2 MB\n\
                   23:00:15.789 [x] INFO suwayomi.tachidesk.server.util.CEFManager -- Downloading 87% of 268.2 MB\n\
                   23:00:20.779 [x] INFO suwayomi.tachidesk.server.util.CEFManager -- Downloaded CEF successfully!";
        assert_eq!(
            download_progress(log),
            Some(DownloadProgress {
                percent: 87,
                total_mb: 268.2
            })
        );
        assert_eq!(download_progress("nothing here"), None);
    }

    #[test]
    fn remove_deletes_runtime_and_profile() {
        let data = std::env::temp_dir().join(format!("mangapoint-kcef-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(runtime_dir(&data)).unwrap();
        fs::create_dir_all(profile_dir(&data)).unwrap();
        fs::write(runtime_dir(&data).join("libcef.dll"), vec![0u8; 1000]).unwrap();
        fs::write(profile_dir(&data).join("cookies"), vec![0u8; 24]).unwrap();
        assert!(is_installed(&data));
        assert_eq!(size_on_disk(&data), 1024);
        remove(&data).unwrap();
        assert!(!is_installed(&data));
        assert_eq!(size_on_disk(&data), 0);
        remove(&data).unwrap(); // removing twice is fine
        let _ = fs::remove_dir_all(data);
    }
}
