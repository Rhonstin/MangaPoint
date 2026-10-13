//! Stage 3: build reader files from the downloaded chapters.
//!
//! Works entirely from the library on disk, so it needs no network and can
//! be repeated with different settings or groupings at will. Each book is
//! assembled in a temporary file rather than in memory, so one file holding
//! a whole series is fine.

use crate::conversion_cache;
use crate::error::{PipelineError, Result};
use crate::export::{output_file_name, sanitize_component, ExportTarget, ExportedFile};
use crate::options::process_options_for;
use crate::preview::decode_page;
use crate::types::{CancelFlag, JobEvent};
use converter::xtc::{Chapter as XtcChapter, Metadata, WriteError, XtcBuilder};
use converter::{GrayImage, ProcessOptions};
use device_profiles::DeviceProfile;
use futures_util::stream::{self, StreamExt};
use library::{DownloadedChapter, DownloadedSeries, LibraryFiles};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

/// Pages decoded at the same time within one chapter: one for each core, but
/// not so many that tall pages fill the memory.
fn page_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 12)
}
/// Chapters converted ahead of the one being written.
fn chapter_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(2, |n| n.get() / 4)
        .clamp(2, 6)
}
/// Just under the format's limit of 65,535 pages per file.
const DEFAULT_MAX_PAGES_PER_FILE: usize = 60_000;

/// How chapters are divided into files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouping {
    /// Everything in one file (the default).
    Single,
    /// One file per chapter. Can mean a lot of files.
    PerChapter,
    /// One file for every N chapters.
    EveryN(usize),
}

#[derive(Clone, Debug)]
pub struct ExportJob {
    pub series: DownloadedSeries,
    /// Which downloaded chapters to include (any order; the series' own
    /// reading order is used).
    pub chapter_keys: Vec<String>,
    pub device: DeviceProfile,
    /// Language tag stored in the file, e.g. "en".
    pub language: String,
    pub grouping: Grouping,
    pub target: ExportTarget,
    /// Scratch space for books being assembled.
    pub work_dir: PathBuf,
    /// Start a new file rather than exceed this many pages.
    pub max_pages_per_file: usize,
    /// Rebuild chapters even when cached pages exist, for diagnostics and
    /// first-export benchmarks. Newly converted pages still refresh caches.
    pub force_conversion: bool,
}

impl ExportJob {
    pub fn new(
        series: DownloadedSeries,
        chapter_keys: Vec<String>,
        device: DeviceProfile,
        target: ExportTarget,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            series,
            chapter_keys,
            device,
            language: "en".into(),
            grouping: Grouping::Single,
            target,
            work_dir,
            max_pages_per_file: DEFAULT_MAX_PAGES_PER_FILE,
            force_conversion: false,
        }
    }
}

#[derive(Debug)]
pub struct ExportOutcome {
    pub files: Vec<ExportedFile>,
    pub pages: usize,
    pub chapters: usize,
    /// Chapters reused without decoding, rendering or packing their pages.
    pub cached_chapters: usize,
}

/// A finished book waiting in the scratch folder to be sent.
struct Volume {
    temp: PathBuf,
    /// Which group of chapters (see [`Grouping`]) it belongs to.
    group: usize,
    chapter_names: Vec<String>,
    first_number: Option<f64>,
    last_number: Option<f64>,
}

/// Deletes the scratch files however the export ends.
#[derive(Default)]
struct Scratch(Vec<PathBuf>);

impl Drop for Scratch {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = fs::remove_file(p);
        }
    }
}

/// Split `count` chapters (by position) into groups.
fn group_ranges(count: usize, grouping: Grouping) -> Vec<std::ops::Range<usize>> {
    let size = match grouping {
        Grouping::Single => count.max(1),
        Grouping::PerChapter => 1,
        Grouping::EveryN(n) => n.max(1),
    };
    (0..count)
        .step_by(size)
        .map(|start| start..(start + size).min(count))
        .collect()
}

