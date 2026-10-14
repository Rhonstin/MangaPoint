//! The app's screens.

pub mod cat;
mod chapter_table;
mod conversion;
mod devices;
mod download_dialog;
mod tasks_page;
mod export_page;
mod hang;
mod icons;
mod library_page;
mod ocean;
mod placeholder;
mod plan_page;
mod presets;
mod scenes;
mod search;
mod settings;
mod setup;
mod shelf;
mod sidebar;
mod sources_page;
mod widgets;
pub mod work;
mod workspace;

use crate::backend;
use crate::state::spawn_forever;
use crate::state::{AppState, BootState, Modal, ModalAction, Page};
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::use_wry_event_handler;
use dioxus::prelude::*;
use sources::SourceManager;
use widgets::{Icon, ProgressBar};

const CSS: &str = include_str!("../style.css");
/// Floats the explanation of a question mark above everything else.
const TOOLTIP_SCRIPT: &str = include_str!("../js/tooltip.js");
/// Keyboard shortcuts.
const KEYS_SCRIPT: &str = include_str!("../js/keys.js");
/// Remembers the scroll position of each page.
const SCROLL_SCRIPT: &str = include_str!("../js/scroll.js");

#[component]
pub fn App() -> Element {
    let state = AppState::init();
    use_context_provider(|| state);

    // Start the catalog server once, when the app opens.
    use_hook(|| {
        spawn_forever(backend::boot(state));
    });
    // Stop it when the window closes, so no process is left behind.
    use_wry_event_handler(|event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            backend::shutdown();
        }
    });

    use_hook(|| {
        spawn(async move {
            let _ = document::eval(TOOLTIP_SCRIPT).await;
            let _ = document::eval(KEYS_SCRIPT).await;
            let _ = document::eval(SCROLL_SCRIPT).await;
        });
    });

    let boot = state.boot.read().clone();
    rsx! {
        style { "{CSS}" }
        style { {include_str!("../page-editor.css")} }
        style { {include_str!("../editor-controls.css")} }
        div { class: "app",
            {match boot {
                // The first run has to download the engine, so it waits on the
                // splash. Later runs open the app at once; only what needs the
                // engine (the search) waits for it.
                BootState::Starting { message, percent } if !state.engine_installed => rsx! { Splash { message, percent } },
                BootState::Failed(message) => rsx! { Failed { message } },
                _ => rsx! { Shell {} },
            }}
            ModalHost {}
            download_dialog::DownloadDialogHost {}
            ToastHost {}
        }
    }
}

#[component]
fn Splash(message: String, percent: Option<u8>) -> Element {
    let fraction = percent.map(|p| p as f32 / 100.0);
    rsx! {
        div { class: "splash",
            cat::Cat {}
            h1 { "MangaPoint" }
            p { class: "muted", "{message}" }
            ProgressBar { fraction }
            p { class: "muted small", style: "margin-top: 16px",
                "The first start downloads the catalog engine (about 340 MB). This only happens once."
            }
        }
    }
}

#[component]
fn Failed(message: String) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "splash",
            h1 { "Something went wrong" }
            div { class: "banner bad", "{message}" }
            button {
                class: "btn primary",
                onclick: move |_| {
                    spawn_forever(backend::boot(state));
                },
                "Try again"
            }
        }
    }
}

