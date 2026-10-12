//! The Presets page: pick a preset, change its settings, and see the result on
//! a test page before saving.

use super::conversion::ConversionControls;
use super::widgets::{Chip, Icon};
use super::work::{choose_lab_sample, schedule_lab, select_lab_preset};
use crate::logic::{describe_settings, plural};
use crate::state::{AppState, Modal, ModalAction};
use dioxus::prelude::*;

/// What the name box on the page is for right now.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Naming {
    New,
    Rename,
}

#[component]
pub fn PresetsLab() -> Element {
    let state = use_context::<AppState>();
    let mut naming = use_signal(|| None::<Naming>);
    let mut name = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    // Keep a valid preset selected: the starting point on first show, and
    // something else if the selected one was deleted.
    use_effect(move || {
        let entries = state.presets.read().all();
        let selected = state.preset_lab.peek().selected.clone();
        let valid = selected
            .as_ref()
            .is_some_and(|id| entries.iter().any(|e| &e.preset.id == id));
        if !valid {
            let start = state
                .settings
                .peek()
                .default_preset_id
                .clone()
                .filter(|id| entries.iter().any(|e| &e.preset.id == id))
                .or_else(|| entries.first().map(|e| e.preset.id.clone()));
            if let Some(id) = start {
                select_lab_preset(state, &id);
            }
        }
    });

    let entries = state.presets.read().all();
    let lab = state.preset_lab.read().clone();
    let selected = lab
        .selected
        .as_ref()
        .and_then(|id| entries.iter().find(|e| &e.preset.id == id))
        .cloned();
    let Some(selected) = selected else {
        return rsx! { p { class: "muted", "Loading presets…" } };
    };
    let changed = selected.preset.settings != lab.draft;
    let defaults = state.settings.read().clone();
    let is_start = defaults.conversion == selected.preset.settings
        && defaults.default_preset_id.as_deref() == Some(selected.preset.id.as_str());

    // The series that can supply a test page.
    let samples: Vec<(String, String)> = state
        .library
        .read()
        .series
        .iter()
        .filter(|s| !s.chapters.is_empty())
        .map(|s| (s.id.clone(), s.title.clone()))
        .collect();
    let sample_value = lab.sample_series.clone().unwrap_or_default();

    let id = selected.preset.id.clone();
    let (save_id, undo_id, rename_id, delete_id, start_id) =
        (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
    let draft_for_save = lab.draft.clone();
    let draft_for_new = lab.draft.clone();
    let preset_settings = selected.preset.settings.clone();
    let builtin = selected.builtin;
    let modified = selected.modified;
    let preset_name = selected.preset.name.clone();
    let delete_name = preset_name.clone();
    let rename_from = preset_name.clone();

    let page_label = if lab.total_pages > 0 {
        format!("Page {} of {}", lab.page + 1, lab.total_pages)
    } else {
        "Built-in sample page".to_string()
    };

    rsx! {
        // ---- the presets, as a row of pills
        div { class: "pills",
            for e in entries.iter() {
                {
                    let pick = e.preset.id.clone();
                    let on = e.preset.id == id;
                    rsx! {
                        button {
                            key: "{e.preset.id}",
                            class: if on { "pill on" } else { "pill" },
                            onclick: move |_| { error.set(None); naming.set(None); select_lab_preset(state, &pick) },
                            "{e.preset.name}"
                            if e.builtin && e.modified { span { class: "dot", title: "Changed by you" } }
                        }
                    }
                }
            }
            button { class: "pill add", title: "Save the settings below as a new preset",
                onclick: move |_| { name.set(String::new()); error.set(None); naming.set(Some(Naming::New)); },
                Icon { name: "plus", size: 14 } "New"
            }
        }

        // ---- the selected preset
        div { class: "card",
            div { class: "row wrap",
                strong { style: "font-size: 1.05rem", "{preset_name}" }
                if builtin { Chip { text: "built in" } }
                if modified { Chip { text: "changed by you", kind: Some("warn".to_string()) } }
                if is_start { Chip { text: "starting point", kind: Some("ok".to_string()) } }
                if changed { Chip { text: "unsaved changes", kind: Some("accent".to_string()) } }
            }
            if !selected.preset.description.is_empty() {
                p { class: "muted small", style: "margin: 6px 0 0", "{selected.preset.description}" }
            }
            p { class: "muted small", style: "margin: 4px 0 0", "{describe_settings(&lab.draft)}" }

            if let Some(mode) = naming() {
                div { class: "row", style: "margin-top: 12px",
                    input {
                        r#type: "text", class: "grow", maxlength: "40", autofocus: true, value: "{name}",
                        placeholder: if mode == Naming::New { "Name the new preset" } else { "New name" },
                        oninput: move |e| name.set(e.value()),
                    }
                    button { class: "btn primary small",
                        onclick: move |_| {
                            let result = match mode {
                                Naming::New => state.edit_presets(|p| p.save_new(&name.read(), draft_for_new.clone())),
                                Naming::Rename => {
                                    state.edit_presets(|p| p.rename(&rename_id, &name.read()).map(|()| rename_id.clone()))
                                }
                            };
                            match result {
                                Ok(new_id) => {
                                    state.save_presets();
                                    naming.set(None);
                                    error.set(None);
                                    if mode == Naming::New { select_lab_preset(state, &new_id); }
                                }
                                Err(e) => error.set(Some(e.to_string())),
                            }
                        },
                        if mode == Naming::New { "Save as new" } else { "Rename" }
                    }
                    button { class: "btn ghost small", onclick: move |_| { naming.set(None); error.set(None); }, "Cancel" }
                }
            } else {
                div { class: "row wrap", style: "margin-top: 12px",
                    button { class: "btn primary small", disabled: !changed,
                        onclick: move |_| {
                            let result = state.edit_presets(|p| p.overwrite(&save_id, draft_for_save.clone()));
                            match result {
                                Ok(()) => { state.save_presets(); state.notify("Preset saved."); }
                                Err(e) => state.error(e.to_string()),
                            }
                        },
                        Icon { name: "check", size: 14 } "Save changes"
                    }
                    button { class: "btn small", disabled: !changed,
                        onclick: move |_| select_lab_preset(state, &undo_id),
                        Icon { name: "rotate-ccw", size: 14 } "Undo changes"
                    }
                    button { class: "btn small",
                        onclick: move |_| { name.set(String::new()); error.set(None); naming.set(Some(Naming::New)); },
                        Icon { name: "plus", size: 14 } "Save as new…"
                    }
                    if !builtin {
                        button { class: "btn small", title: "Rename",
                            onclick: move |_| { name.set(rename_from.clone()); error.set(None); naming.set(Some(Naming::Rename)); },
                            Icon { name: "pencil", size: 14 } "Rename"
                        }
                    }
                    span { class: "grow" }
                    if !is_start {
                        button { class: "btn small", disabled: changed,
                            title: if changed { "Save your changes first" } else { "New series will start with these settings" },
                            onclick: move |_| {
                                let mut settings = state.settings;
                                {
                                    let mut s = settings.write();
                                    s.conversion = preset_settings.clone();
                                    s.default_preset_id = Some(start_id.clone());
                                }
                                state.save_settings();
                            },
                            "Use for new series"
                        }
                    }
                    if !builtin || modified {
                        button { class: "btn small danger",
                            onclick: move |_| state.ask(Modal {
                                title: if builtin { format!("Reset {delete_name}?") } else { format!("Delete {delete_name}?") },
                                body: if builtin {
                                    "Your changes to this preset are discarded and it goes back to how it came. Series that use it keep their current settings.".to_string()
                                } else {
                                    "The preset is deleted. Series that used it keep their current settings.".to_string()
                                },
                                confirm: if builtin { "Reset".into() } else { "Delete".into() },
                                cancel: "Keep".into(),
                                danger: true,
                                action: ModalAction::DeletePreset { id: delete_id.clone() },
                            }),
                            if builtin { Icon { name: "rotate-ccw", size: 14 } "Reset" } else { Icon { name: "trash", size: 14 } "Delete" }
                        }
                    }
                }
            }
            if let Some(message) = error() {
                p { class: "small", style: "color: var(--bad); margin: 8px 0 0", "{message}" }
            }
        }

        // ---- the settings, and the test page
        div { class: "lab",
            div { class: "card lab-controls",
                h2 { Icon { name: "sliders-horizontal", size: 17 } "Settings" }
                ConversionControls {
                    value: lab.draft.clone(),
                    compact: true,
                    onchange: move |v| {
                        let mut l = state.preset_lab;
                        l.write().draft = v;
                        schedule_lab(state, 250);
                    },
                }
            }
            div { class: "card lab-preview",
                div { class: "row wrap",
                    h2 { class: "grow", Icon { name: "eye", size: 17 } "Test page" }
                    if lab.busy { span { class: "spin muted", Icon { name: "loader-circle", size: 16 } } }
                }
                div { class: "row wrap", style: "margin-bottom: 10px",
                    select {
                        value: "{sample_value}",
                        onchange: move |e| {
                            let v = e.value();
                            choose_lab_sample(state, if v.is_empty() { None } else { Some(v) });
                        },
                        option { value: "", selected: sample_value.is_empty(), "Built-in sample page" }
                        for (sid, title) in samples.iter() {
                            option { key: "{sid}", value: "{sid}", selected: *sid == sample_value, "{title}" }
                        }
                    }
                    if lab.total_pages > 0 {
                        button { class: "btn small", disabled: lab.page == 0,
                            onclick: move |_| { let mut l = state.preset_lab; let p = l.peek().page; l.write().page = p.saturating_sub(1); schedule_lab(state, 0); },
                            Icon { name: "arrow-left", size: 14 }
                        }
                        span { class: "muted small", "{page_label}" }
                        button { class: "btn small", disabled: lab.page + 1 >= lab.total_pages,
                            onclick: move |_| { let mut l = state.preset_lab; let p = l.peek().page; l.write().page = p + 1; schedule_lab(state, 0); },
                            span { style: "display: inline-flex; transform: rotate(180deg)", Icon { name: "arrow-left", size: 14 } }
                        }
                    } else {
                        span { class: "muted small", "{page_label}" }
                    }
                }
                if let Some(message) = &lab.error {
                    div { class: "banner bad small", Icon { name: "circle-x", size: 16 } "{message}" }
                }
                div { class: "lab-shots",
                    if let Some(original) = &lab.original {
                        figure { class: "lab-shot",
                            figcaption { "Original" }
                            div { class: "lab-box",
                                img { src: "{original.url}", alt: "The test page as it is" }
                                if let Some([x, y, w, h]) = lab.kept {
                                    if w < 0.999 || h < 0.999 {
                                        div { class: "kept", style: "left: {x * 100.0:.3}%; top: {y * 100.0:.3}%; width: {w * 100.0:.3}%; height: {h * 100.0:.3}%;" }
                                    }
                                }
                            }
                        }
                    }
                    if !lab.converted.is_empty() {
                        figure { class: "lab-shot",
                            figcaption {
                                if lab.converted.len() > 1 { "On your reader · {plural(lab.converted.len(), \"screen\")}" } else { "On your reader" }
                            }
                            div { class: "lab-box",
                                for (i, shot) in lab.converted.iter().enumerate() {
                                    img { key: "{i}", src: "{shot.url}", alt: "The test page on the reader" }
                                }
                            }
                        }
                    }
                }
                p { class: "muted small", style: "margin: 10px 0 0",
                    "This uses the same conversion as an export. The dimmed area on the original is what trimming and the margins cut away."
                }
            }
        }
    }
}
