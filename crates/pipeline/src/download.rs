//! Stage 1: download chapters into the library.
//!
//! Pages are saved exactly as downloaded (or shrunk, if the user chose to
//! save space) and stay on disk until the user deletes them. An interrupted
//! chapter resumes where it stopped, and a chapter that a source cannot
//! supply falls back to the next source that has it.
//!
//! Several chapters, and several pages of each, download at once, up to the
//! limits the user chose. A source that pushes back makes the whole download
//! ease off by itself (see [`crate::limiter`]).

use crate::balance::Balancer;
use crate::error::{PipelineError, Result};
use crate::fetch::{fetch_page_limited, with_retry, RetryPolicy};
use crate::limiter::Limiter;
use crate::types::{CancelFlag, ChapterRef, ChapterRequest, JobEvent, SkippedChapter, SourceUse};
use catalog::{Catalog, CatalogError};
use converter::decode::{image_dimensions, shrink_for_storage, STORAGE_MAX_HEIGHT};
use futures_util::stream::{self, StreamExt};
use library::{chapter_key, DownloadLimits, DownloadedChapter, LibraryFiles};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;

/// JPEG quality used when shrinking pages to save space.
const SHRINK_QUALITY: u8 = 88;

#[derive(Debug, Default)]
pub struct DownloadOutcome {
    /// Chapters now completely on disk, in the order requested.
    pub chapters: Vec<DownloadedChapter>,
    /// Chapters no source could supply.
    pub skipped: Vec<SkippedChapter>,
    pub source_use: Vec<SourceUse>,
}

type Events = Option<UnboundedSender<JobEvent>>;

fn send(events: &Events, event: JobEvent) {
    if let Some(tx) = events {
        let _ = tx.send(event);
    }
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> PipelineError + '_ {
    move |e| PipelineError::Io(path.to_path_buf(), e)
}

/// What became of one requested chapter.
struct ChapterResult {
    saved: Option<DownloadedChapter>,
    skipped: Option<SkippedChapter>,
    source_use: Vec<SourceUse>,
}

/// Everything one chapter's download needs besides the chapter itself.
struct Context<'a, C> {
    catalog: &'a C,
    files: &'a LibraryFiles,
    series_id: &'a str,
    shrink: bool,
    policy: &'a RetryPolicy,
    limiter: &'a Limiter,
    balancer: &'a Balancer,
    /// Page requests allowed at once, as chosen.
    pages_at_once: usize,
    events: &'a Events,
    cancel: &'a CancelFlag,
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
    limits: DownloadLimits,
    policy: &RetryPolicy,
    events: Option<UnboundedSender<JobEvent>>,
    cancel: &CancelFlag,
) -> Result<DownloadOutcome> {
    if requests.is_empty() {
        return Err(PipelineError::EmptyJob);
    }
    let pages_at_once = limits.pages.max(1);
    let limiter = Limiter::new(pages_at_once);
    let balancer = Balancer::new(limits.spread_sources);
    let cx = Context {
        catalog,
        files,
        series_id,
        shrink,
        policy,
        limiter: &limiter,
        balancer: &balancer,
        pages_at_once,
        events: &events,
        cancel,
    };
    let total = requests.len();
    let cx = &cx;

    let mut results: Vec<(usize, Result<ChapterResult>)> =
        stream::iter(requests.iter().enumerate())
            .map(|(index, request)| async move {
                (index, download_request(cx, index, total, request).await)
            })
            .buffer_unordered(limits.chapters.max(1))
            .collect()
            .await;
    results.sort_by_key(|(index, _)| *index);

    let mut outcome = DownloadOutcome::default();
    for (_, result) in results {
        let result = result?; // only a cancellation comes back as an error
        outcome.source_use.extend(result.source_use);
        outcome.chapters.extend(result.saved);
        outcome.skipped.extend(result.skipped);
    }
    if outcome.chapters.is_empty() {
        return Err(PipelineError::NothingDownloaded);
    }
    Ok(outcome)
}

