//! Tab 3: scroll through the chapter as it will look on the reader, and tune
//! the settings. Every page is drawn in one long column, the original beside
//! the converted page, so you read it like a normal screen.

use crate::state::{AppState, PreviewPage, SeriesTab, Zoom};
use crate::ui::conversion::ConversionControls;
use crate::ui::devices::DeviceSelect;
use crate::ui::presets::{apply_to_series, PresetPicker};
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{prepare_preview, schedule_preview};
use dioxus::prelude::*;
use std::rc::Rc;

/// The tallest a page is drawn when it is sized to "fit", in screen pixels.
const FIT_MAX_HEIGHT: u32 = 1100;
/// Space between the pictures in a row, in screen pixels.
const GAP: u32 = 16;

/// One picture to draw.
struct Figure {
    key: String,
    caption: String,
    url: String,
    width: u32,
    height: u32,
    /// The kept area to outline on the original: left, top, width, height.
    kept: Option<[f32; 4]>,
}

/// How big to draw a picture. In "fit" the pictures of a row share one height:
/// the largest that lets them sit side by side across the width.
fn image_style(zoom: Zoom, width: u32, height: u32, total_aspect: f32, count: usize) -> String {
    let gaps = GAP * count.saturating_sub(1) as u32;
    let ratio = format!("aspect-ratio: {width} / {height};");
    // Every size property is always set: the web view only updates the
    // properties present in a style, so one left out would keep its old value
    // (a 200% width surviving a switch back to "fit").
    match zoom {
        Zoom::Fit => format!(
            "width: auto; height: min(calc((100cqw - {gaps}px) / {total_aspect:.4}), {FIT_MAX_HEIGHT}px); {ratio}"
        ),
        Zoom::Width => format!(
            "width: auto; height: calc((100cqw - {gaps}px) / {total_aspect:.4}); {ratio}"
        ),
        Zoom::Actual => format!("width: {width}px; height: {height}px; {ratio}"),
        Zoom::Double => format!(
            "width: {}px; height: {}px; {ratio}",
            width * 2,
            height * 2
        ),
    }
}

fn skeleton(index: usize, view: crate::state::PreviewView) -> Element {
    // A guess at the row's shape, so the page does not jump as pages arrive.
    let aspect = if view.show_original { 1.25 } else { 0.6 };
    let style = image_style(view.zoom, 480, 800, aspect, 1);
    rsx! {
        div { class: "page-row", key: "{index}",
            div { class: "page-label", "Page {index + 1}" }
            div { class: "skeleton", style: "{style}" }
        }
    }
}

/// The pages that come from one picture: one, or two for a split double page.
/// Each page of a joined webtoon is its own group.
struct Group {
    first: usize,
    pages: Vec<Rc<PreviewPage>>,
}

fn groups(pages: &[Rc<PreviewPage>]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        match out.last_mut() {
            Some(last) if page.image.is_some() && last.pages[0].image == page.image => {
                last.pages.push(page.clone());
            }
            _ => out.push(Group {
                first: i,
                pages: vec![page.clone()],
            }),
        }
    }
    out
}