#[component]
fn Shell() -> Element {
    let state = use_context::<AppState>();
    let page = *state.page.read();
    // How many batches are downloading or waiting, for the badge on Downloads.
    let (running, waiting, _) = state.queue.read().counts();
    let in_queue = running + waiting;
    rsx! {
        nav { class: "nav",
            div { class: "brand",
                cat::Cat {}
                span { "MangaPoint" }
            }
            NavItem { page: Page::Search, label: "Search", icon: "search" }
            NavItem { page: Page::Library, label: "Library", icon: "book-open" }
            NavItem { page: Page::Export, label: "Export", icon: "file-down" }
            div { class: "nav-spacer" }
            sidebar::SidebarJobs {}
            NavItem {
                page: Page::Tasks, label: "Tasks", icon: "list-checks",
                badge: (in_queue > 0).then(|| in_queue.to_string()),
            }
            NavItem { page: Page::Sources, label: "Sources", icon: "layers" }
            NavItem { page: Page::Settings, label: "Settings", icon: "settings" }
        }
        main { class: "content",
            div { class: "page",
                // The search stays built while another page is open, so coming
                // back to it is instant and it is as it was left.
                div { class: if page == Page::Search { "keep" } else { "keep gone" },
                    search::SearchPage {}
                }
                div { class: if page == Page::Library { "keep" } else { "keep gone" },
                    library_page::LibraryPage {}
                }
                {match page {
                    Page::Setup => rsx! { setup::Setup {} },
                    Page::Search => rsx! {},
                    Page::Series => rsx! { workspace::WorkspacePage {} },
                    Page::Library => rsx! {},
                    Page::Tasks => rsx! { tasks_page::TasksPage {} },
                    Page::Plan => rsx! { plan_page::PlanPage {} },
                    Page::Export => rsx! { export_page::ExportPage {} },
                    Page::Sources => rsx! { sources_page::SourcesPage {} },
                    Page::Settings => rsx! { settings::SettingsPage {} },
                }}
            }
        }
    }
}

#[component]
fn NavItem(page: Page, label: &'static str, icon: &'static str, badge: Option<String>) -> Element {
    let state = use_context::<AppState>();
    let current = *state.page.read();
    // An open series belongs to wherever it was opened from; first-run
    // setup belongs to Sources.
    let opened_from_library = state
        .workspace
        .read()
        .as_ref()
        .is_some_and(|w| w.origin == Page::Library);
    // On the Export screen the series that was open waits to be put back.
    let open_from = if current == Page::Export {
        state.parked_workspace.read().as_ref().map(|w| w.origin)
    } else {
        state.workspace.read().as_ref().map(|w| w.origin)
    };
    let active = current == page
        || (page == Page::Search && current == Page::Series && !opened_from_library)
        || (page == Page::Library && current == Page::Series && opened_from_library)
        || (page == Page::Sources && current == Page::Setup);
    let class = if active {
        "nav-item active"
    } else {
        "nav-item"
    };
    rsx! {
        button { class: "{class}",
            onclick: move |_| {
                // A series left open is where you come back to; pressing the
                // tab it belongs to while looking at it goes to the list.
                match open_from {
                    Some(origin) if origin == page && current == Page::Series => state.close_series(page),
                    Some(origin) if origin == page && current != Page::Series => state.go(Page::Series),
                    _ => state.go(page),
                }
            },
            Icon { name: icon, size: 17 }
            span { class: "grow", "{label}" }
            if let Some(count) = badge { span { class: "nav-badge", "{count}" } }
        }
    }
}

/// Confirmation dialogs.
#[component]
fn ModalHost() -> Element {
    let state = use_context::<AppState>();
    let modal = state.modal.read().clone();
    let Some(modal) = modal else { return rsx! {} };
    let confirm_class = if modal.danger {
        "btn danger"
    } else {
        "btn primary"
    };
    let action = modal.action.clone();
    rsx! {
        div { class: "overlay",
            div { class: "modal",
                h2 { "{modal.title}" }
                p { "{modal.body}" }
                div { class: "actions",
                    button {
                        class: "btn",
                        onclick: move |_| {
                            let mut m = state.modal;
                            m.set(None);
                        },
                        "{modal.cancel}"
                    }
                    button {
                        class: "{confirm_class}",
                        onclick: move |_| {
                            let mut m = state.modal;
                            m.set(None);
                            run_action(state, action.clone());
                        },
                        "{modal.confirm}"
                    }
                }
            }
        }
    }
}

