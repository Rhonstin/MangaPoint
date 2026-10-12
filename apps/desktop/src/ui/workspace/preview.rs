//! Tab 3: the pages the chapter becomes on the reader, and the settings that
//! shape them.
//!
//! Every page is shown as the exact image that goes into the file. Where the
//! plan got a page wrong you fix it on the page itself: cut a page in two,
//! join it to the one before, move where it starts, leave it out, or choose
//! what happens to a double page. Those corrections are saved with the series
//! and applied again on export.

use crate::state::{AppState, PreviewPage, SeriesTab, Zoom};
use crate::ui::conversion::ConversionControls;
use crate::ui::presets::{apply_to_series, PresetPicker};
use crate::ui::widgets::{Chip, EmptyState, Icon};
use crate::ui::work::{edit_pages, prepare_preview, schedule_preview};
use converter::process::SpreadMode;
use dioxus::prelude::*;
use std::rc::Rc;

/// How far one press of "earlier" or "later" moves a cut, in strip units
/// (a strip is 1000 wide, so a screen is roughly 1600 units tall).
const NUDGE_UNITS: i32 = 40;

/// How wide a page card is drawn, in screen pixels.
fn card_width(zoom: Zoom) -> u32 {
    match zoom {
        Zoom::Fit => 250,
        Zoom::Width => 380,
        Zoom::Actual => 480,
        Zoom::Double => 960,
    }
}

/// What a landscape image currently does, from the pages it produced.
fn spread_mode_of(page: &PreviewPage) -> SpreadMode {
    if page.half.is_some() {
        SpreadMode::Split
    } else if page.rotated {
        SpreadMode::Rotate
    } else {
        SpreadMode::Fit
    }
}

fn skeleton(width: u32) -> Element {
    rsx! { div { class: "skeleton", style: "width: {width}px; aspect-ratio: 3 / 5;" } }
}

