//! First run: the engine, then sources, then a reader, each of which can be
//! skipped once it is reached. Where it got to is kept, so closing the app
//! part way through picks up again where it stopped.

use super::devices::DeviceAdder;
use super::setup::Setup;
use super::shelf::ShelfCat;
use super::walker::WalkingCat;
use super::widgets::{Icon, ProgressBar};
use crate::backend;
use crate::state::{spawn_forever, AppState, BootState};
use dioxus::prelude::*;
use library::SetupStep;

/// Which step is on show when it is not the one the setup has reached: Back
/// goes to an earlier one, to look at or change it.
type View = Signal<Option<SetupStep>>;

/// Move first-run setup on to `step` and remember it. It never goes back.
fn set_step(state: AppState, step: SetupStep) {
    let reached = state.settings.peek().setup_step;
    if step == SetupStep::Done || position(step) > position(reached) {
        let mut settings = state.settings;
        settings.write().setup_step = step;
        state.save_settings();
    }
}

const STEPS: [SetupStep; 3] = [SetupStep::Engine, SetupStep::Sources, SetupStep::Reader];

fn position(step: SetupStep) -> usize {
    STEPS.iter().position(|s| *s == step).unwrap_or(STEPS.len())
}

#[component]
pub fn Onboarding() -> Element {
    let state = use_context::<AppState>();
    let reached = state.settings.read().setup_step;
    let mut view: View = use_signal(|| None);
    // Which way the page came in from, so it slides in from that side.
    let mut forward = use_signal(|| true);
    let shown = view().filter(|s| position(*s) <= position(reached)).unwrap_or(reached);
    let pos = position(shown);

    let engine_ready = matches!(*state.boot.read(), BootState::Ready);
    let source_installed = state
        .sources
        .read()
        .choices
        .values()
        .any(|c| *c == library::Choice::Install);
    let reader_added = !state.settings.read().devices().is_empty();

    let next_label = match shown {
        SetupStep::Engine => "Next",
        SetupStep::Sources if source_installed => "Next",
        SetupStep::Sources => "Skip",
        _ if reader_added => "Finish",
        _ => "Skip",
    };
    let next_disabled = shown == SetupStep::Engine && !engine_ready;

    let go_next = move |_| {
        forward.set(true);
        match shown {
            SetupStep::Engine => set_step(state, SetupStep::Sources),
            SetupStep::Sources => {
                if source_installed {
                    super::setup::finish_sources(state);
                }
                set_step(state, SetupStep::Reader);
            }
            _ => set_step(state, SetupStep::Done),
        }
        view.set(None);
    };
    let go_back = move |_| {
        if pos > 0 {
            forward.set(false);
            view.set(Some(STEPS[pos - 1]));
        }
    };

    rsx! {
        div { class: "onboard",
            div {
                class: if forward() { "onboard-body from-right" } else { "onboard-body from-left" },
                key: "{pos}",
                {match shown {
                    SetupStep::Engine => rsx! { EngineStep {} },
                    SetupStep::Sources => rsx! { Setup { onboarding: true } },
                    _ => rsx! { ReaderStep {} },
                }}
            }
            div { class: "onboard-nav",
                if pos > 0 {
                    button { class: "btn ghost", onclick: go_back,
                        Icon { name: "arrow-left", size: 15 }
                        "Back"
                    }
                }
                div { class: "grow" }
                button { class: "btn primary", disabled: next_disabled, onclick: go_next,
                    "{next_label}"
                    if next_label == "Finish" { Icon { name: "check", size: 15 } } else { Icon { name: "chevron-right", size: 15 } }
                }
            }
        }
    }
}

/// The engine: what it is and why, a button to install it, and the cat walking
/// while it downloads.
#[component]
fn EngineStep() -> Element {
    let state = use_context::<AppState>();
    let boot = state.boot.read().clone();
    let folder = crate::state::paths().suwayomi_dir().display().to_string();
    rsx! {
        div { class: "engine-step",
            WalkingCat {}
            {match boot {
                BootState::NeedsEngine => rsx! {
                    h1 { "Install the catalog engine" }
                    p { class: "muted",
                        "MangaPoint finds manga and downloads chapters through a helper program, the catalog engine (Suwayomi). "
                        "It is necessary: without it, searching and downloading cannot work."
                    }
                    ul { class: "engine-facts",
                        li { strong { "About 340 MB" } span { class: "muted", "downloaded once, from the Suwayomi project on GitHub" } }
                        li { strong { "Kept in MangaPoint\u{2019}s folder" } span { class: "muted", title: "{folder}", "nothing is installed on the rest of your computer" } }
                        li { strong { "Runs only while the app is open" } span { class: "muted", "and only answers to this computer" } }
                    }
                    button { class: "btn primary big",
                        onclick: move |_| spawn_forever(backend::boot(state, true)),
                        Icon { name: "download", size: 16 }
                        "Install the engine"
                    }
                },
                BootState::Starting { message, percent } => rsx! {
                    h1 { "Getting the engine ready" }
                    p { class: "muted", "{message}" }
                    div { class: "engine-bar", ProgressBar { fraction: percent.map(|p| f32::from(p) / 100.0) } }
                    p { class: "muted small", "This only happens once. You can leave the window open and come back." }
                },
                _ => rsx! {
                    h1 { "The engine is ready" }
                    p { class: "muted", "Nothing more to install." }
                },
            }}
        }
    }
}

#[component]
fn ReaderStep() -> Element {
    let state = use_context::<AppState>();
    let devices = state.settings.read().devices();
    let added = !devices.is_empty();
    rsx! {
        div { class: "onboard-head",
            div { class: "step-hero", ShelfCat {} }
            h1 { "Choose your reader" }
            p { class: "muted",
                "Pages are made at the size of your reader\u{2019}s screen. Search for it, or type a name to add one of your own. "
                "You can add, change or remove readers any time in Settings."
            }
        }
        div { class: "reader-pick",
            DeviceAdder { always_open: !added, on_added: move |_| {} }
            if added {
                div { class: "px-label", style: "margin-top: 16px", "Your readers" }
                div { class: "pills",
                    for d in devices {
                        span { class: "pill on", key: "{d.id}", "{d.name}" }
                    }
                }
            }
        }
    }
}
