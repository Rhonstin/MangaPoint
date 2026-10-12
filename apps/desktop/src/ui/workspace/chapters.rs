//! Tab 1: pick the chapters you want.

use crate::logic::{self, plural};
use crate::state::{AppState, ChaptersState, Modal, ModalAction, Page, SeriesTab};
use crate::ui::chapter_table::{ChapterGrid, GridRow};
use crate::ui::search::run_search;
use crate::ui::widgets::{Chip, EmptyState, Icon};
use crate::ui::work::{downloaded_keys, merged_key, queue_download};
use dioxus::prelude::*;
use pipeline::SourceChoice;
use search::missing_numbers;
use std::collections::HashSet;

#[component]
pub fn ChaptersTab() -> Element {
    let state = use_context::<AppState>();
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);

    let loaded = match &*state.chapters.read() {
        ChaptersState::Idle => {
            let title = ws.title.clone();
            return rsx! {
                EmptyState {
                    title: "This series is in your Library",
                    message: "Its chapters come from your sources. Search for it to download more.",
                }
                div { style: "text-align: center",
                    button {
                        class: "btn primary",
                        onclick: move |_| {
                            run_search(state, title.clone());
                            state.go(Page::Search);
                        },
                        Icon { name: "search", size: 16 }
                        "Search for more chapters"
                    }
                }
            };
        }
        ChaptersState::Loading => {
            return rsx! {
                div { class: "card row",
                    Icon { name: "loader-circle", size: 18 }
                    span { class: "muted", "Loading chapters from your sources…" }
                }
            };
        }
        ChaptersState::Failed(message) => {
            let message = message.clone();
            return rsx! { div { class: "banner bad", Icon { name: "circle-x" } "{message}" } };
        }
        ChaptersState::Ready(loaded) => loaded.clone(),
    };

    let have = downloaded_keys(state, &ws.series_id);
    let queue = state.queue.read().clone();
    let queued =
        |key: &str| !have.contains(key) && queue.chapter_state(&ws.series_id, key).is_some();
    let total = loaded.merged.len();
    let numbers: Vec<Option<f64>> = loaded.merged.iter().map(|m| m.number).collect();
    let missing = missing_numbers(&loaded.merged);
    let (filter, (sort_key, sort_desc)) = {
        let t = state.table.read();
        (t.filter.clone(), t.sort)
    };
    let order = logic::visible_chapters(&loaded.merged, &filter, sort_key, sort_desc);
    let selected_now = state.selected.read().clone();
    let picked = selected_now.len();
    let already = selected_now
        .iter()
        .filter(|&&i| {
            loaded
                .merged
                .get(i)
                .is_some_and(|c| have.contains(&merged_key(c)))
        })
        .count();
    let queued_picked = selected_now
        .iter()
        .filter(|&&i| loaded.merged.get(i).is_some_and(|c| queued(&merged_key(c))))
        .count();
    let downloaded_picked: Vec<String> = selected_now
        .iter()
        .filter_map(|&i| loaded.merged.get(i).map(merged_key))
        .filter(|k| have.contains(k))
        .collect();
    let delete_modal = Modal {
        title: format!("Delete {}?", plural(downloaded_picked.len(), "downloaded chapter")),
        body: format!(
            "The downloaded pages of the chapters you ticked in {} will be deleted from this computer. The series stays in your library and you can download them again. Files you already exported are kept.",
            ws.title
        ),
        confirm: "Delete".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteChapters {
            series_id: ws.series_id.clone(),
            keys: downloaded_picked,
        },
    };
    // Picked chapters that are neither downloaded nor already in the queue.
    let to_download = picked - already - queued_picked;
    // Which sources can supply the chapters still to download, and how many each.
    let mut per_source: Vec<(String, String, usize)> = Vec::new();
    for &i in selected_now.iter() {
        let Some(ch) = loaded.merged.get(i) else {
            continue;
        };
        if have.contains(&merged_key(ch)) || queued(&merged_key(ch)) {
            continue;
        }
        let mut seen: Vec<&str> = Vec::new();
        for option in &ch.options {
            let Some(id) = loaded.member_sources.get(option.member) else {
                continue;
            };
            if seen.contains(&id.as_str()) {
                continue;
            }
            seen.push(id);
            match per_source.iter_mut().find(|(source, _, _)| source == id) {
                Some((_, _, n)) => *n += 1,
                None => {
                    let name = loaded
                        .member_names
                        .get(option.member)
                        .cloned()
                        .unwrap_or_else(|| id.clone());
                    per_source.push((id.clone(), name, 1));
                }
            }
        }
    }
    per_source.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    let choice_id = match &*state.download_source.read() {
        SourceChoice::Only(id) if per_source.iter().any(|(source, _, _)| source == id) => {
            id.clone()
        }
        _ => "adaptive".to_string(),
    };
    let not_offered = per_source
        .iter()
        .find(|(source, _, _)| *source == choice_id)
        .map_or(0, |(_, _, n)| to_download - n);
    let source_options: Vec<(String, String)> = {
        let speeds = state.speeds.read();
        per_source
            .iter()
            .map(|(id, name, n)| {
                let speed = speeds
                    .speed(id)
                    .map_or(String::new(), |v| format!(" · about {v:.0} pages/s"));
                (id.clone(), format!("{name} · {n} of {to_download}{speed}"))
            })
            .collect()
    };
    let current_sort = (sort_key, sort_desc);
    let numbers_all = numbers.clone();
    let numbers_latest = numbers.clone();
    let numbers_range = numbers.clone();
    let missing_text = {
        let first: Vec<String> = missing.iter().take(12).map(|n| n.to_string()).collect();
        format!(
            "{}{}",
            first.join(", "),
            if missing.len() > 12 { " …" } else { "" }
        )
    };

    let grid_rows: Vec<GridRow> = order
        .iter()
        .map(|&i| {
            let ch = &loaded.merged[i];
            GridRow {
                index: i,
                number: ch
                    .number
                    .map(logic::format_number)
                    .unwrap_or_else(|| "–".to_string()),
                name: ch.name.clone(),
                sources: ch.options.len(),
                tip: ch
                    .options
                    .iter()
                    .filter_map(|o| loaded.member_names.get(o.member).cloned())
                    .collect::<Vec<_>>()
                    .join(", "),
                date: logic::format_date(logic::latest_upload(ch)),
                saved: have.contains(&merged_key(ch)),
                queued: queued(&merged_key(ch)),
            }
        })
        .collect();

    let mut selected = state.selected;
    let mut table = state.table;
    rsx! {
        if loaded.unreachable > 0 {
            div { class: "banner warn small",
                Icon { name: "triangle-alert", size: 16 }
                "{loaded.unreachable} source(s) did not answer, so some chapters may be missing."
            }
        }
        div { class: "toolbar",
            div { class: "input-icon", style: "max-width: 280px",
                Icon { name: "list-filter", size: 16 }
                input {
                    r#type: "search", placeholder: "Filter chapters",
                    value: "{filter}",
                    oninput: move |e| table.write().filter = e.value(),
                }
            }
            button { class: "btn small", onclick: move |_| selected.set((0..numbers_all.len()).collect()), "All" }
            button { class: "btn small", onclick: move |_| selected.set(logic::latest(&numbers_latest, 10).into_iter().collect()), "Latest 10" }
            button { class: "btn small", onclick: move |_| selected.set(HashSet::new()), "None" }
            form {
                class: "row", style: "gap: 6px; margin-left: auto",
                onsubmit: move |e| {
                    e.prevent_default();
                    match logic::select_by_text(&range_text.read(), &numbers_range) {
                        Ok(found) => { range_error.set(None); selected.set(found.into_iter().collect()); }
                        Err(msg) => range_error.set(Some(msg)),
                    }
                },
                input {
                    r#type: "text", style: "width: 190px",
                    placeholder: "Range: 1-20, 25, 30-",
                    value: "{range_text}",
                    oninput: move |e| range_text.set(e.value()),
                }
                button { class: "btn small", r#type: "submit", "Select" }
            }
        }
        if let Some(msg) = range_error() {
            p { class: "small", style: "color: var(--bad); margin: 0 0 8px", "{msg}" }
        }
        ChapterGrid {
            rows: grid_rows,
            picked: selected_now.clone(),
            sort: current_sort,
            lock_done: false,
            on_sort: move |key| {
                let current = table.peek().sort;
                table.write().sort = logic::next_sort(current, key);
            },
            on_toggle: move |index: usize| {
                let mut s = selected.write();
                if !s.remove(&index) {
                    s.insert(index);
                }
            },
            on_set: move |(indexes, on): (Vec<usize>, bool)| {
                let mut s = selected.write();
                for i in indexes {
                    if on {
                        s.insert(i);
                    } else {
                        s.remove(&i);
                    }
                }
            },
            empty_message: format!("Nothing matches “{filter}”. Try fewer words."),
            max_height: None,
        }
        p { class: "muted small", style: "margin: 8px 0 0",
            "Showing {order.len()} of {total} chapters. Click a heading to sort, a row to tick it."
            if !missing.is_empty() { " Not available from any source: {missing_text}." }
        }

        if picked > 0 {
            div { class: "action-bar",
                div { class: "bar-count",
                    strong { "{picked} selected" }
                    span { class: "muted small",
                        if already > 0 { "{already} downloaded" }
                        if already > 0 && queued_picked > 0 { " · " }
                        if queued_picked > 0 { "{queued_picked} in the queue" }
                    }
                }
                span { class: "grow" }
                if per_source.len() >= 2 && to_download > 0 {
                    div { class: "bar-source",
                        title: "Adaptive uses the source with the most of your chapters, tests which is fastest, and fills any gaps from the others. Pick one source to use only that one.",
                        Icon { name: "layers", size: 15 }
                        select {
                            value: "{choice_id}",
                            onchange: move |e| {
                                let mut source = state.download_source;
                                let value = e.value();
                                source.set(if value == "adaptive" { SourceChoice::Adaptive } else { SourceChoice::Only(value) });
                            },
                            option { value: "adaptive", selected: choice_id == "adaptive", "Adaptive (recommended)" }
                            for (id, label) in source_options.iter() {
                                option { key: "{id}", value: "{id}", selected: *id == choice_id, "{label}" }
                            }
                        }
                    }
                }
                if not_offered > 0 {
                    Chip { text: format!("{not_offered} not offered"), kind: Some("warn".to_string()) }
                }
                if already > 0 {
                    button { class: "btn danger", title: "Delete the downloaded pages of the chapters you ticked. The series stays in your library.",
                        onclick: move |_| state.ask(delete_modal.clone()),
                        Icon { name: "trash", size: 15 } "Delete {already} downloaded"
                    }
                }
                if to_download == 0 && queued_picked > 0 {
                    button { class: "btn primary", onclick: move |_| state.go(Page::Downloads),
                        Icon { name: "download", size: 16 } "View downloads"
                    }
                } else if to_download == 0 {
                    button { class: "btn primary", onclick: move |_| {
                        state.open_tab(SeriesTab::Preview);
                        crate::ui::work::prepare_preview(state);
                    },
                        Icon { name: "eye", size: 16 } "Preview them"
                    }
                } else {
                    button { class: "btn primary", onclick: move |_| queue_download(state),
                        Icon { name: "download", size: 16 } "Add {to_download - not_offered} to downloads"
                    }
                }
            }
        }
    }
}
