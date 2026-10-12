//! Starting, stopping and restarting the local catalog server.

use crate::state::{paths, AppState, BootState, Services};
use catalog::provision::Progress;
use catalog::release::current_asset;
use catalog::server::read_tail;
use catalog::{browser, provision, RunningServer, ServerConfig};
use dioxus::prelude::*;
use library::Choice;
use sources::SourceManager;
use std::cell::Cell;
use std::fs;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

static SERVER: Mutex<Option<RunningServer>> = Mutex::new(None);

fn pid_file() -> std::path::PathBuf {
    paths().root().join("server.pid")
}

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
    kill_orphan();

    match start(state).await {
        Ok(svc) => {
            services.set(Some(svc));
            // Make sure the default extension store is known.
            if let Some(client) = state.client() {
                let mut sources = state.sources.read().clone();
                let result = SourceManager::new(&*client)
                    .ensure_default_store(&mut sources)
                    .await;
                let mut s = state.sources;
                s.set(sources);
                state.save_sources();
                if let Err(e) = result {
                    state.error(format!("Could not add the source catalog: {e}"));
                }
            }
            boot.set(BootState::Ready);
        }
        Err(message) => boot.set(BootState::Failed(message)),
    }
}

async fn start(state: AppState) -> Result<Services, String> {
    let mut boot = state.boot;
    let asset = current_asset().map_err(|e| e.to_string())?;

    let mut last_percent = 255u8;
    let install = provision(&paths().suwayomi_dir(), &asset, |p| {
        let (message, percent) = match p {
            Progress::Downloading {
                done,
                total: Some(t),
            } if t > 0 => {
                let pct = (done * 100 / t) as u8;
                (format!("Downloading the catalog engine… {pct}%"), Some(pct))
            }
            Progress::Downloading { .. } => ("Downloading the catalog engine…".to_string(), None),
            Progress::Verifying => ("Checking the download…".to_string(), None),
            Progress::Extracting => ("Unpacking…".to_string(), None),
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

    let browser_on = state.settings.read().browser_support && browser::supported();
    let config = ServerConfig {
        install,
        data_dir: paths().server_data_dir(),
        browser_support: browser_on,
    };
    boot.set(BootState::Starting {
        message: if browser_on && !browser::is_installed(&config.data_dir) {
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
            while !finished.get() {
                tokio::time::sleep(Duration::from_millis(800)).await;
                if let Some(p) = browser::download_progress(&read_tail(&log, 4096)) {
                    let mut b = boot;
                    b.set(BootState::Starting {
                        message: format!("Downloading browser support… {}%", p.percent),
                        percent: Some(p.percent),
                    });
                }
            }
        });
    }
    let started = RunningServer::start(&config).await;
    finished.set(true);
    let server = started.map_err(|e| format!("The catalog engine did not start: {e}"))?;

    let services = Services {
        client: Arc::new(server.client.clone()),
        port: server.port,
        data_dir: config.data_dir.clone(),
    };
    if let Some(pid) = server.pid() {
        let _ = fs::write(pid_file(), pid.to_string());
    }
    *SERVER.lock().unwrap() = Some(server);
    Ok(services)
}

async fn stop_running() {
    let server = SERVER.lock().unwrap().take();
    if let Some(server) = server {
        let _ = server.stop().await;
    }
    let _ = fs::remove_file(pid_file());
}

/// End the server right now. Used when the window closes.
pub fn shutdown() {
    if let Some(mut server) = SERVER.lock().unwrap().take() {
        server.kill_now();
    }
    let _ = fs::remove_file(pid_file());
}

/// If the app crashed last time, its server may still be running. Stop it,
/// but only if the recorded process really is the Suwayomi server (process
/// ids get reused).
fn kill_orphan() {
    let Ok(text) = fs::read_to_string(pid_file()) else {
        return;
    };
    let _ = fs::remove_file(pid_file());
    let Ok(pid) = text.trim().parse::<u32>() else {
        return;
    };
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    if let Some(process) = sys.process(pid) {
        if process
            .cmd()
            .iter()
            .any(|arg| arg.to_string_lossy().contains("Suwayomi-Server.jar"))
        {
            process.kill();
        }
    }
}

/// Turn browser support on and restart the server so it downloads and uses it.
pub async fn enable_browser(state: AppState) {
    let mut settings = state.settings;
    settings.write().browser_support = true;
    state.save_settings();
    boot(state).await;
}

/// Turn browser support off and delete the downloaded runtime.
pub async fn disable_browser(state: AppState) {
    let mut settings = state.settings;
    settings.write().browser_support = false;
    state.save_settings();
    stop_running().await;
    if let Err(e) = browser::remove(&paths().server_data_dir()) {
        state.error(format!("Could not remove browser support: {e}"));
    }
    boot(state).await;
}

/// Install a built-in source, asking first if it needs browser support that
/// is switched off. Returns whether the extension was installed now.
pub async fn install_default(state: AppState, id: &str) -> bool {
    let Some(client) = state.client() else {
        return false;
    };
    let Some(default) = sources::defaults::find(id) else {
        return false;
    };
    let mut sources_state = state.sources.read().clone();
    let result = SourceManager::new(&*client)
        .answer(&mut sources_state, id, Choice::Install)
        .await;
    let mut s = state.sources;
    s.set(sources_state);
    state.save_sources();
    match result {
        Ok(()) => true,
        Err(e) => {
            state.error(format!("Could not install {}: {e}", default.name));
            false
        }
    }
}
