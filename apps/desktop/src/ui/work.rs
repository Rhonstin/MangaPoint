//! Background work: opening a series, downloading, previewing, exporting,
//! and deleting. Everything here runs with `spawn_forever`, so leaving the
//! screen that started it does not stop it.

use crate::logic::{self, JobProgress};
use crate::state::{
    library_files, now_secs, paths, spawn_forever, AppState, ChaptersState, ExportOpts,
    GroupingChoice, JobKind, JobView, LoadedChapters, Page, PreviewPage, PreviewState, SeriesTab,
    Shot, TableView, Workspace,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use futures_util::future::{join, join_all};
use futures_util::stream::{self, StreamExt};
use library::{chapter_key, ExportRecord, ExportedFileRecord};
use pipeline::{
    download_chapters, export_series, requests_from_merged, CancelFlag, ExportJob, ExportTarget,
    Grouping, JobEvent, PipelineError, PreviewImage, RetryPolicy, SourceChoice,
};
use search::{merge_chapters, RankedSeries, ScoredMember};
use std::collections::{HashMap, HashSet};
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
    // A new series starts with an empty preview, but keeps how the user likes to view it.
    let view = preview.peek().view;
    preview.set(PreviewState {
        view,
        ..Default::default()
    });
    opts.set(ExportOpts::default());
    let mut source = state.download_source;
    source.set(SourceChoice::Adaptive);
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
        tab: SeriesTab::Overview,
        origin: Page::Library,
    }));
    let mut chapters = state.chapters;
    chapters.set(ChaptersState::Idle);
    state.go(Page::Series);
}

/// The key a series' saved cover is kept under in `state.covers`.
pub fn saved_cover_key(series_id: &str) -> String {
    format!("saved:{series_id}")
}

/// Show the cover saved with a series, once it has been read from disk.
pub fn load_saved_cover(state: AppState, series_id: &str) {
    let key = saved_cover_key(series_id);
    if state.covers.peek().contains_key(&key) {
        return;
    }
    let path = library_files().cover_path(series_id);
    spawn_forever(async move {
        let Ok(bytes) = tokio::fs::read(&path).await else {
            return;
        };
        let mime = match bytes.first() {
            Some(0x89) => "image/png",
            Some(b'R') => "image/webp",
            Some(b'G') => "image/gif",
            _ => "image/jpeg",
        };
        let url = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
        let mut covers = state.covers;
        covers.write().insert(key, url);
    });
}

