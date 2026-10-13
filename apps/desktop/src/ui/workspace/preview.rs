//! The page editor: every page of a chapter in one continuous scroll, like a
//! PDF viewer, with the original beside each converted page.
use super::crop_editor::{CropInspector, SourceCanvas};
use crate::state::{AppState, SeriesTab, Shot, Zoom};
use crate::ui::conversion::ConversionControls;
use crate::ui::devices::DeviceSelect;
use crate::ui::presets::{apply_to_series, PresetPicker};
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{prepare_preview, schedule_preview, select_preview_page};
use converter::plan::Source;
use dioxus::prelude::*;
use library::CropRect;

/// What the page after this one repeats of it: how many rows, and where those
/// rows sit on the page (as shares of the screen height).
#[derive(Clone, PartialEq)]
struct Seam {
    rows: u32,
    band_top: f32,
    band_height: f32,
}

/// The seam after each page of a strip chapter; `None` for the last page and
/// for chapters that are not strips.
fn seams(
    pages: &[std::rc::Rc<crate::state::PreviewPage>],
    screen_height: u32,
) -> Vec<Option<Seam>> {
    let rows_of = |page: &crate::state::PreviewPage| match page.source.as_ref().map(|p| p.source) {
        Some(Source::Strip { top, height }) => Some((top, height)),
        _ => None,
    };
    (0..pages.len())
        .map(|i| {
            let (top, height) = rows_of(&pages[i])?;
            let (next_top, _) = rows_of(pages.get(i + 1)?)?;
            let rows = (top + height).saturating_sub(next_top);
            let screen = screen_height.max(1) as f32;
            Some(Seam {
                rows,
                band_top: height.saturating_sub(rows) as f32 / screen * 100.0,
                band_height: rows as f32 / screen * 100.0,
            })
        })
        .collect()
}

/// Sizes the viewer to the window, reports which pages are on screen (as
/// `[first, last, current]`, `-1` when there are none), and wires the keys.
const EDITOR_SCRIPT: &str = r#"
let tries = 0;
const start = () => {
 const root = document.getElementById('page-editor');
 if (!root) { if (tries++ < 120) requestAnimationFrame(start); return; }
 if (root.dataset.bound) return;
 root.dataset.bound = '1';
 const scroller = root.querySelector('.editor-scroll');
 let frame = 0, last = '';
 const vars = {};
 const set = (name, value) => { if (vars[name] !== value) { vars[name] = value; root.style.setProperty(name, value); } };
 // The viewer is as tall as the window allows, and each page as tall as the
 // viewer, so a page fits the screen at any window size.
 const fit = () => {
  const top = scroller.getBoundingClientRect().top;
  set('--canvas-height', Math.max(220, Math.round(window.innerHeight - top - 40)) + 'px');
  const style = getComputedStyle(scroller);
  const room = scroller.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom) - 96;
  set('--reader-page-height', Math.max(120, Math.round(room)) + 'px');
  const pane = root.querySelector('.row-pane.result-pane');
  if (pane && pane.clientWidth > 0) set('--reader-page-width', pane.clientWidth + 'px');
 };
 const report = () => {
  const box = scroller.getBoundingClientRect();
  const probe = box.top + box.height * 0.3;
  let lo = -1, hi = -1, cur = -1;
  for (const row of scroller.querySelectorAll('.editor-row')) {
   const b = row.getBoundingClientRect(), i = Number(row.dataset.row);
   if (b.bottom >= box.top - box.height * 0.5 && b.top <= box.bottom + box.height * 0.5) {
    if (lo < 0) lo = i;
    hi = i;
   }
   if (cur < 0 && b.bottom > probe) cur = i;
  }
  const message = lo + ',' + hi + ',' + cur;
  if (message !== last) { last = message; dioxus.send([lo, hi, cur]); }
 };
 const run = () => { frame = 0; if (root.isConnected) { fit(); report(); } };
 const later = () => { if (!frame) frame = requestAnimationFrame(run); };
 scroller.addEventListener('scroll', later, { passive: true });
 window.addEventListener('resize', later);
 new ResizeObserver(later).observe(scroller);
 new ResizeObserver(later).observe(root.querySelector('.editor-toolbar'));
 new MutationObserver(later).observe(scroller, { childList: true, subtree: true });
 // Things above the viewer (the series header, a cover) can still be loading.
 const timer = setInterval(() => { if (!root.isConnected) clearInterval(timer); else later(); }, 400);
 root.addEventListener('keydown', e => {
  if (e.target.closest('input,select,textarea,[contenteditable=true]')) return;
  const ids = { ArrowRight: 'editor-next', ArrowLeft: 'editor-previous', c: 'editor-crop-toggle', Escape: 'editor-close-overview' };
  const id = ids[e.key];
  if (id) { const b = document.getElementById(id); if (b && !b.disabled) { e.preventDefault(); b.click(); } }
 });
 later();
};
start();
// Stay alive: messages can only be sent while the script has not finished.
await new Promise(() => {});
"#;

