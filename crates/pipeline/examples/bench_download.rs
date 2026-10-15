//! How much the download code itself costs, with no network in the way: a fake
//! source serves pages from memory as fast as it can, and the time the whole
//! download takes is the cost of fetching, checking, naming and writing them.
//!   cargo run --release -p pipeline --example bench_download -- [chapters] [pages] [page KB]
use catalog::fake::FakeCatalog;
use library::{DownloadLimits, LibraryFiles};
use pipeline::{download_chapters, CancelFlag, ChapterRef, ChapterRequest, RetryPolicy};
use std::time::Instant;

fn jpeg_like(kb: usize, seed: u8) -> Vec<u8> {
    // A real JPEG of about the size asked for (a noisy picture does not compress).
    let side = ((kb * 1024) as f64 / 0.9).sqrt() as u32;
    let img = image::GrayImage::from_fn(side, side * 3 / 2, |x, y| {
        image::Luma([((x * 7 + y * 13) as u8 ^ seed).wrapping_mul(31)])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::L8)
        .unwrap();
    out
}

#[tokio::main]
async fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (chapters, pages, kb) = (
        args.first().copied().unwrap_or(30),
        args.get(1).copied().unwrap_or(40),
        args.get(2).copied().unwrap_or(250),
    );
    let blobs: Vec<Vec<u8>> = (0..16).map(|s| jpeg_like(kb, s as u8)).collect();
    println!("{chapters} chapters of {pages} pages, each page about {} KB", blobs[0].len() / 1024);
    let catalog = FakeCatalog::new();
    for ch in 1..=chapters as i64 {
        let paths: Vec<String> = (0..pages).map(|i| format!("/c{ch}/p{i}")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        catalog.set_pages(ch, &refs);
        for (i, p) in paths.iter().enumerate() {
            catalog.set_bytes(p, blobs[(ch as usize + i) % blobs.len()].clone());
        }
    }
    let requests: Vec<ChapterRequest> = (1..=chapters as i64)
        .map(|n| ChapterRequest {
            number: Some(n as f64),
            name: format!("Chapter {n}"),
            key: None,
            options: vec![ChapterRef { source_id: "s".into(), chapter_id: n }],
        })
        .collect();
    for shrink in [false, true] {
        let root = std::env::temp_dir().join(format!("mangapoint-bench-dl-{shrink}"));
        let _ = std::fs::remove_dir_all(&root);
        let files = LibraryFiles::new(root.join("library"));
        let started = Instant::now();
        let out = download_chapters(
            &catalog,
            &files,
            "bench-series",
            &requests,
            shrink,
            DownloadLimits::default(),
            &RetryPolicy::default(),
            None,
            &CancelFlag::new(),
        )
        .await
        .unwrap();
        let secs = started.elapsed().as_secs_f64();
        let total: u64 = out.chapters.iter().map(|c| c.bytes).sum();
        let n: usize = out.chapters.iter().map(|c| c.pages).sum();
        println!(
            "shrink={shrink}: {n} pages in {secs:.2}s = {:.0} pages/s, {:.0} MB/s written ({:.0} MB on disk)",
            n as f64 / secs,
            total as f64 / 1e6 / secs,
            total as f64 / 1e6
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
