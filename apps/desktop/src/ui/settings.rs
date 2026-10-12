//! Preferences.

use super::ask_enable_browser;
use super::conversion::ConversionControls;
use super::presets_lab::PresetsLab;
use super::widgets::Icon;
use crate::logic::human_bytes;
use crate::state::{paths, AppState, BootState, Modal, ModalAction, Page, SettingsSection};
use catalog::browser;
use device_profiles::{Device, Firmware};
use dioxus::prelude::*;
use library::{DownloadLimits, DownloadSpeed};

/// The sections, in the order the sidebar lists them.
const SECTIONS: [(SettingsSection, &str, &str); 7] = [
    (SettingsSection::Reader, "hard-drive", "Your reader"),
    (
        SettingsSection::Pages,
        "sliders-horizontal",
        "How pages look",
    ),
    (SettingsSection::Presets, "layers", "Presets"),
    (SettingsSection::Downloads, "download", "Downloads"),
    (SettingsSection::Files, "folder-open", "Files"),
    (SettingsSection::Browser, "globe", "Browser support"),
    (SettingsSection::About, "info", "About"),
];

#[component]
pub fn SettingsPage() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let limits = s.download.limits();
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

    let section = *state.settings_section.read();

    rsx! {
        h1 { "Settings" }
        div { class: "settings",
            nav { class: "settings-nav",
                for (sec, icon, label) in SECTIONS {
                    button {
                        key: "{label}",
                        class: if sec == section { "nav-item on" } else { "nav-item" },
                        onclick: move |_| { let mut current = state.settings_section; current.set(sec); },
                        Icon { name: icon, size: 17 }
                        "{label}"
                    }
                }
            }
            div { class: "settings-content",
                {match section {
                    SettingsSection::Reader => rsx! {
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


                    },
                    SettingsSection::Pages => rsx! {
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


                        div { class: "banner small",
                            Icon { name: "layers", size: 16 }
                            div {
                                "Presets (manga, manhwa, manhua and your own) are on their own page, where you can try them on a test page."
                                " "
                                button { class: "btn small", onclick: move |_| { let mut current = state.settings_section; current.set(SettingsSection::Presets); }, "Open presets" }
                            }
                        }
                    },
                    SettingsSection::Presets => rsx! { PresetsLab {} },
                    SettingsSection::Downloads => rsx! {
        div { class: "card",
            h2 { Icon { name: "download", size: 17 } "Download speed" }
            p { class: "muted small",
                "Faster downloads ask a source for several pages at once. Some sources hardly get faster, and a source that is pushed too hard can block your internet address for a while."
            }
            div { class: "seg", style: "max-width: 520px",
                for (speed, label) in [
                    (DownloadSpeed::Gentle, "Gentle"),
                    (DownloadSpeed::Normal, "Normal"),
                    (DownloadSpeed::Fast, "Fast"),
                    (DownloadSpeed::Ultra, "Ultra"),
                    (DownloadSpeed::Custom, "Advanced"),
                ] {
                    button {
                        key: "{label}",
                        class: if s.download.speed == speed { "on" } else { "" },
                        onclick: move |_| { let mut st = state.settings; st.write().download.speed = speed; state.save_settings(); },
                        "{label}"
                    }
                }
            }
            p { class: "muted small", style: "margin: 8px 0 0",
                {match s.download.speed {
                    DownloadSpeed::Gentle => "Slow and careful. Best for sources that block easily.",
                    DownloadSpeed::Normal => "A good balance of speed and care.",
                    DownloadSpeed::Fast => "Quick. Some sources may start refusing requests.",
                    DownloadSpeed::Ultra => "The fastest: many chapters and pages at once. Every chapter comes from the same source, so the translation stays consistent. Recommended.",
                    DownloadSpeed::Custom => "Choose the numbers yourself.",
                }}
                " Right now: {limits.chapters} chapter(s) at a time, {limits.pages} pages in flight."
            }
            p { class: "muted small", style: "margin: 4px 0 0",
                "If a source starts refusing requests, MangaPoint slows down by itself and speeds up again after a while. Some sources cap their own speed (MangaFire gives about 2 pages a second however you ask)."
            }
            if s.download.speed == DownloadSpeed::Custom {
                div { class: "field",
                    label { "Chapters at a time" }
                    input {
                        r#type: "number", min: "1", max: "{DownloadLimits::MAX_CHAPTERS}", value: "{s.download.custom_chapters}",
                        oninput: move |e| update(|s, v| s.download.custom_chapters = v.trim().parse().unwrap_or(1), e.value()),
                    }
                    span { class: "hint muted small", "1 to {DownloadLimits::MAX_CHAPTERS}" }
                }
                div { class: "field",
                    label { "Pages in flight" }
                    input {
                        r#type: "number", min: "1", max: "{DownloadLimits::MAX_PAGES}", value: "{s.download.custom_pages}",
                        oninput: move |e| update(|s, v| s.download.custom_pages = v.trim().parse().unwrap_or(1), e.value()),
                    }
                    span { class: "hint muted small", "1 to {DownloadLimits::MAX_PAGES}, shared by all the chapters downloading together. In the tests, going past about 50 made no difference." }
                }
                div { class: "banner warn small",
                    Icon { name: "triangle-alert", size: 16 }
                    "High numbers can get you blocked."
                }
                h3 { class: "group", style: "margin: 14px 0 4px; font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted)", "Mixing sources" }
                p { class: "muted small", style: "margin: 0 0 6px",
                    "Both options below can put chapters of one series from different sources, and so from different translations. Leave them off if you want a consistent series."
                }
                label { class: "check-line",
                    input {
                        r#type: "checkbox", checked: s.download.spread_sources,
                        onchange: move |e| update(|s, v| s.download.spread_sources = v == "true", e.checked().to_string()),
                    }
                    span { "Share chapters between sources" }
                }
                p { class: "muted small", style: "margin: 2px 0 8px",
                    "When several sources offer a chapter, different chapters download from different sources at once, which adds their speeds together."
                }
                label { class: "check-line",
                    input {
                        r#type: "checkbox", checked: s.download.race_sources,
                        onchange: move |e| update(|s, v| s.download.race_sources = v == "true", e.checked().to_string()),
                    }
                    span { "Race every source" }
                }
                p { class: "muted small", style: "margin: 2px 0 0",
                    "Asks every source that has a chapter at the same time and keeps whichever finishes first. Fastest per chapter, but wastes downloads and sends the most requests."
                }
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


                    },
                    SettingsSection::Files => rsx! {
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


                    },
                    SettingsSection::Browser => rsx! {
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


                    },
                    SettingsSection::About => rsx! {
        div { class: "card",
            h2 { Icon { name: "info", size: 17 } "About" }
            p { class: "muted small",
                "Catalog engine: Suwayomi {catalog::release::VERSION_TAG}"
                if let Some(sv) = &services { ", running on this computer only (port {sv.port})" }
                br {}
                "Your data: {paths().root().display()}"
            }
        }
                    },
                }}
            }
        }
    }
}
