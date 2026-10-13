//! The Export screen: its list of series and the export itself.

use super::*;

/// Open the Export screen for these series. Only series with downloaded
/// chapters can be exported, so the others are left out.
pub fn open_export(state: AppState, series_ids: Vec<String>) {
    let items: Vec<ExportItem> = series_ids
        .iter()
        .filter(|id| {
            state
                .library
                .peek()
                .find(id)
                .is_some_and(|s| !s.chapters.is_empty())
        })
        .map(|id| ExportItem {
            series_id: id.clone(),
            ..Default::default()
        })
        .collect();
    if items.is_empty() {
        state.error("Download some chapters first, then export them.");
        return;
    }
    let mut plan = state.export_plan;
    plan.set(ExportPlan { items, active: 0 });
    activate_export_series(state, 0);
    state.go(Page::Export);
}

/// Open the Export screen on the sample pages, to look around and try presets,
/// when nothing has been chosen to export.
pub fn open_sample(state: AppState) {
    let id = crate::sample::ensure_sample(state);
    let mut plan = state.export_plan;
    plan.set(ExportPlan {
        items: vec![ExportItem {
            series_id: id,
            ..Default::default()
        }],
        active: 0,
    });
    activate_export_series(state, 0);
    state.go(Page::Export);
}

/// Edit and preview this series of the Export screen's list.
pub fn activate_export_series(state: AppState, index: usize) {
    let id = state
        .export_plan
        .peek()
        .items
        .get(index)
        .map(|i| i.series_id.clone());
    let Some(id) = id else { return };
    let series_id = id.clone();
    let title = ws_title(state, &id);
    let mut plan = state.export_plan;
    plan.write().active = index;
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: id,
        title,
        ranked: None,
        origin: Page::Library,
    }));
    {
        let mut preview = state.preview;
        let mut p = preview.write();
        p.chapter_key = None;
        p.pages.clear();
        p.fresh = 0;
        p.selected = 0;
        p.thumb_start = 0;
        p.window = (0, 0);
        p.draft_crop = None;
        p.shading_samples.clear();
        // Back where it was, if the screen was left and opened again.
        let remembered = state
            .export_plan
            .peek()
            .items
            .get(index)
            .and_then(|i| i.chapter.clone());
        let valid = remembered.filter(|key| {
            state
                .library
                .peek()
                .find(&series_id)
                .is_some_and(|s| s.chapters.iter().any(|c| &c.key == key))
        });
        p.chapter_key = valid;
    }
    let mut draft = state.crop_draft;
    draft.set(library::CropRect::default());
    prepare_preview(state);
}

/// Add a series to the Export screen's list.
pub fn add_export_series(state: AppState, series_id: String) {
    let has_chapters = state
        .library
        .peek()
        .find(&series_id)
        .is_some_and(|s| !s.chapters.is_empty());
    if !has_chapters {
        return;
    }
    let mut plan = state.export_plan;
    let mut p = plan.write();
    if !p.items.iter().any(|i| i.series_id == series_id) {
        p.items.push(ExportItem {
            series_id,
            ..Default::default()
        });
    }
}

/// Take a series off the Export screen's list. With none left, go back.
pub fn remove_export_series(state: AppState, index: usize) {
    let (left, active) = {
        let mut plan = state.export_plan;
        let mut p = plan.write();
        if index >= p.items.len() {
            return;
        }
        p.items.remove(index);
        let last = p.items.len().saturating_sub(1);
        p.active = if index < p.active || p.active > last {
            p.active.saturating_sub(1).min(last)
        } else {
            p.active
        };
        (p.items.len(), p.active)
    };
    if left == 0 {
        state.go(Page::Library);
    } else {
        activate_export_series(state, active);
    }
}

/// Export every series switched on in the Export screen's list, one after
/// another, to the chosen place.
pub fn start_export_plan(state: AppState) {
    if job_running(state) {
        state.error("Something is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let plan = state.export_plan.peek().clone();
    let opts = state.export_opts.peek().clone();
    let settings = state.settings.peek().clone();
    let list: Vec<(library::DownloadedSeries, Vec<String>)> = plan
        .items
        .iter()
        .filter(|i| !i.off)
        .filter_map(|i| {
            let series = state.library.peek().find(&i.series_id).cloned()?;
            if series.is_sample() {
                return None;
            }
            let keys: Vec<String> = series
                .chapters
                .iter()
                .filter(|c| !i.excluded.contains(&c.key))
                .map(|c| c.key.clone())
                .collect();
            (!keys.is_empty()).then_some((series, keys))
        })
        .collect();
    if list.is_empty() {
        state.error("Choose at least one chapter to export.");
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
    let grouping = grouping_of(&opts);
    spawn_forever(async move {
        let total = list.len();
        for (n, (series, keys)) in list.into_iter().enumerate() {
            let cancel = CancelFlag::new();
            let mut export = ExportJob::new(
                series.clone(),
                keys.clone(),
                settings.device_profile(),
                target.clone(),
                paths().spool_dir(),
            );
            export.language = settings.language.clone();
            export.grouping = grouping;
            let mut job = state.job;
            job.set(Some(JobView {
                kind: JobKind::Export,
                title: if total > 1 {
                    format!("{} ({} of {total})", series.title, n + 1)
                } else {
                    series.title.clone()
                },
                progress: JobProgress::default(),
                result: None,
                cancel: cancel.clone(),
            }));
            info!(
                "Export started: \"{}\", {} chapter(s) as {}, {}, to {}",
                series.title,
                keys.len(),
                series.settings.format.extension().to_uppercase(),
                match grouping {
                    Grouping::Single => "one file".to_string(),
                    Grouping::PerChapter => "one file per chapter".to_string(),
                    Grouping::EveryN(n) => format!("one file per {n} chapters"),
                },
                describe_target(&target)
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
        if total > 1 {
            let mut job = state.job;
            let mut guard = job.write();
            if let Some(view) = guard.as_mut() {
                view.result = Some(Ok(format!("Exported {total} series")));
            }
        }
    });
}

/// Run one export, keeping the job bar up to date, and record the result.
pub(super) async fn run_export(
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
pub(super) fn log_event(event: &JobEvent, logged_pages: &mut HashMap<usize, usize>) {
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
