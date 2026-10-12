//! Small pieces used by several screens.

use crate::state::spawn_forever;
use crate::state::AppState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use sources::check::image_kind;

#[component]
pub fn ProgressBar(fraction: Option<f32>) -> Element {
    match fraction {
        Some(f) => {
            let pct = (f.clamp(0.0, 1.0) * 100.0).round();
            rsx! { div { class: "progress", div { style: "width: {pct}%" } } }
        }
        None => rsx! { div { class: "progress indeterminate", div {} } },
    }
}

#[component]
pub fn Chip(text: String, kind: Option<String>) -> Element {
    let class = format!("chip {}", kind.unwrap_or_default());
    rsx! { span { class: "{class}", "{text}" } }
}

/// A cover image, or a placeholder until it has loaded.
#[component]
pub fn Cover(key_id: String) -> Element {
    let state = use_context::<AppState>();
    let url = state.covers.read().get(&key_id).cloned();
    match url {
        Some(src) => rsx! { img { class: "cover", src: "{src}", loading: "lazy" } },
        None => rsx! { div { class: "cover empty", "📖" } },
    }
}

/// Fetch a cover through the catalog server (it needs the password, so the
/// web view cannot load it directly) and keep it as a data URL.
pub fn load_cover(state: AppState, key: String, path: String) {
    if state.covers.peek().contains_key(&key) {
        return;
    }
    let Some(client) = state.client() else { return };
    spawn_forever(async move {
        if let Ok(bytes) = client.get_bytes(&path).await {
            let mime = match image_kind(&bytes) {
                Some("PNG") => "image/png",
                Some("WebP") => "image/webp",
                Some("GIF") => "image/gif",
                Some(_) => "image/jpeg",
                None => return,
            };
            let url = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
            let mut covers = state.covers;
            covers.write().insert(key, url);
        }
    });
}

pub fn cover_key(source_id: &str, manga_id: i64) -> String {
    format!("{source_id}:{manga_id}")
}

/// Open a file's folder in the system file manager.
pub fn show_in_folder(path: &str) {
    let path = std::path::Path::new(path);
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}
