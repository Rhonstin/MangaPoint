//! Background work: opening a series, downloading, previewing, exporting,
//! and deleting. Everything here runs with `spawn_forever`, so leaving the
//! screen that started it does not stop it.

use crate::logic::{self, JobProgress};
use crate::state::{
    library_files, now_secs, paths, spawn_forever, AppState, ChaptersState, ExportOpts,
    GroupingChoice, JobKind, JobView, LoadedChapters, Page, PreviewState, SeriesTab, Shot,
    TableView, Workspace,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use futures_util::future::{join, join_all};
use library::{chapter_key, ExportRecord, ExportedFileRecord};
use pipeline::{
    download_chapters, export_series, requests_from_merged, CancelFlag, ExportJob, ExportTarget,
    Grouping, JobEvent, PipelineError, PreviewImage, RetryPolicy,
};
use search::{merge_chapters, RankedSeries, ScoredMember};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

// ---- Opening a series -------------------------------------------------------

fn reset_workspace_state(state: AppState) {
    let (mut selected, mut table, mut preview, mut opts) = (
        state.selected,
        state.table,
        state.preview,
        state.export_opts,
    );
    selected.set(HashSet::new());
    table.set(TableView::default());
    preview.set(PreviewState::default());
    opts.set(ExportOpts::default());
}

/// Open a search result: show its chapters and fetch their lists.
pub fn open_series_from_search(state: AppState, ranked: RankedSeries) {
    let title = ranked.title.clone();
    let series_id = library::series_id(&title);
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: series_id.clone(),
        title,
        ranked: Some(ranked.clone()),
        tab: SeriesTab::Chapters,
        origin: Page::Search,
    }));
    let mut chapters = state.chapters;
    chapters.set(ChaptersState::Loading);
    state.go(Page::Series);

    let members = logic::one_per_source(&ranked.members, 4);
    spawn_forever(async move {
        let result = load_chapters(state, members).await;
        // The user may have opened something else meanwhile.
        let still_here = state
            .workspace
            .peek()
            .as_ref()
            .is_some_and(|w| w.series_id == series_id);
        if still_here {
            let mut chapters = state.chapters;
            chapters.set(match result {
                Some(loaded) => ChaptersState::Ready(Rc::new(loaded)),
                None => ChaptersState::Failed("The catalog engine is not running.".into()),
            });
        }
    });
}

/// Open a downloaded series from the Library, ready to preview.
pub fn open_series_from_library(state: AppState, series_id: &str) {
    let Some(title) = state
        .library
        .peek()
        .find(series_id)
        .map(|s| s.title.clone())
    else {
        return;
    };
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: series_id.to_string(),
        title,
        ranked: None,
        tab: SeriesTab::Preview,
        origin: Page::Library,
    }));
    let mut chapters = state.chapters;
    chapters.set(ChaptersState::Idle);
    state.go(Page::Series);
    prepare_preview(state);
}

async fn load_chapters(state: AppState, members: Vec<ScoredMember>) -> Option<LoadedChapters> {
    let client = state.client()?;
    let names: Vec<&str> = members
        .iter()
        .map(|m| m.candidate.source_name.as_str())
        .collect();
    info!("Loading chapter lists from {}", names.join(", "));
    let started = Instant::now();
    let fetches = members.iter().map(|m| {
        let client = client.clone();
        let id = m.candidate.manga_id;
        async move { tokio::time::timeout(Duration::from_secs(60), client.fetch_chapters(id)).await }
    });
    let mut lists = Vec::new();
    let mut unreachable = 0;
    for result in join_all(fetches).await {
        match result {
            Ok(Ok(chapters)) => lists.push(chapters),
            _ => {
                unreachable += 1;
                lists.push(Vec::new());
            }
        }
    }
    let merged = merge_chapters(&lists);
    let both = merged.iter().filter(|m| m.options.len() > 1).count();
    info!(
        "{} chapters in total ({both} available from more than one source) in {:.1}s{}",
        merged.len(),
        started.elapsed().as_secs_f32(),
        if unreachable > 0 {
            format!("; {unreachable} source(s) did not answer")
        } else {
            String::new()
        }
    );
    Some(LoadedChapters {
        merged,
        member_sources: members
            .iter()
            .map(|m| m.candidate.source_id.clone())
            .collect(),
        member_names: members
            .iter()
            .map(|m| m.candidate.source_name.clone())
            .collect(),
        unreachable,
    })
}

