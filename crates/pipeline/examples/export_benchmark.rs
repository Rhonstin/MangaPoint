//! Time complete exports twice, including final file writing. The first run
//! populates per-chapter caches; the second reuses them. Original images and
//! library metadata are unchanged. Output goes only to the supplied folder.
//!
//! Add `--fresh` to run once with zero cache reuse, measuring first-export
//! conversion even if the chapters already have caches.
//!
//! cargo run --release -p pipeline --example export_benchmark -- <library-dir> <library.json> "<title part>" <output-dir> [chapters] [--fresh]

use library::{JsonFile, LibraryDb, LibraryFiles};
use pipeline::{CancelFlag, ExportJob, ExportTarget, JobEvent};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("usage: export_benchmark <library-dir> <library.json> <title part> <output-dir> [chapters]".into());
    }
    let files = LibraryFiles::new(&args[0]);
    let db: LibraryDb = JsonFile::new(&args[1]).load();
    let wanted = args[2].to_lowercase();
    let series = db
        .series
        .iter()
        .find(|s| s.title.to_lowercase().contains(&wanted))
        .ok_or("no such series")?;
    let take = args
        .get(4)
        .filter(|arg| !arg.starts_with("--"))
        .map(|n| n.parse::<usize>())
        .transpose()?
        .unwrap_or(series.chapters.len());
    let output = PathBuf::from(&args[3]);
    let mut job = ExportJob::new(
        series.clone(),
        series
            .chapters
            .iter()
            .take(take)
            .map(|c| c.key.clone())
            .collect(),
        device_profiles::x4_pro(),
        ExportTarget::Folder {
            path: output.join("out"),
        },
        output.join("work"),
    );
    job.force_conversion = args.iter().any(|arg| arg == "--fresh");
    println!(
        "{}: {} selected chapters, {}x{}, {:?}",
        series.title,
        job.chapter_keys.len(),
        job.device.width,
        job.device.height,
        series.settings.format
    );
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        for run in 1..=if job.force_conversion { 1 } else { 2 } {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let progress = tokio::spawn(async move {
                let mut completed = 0;
                while let Some(event) = rx.recv().await {
                    if let JobEvent::ChapterDone { .. } = event {
                        completed += 1;
                        if completed % 25 == 0 {
                            println!("  run {run}: {completed} chapters completed");
                        }
                    }
                }
            });
            let started = Instant::now();
            let result =
                pipeline::export_series(&files, &job, Some(tx), &CancelFlag::new()).await?;
            let elapsed = started.elapsed().as_secs_f64();
            progress.await?;
            println!(
                "run {run}: {elapsed:.3}s; {} chapters ({} cached), {} pages, {} files, {} bytes",
                result.chapters,
                result.cached_chapters,
                result.pages,
                result.files.len(),
                result.files.iter().map(|f| f.bytes).sum::<u64>()
            );
            for file in result.files {
                println!("  {}", file.location);
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
