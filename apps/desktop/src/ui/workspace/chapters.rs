//! Tab 1: pick the chapters you want.

use crate::logic::{self, SortKey};
use crate::state::{AppState, ChaptersState, Page, SeriesTab};
use crate::ui::search::run_search;
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{downloaded_keys, merged_key, start_download};
use dioxus::prelude::*;
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
    let to_download = picked - already;
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
            let on = selected_now.contains(&i);
            let saved = have.contains(&merged_key(ch));
            let number = ch.number.map(logic::format_number).unwrap_or_else(|| "–".to_string());
            let date = logic::format_date(logic::latest_upload(ch));
            let count = ch.options.len();
            let tip = ch
                .options
                .iter()
                .filter_map(|o| loaded.member_names.get(o.member).cloned())
                .collect::<Vec<_>>()
                .join(", ");
            let name = ch.name.clone();
            let mut selected = state.selected;
            rsx! {
                tr {
                    key: "{i}",
                    class: if on { "on" } else { "" },
                    onclick: move |_| {
                        let mut s = selected.write();
                        if !s.remove(&i) { s.insert(i); }
                    },
                    td { class: "check", input { r#type: "checkbox", checked: on, readonly: true, style: "pointer-events: none" } }
                    td { class: "num", "{number}" }
                    td { class: "name", title: "{name}", "{name}" }
                    td { class: "saved", if saved { Icon { name: "circle-check", size: 15 } } }
                    td { class: "src", title: "{tip}", "{count}" }
                    td { class: "date", "{date}" }
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

        div { class: "action-bar",
            div { class: "grow",
                strong { "{picked} selected" }
                if already > 0 { span { class: "muted", " · {already} already downloaded" } }
            }
            if picked == 0 {
                span { class: "muted small", "Tick the chapters you want to read." }
            } else if to_download == 0 {
                button { class: "btn primary", onclick: move |_| {
                    state.open_tab(SeriesTab::Preview);
                    crate::ui::work::prepare_preview(state);
                },
                    Icon { name: "eye", size: 16 } "Preview them"
                }
            } else {
                button { class: "btn primary", onclick: move |_| start_download(state),
                    Icon { name: "download", size: 16 } "Download {to_download} chapter(s)"
                }
            }
        }
    }
}
