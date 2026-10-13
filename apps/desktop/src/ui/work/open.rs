//! Opening a series: search results and library entries, their chapter lists and covers.

use super::*;

pub(super) fn reset_workspace_state(state: AppState) {
    let (mut selected, mut table, mut preview) = (state.selected, state.table, state.preview);
    selected.set(HashSet::new());
    table.set(TableView::default());
    // A new series starts with an empty preview, but keeps how the user likes to view it.
    let view = preview.peek().view;
    preview.set(PreviewState {
        view,
        ..Default::default()
    });
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
pub(super) async fn backfill_sources(
    state: AppState,
    series_id: &str,
) -> Vec<library::SavedSource> {
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

pub(super) fn ws_title(state: AppState, series_id: &str) -> String {
    state
        .library
        .peek()
        .find(series_id)
        .map(|s| s.title.clone())
        .unwrap_or_default()
}

/// A library series' saved sources, as search results again.
pub(super) fn members_from_saved(
    sources: &[library::SavedSource],
    title: &str,
) -> Vec<ScoredMember> {
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

pub(super) async fn load_chapters(
    state: AppState,
    members: Vec<ScoredMember>,
) -> Option<LoadedChapters> {
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
