//! Background work: opening a series, downloading, previewing, exporting,
//! and deleting. Everything here runs with `spawn_forever`, so leaving the
//! screen that started it does not stop it.

use crate::logic::{self, JobProgress};
use crate::queue::{Batch, BatchStatus};
use crate::state::{
    apply_library_dirs, library_files, now_secs, paths, spawn_forever, AppState, ChaptersState,
    DownloadDialog, ExportOpts, GroupingChoice, JobKind, JobView, LoadedChapters, Page,
    PendingDownload, PlanChapters, PlanEntry, PreviewPage, PreviewState, SeriesTab, Shot,
    TableView, Workspace,
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
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
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
    let Some((title, sources)) = state
        .library
        .peek()
        .find(series_id)
        .map(|s| (s.title.clone(), s.sources.clone()))
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
    state.go(Page::Series);
    chapters.set(ChaptersState::Loading);
    let id = series_id.to_string();
    spawn_forever(async move {
        // Series downloaded before sources were kept find theirs again from
        // their downloaded chapters.
        let sources = if sources.is_empty() {
            backfill_sources(state, &id).await
        } else {
            sources
        };
        let result = if sources.is_empty() {
            Some(Err("none".to_string()))
        } else {
            let members = members_from_saved(&sources, &ws_title(state, &id));
            load_chapters(state, members).await.map(Ok)
        };
        let still_here = state
            .workspace
            .peek()
            .as_ref()
            .is_some_and(|w| w.series_id == id);
        if still_here {
            let mut chapters = state.chapters;
            chapters.set(match result {
                Some(Ok(loaded)) => ChaptersState::Ready(Rc::new(loaded)),
                Some(Err(_)) => ChaptersState::Idle,
                None => ChaptersState::Failed("The catalog engine is not running.".into()),
            });
        }
    });
}

/// Find where a library series lives on the sources again, by asking about the
/// chapters downloaded from each one. They are kept with the series after that.
async fn backfill_sources(state: AppState, series_id: &str) -> Vec<library::SavedSource> {
    let Some(client) = state.client() else {
        return Vec::new();
    };
    let Some(series) = state.library.peek().find(series_id).cloned() else {
        return Vec::new();
    };
    let sources: HashMap<String, (String, String)> = client
        .list_sources()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.id, (s.name, s.lang)))
        .collect();
    let mut found: Vec<library::SavedSource> = Vec::new();
    for chapter in &series.chapters {
        if found.iter().any(|f| f.source_id == chapter.source_id) {
            continue;
        }
        let from_source = series
            .chapters
            .iter()
            .filter(|c| c.source_id == chapter.source_id)
            .take(3);
        for c in from_source {
            if let Ok(details) = client.manga_of_chapter(c.chapter_id).await {
                let (name, lang) = sources.get(&c.source_id).cloned().unwrap_or_default();
                found.push(library::SavedSource {
                    source_id: c.source_id.clone(),
                    source_name: name,
                    lang,
                    manga_id: details.id,
                    thumbnail: details.thumbnail_url,
                });
                break;
            }
        }
    }
    if !found.is_empty() {
        let mut library = state.library;
        library.write().add_sources(series_id, found.clone());
        state.save_library();
        info!(
            "Found the sources of \"{}\" again from its downloaded chapters",
            series.title
        );
    }
    found
}

fn ws_title(state: AppState, series_id: &str) -> String {
    state
        .library
        .peek()
        .find(series_id)
        .map(|s| s.title.clone())
        .unwrap_or_default()
}

/// A library series' saved sources, as search results again.
fn members_from_saved(sources: &[library::SavedSource], title: &str) -> Vec<ScoredMember> {
    sources
        .iter()
        .map(|s| ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: s.source_id.clone(),
                    name: s.source_name.clone(),
                    lang: s.lang.clone(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: s.manga_id,
                    title: title.to_string(),
                    thumbnail_url: s.thumbnail.clone(),
                },
            ),
            score: 0.5,
            enrichment: None,
        })
        .collect()
}

/// What the library keeps of a series' sources: the best entry from each.
pub fn saved_sources_of(members: &[ScoredMember]) -> Vec<library::SavedSource> {
    logic::one_per_source(members, 8)
        .iter()
        .map(|m| library::SavedSource {
            source_id: m.candidate.source_id.clone(),
            source_name: m.candidate.source_name.clone(),
            lang: m.candidate.lang.clone(),
            manga_id: m.candidate.manga_id,
            thumbnail: m.candidate.thumbnail.clone(),
        })
        .collect()
}

/// Put a series in the library without downloading anything.
pub fn add_to_library(state: AppState, title: &str, sources: Vec<library::SavedSource>) {
    let (defaults, preset) = state.new_series_defaults();
    let already = state
        .library
        .peek()
        .find(&library::series_id(title))
        .is_some();
    let id = {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_saved(title, sources, &defaults, now_secs());
        db.adopt_default_preset(&id, &defaults, &preset);
        id
    };
    state.save_library();
    ensure_series_info(state, &id);
    if already {
        state.notify(format!("{title} is already in your library."));
    } else {
        state.notify(format!("Added {title} to your library."));
    }
}

/// Add a search result to the library.
pub fn add_ranked_to_library(state: AppState, ranked: &RankedSeries) {
    add_to_library(state, &ranked.title, saved_sources_of(&ranked.members));
}