/// How tall a page is at this zoom, as a CSS length.
fn row_height(zoom: Zoom, width: u32, height: u32) -> String {
    match zoom {
        Zoom::Fit => format!(
            "min(var(--reader-page-height),calc(var(--reader-page-width) * {height} / {width}))"
        ),
        Zoom::Width => format!("calc(var(--reader-page-width) * {height} / {width})"),
        Zoom::Actual => format!("{height}px"),
        Zoom::Double => format!("{}px", height * 2),
    }
}

fn reader_style(zoom: Zoom, width: u32, height: u32) -> String {
    let size = match zoom {
        Zoom::Fit => format!("height:{};width:auto;", row_height(zoom, width, height)),
        Zoom::Width => "height:auto;width:100%;".to_string(),
        Zoom::Actual => format!("height:{height}px;width:{width}px;"),
        Zoom::Double => format!("height:{}px;width:{}px;", height * 2, width * 2),
    };
    format!("{size}aspect-ratio:{width}/{height};")
}

/// One page of the chapter in the scrolling list: the original on the left
/// (when comparing) and the page as the reader will show it on the right.
#[component]
fn EditorRow(
    index: usize,
    selected: bool,
    result: Option<Shot>,
    source: Option<Shot>,
    compare: bool,
    editing: bool,
    guide: bool,
    row_style: String,
    frame_style: String,
    seam: Option<Seam>,
    chapter: String,
    crop_key: String,
    draft: Signal<CropRect>,
) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        article {
            class: if selected { "editor-row selected" } else { "editor-row" },
            "data-row": "{index}",
            style: "{row_style}",
            onclick: move |_| {
                if !selected {
                    let mut preview = state.preview;
                    let mut p = preview.write();
                    p.selected = index;
                    p.draft_crop = None;
                }
            },
            div { class: "editor-row-label", "Page {index + 1}" }
            div { class: if compare { "editor-row-panes comparing" } else { "editor-row-panes" },
                if compare {
                    div { class: "row-pane source-pane",
                        if let Some(source) = source {
                            if selected {
                                SourceCanvas {
                                    key: "{chapter}/{crop_key}",
                                    source: source.clone(),
                                    draft,
                                    crop_key: crop_key.clone(),
                                    editing,
                                    guide,
                                }
                            } else {
                                div { class: "editor-source-scroll",
                                    div {
                                        class: "editor-source-art",
                                        style: "aspect-ratio: {source.width} / {source.height}; --source-aspect: {source.width as f64 / source.height.max(1) as f64};",
                                        img { src: "{source.url}", alt: "Source artwork of page {index + 1}", draggable: "false" }
                                    }
                                }
                            }
                        } else {
                            div { class: "editor-image-loading", style: "{frame_style}", "Preparing source…" }
                        }
                    }
                }
                div { class: "row-pane result-pane",
                    div { class: "editor-reader-frame", style: "{frame_style}",
                        if let Some(shot) = result {
                            img { src: "{shot.url}", alt: "Page {index + 1} exactly as exported to the reader", draggable: "false" }
                        } else {
                            div { class: "editor-image-loading", "Drawing page…" }
                        }
                        if let Some(seam) = seam.as_ref().filter(|s| s.rows > 0) {
                            div {
                                class: "editor-seam-band",
                                style: "top: {seam.band_top}%; height: {seam.band_height}%;",
                                title: "The next page starts with these rows again",
                                span { "Repeated on the next page" }
                            }
                        }
                    }
                }
            }
            if let Some(seam) = seam {
                div { class: if seam.rows > 0 { "editor-seam-note repeats" } else { "editor-seam-note clean" },
                    if seam.rows > 0 {
                        Icon { name: "layers", size: 13 }
                        "Next page repeats these last {seam.rows} rows"
                    } else {
                        Icon { name: "check", size: 13 }
                        "Clean cut: the next page starts right below"
                    }
                }
            }
        }
    }
}

