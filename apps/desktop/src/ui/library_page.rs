//! The Library: the series you keep (downloaded or saved for later). Exported
//! files are listed on the Downloads page.

use super::widgets::{open_folder, Cover, EmptyState, Icon};
use super::work::{
    ensure_series_info, load_saved_cover, open_plan, open_series_from_library, saved_cover_key,
};
use crate::logic::{human_bytes, library_order, plural, LibrarySort, LibraryView};
use crate::state::{library_files, AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;
use std::collections::HashSet;

#[component]
pub fn LibraryPage() -> Element {
    let state = use_context::<AppState>();
    let mut view = use_signal(LibraryView::default);
    let mut sort = use_signal(LibrarySort::default);
    let mut filter = use_signal(String::new);
    let mut picked = use_signal(HashSet::<String>::new);
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

    // The ticked series that are still shown, in the order shown.
    let chosen: Vec<&library::DownloadedSeries> = order
        .iter()
        .map(|&i| &library.series[i])
        .filter(|s| picked.read().contains(&s.id))
        .collect();
    let chosen_ids: Vec<String> = chosen.iter().map(|s| s.id.clone()).collect();
    let chosen_with_downloads: Vec<String> = chosen
        .iter()
        .filter(|s| !s.chapters.is_empty())
        .map(|s| s.id.clone())
        .collect();
    let chosen_bytes: u64 = chosen.iter().map(|s| s.bytes()).sum();
    let selecting = !chosen.is_empty();
    let all_shown: Vec<String> = order
        .iter()
        .map(|&i| library.series[i].id.clone())
        .collect();
    let all_ticked = selecting && chosen.len() == all_shown.len();

    let remove_modal = Modal {
        title: format!(
            "Remove {} from your library?",
            plural(chosen.len(), "series")
        ),
        body: if chosen_with_downloads.is_empty() {
            "They will be taken off your list. Nothing is stored on this computer for them."
                .to_string()
        } else {
            format!(
                "Their downloaded chapters ({}) will be deleted from this computer too. Files you already exported are kept. To keep them in the library, delete only the downloads instead.",
                human_bytes(chosen_bytes)
            )
        },
        confirm: "Remove".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::RemoveSeries {
            series_ids: chosen_ids.clone(),
        },
    };
    let delete_modal = Modal {
        title: format!(
            "Delete the downloads of {}?",
            plural(chosen_with_downloads.len(), "series")
        ),
        body: format!(
            "The downloaded chapters ({}) will be deleted from this computer. The series stay in your library, so you can download them again later. Files you already exported are kept.",
            human_bytes(chosen_bytes)
        ),
        confirm: "Delete downloads".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteDownloads {
            series_ids: chosen_with_downloads.clone(),
        },
    };

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
            button {
                class: "btn", title: "Open the folder the downloads are kept in",
                onclick: move |_| {
                    let root = library_files().root().to_path_buf();
                    let _ = std::fs::create_dir_all(&root);
                    open_folder(&root);
                },
                Icon { name: "folder-open", size: 16 } "Open folder"
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
            div { class: if selecting { "poster-grid selecting" } else { "poster-grid" },
                for &i in order.iter() {
                    {
                        let s = &library.series[i];
                        let open_id = s.id.clone();
                        let tick_id = s.id.clone();
                        let on = picked.read().contains(&s.id);
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
                        rsx! {
                            div { class: if on { "poster on" } else { "poster" }, key: "{s.id}",
                                div { class: "poster-art", title: "Open {s.title}",
                                    onclick: move |_| {
                                        if picked.read().is_empty() {
                                            open_series_from_library(state, &open_id);
                                        } else {
                                            let mut p = picked.write();
                                            if !p.remove(&open_id) { p.insert(open_id.clone()); }
                                        }
                                    },
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
                                        class: if on { "poster-check on" } else { "poster-check" },
                                        title: if on { "Deselect" } else { "Select" },
                                        onclick: move |e| {
                                            e.stop_propagation();
                                            let mut p = picked.write();
                                            if !p.remove(&tick_id) { p.insert(tick_id.clone()); }
                                        },
                                        if on { Icon { name: "check", size: 14 } }
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

        if selecting {
            div { class: "action-bar",
                div { class: "bar-count",
                    strong { "{chosen.len()} selected" }
                    span { class: "muted small", "{human_bytes(chosen_bytes)} on this computer" }
                }
                button { class: "btn ghost", onclick: move |_| {
                        if all_ticked { picked.write().clear(); } else { picked.set(all_shown.iter().cloned().collect()); }
                    },
                    if all_ticked { "Select none" } else { "Select all" }
                }
                button { class: "btn primary", onclick: {
                        let ids = chosen_ids.clone();
                        move |_| { picked.write().clear(); open_plan(state, ids.clone()); }
                    },
                    Icon { name: "download", size: 16 } "Download…"
                }
                if !chosen_with_downloads.is_empty() {
                    button { class: "btn danger", title: "Delete the downloaded chapters but keep the series",
                        onclick: move |_| { state.ask(delete_modal.clone()); picked.write().clear(); },
                        Icon { name: "trash", size: 15 } "Delete downloads"
                    }
                }
                button { class: "btn danger", title: "Take the series out of your library",
                    onclick: move |_| { state.ask(remove_modal.clone()); picked.write().clear(); },
                    Icon { name: "x", size: 15 } "Remove"
                }
                button { class: "btn ghost", title: "Clear the selection", onclick: move |_| picked.write().clear(),
                    Icon { name: "x", size: 15 }
                }
            }
        }
    }
}
