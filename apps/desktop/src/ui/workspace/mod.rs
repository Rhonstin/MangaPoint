//! One series, worked through in steps: Chapters, Downloads, Preview, Export.

mod chapters;
mod downloads;
mod export;
mod preview;

use super::widgets::{cover_key, Chip, Cover, EmptyState, Icon};
use crate::logic::{tab_blocked, Needs};
use crate::state::{AppState, JobKind, Page, SeriesTab};
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
    let downloading = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.kind == JobKind::Download && j.result.is_none());

    let back_label = if ws.origin == Page::Library {
        "Library"
    } else {
        "Results"
    };
    let origin = ws.origin;
    let cover = ws
        .ranked
        .as_ref()
        .map(|r| cover_key(&r.best().candidate.source_id, r.best().candidate.manga_id));
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
            if let Some(key) = cover {
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
        }

        div { class: "tabs",
            Tab { tab: SeriesTab::Chapters, icon: "list-checks", label: "Chapters", active: ws.tab == SeriesTab::Chapters,
                blocked: tab_blocked(Needs::Nothing, selected, downloaded, downloading),
                badge: (selected > 0).then(|| selected.to_string()) }
            Tab { tab: SeriesTab::Downloads, icon: "download", label: "Downloads", active: ws.tab == SeriesTab::Downloads,
                blocked: tab_blocked(Needs::SelectionOrDownloads, selected, downloaded, downloading),
                badge: (downloaded > 0).then(|| downloaded.to_string()) }
            Tab { tab: SeriesTab::Preview, icon: "eye", label: "Preview", active: ws.tab == SeriesTab::Preview,
                blocked: tab_blocked(Needs::Downloads, selected, downloaded, downloading), badge: None }
            Tab { tab: SeriesTab::Export, icon: "file-down", label: "Export", active: ws.tab == SeriesTab::Export,
                blocked: tab_blocked(Needs::Downloads, selected, downloaded, downloading), badge: None }
        }

        {match ws.tab {
            SeriesTab::Chapters => rsx! { chapters::ChaptersTab {} },
            SeriesTab::Downloads => rsx! { downloads::DownloadsTab {} },
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
