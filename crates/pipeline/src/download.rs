//! Stage 1: download chapters into the library.
//!
//! Pages are saved exactly as downloaded (or shrunk, if the user chose to
//! save space) and stay on disk until the user deletes them. An interrupted
//! chapter resumes where it stopped, and a chapter that a source cannot
//! supply falls back to the next source that has it.

use crate::error::{PipelineError, Result};
use crate::fetch::{fetch_page, with_retry, RetryPolicy};
use crate::types::{CancelFlag, ChapterRef, ChapterRequest, JobEvent, SkippedChapter, SourceUse};
use catalog::{Catalog, CatalogError};
use converter::decode::{image_dimensions, shrink_for_storage, STORAGE_MAX_HEIGHT};
use futures_util::stream::{self, StreamExt};
use library::{chapter_key, DownloadedChapter, LibraryFiles};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

/// Pages downloaded at the same time within one chapter.
const PAGE_CONCURRENCY: usize = 4;
/// JPEG quality used when shrinking pages to save space.
const SHRINK_QUALITY: u8 = 88;

#[derive(Debug, Default)]
pub struct DownloadOutcome {
    /// Chapters now completely on disk.
    pub chapters: Vec<DownloadedChapter>,
    /// Chapters no source could supply.
    pub skipped: Vec<SkippedChapter>,
    pub source_use: Vec<SourceUse>,
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> PipelineError + '_ {
    move |e| PipelineError::Io(path.to_path_buf(), e)
}

/// Download `requests` into the library under `series_id`.
///
/// Pass only chapters that are not downloaded yet. A chapter is reported
/// through [`JobEvent::ChapterSaved`] the moment it is complete, so the
/// caller can record it even if a later chapter fails or the user cancels.
#[allow(clippy::too_many_arguments)]
pub async fn download_chapters<C: Catalog>(
    catalog: &C,
    files: &LibraryFiles,
    series_id: &str,
    requests: &[ChapterRequest],
    shrink: bool,
    policy: &RetryPolicy,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<DownloadOutcome> {
    if requests.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let emit = |e: JobEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(e);
        }
    };
    let total = requests.len();
    let mut outcome = DownloadOutcome::default();

    for (index, request) in requests.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(PipelineError::Cancelled);
        }
        emit(JobEvent::ChapterStarted {
            index,
            total,
            name: request.name.clone(),
        });
        // Named after the first (best) source, whichever one ends up supplying it.
        let key = chapter_key(
            request.number,
            request.options.first().map_or(0, |o| o.chapter_id),
        );

        let mut last_reason = String::from("no source offers this chapter");
        let mut saved = None;
        for (n, option) in request.options.iter().enumerate() {
            let started = Instant::now();
            let attempt = download_one(
                catalog, files, series_id, &key, option, index, shrink, policy, &events, cancel,
            )
            .await;
            outcome.source_use.push(SourceUse {
                source_id: option.source_id.clone(),
                ok: attempt.is_ok(),
                ms: started.elapsed().as_millis() as u64,
            });
            match attempt {
                Ok(done) => {
                    saved = Some((option, done));
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

        match saved {
            Some((option, (pages, bytes))) => {
                let chapter = DownloadedChapter {
                    key,
                    number: request.number,
                    name: request.name.clone(),
                    source_id: option.source_id.clone(),
                    chapter_id: option.chapter_id,
                    pages,
                    bytes,
                    downloaded_at: now_secs(),
                };
                emit(JobEvent::ChapterSaved {
                    chapter: chapter.clone(),
                });
                outcome.chapters.push(chapter);
            }
            None => {
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
            }
        }
    }

    if outcome.chapters.is_empty() {
        return Err(PipelineError::NothingDownloaded);
    }
    Ok(outcome)
}

/// Download one source's copy of a chapter. Returns the page count and the
/// bytes on disk.
#[allow(clippy::too_many_arguments)]
async fn download_one<C: Catalog>(
    catalog: &C,
    files: &LibraryFiles,
    series_id: &str,
    key: &str,
    option: &ChapterRef,
    chapter_index: usize,
    shrink: bool,
    policy: &RetryPolicy,
    events: &Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<(usize, u64)> {
    let dir = files.chapter_dir(series_id, key);
    let marker = dir.join("source.txt");
    let wanted = format!("{}:{}", option.source_id, option.chapter_id);
    if fs::read_to_string(&marker).ok().as_deref() != Some(wanted.as_str()) {
        // Pages from a different copy of the chapter would not line up with
        // this one, so start clean.
        files.remove_chapter(series_id, key).map_err(io_err(&dir))?;
    }
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    fs::write(&marker, &wanted).map_err(io_err(&marker))?;

    let paths = with_retry(policy, || catalog.chapter_pages(option.chapter_id)).await?;
    if paths.is_empty() {
        return Err(
            CatalogError::BadResponse("the source lists no pages for this chapter".into()).into(),
        );
    }
    let total = paths.len();
    let done = AtomicUsize::new(0);
    let done = &done;

    let results: Vec<Result<()>> = stream::iter(paths.iter().enumerate())
        .map(|(index, path)| async move {
            if cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let dest = files.page_path(series_id, key, index);
            let have = fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false);
            if !have {
                let bytes = fetch_page(catalog, path, policy).await?;
                // An error page or a blocked response is not a manga page.
                image_dimensions(&bytes).map_err(|e| {
                    PipelineError::Decode(format!("page {} is not an image ({e})", index + 1))
                })?;
                let bytes = if shrink {
                    tokio::task::spawn_blocking(move || {
                        match shrink_for_storage(&bytes, STORAGE_MAX_HEIGHT, SHRINK_QUALITY) {
                            Ok(Some(small)) => Ok(small),
                            Ok(None) => Ok(bytes),
                            Err(e) => Err(PipelineError::Decode(e.to_string())),
                        }
                    })
                    .await
                    .map_err(|e| PipelineError::Decode(e.to_string()))??
                } else {
                    bytes
                };
                write_atomically(&dest, &bytes)?;
            }
            let n = done.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(tx) = events {
                let _ = tx.send(JobEvent::PageDone {
                    chapter_index,
                    done: n,
                    total,
                });
            }
            Ok(())
        })
        .buffered(PAGE_CONCURRENCY)
        .collect()
        .await;
    for r in results {
        r?;
    }

    let saved = files.page_files(series_id, key).len();
    if saved != total {
        return Err(PipelineError::Decode(format!(
            "only {saved} of {total} pages were saved"
        )));
    }
    Ok((total, files.chapter_bytes(series_id, key)))
}

/// Write to a temporary name, then rename, so a crash never leaves a
/// half-written page that looks complete.
fn write_atomically(dest: &Path, bytes: &[u8]) -> Result<()> {
    let tmp: PathBuf = dest.with_extension("part");
    fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    fs::rename(&tmp, dest).map_err(io_err(dest))?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
