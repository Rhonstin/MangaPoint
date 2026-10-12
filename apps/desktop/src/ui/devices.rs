//! Choosing the reader pages are made for, and adding your own.

use super::widgets::{Chip, Icon};
use crate::state::AppState;
use device_profiles::{DeviceProfile, Firmware, Support};
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Editing {
    New,
    Existing,
}

fn support_chip(support: Support) -> (&'static str, &'static str) {
    match support {
        Support::Documented => ("opens XTC", "ok"),
        Support::CommunityReported => ("reported to open XTC", "ok"),
        Support::Unverified => ("not tried yet", "warn"),
    }
}

#[component]
pub fn DevicesEditor() -> Element {
    let state = use_context::<AppState>();
    let mut editing = use_signal(|| None::<Editing>);
    let mut name = use_signal(String::new);
    let mut width = use_signal(String::new);
    let mut height = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    let s = state.settings.read().clone();
    let devices = s.devices();
    let current = s.device_profile();
    let (stock_text, stock_kind) = support_chip(current.xtc_support(Firmware::Stock));
    let (cross_text, cross_kind) = support_chip(current.xtc_support(Firmware::CrossPoint));
    let presets = state.presets.read().all();
    let preset_for_device = s
        .device_presets
        .get(&current.id)
        .cloned()
        .unwrap_or_default();
    let device_id = current.id.clone();
    let remove_id = current.id.clone();
    let edit_from = current.clone();

    rsx! {
        div { class: "pills", style: "margin-bottom: 12px",
            for d in devices {
                {
                    let id = d.id.clone();
                    let on = d.id == s.device;
                    rsx! {
                        button {
                            key: "{d.id}",
                            class: if on { "pill on" } else { "pill" },
                            onclick: move |_| {
                                let mut settings = state.settings;
                                settings.write().device = id.clone();
                                state.save_settings();
                                editing.set(None);
                            },
                            "{d.name}"
                        }
                    }
                }
            }
            button { class: "pill add", title: "Add a reader with a different screen",
                onclick: move |_| {
                    name.set(String::new());
                    width.set("480".into());
                    height.set("800".into());
                    error.set(None);
                    editing.set(Some(Editing::New));
                },
                Icon { name: "plus", size: 14 }
                "Add device"
            }
        }

        div { class: "card",
            div { class: "row wrap",
                strong { style: "font-size: 1.05rem", "{current.name}" }
                Chip { text: current.size_label() }
                if current.builtin { Chip { text: "built in" } }
            }
            p { class: "muted small", style: "margin: 6px 0 0",
                "Pages are made exactly this size, so nothing is scaled again on the reader."
            }
            div { class: "row wrap", style: "margin-top: 8px",
                span { class: "muted small", "CrossPoint" }
                Chip { text: cross_text, kind: Some(cross_kind.to_string()) }
                span { class: "muted small", "Stock firmware" }
                Chip { text: stock_text, kind: Some(stock_kind.to_string()) }
            }

            if let Some(mode) = editing() {
                div { class: "row wrap", style: "margin-top: 12px",
                    input {
                        r#type: "text", class: "grow", maxlength: "40", value: "{name}",
                        placeholder: "Name, for example Kobo Nia",
                        oninput: move |e| name.set(e.value()),
                    }
                    input {
                        r#type: "number", style: "width: 96px", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                        value: "{width}", title: "Width in pixels",
                        oninput: move |e| width.set(e.value()),
                    }
                    span { class: "muted", "\u{d7}" }
                    input {
                        r#type: "number", style: "width: 96px", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                        value: "{height}", title: "Height in pixels",
                        oninput: move |e| height.set(e.value()),
                    }
                    button { class: "btn primary small",
                        onclick: move |_| {
                            let w = width.read().trim().parse::<u32>().unwrap_or(0);
                            let h = height.read().trim().parse::<u32>().unwrap_or(0);
                            let mut settings = state.settings;
                            let result = {
                                let mut st = settings.write();
                                match mode {
                                    Editing::New => DeviceProfile::custom(&st.devices(), &name.read(), w, h)
                                        .map(|d| { st.device = d.id.clone(); st.custom_devices.push(d); }),
                                    Editing::Existing => {
                                        let id = st.device.clone();
                                        let others: Vec<DeviceProfile> = st.devices();
                                        let mut updated = st.device_profile();
                                        updated.name = name.read().trim().to_string();
                                        updated.width = w;
                                        updated.height = h;
                                        updated.validate(&others).map(|()| {
                                            if let Some(slot) = st.custom_devices.iter_mut().find(|d| d.id == id) {
                                                *slot = updated;
                                            }
                                        })
                                    }
                                }
                            };
                            match result {
                                Ok(()) => { state.save_settings(); editing.set(None); error.set(None); }
                                Err(e) => error.set(Some(e.to_string())),
                            }
                        },
                        if mode == Editing::New { "Add" } else { "Save" }
                    }
                    button { class: "btn ghost small", onclick: move |_| { editing.set(None); error.set(None); }, "Cancel" }
                }
                p { class: "muted small", style: "margin: 6px 0 0",
                    "Size in pixels as the reader shows the page upright: width first, then height. Custom readers count as untested until you try a file on them."
                }
                if let Some(message) = error() {
                    p { class: "small", style: "margin: 6px 0 0; color: var(--bad)", "{message}" }
                }
            } else if !current.builtin {
                div { class: "row wrap", style: "margin-top: 12px",
                    button { class: "btn small",
                        onclick: move |_| {
                            name.set(edit_from.name.clone());
                            width.set(edit_from.width.to_string());
                            height.set(edit_from.height.to_string());
                            error.set(None);
                            editing.set(Some(Editing::Existing));
                        },
                        Icon { name: "pencil", size: 14 }
                        "Edit"
                    }
                    button { class: "btn small danger",
                        onclick: move |_| {
                            let mut settings = state.settings;
                            {
                                let mut st = settings.write();
                                st.custom_devices.retain(|d| d.id != remove_id);
                                st.device_presets.remove(&remove_id);
                                st.device = device_profiles::X4_PRO_ID.into();
                            }
                            state.save_settings();
                        },
                        Icon { name: "trash", size: 14 }
                        "Remove"
                    }
                }
            }
        }

        div { class: "sx-row", style: "margin-top: 12px",
            div { class: "sx-text",
                div { class: "sx-label", "Preset for this reader" }
                div { class: "sx-desc", "New series made for {current.name} start from this preset." }
            }
            div { class: "sx-control",
                select {
                    onchange: move |e| {
                        let mut settings = state.settings;
                        {
                            let mut st = settings.write();
                            let v = e.value();
                            if v.is_empty() {
                                st.device_presets.remove(&device_id);
                            } else {
                                st.device_presets.insert(device_id.clone(), v);
                            }
                        }
                        state.save_settings();
                    },
                    option { value: "", selected: preset_for_device.is_empty(), "Use the default" }
                    for p in presets {
                        option { key: "{p.preset.id}", value: "{p.preset.id}", selected: p.preset.id == preset_for_device, "{p.preset.name}" }
                    }
                }
            }
        }
    }
}

/// A dropdown to choose the reader pages are made for, with its page size.
#[component]
pub fn DeviceSelect() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let current = s.device_profile();
    rsx! {
        div { class: "panel-section",
            label { class: "panel-label", "Reader" }
            select {
                onchange: move |e| {
                    let mut settings = state.settings;
                    settings.write().device = e.value();
                    state.save_settings();
                    crate::ui::work::schedule_preview(state, 0);
                },
                for d in s.devices() {
                    option { key: "{d.id}", value: "{d.id}", selected: d.id == current.id, "{d.name}" }
                }
            }
            p { class: "muted small panel-note", "Pages are made {current.size_label()} pixels, the full screen." }
        }
    }
}
