//! What is running, in the sidebar: the download in progress with the
//! chapters it is on, and the export. Nothing takes room at the top of a page.

use super::downloads_page::ActiveChapters;
use super::widgets::{Icon, ProgressBar};
use super::work::{cancel_job, set_queue_paused};
use crate::logic::{human_duration, plural};
use crate::state::{AppState, Page};
use dioxus::prelude::*;

#[component]
pub fn SidebarJobs() -> Element {
    rsx! {
        ExportCard {}
        DownloadCard {}
    }
}

/// The download in progress, or the queue when it is paused.
#[component]
fn DownloadCard() -> Element {
    let state = use_context::<AppState>();
    let queue = state.queue.read().clone();
    let running = queue.running().cloned();
    let (_, waiting, chapters_left) = queue.counts();
    let paused = queue.paused;
    if running.is_none() && !(paused && waiting > 0) {
        return rsx! {};
    }
    let (toggle_icon, toggle_title) = if paused {
        ("play", "Carry on with the downloads")
    } else {
        ("pause", "Pause the downloads")
    };
    rsx! {
        div { class: "side-job", onclick: move |_| state.go(Page::Downloads),
            div { class: "side-job-head",
                Icon { name: "download", size: 15 }
                if let Some(batch) = &running {
                    span { class: "side-job-title", title: "{batch.title}", "{batch.title}" }
                } else {
                    span { class: "side-job-title", "Downloads paused" }
                }
                button { class: "side-job-btn", title: "{toggle_title}",
                    onclick: move |e| {
                        e.stop_propagation();
                        set_queue_paused(state, !paused);
                    },
                    Icon { name: toggle_icon, size: 13 }
                }
            }
            if let Some(batch) = &running {
                div { class: "side-job-meta",
                    span { "{batch.chapters_done()} of {plural(batch.chapter_count(), \"chapter\")}" }
                    if let Some(v) = batch.progress.pages_per_second() { span { "{v:.1} pages/s" } }
                }
                ProgressBar { fraction: Some(batch.progress.fraction()) }
                ActiveChapters { progress: batch.progress.clone(), limit: 3 }
                if waiting > 0 { div { class: "side-job-meta", "{waiting} more in line" } }
            } else {
                div { class: "side-job-meta", "{plural(chapters_left, \"chapter\")} waiting" }
            }
        }
    }
}

/// The export that is running, or has just ended.
#[component]
fn ExportCard() -> Element {
    let state = use_context::<AppState>();
    let Some(job) = state.job.read().clone() else {
        return rsx! {};
    };
    let running = job.result.is_none();
    let (meta_class, meta) = match &job.result {
        Some(Ok(text)) => ("side-job-meta", text.clone()),
        Some(Err(text)) => ("side-job-meta bad", text.clone()),
        None => ("side-job-meta", String::new()),
    };
    let left = job.progress.eta_secs().map(human_duration);
    rsx! {
        div { class: "side-job", onclick: move |_| state.go(Page::Exports),
            div { class: "side-job-head",
                Icon { name: "file-down", size: 15 }
                span { class: "side-job-title", title: "{job.title}", "{job.title}" }
                if running {
                    button { class: "side-job-btn", title: "Stop the export", disabled: job.progress.stopping,
                        onclick: move |e| {
                            e.stop_propagation();
                            cancel_job(state);
                        },
                        Icon { name: "x", size: 13 }
                    }
                } else {
                    button { class: "side-job-btn", title: "Dismiss",
                        onclick: move |e| {
                            e.stop_propagation();
                            let mut j = state.job;
                            j.set(None);
                        },
                        Icon { name: "x", size: 13 }
                    }
                }
            }
            if running {
                div { class: "side-job-meta",
                    span { "{job.progress.finished} of {plural(job.progress.chapter_total, \"chapter\")}" }
                    if let Some(left) = left { span { "{left} left" } }
                }
                ProgressBar { fraction: Some(job.progress.fraction()) }
                ActiveChapters { progress: job.progress.clone(), limit: 3 }
            } else {
                div { class: "{meta_class}", "{meta}" }
            }
        }
    }
}
