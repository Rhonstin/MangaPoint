//! One series: pick chapters and export them.

use super::jobs::start_job;
use super::widgets::{cover_key, Chip, Cover};
use crate::logic::{self, TargetChoice};
use crate::state::{paths, AppState, Page};
use converter::xtc::Format;
use device_profiles::Support;
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

#[component]
pub fn SeriesPage() -> Element {
    let state = use_context::<AppState>();
    let series = state.series.read().clone();
    let Some(series) = series else {
        return rsx! {
            p { class: "muted", "Pick a series from the search results first." }
            button { class: "btn", onclick: move |_| state.go(Page::Search), "Back to search" }
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
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    let mut target = use_signal(|| TargetChoice::Folder);
    let mut drives = use_signal(removable_drives);

    let best = series.best().candidate.clone();
    let cover = cover_key(&best.source_id, best.manga_id);
    let settings = state.settings.read().clone();
    let format = settings.conversion.format;
    let support = settings.device.xtc_support(settings.firmware);
    let job_running = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.result.is_none());

    rsx! {
        button { class: "btn small", onclick: move |_| state.go(Page::Search), "← Results" }
        div { class: "spacer" }
        div { class: "row", style: "align-items: flex-start; gap: 18px",
            div { style: "width: 120px; flex: none", Cover { key_id: cover } }
            div { class: "grow",
                h1 { "{series.title}" }
                div { class: "row wrap",
                    for (i, m) in shown.iter().enumerate() {
                        Chip {
                            key: "{i}",
                            text: if i == 0 { format!("{} (best)", m.candidate.source_name) } else { m.candidate.source_name.clone() },
                            kind: if i == 0 { Some("ok".to_string()) } else { None },
                        }
                    }
                }
                p { class: "muted small", style: "margin-top: 8px",
                    "If a chapter fails on the best source, MangaPoint tries the others automatically."
                }
            }
        }
        div { class: "spacer" }

        {match &*loaded.read() {
            None => rsx! { p { class: "muted", "Loading chapters from {shown.len()} source(s)…" } },
            Some(None) => rsx! { div { class: "banner bad", "The catalog engine is not running." } },
            Some(Some(l)) => {
                let numbers: Vec<Option<f64>> = l.merged.iter().map(|m| m.number).collect();
                let missing = missing_numbers(&l.merged);
                let total = l.merged.len();
                let picked = selected.read().len();
                let numbers_for_all = numbers.clone();
                let numbers_for_latest = numbers.clone();
                let numbers_for_range = numbers.clone();
                let missing_text = {
                    let shown: Vec<String> = missing.iter().take(15).map(|n| n.to_string()).collect();
                    let more = if missing.len() > 15 { " …" } else { "" };
                    format!("{}{more}", shown.join(", "))
                };
                let firmware_label = if settings.firmware == device_profiles::Firmware::CrossPoint {
                    "CrossPoint"
                } else {
                    "Stock firmware"
                };
                let sources_used = l.member_names.join(", ");
                rsx! {
                    if l.unreachable > 0 {
                        div { class: "banner warn small", "{l.unreachable} source(s) did not answer, so some chapters may be missing." }
                    }
                    div { class: "card",
                        div { class: "row wrap",
                            strong { "{total} chapters" }
                            span { class: "muted", "· {picked} selected" }
                            div { class: "grow" }
                            button { class: "btn small", onclick: move |_| selected.set((0..numbers_for_all.len()).collect()), "All" }
                            button {
                                class: "btn small",
                                onclick: move |_| selected.set(logic::latest(&numbers_for_latest, 10).into_iter().collect()),
                                "Latest 10"
                            }
                            button { class: "btn small", onclick: move |_| selected.set(HashSet::new()), "None" }
                        }
                        form {
                            class: "row", style: "margin-top: 10px",
                            onsubmit: move |e| {
                                e.prevent_default();
                                match logic::select_by_text(&range_text.read(), &numbers_for_range) {
                                    Ok(found) => {
                                        range_error.set(None);
                                        selected.set(found.into_iter().collect());
                                    }
                                    Err(msg) => range_error.set(Some(msg)),
                                }
                            },
                            input {
                                r#type: "text", class: "grow",
                                placeholder: "Chapters, for example 1-20, 25, 30-",
                                value: "{range_text}",
                                oninput: move |e| range_text.set(e.value()),
                            }
                            button { class: "btn small", r#type: "submit", "Select" }
                        }
                        if let Some(msg) = range_error() { p { class: "small", style: "color: var(--bad); margin: 6px 0 0", "{msg}" } }
                        if !missing.is_empty() {
                            p { class: "muted small", style: "margin: 8px 0 0",
                                "Not available from any source: chapter {missing_text}"
                            }
                        }
                        div { class: "chapter-list", style: "margin-top: 10px",
                            for i in (0..total).rev() {
                                {
                                    let ch = &l.merged[i];
                                    let checked = selected.read().contains(&i);
                                    let from = ch.options.len();
                                    rsx! {
                                        label { key: "{i}",
                                            input {
                                                r#type: "checkbox", checked,
                                                onchange: move |e| {
                                                    let mut s = selected.write();
                                                    if e.checked() { s.insert(i); } else { s.remove(&i); }
                                                },
                                            }
                                            span { class: "grow", "{ch.name}" }
                                            if from > 1 { span { class: "muted small", "{from} sources" } }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "card",
                        h2 { "Export" }
                        div { class: "row wrap",
                            span { "{settings.device.profile().name}" }
                            span { class: "muted", "· {firmware_label}" }
                            {match support {
                                Support::Documented => rsx! { Chip { text: "supported", kind: Some("ok".to_string()) } },
                                Support::CommunityReported => rsx! { Chip { text: "reported to work", kind: Some("ok".to_string()) } },
                                Support::Unverified => rsx! { Chip { text: "not verified on this firmware", kind: Some("warn".to_string()) } },
                            }}
                            div { class: "grow" }
                            button { class: "btn small", onclick: move |_| state.go(Page::Settings), "Change" }
                        }
                        div { class: "spacer" }
                        div { class: "row wrap",
                            span { class: "muted", "Quality" }
                            div { class: "seg",
                                button {
                                    class: if format == Format::Xtch { "on" } else { "" },
                                    onclick: move |_| { let mut s = state.settings; s.write().conversion.format = Format::Xtch; state.save_settings(); },
                                    "4 grays (recommended)"
                                }
                                button {
                                    class: if format == Format::Xtc { "on" } else { "" },
                                    onclick: move |_| { let mut s = state.settings; s.write().conversion.format = Format::Xtc; state.save_settings(); },
                                    "Black & white (smaller)"
                                }
                            }
                        }
                        div { class: "spacer" }
                        div { class: "row wrap",
                            span { class: "muted", "Send to" }
                            div { class: "seg",
                                button { class: if matches!(target(), TargetChoice::Folder) { "on" } else { "" }, onclick: move |_| target.set(TargetChoice::Folder), "A folder on this computer" }
                                for d in drives.read().iter() {
                                    {
                                        let mount = d.mount.display().to_string();
                                        let on = target() == TargetChoice::Reader(mount.clone());
                                        let label = format!("Reader / SD card ({mount})");
                                        rsx! { button { key: "{mount}", class: if on { "on" } else { "" }, onclick: move |_| target.set(TargetChoice::Reader(mount.clone())), "{label}" } }
                                    }
                                }
                                if settings.export.webdav_url.as_deref().is_some_and(|u| !u.trim().is_empty()) {
                                    button { class: if matches!(target(), TargetChoice::WebDav) { "on" } else { "" }, onclick: move |_| target.set(TargetChoice::WebDav), "Over Wi-Fi" }
                                }
                            }
                            button { class: "btn small", onclick: move |_| drives.set(removable_drives()), "Look for my reader" }
                        }
                        if drives.read().is_empty() {
                            p { class: "muted small", style: "margin: 8px 0 0",
                                "To copy straight to your reader, plug it in with USB (or insert its SD card) and press “Look for my reader”."
                            }
                        }
                        div { class: "spacer" }
                        button {
                            class: "btn primary",
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
                            if job_running { "An export is running…" } else if picked == 0 { "Select chapters to export" } else { "Convert & export {picked} chapter(s)" }
                        }
                        p { class: "muted small", style: "margin: 8px 0 0",
                            "Sources used: {sources_used}."
                        }
                    }
                }
            }
        }}
    }
}
