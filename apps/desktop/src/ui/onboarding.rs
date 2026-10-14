//! First run: the engine, then sources, then a reader, each of which can be
//! skipped once it is reached. Where it got to is kept, so closing the app
//! part way through picks up again where it stopped.

use super::devices::DeviceAdder;
use super::setup::Setup;
use super::widgets::Icon;
use super::Splash;
use crate::state::{AppState, BootState};
use dioxus::prelude::*;
use library::SetupStep;

/// Move first-run setup on to `step` and remember it.
pub fn set_step(state: AppState, step: SetupStep) {
    let mut settings = state.settings;
    settings.write().setup_step = step;
    state.save_settings();
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
    let step = state.settings.read().setup_step;
    let boot = state.boot.read().clone();

    // The engine step is over as soon as the engine is up.
    use_effect(move || {
        let ready = matches!(*state.boot.read(), BootState::Ready);
        if ready && state.settings.peek().setup_step == SetupStep::Engine {
            set_step(state, SetupStep::Sources);
        }
    });

    rsx! {
        div { class: "onboard",
            div { class: "onboard-steps",
                for (i, (_, label)) in STEPS.iter().enumerate() {
                    {
                        let class = if i == position(step) { "onboard-step on" } else if i < position(step) { "onboard-step done" } else { "onboard-step" };
                        rsx! {
                            if i > 0 { span { class: "onboard-sep" } }
                            div { class,
                                span { class: "num",
                                    if i < position(step) { Icon { name: "check", size: 12 } } else { "{i + 1}" }
                                }
                                "{label}"
                            }
                        }
                    }
                }
            }
            div { class: "onboard-body",
                {match step {
                    SetupStep::Engine => match boot {
                        BootState::Starting { message, percent } => rsx! { Splash { message, percent } },
                        _ => rsx! { Splash { message: "Ready".to_string(), percent: Some(100u8) } },
                    },
                    SetupStep::Sources => rsx! { Setup { onboarding: true } },
                    _ => rsx! { ReaderStep {} },
                }}
            }
        }
    }
}

#[component]
fn ReaderStep() -> Element {
    let state = use_context::<AppState>();
    let devices = state.settings.read().devices();
    let added = !devices.is_empty();
    rsx! {
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
        div { class: "spacer" }
        div { class: "row",
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