/// Add the open series to the library.
pub fn add_open_series_to_library(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let sources = match &*state.chapters.peek() {
        ChaptersState::Ready(loaded) => loaded.saved.clone(),
        _ => ws
            .ranked
            .as_ref()
            .map(|r| saved_sources_of(&r.members))
            .unwrap_or_default(),
    };
    add_to_library(state, &ws.title, sources);
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
        // A series saved without downloading anything is looked up by where it was found.
        if details.is_none() {
            for source in series.sources.iter().take(4) {
                if let Ok(found) = client.manga_details(source.manga_id).await {
                    details = Some(found);
                    break;
                }
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
            let mut source_names: Vec<(String, String)> = series
                .sources
                .iter()
                .map(|s| (s.source_id.clone(), s.source_name.clone()))
                .collect();
            for chapter in &series.chapters {
                if let Some(name) = names.get(&chapter.source_id) {
                    if !source_names.iter().any(|(id, _)| *id == chapter.source_id) {
                        source_names.push((chapter.source_id.clone(), name.clone()));
                    }
                }
            }
            let mut has_cover = false;
            let cover_path = details
                .thumbnail_url
                .clone()
                .or_else(|| series.sources.iter().find_map(|s| s.thumbnail.clone()));
            if let Some(path) = cover_path.as_deref() {
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
        saved: members
            .iter()
            .map(|m| library::SavedSource {
                source_id: m.candidate.source_id.clone(),
                source_name: m.candidate.source_name.clone(),
                lang: m.candidate.lang.clone(),
                manga_id: m.candidate.manga_id,
                thumbnail: m.candidate.thumbnail.clone(),
            })
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

/// Is an export running? (Downloads have their own queue.)
fn job_running(state: AppState) -> bool {
    state
        .job
        .peek()
        .as_ref()
        .is_some_and(|j| j.result.is_none())
}

/// What queueing some chapters did.
struct Enqueued {
    /// Chapters newly in the queue.
    chapters: usize,
    /// Chapters the one chosen source does not have.
    not_offered: usize,
}

/// Put the picked chapters (positions in `loaded.merged`) that are not
/// downloaded yet into the download queue, and the series in the library.
fn enqueue_picked(
    state: AppState,
    series_id: &str,
    title: &str,
    loaded: &LoadedChapters,
    mut picked: Vec<usize>,
    choice: SourceChoice,
) -> Result<Enqueued, String> {
    let have = downloaded_keys(state, series_id);
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
    let source_names: HashMap<String, String> = loaded
        .member_sources
        .iter()
        .cloned()
        .zip(loaded.member_names.iter().cloned())
        .collect();
    let mut not_offered = 0;
    if let SourceChoice::Only(source) = &choice {
        let chosen = pipeline::apply_choice(std::mem::take(&mut requests), &choice, &[]);
        not_offered = chosen.unavailable.len();
        plan = chosen.kept.iter().map(|&i| plan[i].clone()).collect();
        requests = chosen.requests;
        if requests.is_empty() {
            let name = source_names.get(source).cloned().unwrap_or_default();
            return Err(format!(
                "{name} does not have the chapters you picked. Choose Adaptive, or another source."
            ));
        }
    }
    if requests.is_empty() {
        return Ok(Enqueued {
            chapters: 0,
            not_offered,
        });
    }
    // The series is in the library from now on, with where to find it again.
    add_to_library_quietly(state, title, &loaded.saved);
    let batch = Batch::new(
        series_id.to_string(),
        title.to_string(),
        requests,
        plan,
        choice,
        source_names,
        now_secs(),
    );
    let added = state.edit_queue(|q| q.add(batch));
    info!(
        "Queued {} of \"{title}\"",
        logic::plural(added.chapters, "chapter")
    );
    Ok(Enqueued {
        chapters: added.chapters,
        not_offered,
    })
}

/// Start a download, after asking where its files should go (unless the user
/// asked not to be asked).
pub fn ask_download(state: AppState, pending: PendingDownload, summary: String) {
    if !state.settings.peek().ask_download_folder {
        start_pending(state, pending);
        return;
    }
    let mut dialog = state.download_dialog;
    dialog.set(Some(DownloadDialog { pending, summary }));
}

/// Begin what the dialog was asked about.
pub fn start_pending(state: AppState, pending: PendingDownload) {
    match pending {
        PendingDownload::Chapters => queue_download(state),
        PendingDownload::Plan => commit_plan(state),
    }
}

/// Keep new downloads in this folder. What was downloaded before stays where
/// it is, and is still found there.
pub fn set_download_folder(state: AppState, dir: std::path::PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        state.error(format!("Could not use that folder: {e}"));
        return;
    }
    let mut settings = state.settings;
    {
        let mut st = settings.write();
        let app_folder = paths().library_dir();
        let old = st.library_dir.clone().unwrap_or_else(|| app_folder.clone());
        if old != dir && !st.library_dirs.contains(&old) {
            st.library_dirs.push(old);
        }
        st.library_dir = (dir != app_folder).then_some(dir);
    }
    apply_library_dirs(&settings.peek());
    state.save_settings();
}

/// Add the ticked chapters that are not downloaded yet to the download queue.
/// They download in turn, so there is nothing to wait for or start by hand:
/// keep picking chapters, here or in other series.
pub fn queue_download(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let ChaptersState::Ready(loaded) = state.chapters.peek().clone() else {
        state.error("The chapter list is still loading.");
        return;
    };
    let picked: Vec<usize> = state.selected.peek().iter().copied().collect();
    let choice = state.download_source.peek().clone();
    match enqueue_picked(state, &ws.series_id, &ws.title, &loaded, picked, choice) {
        Err(message) => state.error(message),
        Ok(done) => {
            let mut selected = state.selected;
            selected.set(HashSet::new());
            if done.chapters == 0 {
                state.notify("Those chapters are already downloaded or in the queue.");
                return;
            }
            let skipped = if done.not_offered > 0 {
                format!(", {} not offered by that source", done.not_offered)
            } else {
                String::new()
            };
            state.notify(format!(
                "Added {} of {} to the downloads{skipped}.",
                logic::plural(done.chapters, "chapter"),
                ws.title
            ));
            ensure_queue_running(state);
        }
    }
}

/// Open the download planner for these library series.
pub fn open_plan(state: AppState, series_ids: Vec<String>) {
    let entries: Vec<PlanEntry> = series_ids
        .iter()
        .filter_map(|id| state.library.peek().find(id).cloned())
        .map(|s| PlanEntry {
            series_id: s.id,
            title: s.title,
            chapters: PlanChapters::Loading,
            picked: HashSet::new(),
            open: series_ids.len() == 1,
            filter: String::new(),
            sort: (logic::SortKey::Number, true),
        })
        .collect();
    let ids: Vec<String> = entries.iter().map(|e| e.series_id.clone()).collect();
    let mut plan = state.plan;
    plan.set(entries);
    state.go(Page::Plan);
    for id in ids {
        load_plan_entry(state, id);
    }
}

fn load_plan_entry(state: AppState, id: String) {
    spawn_forever(async move {
        let mut sources = state
            .library
            .peek()
            .find(&id)
            .map(|s| s.sources.clone())
            .unwrap_or_default();
        if sources.is_empty() {
            sources = backfill_sources(state, &id).await;
        }
        let title = ws_title(state, &id);
        let loaded = if sources.is_empty() {
            Err("No source for this series could be found. Search for it once and add it to your library again.".to_string())
        } else {
            let members = members_from_saved(&sources, &title);
            load_chapters(state, members)
                .await
                .ok_or_else(|| "The catalog engine is not running.".to_string())
        };
        let have = downloaded_keys(state, &id);
        let queue = state.queue.peek().clone();
        let mut plan = state.plan;
        let mut entries = plan.write();
        let Some(entry) = entries.iter_mut().find(|e| e.series_id == id) else {
            return;
        };
        match loaded {
            Ok(loaded) => {
                // A series that already has chapters starts with the missing
                // ones ticked; a new one waits for the user to choose.
                if !have.is_empty() {
                    entry.picked = loaded
                        .merged
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| {
                            let key = merged_key(c);
                            !have.contains(&key) && queue.chapter_state(&id, &key).is_none()
                        })
                        .map(|(i, _)| i)
                        .collect();
                }
                entry.chapters = PlanChapters::Ready(Rc::new(loaded));
            }
            Err(message) => entry.chapters = PlanChapters::Failed(message),
        }
    });
}

/// Queue everything ticked in the planner, and show the downloads.
pub fn commit_plan(state: AppState) {
    let entries = state.plan.peek().clone();
    let (mut chapters, mut series, mut not_offered) = (0, 0, 0);
    for entry in entries {
        let PlanChapters::Ready(loaded) = &entry.chapters else {
            continue;
        };
        if entry.picked.is_empty() {
            continue;
        }
        let picked: Vec<usize> = entry.picked.iter().copied().collect();
        match enqueue_picked(
            state,
            &entry.series_id,
            &entry.title,
            loaded,
            picked,
            SourceChoice::Adaptive,
        ) {
            Ok(done) if done.chapters > 0 => {
                chapters += done.chapters;
                not_offered += done.not_offered;
                series += 1;
            }
            Ok(_) => {}
            Err(message) => state.error(message),
        }
    }
    let mut plan = state.plan;
    plan.set(Vec::new());
    if chapters == 0 {
        state.notify("Nothing new to download.");
        state.go(Page::Library);
        return;
    }
    let skipped = if not_offered > 0 {
        format!(" ({not_offered} not offered by that source)")
    } else {
        String::new()
    };
    state.notify(format!(
        "Added {} from {series} series to the downloads{skipped}.",
        logic::plural(chapters, "chapter"),
    ));
    state.go(Page::Downloads);
    ensure_queue_running(state);
}

/// Put a series in the library as a download is queued, without a message.
fn add_to_library_quietly(state: AppState, title: &str, sources: &[library::SavedSource]) {
    let (defaults, preset) = state.new_series_defaults();
    let id = {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_saved(title, sources.to_vec(), &defaults, now_secs());
        db.adopt_default_preset(&id, &defaults, &preset);
        id
    };
    state.save_library();
    ensure_series_info(state, &id);
}

/// Start working through the download queue, unless that is already going on.
pub fn ensure_queue_running(state: AppState) {
    {
        let queue = state.queue.peek();
        if queue.runner_active || queue.next_to_start().is_none() {
            return;
        }
    }
    state.edit_queue(|q| q.runner_active = true);
    spawn_forever(async move {
        loop {
            let next = state.queue.peek().next_to_start();
            let Some(id) = next else { break };
            run_batch(state, id).await;
        }
        state.edit_queue(|q| q.runner_active = false);
        // Something may have been added just as the last check was made.
        ensure_queue_running(state);
    });
}

/// Download one batch of the queue.
async fn run_batch(state: AppState, id: u64) {
    let Some(batch) = state.queue.peek().get(id).cloned() else {
        return;
    };
    state.edit_queue(|q| q.start(id));
    let fail = |status: BatchStatus, summary: String, failed: Vec<String>| {
        state.edit_queue(|q| q.finish(id, status, summary, Vec::new(), failed));
    };
    let Some(client) = state.client() else {
        fail(
            BatchStatus::Failed("The catalog engine is not running.".into()),
            "The catalog engine is not running.".into(),
            batch.plan.clone(),
        );
        return;
    };
    let started = Instant::now();
    let files = library_files();
    let (defaults, default_preset) = state.new_series_defaults();
    let shrink = state.settings.peek().shrink_downloads;
    let policy = RetryPolicy::default();
    let limits = state.settings.peek().download.limits();
    info!(
        "Download started: \"{}\", {}{}",
        batch.title,
        logic::plural(batch.requests.len(), "chapter"),
        if shrink { " (saving space)" } else { "" }
    );
    // Adaptive: test the sources that cover about the same, then put the best
    // first for every chapter (the others stay as fallbacks).
    let requests = if batch.choice == SourceChoice::Adaptive {
        order_sources(
            state,
            id,
            &client,
            batch.requests.clone(),
            &batch.source_names,
        )
        .await
    } else {
        batch.requests.clone()
    };
    let (tx, mut rx) = unbounded_channel();
    let run = download_chapters(
        &*client,
        &files,
        &batch.series_id,
        &requests,
        shrink,
        limits,
        &policy,
        Some(tx),
        &batch.cancel,
    );
    let watch = async {
        let mut logged = HashMap::new();
        while let Some(first) = rx.recv().await {
            // Take everything that has arrived and show it in one go:
            // redrawing for every single page would keep the screen busy
            // far more than anyone can see.
            let mut events = vec![first];
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            for event in &events {
                log_event(event, &mut logged);
                if let JobEvent::ChapterSaved { chapter } = event {
                    // Record each chapter as soon as it is safe on disk, so
                    // cancelling or failing later never loses it.
                    let mut library = state.library;
                    {
                        let mut db = library.write();
                        let series =
                            db.record_chapter(&batch.title, &defaults, chapter.clone(), now_secs());
                        db.adopt_default_preset(&series, &defaults, &default_preset);
                    }
                    state.save_library();
                }
            }
            state.edit_queue(|q| {
                if let Some(b) = q.get_mut(id) {
                    for event in &events {
                        b.progress.apply(event);
                    }
                }
            });
            tokio::time::sleep(PROGRESS_REFRESH).await;
        }
    };
    let (result, ()) = join(run, watch).await;

    // Chapters of the batch that did not end up saved.
    let unsaved = |state: AppState| -> Vec<String> {
        let have = downloaded_keys(state, &batch.series_id);
        batch
            .plan
            .iter()
            .filter(|k| !have.contains(*k))
            .cloned()
            .collect()
    };
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
                "Downloaded {}, {}{}",
                logic::plural(outcome.chapters.len(), "chapter"),
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
                .filter_map(|s| batch.plan.get(s.index).cloned())
                .collect();
            state.edit_queue(|q| q.finish(id, BatchStatus::Done, summary.clone(), skipped, failed));
            state.notify(format!("{}: {}", batch.title, summary.to_lowercase()));
        }
        Err(e) => {
            let cancelled = matches!(e, PipelineError::Cancelled);
            let message = if cancelled {
                "Cancelled. Chapters that finished are kept.".to_string()
            } else {
                e.to_string()
            };
            if cancelled {
                info!("Download of \"{}\" cancelled", batch.title);
            } else {
                warn!("Download of \"{}\" failed: {message}", batch.title);
            }
            let status = if cancelled {
                BatchStatus::Cancelled
            } else {
                BatchStatus::Failed(message.clone())
            };
            fail(status, message, unsaved(state));
        }
    }
    ensure_series_info(state, &batch.series_id);
}