/// Keys of the chapters of this series that are already downloaded.
pub fn downloaded_keys(state: AppState, series_id: &str) -> HashSet<String> {
    state
        .library
        .read()
        .find(series_id)
        .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
        .unwrap_or_default()
}

/// The library key of a merged chapter (named after its best source's copy).
pub fn merged_key(ch: &search::MergedChapter) -> String {
    chapter_key(ch.number, ch.options.first().map_or(0, |o| o.chapter_id))
}

// ---- Downloading -----------------------------------------------------------

fn job_running(state: AppState) -> bool {
    state
        .job
        .peek()
        .as_ref()
        .is_some_and(|j| j.result.is_none())
}

/// Download the ticked chapters that are not downloaded yet.
pub fn start_download(state: AppState) {
    if job_running(state) {
        state.error("Something is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let ChaptersState::Ready(loaded) = state.chapters.peek().clone() else {
        state.error("The chapter list is still loading.");
        return;
    };
    let have = downloaded_keys(state, &ws.series_id);
    let mut picked: Vec<usize> = state.selected.peek().iter().copied().collect();
    picked.sort_unstable();

    let mut plan = Vec::new();
    let mut requests = Vec::new();
    for i in picked {
        let Some(ch) = loaded.merged.get(i) else {
            continue;
        };
        let key = merged_key(ch);
        if have.contains(&key) {
            continue;
        }
        requests.extend(requests_from_merged(
            std::slice::from_ref(ch),
            &loaded.member_sources,
        ));
        plan.push(key);
    }
    if requests.is_empty() {
        state.notify("Everything you picked is already downloaded.");
        if !have.is_empty() {
            state.open_tab(SeriesTab::Preview);
            prepare_preview(state);
        }
        return;
    }

    let cancel = CancelFlag::new();
    let plan_keys = plan.clone();
    let mut job = state.job;
    job.set(Some(JobView {
        kind: JobKind::Download,
        title: ws.title.clone(),
        plan,
        progress: JobProgress::default(),
        result: None,
        skipped: Vec::new(),
        failed: Vec::new(),
        cancel: cancel.clone(),
    }));
    state.open_tab(SeriesTab::Downloads);
    let shrink = state.settings.peek().shrink_downloads;
    info!(
        "Download started: \"{}\", {} chapter(s){}",
        ws.title,
        requests.len(),
        if shrink { " (saving space)" } else { "" }
    );

    spawn_forever(async move {
        let Some(client) = state.client() else {
            return;
        };
        let started = Instant::now();
        let files = library_files();
        let defaults = state.settings.peek().conversion.clone();
        let policy = RetryPolicy::default();
        let (tx, mut rx) = unbounded_channel();
        let run = download_chapters(
            &*client,
            &files,
            &ws.series_id,
            &requests,
            shrink,
            &policy,
            Some(tx),
            &cancel,
        );
        let watch = async {
            let mut logged = 0;
            while let Some(event) = rx.recv().await {
                log_event(&event, &mut logged);
                if let JobEvent::ChapterSaved { chapter } = &event {
                    // Record each chapter as soon as it is safe on disk, so
                    // cancelling or failing later never loses it.
                    let mut library = state.library;
                    library.write().record_chapter(
                        &ws.title,
                        &defaults,
                        chapter.clone(),
                        now_secs(),
                    );
                    state.save_library();
                }
                if let Some(view) = job.write().as_mut() {
                    view.progress.apply(&event);
                }
            }
        };
        let (result, ()) = join(run, watch).await;

        match result {
            Ok(outcome) => {
                {
                    let mut health = state.health;
                    let mut h = health.write();
                    for u in &outcome.source_use {
                        h.record_use(&u.source_id, u.ok, u.ms);
                    }
                }
                state.save_health();
                let bytes: u64 = outcome.chapters.iter().map(|c| c.bytes).sum();
                let skipped: Vec<String> = outcome
                    .skipped
                    .iter()
                    .map(|s| format!("{}: {}", s.name, s.reason))
                    .collect();
                let summary = format!(
                    "Downloaded {} chapter(s), {}{}",
                    outcome.chapters.len(),
                    logic::human_bytes(bytes),
                    if skipped.is_empty() {
                        String::new()
                    } else {
                        format!("; {} could not be downloaded", skipped.len())
                    }
                );
                info!("{summary} in {:.0}s", started.elapsed().as_secs_f32());
                // `skipped.index` counts within the chapters we asked for.
                let failed: Vec<String> = outcome
                    .skipped
                    .iter()
                    .filter_map(|s| plan_keys.get(s.index).cloned())
                    .collect();
                if let Some(view) = job.write().as_mut() {
                    view.skipped = skipped;
                    view.failed = failed;
                    view.result = Some(Ok(summary));
                }
                // Take the user on to the preview.
                let same = state
                    .workspace
                    .peek()
                    .as_ref()
                    .is_some_and(|w| w.series_id == ws.series_id);
                if same {
                    state.open_tab(SeriesTab::Preview);
                    prepare_preview(state);
                }
            }
            Err(e) => {
                let cancelled = matches!(e, PipelineError::Cancelled);
                let message = if cancelled {
                    "Cancelled. Chapters that finished are kept.".to_string()
                } else {
                    e.to_string()
                };
                if cancelled {
                    info!("Download cancelled");
                } else {
                    warn!("Download failed: {message}");
                }
                if let Some(view) = job.write().as_mut() {
                    view.result = Some(Err(message));
                }
            }
        }
    });
}

// ---- Previewing ------------------------------------------------------------

fn png_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", STANDARD.encode(png))
}

