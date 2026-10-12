//! Tab 0: what the series is, and what you have of it.

use crate::logic::{self, ago, describe_settings, human_bytes};
use crate::state::{now_secs, AppState};
use crate::ui::widgets::{Chip, Cover, EmptyState, Icon};
use crate::ui::work::{ensure_series_info, load_saved_cover, saved_cover_key};
use dioxus::prelude::*;

#[component]
pub fn OverviewTab() -> Element {
    let state = use_context::<AppState>();
    let mut expanded = use_signal(|| false);

    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    // Make sure the details and cover are there; both are no-ops once they are.
    {
        let id = ws.series_id.clone();
        use_effect(move || {
            ensure_series_info(state, &id);
            load_saved_cover(state, &id);
        });
    }
    let Some(series) = state.library.read().find(&ws.series_id).cloned() else {
        return rsx! {
            EmptyState {
                title: "Nothing downloaded yet",
                message: "Download a chapter and the details of this series will show up here.",
            }
        };
    };

    let range = logic::number_range(series.chapters.iter().map(|c| c.number));
    let range_text = match range {
        Some((lo, hi)) if lo != hi => {
            format!(
                "{} to {}",
                logic::format_number(lo),
                logic::format_number(hi)
            )
        }
        Some((lo, _)) => logic::format_number(lo),
        None => String::new(),
    };
    // Chapters per source, most first.
    let mut per_source: Vec<(String, usize)> = Vec::new();
    for chapter in &series.chapters {
        let name = series
            .info
            .as_ref()
            .and_then(|i| i.source_name(&chapter.source_id))
            .map(str::to_string)
            .unwrap_or_else(|| "another source".to_string());
        match per_source.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => per_source.push((name, 1)),
        }
    }
    per_source.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let sources_text = per_source
        .iter()
        .map(|(name, n)| format!("{name} ({n})"))
        .collect::<Vec<_>>()
        .join(" · ");

    let info = series.info.clone();
    let loading = state.info_busy.read().contains(&ws.series_id);
    let cover_key = saved_cover_key(&series.id);
    let has_cover = info.as_ref().is_some_and(|i| i.has_cover);
    let description = info
        .as_ref()
        .map(|i| i.description.clone())
        .unwrap_or_default();
    let long_text = description.chars().count() > 360;
    let by_line = info
        .as_ref()
        .map(|i| match (i.author.is_empty(), i.artist.is_empty()) {
            (false, false) if i.author != i.artist => {
                format!("By {} · Art by {}", i.author, i.artist)
            }
            (false, _) => format!("By {}", i.author),
            (true, false) => format!("Art by {}", i.artist),
            (true, true) => String::new(),
        });

    rsx! {
        div { class: "overview",
            div { class: "overview-cover",
                if has_cover { Cover { key_id: cover_key } } else { div { class: "cover empty", "📖" } }
            }
            div { class: "overview-body",
                div { class: "row wrap",
                    if let Some(i) = &info {
                        if !i.status.is_empty() { Chip { text: i.status.clone(), kind: Some("accent".to_string()) } }
                    }
                }
                if let Some(line) = by_line.filter(|l| !l.is_empty()) {
                    p { class: "muted", style: "margin: 10px 0 0", "{line}" }
                }
                if let Some(i) = &info {
                    if !i.genres.is_empty() {
                        div { class: "row wrap", style: "margin-top: 10px",
                            for g in i.genres.iter() { Chip { key: "{g}", text: g.clone() } }
                        }
                    }
                }
                if !description.is_empty() {
                    p { class: if expanded() || !long_text { "description" } else { "description clamp" }, "{description}" }
                    if long_text {
                        button { class: "btn ghost small", onclick: move |_| expanded.set(!expanded()),
                            if expanded() { "Show less" } else { "Read more" }
                        }
                    }
                } else if loading {
                    p { class: "muted", style: "margin-top: 12px", span { class: "spin", Icon { name: "loader-circle", size: 14 } } " Loading the details…" }
                } else if info.is_none() {
                    p { class: "muted small", style: "margin-top: 12px",
                        "The details of this series haven't been loaded yet. They need the catalog to be running."
                    }
                    button { class: "btn small", onclick: move |_| ensure_series_info(state, &ws.series_id),
                        Icon { name: "refresh-cw", size: 14 } "Load details"
                    }
                }

                if series.chapters.is_empty() {
                    p { class: "muted", style: "margin: 14px 0 0",
                        "Not downloaded yet. Use the Chapters tab to choose what to download."
                    }
                }
                div { class: "facts",
                    div { class: "summary-line", span { class: "muted", "Chapters" } span { "{series.chapters.len()}" if series.chapters.len() > 1 && !range_text.is_empty() { " (numbered {range_text})" } } }
                    div { class: "summary-line", span { class: "muted", "Pages" } span { "{series.page_count()}" } }
                    div { class: "summary-line", span { class: "muted", "On this computer" } span { "{human_bytes(series.bytes())}" } }
                    if !sources_text.is_empty() {
                        div { class: "summary-line", span { class: "muted", "From" } span { class: "right", "{sources_text}" } }
                    }
                    div { class: "summary-line", span { class: "muted", "Converted as" } span { class: "right", "{describe_settings(&series.settings)}" } }
                    if series.updated_at > 0 {
                        div { class: "summary-line", span { class: "muted", "Last downloaded" } span { "{ago(now_secs(), series.updated_at)}" } }
                    }
                }
            }
        }
    }
}