#[component]
fn PageCard(index: usize, page: Rc<PreviewPage>, strip: bool, width: u32) -> Element {
    let state = use_context::<AppState>();
    let number = index + 1;
    let class = if page.skipped {
        "page-card skipped"
    } else {
        "page-card"
    };
    let unit_top = page.unit_top;
    let middle = page.unit_top + page.unit_height / 2;
    let image = page.image;
    let current_mode = spread_mode_of(&page);
    // A split spread is two pages from one image; its controls sit on the first.
    let show_spread = page.landscape && page.half != Some(1);
    let skipped = page.skipped;

    rsx! {
        div { class: "{class}", style: "width: {width}px;",
            div { class: "page-card-head",
                strong { "Page {number}" }
                if page.half == Some(0) { Chip { text: "first half" } }
                if page.half == Some(1) { Chip { text: "second half" } }
                if page.rotated { Chip { text: "turned" } }
                if page.manual { Chip { text: "set by you", kind: Some("accent".to_string()) } }
                if skipped { Chip { text: "left out", kind: Some("warn".to_string()) } }
            }
            match &page.shot {
                Some(shot) => rsx! {
                    img { class: "page-shot", src: "{shot.url}", alt: "Page {number}",
                        style: "width: 100%; height: auto; aspect-ratio: {shot.width} / {shot.height};" }
                },
                None => skeleton(width),
            }
            div { class: "page-card-actions",
                if strip {
                    if index > 0 {
                        button { class: "btn small", title: "Start this page a little higher",
                            onclick: move |_| edit_pages(state, |e| e.nudge_cut(unit_top, -NUDGE_UNITS)),
                            Icon { name: "chevron-up", size: 14 }
                            "Earlier"
                        }
                        button { class: "btn small", title: "Start this page a little lower",
                            onclick: move |_| edit_pages(state, |e| e.nudge_cut(unit_top, NUDGE_UNITS)),
                            Icon { name: "chevron-down", size: 14 }
                            "Later"
                        }
                        button { class: "btn small", title: "Take away the cut above this page and join it to the one before",
                            onclick: move |_| edit_pages(state, |e| e.join_at(unit_top)),
                            Icon { name: "arrow-up-to-line", size: 14 }
                            "Join up"
                        }
                    }
                    button { class: "btn small", title: "Cut this page in the middle, into two pages",
                        onclick: move |_| edit_pages(state, |e| e.split_at(middle)),
                        Icon { name: "plus", size: 14 }
                        "Cut in two"
                    }
                    button { class: "btn small ghost",
                        title: if skipped { "Put this page back in the file" } else { "Leave this page out of the file" },
                        onclick: move |_| edit_pages(state, |e| e.toggle_skip_at(unit_top)),
                        if skipped { "Put back" } else { "Leave out" }
                    }
                } else if let Some(i) = image {
                    if show_spread {
                        div { class: "seg", title: "What to do with this double page",
                            for (mode, label) in [(SpreadMode::Split, "Split"), (SpreadMode::Fit, "Fit"), (SpreadMode::Rotate, "Turn")] {
                                button {
                                    key: "{label}",
                                    class: if current_mode == mode { "on" } else { "" },
                                    onclick: move |_| edit_pages(state, |e| e.set_spread(i as u32, Some(mode))),
                                    "{label}"
                                }
                            }
                        }
                    }
                    button { class: "btn small ghost",
                        title: if skipped { "Put this page back in the file" } else { "Leave this page out of the file" },
                        onclick: move |_| edit_pages(state, |e| e.toggle_skip_image(i as u32)),
                        if skipped { "Put back" } else { "Leave out" }
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
    let kept = p.pages.iter().filter(|page| !page.skipped).count();
    let device = state.settings.read().device_profile();
    let width = card_width(view.zoom);
    let edited = series
        .page_edits
        .get(&current)
        .is_some_and(|e| !e.is_empty());
    let any_skipped = p.pages.iter().any(|page| page.skipped);

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
                        span { class: "muted small",
                            if kept == total { "{total} pages" } else { "{kept} of {total} pages in the file" }
                        }
                    } else if p.busy {
                        span { class: "spin muted", Icon { name: "loader-circle", size: 16 } }
                    }
                    span { class: "grow" }
                    div { class: "seg", title: "How big the pages are drawn",
                        for (zoom, label) in [(Zoom::Fit, "Small"), (Zoom::Width, "Medium"), (Zoom::Actual, "100%"), (Zoom::Double, "200%")] {
                            button {
                                key: "{label}",
                                class: if view.zoom == zoom { "on" } else { "" },
                                onclick: move |_| { let mut pv = state.preview; pv.write().view.zoom = zoom; },
                                "{label}"
                            }
                        }
                    }
                    if any_skipped {
                        button {
                            class: if view.show_skipped { "btn small on" } else { "btn small" },
                            title: "Show the pages you left out, dimmed",
                            onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.show_skipped = !w.view.show_skipped; },
                            Icon { name: "eye", size: 14 }
                            "Left out"
                        }
                    }
                    if edited {
                        button { class: "btn small",
                            title: "Forget every correction you made to this chapter's pages",
                            onclick: move |_| edit_pages(state, |e| *e = converter::PageEdits::default()),
                            Icon { name: "rotate-ccw", size: 14 }
                            "Undo my page changes"
                        }
                    }
                    button {
                        class: "btn small",
                        title: if view.focus { "Show the settings" } else { "Hide the settings to give the pages the whole width" },
                        onclick: move |_| { let mut pv = state.preview; let mut w = pv.write(); w.view.focus = !w.view.focus; },
                        if view.focus { Icon { name: "panel-right-open", size: 14 } "Settings" } else { Icon { name: "panel-right-close", size: 14 } "Hide settings" }
                    }
                }
                if let Some(message) = &p.error {
                    div { class: "banner bad small", Icon { name: "circle-x", size: 16 } "{message}" }
                }
                if total > 0 {
                    p { class: "muted small", style: "margin: 0 0 10px",
                        if p.strip {
                            "This chapter is a long strip: its {p.images} images are joined and cut into pages of {device.size_label()}, at the blank gaps between scenes where there is one. "
                        } else {
                            "This chapter is one page per image, made {device.size_label()} for your {device.name}. "
                        }
                        "Fix any page right on it; your changes are kept for this chapter."
                    }
                }
                div { class: if p.busy { "pages-grid busy" } else { "pages-grid" },
                    for (index, page) in p.pages.iter().enumerate() {
                        if view.show_skipped || !page.skipped {
                            PageCard { key: "{index}-{page.unit_top}", index, page: page.clone(), strip: p.strip, width }
                        }
                    }
                }
                p { class: "muted small", style: "margin: 14px 0 0",
                    "These are the exact images that go into the file. The real screen's gray shades can look a little different; the first time you test on your reader, compare it with this."
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
                        p { class: "muted small",
                            "These settings apply to every chapter of {ws.title}."
                            if overridden > 0 {
                                " {overridden} of them differ from the preset, so they stay as you set them when the preset changes."
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

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> PreviewPage {
        PreviewPage {
            shot: None,
            skipped: false,
            manual: false,
            unit_top: 0,
            unit_height: 0,
            image: Some(0),
            landscape: true,
            half: None,
            rotated: false,
        }
    }

    #[test]
    fn cards_get_wider_with_the_zoom() {
        let widths: Vec<u32> = [Zoom::Fit, Zoom::Width, Zoom::Actual, Zoom::Double]
            .into_iter()
            .map(card_width)
            .collect();
        assert!(widths.windows(2).all(|w| w[0] < w[1]), "{widths:?}");
        assert_eq!(
            card_width(Zoom::Actual),
            480,
            "one screen pixel per device pixel"
        );
    }

    #[test]
    fn a_double_pages_mode_is_read_from_the_pages_it_made() {
        assert_eq!(spread_mode_of(&page()), SpreadMode::Fit);
        let half = PreviewPage {
            half: Some(0),
            ..page()
        };
        assert_eq!(spread_mode_of(&half), SpreadMode::Split);
        let turned = PreviewPage {
            rotated: true,
            ..page()
        };
        assert_eq!(spread_mode_of(&turned), SpreadMode::Rotate);
    }
}
