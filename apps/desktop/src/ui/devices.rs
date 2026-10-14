//! Choosing the reader pages are made for, and adding your own.

use super::widgets::{Help, Icon};
use crate::state::AppState;
use device_profiles::{DeviceError, DeviceProfile, Firmware};
use dioxus::prelude::*;
use crate::state::{Page, SettingsSection};
use library::ReaderSetup;

/// The file types a reader can be said to open.
const FORMAT_OPTIONS: [&str; 2] = ["XTC", "XTCH"];

/// The formats as a row of pills inside a box: type one and press Enter, or
/// pick from the list that opens under the box.
#[component]
fn FormatTags(value: Vec<String>, on_change: EventHandler<Vec<String>>) -> Element {
    let mut text = use_signal(String::new);
    let mut focused = use_signal(|| false);
    let query = text().trim().to_uppercase();
    let free: Vec<&str> = FORMAT_OPTIONS
        .iter()
        .copied()
        .filter(|o| !value.iter().any(|v| v.eq_ignore_ascii_case(o)) && o.contains(query.as_str()))
        .collect();
    let with = {
        let value = value.clone();
        move |option: &str| {
            let mut next = value.clone();
            next.push(option.to_string());
            next.sort_by_key(|v| FORMAT_OPTIONS.iter().position(|o| o.eq_ignore_ascii_case(v)));
            next
        }
    };
    let first_free = free.first().map(|f| f.to_string());
    let on_enter = with.clone();
    let last_removable = value.last().cloned();
    let value_for_backspace = value.clone();
    rsx! {
        div { class: if focused() { "tags focused" } else { "tags" },
            for v in value.iter() {
                {
                    let name = v.clone();
                    let rest: Vec<String> = value.iter().filter(|x| *x != v).cloned().collect();
                    rsx! {
                        span { class: "tag", key: "{name}", role: "button", title: "Remove {name}",
                            onclick: move |_| on_change.call(rest.clone()),
                            "{name}"
                            span { class: "tag-x", Icon { name: "x", size: 11 } }
                        }
                    }
                }
            }
            input {
                r#type: "text", value: "{text}", spellcheck: "false", "aria-label": "Add a file format",
                placeholder: if value.is_empty() { "Add a format" } else { "" },
                onfocus: move |_| focused.set(true),
                onblur: move |_| focused.set(false),
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        e.prevent_default();
                        if let Some(option) = &first_free {
                            on_change.call(on_enter(option));
                            text.set(String::new());
                        }
                    } else if e.key() == Key::Backspace && text().is_empty() && last_removable.is_some() {
                        let mut next = value_for_backspace.clone();
                        next.pop();
                        on_change.call(next);
                    }
                },
            }
            if focused() && !free.is_empty() {
                div { class: "tag-options",
                    for option in free.iter().copied() {
                        button {
                            key: "{option}", class: "tag-option", r#type: "button",
                            // Pressed before the box loses focus, so the list stays until it is used.
                            onmousedown: {
                                let with = with.clone();
                                move |e| {
                                    e.prevent_default();
                                    on_change.call(with(option));
                                    text.set(String::new());
                                }
                            },
                            "{option}"
                        }
                    }
                }
            }
        }
    }
}