/// The pages now on screen changed: draw them, and follow the page at the top.
fn track_scroll(state: AppState, first: i64, last: i64, current: i64) {
    if first < 0 || last < 0 {
        return;
    }
    let (window, selected) = ((first as usize, last as usize), current.max(0) as usize);
    let (window_changed, page_changed) = {
        let mut preview = state.preview;
        let mut p = preview.write();
        let total = p.pages.len();
        if total == 0 {
            return;
        }
        let window = (window.0.min(total - 1), window.1.min(total - 1));
        let window_changed = p.window != window;
        p.window = window;
        // While a crop is being dragged, the page stays where it is.
        let selected = selected.min(total - 1);
        let page_changed = !p.view.editing_crop && p.selected != selected;
        if page_changed {
            p.selected = selected;
            p.draft_crop = None;
        }
        (window_changed, page_changed)
    };
    if page_changed {
        let selected = state.preview.peek().selected;
        spawn(async move {
            let js = format!("document.querySelector('[data-editor-page=\"{selected}\"]')?.scrollIntoView({{block:'nearest'}})");
            let _ = document::eval(&js).await;
        });
    }
    if window_changed || page_changed {
        schedule_preview(state, 120);
    }
}

#[component]
pub fn PreviewTab() -> Element {
    let mut state = use_context::<AppState>();
    let mut draft = use_signal(CropRect::default);
    let mut identity = use_signal(String::new);
    use_effect(move || prepare_preview(state));
    use_future(move || async move {
        let mut eval = document::eval(EDITOR_SCRIPT);
        while let Ok(message) = eval.recv::<Vec<i64>>().await {
            if let [first, last, current] = message.as_slice() {
                track_scroll(state, *first, *last, *current);
            }
        }
    });
    use_effect(move || {
        let p = state.preview.read();
        let ws = state.workspace.read();
        let lib = state.library.read();
        if let (Some(ws), Some(chapter), Some(page)) =
            (ws.as_ref(), p.chapter_key.as_ref(), p.pages.get(p.selected))
        {
            if let Some(series) = lib.find(&ws.series_id) {
                let crop = series
                    .crops
                    .effective(chapter, &page.crop_key)
                    .unwrap_or_default();
                let next = format!("{}/{}/{}:{crop:?}", ws.series_id, chapter, page.crop_key);
                if *identity.peek() != next {
                    identity.set(next);
                    draft.set(crop);
                }
            }
        }
    });
    let Some(ws) = state.workspace.read().clone() else {
        return rsx! {};
    };
    let Some(series) = state.library.read().find(&ws.series_id).cloned() else {
        return rsx! {EmptyState{title:"Nothing to edit yet",message:"Download a chapter to open it in the editor."}};
    };
    let p = state.preview.read().clone();
    let view = p.view;
    let chapter = p.chapter_key.clone().unwrap_or_default();
    let total = p.pages.len();
    let selected = p.selected.min(total.saturating_sub(1));
    let number = if total == 0 { 0 } else { selected + 1 };
    let active = p.pages.get(selected).cloned();
    let key = active
        .as_ref()
        .map(|p| p.crop_key.clone())
        .unwrap_or_default();
    let saved = series.crops.scope_for(&chapter, &key);
    let device = state.settings.read().device_profile();
    let frame_style = reader_style(view.zoom, device.width, device.height);
    let row_style = format!(
        "--row-h:{};",
        row_height(view.zoom, device.width, device.height)
    );
    let id = series.id.clone();
    let pick_id = id.clone();
    let save_id = id.clone();
    let mode_id = id.clone();
    let mode_key = key.clone();
    let mode_chapter = chapter.clone();
    let preset_base = series
        .preset_id
        .as_ref()
        .and_then(|id| state.presets.read().find(id))
        .map(|e| e.preset.settings);
    let classes = format!(
        "page-editor{}{}",
        if view.focus { " inspector-hidden" } else { "" },
        if view.source_focus {
            " source-focused"
        } else {
            ""
        }
    );
    let compare = view.show_original;
    let seams = seams(&p.pages, device.height);
    rsx! {
        div { id: "page-editor", class: "{classes}", tabindex: "0",
            div { class: "editor-toolbar",
                div { class: "editor-chapter-picker",
                    label { r#for: "editor-chapter", "Chapter" }
                    select {
                        id: "editor-chapter", value: "{chapter}",
                        onchange: move |e| {
                            let mut pv = state.preview;
                            let mut w = pv.write();
                            w.chapter_key = Some(e.value());
                            w.selected = 0;
                            w.thumb_start = 0;
                            w.window = (0, 0);
                            w.pages.clear();
                            w.draft_crop = None;
                            w.shading_samples.clear();
                            drop(w);
                            spawn(async move {
                                let _ = document::eval("document.querySelector('.editor-scroll')?.scrollTo(0,0)").await;
                            });
                            schedule_preview(state, 0);
                        },
                        for c in &series.chapters {
                            option { key: "{c.key}", value: "{c.key}", selected: c.key == chapter, "{c.name}" }
                        }
                    }
                }
                div { class: "editor-page-nav",
                    button {
                        id: "editor-previous", class: "btn small",
                        disabled: selected == 0 || total == 0, title: "Previous page · Left arrow",
                        onclick: move |_| select_preview_page(state, selected.saturating_sub(1)),
                        Icon { name: "arrow-left", size: 15 }
                    }
                    span { class: "editor-page-count", "Page " strong { "{number}" } " / {total}" }
                    button {
                        id: "editor-next", class: "btn small",
                        disabled: selected + 1 >= total, title: "Next page · Right arrow",
                        onclick: move |_| select_preview_page(state, selected + 1),
                        span { class: "turn-arrow", Icon { name: "arrow-left", size: 15 } }
                    }
                }
                div { class: "editor-toolbar-actions",
                    button {
                        class: if compare { "btn small on" } else { "btn small" },
                        title: "Show the downloaded artwork beside each reader page",
                        onclick: move |_| {
                            state.preview.write().view.show_original = !compare;
                            schedule_preview(state, 0);
                        },
                        Icon { name: "eye", size: 14 } "Compare"
                    }
                    button {
                        id: "editor-crop-toggle",
                        class: if view.editing_crop { "btn small on" } else { "btn small" },
                        disabled: active.as_ref().and_then(|p| p.original.as_ref()).is_none(),
                        title: "Drag crop handles · C",
                        onclick: move |_| {
                            let mut pv = state.preview;
                            let mut w = pv.write();
                            w.view.editing_crop = !view.editing_crop;
                            w.view.show_original = true;
                            w.view.source_focus = !view.editing_crop;
                        },
                        Icon { name: "crop", size: 14 } "Crop"
                    }
                    button {
                        class: if view.show_crop { "btn small on" } else { "btn small" },
                        title: "Show excluded artwork without editing",
                        onclick: move |_| { state.preview.write().view.show_crop = !view.show_crop; },
                        "Guide"
                    }
                    select {
                        class: "editor-zoom", "aria-label": "Preview zoom",
                        onchange: move |e| {
                            state.preview.write().view.zoom = match e.value().as_str() {
                                "width" => Zoom::Width,
                                "actual" => Zoom::Actual,
                                "double" => Zoom::Double,
                                _ => Zoom::Fit,
                            };
                        },
                        option { value: "fit", selected: view.zoom == Zoom::Fit, "Fit page" }
                        option { value: "width", selected: view.zoom == Zoom::Width, "Fit width" }
                        option { value: "actual", selected: view.zoom == Zoom::Actual, "100%" }
                        option { value: "double", selected: view.zoom == Zoom::Double, "200%" }
                    }
                    button {
                        class: "btn small",
                        onclick: move |_| {
                            state.preview.write().overview = true;
                            schedule_preview(state, 0);
                        },
                        "Overview"
                    }
                    button {
                        class: if view.focus { "btn small" } else { "btn small on" },
                        title: "Show or hide properties",
                        onclick: move |_| { state.preview.write().view.focus = !view.focus; },
                        Icon { name: "settings", size: 15 }
                    }
                }
            }
            if let Some(error) = &p.error { div { class: "banner bad small", "{error}" } }
            div { class: "editor-body",
                nav {
                    class: "editor-thumbnails", "aria-label": "Reader pages",
                    onscroll: move |e| {
                        let start = (e.data.scroll_top() / 132.0).floor().max(0.0) as usize;
                        let needs = {
                            let mut pv = state.preview;
                            let mut w = pv.write();
                            if w.thumb_start != start { w.thumb_start = start; true } else { false }
                        };
                        if needs && !state.preview.peek().overview { schedule_preview(state, 80); }
                    },
                    for (i, page) in p.pages.iter().enumerate() {
                        button {
                            key: "{i}",
                            class: if i == selected { "editor-thumb selected" } else { "editor-thumb" },
                            "data-editor-page": "{i}",
                            "aria-label": "Open page {i + 1}",
                            "aria-current": if i == selected { "page" } else { "false" },
                            onclick: move |_| select_preview_page(state, i),
                            div { class: "editor-thumb-art",
                                if let Some(shot) = page.thumbnail.as_ref().or(page.shot.as_ref()) {
                                    img { src: "{shot.url}", alt: "Reader page {i + 1}" }
                                } else {
                                    Icon { name: "file-image", size: 23 }
                                }
                            }
                            span { "{i + 1}" }
                        }
                    }
                }
                main { class: "editor-main",
                    div { class: "editor-mobile-switch",
                        button {
                            class: if !view.source_focus { "selected" } else { "" },
                            onclick: move |_| { state.preview.write().view.source_focus = false; },
                            "Reader result"
                        }
                        button {
                            class: if view.source_focus { "selected" } else { "" },
                            onclick: move |_| {
                                let mut pv = state.preview;
                                let mut w = pv.write();
                                w.view.source_focus = true;
                                w.view.show_original = true;
                            },
                            "Source"
                        }
                    }
                    div { class: "editor-scroll",
                        div { class: if compare { "editor-columns comparing" } else { "editor-columns" },
                            if compare {
                                span { class: "source-pane",
                                    if p.strip { "Source section · joined strip" } else { "Source artwork" }
                                    if view.editing_crop { " · drag the edges to crop" }
                                }
                            }
                            span { class: "result-pane", "Reader result · {device.name} · {device.width} × {device.height}" }
                        }
                        if total == 0 && p.error.is_none() {
                            div { class: "editor-image-loading", "Preparing the chapter…" }
                        }
                        for (i, page) in p.pages.iter().enumerate() {
                            EditorRow {
                                key: "{chapter}/{i}",
                                index: i,
                                selected: i == selected,
                                result: page.shot.clone(),
                                source: page.original.clone(),
                                compare,
                                editing: view.editing_crop && i == selected,
                                guide: view.show_crop,
                                row_style: row_style.clone(),
                                frame_style: frame_style.clone(),
                                seam: seams.get(i).cloned().flatten(),
                                chapter: chapter.clone(),
                                crop_key: page.crop_key.clone(),
                                draft,
                            }
                        }
                    }
                    div { class: "editor-status",
                        span {
                            if p.busy {
                                span { class: "spin", Icon { name: "loader-circle", size: 13 } }
                                " Updating visible pages…"
                            } else {
                                Icon { name: "check", size: 13 }
                                " Preview ready"
                            }
                        }
                        span {
                            if p.strip && total > 0 { "Section {number} of {total} of the joined strip · " }
                            if p.draft_crop.is_some() { "Unsaved crop · Apply to include it in the export" } else { "Preview and export use the same saved settings" }
                        }
                    }
                }
                if !view.focus {
                    aside { class: "editor-inspector",
                        div { class: "editor-inspector-top",
                            PresetPicker {
                                value: series.settings.clone(), preset_id: series.preset_id.clone(),
                                on_pick: move |(id, settings)| apply_to_series(state, &pick_id, Some(id), settings),
                                on_saved: move |id| { state.library.write().set_preset(&save_id, Some(id)); state.save_library(); },
                            }
                            DeviceSelect {}
                            p { class: "editor-caption",
                                if preset_base.is_none() { "Custom settings for this series" }
                                else if series.overridden.is_empty() { "Following preset settings" }
                                else { "{series.overridden.len()} settings changed from preset" }
                            }
                        }
                        CropInspector { key: "{chapter}/{key}", draft, crop_key: key.clone(), saved }
                        div { class: "editor-settings-scope", "Layout and appearance apply to this series" }
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
                                if let Some(series) = state.library.write().series.iter_mut().find(|s| s.id == mode_id) {
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
                                state.library.write().set_settings(&id, v, preset_base.as_ref());
                                state.save_library();
                                schedule_preview(state, 180);
                            },
                        }
                        button { class: "btn primary editor-export", onclick: move |_| state.open_tab(SeriesTab::Export),
                            "Continue to export"
                            span { class: "turn-arrow", Icon { name: "arrow-left", size: 15 } }
                        }
                    }
                }
            }
            if p.overview {
                div { class: "editor-overview-backdrop",
                    section { class: "editor-overview", role: "dialog", "aria-modal": "true", "aria-label": "Chapter page overview",
                        header {
                            strong { "Chapter overview · {total} reader pages" }
                            button { id: "editor-close-overview", class: "btn small", onclick: move |_| { state.preview.write().overview = false; }, Icon { name: "x", size: 16 } }
                        }
                        div { class: "editor-overview-grid",
                            for (i, page) in p.pages.iter().enumerate() {
                                button {
                                    key: "{i}",
                                    class: if i == selected { "selected" } else { "" },
                                    onclick: move |_| select_preview_page(state, i),
                                    div { class: "editor-overview-art",
                                        if let Some(shot) = page.thumbnail.as_ref().or(page.shot.as_ref()) {
                                            img { src: "{shot.url}", alt: "Reader page {i + 1}" }
                                        } else {
                                            Icon { name: "file-image", size: 24 }
                                        }
                                    }
                                    span { "Page {i + 1}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
