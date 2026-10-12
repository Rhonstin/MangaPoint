//! Choosing what to download from one or more library series, before it goes
//! to the download queue: each series has its own list of chapters to tick.

use super::widgets::{Cover, EmptyState, Icon};
use super::work::{commit_plan, downloaded_keys, load_saved_cover, merged_key, saved_cover_key};
use crate::logic::{self, plural};
use crate::state::{AppState, Page, PlanChapters};
use dioxus::prelude::*;

#[component]
pub fn PlanPage() -> Element {
    let state = use_context::<AppState>();
    let entries = state.plan.read().clone();
    let ids: Vec<String> = entries.iter().map(|e| e.series_id.clone()).collect();
    {
        let ids = ids.clone();
        use_effect(move || {
            for id in &ids {
                load_saved_cover(state, id);
            }
        });
    }
    let chapters: usize = entries.iter().map(|e| e.picked.len()).sum();
    let with_picks = entries.iter().filter(|e| !e.picked.is_empty()).count();
    let loading = entries
        .iter()
        .filter(|e| matches!(e.chapters, PlanChapters::Loading))
        .count();

    if entries.is_empty() {
        return rsx! {
            EmptyState {
                title: "Nothing to plan",
                message: "Select series in your library and choose Download.",
                shelf: true,
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.go(Page::Library), "Back to the library" }
            }
        };
    }

    rsx! {
        button { class: "btn ghost small back", onclick: move |_| {
                let mut plan = state.plan;
                plan.set(Vec::new());
                state.go(Page::Library);
            },
            Icon { name: "arrow-left", size: 14 } "Library"
        }
        h1 { "Choose what to download" }
        p { class: "muted",
            "Tick the chapters you want from each series. They go to the download queue and download one series after another."
        }
        for id in ids.iter() {
            PlanSection { key: "{id}", series_id: id.clone() }
        }
        div { class: "action-bar",
            div { class: "bar-count",
                strong { "{plural(chapters, \"chapter\")} chosen" }
                span { class: "muted small",
                    if with_picks > 0 { "from {with_picks} of {entries.len()} series" }
                    if with_picks == 0 && loading > 0 { "Loading the chapter lists…" }
                    if with_picks == 0 && loading == 0 { "Tick some chapters" }
                }
            }
            button { class: "btn", onclick: move |_| {
                    let mut plan = state.plan;
                    plan.set(Vec::new());
                    state.go(Page::Library);
                },
                "Cancel"
            }
            button { class: "btn primary", disabled: chapters == 0, onclick: move |_| commit_plan(state),
                Icon { name: "download", size: 16 } "Add to downloads"
            }
        }
    }
}