/// Options of which any number can be on, looking like the one-of-several
/// rows but each with a box that is ticked when it is on.
#[component]
fn MultiSeg(items: Vec<(String, bool)>, on_toggle: EventHandler<usize>) -> Element {
    rsx! {
        div { class: "seg multi", role: "group",
            for (i, (label, on)) in items.iter().enumerate() {
                button {
                    key: "{label}", r#type: "button", role: "checkbox", "aria-checked": "{on}",
                    class: if *on { "on" } else { "" },
                    onclick: move |_| on_toggle.call(i),
                    span { class: "mark",
                        if *on { Icon { name: "check", size: 12 } }
                    }
                    "{label}"
                }
            }
        }
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

/// The other edge of a screen when one edge is typed and the shape is kept.
fn keep_shape(typed: u32, old_typed: u32, old_other: u32) -> u32 {
    if old_typed == 0 {
        return old_other;
    }
    (typed as f64 * old_other as f64 / old_typed as f64).round() as u32
}

/// What the reader form holds before it is saved.
#[derive(Clone, PartialEq, Debug)]
struct ReaderDraft {
    name: String,
    width: u32,
    height: u32,
    firmware: Firmware,
    /// How the reader receives files, and the file types it opens.
    setup: ReaderSetup,
}

/// Save a draft over the chosen reader. A built-in reader whose size changes
/// is copied into a reader of your own, which is then chosen. Nothing changes
/// if the result is not allowed.
fn apply_reader(settings: &mut library::Settings, draft: &ReaderDraft) -> Result<(), DeviceError> {
    let others = settings.devices();
    let base = settings.device_profile();
    let id = if base.builtin {
        if (draft.width, draft.height) == (base.width, base.height) {
            base.id.clone()
        } else {
            let copy =
                DeviceProfile::custom(&others, &unused_name(&others), draft.width, draft.height)?;
            let id = copy.id.clone();
            settings.device = id.clone();
            settings.custom_devices.push(copy);
            id
        }
    } else {
        let mut updated = base.clone();
        updated.name = draft.name.trim().to_string();
        updated.width = draft.width;
        updated.height = draft.height;
        updated.validate(&others)?;
        if let Some(slot) = settings.custom_devices.iter_mut().find(|d| d.id == base.id) {
            *slot = updated;
        }
        base.id.clone()
    };
    settings.firmware = draft.firmware;
    settings.reader_setups.insert(id, draft.setup.clone());
    Ok(())
}

#[component]
pub fn DevicesEditor() -> Element {
    let state = use_context::<AppState>();
    // Bumped after a save, so the form starts again from what was saved.
    let revision = use_signal(|| 0u32);
    let s = state.settings.read().clone();
    let devices = s.devices();
    let current = s.device_profile();
    let form_key = format!("{}-{}", current.id, revision());

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
                    // and set its size below.
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
        // Keyed, so choosing another reader (or saving) starts the form afresh.
        for key in [form_key] {
            ReaderForm { key: "{key}", revision }
        }
    }
}

/// The chosen reader, edited as a draft and written when Save is pressed.
#[component]
fn ReaderForm(revision: Signal<u32>) -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let current = s.device_profile();
    let editable = !current.builtin;
    let saved = ReaderDraft {
        name: current.name.clone(),
        width: current.width,
        height: current.height,
        firmware: s.firmware,
        setup: s.reader_setup(),
    };
    let mut setup = use_signal(|| saved.setup.clone());
    let mut wifi_test = use_signal(|| None::<Result<(), String>>);
    let mut testing = use_signal(|| false);
    let mut name = use_signal(|| saved.name.clone());
    let mut width = use_signal(|| saved.width.to_string());
    let mut height = use_signal(|| saved.height.to_string());
    let mut firmware = use_signal(|| saved.firmware);
    let mut locked = use_signal(|| false);

    let draft = ReaderDraft {
        name: name(),
        width: width().trim().parse().unwrap_or(0),
        height: height().trim().parse().unwrap_or(0),
        firmware: firmware(),
        setup: setup(),
    };
    let dirty = draft != saved;
    let wifi_url_now = setup().wifi_url.clone().unwrap_or_default();
    let wifi_url_to_test = wifi_url_now.clone();
    let (w0, h0) = (saved.width, saved.height);
    let remove_id = current.id.clone();
    let lock_class = if locked() { "btn icon on" } else { "btn icon" };
    let lock_icon = if locked() { "lock" } else { "lock-open" };
    let lock_title = if locked() {
        "The shape is kept while you change a size. Click to unlock."
    } else {
        "Lock the shape, so changing one size changes the other"
    };
    let pressed = if locked() { "true" } else { "false" };

    rsx! {
        div { class: "reader-line",
            if editable {
                input {
                    class: "title-input", r#type: "text", maxlength: "40", value: "{name}",
                    title: "Click to rename this reader", "aria-label": "Reader name",
                    oninput: move |e| name.set(e.value()),
                }
            } else {
                div { class: "title-input fixed", title: "A built-in reader keeps its name", "{current.name}" }
            }
        }

        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Screen size"
                    Help { text: "The size of the reader's screen in pixels (dots), as its maker lists it. Pages are made exactly this size, so nothing is scaled again on the reader. Changing the size of a built-in reader saves a reader of your own." }
                }
            }
            div { class: "sx-control",
                div { class: "size-boxes labelled",
                    label { class: "size-field",
                        span { "Width" }
                        div { class: "unit-box",
                            input {
                                r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                                value: "{width}", "aria-label": "Screen width in pixels",
                                oninput: move |e| {
                                    let typed = e.value();
                                    if locked() {
                                        if let Ok(w) = typed.trim().parse::<u32>() {
                                            height.set(keep_shape(w, w0, h0).to_string());
                                        }
                                    }
                                    width.set(typed);
                                },
                            }
                            span { class: "unit", "px" }
                        }
                    }
                    label { class: "size-field",
                        span { "Height" }
                        div { class: "unit-box",
                            input {
                                r#type: "number", min: "{device_profiles::MIN_EDGE}", max: "{device_profiles::MAX_EDGE}",
                                value: "{height}", "aria-label": "Screen height in pixels",
                                oninput: move |e| {
                                    let typed = e.value();
                                    if locked() {
                                        if let Ok(h) = typed.trim().parse::<u32>() {
                                            width.set(keep_shape(h, h0, w0).to_string());
                                        }
                                    }
                                    height.set(typed);
                                },
                            }
                            span { class: "unit", "px" }
                        }
                    }
                    button { class: "{lock_class}", title: "{lock_title}", "aria-pressed": "{pressed}",
                        onclick: move |_| locked.set(!locked()),
                        Icon { name: lock_icon, size: 16 }
                    }
                }
            }
        }

        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "File formats"
                    Help { text: "The file types this reader opens. Type one and press Enter, or pick from the list." }
                }
            }
            div { class: "sx-control",
                FormatTags {
                    value: setup().formats.clone(),
                    on_change: move |list: Vec<String>| setup.write().formats = list,
                }
            }
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Firmware"
                    Help { text: "What the reader runs. It decides which file types it can open." }
                }
            }
            div { class: "sx-control",
                select {
                    onchange: move |e| firmware.set(if e.value() == "cross" { Firmware::CrossPoint } else { Firmware::Stock }),
                    option { value: "cross", selected: firmware() == Firmware::CrossPoint, "CrossPoint" }
                    option { value: "stock", selected: firmware() == Firmware::Stock, "Stock (the one it came with)" }
                }
            }
        }

        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Export method"
                    Help { text: "How files can get to this reader. Pick every way that works for it; when you export you choose which one to use." }
                }
            }
            div { class: "sx-control",
                MultiSeg {
                    items: vec![("USB".to_string(), setup().usb), ("Wi-Fi".to_string(), setup().wifi)],
                    on_toggle: move |i: usize| {
                        // At least one stays on: there has to be a way to reach the reader.
                        let mut s = setup.write();
                        if i == 0 {
                            if !s.usb || s.wifi { s.usb = !s.usb; }
                        } else if !s.wifi || s.usb {
                            s.wifi = !s.wifi;
                        }
                    },
                }
            }
        }
        if setup().wifi {
            div { class: "sx-row",
                div { class: "sx-text",
                    div { class: "sx-label", "Reader address"
                        Help { text: "The address the reader shows on its screen when its Wi-Fi file transfer is on, for example http://192.168.3.3" }
                    }
                }
                div { class: "sx-control sx-inline",
                    input {
                        r#type: "text", placeholder: "http://192.168.3.3", value: "{wifi_url_now}",
                        oninput: move |e| {
                            let v = e.value();
                            setup.write().wifi_url = (!v.trim().is_empty()).then(|| v.trim().to_string());
                            wifi_test.set(None);
                        },
                    }
                    button { class: "btn", disabled: testing() || wifi_url_now.trim().is_empty(),
                        onclick: move |_| {
                            let url = wifi_url_to_test.clone();
                            spawn(async move {
                                testing.set(true);
                                wifi_test.set(None);
                                let result = pipeline::export::check_webdav(&url).await;
                                wifi_test.set(Some(result));
                                testing.set(false);
                            });
                        },
                        if testing() { "Looking…" } else { "Test" }
                    }
                }
            }
            {match &*wifi_test.read() {
                Some(Ok(())) => rsx! { p { class: "sx-note", style: "color: var(--ok)", "The reader answered." } },
                Some(Err(message)) => rsx! { p { class: "sx-note", style: "color: var(--bad)", "{message}" } },
                None => rsx! {},
            }}
        }
        if setup().usb {
            p { class: "sx-note", "For USB, plug the reader in (drive mode) or put its SD card in this computer. You pick the drive on the Export screen." }
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Folder on the reader"
                    Help { text: "Files go into this folder on the reader. It is made if it is not there." }
                }
            }
            div { class: "sx-control",
                input {
                    r#type: "text", value: "{setup().folder}",
                    oninput: move |e| setup.write().folder = e.value(),
                }
            }
        }

        div { class: "form-footer",
            if editable {
                button { class: "btn danger",
                    title: "Series made for it keep their settings. Your Xteink X4 Pro is chosen instead.",
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
                    "Remove this reader"
                }
            }
            span { class: "grow" }
            button { class: "btn primary", disabled: !dirty,
                onclick: {
                    let draft = draft.clone();
                    let mut revision = revision;
                    move |_| {
                        let mut settings = state.settings;
                        let result = apply_reader(&mut settings.write(), &draft);
                        match result {
                            Ok(()) => {
                                state.save_settings();
                                state.notify("Reader saved.");
                                let again = *revision.peek() + 1;
                                revision.set(again);
                            }
                            Err(e) => state.error(e.to_string()),
                        }
                    }
                },
                Icon { name: "check", size: 16 }
                "Save"
            }
        }
    }
}

