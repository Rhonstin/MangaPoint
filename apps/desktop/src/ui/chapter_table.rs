//! The chapter table, shared by the Chapters tab and the download planner:
//! tick boxes with a select-all in the header, and sortable columns. It only
//! draws what it is given; whoever uses it decides what ticking and sorting do.

use super::widgets::{EmptyState, Icon};
use crate::logic::SortKey;
use dioxus::prelude::*;
use std::collections::HashSet;

/// One chapter as the table shows it.
#[derive(Clone, PartialEq)]
pub struct GridRow {
    /// Its position in the merged chapter list; what the handlers are given.
    pub index: usize,
    pub number: String,
    pub name: String,
    pub sources: usize,
    /// Names of the sources, shown when hovering the count.
    pub tip: String,
    pub date: String,
    pub saved: bool,
    pub queued: bool,
}

fn sort_icon(current: (SortKey, bool), column: SortKey) -> &'static str {
    if current.0 != column {
        "chevrons-up-down"
    } else if current.1 {
        "chevron-down"
    } else {
        "chevron-up"
    }
}

/// The table. `rows` are already filtered and in the order to show.
/// With `lock_done`, chapters that are downloaded or queued cannot be ticked.
#[component]
pub fn ChapterGrid(
    rows: Vec<GridRow>,
    picked: HashSet<usize>,
    sort: (SortKey, bool),
    lock_done: bool,
    /// A column heading was clicked.
    on_sort: EventHandler<SortKey>,
    /// One row was clicked.
    on_toggle: EventHandler<usize>,
    /// The header box: tick (`true`) or clear (`false`) all these rows.
    on_set: EventHandler<(Vec<usize>, bool)>,
    /// What to show when there are no rows.
    empty_message: String,
    /// Tallest the table gets before it scrolls, for example "360px".
    max_height: Option<String>,
) -> Element {
    let tickable: Vec<usize> = rows
        .iter()
        .filter(|r| !(lock_done && (r.saved || r.queued)))
        .map(|r| r.index)
        .collect();
    let all_on = !tickable.is_empty() && tickable.iter().all(|i| picked.contains(i));
    let none_to_tick = tickable.is_empty();
    let scroll_style = max_height
        .map(|h| format!("max-height: {h}"))
        .unwrap_or_default();

    let header = |key: SortKey, class: &'static str, label: &'static str| {
        let sorted = sort.0 == key;
        rsx! {
            th {
                class: if sorted { format!("{class} sortable sorted") } else { format!("{class} sortable") },
                onclick: move |_| on_sort.call(key),
                span { class: "th", "{label}" Icon { name: sort_icon(sort, key), size: 14 } }
            }
        }
    };

    rsx! {
        div { class: "table-wrap",
            div { class: "table-scroll", style: "{scroll_style}",
                table { class: "chapters",
                    thead {
                        tr {
                            th { class: "check",
                                input {
                                    r#type: "checkbox", checked: all_on, disabled: none_to_tick,
                                    title: "Tick or clear everything shown",
                                    onclick: move |_| on_set.call((tickable.clone(), !all_on)),
                                }
                            }
                            {header(SortKey::Number, "num", "No.")}
                            {header(SortKey::Name, "", "Title")}
                            th { class: "saved", title: "Downloaded, or in the queue" }
                            {header(SortKey::Sources, "src", "Sources")}
                            {header(SortKey::Date, "date", "Updated")}
                        }
                    }
                    tbody {
                        for r in rows.iter().cloned() {
                            ChapterRow {
                                key: "{r.index}",
                                on: picked.contains(&r.index),
                                locked: lock_done && (r.saved || r.queued),
                                row: r,
                                on_toggle,
                            }
                        }
                    }
                }
                if rows.is_empty() {
                    EmptyState { title: "No chapters match", message: empty_message, compact: true }
                }
            }
        }
    }
}

/// One row, a component of its own so that ticking a chapter redraws that
/// row and not the whole table.
#[component]
fn ChapterRow(row: GridRow, on: bool, locked: bool, on_toggle: EventHandler<usize>) -> Element {
    let index = row.index;
    let class = match (on, locked) {
        (_, true) => "locked",
        (true, _) => "on",
        _ => "",
    };
    rsx! {
        tr {
            class: "{class}",
            onclick: move |_| if !locked { on_toggle.call(index) },
            td { class: "check", input { r#type: "checkbox", checked: on, disabled: locked, readonly: true, style: "pointer-events: none" } }
            td { class: "num", "{row.number}" }
            td { class: "name", title: "{row.name}", "{row.name}" }
            td { class: "saved",
                if row.saved { span { title: "Downloaded", Icon { name: "circle-check", size: 15 } } }
                else if row.queued { span { title: "In the download queue", Icon { name: "clock-3", size: 15 } } }
            }
            td { class: "src", title: "{row.tip}", "{row.sources}" }
            td { class: "date", "{row.date}" }
        }
    }
}