fn shot(image: &PreviewImage) -> Shot {
    Shot {
        url: png_url(&image.png),
        width: image.width,
        height: image.height,
    }
}

/// Make sure the preview points at a downloaded chapter, then render it.
pub fn prepare_preview(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let keys: Vec<String> = state
        .library
        .peek()
        .find(&ws.series_id)
        .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
        .unwrap_or_default();
    let mut preview = state.preview;
    {
        let mut p = preview.write();
        let valid = p.chapter_key.as_ref().is_some_and(|k| keys.contains(k));
        if !valid {
            p.chapter_key = keys.first().cloned();
            p.page = 0;
            p.decoded = None;
        }
    }
    schedule_preview(state, 0);
}

/// Render the current page with the series' settings. Settings changes call
/// this with a short delay, so dragging a slider renders once it settles.
pub fn schedule_preview(state: AppState, delay_ms: u64) {
    let mut preview = state.preview;
    let generation = {
        let mut p = preview.write();
        p.generation += 1;
        p.busy = true;
        p.error = None;
        p.generation
    };
    spawn_forever(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        let current = |state: AppState| state.preview.peek().generation == generation;
        if !current(state) {
            return;
        }
        let fail = |message: String| {
            let mut p = state.preview;
            if p.peek().generation == generation {
                let mut w = p.write();
                w.busy = false;
                w.error = Some(message);
                w.converted.clear();
                w.original = None;
            }
        };
        let Some(ws) = state.workspace.peek().clone() else {
            return;
        };
        let (key, wanted_page, cached) = {
            let p = state.preview.peek();
            (p.chapter_key.clone(), p.page, p.decoded.clone())
        };
        let Some(key) = key else {
            let mut p = state.preview;
            if current(state) {
                p.write().busy = false;
            }
            return;
        };
        let Some(series) = state.library.peek().find(&ws.series_id).cloned() else {
            return fail("This series is not in your library.".into());
        };
        let device = state.settings.peek().device;
        let files = library_files();
        let pages = files.page_files(&ws.series_id, &key);
        if pages.is_empty() {
            return fail("This chapter has no saved pages. Download it again.".into());
        }
        let page = wanted_page.min(pages.len() - 1);

        // Decode the page once; changing a setting reuses it.
        let gray = match cached {
            Some((k, p, g)) if k == key && p == page => g,
            _ => {
                let bytes = match tokio::fs::read(&pages[page]).await {
                    Ok(b) => b,
                    Err(e) => return fail(format!("Could not read the saved page: {e}")),
                };
                match tokio::task::spawn_blocking(move || pipeline::decode_page(&bytes)).await {
                    Ok(Ok(g)) => Rc::new(g),
                    Ok(Err(e)) => return fail(e.to_string()),
                    Err(e) => return fail(e.to_string()),
                }
            }
        };
        if !current(state) {
            return;
        }

        // The same conversion the export runs, on a background thread.
        let work = (*gray).clone();
        let conversion = series.settings.clone();
        let rendered = tokio::task::spawn_blocking(move || {
            let opts = pipeline::process_options_for(device, &conversion);
            let levels = pipeline::render_levels(&work, &opts);
            (
                pipeline::levels_to_previews(&levels),
                pipeline::render_original(&work, &opts),
            )
        })
        .await;
        let Ok((converted, original)) = rendered else {
            return fail("The preview could not be drawn.".into());
        };
        let mut p = state.preview;
        if p.peek().generation != generation {
            return;
        }
        let mut w = p.write();
        w.converted = converted.iter().map(shot).collect();
        w.original = Some(shot(&original));
        w.total_pages = pages.len();
        w.page = page;
        w.decoded = Some((key, page, gray));
        w.busy = false;
        w.error = None;
    });
}