/// Fetch and keep what the sources say about a downloaded series (its
/// description, author, genres and cover), if that has not been done. It asks
/// about a chapter the series was downloaded from, so it also works for series
/// downloaded before details were saved.
pub fn ensure_series_info(state: AppState, series_id: &str) {
    let Some(series) = state.library.peek().find(series_id).cloned() else {
        return;
    };
    if series.info.is_some() || state.info_busy.peek().contains(series_id) {
        return;
    }
    let Some(client) = state.client() else {
        return;
    };
    let id = series_id.to_string();
    let mut busy = state.info_busy;
    busy.write().insert(id.clone());
    spawn_forever(async move {
        let files = library_files();
        // The first chapter that the server still knows about.
        let mut details = None;
        for chapter in series.chapters.iter().take(4) {
            if let Ok(found) = client.manga_of_chapter(chapter.chapter_id).await {
                details = Some(found);
                break;
            }
        }
        if let Some(details) = details {
            let names: HashMap<String, String> = client
                .list_sources()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.id, s.name))
                .collect();
            let mut source_names: Vec<(String, String)> = Vec::new();
            for chapter in &series.chapters {
                if let Some(name) = names.get(&chapter.source_id) {
                    if !source_names.iter().any(|(id, _)| *id == chapter.source_id) {
                        source_names.push((chapter.source_id.clone(), name.clone()));
                    }
                }
            }
            let mut has_cover = false;
            if let Some(path) = details.thumbnail_url.as_deref() {
                if let Ok(bytes) = client.get_bytes(path).await {
                    if converter::decode::image_dimensions(&bytes).is_ok() {
                        let target = files.cover_path(&id);
                        if let Some(dir) = target.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        has_cover = std::fs::write(&target, &bytes).is_ok();
                    }
                }
            }
            let info = logic::series_info_from(&details, source_names, has_cover, now_secs());
            let mut library = state.library;
            library.write().set_info(&id, info);
            state.save_library();
            info!("Saved details for \"{}\"", series.title);
            if has_cover {
                load_saved_cover(state, &id);
            }
        } else {
            warn!("Could not fetch details for \"{}\"", series.title);
        }
        let mut busy = state.info_busy;
        busy.write().remove(&id);
    });
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
    // A single chosen source: leave out what it does not have, and say so.
    let choice = state.download_source.peek().clone();
    let source_names: HashMap<String, String> = loaded
        .member_sources
        .iter()
        .cloned()
        .zip(loaded.member_names.iter().cloned())
        .collect();
    let mut not_offered: Vec<(String, String)> = Vec::new();
    if let SourceChoice::Only(source) = &choice {
        let names: Vec<String> = requests.iter().map(|r| r.name.clone()).collect();
        let chosen = pipeline::apply_choice(std::mem::take(&mut requests), &choice, &[]);
        for &i in &chosen.unavailable {
            not_offered.push((plan[i].clone(), names[i].clone()));
        }
        plan = chosen.kept.iter().map(|&i| plan[i].clone()).collect();
        requests = chosen.requests;
        if requests.is_empty() {
            let name = source_names.get(source).cloned().unwrap_or_default();
            state.error(format!(
                "{name} does not have the chapters you picked. Choose Adaptive, or another source."
            ));
            return;
        }
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
        skipped: not_offered
            .iter()
            .map(|(_, name)| {
                let source = match &choice {
                    SourceChoice::Only(id) => source_names.get(id).cloned().unwrap_or_default(),
                    SourceChoice::Adaptive => String::new(),
                };
                format!("{name}: {source} does not have it")
            })
            .collect(),
        failed: not_offered.iter().map(|(key, _)| key.clone()).collect(),
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
        let default_preset = state.settings.peek().default_preset_id.clone();
        let policy = RetryPolicy::default();
        let limits = state.settings.peek().download.limits();
        // Adaptive: test the sources that cover about the same, then put the
        // best first for every chapter (the others stay as fallbacks).
        let requests = if choice == SourceChoice::Adaptive {
            order_sources(state, &client, requests, &source_names).await
        } else {
            requests
        };
        let (tx, mut rx) = unbounded_channel();
        let run = download_chapters(
            &*client,
            &files,
            &ws.series_id,
            &requests,
            shrink,
            limits,
            &policy,
            Some(tx),
            &cancel,
        );
        let watch = async {
            let mut logged = HashMap::new();
            while let Some(first) = rx.recv().await {
                // Take everything that has arrived and show it in one go:
                // redrawing for every single page would keep the screen busy
                // far more than anyone can see.
                let mut batch = vec![first];
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                }
                for event in &batch {
                    log_event(event, &mut logged);
                    if let JobEvent::ChapterSaved { chapter } = event {
                        // Record each chapter as soon as it is safe on disk, so
                        // cancelling or failing later never loses it.
                        let mut library = state.library;
                        {
                            let mut db = library.write();
                            let id = db.record_chapter(
                                &ws.title,
                                &defaults,
                                chapter.clone(),
                                now_secs(),
                            );
                            db.adopt_default_preset(&id, &defaults, &default_preset);
                        }
                        state.save_library();
                    }
                }
                if let Some(view) = job.write().as_mut() {
                    for event in &batch {
                        view.progress.apply(event);
                    }
                }
                tokio::time::sleep(PROGRESS_REFRESH).await;
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

/// Rank the sources for these chapters, speed-testing the ones that matter
/// and have not been tested lately, and put the best first for every chapter.
async fn order_sources(
    state: AppState,
    client: &catalog::SuwayomiClient,
    requests: Vec<pipeline::ChapterRequest>,
    names: &HashMap<String, String>,
) -> Vec<pipeline::ChapterRequest> {
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let now = now_secs();
    let ranked = pipeline::rank_sources(&requests, |s| state.speeds.peek().speed(s));
    // Sources that cover about as much as the best are worth comparing; one
    // that was tested recently does not need testing again.
    let to_test: Vec<(String, pipeline::ChapterRef)> = {
        let speeds = state.speeds.peek();
        let tier = pipeline::top_tier(&ranked);
        if tier.len() < 2 {
            Vec::new()
        } else {
            tier.iter()
                .filter(|r| !speeds.is_fresh(&r.source_id, now))
                .filter_map(|r| {
                    let option = requests
                        .iter()
                        .find_map(|q| q.options.iter().find(|o| o.source_id == r.source_id))?;
                    Some((r.source_id.clone(), option.clone()))
                })
                .collect()
        }
    };
    if !to_test.is_empty() {
        let mut job = state.job;
        if let Some(view) = job.write().as_mut() {
            view.progress.note = Some("Checking which source is fastest…".into());
        }
        let results = pipeline::probe_sources(client, &to_test, &RetryPolicy::default()).await;
        for (source, speed) in results {
            match speed {
                Some(pages_per_second) => {
                    info!(
                        "Speed test: {} {pages_per_second:.1} pages/s",
                        name_of(&source)
                    );
                    state.edit_speeds(|db| db.record(&source, pages_per_second, now_secs()));
                }
                None => {
                    warn!("Speed test: {} gave no usable pages", name_of(&source));
                    state.edit_speeds(|db| db.record_failure(&source, now_secs()));
                }
            }
        }
        state.save_speeds();
    }
    let ranked = pipeline::rank_sources(&requests, |s| state.speeds.peek().speed(s));
    for r in &ranked {
        info!(
            "Source order: {} covers {} chapter(s), {}",
            name_of(&r.source_id),
            r.coverage,
            r.speed
                .map_or("not measured".to_string(), |v| format!("{v:.1} pages/s"))
        );
    }
    pipeline::apply_choice(requests, &SourceChoice::Adaptive, &ranked).requests
}

/// How often the progress on screen is brought up to date while a download or
/// export runs (about seven times a second).
const PROGRESS_REFRESH: Duration = Duration::from_millis(140);

// ---- The Presets page ---------------------------------------------------------

/// Start editing a preset: its settings become the draft, and the test page is
/// drawn with them.
pub fn select_lab_preset(state: AppState, id: &str) {
    let Some(entry) = state.presets.peek().find(id) else {
        return;
    };
    let mut lab = state.preset_lab;
    {
        let mut l = lab.write();
        l.selected = Some(entry.preset.id.clone());
        l.draft = entry.preset.settings.clone();
    }
    schedule_lab(state, 0);
}

/// Choose the test page: a downloaded series' pages, or the built-in sample.
pub fn choose_lab_sample(state: AppState, series_id: Option<String>) {
    let mut lab = state.preset_lab;
    {
        let mut l = lab.write();
        l.sample_series = series_id;
        // Early pages are often a banner or credits; the third usually has art.
        l.page = 2;
        l.total_pages = 0;
    }
    schedule_lab(state, 0);
}

/// Draw the test page with the draft settings. Changes call this with a short
/// delay, so dragging a slider draws once it settles.
pub fn schedule_lab(state: AppState, delay_ms: u64) {
    let mut lab = state.preset_lab;
    let generation = {
        let mut l = lab.write();
        l.generation += 1;
        l.busy = true;
        l.error = None;
        l.generation
    };
    spawn_forever(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        let current = |state: AppState| state.preset_lab.peek().generation == generation;
        if !current(state) {
            return;
        }
        let (draft, sample, wanted_page) = {
            let l = state.preset_lab.peek();
            (l.draft.clone(), l.sample_series.clone(), l.page)
        };
        let device = state.settings.peek().device;
        let opts = pipeline::process_options_for(device, &draft);

        // The page to try: from the library, or drawn from scratch.
        let mut total_pages = 0;
        let mut page_index = 0;
        let mut file = None;
        if let Some(series_id) = &sample {
            let first = state
                .library
                .peek()
                .find(series_id)
                .and_then(|s| s.chapters.first().map(|c| c.key.clone()));
            if let Some(key) = first {
                let pages = library_files().page_files(series_id, &key);
                if !pages.is_empty() {
                    total_pages = pages.len();
                    page_index = wanted_page.min(pages.len() - 1);
                    file = Some(pages[page_index].clone());
                }
            }
        }
        let rendered = tokio::task::spawn_blocking(move || match file {
            Some(path) => std::fs::read(&path)
                .map_err(|e| format!("Could not read the page: {e}"))
                .and_then(|bytes| pipeline::render_page(&bytes, &opts).map_err(|e| e.to_string())),
            None => Ok(pipeline::render_gray(&pipeline::sample_page(), &opts)),
        })
        .await;
        if !current(state) {
            return;
        }
        let mut lab = state.preset_lab;
        let mut l = lab.write();
        l.busy = false;
        match rendered {
            Ok(Ok(page)) => {
                l.converted = page.converted.iter().map(shot).collect();
                l.original = Some(shot(&page.original.image));
                l.kept = Some(page.original.kept);
                l.total_pages = total_pages;
                l.page = page_index;
            }
            Ok(Err(message)) => l.error = Some(message),
            Err(e) => l.error = Some(e.to_string()),
        }
    });
}

/// Stop the running download or export. The screen says so at once; the job
/// itself ends as soon as the pages already on their way are dropped.
pub fn cancel_job(state: AppState) {
    let mut job = state.job;
    let mut guard = job.write();
    if let Some(view) = guard.as_mut() {
        if view.result.is_none() {
            view.cancel.cancel();
            view.progress.stopping = true;
        }
    }
}

// ---- Previewing ------------------------------------------------------------

fn data_url(image: &PreviewImage) -> String {
    format!(
        "data:{};base64,{}",
        image.mime,
        STANDARD.encode(&image.bytes)
    )
}

fn shot(image: &PreviewImage) -> Shot {
    Shot {
        url: data_url(image),
        width: image.width,
        height: image.height,
    }
}

/// Make sure the preview points at a downloaded chapter, and that it is drawn.
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
    let up_to_date = {
        let mut p = preview.write();
        let valid = p.chapter_key.as_ref().is_some_and(|k| keys.contains(k));
        if !valid {
            p.chapter_key = keys.first().cloned();
            p.pages.clear();
            p.fresh = 0;
        }
        let on_disk = p
            .chapter_key
            .as_ref()
            .map_or(0, |k| library_files().page_files(&ws.series_id, k).len());
        valid && on_disk > 0 && p.pages.len() == on_disk && p.fresh == on_disk && !p.busy
    };
    if !up_to_date {
        schedule_preview(state, 0);
    }
}