/// Stop one batch of the queue (a waiting one is simply dropped).
pub fn cancel_batch(state: AppState, id: u64) {
    state.edit_queue(|q| {
        q.cancel(id);
    });
}

/// Stop everything in the queue.
pub fn cancel_all_downloads(state: AppState) {
    state.edit_queue(|q| q.cancel_all());
}

/// Pause the queue (the batch running carries on) or let it go on.
pub fn set_queue_paused(state: AppState, paused: bool) {
    state.edit_queue(|q| q.paused = paused);
    if !paused {
        ensure_queue_running(state);
    }
}

/// Try the chapters of a finished batch that were not downloaded once more.
pub fn retry_batch(state: AppState, id: u64) {
    let queued = state.edit_queue(|q| q.retry(id, now_secs()));
    if queued > 0 {
        state.notify(format!(
            "Trying {} again.",
            logic::plural(queued, "chapter")
        ));
        ensure_queue_running(state);
    }
}

/// Move a waiting batch to the front of the line.
pub fn prioritize_batch(state: AppState, id: u64) {
    state.edit_queue(|q| q.prioritize(id));
}

/// Rank the sources for these chapters, speed-testing the ones that matter
/// and have not been tested lately, and put the best first for every chapter.
async fn order_sources(
    state: AppState,
    batch_id: u64,
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
        state.edit_queue(|q| {
            if let Some(b) = q.get_mut(batch_id) {
                b.progress.note = Some("Checking which source is fastest…".into());
            }
        });
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
        let device = state.settings.peek().device_profile();
        let opts = pipeline::process_options_for(&device, &draft);

        // The page to try: from the library, or drawn from scratch. A few
        // pages from there on are planned together, so a webtoon preset shows
        // its stitched pages the way the export will make them.
        let mut total_pages = 0;
        let mut page_index = 0;
        let mut files = Vec::new();
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
                    files = pages[page_index..]
                        .iter()
                        .take(LAB_WINDOW)
                        .cloned()
                        .collect();
                }
            }
        }
        let rendered = tokio::task::spawn_blocking(move || {
            let grays = if files.is_empty() {
                vec![pipeline::sample_page()]
            } else {
                files
                    .iter()
                    .map(|path| {
                        std::fs::read(path)
                            .map_err(|e| format!("Could not read the page: {e}"))
                            .and_then(|bytes| {
                                pipeline::decode_page(&bytes).map_err(|e| e.to_string())
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            let chapter = pipeline::prepare_chapter(&grays, &opts);
            let converted: Vec<PreviewImage> = chapter
                .plan
                .pages
                .iter()
                .take(3)
                .map(|page| pipeline::render_planned(&chapter.prepared, page, &opts))
                .collect();
            let original = pipeline::render_original(&grays[0], &opts);
            Ok::<_, String>((converted, original))
        })
        .await;
        if !current(state) {
            return;
        }
        let mut lab = state.preset_lab;
        let mut l = lab.write();
        l.busy = false;
        match rendered {
            Ok(Ok((converted, original))) => {
                l.converted = converted.iter().map(shot).collect();
                l.original = Some(shot(&original.image));
                l.kept = Some(original.kept);
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
        valid && on_disk > 0 && !p.pages.is_empty() && p.fresh == p.pages.len() && !p.busy
    };
    if !up_to_date {
        schedule_preview(state, 0);
    }
}

/// The decoded images of the chapter last previewed, so tuning a slider does
/// not read and decode every page again.
type DecodedChapter = (String, Arc<Vec<converter::GrayImage>>);
static DECODED: Mutex<Option<DecodedChapter>> = Mutex::new(None);
static PREPARED_PREVIEW: Mutex<
    Option<(String, converter::ProcessOptions, pipeline::ChapterPreview)>,
> = Mutex::new(None);
static PREVIEW_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_preview_request() -> u64 {
    PREVIEW_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn shape_options(opts: &converter::ProcessOptions) -> converter::ProcessOptions {
    let mut key = opts.clone();
    key.contrast = 50;
    key.auto_levels = false;
    key.dither = converter::Dither::Threshold;
    key.depth = converter::Depth::One;
    key
}

/// Most pages drawn full size at once while scrolling.
const MAX_FULL_PAGES: usize = 10;

/// The pages to draw full size: the selected one, then those on or near the
/// screen, nearest first.
pub fn full_size_pages(total: usize, selected: usize, window: (usize, usize)) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let selected = selected.min(total - 1);
    let mut pages: Vec<usize> = (window.0.min(total - 1)..=window.1.min(total - 1)).collect();
    pages.sort_by_key(|&i| i.abs_diff(selected));
    pages.truncate(MAX_FULL_PAGES);
    if !pages.contains(&selected) {
        pages.insert(0, selected);
    }
    pages
}

pub fn preview_render_order(
    total: usize,
    selected: usize,
    thumb_start: usize,
    overview: bool,
    window: (usize, usize),
) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let selected = selected.min(total - 1);
    let mut order = vec![selected];
    let range = if overview {
        0..total
    } else {
        thumb_start.min(total)..(thumb_start + 8).min(total)
    };
    for i in range {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    for i in selected.saturating_sub(1)..(selected + 2).min(total) {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    for i in full_size_pages(total, selected, window) {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    order
}

pub fn select_preview_page(state: AppState, selected: usize) {
    let mut preview = state.preview;
    let mut p = preview.write();
    if p.pages.is_empty() {
        return;
    }
    let next = selected.min(p.pages.len() - 1);
    if next != p.selected {
        p.draft_crop = None;
    }
    p.selected = next;
    p.overview = false;
    drop(p);
    schedule_preview(state, 0);
    spawn(async move {
        let js = format!(
            "document.querySelector('[data-editor-page=\"{next}\"]')?.scrollIntoView({{block:'nearest'}});\
             document.querySelector('.editor-row[data-row=\"{next}\"]')?.scrollIntoView({{block:'start'}})"
        );
        let _ = document::eval(&js).await;
    });
}

/// Read and decode a chapter's saved pages, in order.
async fn decode_chapter(
    cache_key: String,
    files: Vec<std::path::PathBuf>,
) -> Result<(String, Arc<Vec<converter::GrayImage>>), String> {
    let raw = tokio::task::spawn_blocking(move || {
        files
            .into_iter()
            .map(|path| {
                std::fs::read(path).map_err(|e| format!("Could not read a saved page: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut digest = Sha256::new();
    for bytes in &raw {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    let cache_key = format!("{cache_key}/{:x}", digest.finalize());
    if let Some((k, grays)) = DECODED.lock().ok().and_then(|g| g.clone()) {
        if k == cache_key {
            return Ok((cache_key, grays));
        }
    }
    let decoded: Vec<Result<converter::GrayImage, String>> = stream::iter(raw.into_iter())
        .map(|bytes| async move {
            tokio::task::spawn_blocking(move || {
                pipeline::decode_page(&bytes).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        })
        .buffered(PREVIEW_THREADS)
        .collect()
        .await;
    let grays = Arc::new(decoded.into_iter().collect::<Result<Vec<_>, _>>()?);
    if let Ok(mut g) = DECODED.lock() {
        *g = Some((cache_key.clone(), grays.clone()));
    }
    Ok((cache_key, grays))
}

/// Plan the chapter with the series' settings, then draw each page a few at a
/// time in the background. The plan appears first, so the screen takes its
/// shape at once. Settings changes call this with a short delay, so dragging a
/// slider draws once it settles.
pub fn schedule_preview(state: AppState, delay_ms: u64) {
    let mut preview = state.preview;
    let generation = {
        let mut p = preview.write();
        p.generation = next_preview_request();
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
        let device = state.settings.peek().device_profile();
        let files = library_files().page_files(&ws.series_id, &key);
        if files.is_empty() {
            return fail("This chapter has no saved pages. Download it again.".into());
        }
        let image_count = files.len();
        let (cache_key, grays) =
            match decode_chapter(format!("{}/{key}/{image_count}", ws.series_id), files).await {
                Ok(g) => g,
                Err(message) => return fail(message),
            };
        if !current(state) {
            return;
        }

        // The same planning and conversion the export runs.
        let opts = pipeline::process_options_for(&device, &series.settings);
        let shape_key = shape_options(&opts);
        let cached = PREPARED_PREVIEW.lock().ok().and_then(|g| {
            g.as_ref()
                .filter(|(k, o, _)| k == &cache_key && o == &shape_key)
                .map(|(_, _, p)| p.clone())
        });
        let chapter = if let Some(cached) = cached {
            cached
        } else {
            let (prepare_opts, grays) = (opts.clone(), grays.clone());
            match tokio::task::spawn_blocking(move || {
                pipeline::prepare_chapter(&grays, &prepare_opts)
            })
            .await
            {
                Ok(c) => {
                    if let Ok(mut cache) = PREPARED_PREVIEW.lock() {
                        *cache = Some((cache_key, shape_key, c.clone()));
                    }
                    c
                }
                Err(e) => return fail(e.to_string()),
            }
        };
        if !current(state) {
            return;
        }
        {
            let mut p = state.preview;
            let mut w = p.write();
            if w.generation != generation {
                return;
            }
            let old = std::mem::take(&mut w.pages);
            w.strip = chapter.plan.strip;
            w.images = image_count;
            let near = (
                w.window.0.saturating_sub(KEEP_DRAWN_AROUND),
                w.window.1 + KEEP_DRAWN_AROUND,
            );
            w.pages = chapter
                .plan
                .pages
                .iter()
                .enumerate()
                .map(|(position, page)| {
                    // Keep the previous drawing of this page until it is
                    // redrawn, but only near the screen: the full-size
                    // drawings of a long chapter would fill the memory.
                    let (shot, thumbnail, original, kept) = old
                        .iter()
                        .find(|o| o.crop_key == pipeline::crop_page_key(page))
                        .map_or((None, None, None, None), |o| {
                            let close = (near.0..=near.1).contains(&position);
                            (
                                o.shot.clone().filter(|_| close),
                                o.thumbnail.clone(),
                                o.original.clone().filter(|_| close),
                                o.kept,
                            )
                        });
                    let image = match page.source {
                        converter::Source::Image { index, .. } => Some(index),
                        converter::Source::Strip { .. } => None,
                    };
                    Rc::new(PreviewPage {
                        shot,
                        thumbnail,
                        original,
                        kept,
                        image,
                        crop_key: pipeline::crop_page_key(page),
                        source: Some(page.clone()),
                        generation: 0,
                    })
                })
                .collect();
            w.selected = w.selected.min(w.pages.len().saturating_sub(1));
        }

        let prepared = chapter.prepared.clone();
        let (selected, thumb_start, overview, draft, window, want_source) = {
            let p = state.preview.peek();
            (
                p.selected,
                p.thumb_start,
                p.overview,
                p.draft_crop.clone(),
                p.window,
                p.view.show_original,
            )
        };
        let full: std::collections::HashSet<usize> =
            full_size_pages(chapter.plan.pages.len(), selected, window)
                .into_iter()
                .collect();
        let jobs: Vec<_> = preview_render_order(
            chapter.plan.pages.len(),
            selected,
            thumb_start,
            overview,
            window,
        )
        .into_iter()
        .map(|i| (i, chapter.plan.pages[i].clone()))
        .collect();
        let crops = series.crops.clone();
        let chapter_key = key.clone();
        let mut drawn = stream::iter(jobs)
            .map(|(index, page)| {
                let (opts, prepared, crops, chapter_key, draft) = (
                    opts.clone(),
                    prepared.clone(),
                    crops.clone(),
                    chapter_key.clone(),
                    draft.clone(),
                );
                let (is_full, is_selected) = (full.contains(&index), index == selected);
                async move {
                    let result = tokio::task::spawn_blocking(move || {
                        let key = pipeline::crop_page_key(&page);
                        let crop = draft
                            .as_ref()
                            .filter(|(k, _)| k == &key)
                            .map(|(_, crop)| *crop)
                            .or_else(|| crops.effective(&chapter_key, &key));
                        let levels = converter::plan::render_page_cropped(
                            &prepared,
                            &page,
                            &opts,
                            crop.unwrap_or_default().as_array(),
                        );
                        let thumbnail = pipeline::levels_to_thumbnail(&levels);
                        let converted =
                            is_full.then(|| pipeline::levels_to_previews(&[levels]).remove(0));
                        let original = (is_full && (want_source || is_selected))
                            .then(|| pipeline::render_source_preview(&prepared, &page, &opts));
                        let samples = if is_selected {
                            let source = converter::plan::page_source(&prepared, &page, &opts);
                            [
                                converter::Dither::FloydSteinberg,
                                converter::Dither::Atkinson,
                                converter::Dither::Ordered,
                                converter::Dither::Threshold,
                            ]
                            .into_iter()
                            .map(|dither| {
                                let mut sample_opts = opts.clone();
                                sample_opts.width = 96;
                                sample_opts.height = 160;
                                sample_opts.layout = converter::process::Layout::Manga;
                                sample_opts.crop = converter::process::CropMode::Full;
                                sample_opts.margins = converter::process::Margins::default();
                                sample_opts.spread = converter::process::SpreadMode::Fit;
                                sample_opts.dither = dither;
                                let levels =
                                    converter::process::process_page(&source, &sample_opts);
                                pipeline::levels_to_previews(&levels)
                                    .first()
                                    .map(|image| shot(image).url)
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        (converted, thumbnail, original, samples)
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
                Ok((converted, thumbnail, original, samples)) => {
                    let mut p = state.preview;
                    let mut w = p.write();
                    if let Some(slot) = w.pages.get_mut(index) {
                        let mut next = (**slot).clone();
                        if let Some(converted) = converted {
                            next.shot = Some(shot(&converted));
                        }
                        next.thumbnail = Some(shot(&thumbnail));
                        if let Some(original) = original {
                            next.original = Some(shot(&original));
                        }
                        next.generation = generation;
                        *slot = Rc::new(next);
                    }
                    if index == selected {
                        w.shading_samples = samples;
                    }
                    w.fresh += 1;
                }
                Err(e) => {
                    first_error.get_or_insert(format!("Page {}: {e}", index + 1));
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
const PREVIEW_THREADS: usize = 4;
/// Pages either side of the screen whose full-size drawings are kept.
const KEEP_DRAWN_AROUND: usize = 12;

/// Pages from the chosen one on that the Presets test page plans together.
const LAB_WINDOW: usize = 6;

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
        settings.device_profile(),
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
        progress: JobProgress::default(),
        result: None,
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

    spawn_forever(run_export(state, series, export, target, cancel));
}
/// Run one export, keeping the job bar up to date, and record the result.
async fn run_export(
    state: AppState,
    series: library::DownloadedSeries,
    export: ExportJob,
    target: ExportTarget,
    cancel: CancelFlag,
) {
    let mut job = state.job;
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
}

/// Export every downloaded chapter of these series, one file each, to the
/// export folder from Settings, one series after another.
pub fn export_series_many(state: AppState, series_ids: Vec<String>) {
    if job_running(state) {
        state.error("Something is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let settings = state.settings.peek().clone();
    let list: Vec<library::DownloadedSeries> = series_ids
        .iter()
        .filter_map(|id| state.library.peek().find(id).cloned())
        .filter(|s| !s.chapters.is_empty())
        .collect();
    if list.is_empty() {
        state.error("Nothing is downloaded for those series yet.");
        return;
    }
    let target = match logic::export_target(
        &crate::logic::TargetChoice::Folder,
        &settings,
        &paths().default_export_dir(),
    ) {
        Ok(t) => t,
        Err(message) => {
            state.error(message);
            return;
        }
    };
    spawn_forever(async move {
        let total = list.len();
        for (n, series) in list.into_iter().enumerate() {
            let cancel = CancelFlag::new();
            let keys: Vec<String> = series.chapters.iter().map(|c| c.key.clone()).collect();
            let mut export = ExportJob::new(
                series.clone(),
                keys,
                settings.device_profile(),
                target.clone(),
                paths().spool_dir(),
            );
            export.language = settings.language.clone();
            export.grouping = Grouping::Single;
            let mut job = state.job;
            job.set(Some(JobView {
                kind: JobKind::Export,
                title: format!("{} ({} of {total})", series.title, n + 1),
                progress: JobProgress::default(),
                result: None,
                cancel: cancel.clone(),
            }));
            info!(
                "Export started: \"{}\" ({} of {total})",
                series.title,
                n + 1
            );
            run_export(state, series, export, target.clone(), cancel).await;
            let failed = matches!(
                job.peek().as_ref().and_then(|j| j.result.as_ref()),
                Some(Err(_))
            );
            if failed {
                return;
            }
        }
        let mut job = state.job;
        let mut guard = job.write();
        if let Some(view) = guard.as_mut() {
            view.result = Some(Ok(format!("Exported {total} series")));
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
pub fn remove_from_library(state: AppState, series_ids: Vec<String>) {
    // Stop anything downloading for them, and give that a moment to let go.
    for id in &series_ids {
        state.edit_queue(|q| q.cancel_series(id));
    }
    spawn_forever(async move {
        for _ in 0..40 {
            if !series_ids
                .iter()
                .any(|id| state.queue.peek().series_has_work(id))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut removed = 0;
        for series_id in series_ids {
            let files = library_files();
            let id = series_id.clone();
            match tokio::task::spawn_blocking(move || files.remove_series(&id)).await {
                Ok(Ok(())) => {
                    let mut library = state.library;
                    let title = library.write().remove_series(&series_id).map(|s| s.title);
                    state.save_library();
                    removed += 1;
                    info!(
                        "Removed {} from the library",
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
        }
        if removed > 0 {
            state.notify(format!(
                "Removed {} from your library.",
                logic::plural(removed, "series")
            ));
        }
    });
}

/// Delete the downloaded pages of these chapters. Returns how many went.
async fn delete_chapter_files(state: AppState, series_id: &str, keys: Vec<String>) -> usize {
    let mut deleted = 0;
    for key in keys {
        let files = library_files();
        let (id, k) = (series_id.to_string(), key.clone());
        match tokio::task::spawn_blocking(move || files.remove_chapter(&id, &k)).await {
            Ok(Ok(())) => {
                let mut library = state.library;
                library.write().remove_chapter(series_id, &key);
                deleted += 1;
            }
            Ok(Err(e)) => state.error(format!("Could not delete that chapter: {e}")),
            Err(e) => state.error(format!("Could not delete that chapter: {e}")),
        }
    }
    state.save_library();
    deleted
}

/// Delete some downloaded chapters of a series; the series stays in the library.
pub fn delete_chapters(state: AppState, series_id: String, keys: Vec<String>) {
    spawn_forever(async move {
        let deleted = delete_chapter_files(state, &series_id, keys).await;
        if deleted == 0 {
            return;
        }
        state.notify(format!("Deleted {}.", logic::plural(deleted, "chapter")));
        // With nothing left to preview or export, go back to the overview.
        let empty = state
            .library
            .peek()
            .find(&series_id)
            .is_none_or(|s| s.chapters.is_empty());
        let open_here = state
            .workspace
            .peek()
            .as_ref()
            .is_some_and(|w| w.series_id == series_id);
        if open_here && *state.page.peek() == Page::Series {
            let tab = state.workspace.peek().as_ref().map(|w| w.tab);
            if empty && matches!(tab, Some(SeriesTab::Preview | SeriesTab::Export)) {
                state.open_tab(SeriesTab::Overview);
            } else if tab == Some(SeriesTab::Preview) {
                prepare_preview(state);
            }
        }
    });
}

/// Delete every downloaded chapter of these series; they stay in the library.
pub fn delete_downloads(state: AppState, series_ids: Vec<String>) {
    for id in &series_ids {
        state.edit_queue(|q| q.cancel_series(id));
    }
    spawn_forever(async move {
        for _ in 0..40 {
            if !series_ids
                .iter()
                .any(|id| state.queue.peek().series_has_work(id))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let mut total = 0;
        for id in series_ids {
            let keys: Vec<String> = state
                .library
                .peek()
                .find(&id)
                .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
                .unwrap_or_default();
            total += delete_chapter_files(state, &id, keys).await;
        }
        state.notify(format!(
            "Deleted {}. The series stay in your library.",
            logic::plural(total, "downloaded chapter")
        ));
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

#[cfg(test)]
mod page_editor_tests {
    use super::*;

    #[test]
    fn selected_page_precedes_visible_thumbnails_without_rendering_the_whole_chapter() {
        let order = preview_render_order(148, 100, 90, false, (99, 101));
        assert_eq!(order[0], 100);
        for i in 90..98 {
            assert!(order.contains(&i));
        }
        assert!(order.contains(&99));
        assert!(order.contains(&101));
        assert!(order.len() <= 11, "{}", order.len());
        let mut unique = order.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), order.len());
        assert_eq!(
            preview_render_order(0, 10, 0, false, (0, 0)),
            Vec::<usize>::new()
        );
        assert_eq!(preview_render_order(3, 99, 0, true, (0, 0)), [2, 0, 1]);
    }

    #[test]
    fn the_pages_on_screen_are_drawn_full_size_nearest_first_and_not_too_many() {
        assert_eq!(full_size_pages(0, 0, (0, 5)), Vec::<usize>::new());
        // The selected page is always drawn, even off screen.
        assert_eq!(full_size_pages(50, 40, (3, 4))[0], 40);
        // The window is drawn nearest the selected page first.
        assert_eq!(full_size_pages(50, 4, (3, 5)), [4, 3, 5]);
        // A tall window is capped.
        assert_eq!(full_size_pages(50, 10, (0, 40)).len(), MAX_FULL_PAGES);
        // A window past the end is clamped.
        assert_eq!(full_size_pages(5, 4, (3, 99)), [4, 3]);
    }

    #[test]
    fn appearance_changes_reuse_the_prepared_chapter_but_layout_changes_do_not() {
        let opts = converter::ProcessOptions::manga_default(480, 800, converter::Depth::Two);
        let mut changed = opts.clone();
        changed.contrast = 90;
        changed.auto_levels = !opts.auto_levels;
        changed.dither = converter::Dither::Atkinson;
        assert_eq!(shape_options(&opts), shape_options(&changed));
        changed.layout = converter::process::Layout::Webtoon;
        assert_ne!(shape_options(&opts), shape_options(&changed));
    }

    #[test]
    fn preview_tokens_do_not_reset_between_workspaces() {
        let first = next_preview_request();
        let second = next_preview_request();
        assert!(second > first);
    }

    #[tokio::test]
    async fn preview_reloads_replaced_sources_with_the_same_page_count() {
        let root = std::env::temp_dir().join(format!(
            "mangapoint-preview-revision-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("00001.img");
        fn image(value: u8) -> Vec<u8> {
            use image::ImageEncoder;
            let mut bytes = Vec::new();
            image::codecs::png::PngEncoder::new(&mut bytes)
                .write_image(&[value; 16], 4, 4, image::ExtendedColorType::L8)
                .unwrap();
            bytes
        }
        std::fs::write(&path, image(0)).unwrap();
        let (old, first) = decode_chapter("revision-test".into(), vec![path.clone()])
            .await
            .unwrap();
        std::fs::write(&path, image(255)).unwrap();
        let (new, second) = decode_chapter("revision-test".into(), vec![path.clone()])
            .await
            .unwrap();
        assert_ne!(old, new);
        assert_eq!(first[0].pixels, [0; 16]);
        assert_eq!(second[0].pixels, [255; 16]);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(root);
    }
}
