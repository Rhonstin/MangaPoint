//! Crop interaction is a local draft; Apply saves an explicit scope.
use crate::state::{AppState, Shot};
use crate::ui::widgets::Icon;
use crate::ui::work::schedule_preview;
use dioxus::prelude::*;
use library::{CropRect, CropScope};

pub fn update_draft(mut state: AppState, mut draft: Signal<CropRect>, key: &str, crop: CropRect) {
    let crop = crop.normalized();
    draft.set(crop);
    state.preview.write().draft_crop = Some((key.into(), crop));
    schedule_preview(state, 180);
}

pub fn save_crop(mut state: AppState, scope: CropScope, key: &str, crop: Option<CropRect>) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let Some(chapter) = state.preview.peek().chapter_key.clone() else {
        return;
    };
    if let Some(series) = state
        .library
        .write()
        .series
        .iter_mut()
        .find(|s| s.id == ws.series_id)
    {
        series.crops.set(scope, &chapter, key, crop);
    }
    state.preview.write().draft_crop = None;
    state.save_library();
    schedule_preview(state, 0);
}

fn saved_crop(state: AppState, key: &str) -> CropRect {
    let ws = state.workspace.peek();
    let preview = state.preview.peek();
    let lib = state.library.peek();
    ws.as_ref()
        .and_then(|ws| lib.find(&ws.series_id))
        .and_then(|series| {
            preview
                .chapter_key
                .as_ref()
                .and_then(|chapter| series.crops.effective(chapter, key))
        })
        .unwrap_or_default()
}

// Capture pointers on the image itself. Geometry comes from its actual
// letterboxed content bounds, not the viewport or the tiny handle bounds.
const DRAG_SCRIPT: &str = r#"
const root = document.getElementById('editor-source-art');
if (root && !root.dataset.cropBound) {
  root.dataset.cropBound='1';
  let drag=null;
  root.addEventListener('pointerdown', e => {
    const handle=e.target.closest('[data-crop-handle]');
    if (!handle || !root.classList.contains('is-editing')) return;
    const input=document.getElementById('editor-crop-draft');
    const edges=input.value.split(',').map(Number);
    drag={edge:handle.dataset.cropHandle, x:e.clientX, y:e.clientY, bounds:root.getBoundingClientRect(), edges};
    root.setPointerCapture(e.pointerId); e.preventDefault();
  });
  root.addEventListener('pointermove', e => {
    if (!drag) return;
    const dx=(e.clientX-drag.x)/drag.bounds.width, dy=(e.clientY-drag.y)/drag.bounds.height;
    let [l,t,r,b]=drag.edges; r=1-r; b=1-b;
    if (drag.edge==='move') {
      const mx=Math.max(-l,Math.min(1-r,dx)), my=Math.max(-t,Math.min(1-b,dy));
      l+=mx; r+=mx; t+=my; b+=my;
    } else {
      if (drag.edge.includes('w')) l=Math.max(0,Math.min(r-.01,l+dx));
      if (drag.edge.includes('e')) r=Math.min(1,Math.max(l+.01,r+dx));
      if (drag.edge.includes('n')) t=Math.max(0,Math.min(b-.01,t+dy));
      if (drag.edge.includes('s')) b=Math.min(1,Math.max(t+.01,b+dy));
    }
    const box=root.querySelector('.editor-crop-box');
    Object.assign(box.style,{left:(l*100)+'%',top:(t*100)+'%',width:((r-l)*100)+'%',height:((b-t)*100)+'%'});
    const input=document.getElementById('editor-crop-draft');
    const value=[l,t,1-r,1-b].join(',');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,value);
    input.dispatchEvent(new Event('input',{bubbles:true}));
  });
  const stop=()=>{drag=null;};
  root.addEventListener('pointerup',stop); root.addEventListener('pointercancel',stop);
}
"#;