fn unique_name(kind: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "mangapoint-{}-{}.{kind}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// Build the files and send them to the target.
pub async fn export_series(
    files: &LibraryFiles,
    job: &ExportJob,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<ExportOutcome> {
    let chapters: Vec<&DownloadedChapter> = job
        .series
        .chapters
        .iter()
        .filter(|c| job.chapter_keys.contains(&c.key))
        .collect();
    if chapters.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let emit = |e: JobEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(e);
        }
    };
    fs::create_dir_all(&job.work_dir).map_err(|e| PipelineError::Io(job.work_dir.clone(), e))?;

    let settings = &job.series.settings;
    let opts = process_options_for(&job.device, settings);
    let format = settings.format;
    let mut scratch = Scratch::default();
    let mut volumes: Vec<Volume> = Vec::new();
    let mut total_pages = 0usize;
    let mut cached_chapters = 0usize;
    let total = chapters.len();
    // More chapters can prepare/pack independently, while the total number
    // of simultaneous decodes remains capped at the old two-chapter budget.
    let decode_slots = Semaphore::new(page_concurrency() * 2);

    // Chapters are converted a little ahead of the one being written, so the
    // cores stay busy while a finished chapter is added to its book. They come
    // out of the stream in order.
    let order: Vec<(usize, usize)> = group_ranges(total, job.grouping)
        .into_iter()
        .enumerate()
        .flat_map(|(group, range)| range.map(move |ci| (group, ci)))
        .collect();
    let mut converted = stream::iter(order)
        .map(|(group, ci)| {
            let chapter = chapters[ci];
            let (series_id, opts, events, settings, decode_slots) =
                (&job.series.id, &opts, &events, settings, &decode_slots);
            async move {
                if cancel.is_cancelled() {
                    return (group, ci, Err(PipelineError::Cancelled));
                }
                if let Some(tx) = events {
                    let _ = tx.send(JobEvent::ChapterStarted {
                        index: ci,
                        total,
                        name: chapter.name.clone(),
                    });
                }
                let images = convert_chapter(
                    files,
                    series_id,
                    chapter,
                    opts,
                    settings,
                    &job.series.crops,
                    job.force_conversion,
                    decode_slots,
                    ci,
                    events,
                    cancel,
                )
                .await;
                if images.is_ok() {
                    if let Some(tx) = events {
                        let _ = tx.send(JobEvent::ChapterDone { index: ci });
                    }
                }
                (group, ci, images)
            }
        })
        .buffered(chapter_concurrency());

    let mut open: Option<(XtcBuilder, Volume)> = None;
    let mut open_group = usize::MAX;
    while let Some((group, ci, images)) = converted.next().await {
        let (images, cached) = images?;
        cached_chapters += usize::from(cached);
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        let chapter = chapters[ci];

        // A new group starts a new file.
        if group != open_group {
            if let Some((builder, volume)) = open.take() {
                volumes.push(finish_volume(builder, volume, &job.work_dir, &mut scratch)?);
            }
            open_group = group;
        }
        // Start a new file if this chapter would not fit in the open one.
        if let Some((builder, _)) = &open {
            if builder.page_count() + images.len() > job.max_pages_per_file {
                let (builder, volume) = open.take().expect("checked above");
                volumes.push(finish_volume(builder, volume, &job.work_dir, &mut scratch)?);
            }
        }
        if open.is_none() {
            let spool = job.work_dir.join(unique_name("spool"));
            scratch.0.push(spool.clone());
            let builder = XtcBuilder::new(format)
                .direction(settings.direction)
                .metadata(Metadata {
                    title: job.series.title.clone(),
                    language: job.language.clone(),
                    create_time: now_secs() as u32,
                    cover_page: Some(0),
                    ..Default::default()
                })
                .spool_to(&spool)?;
            open = Some((
                builder,
                Volume {
                    temp: PathBuf::new(),
                    group,
                    chapter_names: Vec::new(),
                    first_number: None,
                    last_number: None,
                },
            ));
        }
        let (builder, volume) = open.as_mut().expect("opened above");
        let start = builder.page_count();
        for image in &images {
            builder.add_encoded_page(image)?;
        }
        builder.push_chapter(XtcChapter {
            name: chapter.name.clone(),
            start_page: start as u16,
            end_page: (builder.page_count() - 1) as u16,
        });
        volume.chapter_names.push(chapter.name.clone());
        if volume.first_number.is_none() {
            volume.first_number = chapter.number;
        }
        if chapter.number.is_some() {
            volume.last_number = chapter.number;
        }
        total_pages += images.len();
    }
    if let Some((builder, volume)) = open.take() {
        volumes.push(finish_volume(builder, volume, &job.work_dir, &mut scratch)?);
    }

    // Now that every file exists, name them (part numbers need the count) and
    // send them.
    let mut sent = Vec::new();
    for (i, volume) in volumes.iter().enumerate() {
        let name = volume_name(job, volume, &volumes, i, format.extension());
        emit(JobEvent::Writing { file: name.clone() });
        sent.push(job.target.write_file(&name, &volume.temp).await?);
    }
    Ok(ExportOutcome {
        files: sent,
        pages: total_pages,
        chapters: total,
        cached_chapters,
    })
}

