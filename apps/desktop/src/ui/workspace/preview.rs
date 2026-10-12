//! Tab 3: see how pages will look on the reader, and tune them.

use crate::state::{AppState, SeriesTab, Zoom};
use crate::ui::conversion::ConversionControls;
use crate::ui::presets::{apply_to_series, PresetPicker};
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{prepare_preview, schedule_preview};
use dioxus::prelude::*;

/// Go to a page and render it. A short delay lets a dragged slider settle.
fn set_page(state: AppState, page: usize, delay_ms: u64) {
    let mut preview = state.preview;
    preview.write().page = page;
    schedule_preview(state, delay_ms);
}

/// One picture in the stage: the original or a converted page.
struct Figure {
    key: String,
    caption: String,
    url: String,
    width: u32,
    height: u32,
    /// The kept area to outline on the original: left, top, width, height.
    kept: Option<[f32; 4]>,
}

/// How big to draw a picture. In "fit" every picture shares one height, the
/// largest that lets all of them sit side by side without scrolling.
fn size_style(zoom: Zoom, figure: &Figure, total_aspect: f32, count: usize) -> String {
    let gaps = 16 * count.saturating_sub(1);
    let aspect = format!("{} / {}", figure.width, figure.height);
    match zoom {
        Zoom::Fit => format!(
            "height: min(var(--stage-height), calc((100cqw - {gaps}px) / {total_aspect:.4})); aspect-ratio: {aspect};"
        ),
        Zoom::Width => format!(
            "height: calc((100cqw - {gaps}px) / {total_aspect:.4}); aspect-ratio: {aspect};"
        ),
        Zoom::Actual => format!("width: {}px; height: {}px;", figure.width, figure.height),
        Zoom::Double => format!(
            "width: {}px; height: {}px;",
            figure.width * 2,
            figure.height * 2
        ),
    }
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
    let view = p.view;
    let current = p.chapter_key.clone().unwrap_or_default();
    let total = p.total_pages;
    let page = p.page;
    let device_name = state.settings.read().device.profile().name;

    // The pictures to show, left to right.
    let mut figures: Vec<Figure> = Vec::new();
    if view.show_original {
        if let Some(original) = &p.original {
            figures.push(Figure {
                key: "original".into(),
                caption: "Original".into(),
                url: original.url.clone(),
                width: original.width,
                height: original.height,
                kept: if view.show_crop { p.kept } else { None },
            });
        }
    }
    let parts = p.converted.len();
    for (i, shot) in p.converted.iter().enumerate() {
        figures.push(Figure {
            key: format!("page-{i}"),
            caption: if parts > 1 {
                format!("On your {device_name} · part {} of {parts}", i + 1)
            } else {
                format!("On your {device_name}")
            },
            url: shot.url.clone(),
            width: shot.width,
            height: shot.height,
            kept: None,
        });
    }
    let total_aspect: f32 = figures
        .iter()
        .map(|f| f.width as f32 / f.height.max(1) as f32)
        .sum::<f32>()
        .max(0.1);
    let count = figures.len();
    let scrolling = matches!(view.zoom, Zoom::Actual | Zoom::Double);

    let series_id = series.id.clone();
    let pick_id = series.id.clone();
    let saved_id = series.id.clone();
    let reset_id = series.id.clone();
    let copy_settings = series.settings.clone();
    let copy_preset = series.preset_id.clone();
    let layout_class = if view.focus {
        "preview-layout focus"
    } else {
        "preview-layout"
    };

    rsx! {
        div { class: "{layout_class}",
            // ---- the pages ----
            div { class: "preview-main",
                div { class: "toolbar",
                    select {
                        style: "min-width: 200px; max-width: 320px",
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
                    span { class: "grow" }
                    div { class: "seg", title: "How big the pages are drawn",
                        for (zoom, label) in [(Zoom::Fit, "Fit"), (Zoom::Width, "Wide"), (Zoom::Actual, "100%"), (Zoom::Double, "200%")] {
                            button {
                                key: "{label}",
                                class: if view.zoom == zoom { "on" } else { "" },
                                onclick: move |_| { let mut pv = state.preview; pv.write().view.zoom = zoom; },
                                "{label}"
                            }
                        }
                    }
                    button {
                        class: if view.show_original { "btn small on" } else { "btn small" },
                        title: "Show the downloaded page next to the converted one",
                        onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.show_original = !w.view.show_original; },
                        Icon { name: "eye", size: 14 } "Original"
                    }
                    button {
                        class: if view.show_crop { "btn small on" } else { "btn small" },
                        title: "Outline what the cropping keeps, and dim what it cuts",
                        disabled: !view.show_original,
                        onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.show_crop = !w.view.show_crop; },
                        Icon { name: "crop", size: 14 } "Crop"
                    }
                    button {
                        class: "btn small",
                        title: if view.focus { "Show the settings" } else { "Hide the settings to give the pages the whole width" },
                        onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.focus = !w.view.focus; },
                        if view.focus { Icon { name: "panel-right-open", size: 14 } "Settings" } else { Icon { name: "panel-right-close", size: 14 } "Hide settings" }
                    }
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
                div {
                    class: if p.busy { "preview-stage busy" } else { "preview-stage" },
                    style: if scrolling { "justify-content: flex-start" } else { "" },
                    tabindex: "0",
                    onkeydown: move |e| {
                        match e.key() {
                            Key::ArrowLeft if page > 0 => set_page(state, page - 1, 0),
                            Key::ArrowRight if page + 1 < total => set_page(state, page + 1, 0),
                            _ => {}
                        }
                    },
                    for f in figures.iter() {
                        figure { class: "device", key: "{f.key}",
                            figcaption { "{f.caption}" }
                            div { class: "shot",
                                img {
                                    src: "{f.url}",
                                    style: "{size_style(view.zoom, f, total_aspect, count)}",
                                    alt: "{f.caption}",
                                }
                                if let Some([x, y, w, h]) = f.kept {
                                    if w < 0.999 || h < 0.999 {
                                        div {
                                            class: "kept",
                                            style: "left: {x * 100.0:.3}%; top: {y * 100.0:.3}%; width: {w * 100.0:.3}%; height: {h * 100.0:.3}%;",
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "muted small", style: "margin: 10px 0 0",
                    "The converted page is the exact image that goes into the file. The real screen's gray shades can look a little different; the first time you test on your reader, compare it with this. Click the pages and use the arrow keys to turn them."
                }
            }

            // ---- this series' settings ----
            if !view.focus {
                div { class: "side panel",
                    PresetPicker {
                        value: series.settings.clone(),
                        preset_id: series.preset_id.clone(),
                        on_pick: move |(id, settings)| apply_to_series(state, &pick_id, Some(id), settings),
                        on_saved: move |id| {
                            let mut library = state.library;
                            library.write().set_preset(&saved_id, Some(id));
                            state.save_library();
                        },
                    }
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
                                    let (defaults, preset) = {
                                        let s = state.settings.peek();
                                        (s.conversion.clone(), s.default_preset_id.clone())
                                    };
                                    apply_to_series(state, &reset_id, preset, defaults);
                                },
                                Icon { name: "rotate-ccw", size: 14 } "Back to my defaults"
                            }
                            button { class: "btn ghost small",
                                onclick: move |_| {
                                    let linked = copy_preset.clone().filter(|id| {
                                        state.presets.peek().find(id).is_some_and(|e| e.preset.settings == copy_settings)
                                    });
                                    let mut settings = state.settings;
                                    {
                                        let mut s = settings.write();
                                        s.conversion = copy_settings.clone();
                                        s.default_preset_id = linked;
                                    }
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
}
