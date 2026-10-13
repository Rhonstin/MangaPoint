//! One series, worked through in steps: Chapters, Downloads, Preview, Export.

mod chapters;
mod crop_editor;
mod downloads;
mod export;
mod overview;
mod preview;

use super::widgets::{cover_key, Chip, Cover, EmptyState, Icon};
use super::work::{add_open_series_to_library, load_saved_cover, saved_cover_key};
use crate::logic::{tab_blocked, Needs};
use crate::state::{AppState, Modal, ModalAction, Page, SeriesTab};
use dioxus::prelude::*;

#[component]
pub fn WorkspacePage() -> Element {
    let state = use_context::<AppState>();
    let ws = state.workspace.read().clone();
    let Some(ws) = ws else {
        return rsx! {
            EmptyState {
                title: "No series open",
                message: "Pick one from the search results, or open one from your Library.",
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.go(Page::Search), Icon { name: "arrow-left" } "Back to search" }
            }
        };
    };

    let (downloaded, library_bytes) = state
        .library
        .read()
        .find(&ws.series_id)
        .map(|s| (s.chapters.len(), s.bytes()))
        .unwrap_or((0, 0));
    let selected = state.selected.read().len();
    let in_library = state.library.read().find(&ws.series_id).is_some();
    let remove_body = if downloaded > 0 {
        format!(
            "{} and its {} downloaded chapters ({}) will be deleted from this computer. Files you already exported are kept. To keep it in the library, delete just the chapters on the Chapters or Downloads tab.",
            ws.title,
            downloaded,
            crate::logic::human_bytes(library_bytes)
        )
    } else {
        format!(
            "{} will be taken off your list. Nothing is stored on this computer for it.",
            ws.title
        )
    };
    let remove_modal = Modal {
        title: format!("Remove {} from your library?", ws.title),
        body: remove_body,
        confirm: "Remove".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::RemoveSeries {
            series_ids: vec![ws.series_id.clone()],
        },
    };

    let back_label = if ws.origin == Page::Library {
        "Library"
    } else {
        "Results"
    };
    let origin = ws.origin;
    // The cover from the search that found it, or the one saved with the series.
    let saved_cover = state
        .library
        .read()
        .find(&ws.series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let cover = ws
        .ranked
        .as_ref()
        .map(|r| cover_key(&r.best().candidate.source_id, r.best().candidate.manga_id))
        .or_else(|| saved_cover.then(|| saved_cover_key(&ws.series_id)));
    {
        let id = ws.series_id.clone();
        use_effect(move || load_saved_cover(state, &id));
    }
    let source_chips: Vec<String> = ws
        .ranked
        .as_ref()
        .map(|r| {
            crate::logic::one_per_source(&r.members, 4)
                .iter()
                .map(|m| m.candidate.source_name.clone())
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        button { class: "btn ghost small", onclick: move |_| state.go(origin),
            Icon { name: "arrow-left", size: 16 }
            "{back_label}"
        }
        div { class: "spacer" }
        div { class: "page-head",
            // The Overview tab shows the cover large, so it is not repeated here.
            if let Some(key) = cover.filter(|_| ws.tab != SeriesTab::Overview) {
                div { style: "width: 72px; flex: none; border-radius: 8px; overflow: hidden", Cover { key_id: key } }
            }
            div { class: "grow",
                h1 { "{ws.title}" }
                div { class: "row wrap",
                    for (i, name) in source_chips.iter().enumerate() {
                        Chip {
                            key: "{i}",
                            text: if i == 0 { format!("{name} · best") } else { name.clone() },
                            kind: if i == 0 { Some("accent".to_string()) } else { None },
                        }
                    }
                    if downloaded > 0 {
                        Chip { text: format!("{downloaded} downloaded · {}", crate::logic::human_bytes(library_bytes)), kind: Some("ok".to_string()) }
                    }
                }
            }
            if in_library {
                button { class: "btn small", title: "Remove it from your library",
                    onclick: move |_| state.ask(remove_modal.clone()),
                    Icon { name: "bookmark-check", size: 15 } "In your library"
                }
            } else {
                button { class: "btn primary small", title: "Keep it in your library without downloading anything",
                    onclick: move |_| add_open_series_to_library(state),
                    Icon { name: "bookmark-plus", size: 15 } "Add to library"
                }
            }
        }

        div { class: "tabs",
            Tab { tab: SeriesTab::Overview, icon: "info", label: "Overview", active: ws.tab == SeriesTab::Overview,
                blocked: if in_library { None } else { Some("Add this series to your library first") }, badge: None }
            Tab { tab: SeriesTab::Chapters, icon: "list-checks", label: "Chapters", active: ws.tab == SeriesTab::Chapters,
                blocked: tab_blocked(Needs::Nothing, downloaded),
                badge: (selected > 0).then(|| selected.to_string()).or_else(|| (downloaded > 0).then(|| downloaded.to_string())) }
            Tab { tab: SeriesTab::Preview, icon: "eye", label: "Preview", active: ws.tab == SeriesTab::Preview,
                blocked: tab_blocked(Needs::Downloads, downloaded), badge: None }
            Tab { tab: SeriesTab::Export, icon: "file-down", label: "Export", active: ws.tab == SeriesTab::Export,
                blocked: tab_blocked(Needs::Downloads, downloaded), badge: None }
        }

        {match ws.tab {
            SeriesTab::Overview => rsx! { overview::OverviewTab {} },
            SeriesTab::Chapters => rsx! { chapters::ChaptersTab {} },
            SeriesTab::Preview => rsx! { preview::PreviewTab {} },
            SeriesTab::Export => rsx! { export::ExportTab {} },
        }}
    }
}

/// One tab. A tab whose step cannot work yet is greyed out and says why.
#[component]
fn Tab(
    tab: SeriesTab,
    icon: &'static str,
    label: &'static str,
    active: bool,
    blocked: Option<&'static str>,
    badge: Option<String>,
) -> Element {
    let state = use_context::<AppState>();
    let class = if active { "tab on" } else { "tab" };
    let hint = blocked.unwrap_or("");
    rsx! {
        button {
            class: "{class}",
            disabled: blocked.is_some(),
            title: "{hint}",
            // The Preview tab prepares itself when it opens.
            onclick: move |_| state.open_tab(tab),
            Icon { name: icon, size: 16 }
            span { "{label}" }
            if let Some(b) = badge { span { class: "badge", "{b}" } }
        }
    }
}
