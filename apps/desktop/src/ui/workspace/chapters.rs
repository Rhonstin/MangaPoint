//! Tab 1: pick the chapters you want.

use crate::logic::{self, SortKey};
use crate::state::{AppState, ChaptersState, Page, SeriesTab};
use crate::ui::search::run_search;
use crate::ui::widgets::{Chip, EmptyState, Icon};
use crate::ui::work::{downloaded_keys, merged_key, queue_download};
use dioxus::prelude::*;
use pipeline::SourceChoice;
use search::missing_numbers;
use std::collections::HashSet;

/// The arrow shown in a column header.
fn sort_icon(current: (SortKey, bool), column: SortKey) -> &'static str {
    if current.0 != column {
        "chevrons-up-down"
    } else if current.1 {
        "chevron-down"
    } else {
        "chevron-up"
    }
}

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
    let all_visible_picked = !order.is_empty() && order.iter().all(|i| selected_now.contains(i));
    let current_sort = (sort_key, sort_desc);
    let order_for_header = order.clone();
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

    let rows: Vec<Element> = order
        .iter()
        .map(|&i| {
            let ch = &loaded.merged[i];
            let tip = ch
                .options
                .iter()
                .filter_map(|o| loaded.member_names.get(o.member).cloned())
                .collect::<Vec<_>>()
                .join(", ");
            rsx! {
                ChapterRow {
                    key: "{i}",
                    index: i,
                    on: selected_now.contains(&i),
                    saved: have.contains(&merged_key(ch)),
                    queued: queued(&merged_key(ch)),
                    number: ch.number.map(logic::format_number).unwrap_or_else(|| "–".to_string()),
                    name: ch.name.clone(),
                    sources: ch.options.len(),
                    tip,
                    date: logic::format_date(logic::latest_upload(ch)),
                }
            }
        })
        .collect();

    let header = |key: SortKey, class: &'static str, label: &'static str| {
        let mut table = state.table;
        let sorted = sort_key == key;
        rsx! {
            th {
                class: if sorted { format!("{class} sortable sorted") } else { format!("{class} sortable") },
                onclick: move |_| {
                    let cur = table.peek().sort;
                    table.write().sort = logic::next_sort(cur, key);
                },
                span { class: "th", "{label}" Icon { name: sort_icon(current_sort, key), size: 14 } }
            }
        }
    };

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
        div { class: "table-wrap",
            div { class: "table-scroll",
                table { class: "chapters",
                    thead {
                        tr {
                            th { class: "check",
                                input {
                                    r#type: "checkbox", checked: all_visible_picked,
                                    title: "Select or clear everything shown",
                                    onclick: move |_| {
                                        let mut s = selected.write();
                                        if all_visible_picked {
                                            for i in &order_for_header { s.remove(i); }
                                        } else {
                                            for i in &order_for_header { s.insert(*i); }
                                        }
                                    },
                                }
                            }
                            {header(SortKey::Number, "num", "No.")}
                            {header(SortKey::Name, "", "Title")}
                            th { class: "saved", title: "Already downloaded" }
                            {header(SortKey::Sources, "src", "Sources")}
                            {header(SortKey::Date, "date", "Updated")}
                        }
                    }
                    tbody { {rows.into_iter()} }
                }
                if order.is_empty() {
                    EmptyState {
                        title: "No chapters match",
                        message: format!("Nothing matches “{filter}”. Try fewer words."),
                        compact: true,
                    }
                }
            }
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

/// One row of the chapter table, a component of its own so that ticking a
/// chapter redraws that row and not the whole table.
#[component]
fn ChapterRow(
    index: usize,
    on: bool,
    saved: bool,
    queued: bool,
    number: String,
    name: String,
    sources: usize,
    tip: String,
    date: String,
) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        tr {
            class: if on { "on" } else { "" },
            onclick: move |_| {
                let mut selected = state.selected;
                let mut s = selected.write();
                if !s.remove(&index) { s.insert(index); }
            },
            td { class: "check", input { r#type: "checkbox", checked: on, readonly: true, style: "pointer-events: none" } }
            td { class: "num", "{number}" }
            td { class: "name", title: "{name}", "{name}" }
            td { class: "saved",
                if saved { Icon { name: "circle-check", size: 15 } }
                else if queued { span { title: "In the download queue", Icon { name: "clock-3", size: 15 } } }
            }
            td { class: "src", title: "{tip}", "{sources}" }
            td { class: "date", "{date}" }
        }
    }
}
