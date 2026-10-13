//! The dialog that asks where downloaded files should go, before a download
//! starts.

use super::widgets::{pick_folder, Icon, Toggle};
use super::work::{set_download_folder, start_pending};
use crate::state::{library_files, AppState};
use dioxus::prelude::*;

#[component]
pub fn DownloadDialogHost() -> Element {
    let state = use_context::<AppState>();
    let mut dont_ask = use_signal(|| false);
    let Some(dialog) = state.download_dialog.read().clone() else {
        return rsx! {};
    };
    let folder = library_files().root().display().to_string();
    let pending = dialog.pending;
    let close = move || {
        let mut d = state.download_dialog;
        d.set(None);
    };
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "modal", onclick: move |e| e.stop_propagation(),
                h2 { "Download {dialog.summary}?" }
                p { class: "muted small", style: "margin: 0 0 10px",
                    "Choose where the downloaded pages are kept. Series you have already downloaded stay where they are; new ones go to this folder."
                }
                div { class: "folder-pick",
                    Icon { name: "folder-open", size: 16 }
                    span { class: "grow folder-path", title: "{folder}", "{folder}" }
                    button { class: "btn small",
                        onclick: move |_| {
                            spawn(async move {
                                let start = Some(library_files().root().to_path_buf());
                                if let Some(dir) = pick_folder("Where should downloads be kept?", start).await {
                                    set_download_folder(state, dir);
                                }
                            });
                        },
                        "Choose folder…"
                    }
                }
                div { class: "check-line", style: "margin-top: 14px",
                    Toggle { on: dont_ask(), onchange: move |on: bool| dont_ask.set(on) }
                    span { "Don't ask again. You can change it in Settings → Files." }
                }
                div { class: "actions",
                    button { class: "btn", onclick: move |_| close(), "Cancel" }
                    button { class: "btn primary",
                        onclick: move |_| {
                            if dont_ask() {
                                let mut settings = state.settings;
                                settings.write().ask_download_folder = false;
                                state.save_settings();
                            }
                            close();
                            start_pending(state, pending);
                        },
                        Icon { name: "download", size: 16 }
                        "Download"
                    }
                }
            }
        }
    }
}