/// Write an assembled book to the scratch folder.
fn finish_volume(
    builder: XtcBuilder,
    mut volume: Volume,
    work_dir: &Path,
    scratch: &mut Scratch,
) -> Result<Volume> {
    let temp = work_dir.join(unique_name("book"));
    scratch.0.push(temp.clone());
    let file = File::create(&temp).map_err(|e| PipelineError::Io(temp.clone(), e))?;
    let mut writer = BufWriter::new(file);
    builder.write_to(&mut writer).map_err(|e| match e {
        WriteError::Xtc(e) => PipelineError::Xtc(e),
        WriteError::Io(e) => PipelineError::Io(temp.clone(), e),
    })?;
    writer
        .flush()
        .map_err(|e| PipelineError::Io(temp.clone(), e))?;
    volume.temp = temp;
    Ok(volume)
}

/// `Title - Ch 1-12.xtch`; with several files from one group,
/// `Title - Part 2 - Ch 13-25.xtch`; a lone unnumbered chapter is named
/// after itself.
fn volume_name(
    job: &ExportJob,
    volume: &Volume,
    all: &[Volume],
    index: usize,
    ext: &str,
) -> String {
    let in_group: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, v)| v.group == volume.group)
        .map(|(i, _)| i)
        .collect();
    let part =
        (in_group.len() > 1).then(|| in_group.iter().position(|&i| i == index).unwrap_or(0) + 1);
    if volume.chapter_names.len() == 1 && volume.first_number.is_none() {
        let mut name = format!(
            "{} - {}",
            sanitize_component(&job.series.title),
            sanitize_component(&volume.chapter_names[0])
        );
        if let Some(p) = part {
            name.push_str(&format!(" - Part {p}"));
        }
        return format!("{name}.{ext}");
    }
    let format = job.series.settings.format;
    output_file_name(
        &job.series.title,
        volume.first_number,
        volume.last_number,
        part,
        format,
    )
}

