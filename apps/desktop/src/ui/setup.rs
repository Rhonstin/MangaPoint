//! First run: choose which sources to install.

use super::ask_enable_browser;
use super::scenes::SearchCat;
use super::widgets::{Chip, Icon};
use crate::backend;
use crate::state::spawn_forever;
use crate::state::{AppState, Page};
use dioxus::prelude::*;
use library::Choice;
use sources::{Requires, SourceManager};

/// The sources step is done: whatever was not answered counts as skipped.
pub fn finish_sources(state: AppState) {
    let mut sources_state = state.sources.read().clone();
    if let Some(client) = state.client() {
        SourceManager::new(&*client).finish_onboarding(&mut sources_state);
    }
    let installed = sources_state
        .choices
        .values()
        .filter(|c| **c == Choice::Install)
        .count();
    let mut s = state.sources;
    s.set(sources_state);
    state.save_sources();
    tracing::info!("First-run setup finished: {installed} source(s) installed");
}

#[component]
pub fn Setup(#[props(default = false)] onboarding: bool) -> Element {
    let state = use_context::<AppState>();

    let offers = use_resource(move || async move {
        let _ = state.sources_version.read();
        let client = state.client()?;
        let snapshot = state.sources.read().clone();
        SourceManager::new(&*client).offers(&snapshot).await.ok()
    });

    let install_recommended = move |_| {
        let mut installing = state.installing;
        installing.set(true);
        spawn_forever(async move {
            for d in sources::DEFAULTS
                .iter()
                .filter(|d| d.requires == Requires::Nothing)
            {
                let answered = state.sources.read().choices.contains_key(d.id);
                if !answered {
                    backend::install_default(state, d.id).await;
                }
            }
            installing.set(false);
            state.bump_sources();
        });
    };
    let busy = *state.installing.read();

    let finish = move |_| {
        finish_sources(state);
        state.go(Page::Search);
    };

    let rows = match &*offers.read() {
        Some(Some(list)) => list
            .iter()
            .map(|o| {
                let id = o.default.id;
                let needs_browser = o.default.requires == Requires::Browser;
                let installed = o.installed;
                let skipped = o.choice == Some(Choice::Skip) && !installed;
                rsx! {
                    div { class: "setup-row", key: "{id}",
                        div { class: "src-main",
                            div { class: "row wrap",
                                strong { class: "src-name", "{o.default.name}" }
                                if needs_browser { Chip { text: "needs browser support", kind: Some("warn".to_string()) } }
                                if installed { Chip { text: "installed", kind: Some("ok".to_string()) } }
                                if skipped { Chip { text: "skipped" } }
                                if !o.available { Chip { text: "not offered right now", kind: Some("bad".to_string()) } }
                            }
                            div { class: "muted small", "{o.default.description}" }
                        }
                        if o.available && !installed {
                            div { class: "src-actions",
                                button {
                                    class: "btn primary small",
                                    onclick: move |_| {
                                        let browser_off = !state.settings.read().browser_support;
                                        if needs_browser && browser_off {
                                            ask_enable_browser(state, Some(id.to_string()));
                                        } else {
                                            spawn_forever(async move {
                                                backend::install_default(state, id).await;
                                                state.bump_sources();
                                            });
                                        }
                                    },
                                    Icon { name: "download", size: 15 }
                                    "Install"
                                }
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let loaded = matches!(&*offers.read(), Some(Some(_)));
    let installed_any = state
        .sources
        .read()
        .choices
        .values()
        .any(|c| *c == Choice::Install);

    rsx! {
        div { class: "onboard-head",
            if onboarding {
                div { class: "step-hero", SearchCat {} }
            }
            h1 { "Choose your sources" }
            p { class: "muted",
                "Sources are the websites MangaPoint searches. Pick the ones you want; you can change this any time in Sources. "
                "These all passed our full test (search, chapters and page images) when we last checked."
            }
        }
        div { class: "setup-tools",
            if !installed_any {
                span { class: "muted small", "Install at least one source to start searching." }
            }
            div { class: "grow" }
            button {
                class: "btn small",
                disabled: busy,
                onclick: install_recommended,
                if busy { "Installing\u{2026}" } else { "Install all recommended" }
            }
            if !onboarding {
                button {
                    class: "btn primary small",
                    disabled: !installed_any,
                    onclick: finish,
                    Icon { name: "check", size: 15 }
                    "Done"
                }
            }
        }
        if !loaded {
            p { class: "muted", "Loading the list\u{2026}" }
        }
        div { class: "setup-list", {rows.into_iter()} }
    }
}
