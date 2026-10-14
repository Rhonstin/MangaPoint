//! Make an EPUB from a real downloaded series and say how big it came out.
//!   cargo run --release -p pipeline --example epub_real -- <data folder> <series id part> <chapters> <out dir> [device id]
use library::{JsonFile, LibraryDb, LibraryFiles, OutputFormat};
use pipeline::{export_series, CancelFlag, ExportJob, ExportTarget, Grouping};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&args[0]);
    let db: LibraryDb = JsonFile::new(root.join("library.json")).load();
    let mut series = db
        .series
        .iter()
        .find(|s| s.id.contains(&args[1]))
        .expect("no such series")
        .clone();
    let count: usize = args[2].parse().unwrap();
    let out = PathBuf::from(&args[3]);
    let device = device_profiles::known(args.get(4).map_or("xteink-x4-pro", String::as_str)).unwrap();
    series.settings.set_output(match std::env::var("OUT").as_deref() {
        Ok("xtc") => OutputFormat::Xtc,
        Ok("xtch") => OutputFormat::Xtch,
        _ => OutputFormat::Epub,
    });
    match std::env::var("DITHER").as_deref() {
        Ok("atkinson") => series.settings.dither = converter::Dither::Atkinson,
        Ok("ordered") => series.settings.dither = converter::Dither::Ordered,
        Ok("threshold") => series.settings.dither = converter::Dither::Threshold,
        _ => {}
    }
    let keys: Vec<String> = series.chapters.iter().take(count).map(|c| c.key.clone()).collect();
    let mut job = ExportJob::new(
        series.clone(),
        keys,
        device.clone(),
        ExportTarget::Folder { path: out.clone() },
        out.join("work"),
    );
    job.grouping = Grouping::Single;
    let files = LibraryFiles::new(root.join("library"));
    let started = std::time::Instant::now();
    let done = export_series(&files, &job, None, &CancelFlag::new()).await.unwrap();
    println!("{} on a {} ({}x{}, {} grays): {} pages in {:.1}s", series.title, device.name, device.width, device.height, device.gray_levels, done.pages, started.elapsed().as_secs_f32());
    let total: u64 = done.files.iter().map(|f| f.bytes).sum();
    println!("  {} files, {:.1} MB in all, {:.1} KB a page", done.files.len(), total as f64 / 1e6, total as f64 / 1e3 / done.pages as f64);
}
