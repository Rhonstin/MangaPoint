//! Tab 2: what is downloaded, and what is on its way.

use crate::logic::{self, ChapterStatus};
use crate::state::{AppState, ChaptersState, JobKind, Modal, ModalAction, SeriesTab};
use crate::ui::widgets::{Chip, EmptyState, Icon, ProgressBar};
use crate::ui::work::{cancel_job, merged_key, prepare_preview, start_download};
use dioxus::prelude::*;
use std::collections::HashSet;

/// A chapter shown in the table: saved already, or ticked and waiting.
struct Row {
    key: String,
    number: Option<f64>,
    name: String,
    pages: Option<usize>,
    bytes: Option<u64>,
}

#[component]
pub fn DownloadsTab() -> Element {
    let state = use_context::<AppState>();
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };

    // Saved chapters first, then ticked ones that are not saved yet.
    let mut rows: Vec<Row> = state
        .library
        .read()
        .find(&ws.series_id)
        .map(|s| {
            s.chapters
                .iter()
                .map(|c| Row {
                    key: c.key.clone(),
                    number: c.number,
                    name: c.name.clone(),
                    pages: Some(c.pages),
                    bytes: Some(c.bytes),
                })
                .collect()
        })
        .unwrap_or_default();
    let saved_keys: HashSet<String> = rows.iter().map(|r| r.key.clone()).collect();
    let saved_count = rows.len();
    let saved_bytes: u64 = rows.iter().filter_map(|r| r.bytes).sum();
    if let ChaptersState::Ready(loaded) = &*state.chapters.read() {
        for &i in state.selected.read().iter() {
            if let Some(ch) = loaded.merged.get(i) {
                let key = merged_key(ch);
                if !saved_keys.contains(&key) {
                    rows.push(Row {
                        key,
                        number: ch.number,
                        name: ch.name.clone(),
                        pages: None,
                        bytes: None,
                    });
                }
            }
        }
    }
    rows.sort_by(|a, b| {
        a.number
            .unwrap_or(f64::MAX)
            .total_cmp(&b.number.unwrap_or(f64::MAX))
            .then_with(|| a.name.cmp(&b.name))
    });

    let job = state
        .job
        .read()
        .clone()
        .filter(|j| j.kind == JobKind::Download);
    let running = job.as_ref().is_some_and(|j| j.result.is_none());
    let stopping = job.as_ref().is_some_and(|j| j.progress.stopping);
    let plan: Vec<String> = job.as_ref().map(|j| j.plan.clone()).unwrap_or_default();
    let failed: Vec<String> = job.as_ref().map(|j| j.failed.clone()).unwrap_or_default();
    let active: Vec<usize> = job
        .as_ref()
        .map(|j| j.progress.active_indexes())
        .unwrap_or_default();
    let waiting = rows.iter().filter(|r| !saved_keys.contains(&r.key)).count();
    let shrink = state.settings.read().shrink_downloads;

    if rows.is_empty() {
        return rsx! {
            EmptyState {
                title: "Nothing downloaded yet",
                message: "Tick some chapters on the Chapters tab, then download them.",
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.open_tab(SeriesTab::Chapters), Icon { name: "list-checks", size: 16 } "Pick chapters" }
            }
        };
    }

    let table_rows: Vec<Element> = rows
        .iter()
        .map(|r| {
            let status = logic::download_status(&r.key, &saved_keys, &plan, &active, running, &failed);
            let number = r.number.map(logic::format_number).unwrap_or_else(|| "–".to_string());
            let size = r.bytes.map(logic::human_bytes).unwrap_or_default();
            let pages = r.pages.map(|p| p.to_string()).unwrap_or_default();
            let name = r.name.clone();
            let delete_name = r.name.clone();
            let key = r.key.clone();
            let series_id = ws.series_id.clone();
            let title = ws.title.clone();
            let own = job.as_ref().and_then(|j| {
                let index = plan.iter().position(|k| *k == r.key)?;
                j.progress.active.get(&index).cloned()
            });
            rsx! {
                tr { key: "{r.key}",
                    td { class: "num", "{number}" }
                    td { class: "name", title: "{name}", "{name}" }
                    td { class: "status",
                        {match status {
                            ChapterStatus::Saved => rsx! { Chip { text: "downloaded", kind: Some("ok".to_string()) } },
                            ChapterStatus::Downloading => rsx! {
                                Chip { text: match &own { Some(c) if c.pages_total > 0 => format!("{} / {} pages", c.pages_done, c.pages_total), _ => "starting…".to_string() }, kind: Some("accent".to_string()) }
                            },
                            ChapterStatus::Waiting => rsx! { Chip { text: if running { "waiting" } else { "not downloaded" } } },
                            ChapterStatus::Failed => rsx! { Chip { text: "couldn't download", kind: Some("bad".to_string()) } },
                        }}
                    }
                    td { class: "date", "{pages}" }
                    td { class: "date", "{size}" }
                    td { class: "check",
                        if status == ChapterStatus::Saved && !running {
                            button {
                                class: "btn ghost small", title: "Delete the downloaded pages of this chapter",
                                onclick: move |_| state.ask(Modal {
                                    title: "Delete this chapter?".into(),
                                    body: format!("The downloaded pages of {delete_name} in {title} will be deleted. Files you already exported are kept."),
                                    confirm: "Delete".into(),
                                    cancel: "Keep".into(),
                                    danger: true,
                                    action: ModalAction::DeleteDownloadedChapter { series_id: series_id.clone(), key: key.clone() },
                                }),
                                Icon { name: "trash", size: 14 }
                            }
                        }
                    }
                }
            }
        })
        .collect();

    rsx! {
        div { class: "card",
            div { class: "row wrap",
                div { class: "grow",
                    strong { "{saved_count} downloaded" }
                    span { class: "muted", " · {logic::human_bytes(saved_bytes)} on this computer" }
                    if waiting > 0 && !running { span { class: "muted", " · {waiting} to go" } }
                }
                if running {
                    button {
                        class: "btn small",
                        disabled: stopping,
                        onclick: move |_| cancel_job(state),
                        Icon { name: "x", size: 14 }
                        if stopping { "Stopping…" } else { "Cancel" }
                    }
                } else {
                    if waiting > 0 {
                        button { class: "btn primary", onclick: move |_| start_download(state),
                            Icon { name: "download", size: 16 } "Download {waiting} chapter(s)"
                        }
                    }
                    if saved_count > 0 {
                        button { class: if waiting > 0 { "btn" } else { "btn primary" },
                            onclick: move |_| { state.open_tab(SeriesTab::Preview); prepare_preview(state); },
                            Icon { name: "eye", size: 16 } "Preview"
                        }
                    }
                }
            }
            if running {
                {
                    let fraction = job.as_ref().map(|j| j.progress.fraction());
                    let label = job.as_ref().map(|j| j.progress.label()).unwrap_or_default();
                    let note = job.as_ref().and_then(|j| j.progress.note.clone());
                    let speed = job.as_ref().and_then(|j| j.progress.pages_per_second());
                    rsx! {
                        div { style: "margin-top: 12px", ProgressBar { fraction } }
                        p { class: "muted small", style: "margin: 6px 0 0", "{label}" if let Some(v) = speed { " · about {v:.1} pages a second" } if let Some(n) = note { " · {n}" } }
                    }
                }
            }
            if let Some(Err(message)) = job.as_ref().and_then(|j| j.result.clone()) {
                div { class: "banner bad small", style: "margin-bottom: 0", Icon { name: "circle-x", size: 16 } "{message}" }
            }
            if !running && !failed.is_empty() {
                div { class: "banner warn small", style: "margin-bottom: 0",
                    Icon { name: "triangle-alert", size: 16 }
                    div {
                        "{failed.len()} chapter(s) could not be downloaded from any source. You can try again."
                        if let Some(j) = &job { for line in j.skipped.iter() { div { class: "muted", "{line}" } } }
                    }
                }
            }
        }
        div { class: "table-wrap", style: "margin-top: 14px",
            div { class: "table-scroll",
                table { class: "chapters",
                    thead { tr {
                        th { class: "num", "No." }
                        th { "Title" }
                        th { class: "status", "Status" }
                        th { class: "date", "Pages" }
                        th { class: "date", "Size" }
                        th { class: "check" }
                    } }
                    tbody { {table_rows.into_iter()} }
                }
            }
        }
        p { class: "muted small", style: "margin: 8px 0 0",
            if shrink {
                "Space-saving mode is on, so tall pages are shrunk as they download. Change it in Settings → Saved pages."
            } else {
                "Pages are saved exactly as downloaded, for the best quality. Settings → Saved pages has a save-space option."
            }
        }
    }
}
