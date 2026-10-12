//! The app's screens.

pub mod cat;
mod icons;
mod jobs;
mod search;
mod series;
mod settings;
mod setup;
mod sources_page;
mod widgets;

use crate::backend;
use crate::state::spawn_forever;
use crate::state::{AppState, BootState, Modal, ModalAction, Page};
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::use_wry_event_handler;
use dioxus::prelude::*;
use sources::SourceManager;
use widgets::{Icon, ProgressBar};

const CSS: &str = include_str!("../style.css");

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

    let boot = state.boot.read().clone();
    rsx! {
        style { "{CSS}" }
        div { class: "app",
            {match boot {
                BootState::Starting { message, percent } => rsx! { Splash { message, percent } },
                BootState::Failed(message) => rsx! { Failed { message } },
                BootState::Ready => rsx! { Shell {} },
            }}
            ModalHost {}
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
    rsx! {
        nav { class: "nav",
            div { class: "brand",
                cat::Cat {}
                span { "MangaPoint" }
            }
            NavItem { page: Page::Search, label: "Search", icon: "search" }
            NavItem { page: Page::Jobs, label: "Exports", icon: "download" }
            NavItem { page: Page::Sources, label: "Sources", icon: "layers" }
            NavItem { page: Page::Settings, label: "Settings", icon: "settings" }
        }
        main { class: "content",
            div { class: "page",
                JobBar {}
                {match page {
                    Page::Setup => rsx! { setup::Setup {} },
                    Page::Search => rsx! { search::SearchPage {} },
                    Page::Series => rsx! { series::SeriesPage {} },
                    Page::Sources => rsx! { sources_page::SourcesPage {} },
                    Page::Jobs => rsx! { jobs::JobsPage {} },
                    Page::Settings => rsx! { settings::SettingsPage {} },
                }}
            }
        }
    }
}

#[component]
fn NavItem(page: Page, label: &'static str, icon: &'static str) -> Element {
    let state = use_context::<AppState>();
    let current = *state.page.read();
    // The series page belongs to Search; first-run setup belongs to Sources.
    let active = current == page
        || (page == Page::Search && current == Page::Series)
        || (page == Page::Sources && current == Page::Setup);
    let class = if active {
        "nav-item active"
    } else {
        "nav-item"
    };
    rsx! {
        button { class: "{class}", onclick: move |_| state.go(page),
            Icon { name: icon, size: 17 }
            "{label}"
        }
    }
}

/// The running (or just finished) export, shown above every screen.
#[component]
fn JobBar() -> Element {
    let state = use_context::<AppState>();
    let job = state.job.read().clone();
    let Some(job) = job else { return rsx! {} };
    let running = job.result.is_none();
    rsx! {
        div { class: "jobbar",
            div { class: "row",
                div { class: "grow",
                    strong { "{job.title}" }
                    {match &job.result {
                        None => rsx! { span { class: "muted", " · {job.progress.label()}" } },
                        Some(Ok(files)) => rsx! {
                            span { class: "chip ok", style: "margin-left: 8px", "Done" }
                            span { class: "muted small", " {files.len()} file(s) written" }
                        },
                        Some(Err(e)) => rsx! { span { class: "chip bad", style: "margin-left: 8px", "Failed" } span { class: "muted small", " {e}" } },
                    }}
                }
                if running {
                    button {
                        class: "btn small",
                        onclick: move |_| job.cancel.cancel(),
                        Icon { name: "x", size: 14 }
                        "Cancel"
                    }
                } else {
                    button {
                        class: "btn small",
                        onclick: move |_| state.go(Page::Jobs),
                        Icon { name: "info", size: 14 }
                        "Details"
                    }
                    button {
                        class: "btn small",
                        onclick: move |_| {
                            let mut j = state.job;
                            j.set(None);
                        },
                        "Dismiss"
                    }
                }
            }
            if running {
                div { style: "margin-top: 8px",
                    ProgressBar { fraction: Some(job.progress.fraction()) }
                }
                if let Some(note) = &job.progress.note {
                    p { class: "muted small", style: "margin: 6px 0 0", "{note}" }
                }
            }
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
