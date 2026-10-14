//! First run: the engine, then sources, then a reader, each of which can be
//! skipped once it is reached. Where it got to is kept, so closing the app
//! part way through picks up again where it stopped.

use super::devices::DeviceAdder;
use super::setup::Setup;
use super::walker::WalkingCat;
use super::widgets::{Icon, ProgressBar};
use crate::backend;
use crate::state::{spawn_forever, AppState, BootState};
use dioxus::prelude::*;
use library::SetupStep;

/// Which step is on show when it is not the one the setup has reached: the
/// tabs let you go back to look at or change an earlier one.
type View = Signal<Option<SetupStep>>;

/// Move first-run setup on to `step` and remember it. It never goes back, and
/// the screen follows it.
pub fn set_step(state: AppState, step: SetupStep) {
    let reached = state.settings.peek().setup_step;
    if step == SetupStep::Done || position(step) > position(reached) {
        let mut settings = state.settings;
        settings.write().setup_step = step;
        state.save_settings();
    }
    if let Some(mut view) = try_consume_context::<View>() {
        view.set(None);
    }
}

const STEPS: [(SetupStep, &str); 3] = [
    (SetupStep::Engine, "Engine"),
    (SetupStep::Sources, "Sources"),
    (SetupStep::Reader, "Reader"),
];

fn position(step: SetupStep) -> usize {
    STEPS.iter().position(|(s, _)| *s == step).unwrap_or(STEPS.len())
}

#[component]
pub fn Onboarding() -> Element {
    let state = use_context::<AppState>();
    let reached = state.settings.read().setup_step;
    let view: View = use_context_provider(|| Signal::new(None));
    let shown = view().filter(|s| position(*s) <= position(reached)).unwrap_or(reached);

    // The engine step is over as soon as the engine is up.
    use_effect(move || {
        let ready = matches!(*state.boot.read(), BootState::Ready);
        if ready && state.settings.peek().setup_step == SetupStep::Engine {
            set_step(state, SetupStep::Sources);
        }
    });

    rsx! {
        div { class: "onboard",
            div { class: "onboard-tabs", role: "tablist",
                for (i, (step, label)) in STEPS.iter().copied().enumerate() {
                    {
                        let locked = i > position(reached);
                        let class = if step == shown { "onboard-tab on" } else if locked { "onboard-tab locked" } else { "onboard-tab" };
                        rsx! {
                            button {
                                key: "{label}", class, role: "tab", disabled: locked,
                                "aria-selected": "{step == shown}",
                                onclick: {
                                    let mut view = view;
                                    move |_| view.set(Some(step))
                                },
                                span { class: "num",
                                    if i < position(reached) { Icon { name: "check", size: 12 } } else { "{i + 1}" }
                                }
                                "{label}"
                            }
                        }
                    }
                }
            }
            div { class: "onboard-body", key: "{position(shown)}",
                {match shown {
                    SetupStep::Engine => rsx! { EngineStep {} },
                    SetupStep::Sources => rsx! { Setup { onboarding: true } },
                    _ => rsx! { ReaderStep {} },
                }}
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
        div { class: "engine-step rise-in",
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
        div { class: "rise-in",
            h1 { "Choose your reader" }
            p { class: "muted",
                "Pages are made at the size of your reader\u{2019}s screen. Search for it, or type a name to add one of your own. "
                "You can add, change or remove readers any time in Settings."
            }
            div { style: "margin: 18px 0 8px; min-height: 330px",
                DeviceAdder { always_open: !added, on_added: move |_| {} }
            }
            if added {
                div { class: "px-label", style: "margin-top: 16px", "Your readers" }
                div { class: "pills",
                    for d in devices {
                        span { class: "pill on", key: "{d.id}", "{d.name}" }
                    }
                }
            }
            div { class: "onboard-foot",
                div { class: "grow" }
                button {
                    class: if added { "btn primary" } else { "btn" },
                    onclick: move |_| set_step(state, SetupStep::Done),
                    if added {
                        Icon { name: "check", size: 15 }
                        "Finish"
                    } else {
                        "Skip for now"
                    }
                }
            }
        }
    }
}
