//! Focused page editing with independent source and reader viewports.
use super::crop_editor::{CropInspector, SourceCanvas};
use crate::state::{AppState, SeriesTab, Zoom};
use crate::ui::conversion::ConversionControls;
use crate::ui::devices::DeviceSelect;
use crate::ui::presets::{apply_to_series, PresetPicker};
use crate::ui::widgets::{EmptyState, Icon};
use crate::ui::work::{prepare_preview, schedule_preview, select_preview_page};
use dioxus::prelude::*;
use library::CropRect;

const EDITOR_SCRIPT: &str = r#"
const root=document.getElementById('page-editor');
if(root&&!root.dataset.bound){
 root.dataset.bound='1';const canvas=root.querySelector('.editor-canvas');
 const fit=()=>{if(!root.isConnected){window.removeEventListener('resize',fit);observer.disconnect();return;}
 root.style.setProperty('--canvas-height',Math.max(220,window.innerHeight-canvas.getBoundingClientRect().top-40)+'px');
 const viewport=root.querySelector('.editor-result-scroll');const style=getComputedStyle(viewport);
 if(viewport.clientHeight>0){root.style.setProperty('--reader-page-height',Math.max(80,viewport.clientHeight-parseFloat(style.paddingTop)-parseFloat(style.paddingBottom)-2)+'px');root.style.setProperty('--reader-page-width',Math.max(40,viewport.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight)-2)+'px');}};
 const observer=new ResizeObserver(()=>requestAnimationFrame(fit));observer.observe(root.querySelector('.editor-toolbar'));observer.observe(root.querySelector('.editor-result-scroll'));
 window.addEventListener('resize',fit);requestAnimationFrame(fit);
 root.addEventListener('keydown',e=>{
  if(e.target.closest('input,select,textarea,[contenteditable=true]'))return;
  const ids={ArrowRight:'editor-next',ArrowDown:'editor-next',ArrowLeft:'editor-previous',ArrowUp:'editor-previous',c:'editor-crop-toggle',Escape:'editor-close-overview'};
  const id=ids[e.key];if(id){const b=document.getElementById(id);if(b&&!b.disabled){e.preventDefault();b.click();}}
 });
}
"#;

fn reader_style(zoom: Zoom, width: u32, height: u32) -> String {
    let size=match zoom {
        Zoom::Fit=>format!("height:min(var(--reader-page-height),calc(var(--reader-page-width) * {height} / {width}));width:auto;"),
        Zoom::Width=>"height:auto;width:100%;".to_string(),
        Zoom::Actual=>format!("height:{height}px;width:{width}px;"),
        Zoom::Double=>format!("height:{}px;width:{}px;",height*2,width*2),
    };
    format!("{size}aspect-ratio:{width}/{height};")
}

