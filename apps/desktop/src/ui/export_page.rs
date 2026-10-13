//! The Export screen: full window. Editing the series is part of exporting
//! them, so this is where it happens. Presets, the editing controls and the
//! list of series being exported are panels on the left, the pages in the
//! middle, and the chapters and where to send them on the right. Every panel
//! folds away to give the others room.

use super::conversion::ConversionControls;
use super::devices::DeviceSelect;
use super::presets::{apply_to_series, delete_preset};
use super::widgets::{pick_folder, Chip, Cover, Icon};
use super::work::{
    activate_export_series, add_export_series, load_saved_cover, open_sample, remove_export_series,
    saved_cover_key, schedule_preview, start_export_plan,
};
use super::workspace::crop_editor::CropInspector;
use super::workspace::preview::{switch_chapter, PreviewTab};
use crate::logic::{self, plural, TargetChoice};
use crate::state::{AppState, GroupingChoice, JobKind, Modal, ModalAction, Page};
use dioxus::prelude::*;
use library::{CropRect, DownloadedSeries};
use pipeline::export::removable_drives;
use std::time::Duration;

/// The series on screen in the Export list.
fn active_series(state: AppState) -> Option<DownloadedSeries> {
    let plan = state.export_plan.read();
    let id = plan.items.get(plan.active)?.series_id.clone();
    drop(plan);
    state.library.read().find(&id).cloned()
}

/// A panel with a title that folds away.
#[component]
fn Panel(
    title: String,
    icon: &'static str,
    /// What the splitters call this panel.
    id: &'static str,
    /// A starting height in pixels. Without one the panel takes what is left.
    height: Option<u32>,
    /// Buttons for the title bar, which do not fold the panel.
    actions: Option<Element>,
    children: Element,
) -> Element {
    let mut open = use_signal(|| true);
    let class = match (open(), height.is_some()) {
        (true, true) => "px-panel open fixed",
        (true, false) => "px-panel open",
        _ => "px-panel collapsed",
    };
    let style = height.map(|h| format!("--h: {h}px;")).unwrap_or_default();
    let chevron = if open() {
        "chevron-down"
    } else {
        "chevron-right"
    };
    rsx! {
        section { class: "{class}", style: "{style}", "data-panel": "{id}",
            header { class: "px-panel-head", onclick: move |_| open.set(!open()),
                Icon { name: chevron, size: 15 }
                Icon { name: icon, size: 15 }
                span { class: "grow", "{title}" }
                if let Some(actions) = actions {
                    div { class: "px-panel-actions", onclick: move |e| e.stop_propagation(), {actions} }
                }
            }
            if open() {
                div { class: "px-panel-body", {children} }
            }
        }
    }
}

