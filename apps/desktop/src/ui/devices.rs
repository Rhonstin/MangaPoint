//! Choosing the reader pages are made for, and adding your own.

use super::widgets::{Chip, Icon};
use crate::state::AppState;
use device_profiles::{DeviceProfile, Firmware, Support};
use dioxus::prelude::*;

fn support_chip(support: Support) -> (&'static str, &'static str) {
    match support {
        Support::Documented => ("opens XTC", "ok"),
        Support::CommunityReported => ("reported to open XTC", "ok"),
        Support::Unverified => ("not tried yet", "warn"),
    }
}

/// A name for a new reader that no other reader has.
fn unused_name(existing: &[DeviceProfile]) -> String {
    let taken = |name: &str| {
        existing
            .iter()
            .any(|d| d.name.trim().eq_ignore_ascii_case(name))
    };
    let mut name = "My reader".to_string();
    let mut n = 2;
    while taken(&name) {
        name = format!("My reader {n}");
        n += 1;
    }
    name
}

/// Change the chosen custom reader, if the result is allowed. A refused change
/// says why and puts the old value back on the screen.
fn edit_current(state: AppState, mut nonce: Signal<u32>, edit: impl FnOnce(&mut DeviceProfile)) {
    let mut settings = state.settings;
    let result = {
        let mut st = settings.write();
        let id = st.device.clone();
        let others = st.devices();
        let mut updated = st.device_profile();
        edit(&mut updated);
        updated.validate(&others).map(|()| {
            if let Some(slot) = st.custom_devices.iter_mut().find(|d| d.id == id) {
                *slot = updated;
            }
        })
    };
    match result {
        Ok(()) => state.save_settings(),
        Err(e) => {
            state.error(e.to_string());
            let again = *nonce.peek() + 1;
            nonce.set(again);
        }
    }
}

#[component]
pub fn DevicesEditor() -> Element {
    let state = use_context::<AppState>();
    // Bumped when a change is refused, so the boxes show the old value again.
    let nonce = use_signal(|| 0u32);

    let s = state.settings.read().clone();
    let devices = s.devices();
    let current = s.device_profile();
    let editable = !current.builtin;
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
    let key = format!("{}-{}", current.id, nonce());

    rsx! {
        div { class: "pills", style: "margin-bottom: 18px",
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
                            },
                            "{d.name}"
                        }
                    }
                }
            }
            button { class: "pill add", title: "Add a reader with a different screen",
                onclick: move |_| {
                    // A new reader appears at once, a copy of this one; rename it
                    // in the heading and set its size below.
                    let mut settings = state.settings;
                    {
                        let mut st = settings.write();
                        let all = st.devices();
                        let base = st.device_profile();
                        if let Ok(d) = DeviceProfile::custom(&all, &unused_name(&all), base.width, base.height) {
                            st.device = d.id.clone();
                            st.custom_devices.push(d);
                        }
                    }
                    state.save_settings();
                },
                Icon { name: "plus", size: 14 }
                "Add device"
            }
        }

        div { class: "sx-eyebrow", "Your reader" }
        div { class: "reader-title",
            if editable {
                input {
                    key: "{key}",
                    class: "title-input", r#type: "text", maxlength: "40", value: "{current.name}",
                    title: "Click to rename this reader", "aria-label": "Reader name",
                    onchange: move |e| edit_current(state, nonce, |d| d.name = e.value().trim().to_string()),
                }
                Icon { name: "pencil", size: 16 }
            } else {
                h1 { class: "sx-title", style: "margin: 0", "{current.name}" }
            }
            Chip { text: current.size_label() }
            if current.builtin { Chip { text: "built in" } }
        }
        p { class: "muted small", style: "margin: 6px 0 4px",
            "Pages are made exactly this size, so nothing is scaled again on the reader."
        }

        if editable {
            div { class: "sx-row",
                div { class: "sx-text",
                    div { class: "sx-label", "Screen size" }
                    div { class: "sx-desc", "Pixels as the reader shows the page upright: width, then height." }
                }
                div { class: "sx-control sx-inline",
                    input {
                        key: "w-{key}",
                        r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                        value: "{current.width}", title: "Width in pixels", "aria-label": "Width in pixels",
                        onchange: move |e| {
                            let w = e.value().trim().parse::<u32>().unwrap_or(0);
                            edit_current(state, nonce, |d| d.width = w);
                        },
                    }
                    span { class: "muted", style: "align-self: center", "\u{d7}" }
                    input {
                        key: "h-{key}",
                        r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                        value: "{current.height}", title: "Height in pixels", "aria-label": "Height in pixels",
                        onchange: move |e| {
                            let h = e.value().trim().parse::<u32>().unwrap_or(0);
                            edit_current(state, nonce, |d| d.height = h);
                        },
                    }
                }
            }
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Opens XTC files" }
                div { class: "sx-desc",
                    if editable { "A custom reader counts as untested until you try a file on it." }
                    else { "What we know about this reader and each firmware." }
                }
            }
            div { class: "sx-control sx-inline", style: "flex-wrap: wrap; justify-content: flex-end",
                span { class: "muted small", style: "align-self: center", "CrossPoint" }
                Chip { text: cross_text, kind: Some(cross_kind.to_string()) }
                span { class: "muted small", style: "align-self: center", "Stock" }
                Chip { text: stock_text, kind: Some(stock_kind.to_string()) }
            }
        }
        div { class: "sx-row",
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
        if editable {
            div { class: "sx-row",
                div { class: "sx-text",
                    div { class: "sx-label", "Remove this reader" }
                    div { class: "sx-desc", "Series made for it keep their settings. Your Xteink X4 Pro is chosen instead." }
                }
                div { class: "sx-control",
                    button { class: "btn danger",
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
                        Icon { name: "trash", size: 15 }
                        "Remove"
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