/// Open Settings on the page to add or change a reader.
fn open_readers_settings(state: AppState) {
    let mut section = state.settings_section;
    section.set(SettingsSection::Reader);
    state.go(Page::Settings);
}

/// One reader in the list on the Export screen: its name, and its screen size
/// on the same row. Choosing it makes pages for it. Its name and size are
/// changed in Settings.
#[component]
fn ReaderItem(id: String, on: bool) -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let Some(device) = s.devices().into_iter().find(|d| d.id == id) else {
        return rsx! {};
    };
    rsx! {
        div { class: if on { "px-preset on" } else { "px-preset" },
            onclick: move |_| {
                if !on {
                    let mut settings = state.settings;
                    settings.write().device = id.clone();
                    state.save_settings();
                    crate::ui::work::schedule_preview(state, 0);
                }
            },
            div { class: "px-preset-head",
                span { class: "grow px-item-name", "{device.name}" }
                span { class: "px-size", "{device.width} \u{d7} {device.height} px" }
                if on { Icon { name: "check", size: 14 } }
            }
        }
    }
}

/// The readers as a short scrollable list, and a way to Settings to add one.
#[component]
pub fn ReaderPicker() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    rsx! {
        div { class: "px-list px-scroll",
            for d in s.devices() {
                ReaderItem { key: "{d.id}", id: d.id.clone(), on: d.id == s.device }
            }
        }
        button { class: "px-new", style: "margin-top: 6px; width: 100%", title: "Readers are added in Settings",
            onclick: move |_| open_readers_settings(state),
            Icon { name: "plus", size: 14 } "New device"
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

    fn draft(name: &str, width: u32, height: u32) -> ReaderDraft {
        ReaderDraft {
            name: name.into(),
            width,
            height,
            firmware: Firmware::CrossPoint,
            setup: ReaderSetup::default(),
        }
    }

    #[test]
    fn changing_a_built_in_readers_size_saves_a_copy_and_chooses_it() {
        let mut s = library::Settings::default();
        let before = s.devices().len();
        let d = draft("Xteink X4 Pro", 240, 400);
        apply_reader(&mut s, &d).unwrap();
        assert_eq!(s.devices().len(), before + 1);
        let chosen = s.device_profile();
        assert!(!chosen.builtin);
        assert_eq!((chosen.width, chosen.height), (240, 400));
    }

    #[test]
    fn a_custom_reader_is_changed_in_place_and_a_bad_size_changes_nothing() {
        let mut s = library::Settings::default();
        apply_reader(&mut s, &draft("x", 240, 400)).unwrap();
        let id = s.device.clone();
        apply_reader(&mut s, &draft("  Travel  ", 300, 500)).unwrap();
        let now = s.device_profile();
        assert_eq!(
            (now.id.as_str(), now.name.as_str(), now.width),
            (id.as_str(), "Travel", 300)
        );
        let before = s.clone();
        assert!(apply_reader(&mut s, &draft("Travel", 5, 500)).is_err());
        assert_eq!(s.device_profile().width, before.device_profile().width);
        assert!(apply_reader(&mut s, &draft("  ", 300, 500)).is_err());
    }
}
