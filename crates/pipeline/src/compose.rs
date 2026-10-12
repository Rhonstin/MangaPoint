//! Stage 3: build reader files from the downloaded chapters.
//!
//! Works entirely from the library on disk, so it needs no network and can
//! be repeated with different settings or groupings at will. Each book is
//! assembled in a temporary file rather than in memory, so one file holding
//! a whole series is fine.

use crate::error::{PipelineError, Result};
use crate::export::{output_file_name, sanitize_component, ExportTarget, ExportedFile};
use crate::options::process_options_for;
use crate::preview::{decode_page, render_levels};
use crate::types::{CancelFlag, JobEvent};
use converter::xtc::{Chapter as XtcChapter, Metadata, WriteError, XtcBuilder};
use converter::{LevelImage, ProcessOptions};
use device_profiles::DeviceProfile;
use futures_util::stream::{self, StreamExt};
use library::{DownloadedChapter, DownloadedSeries, LibraryFiles};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

/// Pages converted at the same time within one chapter.
const PAGE_CONCURRENCY: usize = 4;
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
        }
    }
}

#[derive(Debug)]
pub struct ExportOutcome {
    pub files: Vec<ExportedFile>,
    pub pages: usize,
    pub chapters: usize,
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
    let total = chapters.len();

    for (group, range) in group_ranges(total, job.grouping).into_iter().enumerate() {
        let mut open: Option<(XtcBuilder, Volume)> = None;

        for ci in range {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let chapter = chapters[ci];
            emit(JobEvent::ChapterStarted {
                index: ci,
                total,
                name: chapter.name.clone(),
            });
            let images =
                convert_chapter(files, &job.series.id, chapter, &opts, ci, &events, cancel).await?;
            emit(JobEvent::ChapterDone { index: ci });

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
                builder.add_page(image)?;
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

/// Decode and convert every saved page of a chapter, in reading order.
async fn convert_chapter(
    files: &LibraryFiles,
    series_id: &str,
    chapter: &DownloadedChapter,
    opts: &ProcessOptions,
    chapter_index: usize,
    events: &Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<Vec<LevelImage>> {
    let pages = files.page_files(series_id, &chapter.key);
    if pages.is_empty() {
        return Err(PipelineError::Decode(format!(
            "{} has no saved pages; download it again",
            chapter.name
        )));
    }
    let total = pages.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let done = &done;
    let name = chapter.name.as_str();

    let results: Vec<Result<Vec<LevelImage>>> = stream::iter(pages.into_iter().enumerate())
        .map(|(index, path)| {
            let opts = opts.clone();
            async move {
                if cancel.is_cancelled() {
                    return Err(PipelineError::Cancelled);
                }
                let bytes = tokio::fs::read(&path)
                    .await
                    .map_err(|e| PipelineError::Io(path.clone(), e))?;
                let levels = tokio::task::spawn_blocking(move || {
                    decode_page(&bytes).map(|gray| render_levels(&gray, &opts))
                })
                .await
                .map_err(|e| PipelineError::Decode(e.to_string()))?
                .map_err(|e| {
                    PipelineError::Decode(format!(
                        "page {} of {name} could not be read ({e}); delete the chapter and download it again",
                        index + 1
                    ))
                })?;
                let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(tx) = events {
                    let _ = tx.send(JobEvent::PageDone {
                        chapter_index,
                        done: n,
                        total,
                    });
                }
                Ok(levels)
            }
        })
        .buffered(PAGE_CONCURRENCY)
        .collect()
        .await;

    let mut images = Vec::new();
    for r in results {
        images.extend(r?);
    }
    Ok(images)
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
