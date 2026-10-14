//! System notifications for finished downloads and exports (Windows, macOS
//! and Linux), which can be switched off in Settings.

use crate::state::AppState;
use dioxus::prelude::*;

/// Tell the user something has finished, unless they turned that off. Shown
/// from its own thread, because some systems make the caller wait.
pub fn finished(state: AppState, title: &str, body: &str) {
    if !state.settings.peek().notifications {
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let shown = notify_rust::Notification::new()
            .appname("MangaPoint")
            .summary(&title)
            .body(&body)
            .show();
        if let Err(e) = shown {
            tracing::warn!("Could not show a notification: {e}");
        }
    });
}
