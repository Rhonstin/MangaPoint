//! Preferences.

use super::ask_enable_browser;
use super::conversion::ConversionControls;
use super::widgets::Icon;
use crate::logic::human_bytes;
use crate::state::{paths, AppState, BootState, Modal, ModalAction, Page};
use catalog::browser;
use device_profiles::{Device, Firmware};
use dioxus::prelude::*;

#[component]
pub fn SettingsPage() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let services = state.services.read().clone();
    let browser_size = services
        .as_ref()
        .map(|sv| browser::size_on_disk(&sv.data_dir))
        .unwrap_or(0);
    let on_macos = !browser::supported();
    let starting = matches!(*state.boot.read(), BootState::Starting { .. });
    let (series_count, chapter_count, library_bytes) = {
        let library = state.library.read();
        (
            library.series.len(),
            library
                .series
                .iter()
                .map(|s| s.chapters.len())
                .sum::<usize>(),
            library.total_bytes(),
        )
    };

    let update = move |change: fn(&mut library::Settings, String), value: String| {
        let mut settings = state.settings;
        change(&mut settings.write(), value);
        state.save_settings();
    };

    rsx! {
        h1 { "Settings" }

        div { class: "card",
            h2 { Icon { name: "hard-drive", size: 17 } "Your reader" }
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
            h2 { Icon { name: "sliders-horizontal", size: 17 } "How pages look" }
            p { class: "muted small", "Your starting point for every new series. Each series can be fine-tuned while you preview it." }
            ConversionControls {
                value: s.conversion.clone(),
                onchange: move |v| {
                    let mut settings = state.settings;
                    settings.write().conversion = v;
                    state.save_settings();
                },
            }
        }

        div { class: "card",
            h2 { Icon { name: "image", size: 17 } "Saved pages" }
            p { class: "muted small",
                "Downloaded chapters are kept on this computer, so you can preview and export them again without downloading."
            }
            div { class: "seg", style: "max-width: 420px",
                button {
                    class: if !s.shrink_downloads { "on" } else { "" },
                    onclick: move |_| { let mut st = state.settings; st.write().shrink_downloads = false; state.save_settings(); },
                    "Full quality"
                }
                button {
                    class: if s.shrink_downloads { "on" } else { "" },
                    onclick: move |_| { let mut st = state.settings; st.write().shrink_downloads = true; state.save_settings(); },
                    "Save space"
                }
            }
            p { class: "muted small", style: "margin: 8px 0 0",
                if s.shrink_downloads {
                    "Tall pages are shrunk to fit twice your reader's screen, which roughly halves the disk space. Applies to chapters you download from now on."
                } else {
                    "Pages are kept exactly as downloaded, for the best quality. This is the recommended setting."
                }
            }
            div { class: "row", style: "margin-top: 12px",
                span { class: "muted small grow",
                    "Library: {series_count} series, {chapter_count} chapters, {human_bytes(library_bytes)}"
                }
                button { class: "btn small", onclick: move |_| state.go(Page::Library), "Open Library" }
            }
        }

        div { class: "card",
            h2 { Icon { name: "folder-open", size: 17 } "Where files go" }
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
            h2 { Icon { name: "globe", size: 17 } "Browser support" }
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
            h2 { Icon { name: "info", size: 17 } "About" }
            p { class: "muted small",
                "Catalog engine: Suwayomi {catalog::release::VERSION_TAG}"
                if let Some(sv) = &services { ", running on this computer only (port {sv.port})" }
                br {}
                "Your data: {paths().root().display()}"
            }
        }
    }
}