#[component]
pub fn PreviewTab() -> Element {
    let mut state = use_context::<AppState>();
    let mut draft = use_signal(CropRect::default);
    let mut identity = use_signal(String::new);
    use_effect(move || prepare_preview(state));
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
    let reader_size = reader_style(view.zoom, device.width, device.height);
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
    rsx! {
        div{id:"page-editor",class:"{classes}",tabindex:"0",onmounted:move |_|{spawn(async move{let _=document::eval(EDITOR_SCRIPT).await;});},
            div{class:"editor-toolbar",
                div{class:"editor-chapter-picker",label{r#for:"editor-chapter","Chapter"}
                    select{id:"editor-chapter",value:"{chapter}",onchange:move |e|{
                        let mut pv=state.preview;let mut w=pv.write();w.chapter_key=Some(e.value());w.selected=0;w.thumb_start=0;w.pages.clear();w.draft_crop=None;w.shading_samples.clear();drop(w);schedule_preview(state,0);
                    },for c in &series.chapters{option{key:"{c.key}",value:"{c.key}",selected:c.key==chapter,"{c.name}"}}}
                }
                div{class:"editor-page-nav",
                    button{id:"editor-previous",class:"btn small",disabled:selected==0||total==0,title:"Previous page · Left arrow",onclick:move |_|select_preview_page(state,selected.saturating_sub(1)),Icon{name:"arrow-left",size:15}}
                    span{class:"editor-page-count","Page {number} / {total}"}
                    button{id:"editor-next",class:"btn small",disabled:selected+1>=total,title:"Next page · Right arrow",onclick:move |_|select_preview_page(state,selected+1),span{class:"turn-arrow",Icon{name:"arrow-left",size:15}}}
                }
                div{class:"editor-toolbar-actions",
                    button{class:if view.show_original{"btn small on"}else{"btn small"},title:"Compare source artwork with the reader result",onclick:move |_|{state.preview.write().view.show_original=!view.show_original;},Icon{name:"eye",size:14}"Compare"}
                    button{id:"editor-crop-toggle",class:if view.editing_crop{"btn small on"}else{"btn small"},disabled:active.as_ref().and_then(|p|p.original.as_ref()).is_none(),title:"Drag crop handles · C",onclick:move |_|{let mut pv=state.preview;let mut w=pv.write();w.view.editing_crop=!view.editing_crop;w.view.show_original=true;w.view.source_focus=!view.editing_crop;},Icon{name:"crop",size:14}"Crop"}
                    button{class:if view.show_crop{"btn small on"}else{"btn small"},title:"Show excluded artwork without editing",onclick:move |_|{state.preview.write().view.show_crop=!view.show_crop;},"Guide"}
                    select{class:"editor-zoom","aria-label":"Preview zoom",onchange:move |e|{state.preview.write().view.zoom=match e.value().as_str(){"width"=>Zoom::Width,"actual"=>Zoom::Actual,"double"=>Zoom::Double,_=>Zoom::Fit};},
                        option{value:"fit",selected:view.zoom==Zoom::Fit,"Fit page"}option{value:"width",selected:view.zoom==Zoom::Width,"Fit width"}option{value:"actual",selected:view.zoom==Zoom::Actual,"100%"}option{value:"double",selected:view.zoom==Zoom::Double,"200%"}
                    }
                    button{class:"btn small",onclick:move |_|{state.preview.write().overview=true;schedule_preview(state,0);},"Overview"}
                    button{class:if view.focus{"btn small"}else{"btn small on"},title:"Show or hide properties",onclick:move |_|{state.preview.write().view.focus=!view.focus;},Icon{name:"settings",size:15}}
                }
            }
            if let Some(error)=&p.error{div{class:"banner bad small","{error}"}}
            div{class:"editor-body",
                nav{class:"editor-thumbnails","aria-label":"Reader pages",onscroll:move |e|{
                    let start=(e.data.scroll_top()/132.0).floor().max(0.0)as usize;
                    let needs={let mut pv=state.preview;let mut w=pv.write();if w.thumb_start!=start{w.thumb_start=start;true}else{false}};
                    if needs&&!state.preview.peek().overview{schedule_preview(state,80);}
                },
                    for(i,page)in p.pages.iter().enumerate(){button{key:"{i}",class:if i==selected{"editor-thumb selected"}else{"editor-thumb"},"data-editor-page":"{i}","aria-label":"Open page {i+1}","aria-current":if i==selected{"page"}else{"false"},onclick:move |_|select_preview_page(state,i),
                        div{class:"editor-thumb-art",if let Some(shot)=page.thumbnail.as_ref().or(page.shot.as_ref()){img{src:"{shot.url}",alt:"Reader page {i+1}"}}else{Icon{name:"file-image",size:23}}}span{"{i+1}"}
                    }}
                }
                main{class:"editor-main",
                    div{class:"editor-mobile-switch",
                        button{class:if !view.source_focus{"selected"}else{""},onclick:move |_|{state.preview.write().view.source_focus=false;},"Reader result"}
                        button{class:if view.source_focus{"selected"}else{""},onclick:move |_|{let mut p=state.preview;let mut w=p.write();w.view.source_focus=true;w.view.show_original=true;},"Source"}
                    }
                    div{class:if view.show_original{"editor-canvas comparing"}else{"editor-canvas"},
                        if view.show_original{section{class:"editor-pane source-pane",
                            div{class:"editor-pane-heading",strong{if p.strip{"Source section"}else{"Source artwork"}}span{if view.editing_crop{"Drag edges to crop"}else{"Before reader shading"}}}
                            if let Some(source)=active.as_ref().and_then(|page|page.original.as_ref()){
                                SourceCanvas{key:"{chapter}/{key}",source:source.clone(),draft,crop_key:key.clone(),editing:view.editing_crop,guide:view.show_crop}
                            }else{div{class:"editor-image-loading","Preparing source…"}}
                            if p.strip&&total>0{div{class:"editor-strip-overview",title:"Position in the continuous strip",
                                div{class:"editor-strip-track",for i in 0..total{button{key:"{i}",class:if i==selected{"current"}else{""},"aria-label":"Jump to strip section {i+1}",onclick:move |_|select_preview_page(state,i)}}}
                                span{"Section {number} of {total} · joined strip"}
                            }}
                        }}
                        section{class:"editor-pane result-pane",
                            div{class:"editor-pane-heading",strong{"Reader result"}span{"{device.name} · {device.width} × {device.height}"}}
                            div{class:"editor-result-scroll",div{class:"editor-reader-frame",style:"{reader_size}",
                                if let Some(shot)=active.as_ref().and_then(|page|page.shot.as_ref()){img{src:"{shot.url}",alt:"Page {number} exactly as exported to the reader",draggable:"false"}}else{div{class:"editor-image-loading","Drawing page…"}}
                            }}
                        }
                    }
                    div{class:"editor-status",
                        span{if p.busy{span{class:"spin",Icon{name:"loader-circle",size:13}}" Updating visible pages…"}else{Icon{name:"check",size:13}" Preview ready"}}
                        span{if p.draft_crop.is_some(){"Unsaved crop · Apply to include it in the export"}else{"Preview and export use the same saved settings"}}
                    }
                }
                if !view.focus{aside{class:"editor-inspector",
                    div{class:"editor-inspector-top",
                        PresetPicker{value:series.settings.clone(),preset_id:series.preset_id.clone(),on_pick:move |(id,settings)|apply_to_series(state,&pick_id,Some(id),settings),on_saved:move |id|{state.library.write().set_preset(&save_id,Some(id));state.save_library();}}
                        DeviceSelect{}
                        p{class:"editor-caption",if preset_base.is_none(){"Custom settings for this series"}else if series.overridden.is_empty(){"Following preset settings"}else{"{series.overridden.len()} settings changed from preset"}}
                    }
                    CropInspector{key:"{chapter}/{key}",draft,crop_key:key.clone(),saved}
                    div{class:"editor-settings-scope","Layout and appearance apply to this series"}
                    ConversionControls{value:series.settings.clone(),compact:true,shading_samples:p.shading_samples.clone(),manual_crop_active:series.crops.effective(&chapter,&key).unwrap_or_default()!=CropRect::default()||view.editing_crop,
                        on_manual_crop:move |_|{let mut pv=state.preview;let mut w=pv.write();w.view.editing_crop=true;w.view.show_original=true;w.view.source_focus=true;},
                        on_crop_mode:move |_|{
                            if let Some(series)=state.library.write().series.iter_mut().find(|s|s.id==mode_id){
                                series.crops.set(library::CropScope::Page,&mode_chapter,&mode_key,Some(CropRect::default()));
                            }
                            let mut pv=state.preview;let mut w=pv.write();w.draft_crop=None;w.view.editing_crop=false;drop(w);state.save_library();
                        },
                        onchange:move |v|{state.library.write().set_settings(&id,v,preset_base.as_ref());state.save_library();schedule_preview(state,180);}
                    }
                    button{class:"btn primary editor-export",onclick:move |_|state.open_tab(SeriesTab::Export),"Continue to export" span{class:"turn-arrow",Icon{name:"arrow-left",size:15}}}
                }}
            }
            if p.overview{div{class:"editor-overview-backdrop",section{class:"editor-overview",role:"dialog","aria-modal":"true","aria-label":"Chapter page overview",
                header{strong{"Chapter overview · {total} reader pages"}button{id:"editor-close-overview",class:"btn small",onclick:move |_|{state.preview.write().overview=false;},Icon{name:"x",size:16}}}
                div{class:"editor-overview-grid",for(i,page)in p.pages.iter().enumerate(){button{key:"{i}",class:if i==selected{"selected"}else{""},onclick:move |_|select_preview_page(state,i),
                    div{class:"editor-overview-art",if let Some(shot)=page.thumbnail.as_ref().or(page.shot.as_ref()){img{src:"{shot.url}",alt:"Reader page {i+1}"}}else{Icon{name:"file-image",size:24}}}span{"Page {i+1}"}
                }}}
            }}}
        }
    }
}
