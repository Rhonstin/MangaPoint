//! Tab 4: choose what to export and where it goes.

use crate::logic::{self, TargetChoice};
use crate::state::paths;
use crate::state::{AppState, GroupingChoice, JobKind, Page, SeriesTab};
use crate::ui::widgets::{show_in_folder, Chip, EmptyState, Icon};
use crate::ui::work::start_export;
use dioxus::prelude::*;
use pipeline::export::removable_drives;

#[component]
pub fn ExportTab() -> Element {
    let state = use_context::<AppState>();
    let mut drives = use_signal(removable_drives);
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let Some(series) = state.library.read().find(&ws.series_id).cloned() else {
        return rsx! {
            EmptyState {
                title: "Nothing to export yet",
                message: "Download a chapter first, then come back here.",
            }
        };
    };
    let opts = state.export_opts.read().clone();
    let settings = state.settings.read().clone();
    let busy = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.result.is_none());
    let exporting = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.kind == JobKind::Export && j.result.is_none());

    let included: Vec<&library::DownloadedChapter> = series
        .chapters
        .iter()
        .filter(|c| !opts.excluded.contains(&c.key))
        .collect();
    let chapter_count = included.len();
    let page_count: usize = included.iter().map(|c| c.pages).sum();
    let estimate = logic::estimate_export_bytes(page_count, series.settings.format);
    let all_included = chapter_count == series.chapters.len();
    let format_label = match series.settings.format {
        converter::xtc::Format::Xtch => "4 grays",
        converter::xtc::Format::Xtc => "black & white",
    };
    let direction_label = match series.settings.direction {
        converter::xtc::ReadDirection::RightToLeft => "right to left",
        converter::xtc::ReadDirection::LeftToRight => "left to right",
        converter::xtc::ReadDirection::TopToBottom => "top to bottom",
    };
    let folder_hint = settings
        .export
        .output_dir
        .clone()
        .unwrap_or_else(|| paths().default_export_dir())
        .display()
        .to_string();
    let every_n_valid = opts.every_n.trim().parse::<usize>().is_ok_and(|n| n >= 1);
    let past_exports: Vec<library::ExportRecord> = state
        .exports
        .read()
        .records
        .iter()
        .rev()
        .filter(|r| r.series_id == series.id)
        .cloned()
        .collect();

    let mut export_opts = state.export_opts;
    let all_keys: Vec<String> = series.chapters.iter().map(|c| c.key.clone()).collect();

    let rows: Vec<Element> = series
        .chapters
        .iter()
        .map(|c| {
            let on = !opts.excluded.contains(&c.key);
            let key = c.key.clone();
            let number = c.number.map(logic::format_number).unwrap_or_else(|| "–".to_string());
            let name = c.name.clone();
            let size = logic::human_bytes(c.bytes);
            let pages = c.pages;
            rsx! {
                tr {
                    key: "{c.key}",
                    class: if on { "on" } else { "" },
                    onclick: move |_| {
                        let mut o = export_opts.write();
                        if !o.excluded.remove(&key) { o.excluded.insert(key.clone()); }
                    },
                    td { class: "check", input { r#type: "checkbox", checked: on, readonly: true, style: "pointer-events: none" } }
                    td { class: "num", "{number}" }
                    td { class: "name", title: "{name}", "{name}" }
                    td { class: "date", "{pages}" }
                    td { class: "date", "{size}" }
                }
            }
        })
        .collect();

    let more_icon: &'static str = if opts.advanced_open {
        "chevron-up"
    } else {
        "chevron-down"
    };
    let mut set_grouping = move |g: GroupingChoice| {
        export_opts.write().grouping = g;
    };

    rsx! {
        div { class: "series-layout",
            // ---- which chapters ----
            div {
                div { class: "toolbar",
                    strong { "{chapter_count} of {series.chapters.len()} chapters included" }
                    span { class: "muted small", "Click a row to include or leave out a chapter." }
                }
                div { class: "table-wrap",
                    div { class: "table-scroll",
                        table { class: "chapters",
                            thead { tr {
                                th { class: "check",
                                    input {
                                        r#type: "checkbox", checked: all_included,
                                        title: "Include or leave out everything",
                                        onclick: move |_| {
                                            let mut o = export_opts.write();
                                            if all_included { o.excluded = all_keys.iter().cloned().collect(); } else { o.excluded.clear(); }
                                        },
                                    }
                                }
                                th { class: "num", "No." }
                                th { "Title" }
                                th { class: "date", "Pages" }
                                th { class: "date", "On disk" }
                            } }
                            tbody { {rows.into_iter()} }
                        }
                    }
                }
            }

            // ---- how and where ----
            div { class: "side",
                div { class: "card",
                    h2 { Icon { name: "file-down", size: 17 } "Export" }
                    div { class: "summary-line", span { class: "muted", "Chapters" } span { "{chapter_count}" } }
                    div { class: "summary-line", span { class: "muted", "Pages" } span { "{page_count}" } }
                    div { class: "summary-line", span { class: "muted", "Size, about" } span { "{logic::human_bytes(estimate)}" } }
                    div { class: "summary-line", span { class: "muted", "Format" } span { "{format_label}, {direction_label}" } }
                    button { class: "btn ghost small", style: "margin: 2px 0 8px", onclick: move |_| state.open_tab(SeriesTab::Preview),
                        Icon { name: "eye", size: 14 } "Adjust in Preview"
                    }

                    p { class: "muted small", style: "margin: 8px 0 6px", "Files" }
                    div { class: "seg stack",
                        button {
                            class: if opts.grouping == GroupingChoice::Single { "on" } else { "" },
                            onclick: move |_| set_grouping(GroupingChoice::Single),
                            Icon { name: "book-open", size: 16 } "One file"
                        }
                        if opts.advanced_open {
                            button {
                                class: if opts.grouping == GroupingChoice::PerChapter { "on" } else { "" },
                                onclick: move |_| set_grouping(GroupingChoice::PerChapter),
                                Icon { name: "layers", size: 16 } "One file per chapter"
                            }
                            button {
                                class: if opts.grouping == GroupingChoice::EveryN { "on" } else { "" },
                                onclick: move |_| set_grouping(GroupingChoice::EveryN),
                                Icon { name: "list-checks", size: 16 } "One file every N chapters"
                            }
                        }
                    }
                    if opts.advanced_open && opts.grouping == GroupingChoice::PerChapter {
                        div { class: "banner warn small", style: "margin: 8px 0 0",
                            Icon { name: "triangle-alert", size: 16 }
                            "This makes {chapter_count} separate files, which can be a lot to manage on your reader."
                        }
                    }
                    if opts.advanced_open && opts.grouping == GroupingChoice::EveryN {
                        div { class: "row", style: "margin-top: 8px",
                            span { class: "muted small", "Every" }
                            input {
                                r#type: "text", style: "width: 64px; text-align: center",
                                value: "{opts.every_n}",
                                oninput: move |e| export_opts.write().every_n = e.value(),
                            }
                            span { class: "muted small", "chapters" }
                            if !every_n_valid { span { class: "small", style: "color: var(--bad)", "Type a number" } }
                        }
                    }
                    button { class: "btn ghost small", style: "margin-top: 4px",
                        onclick: move |_| {
                            let mut o = export_opts.write();
                            o.advanced_open = !o.advanced_open;
                            if !o.advanced_open { o.grouping = GroupingChoice::Single; }
                        },
                        Icon { name: more_icon, size: 14 }
                        if opts.advanced_open { "Fewer options" } else { "More options" }
                    }

                    p { class: "muted small", style: "margin: 10px 0 6px", "Send to" }
                    div { class: "seg stack",
                        button {
                            class: if matches!(opts.target, TargetChoice::Folder) { "on" } else { "" },
                            onclick: move |_| export_opts.write().target = TargetChoice::Folder,
                            Icon { name: "folder-open", size: 16 } "Folder on this computer"
                        }
                        for d in drives.read().iter() {
                            {
                                let mount = d.mount.display().to_string();
                                let on = opts.target == TargetChoice::Reader(mount.clone());
                                let label = format!("Reader / SD card ({mount})");
                                rsx! {
                                    button {
                                        key: "{mount}",
                                        class: if on { "on" } else { "" },
                                        onclick: { let mount = mount.clone(); move |_| export_opts.write().target = TargetChoice::Reader(mount.clone()) },
                                        Icon { name: "hard-drive", size: 16 } "{label}"
                                    }
                                }
                            }
                        }
                        if settings.export.webdav_url.as_deref().is_some_and(|u| !u.trim().is_empty()) {
                            button {
                                class: if matches!(opts.target, TargetChoice::WebDav) { "on" } else { "" },
                                onclick: move |_| export_opts.write().target = TargetChoice::WebDav,
                                Icon { name: "wifi", size: 16 } "Over Wi-Fi"
                            }
                        }
                    }
                    if matches!(opts.target, TargetChoice::Folder) {
                        p { class: "muted small", style: "margin: 6px 0 0; word-break: break-all", "{folder_hint}" }
                    }
                    button { class: "btn ghost small", style: "margin-top: 6px", onclick: move |_| drives.set(removable_drives()),
                        Icon { name: "refresh-cw", size: 14 } "Look for my reader"
                    }

                    div { style: "height: 14px" }
                    button {
                        class: "btn primary big",
                        disabled: chapter_count == 0 || busy || (opts.grouping == GroupingChoice::EveryN && !every_n_valid),
                        onclick: move |_| start_export(state),
                        if exporting { Icon { name: "loader-circle", size: 18 } "Exporting…" }
                        else if busy { "Something else is running…" }
                        else if chapter_count == 0 { "Include at least one chapter" }
                        else { Icon { name: "file-down", size: 18 } "Export {chapter_count} chapter(s)" }
                    }
                }

                if !past_exports.is_empty() {
                    div { class: "card",
                        h2 { Icon { name: "clock", size: 17 } "Already exported" }
                        for r in past_exports.iter().take(4) {
                            div { key: "{r.id}", style: "padding: 4px 0",
                                div { class: "row wrap",
                                    Chip { text: r.format.to_uppercase() }
                                    span { class: "muted small", "{r.chapters} chapters · {r.files.len()} file(s)" }
                                }
                                for f in r.files.iter() {
                                    {
                                        let path = f.path.clone();
                                        let is_url = path.starts_with("http");
                                        rsx! {
                                            div { class: "row small", style: "margin-top: 2px",
                                                span { class: "grow muted", style: "word-break: break-all", "{path}" }
                                                if !is_url { button { class: "btn small", onclick: move |_| show_in_folder(&path), Icon { name: "folder-open", size: 14 } } }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        button { class: "btn ghost small", onclick: move |_| state.go(Page::Library), Icon { name: "book-open", size: 15 } "See all in Library" }
                    }
                }
            }
        }
    }
}
