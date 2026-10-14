//! The files exported for a reader: where they are, and what can be done
//! with them. Unlike the downloads, they stay until they are deleted here.

use super::widgets::{show_in_folder, Chip, Cover, Icon, NoCover};
use super::work::{
    forget_exported_file, load_saved_cover, open_series_from_library, saved_cover_key,
};
use crate::logic::{ago, human_bytes, plural};
use crate::state::{now_secs, AppState, Modal, ModalAction};
use dioxus::prelude::*;

#[component]
pub fn ExportsPage() -> Element {
    let state = use_context::<AppState>();
    // Covers of the exported series.
    use_effect(move || {
        let ids: Vec<String> = state
            .exports
            .read()
            .records
            .iter()
            .map(|r| r.series_id.clone())
            .collect();
        for id in ids {
            load_saved_cover(state, &id);
        }
    });
    let count = state.exports.read().records.len();
    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Exported files" }
                p { class: "muted",
                    if count == 0 {
                        "Files made for your reader."
                    } else {
                        "{plural(count, \"export\")} for your reader, newest first."
                    }
                }
            }
        }
        ExportedFiles {}
    }
}

/// Every export, newest first. The title opens the series; Open shows the
/// file in its folder.
#[component]
fn ExportedFiles() -> Element {
    let state = use_context::<AppState>();
    let mut records = state.exports.read().records.clone();
    records.reverse();
    let now = now_secs();
    rsx! {
        if records.is_empty() {
            p { class: "muted small", style: "margin: 4px 0 0",
                "Files you export for your reader are listed here, so you can find or delete them later."
            }
        }
        for r in records.iter() {
            {
                let (has_cover, in_library) = {
                    let library = state.library.read();
                    let series = library.find(&r.series_id);
                    (
                        series.and_then(|s| s.info.as_ref()).is_some_and(|i| i.has_cover),
                        series.is_some(),
                    )
                };
                let first = r.files.first().map(|f| f.path.clone()).unwrap_or_default();
                let can_open = !first.is_empty() && !first.starts_with("http") && std::path::Path::new(&first).exists();
                let total: u64 = r.files.iter().map(|f| f.bytes).sum();
                let place = r.target.strip_prefix("folder ").unwrap_or(&r.target).to_string();
                let series_id = r.series_id.clone();
                // One file is deleted from the buttons on the right; several each have their own.
                let single = (r.files.len() == 1).then(|| (r.id.clone(), first.clone()));
                rsx! {
                    div { class: "q-row q-done", key: "{r.id}",
                        div { class: "q-cover",
                            if has_cover { Cover { key_id: saved_cover_key(&r.series_id) } } else { NoCover {} }
                        }
                        div { class: "q-main",
                            div { class: "row wrap",
                                button { class: "link-title", disabled: !in_library,
                                    title: if in_library { "Open this series" } else { "This series is no longer in your library" },
                                    onclick: move |_| open_series_from_library(state, &series_id),
                                    "{r.title}"
                                }
                                Chip { text: r.format.to_uppercase() }
                            }
                            div { class: "q-stats",
                                span { "{plural(r.chapters, \"chapter\")} · {plural(r.pages, \"page\")}" }
                                span { "{human_bytes(total)}" }
                            }
                            div { class: "q-where", title: "{place}", "{place}" }
                            if r.files.len() > 1 {
                                for f in r.files.iter() {
                                    {
                                        let path = f.path.clone();
                                        let missing = !path.starts_with("http") && !std::path::Path::new(&path).is_file();
                                        let name = std::path::Path::new(&path)
                                            .file_name()
                                            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                                        let size = human_bytes(f.bytes);
                                        let record_id = r.id.clone();
                                        let (p_delete, p_forget) = (path.clone(), path.clone());
                                        let (id_delete, id_forget) = (record_id.clone(), record_id.clone());
                                        rsx! {
                                            div { class: "exp-file", key: "{path}",
                                                span { class: "exp-path", title: "{path}", "{name}" }
                                                if missing { Chip { text: "missing", kind: Some("warn".to_string()) } } else { span { class: "muted small", "{size}" } }
                                                if missing {
                                                    button { class: "btn small ghost", title: "Take it off this list",
                                                        onclick: move |_| forget_exported_file(state, id_forget.clone(), p_forget.clone()),
                                                        Icon { name: "x", size: 14 }
                                                    }
                                                } else {
                                                    button { class: "btn small ghost danger", title: "Delete this file",
                                                        onclick: move |_| ask_delete_export(state, id_delete.clone(), p_delete.clone()),
                                                        Icon { name: "trash", size: 14 }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "q-aside",
                            span { class: "q-when", "Exported {ago(now, r.at)}" }
                            button { class: "btn primary open-big", disabled: !can_open,
                                title: if can_open { "Show the file in its folder" } else { "The file is not there any more" },
                                onclick: move |_| show_in_folder(&first),
                                Icon { name: "folder-open", size: 18 } "Open"
                            }
                            if let Some((record_id, path)) = single {
                                button { class: "btn ghost open-big square danger", title: "Delete this file",
                                    disabled: !can_open,
                                    onclick: move |_| ask_delete_export(state, record_id.clone(), path.clone()),
                                    Icon { name: "trash", size: 18 }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Ask before a file that was exported is deleted from the computer.
fn ask_delete_export(state: AppState, record_id: String, path: String) {
    state.ask(Modal {
        title: "Delete this file?".into(),
        body: format!(
            "{path} will be deleted from your computer. Your downloaded chapters are not affected."
        ),
        confirm: "Delete file".into(),
        cancel: "Keep".into(),
        danger: true,
        action: ModalAction::DeleteExportedFile { record_id, path },
    });
}
