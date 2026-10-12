//! MangaPoint: search manga, convert it for an Xteink reader, export.

mod assets;
mod backend;
mod logic;
mod state;
mod ui;

use dioxus::desktop::tao::window::Icon;
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
    // Developer command: write the app icon files and exit.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--write-icons") {
        let dir = args.next().unwrap_or_else(|| "assets".into());
        match assets::write_icons(std::path::Path::new(&dir)) {
            Ok(()) => println!("wrote icon.png and icon.ico to {dir}"),
            Err(e) => {
                eprintln!("could not write the icons: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

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
    // The cat is the window and taskbar icon (Windows and Linux; macOS takes
    // its Dock icon from the app bundle, which the installer provides).
    let (rgba, w, h) = ui::cat::icon_rgba(8);
    let window = WindowBuilder::new()
        .with_window_icon(Icon::from_rgba(rgba, w, h).ok())
        .with_title("MangaPoint")
        .with_inner_size(LogicalSize::new(1180.0, 800.0))
        .with_min_inner_size(LogicalSize::new(860.0, 560.0));
    let mut config = Config::new().with_window(window).with_menu(None);
    // Developer switch: let tools such as an automated UI test connect to the
    // app's web view. Off unless MANGAPOINT_DEBUG_PORT is set; the port only
    // listens on this computer.
    if let Ok(port) = std::env::var("MANGAPOINT_DEBUG_PORT") {
        if port.parse::<u16>().is_ok() {
            tracing::info!("UI debugging enabled on 127.0.0.1:{port}");
            config = config.with_windows_browser_args(format!(
                "--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
            ));
        }
    }
    // A copy run with its own data folder (MANGAPOINT_HOME) also gets its own
    // web view folder. Otherwise it shares one with any MangaPoint already
    // open, which blocks it from starting with different settings.
    if std::env::var_os("MANGAPOINT_HOME").is_some() {
        config = config.with_data_directory(state::paths().root().join("webview"));
    }
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(ui::App);
}
