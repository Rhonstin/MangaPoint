//! Small pieces used by several screens.

use crate::state::spawn_forever;
use crate::state::AppState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use sources::check::image_kind;

/// A Lucide icon by name (see `icons.rs`). Takes the surrounding text color.
#[component]
pub fn Icon(name: &'static str, size: Option<u32>) -> Element {
    let size = size.unwrap_or(18);
    let body = super::icons::body(name).unwrap_or("");
    rsx! {
        svg {
            class: "icon",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            dangerous_inner_html: "{body}",
        }
    }
}

/// A screen with nothing on it yet: the cat napping, and a friendly line.
#[component]
pub fn EmptyState(
    title: String,
    message: String,
    compact: Option<bool>,
    /// The cat beside the words, in a card, rather than above them.
    side: Option<bool>,
    /// Show the cat asleep on an empty bookshelf instead of on its own.
    shelf: Option<bool>,
    /// Or another scene: "search", "ocean" or "waiting".
    scene: Option<&'static str>,
    children: Element,
) -> Element {
    let class = if side.unwrap_or(false) {
        "empty side"
    } else if compact.unwrap_or(false) {
        "empty compact"
    } else {
        "empty"
    };
    rsx! {
        div { class: "{class}",
            if scene == Some("ocean") {
                super::ocean::OceanCat {}
            } else if scene == Some("search") {
                super::scenes::SearchCat {}
            } else if scene == Some("waiting") {
                super::hang::HangingCat {}
            } else if shelf.unwrap_or(false) {
                super::shelf::ShelfCat {}
            } else {
                super::cat::RestingCat {}
            }
            div { class: "empty-words",
                h2 { "{title}" }
                p { "{message}" }
                {children}
            }
        }
    }
}

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

/// A switch that slides between off and on.
#[component]
pub fn Toggle(on: bool, onchange: EventHandler<bool>) -> Element {
    let checked = if on { "true" } else { "false" };
    rsx! {
        button {
            r#type: "button", role: "switch", "aria-checked": "{checked}",
            class: if on { "toggle on" } else { "toggle" },
            onclick: move |_| onchange.call(!on),
            span { class: "knob" }
        }
    }
}

/// A small question mark that explains a setting when the pointer is on it.
#[component]
pub fn Help(text: String) -> Element {
    rsx! {
        span { class: "help", tabindex: "0", "data-tip": "{text}", "aria-label": "{text}",
            Icon { name: "circle-help", size: 15 }
        }
    }
}

#[component]
pub fn Chip(text: String, kind: Option<String>) -> Element {
    let class = format!("chip {}", kind.unwrap_or_default());
    rsx! { span { class: "{class}", "{text}" } }
}

/// Something is loading: the cat runs in circles in the middle of the place
/// where it will show up.
#[component]
pub fn Loading(label: String) -> Element {
    rsx! {
        div { class: "loading",
            super::cat::CirclingCat {}
            span { "{label}" }
        }
    }
}

/// A small working marker with a few words, to sit in a line of text.
#[component]
pub fn Busy(label: String, size: Option<u32>) -> Element {
    rsx! {
        span { class: "busy",
            super::cat::CirclingCat { size: size.unwrap_or(44) }
            "{label}"
        }
    }
}

/// A cover image, or a placeholder until it has loaded.
#[component]
pub fn Cover(key_id: String) -> Element {
    let state = use_context::<AppState>();
    // A shared string: drawing a cover does not copy its picture.
    let url = state.covers.read().get(&key_id).cloned();
    match url {
        Some(src) => rsx! { img { class: "cover", src: "{src}", loading: "lazy" } },
        None => rsx! { NoCover {} },
    }
}

/// The picture for a cover that is missing or has not loaded yet.
#[component]
pub fn NoCover() -> Element {
    rsx! { div { class: "cover empty art", dangerous_inner_html: super::placeholder::svg() } }
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
            // Shown at poster size, so what the source sent is shrunk the way
            // a saved cover is, as long as that makes it smaller.
            let original = bytes.clone();
            let small = tokio::task::spawn_blocking(move || crate::logic::small_cover(&original))
                .await
                .ok()
                .flatten()
                .filter(|s| s.len() < bytes.len());
            let (mime, bytes) = match small {
                Some(small) => ("image/jpeg", small),
                None => (mime, bytes),
            };
            let url = format!("data:{mime};base64,{}", STANDARD.encode(&bytes));
            let mut covers = state.covers;
            covers.write().insert(key, url.into());
        }
    });
}

pub fn cover_key(source_id: &str, manga_id: i64) -> String {
    format!("{source_id}:{manga_id}")
}

/// A path written the way the system expects: on Windows every separator is a
/// backslash. A path mixing `/` and `\` is not understood by Explorer or the
/// folder dialog, which then open Documents instead.
pub fn native_path(path: &std::path::Path) -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        std::path::PathBuf::from(path.display().to_string().replace('/', "\\"))
    } else {
        path.to_path_buf()
    }
}

/// Open a file's folder in the system file manager, with the file selected.
pub fn show_in_folder(path: &str) {
    let path = native_path(std::path::Path::new(path));
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Explorer wants `/select,"path"` as one piece, quotes included.
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(&path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Open a folder in the system's file browser.
pub fn open_folder(dir: &std::path::Path) {
    let dir = native_path(dir);
    let dir = dir.as_path();
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Open a web address in the default browser.
pub fn open_link(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// The folder itself if it exists, else the closest parent that does.
fn nearest_existing(path: &std::path::Path) -> Option<&std::path::Path> {
    path.ancestors().find(|a| a.is_dir())
}

/// Ask the user for a folder with the system's own dialog. `start` is where
/// it opens, or else its nearest parent that exists (a folder not made yet
/// cannot be shown). `None` if they cancel.
pub async fn pick_folder(
    title: &str,
    start: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(dir) = start.as_deref().and_then(nearest_existing) {
        dialog = dialog.set_directory(native_path(dir));
    }
    dialog.pick_folder().await.map(|h| h.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_picker_opens_at_the_closest_folder_that_exists() {
        let here = std::env::temp_dir();
        assert_eq!(nearest_existing(&here), Some(here.as_path()));
        let missing = here.join("mangapoint-not-made").join("deeper");
        assert_eq!(nearest_existing(&missing), Some(here.as_path()));
    }

    #[test]
    fn a_native_path_keeps_a_unix_path_and_unmixes_a_windows_one() {
        let mixed = native_path(Path::new(r"C:\Data/Series/c1/1.png"));
        if cfg!(target_os = "windows") {
            assert_eq!(mixed, Path::new(r"C:\Data\Series\c1\1.png"));
        } else {
            assert_eq!(mixed, Path::new(r"C:\Data/Series/c1/1.png"));
        }
    }
}
