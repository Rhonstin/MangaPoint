//! Starting, stopping and restarting the local catalog server.

use crate::state::{paths, AppState, BootState, Services};
use catalog::provision::Progress;
use catalog::release::{current_asset, VERSION_TAG};
use catalog::server::read_tail;
use catalog::{browser, provision, RunningServer, ServerConfig};
use dioxus::prelude::*;
use library::Choice;
use sources::SourceManager;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tracing::{error, info, warn};

static SERVER: Mutex<Option<RunningServer>> = Mutex::new(None);

/// Bring the server up, showing progress, and publish it to the app.
/// Safe to call again: any server already running is stopped first.
pub async fn boot(state: AppState) {
    let mut boot = state.boot;
    let mut services = state.services;
    services.set(None);
    boot.set(BootState::Starting {
        message: "Preparing…".into(),
        percent: None,
    });
    stop_running().await;
    stop_leftover_servers().await;

    match start(state).await {
        Ok(svc) => {
            services.set(Some(svc));
            // Make sure the default extension store is known.
            if let Some(client) = state.client() {
                let mut sources = state.sources.read().clone();
                let was_added = sources.default_store_added;
                let result = SourceManager::new(&*client)
                    .ensure_default_store(&mut sources)
                    .await;
                let mut s = state.sources;
                s.set(sources);
                state.save_sources();
                match result {
                    Ok(()) if !was_added => info!("Added the default source catalog (Keiyoushi)"),
                    Ok(()) => {}
                    Err(e) => state.error(format!("Could not add the source catalog: {e}")),
                }
            }
            info!("Ready");
            boot.set(BootState::Ready);
        }
        Err(message) => {
            error!("{message}");
            boot.set(BootState::Failed(message));
        }
    }
}

async fn start(state: AppState) -> Result<Services, String> {
    let mut boot = state.boot;
    let asset = current_asset().map_err(|e| e.to_string())?;

    info!("Checking for the catalog engine (Suwayomi {VERSION_TAG})");
    let mut last_percent = 255u8;
    let mut logged_quarter = 255u64;
    let mut downloaded = false;
    let install = provision(&paths().suwayomi_dir(), &asset, |p| {
        let (message, percent) = match p {
            Progress::Downloading {
                done,
                total: Some(t),
            } if t > 0 => {
                downloaded = true;
                let quarter = done * 4 / t;
                if quarter != logged_quarter {
                    logged_quarter = quarter;
                    info!(
                        "Downloading the catalog engine ({} MB)… {}%",
                        t / 1_000_000,
                        quarter * 25
                    );
                }
                let pct = (done * 100 / t) as u8;
                (format!("Downloading the catalog engine… {pct}%"), Some(pct))
            }
            Progress::Downloading { .. } => ("Downloading the catalog engine…".to_string(), None),
            Progress::Verifying => {
                info!("Download finished; checking its checksum");
                ("Checking the download…".to_string(), None)
            }
            Progress::Extracting => {
                info!("Checksum matches; unpacking (this takes a minute)");
                ("Unpacking…".to_string(), None)
            }
            Progress::Done => return,
        };
        // Redraw only when the percentage actually moves.
        if percent != Some(last_percent) {
            last_percent = percent.unwrap_or(255);
            boot.set(BootState::Starting { message, percent });
        }
    })
    .await
    .map_err(|e| format!("Could not install the catalog engine: {e}"))?;
    if downloaded {
        info!("Catalog engine installed in {}", install.dir.display());
    } else {
        info!("Catalog engine already installed");
    }

    let browser_on = state.settings.read().browser_support && browser::supported();
    let config = ServerConfig {
        install,
        data_dir: paths().server_data_dir(),
        browser_support: browser_on,
    };
    let needs_browser_download = browser_on && !browser::is_installed(&config.data_dir);
    if needs_browser_download {
        info!("Starting the catalog server; browser support is on, so it will download about 270 MB first");
    } else {
        info!(
            "Starting the catalog server (browser support {})",
            if browser_on { "on" } else { "off" }
        );
    }
    boot.set(BootState::Starting {
        message: if needs_browser_download {
            "Starting, and downloading browser support (about 270 MB)…".into()
        } else {
            "Starting the catalog engine…".into()
        },
        percent: None,
    });

    // While the server starts it may download browser support; show how far along it is.
    let log = config.data_dir.join("logs").join("mangapoint-server.log");
    let finished = Rc::new(Cell::new(false));
    {
        let finished = finished.clone();
        spawn(async move {
            let mut logged_quarter = 255u8;
            while !finished.get() {
                tokio::time::sleep(Duration::from_millis(800)).await;
                if let Some(p) = browser::download_progress(&read_tail(&log, 4096)) {
                    if p.percent / 25 != logged_quarter {
                        logged_quarter = p.percent / 25;
                        info!("Downloading browser support… {}%", logged_quarter * 25);
                    }
                    let mut b = boot;
                    b.set(BootState::Starting {
                        message: format!("Downloading browser support… {}%", p.percent),
                        percent: Some(p.percent),
                    });
                }
            }
        });
    }
    let started_at = Instant::now();
    let started = RunningServer::start(&config).await;
    finished.set(true);
    let server = started.map_err(|e| format!("The catalog engine did not start: {e}"))?;
    info!(
        "Catalog server ready on 127.0.0.1:{} after {:.1}s (local only, password protected)",
        server.port,
        started_at.elapsed().as_secs_f32()
    );

    let services = Services {
        client: Arc::new(server.client.clone()),
        port: server.port,
        data_dir: config.data_dir.clone(),
    };
    *SERVER.lock().unwrap() = Some(server);
    Ok(services)
}