#[component]
pub fn ExportPage() -> Element {
    let state = use_context::<AppState>();
    let mut adding = use_signal(|| false);
    let mut closing = use_signal(|| false);
    let plan = state.export_plan.read().clone();

    // Nothing chosen to export: show the sample pages.
    use_effect(move || {
        if state.export_plan.peek().items.is_empty() {
            open_sample(state);
        }
    });
    // Opened again after other pages were visited: they changed what the
    // preview shows, so put the series of the list back.
    use_effect(move || {
        let plan = state.export_plan.peek().clone();
        let wanted = plan.items.get(plan.active).map(|i| i.series_id.clone());
        let shown = state.workspace.peek().as_ref().map(|w| w.series_id.clone());
        if wanted.is_some() && wanted != shown {
            activate_export_series(state, plan.active);
        }
    });
    // Remember the chapter being previewed.
    use_effect(move || {
        let key = state.preview.read().chapter_key.clone();
        let Some(key) = key else { return };
        let mut plan = state.export_plan;
        let differs = {
            let p = plan.peek();
            p.items
                .get(p.active)
                .is_some_and(|i| i.chapter.as_deref() != Some(key.as_str()))
        };
        if differs {
            let mut guard = plan.write();
            let active = guard.active;
            if let Some(item) = guard.items.get_mut(active) {
                item.chapter = Some(key);
            }
        }
    });

    // The covers of the series in the list.
    {
        let ids: Vec<String> = plan.items.iter().map(|i| i.series_id.clone()).collect();
        use_effect(move || {
            for id in &ids {
                load_saved_cover(state, id);
            }
        });
    }

    if plan.items.is_empty() {
        return rsx! { p { class: "muted", style: "padding: 24px", "Opening the sample pages…" } };
    }
    let sample = active_series(state).is_some_and(|s| s.is_sample());

    let mut close = move || {
        if closing() {
            return;
        }
        closing.set(true);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(180)).await;
            adding.set(false);
            closing.set(false);
        });
    };

    rsx! {
        div { class: "export-screen",
            onmounted: move |_| { spawn(async move { let _ = document::eval(SPLIT_SCRIPT).await; }); },
            div { class: "export-top",
                h1 { "Export" }
                if sample {
                    Chip { text: "Sample pages", kind: Some("accent".to_string()) }
                    span { class: "muted small", "Try the controls and presets here. To export your own series, pick them in the Library and choose Export…" }
                    span { class: "grow" }
                    button { class: "btn small", onclick: move |_| state.go(Page::Library),
                        Icon { name: "book-open", size: 15 } "Choose series"
                    }
                } else {
                    span { class: "muted", "{plural(plan.items.len(), \"series\")}" }
                }
            }
            div { class: "export-body",
                aside { class: "export-side left",
                    PresetsPanel {}
                    Split { target: "presets", dir: 1 }
                    EditPanel {}
                    Split { target: "series", dir: -1 }
                    SeriesPanel { on_add: move |_| adding.set(true) }
                    div { class: "px-split-v", "data-split": "left", title: "Drag to make this column wider or narrower" }
                }
                main { class: "export-center", PreviewTab {} }
                aside { class: "export-side right",
                    div { class: "px-split-v", "data-split": "right", title: "Drag to make this column wider or narrower" }
                    ChaptersPanel {}
                    Split { target: "send", dir: -1 }
                    SendPanel {}
                    ExportFooter {}
                }
            }
            if adding() {
                div { class: if closing() { "overlay out" } else { "overlay" }, onclick: move |_| close(),
                    div { class: "modal wide", onclick: move |e| e.stop_propagation(),
                        AddSeries { on_close: move |_| close() }
                    }
                }
            }
        }
    }
}

/// The bar between two panels, which can be dragged to resize them. `target`
/// is the panel that changes size; `dir` is -1 when dragging down shrinks it.
#[component]
fn Split(target: &'static str, dir: i32) -> Element {
    rsx! {
        div { class: "px-split", "data-split": "panel", "data-target": "{target}", "data-dir": "{dir}",
            title: "Drag to resize the panels" }
    }
}

/// Dragging the bars between the panels and the edges of the side columns.
/// The sizes are remembered between visits.
const SPLIT_SCRIPT: &str = include_str!("../js/export-split.js");

// ---- left: presets -------------------------------------------------------

