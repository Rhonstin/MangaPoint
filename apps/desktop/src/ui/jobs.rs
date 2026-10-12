//! Running exports, and the history of past ones.

use super::widgets::{show_in_folder, Chip, Icon};
use crate::logic::{ago, JobProgress};
use crate::state::spawn_forever;
use crate::state::{now_secs, paths, AppState, JobView};
use dioxus::prelude::*;
use pipeline::store::{JobState, JobStore};
use pipeline::{run_job, CancelFlag, JobEvent, JobSpec, PageCache, PipelineError, RetryPolicy};
use std::time::Instant;
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

/// Start an export in the background. It keeps running if you leave the screen.
pub fn start_job(state: AppState, spec: JobSpec) {
    let running = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.result.is_none());
    if running {
        state.error("An export is already running. Wait for it to finish, or cancel it.");
        return;
    }
    let Some(client) = state.client() else {
        state.error("The catalog engine is not running.");
        return;
    };
    let cancel = CancelFlag::new();
    let mut job = state.job;
    job.set(Some(JobView {
        title: spec.series_title.clone(),
        progress: JobProgress::default(),
        result: None,
        skipped: Vec::new(),
        cancel: cancel.clone(),
    }));
    let store = JobStore::new(paths().jobs_file());
    let _ = store.started(&spec);
    state.bump_jobs();
    info!(
        "Export started: \"{}\", {} chapter(s) as {}, to {}",
        spec.series_title,
        spec.chapters.len(),
        spec.settings.conversion.format.extension().to_uppercase(),
        describe_target(&spec.target)
    );

    spawn_forever(async move {
        let started = Instant::now();
        let cache = PageCache::new(paths().page_cache_dir());
        let policy = RetryPolicy::default();
        let (tx, mut rx) = unbounded_channel();
        let run = run_job(&*client, &spec, &cache, &policy, Some(tx), &cancel);
        let watch = async {
            let mut logged_pages = 0;
            while let Some(event) = rx.recv().await {
                log_event(&event, &mut logged_pages);
                if let Some(view) = job.write().as_mut() {
                    view.progress.apply(&event);
                }
            }
        };
        let (result, ()) = futures_util::future::join(run, watch).await;

        match result {
            Ok(outcome) => {
                info!(
                    "Export finished in {:.0}s: {} pages from {} chapter(s){}",
                    started.elapsed().as_secs_f32(),
                    outcome.pages,
                    outcome.chapters_done,
                    if outcome.skipped.is_empty() {
                        String::new()
                    } else {
                        format!(", {} chapter(s) skipped", outcome.skipped.len())
                    }
                );
                for f in &outcome.files {
                    info!(
                        "Wrote {} ({})",
                        f.location,
                        crate::logic::human_bytes(f.bytes)
                    );
                }
                {
                    let mut health = state.health;
                    let mut h = health.write();
                    for u in &outcome.source_use {
                        h.record_use(&u.source_id, u.ok, u.ms);
                    }
                }
                state.save_health();
                let _ = store.finished(&spec, &outcome);
                let files: Vec<String> = outcome.files.iter().map(|f| f.location.clone()).collect();
                let skipped: Vec<String> = outcome
                    .skipped
                    .iter()
                    .map(|s| format!("{}: {}", s.name, s.reason))
                    .collect();
                if let Some(view) = job.write().as_mut() {
                    view.skipped = skipped;
                    view.result = Some(Ok(files));
                }
            }
            Err(e) => {
                let cancelled = matches!(e, PipelineError::Cancelled);
                let message = e.to_string();
                if cancelled {
                    info!("Export cancelled");
                } else {
                    warn!("Export failed: {message}");
                }
                let _ = store.failed(&spec, &message, cancelled);
                if let Some(view) = job.write().as_mut() {
                    view.result = Some(Err(message));
                }
            }
        }
        state.bump_jobs();
    });
}

fn describe_target(target: &pipeline::ExportTarget) -> String {
    match target {
        pipeline::ExportTarget::Folder { path } => format!("folder {}", path.display()),
        pipeline::ExportTarget::Device { mount, folder } => {
            format!("the reader at {}\\{folder}", mount.display())
        }
        pipeline::ExportTarget::WebDav { base_url, .. } => {
            format!("the reader over Wi-Fi ({base_url})")
        }
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
                info!("  {done}/{total} pages downloaded and converted");
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
        JobEvent::Writing { file } => info!("Writing {file}"),
    }
}

#[component]
pub fn JobsPage() -> Element {
    let state = use_context::<AppState>();
    let records = use_resource(move || async move {
        let _ = state.jobs_version.read();
        let mut all = JobStore::new(paths().jobs_file()).all();
        all.reverse();
        all
    });
    let now = now_secs();

    rsx! {
        h1 { "Exports" }
        p { class: "muted", "Everything you have exported. A failed export keeps what it already downloaded, so Retry continues where it stopped." }
        {match &*records.read() {
            None => rsx! { p { class: "muted", "Loading…" } },
            Some(list) if list.is_empty() => rsx! {
                div { class: "card", p { class: "muted", "Nothing exported yet. Search for a series, tick the chapters you want and press Convert & export." } }
            },
            Some(list) => rsx! {
                div { class: "card",
                    for rec in list.iter() {
                        {
                            let spec = rec.spec.clone();
                            let retryable = matches!(rec.state, JobState::Failed | JobState::Cancelled);
                            let (label, kind) = match rec.state {
                                JobState::Running => ("Running", None),
                                JobState::Done => ("Done", Some("ok")),
                                JobState::Failed => ("Failed", Some("bad")),
                                JobState::Cancelled => ("Cancelled", Some("warn")),
                            };
                            rsx! {
                                div { class: "list-row", key: "{rec.id}", style: "align-items: flex-start",
                                    div { class: "grow",
                                        div { class: "row wrap",
                                            strong { "{rec.series_title}" }
                                            Chip { text: label.to_string(), kind: kind.map(String::from) }
                                            span { class: "muted small", "{rec.chapters} chapters · {ago(now, rec.updated_at)}" }
                                        }
                                        if let Some(err) = &rec.error { div { class: "muted small", "{err}" } }
                                        for file in rec.files.iter() {
                                            div { class: "row small",
                                                span { class: "grow muted", style: "word-break: break-all", "{file}" }
                                                {
                                                    let f = file.clone();
                                                    rsx! { button { class: "btn small", onclick: move |_| show_in_folder(&f), Icon { name: "folder-open", size: 14 } "Show" } }
                                                }
                                            }
                                        }
                                        for s in rec.skipped.iter() {
                                            div { class: "small", style: "color: var(--warn)", "Skipped {s}" }
                                        }
                                    }
                                    if retryable {
                                        button {
                                            class: "btn primary small",
                                            onclick: move |_| start_job(state, spec.clone()),
                                            Icon { name: "rotate-ccw", size: 14 }
                                            "Retry"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
        }}
    }
}