async fn stop_running() {
    let server = SERVER.lock().unwrap().take();
    if let Some(server) = server {
        info!("Stopping the catalog server");
        let _ = server.stop().await;
    }
}

/// End the server right now. Used when the window closes or on Ctrl+C.
pub fn shutdown() {
    if let Some(mut server) = SERVER.lock().unwrap().take() {
        info!("Closing: stopping the catalog server");
        server.kill_now();
    }
}

const ROOT_DIR_ARG: &str = "-Dsuwayomi.tachidesk.config.server.rootDir=";

/// Is this command line a Suwayomi server using `data_dir`?
///
/// Matching on the command line (rather than a remembered process id) finds
/// a server left behind however the last run ended, and cannot be fooled by
/// a recycled process id.
fn is_our_server(cmd: &[String], data_dir: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    cmd.iter().any(|a| a.contains("Suwayomi-Server.jar"))
        && cmd.iter().any(|a| {
            a.strip_prefix(ROOT_DIR_ARG)
                .is_some_and(|dir| norm(dir) == norm(data_dir))
        })
}

/// A catalog server left over from an earlier run (a crash, or Ctrl+C in the
/// terminal) keeps the database locked, so a new one cannot start. Stop any
/// such server and wait until it has really exited.
async fn stop_leftover_servers() {
    let data_dir = paths().server_data_dir().display().to_string();
    let find = || {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let leftovers: Vec<Pid> = sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                let cmd: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                is_our_server(&cmd, &data_dir)
            })
            .map(|(pid, p)| {
                p.kill();
                *pid
            })
            .collect();
        leftovers
    };

    let killed = find();
    if killed.is_empty() {
        return;
    }
    warn!(
        "A catalog server from an earlier run was still running (process {}); stopping it",
        killed
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    // Wait for it to let go of the database before starting a new one.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&killed),
            true,
            ProcessRefreshKind::nothing(),
        );
        if killed.iter().all(|pid| sys.process(*pid).is_none()) {
            info!("The earlier server has stopped");
            return;
        }
        if Instant::now() > deadline {
            warn!("The earlier server is slow to stop; continuing anyway");
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Turn browser support on and restart the server so it downloads and uses it.
pub async fn enable_browser(state: AppState) {
    info!("Turning on browser support; restarting the catalog server");
    let mut settings = state.settings;
    settings.write().browser_support = true;
    state.save_settings();
    boot(state).await;
}

/// Turn browser support off and delete the downloaded runtime.
pub async fn disable_browser(state: AppState) {
    info!("Turning off browser support and deleting its files");
    let mut settings = state.settings;
    settings.write().browser_support = false;
    state.save_settings();
    stop_running().await;
    let dir = paths().server_data_dir();
    let freed = browser::size_on_disk(&dir);
    match browser::remove(&dir) {
        Ok(()) => info!("Removed browser support ({} MB freed)", freed / 1_000_000),
        Err(e) => state.error(format!("Could not remove browser support: {e}")),
    }
    boot(state).await;
}

/// Install a built-in source. Returns whether it was installed.
pub async fn install_default(state: AppState, id: &str) -> bool {
    let Some(client) = state.client() else {
        return false;
    };
    let Some(default) = sources::defaults::find(id) else {
        return false;
    };
    info!("Installing source {}…", default.name);
    let started = Instant::now();
    let mut sources_state = state.sources.read().clone();
    let result = SourceManager::new(&*client)
        .answer(&mut sources_state, id, Choice::Install)
        .await;
    let mut s = state.sources;
    s.set(sources_state);
    state.save_sources();
    match result {
        Ok(()) => {
            info!(
                "Installed source {} in {:.1}s",
                default.name,
                started.elapsed().as_secs_f32()
            );
            true
        }
        Err(e) => {
            state.error(format!("Could not install {}: {e}", default.name));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    const DATA: &str = r"C:\Users\me\AppData\Roaming\MangaPoint\server-data";

    #[test]
    fn recognises_our_server_by_jar_and_data_folder() {
        let ours = cmd(&[
            r"C:\x\jre\bin\java.exe",
            "--enable-native-access=ALL-UNNAMED",
            &format!("{ROOT_DIR_ARG}{DATA}"),
            "-Dsuwayomi.tachidesk.config.server.port=4567",
            "-jar",
            r"C:\x\bin\Suwayomi-Server.jar",
        ]);
        assert!(is_our_server(&ours, DATA));
    }

    #[test]
    fn ignores_servers_for_other_folders_and_unrelated_programs() {
        let other_folder = cmd(&[
            "java.exe",
            &format!(r"{ROOT_DIR_ARG}C:\other\data"),
            "-jar",
            "Suwayomi-Server.jar",
        ]);
        assert!(
            !is_our_server(&other_folder, DATA),
            "another data folder is not ours"
        );

        let not_suwayomi = cmd(&[
            "java.exe",
            &format!("{ROOT_DIR_ARG}{DATA}"),
            "-jar",
            "other.jar",
        ]);
        assert!(!is_our_server(&not_suwayomi, DATA));

        let no_root = cmd(&["java.exe", "-jar", "Suwayomi-Server.jar"]);
        assert!(!is_our_server(&no_root, DATA));
        assert!(!is_our_server(&[], DATA));
    }

    #[test]
    fn path_spelling_differences_still_match() {
        let c = cmd(&[
            "java.exe",
            &format!("{ROOT_DIR_ARG}c:/users/me/appdata/roaming/mangapoint/server-data/"),
            "-jar",
            "Suwayomi-Server.jar",
        ]);
        assert!(
            is_our_server(&c, DATA),
            "case, slashes and a trailing slash do not matter"
        );
    }
}