/// Draw every page of the chapter with the series' settings, a few at a time
/// in the background. Settings changes call this with a short delay, so
/// dragging a slider draws once it settles.
pub fn schedule_preview(state: AppState, delay_ms: u64) {
    let mut preview = state.preview;
    let generation = {
        let mut p = preview.write();
        p.generation += 1;
        p.busy = true;
        p.error = None;
        p.fresh = 0;
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
                w.pages.clear();
            }
        };
        let Some(ws) = state.workspace.peek().clone() else {
            return;
        };
        let key = state.preview.peek().chapter_key.clone();
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
        let pages = library_files().page_files(&ws.series_id, &key);
        if pages.is_empty() {
            return fail("This chapter has no saved pages. Download it again.".into());
        }
        {
            let mut p = state.preview;
            let mut w = p.write();
            if w.generation != generation {
                return;
            }
            w.pages.resize(pages.len(), None);
        }

        // The same conversion the export runs, on background threads.
        let opts = pipeline::process_options_for(device, &series.settings);
        let mut drawn = stream::iter(pages.into_iter().enumerate())
            .map(|(index, path)| {
                let opts = opts.clone();
                async move {
                    let result = tokio::task::spawn_blocking(move || {
                        let bytes = std::fs::read(&path)
                            .map_err(|e| format!("Could not read a saved page: {e}"))?;
                        let page =
                            pipeline::render_page(&bytes, &opts).map_err(|e| e.to_string())?;
                        Ok::<PreviewPage, String>(PreviewPage {
                            converted: page.converted.iter().map(shot).collect(),
                            original: shot(&page.original.image),
                            kept: page.original.kept,
                        })
                    })
                    .await;
                    (index, result)
                }
            })
            .buffer_unordered(PREVIEW_THREADS);
        let mut first_error = None;
        while let Some((index, result)) = drawn.next().await {
            if !current(state) {
                return;
            }
            match result {
                Ok(Ok(page)) => {
                    let mut p = state.preview;
                    let mut w = p.write();
                    if let Some(slot) = w.pages.get_mut(index) {
                        *slot = Some(Rc::new(page));
                    }
                    w.fresh += 1;
                }
                Ok(Err(message)) => {
                    first_error.get_or_insert(format!("Page {}: {message}", index + 1));
                }
                Err(e) => {
                    first_error.get_or_insert(e.to_string());
                }
            }
        }
        let mut p = state.preview;
        if p.peek().generation == generation {
            let mut w = p.write();
            w.busy = false;
            w.error = first_error;
        }
    });
}

