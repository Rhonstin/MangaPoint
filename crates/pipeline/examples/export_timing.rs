//! Where does an export spend its time? Reads a library read-only and times,
//! for the first chapters of a series, each stage of converting them.
//!
//!   cargo run --release -p pipeline --example export_timing -- <library-dir> <library.json> "<title part>" [chapters]
//!
//! Nothing is written anywhere.

use converter::plan::{plan, prepare, render_page};
use library::{DownloadedSeries, JsonFile, LibraryDb, LibraryFiles};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err(
            "usage: export_timing <library-dir> <library.json> <title part> [chapters]".into(),
        );
    }
    let files = LibraryFiles::new(&args[0]);
    let db: LibraryDb = JsonFile::new(&args[1]).load();
    let wanted = args[2].to_lowercase();
    let take: usize = args.get(3).and_then(|n| n.parse().ok()).unwrap_or(12);
    let series: &DownloadedSeries = db
        .series
        .iter()
        .find(|s| s.title.to_lowercase().contains(&wanted))
        .ok_or("no such series")?;
    let device = device_profiles::x4_pro();
    let opts = pipeline::process_options_for(&device, &series.settings);
    println!(
        "{}: {} chapters; timing the first {take}. {} threads here.",
        series.title,
        series.chapters.len(),
        std::thread::available_parallelism().map_or(1, |n| n.get())
    );

    let (mut t_read, mut t_decode, mut t_prepare, mut t_plan, mut t_render) =
        (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut pages_in, mut pages_out, mut bytes_in) = (0usize, 0usize, 0u64);
    let mut builder = converter::xtc::XtcBuilder::new(series.settings.format);
    let mut t_add = 0.0;
    let wall = Instant::now();
    for chapter in series.chapters.iter().take(take) {
        let paths = files.page_files(&series.id, &chapter.key);
        let t = Instant::now();
        let raw: Vec<Vec<u8>> = paths
            .iter()
            .map(|p| std::fs::read(p).unwrap_or_default())
            .collect();
        t_read += t.elapsed().as_secs_f64();
        bytes_in += raw.iter().map(|b| b.len() as u64).sum::<u64>();
        let t = Instant::now();
        let grays: Vec<_> = raw
            .iter()
            .filter_map(|b| pipeline::preview::decode_page(b).ok())
            .collect();
        t_decode += t.elapsed().as_secs_f64();
        pages_in += grays.len();
        let t = Instant::now();
        let prepared = prepare(&grays, &opts);
        t_prepare += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let planned = plan(&prepared, &opts);
        t_plan += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let rendered: Vec<_> = planned
            .pages
            .iter()
            .map(|p| render_page(&prepared, p, &opts))
            .collect();
        t_render += t.elapsed().as_secs_f64();
        pages_out += rendered.len();
        let t = Instant::now();
        for r in &rendered {
            builder.add_page(r).unwrap();
        }
        t_add += t.elapsed().as_secs_f64();
    }
    println!(
        "{pages_in} saved pages ({:.1} MB) became {pages_out} reader pages in {:.1}s (one thread)",
        bytes_in as f64 / 1e6,
        wall.elapsed().as_secs_f64()
    );
    println!("  read files  {t_read:6.2}s");
    println!("  decode      {t_decode:6.2}s");
    println!("  prepare     {t_prepare:6.2}s   (joins and trims the images)");
    println!("  plan        {t_plan:6.2}s");
    println!("  render      {t_render:6.2}s   (resize, contrast, dither, per reader page)");
    println!("  add to book {t_add:6.2}s   (packing each page into the file)");

    // The real export, end to end, into a scratch folder of its own.
    let scratch =
        std::env::temp_dir().join(format!("mangapoint-export-timing-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let keys: Vec<String> = series
        .chapters
        .iter()
        .take(take)
        .map(|c| c.key.clone())
        .collect();
    let job = pipeline::ExportJob::new(
        series.clone(),
        keys,
        device,
        pipeline::ExportTarget::Folder {
            path: scratch.join("out"),
        },
        scratch.join("work"),
    );
    let started = Instant::now();
    let cancel = pipeline::CancelFlag::new();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let outcome = rt.block_on(pipeline::export_series(&files, &job, None, &cancel))?;
    println!(
        "the real export of the same chapters: {:.1}s ({} files, {} pages)",
        started.elapsed().as_secs_f64(),
        outcome.files.len(),
        outcome.pages
    );
    // Only ever delete the scratch folder made above, inside the temp folder.
    if scratch.starts_with(std::env::temp_dir())
        && scratch
            .to_string_lossy()
            .contains("mangapoint-export-timing-")
    {
        std::fs::remove_dir_all(&scratch)?;
    }
    Ok(())
}
