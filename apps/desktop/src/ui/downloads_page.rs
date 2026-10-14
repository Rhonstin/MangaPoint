//! The download queue: what is downloading now, what is next, what finished.
//! Chapters are added from any series; they download in turn, so there is
//! nothing to start by hand.

use super::widgets::{open_folder, Chip, Cover, EmptyState, Icon, NoCover, ProgressBar};
use super::work::{
    cancel_all_downloads, cancel_batch, load_saved_cover, open_series_from_library,
    prioritize_batch, retry_batch, saved_cover_key, set_queue_paused,
};
use crate::logic::{ago, human_duration, plural, JobProgress};
use crate::queue::BatchStatus;
use crate::state::{library_files, now_secs, AppState, Page};
use dioxus::prelude::*;

#[component]
pub fn DownloadsPage() -> Element {
    let state = use_context::<AppState>();
    // Covers of whatever is in the queue.
    use_effect(move || {
        let ids: Vec<String> = state
            .queue
            .read()
            .batches
            .iter()
            .map(|b| b.series_id.clone())
            .collect();
        for id in ids {
            load_saved_cover(state, &id);
        }
    });

    let queue = state.queue.read().clone();
    let running = queue.running().map(|b| b.id);
    let waiting: Vec<u64> = queue
        .batches
        .iter()
        .filter(|b| b.status == BatchStatus::Waiting)
        .map(|b| b.id)
        .collect();
    // Newest first.
    let finished: Vec<u64> = queue
        .batches
        .iter()
        .rev()
        .filter(|b| !b.status.is_active())
        .map(|b| b.id)
        .collect();
    let (_, waiting_batches, chapters_left) = queue.counts();
    let paused = queue.paused;
    let busy = running.is_some() || !waiting.is_empty();

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Downloads" }
                p { class: "muted",
                    if busy {
                        "{plural(chapters_left, \"chapter\")} to go"
                        if waiting_batches > 0 { " · {waiting_batches} more in line" }
                        if paused { " · paused" }
                    } else {
                        "Downloads go on here one series after another."
                    }
                }
            }
            if busy {
                button { class: "btn", onclick: move |_| set_queue_paused(state, !paused),
                    if paused { Icon { name: "play", size: 15 } "Resume" } else { Icon { name: "pause", size: 15 } "Pause" }
                }
                button { class: "btn danger", onclick: move |_| cancel_all_downloads(state),
                    Icon { name: "x", size: 15 } "Cancel all"
                }
            }
        }

        if !busy && finished.is_empty() {
            EmptyState {
                title: "Nothing downloading",
                message: "Pick chapters in any series and add them to the downloads. They go on in the background while you carry on.",
                scene: "waiting",
                side: true,
                button { class: "btn primary", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
            }
        }

        if let Some(id) = running {
            h2 { class: "q-title", "Downloading now" }
            BatchRow { id }
        }
        if !waiting.is_empty() {
            h2 { class: "q-title", "Up next" }
            for id in waiting.iter().copied() {
                BatchRow { key: "{id}", id }
            }
        }
        if !finished.is_empty() {
            div { class: "q-section",
                h2 { class: "q-title", "Finished" span { class: "muted", " ({finished.len()})" } }
                span { class: "grow" }
                button { class: "btn small ghost", onclick: move |_| state.edit_queue(|q| q.clear_finished()),
                    "Clear all"
                }
            }
            for id in finished.iter().copied() {
                BatchRow { key: "{id}", id }
            }
        }
    }
}