/// Decode every saved page of a chapter, then plan and convert it as a whole:
/// a webtoon's images are joined before they are cut into pages, so the pages
/// match what the preview showed and the user's corrections apply.
#[allow(clippy::too_many_arguments)]
async fn convert_chapter(
    files: &LibraryFiles,
    series_id: &str,
    chapter: &DownloadedChapter,
    opts: &ProcessOptions,
    settings: &library::ConversionSettings,
    crops: &library::CropOverrides,
    force_conversion: bool,
    decode_slots: &Semaphore,
    chapter_index: usize,
    events: &Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<(Vec<Vec<u8>>, bool)> {
    let pages = files.page_files(series_id, &chapter.key);
    if pages.is_empty() {
        return Err(PipelineError::Decode(format!(
            "{} has no saved pages; download it again",
            chapter.name
        )));
    }
    let total = pages.len();
    let cache_path = files
        .chapter_dir(series_id, &chapter.key)
        .join(conversion_cache::FILE_NAME);
    let (source_pages, saved_settings, cache, flag) = (
        pages.clone(),
        settings.clone(),
        cache_path.clone(),
        cancel.clone(),
    );
    let (width, height, format) = (opts.width, opts.height, settings.format);
    let crop_configuration = crops.chapter_configuration(&chapter.key);
    let lookup_crops = crop_configuration.clone();
    // A first export has no cache to look up. Hash its decoded snapshot
    // below instead of reading every source twice just to establish a miss.
    let cached = if !force_conversion && cache_path.is_file() {
        tokio::task::spawn_blocking(move || {
            let fingerprint = conversion_cache::fingerprint(
                &source_pages,
                &saved_settings,
                width,
                height,
                &flag,
            )?;
            let fingerprint = conversion_cache::with_crops(fingerprint, &lookup_crops);
            Ok::<_, PipelineError>(conversion_cache::load(
                &cache,
                &fingerprint,
                width,
                height,
                format,
            ))
        })
        .await
        .map_err(|e| PipelineError::Decode(e.to_string()))??
    } else {
        None
    };
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    if let Some(cached) = cached {
        return Ok((cached, true));
    }
    let done = std::sync::atomic::AtomicUsize::new(0);
    let done = &done;
    let name = chapter.name.as_str();

    let decoded: Vec<Result<(usize, GrayImage, Vec<u8>)>> = stream::iter(pages.into_iter().enumerate())
        .map(|(index, path)| async move {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let permit = decode_slots.acquire().await.expect("the export decode budget remains open");
            if cancel.is_cancelled() { return Err(PipelineError::Cancelled); }
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| PipelineError::Io(path.clone(), e))?;
            let gray = tokio::task::spawn_blocking(move || decode_page(&bytes).map(|gray| (gray, bytes)))
                .await
                .map_err(|e| PipelineError::Decode(e.to_string()))?
                .map_err(|e| {
                    PipelineError::Decode(format!(
                        "page {} of {name} could not be read ({e}); delete the chapter and download it again",
                        index + 1
                    ))
                })?;
            drop(permit);
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(tx) = events {
                let _ = tx.send(JobEvent::PageDone {
                    chapter_index,
                    done: n,
                    total,
                });
            }
            Ok((index, gray.0, gray.1))
        })
        // A tall early image must not prevent later completed reads from
        // releasing their slots and starting more work. Restore order below.
        .buffer_unordered(page_concurrency())
        .collect()
        .await;

    let mut decoded = decoded.into_iter().collect::<Result<Vec<_>>>()?;
    decoded.sort_unstable_by_key(|(index, _, _)| *index);
    if cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    let opts = opts.clone();
    let cancel = cancel.clone();
    let settings = settings.clone();
    let crops = crops.clone();
    let chapter_key = chapter.key.clone();
    let packed = tokio::task::spawn_blocking(move || {
        let fingerprint = conversion_cache::fingerprint_bytes(
            decoded.iter().map(|(_, _, bytes)| bytes.as_slice()),
            &settings,
            opts.width,
            opts.height,
        );
        let fingerprint = conversion_cache::with_crops(fingerprint, &crop_configuration);
        let grays: Vec<_> = decoded.into_iter().map(|(_, gray, _)| gray).collect();
        let images = if crop_configuration.is_empty() {
            converter::plan::convert_chapter(&grays, &opts)
        } else {
            let prepared = converter::plan::prepare(&grays, &opts);
            let plan = converter::plan::plan(&prepared, &opts);
            let cuts: Vec<_> = plan
                .pages
                .iter()
                .map(|page| {
                    crops
                        .effective(&chapter_key, &crate::preview::crop_page_key(page))
                        .unwrap_or_default()
                        .as_array()
                })
                .collect();
            converter::plan::render_all_cropped(&prepared, &plan.pages, &opts, &cuts)
        };
        let mut packed = Vec::with_capacity(images.len());
        for image in images {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            packed.push(converter::xtc::encode_page(&image)?);
        }
        conversion_cache::save(&cache_path, &fingerprint, &packed);
        Ok::<_, PipelineError>(packed)
    })
    .await
    .map_err(|e| PipelineError::Decode(e.to_string()))??;
    Ok((packed, false))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_split_chapters_the_way_each_option_says() {
        assert_eq!(group_ranges(5, Grouping::Single), vec![0..5]);
        assert_eq!(
            group_ranges(3, Grouping::PerChapter),
            vec![0..1, 1..2, 2..3]
        );
        assert_eq!(group_ranges(5, Grouping::EveryN(2)), vec![0..2, 2..4, 4..5]);
        assert_eq!(
            group_ranges(5, Grouping::EveryN(0)),
            vec![0..1, 1..2, 2..3, 3..4, 4..5],
            "0 counts as 1"
        );
        assert_eq!(group_ranges(2, Grouping::EveryN(10)), vec![0..2]);
        assert!(group_ranges(0, Grouping::Single).is_empty());
    }

    #[test]
    fn scratch_files_are_removed_when_dropped() {
        let dir = std::env::temp_dir().join(format!("mangapoint-scratch-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.tmp");
        fs::write(&file, b"x").unwrap();
        drop(Scratch(vec![file.clone(), dir.join("never-existed")]));
        assert!(!file.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn temporary_names_do_not_repeat() {
        assert_ne!(unique_name("book"), unique_name("book"));
    }
}
