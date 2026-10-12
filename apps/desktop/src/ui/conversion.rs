//! The controls for how pages are converted, shared by Settings (your
//! starting defaults) and the Preview tab (this one series).

use converter::process::{CropMode, SpreadMode};
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use dioxus::prelude::*;
use library::ConversionSettings;
use std::rc::Rc;

type Edit = fn(&mut ConversionSettings, String);

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

    let ch = change.clone();
    let on_format = move |e: Event<FormData>| {
        ch(
            |s, v| {
                s.format = if v == "xtc" {
                    Format::Xtc
                } else {
                    Format::Xtch
                }
            },
            e.value(),
        )
    };
    let ch = change.clone();
    let on_direction = move |e: Event<FormData>| {
        ch(
            |s, v| {
                s.direction = if v == "ltr" {
                    ReadDirection::LeftToRight
                } else {
                    ReadDirection::RightToLeft
                }
            },
            e.value(),
        )
    };
    let ch = change.clone();
    let on_spread = move |e: Event<FormData>| {
        ch(
            |s, v| {
                s.spread = match v.as_str() {
                    "fit" => SpreadMode::Fit,
                    "rotate" => SpreadMode::Rotate,
                    _ => SpreadMode::Split,
                }
            },
            e.value(),
        )
    };
    let ch = change.clone();
    let on_contrast =
        move |e: Event<FormData>| ch(|s, v| s.contrast = v.parse().unwrap_or(55), e.value());
    let ch = change.clone();
    let on_dither = move |e: Event<FormData>| {
        ch(
            |s, v| {
                s.dither = match v.as_str() {
                    "atkinson" => Dither::Atkinson,
                    "ordered" => Dither::Ordered,
                    "none" => Dither::Threshold,
                    _ => Dither::FloydSteinberg,
                }
            },
            e.value(),
        )
    };
    let ch = change.clone();
    let on_trim = move |e: Event<FormData>| {
        ch(
            |s, v| {
                s.crop = if v == "true" {
                    CropMode::Margins
                } else {
                    CropMode::Full
                }
            },
            e.checked().to_string(),
        )
    };
    let class = if compact {
        "controls compact"
    } else {
        "controls"
    };

    rsx! {
        div { class: "{class}",
            div { class: "field",
                label { "Quality" }
                select { onchange: on_format,
                    option { value: "xtch", selected: c.format == Format::Xtch, "4 grays (XTCH), best for manga" }
                    option { value: "xtc", selected: c.format == Format::Xtc, "Black & white (XTC), smaller files" }
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
                label { "Trim blank margins" }
                input { r#type: "checkbox", checked: c.crop == CropMode::Margins, onchange: on_trim }
            }
        }
    }
}