/// One batch: its series, how far it is, and what can be done with it.
#[component]
fn BatchRow(id: u64) -> Element {
    let state = use_context::<AppState>();
    let Some(batch) = state.queue.read().get(id).cloned() else {
        return rsx! {};
    };
    let has_cover = state
        .library
        .read()
        .find(&batch.series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let in_library = state.library.read().find(&batch.series_id).is_some();
    let series_id = batch.series_id.clone();
    let total = batch.chapter_count();
    let done = batch.chapters_done();
    let running = batch.status == BatchStatus::Running;
    let waiting = batch.status == BatchStatus::Waiting;
    let speed = batch.progress.pages_per_second();
    let stopping = batch.progress.stopping;
    let failed = batch.failed.len();
    let skipped_lines: Vec<String> = batch.skipped.iter().take(3).cloned().collect();
    let status_chip = match &batch.status {
        BatchStatus::Done if failed == 0 => Some(("done", "ok")),
        BatchStatus::Done => Some(("partly done", "warn")),
        BatchStatus::Failed(_) => Some(("failed", "bad")),
        BatchStatus::Cancelled => Some(("stopped", "warn")),
        _ => None,
    };
    let cover_key = saved_cover_key(&batch.series_id);
    let open_id = series_id.clone();
    let folder_id = series_id.clone();

    rsx! {
        div { class: if running { "q-row running" } else { "q-row" },
            div { class: "q-cover",
                if has_cover { Cover { key_id: cover_key } } else { NoCover {} }
            }
            div { class: "q-main",
                div { class: "row wrap",
                    button { class: "link-title q-name", disabled: !in_library,
                        title: if in_library { "Open this series" } else { "" },
                        onclick: move |_| open_series_from_library(state, &open_id),
                        "{batch.title}"
                    }
                    if let Some((text, kind)) = status_chip { Chip { text: text.to_string(), kind: Some(kind.to_string()) } }
                    if batch.choice != pipeline::SourceChoice::Adaptive { Chip { text: "one source" } }
                }
                if running {
                    div { class: "q-stats", style: "margin: 2px 0 8px",
                        span { "{done} of {plural(total, \"chapter\")} done" }
                        if let Some(v) = speed { span { "about {v:.1} pages a second" } }
                        if let Some(s) = batch.progress.eta_secs() { span { "about {human_duration(s)} left" } }
                    }
                    ProgressBar { fraction: Some(batch.progress.fraction()) }
                    ActiveChapters { progress: batch.progress.clone() }
                    if let Some(note) = &batch.progress.note {
                        div { class: "muted small q-line", "{note}" }
                    }
                } else if waiting {
                    div { class: "muted small", "{plural(total, \"chapter\")} waiting" }
                } else {
                    div { class: "muted small",
                        if let Some(summary) = &batch.summary { "{summary}" }
                    }
                    if let Some(stats) = batch.stats {
                        div { class: "q-stats",
                            span { "took {human_duration(stats.secs)}" }
                            span { "{plural(stats.pages, \"page\")}" }
                            if stats.secs > 0 && stats.pages > 0 {
                                span { "{stats.pages as f32 / stats.secs as f32:.1} pages a second" }
                            }
                        }
                    }
                    for line in skipped_lines.iter() {
                        div { class: "muted small q-line", key: "{line}", "{line}" }
                    }
                }
            }
            div { class: "q-actions",
                if running {
                    button { class: "btn small", disabled: stopping, onclick: move |_| cancel_batch(state, id),
                        Icon { name: "x", size: 14 }
                        if stopping { "Stopping…" } else { "Cancel" }
                    }
                } else if waiting {
                    button { class: "btn small", title: "Download this next", onclick: move |_| prioritize_batch(state, id),
                        Icon { name: "arrow-up-to-line", size: 14 } "Next"
                    }
                    button { class: "btn small ghost", title: "Take it out of the queue", onclick: move |_| cancel_batch(state, id),
                        Icon { name: "x", size: 14 }
                    }
                } else {
                    if failed > 0 {
                        button { class: "btn small", onclick: move |_| retry_batch(state, id),
                            Icon { name: "rotate-cw", size: 14 } "Try {plural(failed, \"chapter\")} again"
                        }
                    }
                    div { class: "q-aside",
                        if batch.finished_at > 0 {
                            span { class: "q-when", "Completed {ago(now_secs(), batch.finished_at)}" }
                        }
                        if batch.status == BatchStatus::Done {
                            button { class: "btn primary open-big", title: "Open the folder this series is kept in",
                                onclick: move |_| {
                                    let dir = library_files().cover_path(&folder_id).parent().map(|p| p.to_path_buf());
                                    if let Some(dir) = dir {
                                        let _ = std::fs::create_dir_all(&dir);
                                        open_folder(&dir);
                                    }
                                },
                                Icon { name: "folder-open", size: 18 } "Open"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The chapters being worked on, each with its own small bar. `limit` is how
/// many are shown (the others are counted).
#[component]
pub(super) fn ActiveChapters(progress: JobProgress, limit: Option<usize>) -> Element {
    let shown = limit.unwrap_or(12);
    let more = progress.active.len().saturating_sub(shown);
    rsx! {
        if !progress.active.is_empty() {
            div { class: "q-chapters",
                for (index, chapter) in progress.active.iter().take(shown) {
                    div { class: "q-chap", key: "{index}",
                        div { class: "q-chap-top",
                            span { class: "q-chap-name", title: "{chapter.name}", "{chapter.name}" }
                            span { class: "muted",
                                if chapter.pages_total > 0 { "{chapter.pages_done}/{chapter.pages_total}" } else { "starting" }
                            }
                        }
                        ProgressBar {
                            fraction: (chapter.pages_total > 0)
                                .then(|| chapter.pages_done as f32 / chapter.pages_total as f32),
                        }
                    }
                }
                if more > 0 {
                    div { class: "q-chap more muted", "and {more} more" }
                }
            }
        }
    }
}
