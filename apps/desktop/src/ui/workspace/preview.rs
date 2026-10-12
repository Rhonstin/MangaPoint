//! Tab 3: see how pages will look on the reader, and tune them.

use crate::state::{AppState, SeriesTab};
use crate::ui::conversion::ConversionControls;
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{prepare_preview, schedule_preview};
use dioxus::prelude::*;

/// Go to a page and render it. A short delay lets a dragged slider settle.
fn set_page(state: AppState, page: usize, delay_ms: u64) {
    let mut preview = state.preview;
    preview.write().page = page;
    schedule_preview(state, delay_ms);
}

#[component]
pub fn PreviewTab() -> Element {
    let state = use_context::<AppState>();
    // Make sure there is something to show when the tab opens.
    use_effect(move || prepare_preview(state));

    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let Some(series) = state.library.read().find(&ws.series_id).cloned() else {
        return rsx! {
            EmptyState {
                title: "Nothing to preview yet",
                message: "Download a chapter first and the cat will lay it out for you.",
            }
        };
    };
    let p = state.preview.read().clone();
    let current = p.chapter_key.clone().unwrap_or_default();
    let total = p.total_pages;
    let page = p.page;
    let series_id = series.id.clone();
    let reset_id = series.id.clone();
    let copy_settings = series.settings.clone();
    let device_name = state.settings.read().device.profile().name;

    rsx! {
        div { class: "series-layout",
            // ---- the pages ----
            div {
                div { class: "toolbar",
                    select {
                        style: "min-width: 220px",
                        onchange: move |e| {
                            let mut pv = state.preview;
                            {
                                let mut w = pv.write();
                                w.chapter_key = Some(e.value());
                                w.page = 0;
                                w.decoded = None;
                            }
                            schedule_preview(state, 0);
                        },
                        for c in series.chapters.iter() {
                            option { key: "{c.key}", value: "{c.key}", selected: c.key == current, "{c.name} ({c.pages} pages)" }
                        }
                    }
                    button { class: "btn small", disabled: page == 0, onclick: move |_| set_page(state, page.saturating_sub(1), 0),
                        Icon { name: "arrow-left", size: 14 } "Previous"
                    }
                    button { class: "btn small", disabled: page + 1 >= total, onclick: move |_| set_page(state, page + 1, 0),
                        "Next" span { style: "display: inline-flex; transform: rotate(180deg)", Icon { name: "arrow-left", size: 14 } }
                    }
                    span { class: "muted small", "Page" }
                    input {
                        r#type: "text", style: "width: 56px; text-align: center",
                        value: "{page + 1}",
                        onchange: move |e| {
                            if let Ok(n) = e.value().trim().parse::<usize>() {
                                set_page(state, n.clamp(1, total.max(1)) - 1, 0);
                            }
                        },
                    }
                    span { class: "muted small", "of {total}" }
                    if p.busy { span { class: "spin muted", Icon { name: "loader-circle", size: 16 } } }
                }
                if total > 1 {
                    input {
                        r#type: "range", min: "0", max: "{total - 1}", value: "{page}",
                        style: "margin: 2px 0 12px",
                        oninput: move |e| {
                            if let Ok(n) = e.value().parse::<usize>() { set_page(state, n, 80); }
                        },
                    }
                }
                if let Some(message) = &p.error {
                    div { class: "banner bad small", Icon { name: "circle-x", size: 16 } "{message}" }
                }
                div { class: if p.busy { "preview-stage busy" } else { "preview-stage" },
                    if let Some(original) = &p.original {
                        figure { class: "device small",
                            figcaption { "Original" }
                            img { src: "{original.url}", alt: "The page as downloaded" }
                        }
                    }
                    for (i, shot) in p.converted.iter().enumerate() {
                        figure { class: "device", key: "{i}",
                            figcaption {
                                if p.converted.len() > 1 { "On your {device_name} · part {i + 1} of {p.converted.len()}" } else { "On your {device_name}" }
                            }
                            img { src: "{shot.url}", alt: "The page as it will look on the reader" }
                        }
                    }
                }
                p { class: "muted small", style: "margin: 10px 0 0",
                    "This is the exact image that goes into the file. The real screen's gray shades can look a little different; the first time you test on your reader, compare it with this."
                }
            }

            // ---- this series' settings ----
            div { class: "side",
                div { class: "card",
                    h2 { Icon { name: "sliders-horizontal", size: 17 } "This series" }
                    p { class: "muted small", "These settings apply to every chapter of {ws.title}." }
                    ConversionControls {
                        value: series.settings.clone(),
                        compact: true,
                        onchange: move |v| {
                            let mut lib = state.library;
                            lib.write().set_settings(&series_id, v);
                            state.save_library();
                            schedule_preview(state, 150);
                        },
                    }
                    div { class: "row wrap", style: "margin-top: 8px",
                        button { class: "btn ghost small",
                            onclick: move |_| {
                                let defaults = state.settings.peek().conversion.clone();
                                let mut lib = state.library;
                                lib.write().set_settings(&reset_id, defaults);
                                state.save_library();
                                schedule_preview(state, 0);
                            },
                            Icon { name: "rotate-ccw", size: 14 } "Back to my defaults"
                        }
                        button { class: "btn ghost small",
                            onclick: move |_| {
                                let mut settings = state.settings;
                                settings.write().conversion = copy_settings.clone();
                                state.save_settings();
                                state.notify("Saved as your starting point for new series.");
                            },
                            Icon { name: "check", size: 14 } "Use for new series"
                        }
                    }
                    div { style: "height: 12px" }
                    button { class: "btn primary big", onclick: move |_| state.open_tab(SeriesTab::Export),
                        "Next: Export"
                        span { style: "display: inline-flex; transform: rotate(180deg)", Icon { name: "arrow-left", size: 16 } }
                    }
                }
            }
        }
    }
}
