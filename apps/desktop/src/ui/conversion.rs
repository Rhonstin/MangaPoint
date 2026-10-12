//! The controls for how pages are converted, shared by Settings (your
//! starting defaults) and the Preview tab (this one series).

use converter::process::{CropMode, Layout, Margins, SpreadMode};
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use dioxus::prelude::*;
use library::ConversionSettings;
use std::rc::Rc;

type Edit = fn(&mut ConversionSettings, String);

/// A percentage typed or dragged by the user, held to what the converter allows.
fn percent(raw: &str) -> u8 {
    raw.trim()
        .parse::<u8>()
        .unwrap_or(0)
        .min(Margins::MAX_PERCENT)
}

fn number(raw: &str, fallback: u8, max: u8) -> u8 {
    raw.trim().parse::<u8>().unwrap_or(fallback).min(max)
}

#[component]
pub fn ConversionControls(
    value: ConversionSettings,
    onchange: EventHandler<ConversionSettings>,
    compact: Option<bool>,
) -> Element {
    let compact = compact.unwrap_or(false);
    let c = value.clone();
    // Apply one edit to a copy of the settings and report the result.
    let change = Rc::new(move |edit: Edit, raw: String| {
        let mut v = value.clone();
        edit(&mut v, raw);
        onchange.call(v);
    });
    // A handler that feeds the control's value to `edit`.
    let on = |edit: Edit| {
        let ch = change.clone();
        move |e: Event<FormData>| ch(edit, e.value())
    };
    let on_check = |edit: Edit| {
        let ch = change.clone();
        move |e: Event<FormData>| ch(edit, e.checked().to_string())
    };

    let on_layout = on(|s, v| {
        s.layout = match v.as_str() {
            "pages" => Layout::Manga,
            "strip" => Layout::Webtoon,
            _ => Layout::Auto,
        }
    });
    let on_format = on(|s, v| {
        s.format = if v == "xtc" {
            Format::Xtc
        } else {
            Format::Xtch
        }
    });
    let on_direction = on(|s, v| {
        s.direction = if v == "ltr" {
            ReadDirection::LeftToRight
        } else {
            ReadDirection::RightToLeft
        }
    });
    let on_spread = on(|s, v| {
        s.spread = match v.as_str() {
            "fit" => SpreadMode::Fit,
            "rotate" => SpreadMode::Rotate,
            _ => SpreadMode::Split,
        }
    });
    let on_overlap = on(|s, v| s.overlap_percent = number(&v, 8, 30));
    let on_contrast = on(|s, v| s.contrast = number(&v, 55, 100));
    let on_dither = on(|s, v| {
        s.dither = match v.as_str() {
            "atkinson" => Dither::Atkinson,
            "ordered" => Dither::Ordered,
            "none" => Dither::Threshold,
            _ => Dither::FloydSteinberg,
        }
    });
    let on_levels = on_check(|s, v| s.auto_levels = v == "true");
    let on_trim = on_check(|s, v| {
        s.crop = if v == "true" {
            CropMode::Margins
        } else {
            CropMode::Full
        }
    });
    let on_sensitivity = on(|s, v| s.trim_sensitivity = number(&v, 50, 100));
    let on_padding = on(|s, v| s.trim_padding = number(&v, 4, 40));
    let on_top = on(|s, v| s.margins.top = percent(&v));
    let on_bottom = on(|s, v| s.margins.bottom = percent(&v));
    let on_left = on(|s, v| s.margins.left = percent(&v));
    let on_right = on(|s, v| s.margins.right = percent(&v));
    let ch = change.clone();
    let clear_margins = move |_| ch(|s, _| s.margins = Margins::default(), String::new());

    let class = if compact {
        "controls compact"
    } else {
        "controls"
    };
    let trimming = c.crop == CropMode::Margins;
    let strips = c.layout != Layout::Manga;
    let layout_value = match c.layout {
        Layout::Auto => "auto",
        Layout::Manga => "pages",
        Layout::Webtoon => "strip",
    };

    rsx! {
        div { class: "{class}",
            h3 { class: "group", "Layout" }
            div { class: "field",
                label { "Page layout" }
                select { value: "{layout_value}", onchange: on_layout,
                    option { value: "auto", selected: c.layout == Layout::Auto, "Automatic" }
                    option { value: "pages", selected: c.layout == Layout::Manga, "Pages" }
                    option { value: "strip", selected: c.layout == Layout::Webtoon, "Long strips (webtoon)" }
                }
                span { class: "hint muted small",
                    match c.layout {
                        Layout::Auto => "Very tall images become strips, everything else stays a page.",
                        Layout::Manga => "Every image is shown as one page.",
                        Layout::Webtoon => "Tall images are cut into screen-sized slices.",
                    }
                }
            }
            div { class: "field",
                label { "Reading direction" }
                select { onchange: on_direction,
                    option { value: "rtl", selected: c.direction == ReadDirection::RightToLeft, "Right to left (manga)" }
                    option { value: "ltr", selected: c.direction == ReadDirection::LeftToRight, "Left to right (comics, manhwa)" }
                }
            }
            div { class: "field",
                label { "Double-page spreads" }
                select { onchange: on_spread,
                    option { value: "split", selected: c.spread == SpreadMode::Split, "Split into two pages" }
                    option { value: "rotate", selected: c.spread == SpreadMode::Rotate, "Turn sideways" }
                    option { value: "fit", selected: c.spread == SpreadMode::Fit, "Shrink to fit" }
                }
            }
            if strips {
                div { class: "field",
                    label { "Strip overlap ({c.overlap_percent}%)" }
                    input { r#type: "range", min: "0", max: "30", value: "{c.overlap_percent}", oninput: on_overlap }
                    span { class: "hint muted small", "How much of the last screen repeats at the top of the next, so you don't lose your place." }
                }
            }

            h3 { class: "group", "Cropping" }
            div { class: "field",
                label { "Trim blank margins" }
                label { class: "check-line",
                    input { r#type: "checkbox", checked: trimming, onchange: on_trim }
                    span { class: "muted", if trimming { "Automatically" } else { "Off, keep the whole image" } }
                }
            }
            if trimming {
                div { class: "field",
                    label { "Sensitivity ({c.trim_sensitivity})" }
                    input { r#type: "range", min: "0", max: "100", value: "{c.trim_sensitivity}", oninput: on_sensitivity }
                    span { class: "hint muted small", "Higher notices fainter marks, so it trims less. Lower trims harder." }
                }
                div { class: "field",
                    label { "Border kept ({c.trim_padding} px)" }
                    input { r#type: "range", min: "0", max: "40", value: "{c.trim_padding}", oninput: on_padding }
                    span { class: "hint muted small", "Space left around the art after trimming." }
                }
            }
            div { class: "field",
                label { "Cut from the edges (%)" }
                div { class: "margins",
                    label { span { "Top" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.top}", oninput: on_top } }
                    label { span { "Bottom" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.bottom}", oninput: on_bottom } }
                    label { span { "Left" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.left}", oninput: on_left } }
                    label { span { "Right" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.right}", oninput: on_right } }
                }
                span { class: "hint muted small",
                    "Removes the same strip from every page before trimming. Handy for watermarks and site banners."
                    if !c.margins.is_none() {
                        " "
                        button { class: "btn ghost small", onclick: clear_margins, "Clear" }
                    }
                }
            }

            h3 { class: "group", "Look" }
            div { class: "field",
                label { "Quality" }
                select { onchange: on_format,
                    option { value: "xtch", selected: c.format == Format::Xtch, "4 grays (XTCH), best for manga" }
                    option { value: "xtc", selected: c.format == Format::Xtc, "Black & white (XTC), smaller files" }
                }
            }
            div { class: "field",
                label { "Contrast ({c.contrast})" }
                input { r#type: "range", min: "0", max: "100", value: "{c.contrast}", oninput: on_contrast }
                span { class: "hint muted small", "50 leaves the page as it is. A little higher (55) usually looks better on e-paper." }
            }
            div { class: "field",
                label { "Shading style" }
                select { onchange: on_dither,
                    option { value: "fs", selected: c.dither == Dither::FloydSteinberg, "Smooth (recommended)" }
                    option { value: "atkinson", selected: c.dither == Dither::Atkinson, "High contrast" }
                    option { value: "ordered", selected: c.dither == Dither::Ordered, "Patterned" }
                    option { value: "none", selected: c.dither == Dither::Threshold, "None (crisp line art)" }
                }
            }
            div { class: "field",
                label { "Adjust tones" }
                label { class: "check-line",
                    input { r#type: "checkbox", checked: c.auto_levels, onchange: on_levels }
                    span { class: "muted", "Make paper white and ink black" }
                }
            }
        }
    }
}
