//! Choosing the reader pages are made for, and adding your own.

use super::widgets::{Chip, Help, Icon};
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

/// Change the chosen reader, if the result is allowed. Changing a built-in
/// reader makes a reader of your own with the change, and chooses it. A
/// refused change says why and puts the old value back on the screen.
fn edit_current(state: AppState, mut nonce: Signal<u32>, edit: impl FnOnce(&mut DeviceProfile)) {
    let mut settings = state.settings;
    let result = {
        let mut st = settings.write();
        let others = st.devices();
        let base = st.device_profile();
        if base.builtin {
            DeviceProfile::custom(&others, &unused_name(&others), base.width, base.height).and_then(
                |mut copy| {
                    edit(&mut copy);
                    copy.validate(&others)?;
                    st.device = copy.id.clone();
                    st.custom_devices.push(copy);
                    Ok(())
                },
            )
        } else {
            let mut updated = base;
            edit(&mut updated);
            updated.validate(&others).map(|()| {
                if let Some(slot) = st.custom_devices.iter_mut().find(|d| d.id == updated.id) {
                    *slot = updated;
                }
            })
        }
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

/// The other edge of a screen when one edge is typed and the shape is kept.
fn keep_shape(typed: u32, old_typed: u32, old_other: u32) -> u32 {
    if old_typed == 0 {
        return old_other;
    }
    (typed as f64 * old_other as f64 / old_typed as f64).round() as u32
}

/// The width and height of the chosen reader as two boxes, and a lock that
/// keeps the shape while one of them changes.
#[component]
fn ReaderSize(nonce: Signal<u32>, on_change: Option<EventHandler<()>>) -> Element {
    let state = use_context::<AppState>();
    let mut locked = use_signal(|| false);
    let current = state.settings.read().device_profile();
    let key = format!("{}-{}", current.id, nonce());
    let (w0, h0) = (current.width, current.height);
    let changed = move || {
        if let Some(handler) = on_change {
            handler.call(());
        }
    };
    let lock_class = if locked() { "btn icon on" } else { "btn icon" };
    let lock_icon = if locked() { "lock" } else { "lock-open" };
    let lock_title = if locked() {
        "The shape is kept while you change a size. Click to unlock."
    } else {
        "Lock the shape, so changing one size changes the other"
    };
    let pressed = if locked() { "true" } else { "false" };
    rsx! {
        div { class: "size-boxes",
            input {
                key: "w-{key}",
                r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                value: "{current.width}", title: "Width in pixels", "aria-label": "Width in pixels",
                onchange: move |e| {
                    let w = e.value().trim().parse::<u32>().unwrap_or(0);
                    let h = if locked() { keep_shape(w, w0, h0) } else { h0 };
                    edit_current(state, nonce, |d| { d.width = w; d.height = h; });
                    changed();
                },
            }
            span { class: "muted", "\u{d7}" }
            input {
                key: "h-{key}",
                r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                value: "{current.height}", title: "Height in pixels", "aria-label": "Height in pixels",
                onchange: move |e| {
                    let h = e.value().trim().parse::<u32>().unwrap_or(0);
                    let w = if locked() { keep_shape(h, h0, w0) } else { w0 };
                    edit_current(state, nonce, |d| { d.width = w; d.height = h; });
                    changed();
                },
            }
            button { class: "{lock_class}", title: "{lock_title}", "aria-pressed": "{pressed}",
                onclick: move |_| locked.set(!locked()),
                Icon { name: lock_icon, size: 16 }
            }
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

        div { class: "reader-line",
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
                Chip { text: "built in" }
            }
            span { class: "grow" }
            ReaderSize { nonce }
            Help { text: "Pages are made exactly this size, so nothing is scaled again on the reader. Changing the size of a built-in reader makes a reader of your own." }
        }

        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Opens XTC files"
                    Help { text: if editable { "A custom reader counts as untested until you try a file on it." } else { "What we know about this reader and each firmware." } }
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
                div { class: "sx-label", "Preset for this reader"
                    Help { text: format!("New series made for {} start from this preset.", current.name) }
                }
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
                    div { class: "sx-label", "Remove this reader"
                        Help { text: "Series made for it keep their settings. Your Xteink X4 Pro is chosen instead." }
                    }
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

/// A dropdown to choose the reader pages are made for, and its page size as
/// boxes that can be changed.
#[component]
pub fn DeviceSelect() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let current = s.device_profile();
    let nonce = use_signal(|| 0u32);
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
            ReaderSize {
                nonce,
                on_change: move |_| crate::ui::work::schedule_preview(state, 0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str) -> DeviceProfile {
        let mut device = device_profiles::x4();
        device.name = name.to_string();
        device
    }

    #[test]
    fn a_new_reader_is_named_after_the_first_free_number() {
        assert_eq!(unused_name(&[]), "My reader");
        assert_eq!(unused_name(&[custom("My reader")]), "My reader 2");
        assert_eq!(
            unused_name(&[custom("my reader"), custom(" My reader 2 ")]),
            "My reader 3",
            "case and spaces do not make a name different"
        );
    }

    #[test]
    fn a_locked_shape_follows_the_size_typed() {
        assert_eq!(keep_shape(240, 480, 800), 400);
        assert_eq!(keep_shape(500, 480, 800), 833);
        assert_eq!(keep_shape(100, 0, 800), 800, "no old size to follow");
    }

    #[test]
    fn support_is_shown_with_a_colour() {
        assert_eq!(support_chip(Support::Unverified), ("not tried yet", "warn"));
        assert_eq!(support_chip(Support::Documented).1, "ok");
    }
}
