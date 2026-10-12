//! The download queue: what is downloading now, what is next, what finished.
//! Chapters are added from any series; they download in turn, so there is
//! nothing to start by hand.

use super::widgets::{show_in_folder, Chip, Cover, EmptyState, Icon, ProgressBar};
use super::work::{
    cancel_all_downloads, cancel_batch, forget_exported_file, load_saved_cover,
    open_series_from_library, prioritize_batch, retry_batch, saved_cover_key, set_queue_paused,
};
use crate::logic::{ago, human_bytes, plural};
use crate::queue::BatchStatus;
use crate::state::{now_secs, AppState, Modal, ModalAction, Page};
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
                        "Downloads go on here one series after another, and the files you export are listed below."
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
            if !finished.is_empty() {
                button { class: "btn ghost", onclick: move |_| state.edit_queue(|q| q.clear_finished()),
                    "Clear finished"
                }
            }
        }

        if !busy && finished.is_empty() && state.exports.read().records.is_empty() {
            EmptyState {
                title: "Nothing downloading",
                message: "Pick chapters in any series and add them to the downloads. They go on in the background while you carry on.",
                scene: "waiting",
            }
            div { style: "text-align: center",
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
            h2 { class: "q-title", "Finished" }
            for id in finished.iter().copied() {
                BatchRow { key: "{id}", id }
            }
        }
        ExportedFiles {}
    }
}

/// Every file exported for a reader, newest first.
#[component]
fn ExportedFiles() -> Element {
    let state = use_context::<AppState>();
    let mut records = state.exports.read().records.clone();
    records.reverse();
    let now = now_secs();
    rsx! {
        h2 { class: "q-title", "Exported files" }
        if records.is_empty() {
            p { class: "muted small", style: "margin: 4px 0 0",
                "Files you export for your reader are listed here, so you can find or delete them later."
            }
        }
        for r in records.iter() {
            div { class: "list-row", key: "{r.id}", style: "align-items: flex-start",
                div { class: "grow",
                    div { class: "row wrap",
                        strong { "{r.title}" }
                        Chip { text: r.format.to_uppercase() }
                        span { class: "muted small", "{r.chapters} chapters · {ago(now, r.at)} · {r.target}" }
                    }
                    for f in r.files.iter() {
                        {
                            let path = f.path.clone();
                            let is_url = path.starts_with("http");
                            let missing = !is_url && !std::path::Path::new(&path).is_file();
                            let size = human_bytes(f.bytes);
                            let record_id = r.id.clone();
                            let (p_show, p_delete, p_forget) = (path.clone(), path.clone(), path.clone());
                            let (id_delete, id_forget) = (record_id.clone(), record_id.clone());
                            rsx! {
                                div { class: "row small", style: "margin-top: 4px",
                                    span { class: "grow muted", style: "word-break: break-all", "{path}" }
                                    if missing { Chip { text: "missing", kind: Some("warn".to_string()) } } else { span { class: "muted", "{size}" } }
                                    if missing {
                                        button { class: "btn small", onclick: move |_| forget_exported_file(state, id_forget.clone(), p_forget.clone()), "Forget" }
                                    } else if !is_url {
                                        button { class: "btn small", title: "Show in its folder", onclick: move |_| show_in_folder(&p_show),
                                            Icon { name: "folder-open", size: 14 }
                                        }
                                        button {
                                            class: "btn small danger", title: "Delete this file",
                                            onclick: move |_| state.ask(Modal {
                                                title: "Delete this file?".into(),
                                                body: format!("{p_delete} will be deleted from your computer. Your downloaded chapters are not affected."),
                                                confirm: "Delete file".into(),
                                                cancel: "Keep".into(),
                                                danger: true,
                                                action: ModalAction::DeleteExportedFile { record_id: id_delete.clone(), path: p_delete.clone() },
                                            }),
                                            Icon { name: "trash", size: 14 }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
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
    let actives: Vec<String> = batch
        .progress
        .active
        .values()
        .take(4)
        .map(|a| {
            if a.pages_total > 0 {
                format!("{} · {} of {} pages", a.name, a.pages_done, a.pages_total)
            } else {
                format!("{} · starting…", a.name)
            }
        })
        .collect();
    let more_active = batch.progress.active.len().saturating_sub(actives.len());
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

    rsx! {
        div { class: if running { "q-row running" } else { "q-row" },
            div { class: "q-cover",
                if has_cover { Cover { key_id: cover_key } } else { div { class: "cover empty", "📖" } }
            }
            div { class: "q-main",
                div { class: "row wrap",
                    strong { class: "q-name", "{batch.title}" }
                    if let Some((text, kind)) = status_chip { Chip { text: text.to_string(), kind: Some(kind.to_string()) } }
                    if batch.choice != pipeline::SourceChoice::Adaptive { Chip { text: "one source" } }
                }
                if running {
                    div { class: "muted small", style: "margin: 2px 0 8px",
                        "{done} of {plural(total, \"chapter\")} · {batch.progress.label()}"
                        if let Some(v) = speed { " · about {v:.1} pages a second" }
                    }
                    ProgressBar { fraction: Some(batch.progress.fraction()) }
                    for line in actives.iter() {
                        div { class: "muted small q-line", key: "{line}", "{line}" }
                    }
                    if more_active > 0 { div { class: "muted small q-line", "and {more_active} more…" } }
                    if let Some(note) = &batch.progress.note {
                        div { class: "muted small q-line", "{note}" }
                    }
                } else if waiting {
                    div { class: "muted small", "{plural(total, \"chapter\")} waiting" }
                } else {
                    div { class: "muted small",
                        if let Some(summary) = &batch.summary { "{summary}" }
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
                    if in_library {
                        button { class: "btn small ghost", onclick: move |_| open_series_from_library(state, &open_id),
                            "Open"
                        }
                    }
                    button { class: "btn small ghost", title: "Dismiss", onclick: move |_| { state.edit_queue(|q| { q.remove(id); }); },
                        Icon { name: "x", size: 14 }
                    }
                }
            }
        }
    }
}
