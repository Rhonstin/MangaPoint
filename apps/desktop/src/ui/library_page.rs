//! The Library: everything downloaded, and every file exported.

use super::widgets::{show_in_folder, Chip, EmptyState, Icon};
use super::work::{forget_exported_file, open_series_from_library};
use crate::logic::{ago, human_bytes};
use crate::state::{now_secs, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;

#[component]
pub fn LibraryPage() -> Element {
    let state = use_context::<AppState>();
    let library = state.library.read().clone();
    let mut records = state.exports.read().records.clone();
    records.reverse(); // newest first
    let now = now_secs();
    let total = library.total_bytes();

    rsx! {
        h1 { "Library" }
        p { class: "muted", "Everything you have downloaded, and every file you have exported." }

        div { class: "card",
            div { class: "row",
                h2 { class: "grow", Icon { name: "book-open", size: 17 } "Downloaded" }
                if !library.series.is_empty() {
                    span { class: "muted small", "{library.series.len()} series · {human_bytes(total)}" }
                }
            }
            if library.series.is_empty() {
                EmptyState {
                    title: "Nothing downloaded yet",
                    message: "Search for a series, tick the chapters you want and download them. They'll live here.",
                    compact: true,
                }
                div { style: "text-align: center",
                    button { class: "btn", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
                }
            }
            for s in library.series.iter() {
                {
                    let id = s.id.clone();
                    let open_id = s.id.clone();
                    let title = s.title.clone();
                    let size = human_bytes(s.bytes());
                    let body = format!(
                        "The downloaded pages of {} ({size}) will be deleted. Files you already exported are kept, and you can download the chapters again later.",
                        s.title
                    );
                    rsx! {
                        div { class: "list-row", key: "{s.id}",
                            div { class: "grow",
                                strong { "{s.title}" }
                                div { class: "muted small", "{s.chapters.len()} chapters · {s.page_count()} pages · {size}" }
                            }
                            button { class: "btn primary small", onclick: move |_| open_series_from_library(state, &open_id),
                                Icon { name: "eye", size: 14 } "Open"
                            }
                            button {
                                class: "btn small danger", title: "Delete the downloaded pages",
                                onclick: move |_| state.ask(Modal {
                                    title: format!("Delete {title}?"),
                                    body: body.clone(),
                                    confirm: "Delete".into(),
                                    cancel: "Keep".into(),
                                    danger: true,
                                    action: ModalAction::DeleteDownloadedSeries { series_id: id.clone() },
                                }),
                                Icon { name: "trash", size: 14 } "Delete"
                            }
                        }
                    }
                }
            }
        }

        div { class: "card",
            h2 { Icon { name: "file-down", size: 17 } "Exported files" }
            if records.is_empty() {
                EmptyState {
                    title: "No exports yet",
                    message: "Files you export for your reader will be listed here, so you can find or delete them later.",
                    compact: true,
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
}
