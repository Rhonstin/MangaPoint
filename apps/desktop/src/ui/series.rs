//! One series: a sortable chapter table and an export panel.

use super::jobs::start_job;
use super::widgets::{cover_key, Chip, Cover, EmptyState, Icon};
use crate::logic::{self, SortKey, TargetChoice};
use crate::state::{paths, AppState, Page};
use converter::xtc::Format;
use device_profiles::{Firmware, Support};
use dioxus::prelude::*;
use futures_util::future::join_all;
use pipeline::export::removable_drives;
use search::{merge_chapters, missing_numbers, MergedChapter, ScoredMember};
use std::collections::HashSet;
use std::time::Duration;
use tracing::info;

struct Loaded {
    merged: Vec<MergedChapter>,
    /// Source id of each member, best first; chapter options point into this.
    member_sources: Vec<String>,
    member_names: Vec<String>,
    unreachable: usize,
}

async fn load(state: AppState, members: Vec<ScoredMember>) -> Option<Loaded> {
    let client = state.client()?;
    let names: Vec<&str> = members
        .iter()
        .map(|m| m.candidate.source_name.as_str())
        .collect();
    info!("Loading chapter lists from {}", names.join(", "));
    let started = std::time::Instant::now();
    let fetches = members.iter().map(|m| {
        let client = client.clone();
        let id = m.candidate.manga_id;
        async move { tokio::time::timeout(Duration::from_secs(60), client.fetch_chapters(id)).await }
    });
    let mut lists = Vec::new();
    let mut unreachable = 0;
    for result in join_all(fetches).await {
        match result {
            Ok(Ok(chapters)) => lists.push(chapters),
            _ => {
                unreachable += 1;
                lists.push(Vec::new());
            }
        }
    }
    let merged = merge_chapters(&lists);
    let both = merged.iter().filter(|m| m.options.len() > 1).count();
    info!(
        "{} chapters in total ({both} available from more than one source) in {:.1}s{}",
        merged.len(),
        started.elapsed().as_secs_f32(),
        if unreachable > 0 {
            format!("; {unreachable} source(s) did not answer")
        } else {
            String::new()
        }
    );
    Some(Loaded {
        merged,
        member_sources: members
            .iter()
            .map(|m| m.candidate.source_id.clone())
            .collect(),
        member_names: members
            .iter()
            .map(|m| m.candidate.source_name.clone())
            .collect(),
        unreachable,
    })
}

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
pub fn SeriesPage() -> Element {
    let state = use_context::<AppState>();
    let series = state.series.read().clone();
    let Some(series) = series else {
        return rsx! {
            EmptyState {
                title: "No series picked",
                message: "Pick one from the search results and the cat will bring its chapters.",
            }
            div { style: "text-align: center",
                button { class: "btn", onclick: move |_| state.go(Page::Search), Icon { name: "arrow-left" } "Back to search" }
            }
        };
    };

    // One entry per source (a source can list a series twice), best first.
    let shown: Vec<ScoredMember> = logic::one_per_source(&series.members, 4);
    let members = shown.clone();
    let loaded = use_resource(move || {
        let members = members.clone();
        async move { load(state, members).await }
    });

    let mut selected = use_signal(HashSet::<usize>::new);
    let mut filter = use_signal(String::new);
    let mut sort = use_signal(|| (SortKey::Number, true));
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    let mut target = use_signal(|| TargetChoice::Folder);
    let mut drives = use_signal(removable_drives);

    let best = series.best().candidate.clone();
    let cover = cover_key(&best.source_id, best.manga_id);
    let settings = state.settings.read().clone();
    let format = settings.conversion.format;
    let support = settings.device.xtc_support(settings.firmware);
    let firmware_label = if settings.firmware == Firmware::CrossPoint {
        "CrossPoint"
    } else {
        "stock firmware"
    };
    let job_running = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.result.is_none());
    let folder_hint = settings
        .export
        .output_dir
        .clone()
        .unwrap_or_else(|| paths().default_export_dir())
        .display()
        .to_string();

    rsx! {
        button { class: "btn ghost small", onclick: move |_| state.go(Page::Search),
            Icon { name: "arrow-left", size: 16 }
            "Results"
        }
        div { class: "spacer" }
        div { class: "page-head",
            div { style: "width: 84px; flex: none; border-radius: 8px; overflow: hidden", Cover { key_id: cover } }
            div { class: "grow",
                h1 { "{series.title}" }
                div { class: "row wrap",
                    for (i, m) in shown.iter().enumerate() {
                        Chip {
                            key: "{i}",
                            text: if i == 0 { format!("{} · best", m.candidate.source_name) } else { m.candidate.source_name.clone() },
                            kind: if i == 0 { Some("accent".to_string()) } else { None },
                        }
                    }
                }
                p { class: "muted small", style: "margin: 8px 0 0",
                    "If a chapter fails on one source, MangaPoint automatically tries the next."
                }
            }
        }

        {match &*loaded.read() {
            None => rsx! {
                div { class: "card row",
                    Icon { name: "loader-circle", size: 18 }
                    span { class: "muted", "Loading chapters from {shown.len()} source(s)…" }
                }
            },
            Some(None) => rsx! { div { class: "banner bad", Icon { name: "circle-x" } "The catalog engine is not running." } },
            Some(Some(l)) => {
                let numbers: Vec<Option<f64>> = l.merged.iter().map(|m| m.number).collect();
                let total = l.merged.len();
                let missing = missing_numbers(&l.merged);
                let (sort_key, sort_desc) = *sort.read();
                let order = logic::visible_chapters(&l.merged, &filter.read(), sort_key, sort_desc);
                let picked = selected.read().len();
                let all_visible_picked = !order.is_empty() && order.iter().all(|i| selected.read().contains(i));
                let missing_text = {
                    let first: Vec<String> = missing.iter().take(12).map(|n| n.to_string()).collect();
                    format!("{}{}", first.join(", "), if missing.len() > 12 { " …" } else { "" })
                };
                let sources_used = l.member_names.join(", ");
                let numbers_all = numbers.clone();
                let numbers_latest = numbers.clone();
                let numbers_range = numbers.clone();
                let order_for_header = order.clone();
                let current_sort = (sort_key, sort_desc);

                let rows: Vec<Element> = order.iter().map(|&i| {
                    let ch = &l.merged[i];
                    let on = selected.read().contains(&i);
                    let number = ch.number.map(logic::format_number).unwrap_or_else(|| "–".to_string());
                    let date = logic::format_date(logic::latest_upload(ch));
                    let count = ch.options.len();
                    let tip = ch.options.iter()
                        .filter_map(|o| l.member_names.get(o.member).cloned())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let name = ch.name.clone();
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
                            td { class: "src", title: "{tip}", "{count}" }
                            td { class: "date", "{date}" }
                        }
                    }
                }).collect();

                rsx! {
                    if l.unreachable > 0 {
                        div { class: "banner warn small",
                            Icon { name: "triangle-alert", size: 16 }
                            "{l.unreachable} source(s) did not answer, so some chapters may be missing."
                        }
                    }
                    div { class: "series-layout",
                        // ---- chapters ----
                        div {
                            div { class: "toolbar",
                                div { class: "input-icon", style: "max-width: 280px",
                                    Icon { name: "list-filter", size: 16 }
                                    input {
                                        r#type: "search", placeholder: "Filter chapters",
                                        value: "{filter}",
                                        oninput: move |e| filter.set(e.value()),
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
                                                th { class: if sort_key == SortKey::Number { "num sortable sorted" } else { "num sortable" },
                                                    onclick: move |_| { let cur = *sort.read(); sort.set(logic::next_sort(cur, SortKey::Number)); },
                                                    span { class: "th", "No." Icon { name: sort_icon(current_sort, SortKey::Number), size: 14 } }
                                                }
                                                th { class: if sort_key == SortKey::Name { "sortable sorted" } else { "sortable" },
                                                    onclick: move |_| { let cur = *sort.read(); sort.set(logic::next_sort(cur, SortKey::Name)); },
                                                    span { class: "th", "Title" Icon { name: sort_icon(current_sort, SortKey::Name), size: 14 } }
                                                }
                                                th { class: if sort_key == SortKey::Sources { "src sortable sorted" } else { "src sortable" },
                                                    onclick: move |_| { let cur = *sort.read(); sort.set(logic::next_sort(cur, SortKey::Sources)); },
                                                    span { class: "th", "Sources" Icon { name: sort_icon(current_sort, SortKey::Sources), size: 14 } }
                                                }
                                                th { class: if sort_key == SortKey::Date { "date sortable sorted" } else { "date sortable" },
                                                    onclick: move |_| { let cur = *sort.read(); sort.set(logic::next_sort(cur, SortKey::Date)); },
                                                    span { class: "th", "Updated" Icon { name: sort_icon(current_sort, SortKey::Date), size: 14 } }
                                                }
                                            }
                                        }
                                        tbody { {rows.into_iter()} }
                                    }
                                    if order.is_empty() {
                                        EmptyState {
                                            title: "No chapters match",
                                            message: format!("Nothing matches “{}”. Try fewer words.", filter.read()),
                                            compact: true,
                                        }
                                    }
                                }
                            }
                            p { class: "muted small", style: "margin: 8px 0 0",
                                "Showing {order.len()} of {total} chapters. Click a column heading to sort; click a row to tick it."
                                if !missing.is_empty() { " Not available from any source: {missing_text}." }
                            }
                        }

                        // ---- export panel ----
                        div { class: "side",
                            div { class: "card",
                                h2 { Icon { name: "file-down" } "Export" }
                                div { class: "summary-line", span { class: "muted", "Chapters" } span { "{picked} selected" } }
                                div { class: "summary-line", span { class: "muted", "Reader" } span { "{settings.device.profile().name}" } }
                                div { class: "summary-line", span { class: "muted", "Firmware" } span { "{firmware_label}" } }
                                div { style: "margin: 4px 0 10px",
                                    {match support {
                                        Support::Documented => rsx! { Chip { text: "supported", kind: Some("ok".to_string()) } },
                                        Support::CommunityReported => rsx! { Chip { text: "reported to work", kind: Some("ok".to_string()) } },
                                        Support::Unverified => rsx! { Chip { text: "not verified on this firmware", kind: Some("warn".to_string()) } },
                                    }}
                                    button { class: "btn ghost small", onclick: move |_| state.go(Page::Settings), "Change" }
                                }

                                p { class: "muted small", style: "margin: 12px 0 6px", "Quality" }
                                div { class: "seg",
                                    button {
                                        class: if format == Format::Xtch { "on" } else { "" },
                                        onclick: move |_| { let mut s = state.settings; s.write().conversion.format = Format::Xtch; state.save_settings(); },
                                        "4 grays"
                                    }
                                    button {
                                        class: if format == Format::Xtc { "on" } else { "" },
                                        onclick: move |_| { let mut s = state.settings; s.write().conversion.format = Format::Xtc; state.save_settings(); },
                                        "Black & white"
                                    }
                                }

                                p { class: "muted small", style: "margin: 14px 0 6px", "Send to" }
                                div { class: "seg stack",
                                    button {
                                        class: if matches!(target(), TargetChoice::Folder) { "on" } else { "" },
                                        onclick: move |_| target.set(TargetChoice::Folder),
                                        Icon { name: "folder-open", size: 16 } "Folder on this computer"
                                    }
                                    for d in drives.read().iter() {
                                        {
                                            let mount = d.mount.display().to_string();
                                            let on = target() == TargetChoice::Reader(mount.clone());
                                            rsx! {
                                                button {
                                                    key: "{mount}",
                                                    class: if on { "on" } else { "" },
                                                    onclick: { let mount = mount.clone(); move |_| target.set(TargetChoice::Reader(mount.clone())) },
                                                    Icon { name: "hard-drive", size: 16 } "Reader / SD card ({mount})"
                                                }
                                            }
                                        }
                                    }
                                    if settings.export.webdav_url.as_deref().is_some_and(|u| !u.trim().is_empty()) {
                                        button {
                                            class: if matches!(target(), TargetChoice::WebDav) { "on" } else { "" },
                                            onclick: move |_| target.set(TargetChoice::WebDav),
                                            Icon { name: "wifi", size: 16 } "Over Wi-Fi"
                                        }
                                    }
                                }
                                if matches!(target(), TargetChoice::Folder) {
                                    p { class: "muted small", style: "margin: 6px 0 0; word-break: break-all", "{folder_hint}" }
                                }
                                button { class: "btn ghost small", style: "margin-top: 6px", onclick: move |_| drives.set(removable_drives()),
                                    Icon { name: "refresh-cw", size: 14 } "Look for my reader"
                                }

                                div { style: "height: 14px" }
                                button {
                                    class: "btn primary big",
                                    disabled: picked == 0 || job_running,
                                    onclick: {
                                        let merged = l.merged.clone();
                                        let member_sources = l.member_sources.clone();
                                        let title = series.title.clone();
                                        move |_| {
                                            let settings = state.settings.read().clone();
                                            let chosen: Vec<usize> = selected.read().iter().copied().collect();
                                            match logic::export_target(&target.read(), &settings, &paths().default_export_dir()) {
                                                Err(msg) => state.error(msg),
                                                Ok(t) => match logic::build_spec(&title, &merged, &chosen, &member_sources, &settings, t) {
                                                    Some(spec) => start_job(state, spec),
                                                    None => state.error("Tick at least one chapter first."),
                                                },
                                            }
                                        }
                                    },
                                    if job_running { Icon { name: "loader-circle", size: 18 } "An export is running…" }
                                    else if picked == 0 { "Select chapters to export" }
                                    else { Icon { name: "download", size: 18 } "Convert & export {picked}" }
                                }
                                p { class: "muted small", style: "margin: 8px 0 0", "Sources: {sources_used}." }
                            }
                        }
                    }
                }
            }
        }}
    }
}
