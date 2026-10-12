//! Live measurement: how many page downloads can run at once?
//!
//!   cargo run -p pipeline --example live_speed -- <suwayomi-parent-dir> <data-dir> <source-id>...
//!
//! For each source it takes the first popular series, then downloads one
//! fresh chapter per concurrency level (so Suwayomi's own page cache cannot
//! flatter later runs) and reports pages per second and any errors. A last
//! run downloads three chapters at once.

use catalog::provision::locate_install;
use catalog::release::VERSION_TAG;
use catalog::{Browse, Catalog, RunningServer, ServerConfig};
use futures_util::stream::{self, StreamExt};
use std::path::PathBuf;
use std::time::Instant;

const LEVELS: &[usize] = &[1, 2, 4, 8, 12];

struct Run {
    pages: usize,
    bytes: usize,
    errors: Vec<String>,
    secs: f32,
}

async fn fetch_chapter<C: Catalog>(client: &C, chapter_id: i64, concurrency: usize) -> Run {
    let started = Instant::now();
    let paths = match client.chapter_pages(chapter_id).await {
        Ok(p) => p,
        Err(e) => {
            return Run {
                pages: 0,
                bytes: 0,
                errors: vec![format!("listing pages: {e}")],
                secs: started.elapsed().as_secs_f32(),
            }
        }
    };
    let results: Vec<_> = stream::iter(paths.iter())
        .map(|p| async move { client.get_bytes(p).await })
        .buffer_unordered(concurrency)
        .collect()
        .await;
    let mut run = Run {
        pages: paths.len(),
        bytes: 0,
        errors: Vec::new(),
        secs: 0.0,
    };
    for r in results {
        match r {
            Ok(b) => run.bytes += b.len(),
            Err(e) => run.errors.push(e.to_string()),
        }
    }
    run.secs = started.elapsed().as_secs_f32();
    run
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let parent: PathBuf = args
        .next()
        .ok_or("usage: live_speed <suwayomi-dir> <data-dir> <source-id>...")?
        .into();
    let data: PathBuf = args.next().ok_or("missing data dir")?.into();
    let source_ids: Vec<String> = args.collect();
    let install = locate_install(&parent.join(format!("suwayomi-{VERSION_TAG}")))
        .or_else(|_| locate_install(&parent))?;
    let config = ServerConfig {
        install,
        data_dir: data,
        browser_support: false,
    };
    let server = RunningServer::start(&config).await?;
    let client = &server.client;
    let sources = client.list_sources().await?;
    println!("{} sources installed", sources.len());

    for id in &source_ids {
        let Some(source) = sources
            .iter()
            .find(|s| &s.id == id || s.name.eq_ignore_ascii_case(id))
        else {
            println!("\n== {id}: not installed, skipped");
            continue;
        };
        println!("\n== {} ({})", source.name, source.id);
        let page = match client.browse(&source.id, &Browse::Popular, 1).await {
            Ok(p) => p,
            Err(e) => {
                println!("   browse failed: {e}");
                continue;
            }
        };
        // Try the first few series until one has enough chapters.
        let mut chapters = Vec::new();
        for manga in page.mangas.iter().take(4) {
            if let Ok(list) = client.fetch_chapters(manga.id).await {
                if list.len() >= LEVELS.len() + 3 {
                    println!("   series: {} ({} chapters)", manga.title, list.len());
                    chapters = list;
                    break;
                }
            }
        }
        if chapters.is_empty() {
            println!("   no series with enough chapters");
            continue;
        }
        chapters.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
        let mut next = chapters.iter().map(|c| c.id);

        println!("   pages-at-once  pages   MB    seconds  pages/s  errors");
        for &level in LEVELS {
            let Some(chapter_id) = next.next() else { break };
            let run = fetch_chapter(client, chapter_id, level).await;
            println!(
                "   {:>13}  {:>5}  {:>5.1}  {:>7.1}  {:>7.2}  {}{}",
                level,
                run.pages,
                run.bytes as f32 / 1e6,
                run.secs,
                run.pages as f32 / run.secs.max(0.001),
                run.errors.len(),
                run.errors
                    .first()
                    .map(|e| format!("  first: {e}"))
                    .unwrap_or_default(),
            );
        }
        // Three chapters at once, four pages each.
        let ids: Vec<i64> = next.by_ref().take(3).collect();
        if ids.len() == 3 {
            let started = Instant::now();
            let runs: Vec<Run> = stream::iter(ids)
                .map(|id| fetch_chapter(client, id, 4))
                .buffer_unordered(3)
                .collect()
                .await;
            let pages: usize = runs.iter().map(|r| r.pages).sum();
            let errors: usize = runs.iter().map(|r| r.errors.len()).sum();
            let secs = started.elapsed().as_secs_f32();
            println!(
                "   3 chapters x 4  {:>5}          {:>7.1}  {:>7.2}  {}",
                pages,
                secs,
                pages as f32 / secs.max(0.001),
                errors
            );
        }
    }
    let _ = server.stop().await;
    Ok(())
}