/// Pages drawn at the same time while previewing.
const PREVIEW_THREADS: usize = 3;

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
            let mut logged = HashMap::new();
            while let Some(first) = rx.recv().await {
                let mut batch = vec![first];
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                }
                for event in &batch {
                    log_event(event, &mut logged);
                }
                if let Some(view) = job.write().as_mut() {
                    for event in &batch {
                        view.progress.apply(event);
                    }
                }
                tokio::time::sleep(PROGRESS_REFRESH).await;
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

/// One readable line per meaningful step; page progress every 25 pages of
/// each chapter (chapters can download together, so progress is kept per chapter).
fn log_event(event: &JobEvent, logged_pages: &mut HashMap<usize, usize>) {
    match event {
        JobEvent::ChapterStarted { index, total, name } => {
            logged_pages.insert(*index, 0);
            info!("Chapter {}/{}: {name}", index + 1, total);
        }
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        } => {
            let last = logged_pages.entry(*chapter_index).or_insert(0);
            if done / 25 > *last / 25 || done == total {
                info!("  chapter {}: {done}/{total} pages", chapter_index + 1);
            }
            *last = *done;
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
        JobEvent::ChapterDone { .. } => {}
        JobEvent::PagesAtOnce { now, chosen } => {
            if now < chosen {
                warn!("  the source is pushing back: {now} requests at once instead of {chosen}");
            } else {
                info!("  the source recovered: back to {chosen} requests at once");
            }
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
                    let _ = library_files().remove_series(&series_id);
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