#[component]
fn PresetsPanel() -> Element {
    let state = use_context::<AppState>();
    let nonce = use_signal(|| 0u32);
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let entries = state.presets.read().all();
    let default_id = state.settings.read().default_preset_id.clone();
    let current = series
        .preset_id
        .as_ref()
        .and_then(|id| entries.iter().find(|e| &e.preset.id == id))
        .cloned();
    let changed = current
        .as_ref()
        .is_some_and(|e| e.preset.settings != series.settings);
    let series_id = series.id.clone();
    let settings_now = series.settings.clone();
    let rename_key = format!(
        "{}-{nonce}",
        current.as_ref().map_or("none", |e| e.preset.id.as_str())
    );

    // A name for a new preset that no other has.
    let new_name = {
        let mut name = "My preset".to_string();
        let mut n = 2;
        while entries
            .iter()
            .any(|e| e.preset.name.eq_ignore_ascii_case(&name))
        {
            name = format!("My preset {n}");
            n += 1;
        }
        name
    };

    rsx! {
        Panel { title: "Presets", icon: "layers", id: "presets", height: 190,
            div { class: "px-list",
                for e in entries.iter() {
                    {
                        let on = current.as_ref().is_some_and(|c| c.preset.id == e.preset.id);
                        let (id, settings, sid) = (e.preset.id.clone(), e.preset.settings.clone(), series_id.clone());
                        rsx! {
                            button {
                                key: "{e.preset.id}",
                                class: if on { "px-item on" } else { "px-item" },
                                title: "{e.preset.description}",
                                onclick: move |_| apply_to_series(state, &sid, Some(id.clone()), settings.clone()),
                                span { class: "grow px-item-name", "{e.preset.name}" }
                                if default_id.as_deref() == Some(e.preset.id.as_str()) { Chip { text: "default" } }
                                if e.builtin && e.modified { Chip { text: "changed", kind: Some("warn".to_string()) } }
                                if on { Icon { name: "check", size: 14 } }
                            }
                        }
                    }
                }
            }
            if let Some(entry) = current {
                {
                    let (id_a, id_b, id_c, id_d, id_e, id_f) = (
                        entry.preset.id.clone(),
                        entry.preset.id.clone(),
                        entry.preset.id.clone(),
                        entry.preset.id.clone(),
                        entry.preset.id.clone(),
                        entry.preset.id.clone(),
                    );
                    let (name_for_modal, builtin, modified) = (entry.preset.name.clone(), entry.builtin, entry.modified);
                    let (sid_undo, undo_settings) = (series_id.clone(), entry.preset.settings.clone());
                    let (update_settings, default_settings) = (settings_now.clone(), settings_now.clone());
                    let is_default = default_id.as_deref() == Some(entry.preset.id.as_str());
                    rsx! {
                        div { class: "px-section",
                            if !builtin {
                                RenameBox { identity: rename_key.clone(), preset_id: id_a.clone(), name: entry.preset.name.clone(), nonce }
                            }
                            if !entry.preset.description.is_empty() {
                                p { class: "px-note", "{entry.preset.description}" }
                            }
                            if changed {
                                p { class: "px-note", style: "color: var(--warn)", "This series has changes the preset doesn't." }
                                div { class: "px-buttons",
                                    button { class: "btn small",
                                        onclick: move |_| {
                                            let result = state.edit_presets(|p| p.overwrite(&id_b, update_settings.clone()));
                                            match result {
                                                Ok(()) => { state.save_presets(); state.notify("Preset updated."); }
                                                Err(e) => state.error(e.to_string()),
                                            }
                                        },
                                        Icon { name: "check", size: 14 } "Save to preset"
                                    }
                                    button { class: "btn ghost small",
                                        onclick: move |_| apply_to_series(state, &sid_undo, Some(id_c.clone()), undo_settings.clone()),
                                        Icon { name: "rotate-ccw", size: 14 } "Undo"
                                    }
                                }
                            }
                            div { class: "px-buttons",
                                if !is_default {
                                    button { class: "btn ghost small", title: "New series start from this preset",
                                        onclick: move |_| {
                                            let mut s = state.settings;
                                            {
                                                let mut st = s.write();
                                                st.conversion = default_settings.clone();
                                                st.default_preset_id = Some(id_d.clone());
                                            }
                                            state.save_settings();
                                            state.notify("New series will start from this preset.");
                                        },
                                        "Use for new series"
                                    }
                                }
                                if builtin && modified {
                                    button { class: "btn ghost small", onclick: move |_| delete_preset(state, id_e.clone()),
                                        Icon { name: "rotate-ccw", size: 14 } "Put back the original"
                                    }
                                }
                                if !builtin {
                                    button { class: "btn ghost small danger",
                                        onclick: move |_| state.ask(Modal {
                                            title: format!("Delete {name_for_modal}?"),
                                            body: "Series using it keep their settings.".into(),
                                            confirm: "Delete".into(),
                                            cancel: "Keep".into(),
                                            danger: true,
                                            action: ModalAction::DeletePreset { id: id_f.clone() },
                                        }),
                                        Icon { name: "trash", size: 14 } "Delete"
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                p { class: "px-note", "Custom settings for this series. Pick a preset to start from." }
            }
            div { class: "px-buttons",
                button { class: "btn small", title: "Keep this series' settings as a preset of your own",
                    onclick: move |_| {
                        let result = state.edit_presets(|p| p.save_new(&new_name, settings_now.clone()));
                        match result {
                            Ok(id) => {
                                state.save_presets();
                                let mut library = state.library;
                                library.write().set_preset(&series_id, Some(id));
                                state.save_library();
                                state.notify("Preset saved. Rename it here.");
                            }
                            Err(e) => state.error(e.to_string()),
                        }
                    },
                    Icon { name: "plus", size: 14 } "New preset from these settings"
                }
            }
        }
    }
}

// ---- left: editing the series -----------------------------------------

#[component]
fn EditPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let p = state.preview.read().clone();
    let view = p.view;
    let chapter = p.chapter_key.clone().unwrap_or_default();
    let total = p.pages.len();
    let selected = p.selected.min(total.saturating_sub(1));
    let key = p
        .pages
        .get(selected)
        .map(|page| page.crop_key.clone())
        .unwrap_or_default();
    let saved = series.crops.scope_for(&chapter, &key);
    let draft = state.crop_draft;
    let id = series.id.clone();
    let (mode_id, mode_key, mode_chapter) = (id.clone(), key.clone(), chapter.clone());
    let preset_base = series
        .preset_id
        .as_ref()
        .and_then(|id| state.presets.read().find(id))
        .map(|e| e.preset.settings);
    let overridden = series.overridden.len();

    rsx! {
        Panel { title: "Edit", icon: "sliders-horizontal", id: "edit",
            DeviceSelect {}
            p { class: "px-note",
                if preset_base.is_none() { "Custom settings for this series." }
                else if overridden == 0 { "Following the preset." }
                else { "{overridden} settings changed from the preset." }
            }
            CropSection { identity: "{chapter}/{key}", draft, crop_key: key.clone(), saved }
            div { class: "px-label", style: "margin-top: 14px", "Layout and appearance apply to this series" }
            ConversionControls {
                value: series.settings.clone(),
                compact: true,
                shading_samples: p.shading_samples.clone(),
                manual_crop_active: series.crops.effective(&chapter, &key).unwrap_or_default() != CropRect::default() || view.editing_crop,
                on_manual_crop: move |_| {
                    let mut pv = state.preview;
                    let mut w = pv.write();
                    w.view.editing_crop = true;
                    w.view.show_original = true;
                    w.view.source_focus = true;
                },
                on_crop_mode: move |_| {
                    let mut library = state.library;
                    if let Some(series) = library.write().series.iter_mut().find(|s| s.id == mode_id) {
                        series.crops.set(library::CropScope::Page, &mode_chapter, &mode_key, Some(CropRect::default()));
                    }
                    let mut pv = state.preview;
                    let mut w = pv.write();
                    w.draft_crop = None;
                    w.view.editing_crop = false;
                    drop(w);
                    state.save_library();
                },
                onchange: move |v| {
                    let mut library = state.library;
                    library.write().set_settings(&id, v, preset_base.as_ref());
                    state.save_library();
                    schedule_preview(state, 180);
                },
            }
        }
    }
}

// ---- left: the series being exported ----------------------------------

#[component]
fn SeriesPanel(on_add: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    rsx! {
        Panel {
            title: "Series",
            icon: "book-open",
            id: "series",
            height: 150,
            actions: rsx! {
                button { class: "btn ghost small", title: "Add another series to this export", onclick: move |_| on_add.call(()),
                    Icon { name: "plus", size: 14 } "Add"
                }
            },
            div { class: "px-list",
                for (i, item) in plan.items.iter().enumerate() {
                    if let Some(series) = library.find(&item.series_id) {
                        {
                        let included = series.chapters.iter().filter(|c| !item.excluded.contains(&c.key)).count();
                        let has_cover = series.info.as_ref().is_some_and(|i| i.has_cover);
                        let on = i == plan.active;
                        let off = item.off;
                        rsx! {
                            div {
                                key: "{item.series_id}",
                                class: if on { "px-layer on" } else { "px-layer" },
                                onclick: move |_| { if !on { activate_export_series(state, i); } },
                                input {
                                    r#type: "checkbox", checked: !off, title: "Include this series in the export",
                                    onclick: move |e| e.stop_propagation(),
                                    onchange: move |e| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(i) { item.off = !e.checked(); }
                                    },
                                }
                                div { class: "px-layer-cover",
                                    if has_cover { Cover { key_id: saved_cover_key(&series.id) } } else { div { class: "cover empty", "📖" } }
                                }
                                div { class: "grow px-layer-text",
                                    strong { title: "{series.title}", "{series.title}" }
                                    span { class: "muted small", "{included} of {series.chapters.len()} chapters" }
                                }
                                button {
                                    class: "btn ghost small", title: "Take it out of this export",
                                    onclick: move |e| { e.stop_propagation(); remove_export_series(state, i); },
                                    Icon { name: "x", size: 14 }
                                }
                            }
                        }
                        }
                    }
                }
            }
        }
    }
}

// ---- right: which chapters --------------------------------------------

#[component]
fn ChaptersPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let active = state.export_plan.read().active;
    let excluded = state
        .export_plan
        .read()
        .items
        .get(active)
        .map(|i| i.excluded.clone())
        .unwrap_or_default();
    let included = series
        .chapters
        .iter()
        .filter(|c| !excluded.contains(&c.key))
        .count();
    let all_on = included == series.chapters.len();
    let all_keys: Vec<String> = series.chapters.iter().map(|c| c.key.clone()).collect();
    let shown = state.preview.read().chapter_key.clone();

    rsx! {
        Panel {
            title: "Chapters",
            icon: "list-checks",
            id: "chapters",
            actions: rsx! { span { class: "muted small", "{included} of {series.chapters.len()}" } },
            label { class: "px-all",
                input {
                    r#type: "checkbox", checked: all_on,
                    onchange: move |_| {
                        let mut plan = state.export_plan;
                        let mut guard = plan.write();
                        if let Some(item) = guard.items.get_mut(active) {
                            if all_on { item.excluded = all_keys.iter().cloned().collect(); } else { item.excluded.clear(); }
                        }
                    },
                }
                span { "Every chapter" }
            }
            div { class: "px-chapters",
                for c in series.chapters.iter() {
                    {
                        let key = c.key.clone();
                        let show_key = c.key.clone();
                        let on = !excluded.contains(&c.key);
                        let showing = shown.as_deref() == Some(c.key.as_str());
                        rsx! {
                            div { key: "{c.key}",
                                class: match (on, showing) {
                                    (true, true) => "px-chapter on showing",
                                    (true, false) => "px-chapter on",
                                    (false, true) => "px-chapter showing",
                                    _ => "px-chapter",
                                },
                                input {
                                    r#type: "checkbox", checked: on, title: "Include this chapter in the export",
                                    onchange: move |_| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(active) {
                                            if !item.excluded.remove(&key) { item.excluded.insert(key.clone()); }
                                        }
                                    },
                                }
                                button {
                                    class: "px-chapter-open", title: "Show this chapter in the preview",
                                    onclick: move |_| switch_chapter(state, show_key.clone()),
                                    span { class: "px-chapter-no", { c.number.map(logic::format_number).unwrap_or_else(|| "–".into()) } }
                                    span { class: "grow px-chapter-name", title: "{c.name}", "{c.name}" }
                                }
                                span { class: "muted small", "{logic::human_bytes(c.bytes)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- right: where it goes and how it is split ------------------------

#[component]
fn SendPanel() -> Element {
    let state = use_context::<AppState>();
    let mut drives = use_signal(removable_drives);
    let opts = state.export_opts.read().clone();
    let settings = state.settings.read().clone();
    let mut export_opts = state.export_opts;
    let folder = settings
        .export
        .output_dir
        .clone()
        .unwrap_or_else(|| crate::state::paths().default_export_dir());
    let folder_text = folder.display().to_string();
    let every_n_valid = opts.every_n.trim().parse::<usize>().is_ok_and(|n| n >= 1);

    rsx! {
        Panel { title: "Send to", icon: "file-down", id: "send", height: 330,
            div { class: "seg stack",
                button {
                    class: if matches!(opts.target, TargetChoice::Folder) { "on" } else { "" },
                    onclick: move |_| export_opts.write().target = TargetChoice::Folder,
                    Icon { name: "folder-open", size: 16 } "Folder on this computer"
                }
                for d in drives.read().iter() {
                    {
                        let mount = d.mount.display().to_string();
                        let on = opts.target == TargetChoice::Reader(mount.clone());
                        let label = format!("Reader / SD card ({mount})");
                        rsx! {
                            button {
                                key: "{mount}",
                                class: if on { "on" } else { "" },
                                onclick: { let mount = mount.clone(); move |_| export_opts.write().target = TargetChoice::Reader(mount.clone()) },
                                Icon { name: "hard-drive", size: 16 } "{label}"
                            }
                        }
                    }
                }
                if settings.export.webdav_url.as_deref().is_some_and(|u| !u.trim().is_empty()) {
                    button {
                        class: if matches!(opts.target, TargetChoice::WebDav) { "on" } else { "" },
                        onclick: move |_| export_opts.write().target = TargetChoice::WebDav,
                        Icon { name: "wifi", size: 16 } "Over Wi-Fi"
                    }
                }
            }
            if matches!(opts.target, TargetChoice::Folder) {
                div { class: "folder-pick", style: "margin-top: 10px",
                    span { class: "grow folder-path", title: "{folder_text}", "{folder_text}" }
                    button { class: "btn small",
                        onclick: move |_| {
                            let start = state.settings.peek().export.output_dir.clone();
                            spawn(async move {
                                if let Some(dir) = pick_folder("Where should the files go?", start).await {
                                    let mut s = state.settings;
                                    s.write().export.output_dir = Some(dir);
                                    state.save_settings();
                                }
                            });
                        },
                        "Choose folder…"
                    }
                }
            }
            button { class: "btn ghost small", style: "margin-top: 8px", onclick: move |_| drives.set(removable_drives()),
                Icon { name: "refresh-cw", size: 14 } "Look for my reader"
            }

            div { class: "px-label", style: "margin-top: 16px", "Files" }
            div { class: "seg stack",
                button {
                    class: if opts.grouping == GroupingChoice::Single { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::Single,
                    Icon { name: "book-open", size: 16 } "One file per series"
                }
                button {
                    class: if opts.grouping == GroupingChoice::PerChapter { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::PerChapter,
                    Icon { name: "layers", size: 16 } "One file per chapter"
                }
                button {
                    class: if opts.grouping == GroupingChoice::EveryN { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::EveryN,
                    Icon { name: "list-checks", size: 16 } "One file every N chapters"
                }
            }
            if opts.grouping == GroupingChoice::EveryN {
                div { class: "row", style: "margin-top: 8px",
                    span { class: "muted small", "Every" }
                    input {
                        r#type: "text", style: "width: 64px; text-align: center", value: "{opts.every_n}",
                        oninput: move |e| export_opts.write().every_n = e.value(),
                    }
                    span { class: "muted small", "chapters" }
                    if !every_n_valid { span { class: "small", style: "color: var(--bad)", "Type a number" } }
                }
            }
        }
    }
}

#[component]
fn ExportFooter() -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    let opts = state.export_opts.read().clone();
    let busy = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.result.is_none());
    let exporting = state
        .job
        .read()
        .as_ref()
        .is_some_and(|j| j.kind == JobKind::Export && j.result.is_none());

    let (mut series_count, mut chapters, mut pages, mut bytes) = (0usize, 0usize, 0usize, 0u64);
    for item in plan.items.iter().filter(|i| !i.off) {
        let Some(series) = library.find(&item.series_id).filter(|s| !s.is_sample()) else {
            continue;
        };
        let included: Vec<_> = series
            .chapters
            .iter()
            .filter(|c| !item.excluded.contains(&c.key))
            .collect();
        if included.is_empty() {
            continue;
        }
        series_count += 1;
        chapters += included.len();
        let p: usize = included.iter().map(|c| c.pages).sum();
        pages += p;
        bytes += logic::estimate_export_bytes(p, series.settings.format);
    }
    let every_n_valid = opts.every_n.trim().parse::<usize>().is_ok_and(|n| n >= 1);

    rsx! {
        div { class: "px-footer",
            div { class: "summary-line", span { class: "muted", "Series" } span { "{series_count}" } }
            div { class: "summary-line", span { class: "muted", "Chapters" } span { "{chapters}" } }
            div { class: "summary-line", span { class: "muted", "Pages" } span { "{pages}" } }
            div { class: "summary-line", span { class: "muted", "Size, about" } span { "{logic::human_bytes(bytes)}" } }
            button {
                class: "btn primary big", style: "margin-top: 10px",
                disabled: chapters == 0 || busy || (opts.grouping == GroupingChoice::EveryN && !every_n_valid),
                onclick: move |_| start_export_plan(state),
                if exporting { Icon { name: "loader-circle", size: 18 } "Exporting…" }
                else if busy { "Something else is running…" }
                else if chapters == 0 { "Nothing to export yet" }
                else { Icon { name: "file-down", size: 18 } "Export {plural(series_count, \"series\")}" }
            }
        }
    }
}

// ---- adding a series --------------------------------------------------

#[component]
fn AddSeries(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    let available: Vec<&DownloadedSeries> = library
        .series
        .iter()
        .filter(|s| !s.chapters.is_empty() && !s.is_sample())
        .filter(|s| !plan.items.iter().any(|i| i.series_id == s.id))
        .collect();
    rsx! {
        div { class: "modal-head",
            h2 { "Add a series to this export" }
            button { class: "btn ghost small", title: "Close", onclick: move |_| on_close.call(()), Icon { name: "x", size: 16 } }
        }
        if available.is_empty() {
            p { class: "muted", "Every downloaded series is already in this export." }
        }
        for s in available {
            {
                let id = s.id.clone();
                let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                rsx! {
                    button { key: "{s.id}", class: "px-add-row",
                        onclick: move |_| add_export_series(state, id.clone()),
                        div { class: "px-layer-cover",
                            if has_cover { Cover { key_id: saved_cover_key(&s.id) } } else { div { class: "cover empty", "📖" } }
                        }
                        div { class: "grow px-layer-text",
                            strong { "{s.title}" }
                            span { class: "muted small", "{plural(s.chapters.len(), \"chapter\")} · {logic::human_bytes(s.bytes())}" }
                        }
                        Icon { name: "plus", size: 16 }
                    }
                }
            }
        }
        div { class: "actions",
            button { class: "btn primary", onclick: move |_| on_close.call(()), "Done" }
        }
    }
}

/// The crop controls; they start over for each page.
#[component]
fn CropSection(
    identity: String,
    draft: Signal<CropRect>,
    crop_key: String,
    saved: Option<library::CropScope>,
) -> Element {
    rsx! { CropInspector { key: "{identity}", draft, crop_key, saved } }
}

/// The name of one of your presets, edited in place.
#[component]
fn RenameBox(identity: String, preset_id: String, name: String, nonce: Signal<u32>) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { key: "{identity}",
            label { class: "px-label", "Preset name" }
            input {
                class: "px-input", r#type: "text", maxlength: "40", value: "{name}",
                onchange: move |e| {
                    let result = state.edit_presets(|p| p.rename(&preset_id, e.value().trim()));
                    match result {
                        Ok(()) => state.save_presets(),
                        Err(err) => {
                            state.error(err.to_string());
                            let mut n = nonce;
                            n.set(nonce() + 1);
                        }
                    }
                },
            }
        }
    }
}
