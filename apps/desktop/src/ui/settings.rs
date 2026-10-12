//! Preferences.

use super::ask_enable_browser;
use crate::logic::human_bytes;
use crate::state::{paths, AppState, BootState, Modal, ModalAction};
use catalog::browser;
use converter::process::SpreadMode;
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use device_profiles::{Device, Firmware};
use dioxus::prelude::*;

#[component]
pub fn SettingsPage() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let c = &s.conversion;
    let services = state.services.read().clone();
    let browser_size = services
        .as_ref()
        .map(|sv| browser::size_on_disk(&sv.data_dir))
        .unwrap_or(0);
    let on_macos = !browser::supported();
    let starting = matches!(*state.boot.read(), BootState::Starting { .. });

    let update = move |change: fn(&mut library::Settings, String), value: String| {
        let mut settings = state.settings;
        change(&mut settings.write(), value);
        state.save_settings();
    };

    rsx! {
        h1 { "Settings" }

        div { class: "card",
            h2 { "Your reader" }
            div { class: "field",
                label { "Device" }
                select {
                    value: if s.device == Device::XteinkX4 { "x4" } else { "x4pro" },
                    onchange: move |e| update(|s, v| s.device = if v == "x4" { Device::XteinkX4 } else { Device::XteinkX4Pro }, e.value()),
                    option { value: "x4pro", selected: s.device == Device::XteinkX4Pro, "Xteink X4 Pro" }
                    option { value: "x4", selected: s.device == Device::XteinkX4, "Xteink X4" }
                }
            }
            div { class: "field",
                label { "Firmware" }
                select {
                    value: if s.firmware == Firmware::CrossPoint { "cross" } else { "stock" },
                    onchange: move |e| update(|s, v| s.firmware = if v == "cross" { Firmware::CrossPoint } else { Firmware::Stock }, e.value()),
                    option { value: "cross", selected: s.firmware == Firmware::CrossPoint, "CrossPoint" }
                    option { value: "stock", selected: s.firmware == Firmware::Stock, "Stock (the one it came with)" }
                }
            }
        }

        div { class: "card",
            h2 { "How pages look" }
            div { class: "field",
                label { "Quality" }
                select {
                    onchange: move |e| update(|s, v| s.conversion.format = if v == "xtc" { Format::Xtc } else { Format::Xtch }, e.value()),
                    option { value: "xtch", selected: c.format == Format::Xtch, "4 grays (XTCH), best for manga" }
                    option { value: "xtc", selected: c.format == Format::Xtc, "Black & white (XTC), smaller files" }
                }
            }
            div { class: "field",
                label { "Reading direction" }
                select {
                    onchange: move |e| update(|s, v| s.conversion.direction = if v == "ltr" { ReadDirection::LeftToRight } else { ReadDirection::RightToLeft }, e.value()),
                    option { value: "rtl", selected: c.direction == ReadDirection::RightToLeft, "Right to left (manga)" }
                    option { value: "ltr", selected: c.direction == ReadDirection::LeftToRight, "Left to right (comics, manhwa)" }
                }
            }
            div { class: "field",
                label { "Double-page spreads" }
                select {
                    onchange: move |e| update(|s, v| s.conversion.spread = match v.as_str() { "fit" => SpreadMode::Fit, "rotate" => SpreadMode::Rotate, _ => SpreadMode::Split }, e.value()),
                    option { value: "split", selected: c.spread == SpreadMode::Split, "Split into two pages" }
                    option { value: "rotate", selected: c.spread == SpreadMode::Rotate, "Turn sideways" }
                    option { value: "fit", selected: c.spread == SpreadMode::Fit, "Shrink to fit" }
                }
            }
            div { class: "field",
                label { "Contrast ({c.contrast})" }
                input {
                    r#type: "range", min: "0", max: "100", value: "{c.contrast}",
                    oninput: move |e| update(|s, v| s.conversion.contrast = v.parse().unwrap_or(55), e.value()),
                }
                span { class: "hint muted small", "50 leaves the page as it is. A little higher (55) usually looks better on e-paper." }
            }
            div { class: "field",
                label { "Shading style" }
                select {
                    onchange: move |e| update(|s, v| s.conversion.dither = match v.as_str() { "atkinson" => Dither::Atkinson, "ordered" => Dither::Ordered, "none" => Dither::Threshold, _ => Dither::FloydSteinberg }, e.value()),
                    option { value: "fs", selected: c.dither == Dither::FloydSteinberg, "Smooth (recommended)" }
                    option { value: "atkinson", selected: c.dither == Dither::Atkinson, "High contrast" }
                    option { value: "ordered", selected: c.dither == Dither::Ordered, "Patterned" }
                    option { value: "none", selected: c.dither == Dither::Threshold, "None (crisp line art)" }
                }
            }
            div { class: "field",
                label { "Trim blank margins" }
                input {
                    r#type: "checkbox", checked: c.crop == converter::CropMode::Margins,
                    onchange: move |e| {
                        let mut settings = state.settings;
                        settings.write().conversion.crop = if e.checked() { converter::CropMode::Margins } else { converter::CropMode::Full };
                        state.save_settings();
                    },
                }
            }
        }

        div { class: "card",
            h2 { "Where files go" }
            div { class: "field",
                label { "Save to folder" }
                input {
                    r#type: "text",
                    placeholder: "{paths().default_export_dir().display()}",
                    value: "{s.export.output_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default()}",
                    oninput: move |e| update(|s, v| s.export.output_dir = (!v.trim().is_empty()).then(|| v.trim().into()), e.value()),
                }
            }
            div { class: "field",
                label { "Folder on the reader" }
                input {
                    r#type: "text", value: "{s.export.device_folder}",
                    oninput: move |e| update(|s, v| s.export.device_folder = v, e.value()),
                }
            }
            div { class: "field",
                label { "Reader Wi-Fi address" }
                input {
                    r#type: "text", placeholder: "http://192.168.3.3 (optional)",
                    value: "{s.export.webdav_url.clone().unwrap_or_default()}",
                    oninput: move |e| update(|s, v| s.export.webdav_url = (!v.trim().is_empty()).then(|| v.trim().to_string()), e.value()),
                }
                span { class: "hint muted small", "For sending over Wi-Fi to CrossPoint's file transfer. Not yet tested on a real device." }
            }
        }

        div { class: "card",
            h2 { "Browser support" }
            p { class: "muted",
                "A few sources need a real web browser to download chapters. Turning this on downloads about 270 MB once. It is off by default."
            }
            if on_macos {
                div { class: "banner small", "Browser support is not available on macOS." }
            } else if s.browser_support {
                p { "On. Using {human_bytes(browser_size)} of disk space." }
                button {
                    class: "btn danger",
                    disabled: starting,
                    onclick: move |_| state.ask(Modal {
                        title: "Remove browser support?".into(),
                        body: browser::REMOVE_WARNING.into(),
                        confirm: "Remove it".into(),
                        cancel: "Keep it".into(),
                        danger: true,
                        action: ModalAction::DisableBrowser,
                    }),
                    "Remove browser support…"
                }
            } else {
                p { "Off." }
                button { class: "btn", disabled: starting, onclick: move |_| ask_enable_browser(state, None), "Turn on…" }
            }
        }

        div { class: "card",
            h2 { "About" }
            p { class: "muted small",
                "Catalog engine: Suwayomi {catalog::release::VERSION_TAG}"
                if let Some(sv) = &services { ", running on this computer only (port {sv.port})" }
                br {}
                "Your data: {paths().root().display()}"
            }
        }
    }
}