fn run_action(state: AppState, action: ModalAction) {
    spawn_forever(async move {
        match action {
            ModalAction::EnableBrowser { then_install } => {
                backend::enable_browser(state).await;
                if let Some(id) = then_install {
                    backend::install_default(state, &id).await;
                }
            }
            ModalAction::DisableBrowser => backend::disable_browser(state).await,
            ModalAction::RemoveSeries { series_ids } => {
                work::remove_from_library(state, series_ids)
            }
            ModalAction::DeleteChapters { series_id, keys } => {
                work::delete_chapters(state, series_id, keys)
            }
            ModalAction::DeleteDownloads { series_ids } => {
                work::delete_downloads(state, series_ids)
            }
            ModalAction::DeletePreset { id } => presets::delete_preset(state, id),
            ModalAction::DeleteExport { record_id } => work::delete_export(state, record_id),
            ModalAction::DeleteExportedFile { record_id, path } => {
                work::delete_exported_file(state, record_id, path)
            }
            ModalAction::RemoveExtension(pkg) => {
                let Some(client) = state.client() else { return };
                tracing::info!("Removing source {pkg}");
                let mut sources_state = state.sources.read().clone();
                let result = SourceManager::new(&*client)
                    .remove(&mut sources_state, &pkg)
                    .await;
                let mut s = state.sources;
                s.set(sources_state);
                state.save_sources();
                match result {
                    Ok(()) => state.notify("Source removed."),
                    Err(e) => state.error(format!("Could not remove it: {e}")),
                }
            }
        }
    });
}

/// Ask before turning on browser support. Used wherever a source needs it.
pub fn ask_enable_browser(state: AppState, then_install: Option<String>) {
    state.ask(Modal {
        title: "Turn on browser support?".into(),
        body: catalog::browser::ENABLE_PROMPT.into(),
        confirm: "Turn on".into(),
        cancel: "Not now".into(),
        danger: false,
        action: ModalAction::EnableBrowser { then_install },
    });
}

#[component]
fn ToastHost() -> Element {
    let state = use_context::<AppState>();
    let notice = state.notice.read().clone();
    let Some(notice) = notice else { return rsx! {} };
    let class = if notice.error { "toast error" } else { "toast" };
    rsx! {
        div { class: "{class}",
            div { class: "grow", "{notice.text}" }
            button {
                class: "btn small",
                onclick: move |_| {
                    let mut n = state.notice;
                    n.set(None);
                },
                "OK"
            }
        }
    }
}

#[cfg(test)]
mod script_tests {
    const SCRIPTS: [(&str, &str); 6] = [
        ("crop-drag.js", include_str!("../js/crop-drag.js")),
        ("export-split.js", include_str!("../js/export-split.js")),
        ("preview-editor.js", include_str!("../js/preview-editor.js")),
        ("tooltip.js", include_str!("../js/tooltip.js")),
        ("keys.js", include_str!("../js/keys.js")),
        ("scroll.js", include_str!("../js/scroll.js")),
    ];

    /// `cropBound` as it is written in HTML: `data-crop-bound`.
    fn attribute(camel: &str) -> String {
        let mut out = String::from("data-");
        for c in camel.chars() {
            if c.is_ascii_uppercase() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    /// A script that marks an element with `dataset.x = ...` and also looks
    /// elements up with `[data-x]` will find the marked element too. That once
    /// made a whole screen swallow every click, so it must not come back.
    #[test]
    fn a_script_never_marks_what_it_also_selects() {
        for (name, source) in SCRIPTS {
            for line in source.lines() {
                let Some(at) = line.find(".dataset.") else {
                    continue;
                };
                let rest = &line[at + ".dataset.".len()..];
                let field: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !rest[field.len()..].trim_start().starts_with("= ")
                    && !rest[field.len()..].trim_start().starts_with('=')
                {
                    continue;
                }
                let selector = format!("[{}", attribute(&field));
                assert!(
                    !source.contains(&selector),
                    "{name} sets dataset.{field} and also selects {selector}"
                );
            }
        }
    }

    #[test]
    fn the_scripts_are_not_empty() {
        for (name, source) in SCRIPTS {
            assert!(source.trim().len() > 100, "{name}");
        }
    }
}
