//! Conversion presets: choosing, saving, updating and managing them.

use super::widgets::{Chip, Icon};
use super::work::schedule_preview;
use crate::logic::describe_settings;
use crate::state::{AppState, Modal, ModalAction, Page};
use dioxus::prelude::*;
use library::{ConversionSettings, Deleted};

/// Give a series a preset's settings and redraw the preview.
pub fn apply_to_series(
    state: AppState,
    series_id: &str,
    preset_id: Option<String>,
    settings: ConversionSettings,
) {
    let mut library = state.library;
    {
        let mut db = library.write();
        db.set_settings(series_id, settings);
        db.set_preset(series_id, preset_id);
    }
    state.save_library();
    schedule_preview(state, 0);
}

/// Delete one of the user's presets, or put a changed built-in back. Series
/// and defaults that pointed at a deleted preset keep their settings but lose
/// the link.
pub fn delete_preset(state: AppState, id: String) {
    let result = state.edit_presets(|p| p.delete(&id));
    match result {
        Ok(Deleted::Removed) => {
            let mut library = state.library;
            {
                let mut db = library.write();
                for s in db
                    .series
                    .iter_mut()
                    .filter(|s| s.preset_id.as_deref() == Some(&id))
                {
                    s.preset_id = None;
                }
            }
            state.save_library();
            let mut settings = state.settings;
            if settings.peek().default_preset_id.as_deref() == Some(&id) {
                settings.write().default_preset_id = None;
                state.save_settings();
            }
            state.save_presets();
            state.notify("Preset deleted.");
        }
        Ok(Deleted::Reset) => {
            state.save_presets();
            state.notify("Back to the original settings.");
        }
        Err(e) => state.error(e.to_string()),
    }
}

