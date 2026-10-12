//! Live timing of MangaPoint's own downloader on a real series, to compare
//! with another tool on the same chapters.
//!
//!   cargo run -p pipeline --example live_compare -- <suwayomi-dir> <data-dir> <source name> "<title>" <first> <last> <chapters> <pages>
//!
//! Downloads chapters numbered `first..=last` of the first search result
//! whose title contains `<title>` into a temporary library, with the given
//! number of chapters and pages at once, and prints pages per second.

use catalog::provision::locate_install;
use catalog::release::VERSION_TAG;
use catalog::{Browse, RunningServer, ServerConfig};
use library::{DownloadLimits, LibraryFiles};
use pipeline::{download_chapters, CancelFlag, ChapterRef, ChapterRequest, RetryPolicy};
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 8 {
        return Err("usage: live_compare <suwayomi-dir> <data-dir> <source> <title> <first> <last> <chapters> <pages>".into());
    }
    let parent = PathBuf::from(&args[0]);
    let data = PathBuf::from(&args[1]);
    let (source_name, title) = (&args[2], args[3].to_lowercase());
    let (first, last): (f64, f64) = (args[4].parse()?, args[5].parse()?);
    let (chapters, pages): (usize, usize) = (args[6].parse()?, args[7].parse()?);

    let install = locate_install(&parent.join(format!("suwayomi-{VERSION_TAG}")))
        .or_else(|_| locate_install(&parent))?;
    let server = RunningServer::start(&ServerConfig {
        install,
        data_dir: data,
        browser_support: false,
    })
    .await?;
    let client = &server.client;
    let sources = client.list_sources().await?;
    let source = sources
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(source_name))
        .ok_or("source not installed")?;
    let found = client
        .browse(&source.id, &Browse::Search(args[3].clone()), 1)
        .await?;
    let manga = found
        .mangas
        .iter()
        .find(|m| m.title.to_lowercase().contains(&title))
        .ok_or("title not found")?;
    println!("{} on {}", manga.title, source.name);
    let mut list = client.fetch_chapters(manga.id).await?;
    list.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
    let requests: Vec<ChapterRequest> = list
        .iter()
        .filter(|c| c.chapter_number >= first && c.chapter_number <= last)
        .map(|c| ChapterRequest {
            number: Some(c.chapter_number),
            name: c.name.clone(),
            options: vec![ChapterRef {
                source_id: source.id.clone(),
                chapter_id: c.id,
            }],
        })
        .collect();
    println!(
        "{} chapters, {chapters} at a time, {pages} pages at once",
        requests.len()
    );

    let root = std::env::temp_dir().join(format!("mangapoint-compare-{}", std::process::id()));
    let files = LibraryFiles::new(root.join("library"));
    let started = Instant::now();
    let out = download_chapters(
        client,
        &files,
        "compare",
        &requests,
        false,
        DownloadLimits {
            chapters,
            pages,
            spread_sources: false,
            race_sources: false,
        },
        &RetryPolicy::default(),
        None,
        &CancelFlag::new(),
    )
    .await?;
    let secs = started.elapsed().as_secs_f32();
    let total_pages: usize = out.chapters.iter().map(|c| c.pages).sum();
    let mb: f32 = out.chapters.iter().map(|c| c.bytes).sum::<u64>() as f32 / 1e6;
    println!(
        "{} chapters, {total_pages} pages, {mb:.1} MB in {secs:.1} s = {:.2} pages/s, {} skipped",
        out.chapters.len(),
        total_pages as f32 / secs,
        out.skipped.len()
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = server.stop().await;
    Ok(())
}
