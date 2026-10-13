//! Preferences.

use super::ask_enable_browser;

use super::devices::DevicesEditor;
use super::presets_lab::PresetsLab;
use super::widgets::Icon;
use crate::logic::{human_bytes, plural};
use crate::state::{paths, AppState, BootState, Modal, ModalAction, Page, SettingsSection};
use catalog::browser;
use device_profiles::Firmware;
use dioxus::prelude::*;
use library::{DownloadLimits, DownloadSpeed};

/// The sections, in the order the sidebar lists them.
const SECTIONS: [(SettingsSection, &str, &str); 6] = [
    (SettingsSection::Reader, "hard-drive", "Your reader"),
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
        div { class: "settings",
            nav { class: "settings-nav",
                for (sec, icon, label) in SECTIONS {
                    button {
                        key: "{label}",
                        class: if sec == section { "nav-item active" } else { "nav-item" },
                        onclick: move |_| { let mut current = state.settings_section; current.set(sec); },
                        Icon { name: icon, size: 17 }
                        "{label}"
                    }
                }
            }
            div { class: "settings-content",
                {match section {
                    SettingsSection::Reader => rsx! {
                        DevicesEditor {}
                        Group { title: "Firmware" }
                        Row { label: "Firmware", desc: "What the reader runs. It decides which file types it can open.",
                            select {
                                value: if s.firmware == Firmware::CrossPoint { "cross" } else { "stock" },
                                onchange: move |e| update(|s, v| s.firmware = if v == "cross" { Firmware::CrossPoint } else { Firmware::Stock }, e.value()),
                                option { value: "cross", selected: s.firmware == Firmware::CrossPoint, "CrossPoint" }
                                option { value: "stock", selected: s.firmware == Firmware::Stock, "Stock (the one it came with)" }
                            }
                        }
                    },
                    SettingsSection::Presets => rsx! { PresetsLab {} },
                    SettingsSection::Downloads => rsx! {
                        h1 { class: "sx-title", "Downloads" }
                        Group { title: "Speed" }
                        Row {
                            label: "Download speed",
                            desc: format!(
                                "{} Right now: {} at a time, {} pages in flight.",
                                match s.download.speed {
                                    DownloadSpeed::Gentle => "Slow and careful.",
                                    DownloadSpeed::Normal => "A good balance.",
                                    DownloadSpeed::Fast => "Quick; some sources may refuse requests.",
                                    DownloadSpeed::Ultra => "The fastest, and the recommended one.",
                                    DownloadSpeed::Custom => "Your own numbers.",
                                },
                                plural(limits.chapters, "chapter"),
                                limits.pages
                            ),
                            div { class: "seg",
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
                        }
                        if s.download.speed == DownloadSpeed::Custom {
                            Row { label: "Chapters at a time", desc: format!("1 to {}", DownloadLimits::MAX_CHAPTERS),
                                input {
                                    r#type: "number", min: "1", max: "{DownloadLimits::MAX_CHAPTERS}", value: "{s.download.custom_chapters}",
                                    oninput: move |e| update(|s, v| s.download.custom_chapters = v.trim().parse().unwrap_or(1), e.value()),
                                }
                            }
                            Row { label: "Pages in flight", desc: format!("1 to {}. Past about 50 made no difference in tests.", DownloadLimits::MAX_PAGES),
                                input {
                                    r#type: "number", min: "1", max: "{DownloadLimits::MAX_PAGES}", value: "{s.download.custom_pages}",
                                    oninput: move |e| update(|s, v| s.download.custom_pages = v.trim().parse().unwrap_or(1), e.value()),
                                }
                            }
                            div { class: "banner warn small",
                                Icon { name: "triangle-alert", size: 16 }
                                "High numbers can get your address blocked by a source."
                            }
                        }
                        p { class: "sx-note", "If a source starts refusing requests, MangaPoint slows down by itself and speeds up again later." }

                        Group { title: "Mixing sources" }
                        Row { label: "Share chapters between sources", desc: "Different chapters come from different sources at once, which adds their speeds together.",
                            input {
                                r#type: "checkbox", checked: s.download.spread_sources,
                                onchange: move |e| update(|s, v| s.download.spread_sources = v == "true", e.checked().to_string()),
                            }
                        }
                        Row { label: "Race every source", desc: "Asks every source for each chapter and keeps the first to finish. Fastest, but sends the most requests.",
                            input {
                                r#type: "checkbox", checked: s.download.race_sources,
                                onchange: move |e| update(|s, v| s.download.race_sources = v == "true", e.checked().to_string()),
                            }
                        }
                        p { class: "sx-note", "Both can mix translations within one series. Leave them off for a consistent read." }

                        Group { title: "Saved pages" }
                        Row {
                            label: "Page quality",
                            desc: if s.shrink_downloads {
                                "Tall pages are shrunk to twice your reader's screen, roughly halving disk use. Applies to new downloads."
                            } else {
                                "Pages are kept exactly as downloaded. Recommended."
                            },
                            div { class: "seg",
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
                        }
                        Row {
                            label: "Your library",
                            desc: format!("{}, {}, {}", plural(series_count, "series"), plural(chapter_count, "chapter"), human_bytes(library_bytes)),
                            button { class: "btn", onclick: move |_| state.go(Page::Library), Icon { name: "book-open", size: 15 } "Open Library" }
                        }
                    },
                    SettingsSection::Files => rsx! {
                        h1 { class: "sx-title", "Files" }
                        Group { title: "Exported files" }
                        Row { label: "Save to folder", desc: "Where the files for your reader are made.",
                            div { class: "sx-inline",
                                input {
                                    r#type: "text",
                                    placeholder: "{paths().default_export_dir().display()}",
                                    value: "{s.export.output_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default()}",
                                    oninput: move |e| update(|s, v| s.export.output_dir = (!v.trim().is_empty()).then(|| v.trim().into()), e.value()),
                                }
                                button { class: "btn", title: "Open this folder",
                                    onclick: move |_| {
                                        let dir = state.settings.read().export.output_dir.clone().unwrap_or_else(|| paths().default_export_dir());
                                        let _ = std::fs::create_dir_all(&dir);
                                        super::widgets::open_folder(&dir);
                                    },
                                    Icon { name: "folder-open", size: 15 } "Open"
                                }
                            }
                        }
                        Row { label: "Folder on the reader", desc: "Where files go when you export straight to the reader.",
                            input {
                                r#type: "text", value: "{s.export.device_folder}",
                                oninput: move |e| update(|s, v| s.export.device_folder = v, e.value()),
                            }
                        }
                        Row { label: "Reader Wi-Fi address", desc: "For sending over Wi-Fi to CrossPoint's file transfer. Not yet tested on a real device.",
                            input {
                                r#type: "text", placeholder: "http://192.168.3.3 (optional)",
                                value: "{s.export.webdav_url.clone().unwrap_or_default()}",
                                oninput: move |e| update(|s, v| s.export.webdav_url = (!v.trim().is_empty()).then(|| v.trim().to_string()), e.value()),
                            }
                        }
                        Group { title: "Downloaded chapters" }
                        Row {
                            label: "Download folder",
                            desc: format!("New downloads are kept in {}. Series you already have stay where they are.", crate::state::library_files().root().display()),
                            button { class: "btn", title: "Choose where new downloads are kept",
                                onclick: move |_| {
                                    spawn(async move {
                                        let start = Some(crate::state::library_files().root().to_path_buf());
                                        if let Some(dir) = super::widgets::pick_folder("Where should downloads be kept?", start).await {
                                            super::work::set_download_folder(state, dir);
                                        }
                                    });
                                },
                                Icon { name: "folder-open", size: 15 }
                                "Choose folder…"
                            }
                        }
                        Row { label: "Ask each time", desc: "Show where files go whenever you start a download.",
                            input {
                                r#type: "checkbox", checked: s.ask_download_folder,
                                onchange: move |e| { let mut st = state.settings; st.write().ask_download_folder = e.checked(); state.save_settings(); },
                            }
                        }
                        Row { label: "Open the downloads folder", desc: "Where the pages you download are kept.",
                            button { class: "btn", title: "Open the downloads folder",
                                onclick: move |_| {
                                    let dir = crate::state::library_files().root().to_path_buf();
                                    let _ = std::fs::create_dir_all(&dir);
                                    super::widgets::open_folder(&dir);
                                },
                                Icon { name: "folder-open", size: 15 } "Open folder"
                            }
                        }
                    },
                    SettingsSection::Browser => rsx! {
                        h1 { class: "sx-title", "Browser support" }
                        Row {
                            label: "Browser support",
                            desc: if on_macos {
                                "Not available on macOS.".to_string()
                            } else if s.browser_support {
                                format!("On, using {} of disk space.", human_bytes(browser_size))
                            } else {
                                "A few sources need a real web browser. Turning this on downloads about 270 MB, once.".to_string()
                            },
                            if on_macos {
                            } else if s.browser_support {
                                button {
                                    class: "btn danger", disabled: starting,
                                    onclick: move |_| state.ask(Modal {
                                        title: "Remove browser support?".into(),
                                        body: browser::REMOVE_WARNING.into(),
                                        confirm: "Remove it".into(),
                                        cancel: "Keep it".into(),
                                        danger: true,
                                        action: ModalAction::DisableBrowser,
                                    }),
                                    "Remove…"
                                }
                            } else {
                                button { class: "btn", disabled: starting, onclick: move |_| ask_enable_browser(state, None), "Turn on…" }
                            }
                        }
                    },
                    SettingsSection::About => rsx! {
                        h1 { class: "sx-title", "About" }
                        Row {
                            label: "Catalog engine",
                            desc: match &services {
                                Some(sv) => format!("Suwayomi {}, running on this computer only (port {}).", catalog::release::VERSION_TAG, sv.port),
                                None => format!("Suwayomi {}", catalog::release::VERSION_TAG),
                            },
                        }
                        Row { label: "Your data", desc: "{paths().root().display()}",
                            button { class: "btn", title: "Open this folder",
                                onclick: move |_| {
                                    let dir = paths().root().to_path_buf();
                                    let _ = std::fs::create_dir_all(&dir);
                                    super::widgets::open_folder(&dir);
                                },
                                Icon { name: "folder-open", size: 15 } "Open folder"
                            }
                        }
                    },
                }}
            }
        }
    }
}

/// A heading that groups a few settings.
#[component]
fn Group(title: String) -> Element {
    rsx! { h2 { class: "sx-group", "{title}" } }
}

/// One setting: what it is on the left, how to change it on the right.
#[component]
fn Row(label: String, desc: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "{label}" }
                if let Some(text) = desc { div { class: "sx-desc", "{text}" } }
            }
            div { class: "sx-control", {children} }
        }
    }
}