#[component]
fn PlanSection(series_id: String) -> Element {
    let state = use_context::<AppState>();
    let Some(entry) = state
        .plan
        .read()
        .iter()
        .find(|e| e.series_id == series_id)
        .cloned()
    else {
        return rsx! {};
    };
    let has_cover = state
        .library
        .read()
        .find(&series_id)
        .and_then(|s| s.info.as_ref())
        .is_some_and(|i| i.has_cover);
    let have = downloaded_keys(state, &series_id);
    let queue = state.queue.read().clone();

    // Every chapter: its text, and whether it can still be picked.
    let loaded = match &entry.chapters {
        PlanChapters::Ready(loaded) => Some(loaded.clone()),
        _ => None,
    };
    let rows: Vec<(usize, String, String, &'static str)> = loaded
        .as_ref()
        .map(|l| {
            l.merged
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let key = merged_key(c);
                    let tag = if have.contains(&key) {
                        "downloaded"
                    } else if queue.chapter_state(&series_id, &key).is_some() {
                        "in the queue"
                    } else {
                        ""
                    };
                    (
                        i,
                        c.number
                            .map(logic::format_number)
                            .unwrap_or_else(|| "–".into()),
                        c.name.clone(),
                        tag,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let open_positions: Vec<usize> = rows
        .iter()
        .filter(|(_, _, _, tag)| tag.is_empty())
        .map(|(i, ..)| *i)
        .collect();
    let new_count = open_positions.len();
    let total = rows.len();
    let picked = entry.picked.len();

    let id_all = series_id.clone();
    let id_none = series_id.clone();
    let id_latest = series_id.clone();
    let id_toggle = series_id.clone();
    let all_positions = open_positions.clone();
    let latest_positions: Vec<usize> = open_positions.iter().rev().take(10).copied().collect();

    let toggle_title = if entry.open {
        "Hide the chapters"
    } else {
        "Show the chapters"
    };
    let toggle_icon = if entry.open {
        "chevron-up"
    } else {
        "chevron-down"
    };
    let set_picks = move |id: &str, picks: Vec<usize>| {
        let mut plan = state.plan;
        let mut entries = plan.write();
        if let Some(e) = entries.iter_mut().find(|e| e.series_id == id) {
            e.picked = picks.into_iter().collect();
        }
    };
    let set_picks_all = set_picks;
    let set_picks_none = set_picks;

    rsx! {
        div { class: "plan-section",
            div { class: "plan-head",
                div { class: "plan-cover",
                    if has_cover { Cover { key_id: saved_cover_key(&series_id) } } else { div { class: "cover empty", "📖" } }
                }
                div { class: "grow",
                    strong { class: "q-name", "{entry.title}" }
                    div { class: "muted small",
                        match &entry.chapters {
                            PlanChapters::Loading => rsx! { span { class: "spin", Icon { name: "loader-circle", size: 13 } } " Loading chapters…" },
                            PlanChapters::Failed(message) => rsx! { "{message}" },
                            PlanChapters::Ready(_) => rsx! {
                                "{plural(new_count, \"new chapter\")} of {total}"
                                if picked > 0 { " · {picked} chosen" }
                            },
                        }
                    }
                }
                if loaded.is_some() && new_count > 0 {
                    button { class: "btn small", onclick: move |_| set_picks_all(&id_all, all_positions.clone()), "All new" }
                    button { class: "btn small", onclick: move |_| set_picks(&id_latest, latest_positions.clone()), "Latest 10" }
                    button { class: "btn small ghost", onclick: move |_| set_picks_none(&id_none, Vec::new()), "None" }
                }
                if loaded.is_some() {
                    button {
                        class: "btn small ghost", title: toggle_title,
                        onclick: move |_| {
                            let mut plan = state.plan;
                            let mut entries = plan.write();
                            if let Some(e) = entries.iter_mut().find(|e| e.series_id == id_toggle) {
                                e.open = !e.open;
                            }
                        },
                        Icon { name: toggle_icon, size: 16 }
                    }
                }
            }
            if entry.open && loaded.is_some() {
                div { class: "plan-list",
                    for (index, number, name, tag) in rows.iter().cloned() {
                        PlanRow {
                            key: "{index}", series_id: series_id.clone(), index,
                            on: entry.picked.contains(&index), number, name, tag,
                        }
                    }
                }
            }
        }
    }
}

/// One chapter in a series' list; its own component so ticking redraws only it.
#[component]
fn PlanRow(
    series_id: String,
    index: usize,
    on: bool,
    number: String,
    name: String,
    tag: &'static str,
) -> Element {
    let state = use_context::<AppState>();
    let disabled = !tag.is_empty();
    rsx! {
        label { class: if disabled { "plan-row off" } else { "plan-row" },
            input {
                r#type: "checkbox", checked: on, disabled,
                onchange: move |_| {
                    let mut plan = state.plan;
                    let mut entries = plan.write();
                    if let Some(e) = entries.iter_mut().find(|e| e.series_id == series_id) {
                        if !e.picked.remove(&index) {
                            e.picked.insert(index);
                        }
                    }
                },
            }
            span { class: "plan-num", "{number}" }
            span { class: "grow plan-name", "{name}" }
            if disabled { span { class: "muted small", "{tag}" } }
        }
    }
}