/// One requested chapter: try each source in turn until one supplies it.
/// Fails only when the job is cancelled; a chapter nobody can supply is
/// reported as skipped.
async fn download_request<C: Catalog>(
    cx: &Context<'_, C>,
    index: usize,
    total: usize,
    request: &ChapterRequest,
) -> Result<ChapterResult> {
    if cx.cancel.is_cancelled() {
        return Err(PipelineError::Cancelled);
    }
    send(
        cx.events,
        JobEvent::ChapterStarted {
            index,
            total,
            name: request.name.clone(),
        },
    );
    // Named after the first (best) source, whichever one ends up supplying it.
    let key = chapter_key(
        request.number,
        request.options.first().map_or(0, |o| o.chapter_id),
    );

    let mut source_use = Vec::new();
    let mut last_reason = String::from("no source offers this chapter");
    let mut saved = None;
    // The sources in the order to try them: the best that is not already busy.
    let order = cx.balancer.order(&request.options);
    for (n, option) in order.iter().copied().enumerate() {
        let started = Instant::now();
        cx.balancer.begin(&option.source_id);
        let attempt = download_one(cx, &key, option, index).await;
        let pages = attempt.as_ref().map_or(0, |(pages, _)| *pages);
        cx.balancer
            .finish(&option.source_id, pages, started.elapsed().as_secs_f64());
        source_use.push(SourceUse {
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
                if let Some(next) = order.get(n + 1) {
                    send(
                        cx.events,
                        JobEvent::UsedFallback {
                            chapter_index: index,
                            from: option.source_id.clone(),
                            to: next.source_id.clone(),
                            reason: last_reason.clone(),
                        },
                    );
                }
            }
        }
    }

    let result = match saved {
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
            send(
                cx.events,
                JobEvent::ChapterSaved {
                    chapter: chapter.clone(),
                },
            );
            ChapterResult {
                saved: Some(chapter),
                skipped: None,
                source_use,
            }
        }
        None => {
            send(
                cx.events,
                JobEvent::ChapterSkipped {
                    chapter_index: index,
                    name: request.name.clone(),
                    reason: last_reason.clone(),
                },
            );
            ChapterResult {
                saved: None,
                skipped: Some(SkippedChapter {
                    index,
                    name: request.name.clone(),
                    reason: last_reason,
                }),
                source_use,
            }
        }
    };
    send(cx.events, JobEvent::ChapterDone { index });
    Ok(result)
}

/// Download one source's copy of a chapter. Returns the page count and the
/// bytes on disk.
async fn download_one<C: Catalog>(
    cx: &Context<'_, C>,
    key: &str,
    option: &ChapterRef,
    chapter_index: usize,
) -> Result<(usize, u64)> {
    let (files, series_id) = (cx.files, cx.series_id);
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

    let paths = with_retry(cx.policy, || cx.catalog.chapter_pages(option.chapter_id)).await?;
    if paths.is_empty() {
        return Err(
            CatalogError::BadResponse("the source lists no pages for this chapter".into()).into(),
        );
    }
    let total = paths.len();
    let done = AtomicUsize::new(0);
    let done = &done;
    let on_limit = |now: usize| {
        send(
            cx.events,
            JobEvent::PagesAtOnce {
                now,
                chosen: cx.pages_at_once,
            },
        )
    };
    let on_limit = &on_limit;

    let results: Vec<Result<()>> = stream::iter(paths.iter().enumerate())
        .map(|(index, path)| async move {
            if cx.cancel.is_cancelled() {
                return Err(PipelineError::Cancelled);
            }
            let dest = files.page_path(series_id, key, index);
            let have = fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false);
            if !have {
                let bytes =
                    fetch_page_limited(cx.catalog, path, cx.policy, cx.limiter, on_limit).await?;
                // An error page or a blocked response is not a manga page.
                image_dimensions(&bytes).map_err(|e| {
                    PipelineError::Decode(format!("page {} is not an image ({e})", index + 1))
                })?;
                let bytes = if cx.shrink {
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
            send(
                cx.events,
                JobEvent::PageDone {
                    chapter_index,
                    done: n,
                    total,
                },
            );
            Ok(())
        })
        .buffer_unordered(cx.pages_at_once)
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