// ---- Exporting -------------------------------------------------------------

pub fn grouping_of(opts: &ExportOpts) -> Grouping {
    match opts.grouping {
        GroupingChoice::Single => Grouping::Single,
        GroupingChoice::PerChapter => Grouping::PerChapter,
        GroupingChoice::EveryN => {
            Grouping::EveryN(opts.every_n.trim().parse::<usize>().unwrap_or(10).max(1))
        }
    }
}

/// Build the reader files from the downloaded chapters.
pub fn start_export(state: AppState) {
    if job_running(state) {
        state.error("Something is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let Some(series) = state.library.peek().find(&ws.series_id).cloned() else {
        state.error("Download some chapters first.");
        return;
    };
    let opts = state.export_opts.peek().clone();
    let settings = state.settings.peek().clone();
    let keys: Vec<String> = series
        .chapters
        .iter()
        .filter(|c| !opts.excluded.contains(&c.key))
        .map(|c| c.key.clone())
        .collect();
    if keys.is_empty() {
        state.error("Tick at least one chapter to export.");
        return;
    }
    let target = match logic::export_target(&opts.target, &settings, &paths().default_export_dir())
    {
        Ok(t) => t,
        Err(message) => {
            state.error(message);
            return;
        }
    };
    let mut export = ExportJob::new(
        series.clone(),
        keys.clone(),
        settings.device,
        target.clone(),
        paths().spool_dir(),
    );
    export.language = settings.language.clone();
    export.grouping = grouping_of(&opts);

    let cancel = CancelFlag::new();
    let mut job = state.job;
    job.set(Some(JobView {
        kind: JobKind::Export,
        title: ws.title.clone(),
        plan: keys.clone(),
        progress: JobProgress::default(),
        result: None,
        skipped: Vec::new(),
        failed: Vec::new(),
        cancel: cancel.clone(),
    }));
    info!(
        "Export started: \"{}\", {} chapter(s) as {}, {}, to {}",
        series.title,
        keys.len(),
        series.settings.format.extension().to_uppercase(),
        match export.grouping {
            Grouping::Single => "one file".to_string(),
            Grouping::PerChapter => "one file per chapter".to_string(),
            Grouping::EveryN(n) => format!("one file per {n} chapters"),
        },
        describe_target(&target)
    );

    spawn_forever(async move {
        let started = Instant::now();
        let files = library_files();
        let (tx, mut rx) = unbounded_channel();
        let run = export_series(&files, &export, Some(tx), &cancel);
        let watch = async {
            let mut logged = 0;
            while let Some(event) = rx.recv().await {
                log_event(&event, &mut logged);
                if let Some(view) = job.write().as_mut() {
                    view.progress.apply(&event);
                }
            }
        };
        let (result, ()) = join(run, watch).await;

        match result {
            Ok(outcome) => {
                info!(
                    "Export finished in {:.0}s: {} pages from {} chapter(s)",
                    started.elapsed().as_secs_f32(),
                    outcome.pages,
                    outcome.chapters
                );
                for f in &outcome.files {
                    info!("Wrote {} ({})", f.location, logic::human_bytes(f.bytes));
                }
                let record = ExportRecord {
                    id: format!("{}-{}", series.id, now_secs()),
                    series_id: series.id.clone(),
                    title: series.title.clone(),
                    chapters: outcome.chapters,
                    pages: outcome.pages,
                    format: series.settings.format.extension().to_string(),
                    files: outcome
                        .files
                        .iter()
                        .map(|f| ExportedFileRecord {
                            path: f.location.clone(),
                            bytes: f.bytes,
                        })
                        .collect(),
                    target: describe_target(&target),
                    at: now_secs(),
                };
                let mut exports = state.exports;
                exports.write().add(record);
                state.save_exports();
                let summary = format!("Exported {} file(s)", outcome.files.len());
                if let Some(view) = job.write().as_mut() {
                    view.result = Some(Ok(summary));
                }
            }
            Err(e) => {
                let cancelled = matches!(e, PipelineError::Cancelled);
                let message = if cancelled {
                    "Cancelled.".to_string()
                } else {
                    e.to_string()
                };
                if cancelled {
                    info!("Export cancelled");
                } else {
                    warn!("Export failed: {message}");
                }
                if let Some(view) = job.write().as_mut() {
                    view.result = Some(Err(message));
                }
            }
        }
    });
}

pub fn describe_target(target: &ExportTarget) -> String {
    match target {
        ExportTarget::Folder { path } => format!("folder {}", path.display()),
        ExportTarget::Device { mount, folder } => {
            format!("the reader at {}\\{folder}", mount.display())
        }
        ExportTarget::WebDav { base_url, .. } => format!("the reader over Wi-Fi ({base_url})"),
    }
}

/// One readable line per meaningful step; page progress every 25 pages.
fn log_event(event: &JobEvent, logged_pages: &mut usize) {
    match event {
        JobEvent::ChapterStarted { index, total, name } => {
            *logged_pages = 0;
            info!("Chapter {}/{}: {name}", index + 1, total);
        }
        JobEvent::PageDone { done, total, .. } => {
            if done / 25 > *logged_pages / 25 || done == total {
                info!("  {done}/{total} pages");
            }
            *logged_pages = *done;
        }
        JobEvent::UsedFallback {
            from, to, reason, ..
        } => {
            warn!("  source {from} failed ({reason}); trying {to}");
        }
        JobEvent::ChapterSkipped { name, reason, .. } => {
            warn!("  skipped {name}: {reason}");
        }
        JobEvent::ChapterSaved { chapter } => {
            info!(
                "  saved {} ({} pages, {})",
                chapter.name,
                chapter.pages,
                logic::human_bytes(chapter.bytes)
            );
        }
        JobEvent::Writing { file } => info!("Writing {file}"),
    }
}

// ---- Deleting --------------------------------------------------------------

/// Delete every downloaded page of a series. Exported files are kept.
pub fn delete_downloaded_series(state: AppState, series_id: String) {
    spawn_forever(async move {
        let files = library_files();
        let id = series_id.clone();
        let removed = tokio::task::spawn_blocking(move || files.remove_series(&id)).await;
        match removed {
            Ok(Ok(())) => {
                let mut library = state.library;
                let title = library.write().remove_series(&series_id).map(|s| s.title);
                state.save_library();
                info!(
                    "Deleted the downloaded pages of {}",
                    title.unwrap_or(series_id.clone())
                );
                // If it was open, close it.
                let open_here = state
                    .workspace
                    .peek()
                    .as_ref()
                    .is_some_and(|w| w.series_id == series_id);
                if open_here {
                    let mut ws = state.workspace;
                    ws.set(None);
                    if *state.page.peek() == Page::Series {
                        state.go(Page::Library);
                    }
                }
            }
            Ok(Err(e)) => state.error(format!("Could not delete the downloaded pages: {e}")),
            Err(e) => state.error(format!("Could not delete the downloaded pages: {e}")),
        }
    });
}

pub fn delete_downloaded_chapter(state: AppState, series_id: String, key: String) {
    spawn_forever(async move {
        let files = library_files();
        let (id, k) = (series_id.clone(), key.clone());
        let removed = tokio::task::spawn_blocking(move || files.remove_chapter(&id, &k)).await;
        match removed {
            Ok(Ok(())) => {
                let mut library = state.library;
                library.write().remove_chapter(&series_id, &key);
                state.save_library();
                // Keep the preview pointing at something that still exists.
                let gone = state.library.peek().find(&series_id).is_none();
                if gone {
                    let mut ws = state.workspace;
                    ws.set(None);
                    state.go(Page::Library);
                } else {
                    prepare_preview(state);
                }
            }
            Ok(Err(e)) => state.error(format!("Could not delete that chapter: {e}")),
            Err(e) => state.error(format!("Could not delete that chapter: {e}")),
        }
    });
}

/// Delete an exported file from disk (and forget it).
pub fn delete_exported_file(state: AppState, record_id: String, path: String) {
    let result = library::delete_exported_file(std::path::Path::new(&path));
    match result {
        Ok(_) => {
            let mut exports = state.exports;
            exports.write().forget_file(&record_id, &path);
            state.save_exports();
            info!("Deleted {path}");
        }
        Err(e) => state.error(format!("Could not delete the file: {e}")),
    }
}

/// Forget an exported file that is no longer on disk.
pub fn forget_exported_file(state: AppState, record_id: String, path: String) {
    let mut exports = state.exports;
    exports.write().forget_file(&record_id, &path);
    state.save_exports();
}
