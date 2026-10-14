//! Deleting chapters, series and exported files.

use super::*;

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
pub(super) async fn delete_chapter_files(
    state: AppState,
    series_id: &str,
    keys: Vec<String>,
) -> usize {
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

/// Delete every file of one export from disk (the ones already gone are
/// simply forgotten) and take it off the list.
pub fn delete_export(state: AppState, record_id: String) {
    let files: Vec<String> = state
        .exports
        .peek()
        .records
        .iter()
        .find(|r| r.id == record_id)
        .map(|r| r.files.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default();
    let mut failed = 0;
    for path in files {
        let there = !path.starts_with("http") && std::path::Path::new(&path).exists();
        if there {
            if let Err(e) = library::delete_exported_file(std::path::Path::new(&path)) {
                warn!("Could not delete {path}: {e}");
                failed += 1;
                continue;
            }
            info!("Deleted {path}");
        }
        let mut exports = state.exports;
        exports.write().forget_file(&record_id, &path);
    }
    state.save_exports();
    if failed > 0 {
        state.error(format!("{failed} file(s) could not be deleted."));
    }
}

/// Forget an exported file that is no longer on disk.
pub fn forget_exported_file(state: AppState, record_id: String, path: String) {
    let mut exports = state.exports;
    exports.write().forget_file(&record_id, &path);
    state.save_exports();
}