#[component]
pub fn SourceCanvas(
    source: Shot,
    draft: Signal<CropRect>,
    crop_key: String,
    editing: bool,
    guide: bool,
) -> Element {
    let state = use_context::<AppState>();
    let c = draft.read().normalized();
    let image_style = format!(
        "aspect-ratio: {} / {}; --source-aspect: {};",
        source.width,
        source.height,
        source.width as f64 / source.height.max(1) as f64
    );
    let box_style = format!(
        "left:{}%;top:{}%;width:{}%;height:{}%;",
        c.left * 100.0,
        c.top * 100.0,
        (1.0 - c.left - c.right) * 100.0,
        (1.0 - c.top - c.bottom) * 100.0
    );
    let class = if editing {
        "editor-source-art is-editing"
    } else {
        "editor-source-art"
    };
    let csv = format!("{},{},{},{}", c.left, c.top, c.right, c.bottom);
    rsx! {
        div { class: "editor-source-scroll",
            div { id: "editor-source-art", class, style: "{image_style}",
                onmounted: move |_| { spawn(async move { let _ = document::eval(DRAG_SCRIPT).await; }); },
                img { src: source.url.clone(), alt: "Source artwork before manual crop and reader shading", draggable: "false" }
                if guide || editing {
                    div { class: "editor-crop-box", style: "{box_style}", "data-crop-handle": "move",
                        if editing {
                            div { class: "editor-crop-grid" }
                            for edge in ["nw","n","ne","e","se","s","sw","w"] {
                                span { key: "{edge}", class: "crop-handle handle-{edge}", "data-crop-handle": edge, "aria-hidden": "true" }
                            }
                        }
                    }
                }
                input { id: "editor-crop-draft", r#type: "hidden", value: "{csv}",
                    oninput: move |e| {
                        let values: Vec<f32> = e.value().split(',').filter_map(|s| s.parse().ok()).collect();
                        if let [left,top,right,bottom] = values.as_slice() {
                            update_draft(state,draft,&crop_key,CropRect { left:*left,top:*top,right:*right,bottom:*bottom });
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn CropInspector(
    draft: Signal<CropRect>,
    crop_key: String,
    saved: Option<CropScope>,
) -> Element {
    let mut state = use_context::<AppState>();
    let mut draft = draft;
    let mut scope = use_signal(|| saved.unwrap_or_default());
    let c = *draft.read();
    let editing = state.preview.read().view.editing_crop;
    let inherited = match saved {
        Some(CropScope::Page) => "Saved on this page",
        Some(CropScope::Chapter) => "Inherited from this chapter",
        Some(CropScope::Series) => "Inherited from this series",
        None => "No manual crop saved",
    };
    let is_draft = state.preview.read().draft_crop.is_some();
    rsx! {
        section { class: "editor-crop-inspector",
            div { class: "editor-section-heading",
                strong { Icon { name: "crop", size: 15 } "Manual crop" }
                button { class: if editing {"btn small on"} else {"btn small"},
                    onclick: move |_| { let mut p=state.preview; let mut p=p.write(); p.view.editing_crop=!p.view.editing_crop; p.view.show_original=true; p.view.source_focus=p.view.editing_crop; },
                    if editing { "Editing" } else { "Edit crop" }
                }
            }
            p { class: "editor-caption", "{inherited}" if is_draft { " · unsaved changes" } }
            if editing || saved.is_some() || is_draft {
                div { class: "editor-crop-numbers",
                    for (name,value) in [("Left",c.left),("Top",c.top),("Right",c.right),("Bottom",c.bottom)] {
                        label { key:"{name}", span { "{name} %" }
                            input { r#type:"number", min:"0",max:"99",step:"0.1",value:"{value*100.0:.2}",
                                oninput: { let key=crop_key.clone(); move |e| {
                                    if let Ok(percent)=e.value().parse::<f32>() {
                                        let mut next=*draft.peek();
                                        match name { "Left"=>next.left=percent/100.0,"Top"=>next.top=percent/100.0,"Right"=>next.right=percent/100.0,_=>next.bottom=percent/100.0 }
                                        update_draft(state,draft,&key,next);
                                    }
                                } }
                            }
                        }
                    }
                }
                label { class:"editor-label", "Apply to" }
                div { class:"editor-scope", role:"group", "aria-label":"Manual crop scope",
                    for (choice,label) in [(CropScope::Page,"This page"),(CropScope::Chapter,"Chapter"),(CropScope::Series,"Series")] {
                        button { key:"{label}", class:if *scope.read()==choice {"selected"}else{""},"aria-pressed":(*scope.read()==choice).to_string(), onclick:move |_|scope.set(choice), "{label}" }
                    }
                }
                p { class:"editor-caption", match *scope.read() {CropScope::Page=>"Only this source section changes.",CropScope::Chapter=>"All reader pages in this chapter. Page overrides stay in place.",CropScope::Series=>"All chapters in this series. Narrower overrides stay in place."} }
                div { class:"editor-crop-actions",
                    button { class:"btn primary small", onclick:{let key=crop_key.clone();move |_| {save_crop(state,*scope.peek(),&key,Some(*draft.peek()));draft.set(saved_crop(state,&key));state.notify("Crop saved. The export uses these same settings.");}}, "Apply crop" }
                    button { class:"btn ghost small", onclick:{let key=crop_key.clone();move |_| {save_crop(state,*scope.peek(),&key,None);draft.set(saved_crop(state,&key));}}, "Reset scope" }
                    if is_draft {
                        button { class:"btn ghost small",onclick:{let key=crop_key.clone();move |_| {draft.set(saved_crop(state,&key));state.preview.write().draft_crop=None;schedule_preview(state,0);}}, "Cancel" }
                    }
                }
            }
        }
    }
}