/// Choose a preset for some settings, update it, or save the settings as a
/// new one. Used on the Preview tab (for one series) and in Settings (for the
/// starting point of new series).
#[component]
pub fn PresetPicker(
    value: ConversionSettings,
    preset_id: Option<String>,
    /// The user chose a preset: its id and settings.
    on_pick: EventHandler<(String, ConversionSettings)>,
    /// The current settings were saved as a new preset with this id.
    on_saved: EventHandler<String>,
) -> Element {
    let state = use_context::<AppState>();
    let mut naming = use_signal(|| false);
    let mut name = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    let entries = state.presets.read().all();
    let current = preset_id
        .as_ref()
        .and_then(|id| entries.iter().find(|e| &e.preset.id == id))
        .cloned();
    let changed = current.as_ref().is_some_and(|e| e.preset.settings != value);
    let selected = current
        .as_ref()
        .map(|e| e.preset.id.clone())
        .unwrap_or_default();

    let pick_entries = entries.clone();
    let on_select = move |e: Event<FormData>| {
        let id = e.value();
        if let Some(entry) = pick_entries.iter().find(|p| p.preset.id == id) {
            on_pick.call((entry.preset.id.clone(), entry.preset.settings.clone()));
        }
    };

    let update_value = value.clone();
    let update_id = current.as_ref().map(|e| e.preset.id.clone());
    let update_preset = move |_| {
        if let Some(id) = &update_id {
            let result = state.edit_presets(|p| p.overwrite(id, update_value.clone()));
            match result {
                Ok(()) => {
                    state.save_presets();
                    state.notify("Preset updated.");
                }
                Err(e) => state.error(e.to_string()),
            }
        }
    };
    let undo = current.as_ref().map(|e| {
        let (id, settings) = (e.preset.id.clone(), e.preset.settings.clone());
        move |_| on_pick.call((id.clone(), settings.clone()))
    });

    let save_value = value.clone();
    let save_new = move |_| {
        let result = state.edit_presets(|p| p.save_new(&name.read(), save_value.clone()));
        match result {
            Ok(id) => {
                state.save_presets();
                naming.set(false);
                name.set(String::new());
                error.set(None);
                on_saved.call(id);
                state.notify("Preset saved.");
            }
            Err(e) => error.set(Some(e.to_string())),
        }
    };

    rsx! {
        div { class: "card",
            h2 { Icon { name: "layers", size: 17 } "Preset" }
            select { value: "{selected}", onchange: on_select,
                option { value: "", selected: current.is_none(), disabled: true, "Custom settings" }
                for e in entries.iter() {
                    option {
                        key: "{e.preset.id}",
                        value: "{e.preset.id}",
                        selected: e.preset.id == selected,
                        "{e.preset.name}"
                        if e.builtin && e.modified { " (yours)" }
                    }
                }
            }
            if let Some(entry) = &current {
                p { class: "muted small", style: "margin: 8px 0 0",
                    if changed { "Changed from “{entry.preset.name}”. " }
                    if !entry.preset.description.is_empty() { "{entry.preset.description}" }
                }
            } else {
                p { class: "muted small", style: "margin: 8px 0 0", "Pick one to start from, or save your own below." }
            }
            if changed {
                div { class: "row wrap", style: "margin-top: 8px",
                    button { class: "btn small", onclick: update_preset,
                        Icon { name: "check", size: 14 }
                        "Save changes to the preset"
                    }
                    if let Some(undo) = undo {
                        button { class: "btn ghost small", onclick: undo,
                            Icon { name: "rotate-ccw", size: 14 }
                            "Undo changes"
                        }
                    }
                }
            }
            if naming() {
                div { class: "row", style: "margin-top: 8px",
                    input {
                        r#type: "text", class: "grow", placeholder: "Name your preset",
                        maxlength: "40", autofocus: true, value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                    button { class: "btn primary small", onclick: save_new, "Save" }
                    button { class: "btn ghost small", onclick: move |_| { naming.set(false); error.set(None); }, "Cancel" }
                }
                if let Some(message) = error() {
                    p { class: "small", style: "color: var(--bad); margin: 6px 0 0", "{message}" }
                }
            } else {
                div { class: "row wrap", style: "margin-top: 8px",
                    button { class: "btn ghost small", onclick: move |_| naming.set(true),
                        Icon { name: "plus", size: 14 }
                        "Save as a new preset"
                    }
                    button { class: "btn ghost small", onclick: move |_| state.go(Page::Settings),
                        "Manage presets"
                    }
                }
            }
        }
    }
}

/// The list of presets in Settings: use one for new series, rename, delete.
#[component]
pub fn PresetsCard() -> Element {
    let state = use_context::<AppState>();
    let mut renaming = use_signal(|| None::<(String, String)>);
    let mut error = use_signal(|| None::<String>);
    let entries = state.presets.read().all();
    let defaults = state.settings.read().conversion.clone();

    rsx! {
        div { class: "card",
            h2 { Icon { name: "layers", size: 17 } "Presets" }
            p { class: "muted small",
                "A preset is a saved set of page settings. Manga, manhwa and manhua are made differently, so each has its own to start from. "
                "Choose one while previewing a series, or save your own there."
            }
            if let Some(message) = error() {
                div { class: "banner bad small", Icon { name: "circle-x", size: 16 } "{message}" }
            }
            for e in entries.iter() {
                {
                    let id = e.preset.id.clone();
                    let is_start = e.preset.settings == defaults;
                    let editing = renaming().filter(|(rid, _)| rid == &id);
                    let summary = describe_settings(&e.preset.settings);
                    let settings = e.preset.settings.clone();
                    let (use_id, rename_id, delete_id, save_id) = (id.clone(), id.clone(), id.clone(), id.clone());
                    let (current_name, builtin, modified) = (e.preset.name.clone(), e.builtin, e.modified);
                    let delete_name = e.preset.name.clone();
                    rsx! {
                        div { class: "list-row", key: "{id}", style: "align-items: flex-start",
                            div { class: "grow",
                                if let Some((_, draft)) = editing {
                                    div { class: "row",
                                        input {
                                            r#type: "text", class: "grow", maxlength: "40", autofocus: true, value: "{draft}",
                                            oninput: move |ev| {
                                                let v = ev.value();
                                                renaming.set(Some((rename_id.clone(), v)));
                                            },
                                        }
                                        button { class: "btn primary small",
                                            onclick: move |_| {
                                                let draft = renaming().map(|(_, d)| d).unwrap_or_default();
                                                let result = state.edit_presets(|p| p.rename(&save_id, &draft));
                                                match result {
                                                    Ok(()) => { state.save_presets(); renaming.set(None); error.set(None); }
                                                    Err(err) => error.set(Some(err.to_string())),
                                                }
                                            },
                                            "Save"
                                        }
                                        button { class: "btn ghost small", onclick: move |_| { renaming.set(None); error.set(None); }, "Cancel" }
                                    }
                                } else {
                                    div { class: "row wrap",
                                        strong { "{current_name}" }
                                        if builtin { Chip { text: "built in" } }
                                        if modified { Chip { text: "changed by you", kind: Some("warn".to_string()) } }
                                        if is_start { Chip { text: "starting point", kind: Some("ok".to_string()) } }
                                    }
                                }
                                if !e.preset.description.is_empty() {
                                    div { class: "muted small", "{e.preset.description}" }
                                }
                                div { class: "muted small", "{summary}" }
                            }
                            if !is_start {
                                button { class: "btn small", title: "New series start with these settings",
                                    onclick: move |_| {
                                        let mut settings_sig = state.settings;
                                        {
                                            let mut s = settings_sig.write();
                                            s.conversion = settings.clone();
                                            s.default_preset_id = Some(use_id.clone());
                                        }
                                        state.save_settings();
                                    },
                                    "Use for new series"
                                }
                            }
                            if !builtin {
                                button { class: "btn small", title: "Rename",
                                    onclick: move |_| { error.set(None); renaming.set(Some((delete_id.clone(), current_name.clone()))); },
                                    Icon { name: "pencil", size: 14 }
                                }
                            }
                            if !builtin || modified {
                                button {
                                    class: "btn small danger",
                                    title: if builtin { "Put back the original settings" } else { "Delete this preset" },
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
                                        action: ModalAction::DeletePreset { id: id.clone() },
                                    }),
                                    if builtin { Icon { name: "rotate-ccw", size: 14 } } else { Icon { name: "trash", size: 14 } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
