//! The Library: the series you keep (downloaded or saved for later), and
//! every file exported.

use super::widgets::{show_in_folder, Chip, Cover, EmptyState, Icon};
use super::work::{
    ensure_series_info, forget_exported_file, load_saved_cover, open_series_from_library,
    saved_cover_key,
};
use crate::logic::{ago, human_bytes, library_order, plural, LibrarySort, LibraryView};
use crate::state::{now_secs, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;

#[component]
pub fn LibraryPage() -> Element {
    let state = use_context::<AppState>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    // Fetch the details and covers of series that do not have them yet, and
    // show the saved covers. Runs again when the catalog comes up or the
    // library changes.
    use_effect(move || {
        let _catalog_ready = state.services.read().is_some();
        let series: Vec<(String, bool, bool)> = state
            .library
            .read()
            .series
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    s.info.is_some(),
                    s.info.as_ref().is_some_and(|i| i.has_cover),
                )
            })
            .collect();
        for (id, has_info, has_cover) in series {
            if !has_info {
                ensure_series_info(state, &id);
            } else if has_cover {
                load_saved_cover(state, &id);
            }
        }
    });
    let library = state.library.read().clone();
    let mut records = state.exports.read().records.clone();
    records.reverse(); // newest first
    let now = now_secs();
    let total = library.total_bytes();
    let order = library_order(&library.series, view(), sort(), &filter.read());
    let downloaded_count = library
        .series
        .iter()
        .filter(|s| !s.chapters.is_empty())
        .count();
    let saved_count = library.series.len() - downloaded_count;
    let queue = state.queue.read().clone();
    let (running, waiting, chapters_left) = queue.counts();

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Library" }
                p { class: "muted",
                    if library.series.is_empty() {
                        "Series you keep, to read now or later."
                    } else {
                        "{downloaded_count} downloaded · {human_bytes(total)} on this computer"
                        if saved_count > 0 { " · {saved_count} saved for later" }
                    }
                }
            }
        }

        if running + waiting > 0 {
            div { class: "lib-strip", onclick: move |_| state.go(Page::Downloads),
                Icon { name: "download", size: 16 }
                span { class: "grow",
                    "Downloading: {plural(chapters_left, \"chapter\")} to go"
                    if waiting > 0 { " · {waiting} more in line" }
                }
                span { class: "muted small", "See the queue" }
            }
        }

        if library.series.is_empty() {
            EmptyState {
                title: "Your library is empty",
                message: "Find a series and add it to your library. You can download its chapters now or whenever you like.",
                shelf: true,
            }
            div { style: "text-align: center",
                button { class: "btn primary", onclick: move |_| state.go(Page::Search), Icon { name: "search", size: 16 } "Find a series" }
            }
        } else {
            div { class: "lib-tools",
                div { class: "pillrow",
                    for v in LibraryView::ALL {
                        button {
                            key: "{v.label()}",
                            class: if view() == v { "pill on" } else { "pill" },
                            onclick: move |_| view.set(v),
                            "{v.label()}"
                        }
                    }
                }
                input {
                    class: "lib-filter", r#type: "search", placeholder: "Filter your library",
                    value: "{filter}", oninput: move |e| filter.set(e.value()),
                }
                select {
                    class: "lib-sort", value: "{sort().key()}",
                    onchange: move |e| sort.set(LibrarySort::from_key(&e.value())),
                    for o in LibrarySort::ALL {
                        option { key: "{o.key()}", value: "{o.key()}", selected: sort() == o, "{o.label()}" }
                    }
                }
            }
            if order.is_empty() {
                p { class: "muted", style: "margin: 28px 0", "Nothing here matches. Try another filter." }
            }
            div { class: "poster-grid",
                for &i in order.iter() {
                    {
                        let s = &library.series[i];
                        let open_id = s.id.clone();
                        let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                        let cover_key = saved_cover_key(&s.id);
                        let chapters = s.chapters.len();
                        let size = human_bytes(s.bytes());
                        let sub = if chapters == 0 {
                            "Not downloaded yet".to_string()
                        } else {
                            format!("{} · {size}", plural(chapters, "chapter"))
                        };
                        let queued = queue.series_has_work(&s.id);
                        let body = if chapters == 0 {
                            format!("{} will be taken off your list. Nothing is stored on this computer for it.", s.title)
                        } else {
                            format!(
                                "{} and its {} downloaded chapters ({size}) will be deleted from this computer. Files you already exported are kept.",
                                s.title, chapters
                            )
                        };
                        let modal = Modal {
                            title: format!("Remove {} from your library?", s.title),
                            body,
                            confirm: "Remove".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::DeleteDownloadedSeries { series_id: s.id.clone() },
                        };
                        rsx! {
                            div { class: "poster", key: "{s.id}",
                                div { class: "poster-art", title: "Open {s.title}",
                                    onclick: move |_| open_series_from_library(state, &open_id),
                                    if has_cover { Cover { key_id: cover_key } } else { div { class: "cover empty", "📖" } }
                                    span { class: if chapters == 0 { "poster-badge saved" } else { "poster-badge" },
                                        if chapters == 0 { "Saved" } else { "{chapters} ch" }
                                    }
                                    if queued {
                                        span { class: "poster-badge queued", title: "Downloading",
                                            Icon { name: "download", size: 12 }
                                        }
                                    }
                                    button {
                                        class: "poster-remove", title: "Remove from your library",
                                        onclick: move |e| { e.stop_propagation(); state.ask(modal.clone()); },
                                        Icon { name: "x", size: 14 }
                                    }
                                }
                                strong { class: "poster-title", title: "{s.title}", "{s.title}" }
                                span { class: "muted small", "{sub}" }
                            }
                        }
                    }
                }
            }
        }

        div { class: "lib-exports",
            h2 { class: "q-title", "Exported files" }
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
