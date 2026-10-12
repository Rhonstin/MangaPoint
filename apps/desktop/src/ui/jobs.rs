//! Running exports, and the history of past ones.

use super::widgets::{show_in_folder, Chip};
use crate::logic::{ago, JobProgress};
use crate::state::spawn_forever;
use crate::state::{now_secs, paths, AppState, JobView};
use dioxus::prelude::*;
use pipeline::store::{JobState, JobStore};
use pipeline::{run_job, CancelFlag, JobSpec, PageCache, PipelineError, RetryPolicy};
use tokio::sync::mpsc::unbounded_channel;

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

    spawn_forever(async move {
        let cache = PageCache::new(paths().page_cache_dir());
        let policy = RetryPolicy::default();
        let (tx, mut rx) = unbounded_channel();
        let run = run_job(&*client, &spec, &cache, &policy, Some(tx), &cancel);
        let watch = async {
            while let Some(event) = rx.recv().await {
                if let Some(view) = job.write().as_mut() {
                    view.progress.apply(&event);
                }
            }
        };
        let (result, ()) = futures_util::future::join(run, watch).await;

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
                let _ = store.failed(&spec, &message, cancelled);
                if let Some(view) = job.write().as_mut() {
                    view.result = Some(Err(message));
                }
            }
        }
        state.bump_jobs();
    });
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
                                                    rsx! { button { class: "btn small", onclick: move |_| show_in_folder(&f), "Show" } }
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
