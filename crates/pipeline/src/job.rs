//! Running one export job: fetch, convert, assemble, write.

use crate::error::{PipelineError, Result};
use crate::export::{output_file_name, ExportTarget, ExportedFile};
use crate::fetch::{fetch_page, with_retry, PageCache, RetryPolicy};
use crate::options::process_options;
use catalog::{Catalog, CatalogError};
use converter::decode::decode_to_gray;
use converter::xtc::{Chapter as XtcChapter, Metadata, XtcBuilder};
use converter::{process_page, LevelImage, ProcessOptions};
use futures_util::stream::{self, StreamExt};
use library::Settings;
use search::merge::MergedChapter;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

/// Pages fetched and processed at the same time within one chapter.
const PAGE_CONCURRENCY: usize = 4;

/// One way to get a chapter: a specific source's copy of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChapterRef {
    pub source_id: String,
    pub chapter_id: i64,
}

/// A chapter to include, with every source that can supply it (best first).
/// If the first source fails, the next is tried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChapterRequest {
    pub number: Option<f64>,
    pub name: String,
    pub options: Vec<ChapterRef>,
}

/// What to do when no source can supply a chapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnMissing {
    /// Leave it out, carry on, and report it.
    Skip,
    /// Stop the job.
    Abort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    pub series_title: String,
    pub author: Option<String>,
    pub chapters: Vec<ChapterRequest>,
    pub settings: Settings,
    pub target: ExportTarget,
    pub on_missing: OnMissing,
    /// Start a new file after this many pages (memory and file-size limit).
    pub max_pages_per_file: usize,
}

impl JobSpec {
    pub fn new(
        series_title: impl Into<String>,
        chapters: Vec<ChapterRequest>,
        settings: Settings,
        target: ExportTarget,
    ) -> Self {
        Self {
            series_title: series_title.into(),
            author: None,
            chapters,
            settings,
            target,
            on_missing: OnMissing::Skip,
            max_pages_per_file: 1000,
        }
    }

    /// Stable id: the same series and chapters always give the same id, so a
    /// retried job finds the pages it already downloaded.
    pub fn id(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        };
        feed(self.series_title.as_bytes());
        for c in &self.chapters {
            for o in &c.options {
                feed(o.source_id.as_bytes());
                feed(&o.chapter_id.to_le_bytes());
            }
        }
        format!("{hash:016x}")
    }
}

/// Build requests from a merged chapter list. `member_sources[i]` is the
/// source id of member `i` used when merging.
pub fn requests_from_merged(
    merged: &[MergedChapter],
    member_sources: &[String],
) -> Vec<ChapterRequest> {
    merged
        .iter()
        .map(|m| ChapterRequest {
            number: m.number,
            name: m.name.clone(),
            options: m
                .options
                .iter()
                .filter_map(|o| {
                    Some(ChapterRef {
                        source_id: member_sources.get(o.member)?.clone(),
                        chapter_id: o.chapter_id,
                    })
                })
                .collect(),
        })
        .collect()
}

/// Progress reports while a job runs.
#[derive(Clone, Debug, PartialEq)]
pub enum JobEvent {
    ChapterStarted {
        index: usize,
        total: usize,
        name: String,
    },
    /// One more page of the current chapter is downloaded and converted.
    PageDone {
        chapter_index: usize,
        done: usize,
        total: usize,
    },
    /// The first source failed, so another is being used.
    UsedFallback {
        chapter_index: usize,
        from: String,
        to: String,
        reason: String,
    },
    ChapterSkipped {
        chapter_index: usize,
        name: String,
        reason: String,
    },
    Writing {
        file: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedChapter {
    pub index: usize,
    pub name: String,
    pub reason: String,
}

/// How a source did, so the caller can update its health record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUse {
    pub source_id: String,
    pub ok: bool,
    pub ms: u64,
}

#[derive(Debug)]
pub struct JobOutcome {
    pub files: Vec<ExportedFile>,
    pub pages: usize,
    pub chapters_done: usize,
    pub skipped: Vec<SkippedChapter>,
    pub source_use: Vec<SourceUse>,
}

/// Lets the UI stop a running job.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct Volume {
    builder: XtcBuilder,
    pages: usize,
    first_number: Option<f64>,
    last_number: Option<f64>,
}