fn page_row(group: &Group, view: crate::state::PreviewView, device_name: &str) -> Element {
    let first = &group.pages[0];
    let label = if group.pages.len() > 1 {
        format!(
            "Pages {}\u{2013}{}",
            group.first + 1,
            group.first + group.pages.len()
        )
    } else {
        format!("Page {}", group.first + 1)
    };
    if group.pages.iter().any(|p| p.shot.is_none()) {
        return skeleton(group.first, view);
    }
    let original = if view.show_original {
        first.original.as_ref().map(|shot| Figure {
            key: "original".into(),
            caption: "Original".into(),
            url: shot.url.clone(),
            width: shot.width,
            height: shot.height,
            kept: if view.show_crop { first.kept } else { None },
        })
    } else {
        None
    };
    let converted: Vec<Figure> = group
        .pages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            p.shot.as_ref().map(|shot| Figure {
                key: format!("page-{i}"),
                caption: if group.pages.len() > 1 {
                    format!(
                        "On your {device_name} \u{b7} part {} of {}",
                        i + 1,
                        group.pages.len()
                    )
                } else {
                    format!("On your {device_name}")
                },
                url: shot.url.clone(),
                width: shot.width,
                height: shot.height,
                kept: None,
            })
        })
        .collect();

    let mut figures: Vec<&Figure> = Vec::new();
    if let Some(o) = &original {
        figures.push(o);
    }
    figures.extend(converted.iter());
    let total_aspect: f32 = figures
        .iter()
        .map(|f| f.width as f32 / f.height.max(1) as f32)
        .sum::<f32>()
        .max(0.1);
    let count = figures.len();
    rsx! {
        div { class: "page-row", key: "{group.first}",
            div { class: "page-label", "{label}" }
            div { class: "page-pair",
                for f in figures.iter() {
                    figure { class: "device", key: "{f.key}",
                        figcaption { "{f.caption}" }
                        div { class: "shot",
                            img {
                                src: "{f.url}",
                                style: "{image_style(view.zoom, f.width, f.height, total_aspect, count)}",
                                alt: "{f.caption}",
                            }
                            if let Some([x, y, w, h]) = f.kept {
                                if w < 0.999 || h < 0.999 {
                                    div { class: "kept", style: "left: {x * 100.0:.3}%; top: {y * 100.0:.3}%; width: {w * 100.0:.3}%; height: {h * 100.0:.3}%;" }
                                }
                            }
                        }
                    }
                }
            }
        }
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
    let total = p.pages.len();
    let device_name = state.settings.read().device_profile().name;
    let advanced = state.settings.read().advanced_conversion;

    let rows: Vec<Element> = groups(&p.pages)
        .iter()
        .map(|g| page_row(g, view, &device_name))
        .collect();

    let series_id = series.id.clone();
    let pick_id = series.id.clone();
    let saved_id = series.id.clone();
    let reset_id = series.id.clone();
    let preset_base = series
        .preset_id
        .as_ref()
        .and_then(|id| state.presets.read().find(id))
        .map(|e| e.preset.settings);
    let copy_settings = series.settings.clone();
    let copy_preset = series.preset_id.clone();
    let overridden = series.overridden.len();
    let layout_class = if view.focus {
        "preview-layout focus"
    } else {
        "preview-layout"
    };

    rsx! {
        div { class: "{layout_class}",
            // ---- the pages ----
            div { class: "preview-main",
                div { class: "toolbar sticky",
                    select {
                        style: "min-width: 200px; max-width: 320px",
                        onchange: move |e| {
                            let mut pv = state.preview;
                            {
                                let mut w = pv.write();
                                w.chapter_key = Some(e.value());
                                w.pages.clear();
                                w.fresh = 0;
                            }
                            schedule_preview(state, 0);
                        },
                        for c in series.chapters.iter() {
                            option { key: "{c.key}", value: "{c.key}", selected: c.key == current, "{c.name} ({c.pages} pages)" }
                        }
                    }
                    if p.busy && total > 0 {
                        span { class: "muted small", span { class: "spin", Icon { name: "loader-circle", size: 14 } } " Drawing {p.fresh} of {total}" }
                    } else if total > 0 {
                        span { class: "muted small", "{total} pages" }
                    } else if p.busy {
                        span { class: "spin muted", Icon { name: "loader-circle", size: 16 } }
                    }
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
                }
                if let Some(message) = &p.error {
                    div { class: "banner bad small", Icon { name: "circle-x", size: 16 } "{message}" }
                }
                if p.strip && total > 0 {
                    p { class: "muted small", style: "margin: 0 0 8px",
                        "This chapter is a long strip. Its {p.images} images are joined and cut into pages at the gaps between scenes, so no panel is split at an image edge."
                    }
                }
                div { class: if p.busy { "preview-stage busy" } else { "preview-stage" },
                    {rows.into_iter()}
                }
                p { class: "muted small", style: "margin: 14px 0 0",
                    "The converted pages are the exact images that go into the file. The real screen's gray shades can look a little different; the first time you test on your reader, compare it with this."
                }
            }

            // ---- this series' settings ----
            button {
                class: if view.focus { "panel-toggle collapsed" } else { "panel-toggle" },
                title: if view.focus { "Show the settings" } else { "Hide the settings" },
                onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.focus = !w.view.focus; },
                span { class: "panel-toggle-arrow", Icon { name: "arrow-left", size: 16 } }
            }
            if !view.focus {
                div { class: "side settings-panel",
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
                    DeviceSelect {}
                    div { class: "panel-section",
                        label { class: "panel-label", "Settings" }
                        p { class: "muted small panel-note",
                            "These apply to every chapter of {ws.title}."
                            if overridden > 0 {
                                " {overridden} differ from the preset and stay as you set them if the preset changes."
                            }
                        }
                        ConversionControls {
                            value: series.settings.clone(),
                            compact: true,
                            onchange: move |v| {
                                let mut lib = state.library;
                                lib.write().set_settings(&series_id, v, preset_base.as_ref());
                                state.save_library();
                                schedule_preview(state, 250);
                            },
                        }
                        if advanced {
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
                        }
                    }
                    button { class: "btn primary big", onclick: move |_| state.open_tab(SeriesTab::Export),
                        "Next: Export"
                        span { style: "display: inline-flex; transform: rotate(180deg)", Icon { name: "arrow-left", size: 16 } }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(image: Option<usize>) -> Rc<PreviewPage> {
        Rc::new(PreviewPage {
            shot: None,
            original: None,
            kept: None,
            image,
        })
    }

    #[test]
    fn fit_shares_one_height_and_caps_it_while_wide_does_not() {
        let fit = image_style(Zoom::Fit, 480, 800, 1.25, 2);
        assert!(
            fit.contains("min(calc((100cqw - 16px) / 1.2500), 1100px)"),
            "{fit}"
        );
        assert!(fit.contains("aspect-ratio: 480 / 800"));
        let wide = image_style(Zoom::Width, 480, 800, 1.25, 2);
        assert!(
            wide.contains("calc((100cqw - 16px) / 1.2500)") && !wide.contains("min("),
            "{wide}"
        );
    }

    #[test]
    fn fixed_zooms_use_exact_sizes_and_nothing_left_over_from_fit() {
        let actual = image_style(Zoom::Actual, 480, 800, 1.0, 1);
        assert_eq!(
            actual,
            "width: 480px; height: 800px; aspect-ratio: 480 / 800;"
        );
        let double = image_style(Zoom::Double, 480, 800, 1.0, 1);
        assert_eq!(
            double,
            "width: 960px; height: 1600px; aspect-ratio: 480 / 800;"
        );
    }

    #[test]
    fn every_zoom_sets_both_size_properties_so_none_is_left_over_from_the_last() {
        for zoom in [Zoom::Fit, Zoom::Width, Zoom::Actual, Zoom::Double] {
            let style = image_style(zoom, 480, 800, 1.0, 1);
            assert!(style.contains("width:"), "{zoom:?}: {style}");
            assert!(style.contains("height:"), "{zoom:?}: {style}");
        }
        assert!(image_style(Zoom::Fit, 480, 800, 1.0, 1).contains("width: auto"));
    }

    #[test]
    fn a_split_double_page_shares_a_row_and_strip_pages_do_not() {
        // Image 0 gives one page, image 1 is split in two, image 2 gives one.
        let paged = [page(Some(0)), page(Some(1)), page(Some(1)), page(Some(2))];
        let g = groups(&paged);
        assert_eq!(
            g.iter()
                .map(|g| (g.first, g.pages.len()))
                .collect::<Vec<_>>(),
            vec![(0, 1), (1, 2), (3, 1)]
        );
        // Pages cut from a joined strip have no single image: one row each.
        let strip = [page(None), page(None), page(None)];
        assert_eq!(groups(&strip).len(), 3);
    }
}
