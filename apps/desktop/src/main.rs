//! MangaPoint: search manga, convert it for an Xteink reader, export.

mod backend;
mod logic;
mod state;
mod ui;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use tracing_subscriber::fmt::time::uptime;
use tracing_subscriber::EnvFilter;

/// What the console shows by default: MangaPoint's own progress messages
/// (installing, searching, exporting…) and real errors from anything else.
/// Set `MANGAPOINT_LOG` to change it, for example
/// `MANGAPOINT_LOG=mangapoint=debug,warn`.
const DEFAULT_LOG: &str = "mangapoint=info,error";

fn init_logging() {
    let filter =
        EnvFilter::try_from_env("MANGAPOINT_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG));
    // Installed before Dioxus starts, so its own logger steps aside.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(uptime())
        .compact()
        .try_init();
}

fn main() {
    init_logging();
    // Ctrl+C in the terminal would otherwise leave the catalog server running
    // and holding its database, so the next start could not open it.
    let _ = ctrlc::set_handler(|| {
        backend::shutdown();
        std::process::exit(130);
    });
    tracing::info!(
        "MangaPoint starting; data folder: {}",
        state::paths().root().display()
    );
    let window = WindowBuilder::new()
        .with_title("MangaPoint")
        .with_inner_size(LogicalSize::new(1180.0, 800.0))
        .with_min_inner_size(LogicalSize::new(860.0, 560.0));
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .launch(ui::App);
}