/// Run a job to completion.
///
/// On failure the downloaded pages stay in `cache`, so running the same job
/// again resumes where it stopped. On success the cache for this job is
/// cleared.
pub async fn run_job<C: Catalog>(
    catalog: &C,
    spec: &JobSpec,
    cache: &PageCache,
    policy: &RetryPolicy,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<JobOutcome> {
    if spec.chapters.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let job_id = spec.id();
    let opts = process_options(&spec.settings);
    let format = spec.settings.conversion.format;
    let emit = |e: JobEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(e);
        }
    };
    let new_volume = || Volume {
        builder: XtcBuilder::new(format)
            .direction(spec.settings.conversion.direction)
            .metadata(Metadata {
                title: spec.series_title.clone(),
                author: spec.author.clone().unwrap_or_default(),
                language: spec.settings.language.clone(),
                create_time: now_secs() as u32,
                cover_page: Some(0),
                ..Default::default()
            }),
        pages: 0,
        first_number: None,
        last_number: None,
    };

    let mut volume = new_volume();
    let mut parts_written = 0usize;
    let mut outcome = JobOutcome {
        files: Vec::new(),
        pages: 0,
        chapters_done: 0,
        skipped: Vec::new(),
        source_use: Vec::new(),
    };
    let total = spec.chapters.len();

    for (index, request) in spec.chapters.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        emit(JobEvent::ChapterStarted {
            index,
            total,
            name: request.name.clone(),
        });

        let mut last_reason = String::from("no source offers this chapter");
        let mut pages: Option<Vec<LevelImage>> = None;
        for (n, option) in request.options.iter().enumerate() {
            let started = Instant::now();
            let attempt = fetch_chapter(
                catalog, cache, &job_id, option, index, &opts, policy, &events, cancel,
            )
            .await;
            let ok = attempt.is_ok();
            outcome.source_use.push(SourceUse {
                source_id: option.source_id.clone(),
                ok,
                ms: started.elapsed().as_millis() as u64,
            });
            match attempt {
                Ok(images) => {
                    pages = Some(images);
                    break;
                }
                Err(PipelineError::Cancelled) => return Err(PipelineError::Cancelled),
                Err(e) => {
                    last_reason = e.to_string();
                    if let Some(next) = request.options.get(n + 1) {
                        emit(JobEvent::UsedFallback {
                            chapter_index: index,
                            from: option.source_id.clone(),
                            to: next.source_id.clone(),
                            reason: last_reason.clone(),
                        });
                    }
                }
            }
        }

        let Some(images) = pages else {
            match spec.on_missing {
                OnMissing::Abort => {
                    return Err(PipelineError::ChapterFailed {
                        name: request.name.clone(),
                        reason: last_reason,
                    })
                }
                OnMissing::Skip => {
                    emit(JobEvent::ChapterSkipped {
                        chapter_index: index,
                        name: request.name.clone(),
                        reason: last_reason.clone(),
                    });
                    outcome.skipped.push(SkippedChapter {
                        index,
                        name: request.name.clone(),
                        reason: last_reason,
                    });
                    continue;
                }
            }
        };

        // Start a new file rather than let one grow without bound, but never
        // split a chapter across files.
        if volume.pages > 0 && volume.pages + images.len() > spec.max_pages_per_file {
            parts_written += 1;
            let file = flush(volume, spec, parts_written, true, &events).await?;
            outcome.files.push(file);
            volume = new_volume();
        }

        let start = volume.pages;
        for image in &images {
            volume.builder.add_page(image)?;
        }
        volume.pages += images.len();
        volume.builder.push_chapter(XtcChapter {
            name: request.name.clone(),
            start_page: start as u16,
            end_page: (volume.pages - 1) as u16,
        });
        if volume.first_number.is_none() {
            volume.first_number = request.number;
        }
        if request.number.is_some() {
            volume.last_number = request.number;
        }
        outcome.pages += images.len();
        outcome.chapters_done += 1;
    }

    if outcome.chapters_done == 0 {
        return Err(PipelineError::NothingDownloaded);
    }
    parts_written += 1;
    let file = flush(volume, spec, parts_written, parts_written > 1, &events).await?;
    outcome.files.push(file);
    cache.clear_job(&job_id);
    Ok(outcome)
}

async fn flush(
    volume: Volume,
    spec: &JobSpec,
    part: usize,
    numbered: bool,
    events: &Option<UnboundedSender<JobEvent>>,
) -> Result<ExportedFile> {
    let name = output_file_name(
        &spec.series_title,
        volume.first_number,
        volume.last_number,
        numbered.then_some(part),
        spec.settings.conversion.format,
    );
    if let Some(tx) = events {
        let _ = tx.send(JobEvent::Writing { file: name.clone() });
    }
    let bytes = volume.builder.build()?;
    spec.target.write(&name, &bytes).await
}

/// Download and convert every page of one chapter from one source.
#[allow(clippy::too_many_arguments)]
async fn fetch_chapter<C: Catalog>(
    catalog: &C,
    cache: &PageCache,
    job_id: &str,
    option: &ChapterRef,
    chapter_index: usize,
    opts: &ProcessOptions,
    policy: &RetryPolicy,
    events: &Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<Vec<LevelImage>> {
    let paths = with_retry(policy, || catalog.chapter_pages(option.chapter_id)).await?;
    if paths.is_empty() {
        return Err(
            CatalogError::BadResponse("the source lists no pages for this chapter".into()).into(),
        );
    }
    let total = paths.len();
    let done = AtomicUsize::new(0);
    let done = &done;

    let results: Vec<Result<Vec<LevelImage>>> = stream::iter(paths.iter().enumerate())
        .map(|(index, path)| async move {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let bytes = match cache.get(job_id, option.chapter_id, index) {
                Some(bytes) => bytes,
                None => {
                    let bytes = fetch_page(catalog, path, policy).await?;
                    let _ = cache.put(job_id, option.chapter_id, index, &bytes);
                    bytes
                }
            };
            let options = opts.clone();
            let converted = tokio::task::spawn_blocking(move || {
                decode_to_gray(&bytes)
                    .map(|gray| process_page(&gray, &options))
                    .map_err(|e| PipelineError::Decode(e.to_string()))
            })
            .await
            .map_err(|e| PipelineError::Decode(e.to_string()))?;
            match converted {
                Ok(images) => {
                    let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                    if let Some(tx) = events {
                        let _ = tx.send(JobEvent::PageDone {
                            chapter_index,
                            done: n,
                            total,
                        });
                    }
                    Ok(images)
                }
                Err(e) => {
                    // Do not keep a corrupt download; a retry should fetch it afresh.
                    cache.remove(job_id, option.chapter_id, index);
                    Err(e)
                }
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
