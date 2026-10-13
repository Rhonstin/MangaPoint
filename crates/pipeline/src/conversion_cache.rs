//! One compact, replaceable conversion per downloaded chapter. Original
//! images remain the source of truth; a missing or damaged cache is a miss.

use crate::error::{PipelineError, Result};
use crate::types::CancelFlag;
use converter::xtc::{Format, PAGE_HEADER_SIZE};
use library::ConversionSettings;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// Bump this whenever conversion or packing behavior changes.
const MAGIC: &[u8; 8] = b"MPCACHE1";
pub(crate) const FILE_NAME: &str = ".mangapoint-converted-v1.bin";

fn fingerprint_header(
    settings: &ConversionSettings,
    width: u32,
    height: u32,
    pages: usize,
) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(MAGIC);
    hash.update(serde_json::to_vec(&(settings, width, height)).expect("settings serialize"));
    hash.update((pages as u64).to_le_bytes());
    hash
}

/// On a miss, fingerprint the exact snapshot handed to the decoder. This
/// avoids publishing a cache under an earlier hash if a download replaces
/// the sources between the cache lookup and conversion.
pub(crate) fn fingerprint_bytes<'a>(
    pages: impl ExactSizeIterator<Item = &'a [u8]>,
    settings: &ConversionSettings,
    width: u32,
    height: u32,
) -> [u8; 32] {
    let mut hash = fingerprint_header(settings, width, height, pages.len());
    for bytes in pages {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

/// Hash actual source contents (including their order), not just timestamps:
/// replacing an image with one of the same size must invalidate the cache.
pub(crate) fn fingerprint(
    pages: &[PathBuf],
    settings: &ConversionSettings,
    width: u32,
    height: u32,
    cancel: &CancelFlag,
) -> Result<[u8; 32]> {
    let mut hash = fingerprint_header(settings, width, height, pages.len());
    let mut buf = [0u8; 64 * 1024];
    for path in pages {
        let io = |e| PipelineError::Io(path.clone(), e);
        let mut file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        hash.update(len.to_le_bytes());
        loop {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let n = file.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
    }
    Ok(hash.finalize().into())
}

/// Empty crop configuration retains existing uncropped cache compatibility.
pub(crate) fn with_crops(source: [u8; 32], configuration: &[u8]) -> [u8; 32] {
    if configuration.is_empty() {
        return source;
    }
    let mut hash = Sha256::new();
    hash.update(b"manual-crop-v1");
    hash.update(source);
    hash.update(configuration);
    hash.finalize().into()
}

pub(crate) fn load(
    path: &Path,
    fingerprint: &[u8; 32],
    width: u32,
    height: u32,
    format: Format,
) -> Option<Vec<Vec<u8>>> {
    let bytes = fs::read(path).ok()?;
    // magic, source fingerprint, page count, packed pages, SHA-256 checksum.
    if bytes.len() < 76 || &bytes[..8] != MAGIC || &bytes[8..40] != fingerprint {
        return None;
    }
    let end = bytes.len() - 32;
    if Sha256::digest(&bytes[..end]).as_slice() != &bytes[end..] {
        return None;
    }
    let count = u32::from_le_bytes(bytes[40..44].try_into().ok()?) as usize;
    if count == 0 || count > u16::MAX as usize {
        return None;
    }
    let size = PAGE_HEADER_SIZE
        + match format {
            Format::Xtc => converter::xtc::xtg_data_size(width, height),
            Format::Xtch => converter::xtc::xth_data_size(width, height),
        };
    if end != 44 + count.checked_mul(size)? {
        return None;
    }
    let mut pages = Vec::with_capacity(count);
    for page in bytes[44..end].chunks_exact(size) {
        if page[..4] != format.page_magic().to_le_bytes()
            || page[4..6] != (width as u16).to_le_bytes()
            || page[6..8] != (height as u16).to_le_bytes()
            || page[8] != 0
            || page[9] != 0
            || page[10..14] != ((size - PAGE_HEADER_SIZE) as u32).to_le_bytes()
        {
            return None;
        }
        pages.push(page.to_vec());
    }
    Some(pages)
}

/// Publish only complete caches. Failure to cache must never prevent an
/// export (for example when the library is read-only or the disk is full).
pub(crate) fn save(path: &Path, fingerprint: &[u8; 32], pages: &[Vec<u8>]) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!(
        "{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> std::io::Result<()> {
        let mut file = std::io::BufWriter::new(File::create(&tmp)?);
        let mut hash = Sha256::new();
        let mut append = |data: &[u8]| -> std::io::Result<()> {
            hash.update(data);
            file.write_all(data)
        };
        append(MAGIC)?;
        append(fingerprint)?;
        append(&(pages.len() as u32).to_le_bytes())?;
        for page in pages {
            append(page)?;
        }
        file.write_all(&hash.finalize())?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, path)
    };
    if write().is_err() {
        let _ = fs::remove_file(tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_snapshot_fingerprint_matches_disk_and_tracks_replacement() {
        let root =
            std::env::temp_dir().join(format!("mangapoint-cache-hash-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let paths = vec![root.join("a.img"), root.join("b.img")];
        fs::write(&paths[0], b"first").unwrap();
        fs::write(&paths[1], b"second").unwrap();
        let settings = ConversionSettings::default();
        let snapshot = [b"first".as_slice(), b"second".as_slice()];
        let original = fingerprint(&paths, &settings, 480, 800, &CancelFlag::new()).unwrap();
        assert_eq!(
            fingerprint_bytes(snapshot.into_iter(), &settings, 480, 800),
            original
        );
        fs::write(&paths[0], b"other").unwrap();
        let replaced = fingerprint(&paths, &settings, 480, 800, &CancelFlag::new()).unwrap();
        assert_ne!(replaced, original);
        let actual_snapshot = [b"other".as_slice(), b"second".as_slice()];
        assert_eq!(
            fingerprint_bytes(actual_snapshot.into_iter(), &settings, 480, 800),
            replaced
        );
        let reversed = [b"second".as_slice(), b"other".as_slice()];
        assert_ne!(
            fingerprint_bytes(reversed.into_iter(), &settings, 480, 800),
            replaced
        );
        let cancelled = CancelFlag::new();
        cancelled.cancel();
        assert!(matches!(
            fingerprint(&paths, &settings, 480, 800, &cancelled),
            Err(PipelineError::Cancelled)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
