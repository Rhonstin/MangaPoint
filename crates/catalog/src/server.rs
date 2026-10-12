//! Starting and stopping the bundled Suwayomi process.

use crate::client::{Credentials, SuwayomiClient};
use crate::error::{CatalogError, Result};
use crate::provision::Install;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

const CONFIG_PREFIX: &str = "-Dsuwayomi.tachidesk.config.server";
const START_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub install: Install,
    /// Suwayomi's data directory (database, extensions, downloads, logs).
    pub data_dir: PathBuf,
    /// Whether to allow the embedded Chromium. Off by default; see
    /// [`crate::browser`].
    pub browser_support: bool,
}

/// A running server plus a client already connected to it.
pub struct RunningServer {
    child: Child,
    pub client: SuwayomiClient,
    pub port: u16,
    pub log_path: PathBuf,
    pub data_dir: PathBuf,
}

impl RunningServer {
    /// Launch the server on a free loopback port, protected by a random
    /// password, and wait until it answers.
    pub async fn start(config: &ServerConfig) -> Result<Self> {
        let port = free_port()?;
        let credentials = Credentials {
            username: "mangapoint".into(),
            password: random_password()?,
        };
        std::fs::create_dir_all(config.data_dir.join("logs"))?;
        let log_path = config.data_dir.join("logs").join("mangapoint-server.log");
        let log = std::fs::File::create(&log_path)?;

        let mut cmd = Command::new(&config.install.java);
        cmd.args(java_args(config, port, &credentials))
            .current_dir(&config.install.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW: no console flashing up behind the app.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;

        let client = SuwayomiClient::new(format!("http://127.0.0.1:{port}"), Some(credentials))?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return Err(CatalogError::ServerExited(
                    status.to_string(),
                    log_path.display().to_string(),
                ));
            }
            if client.about().await.is_ok() {
                break;
            }
            if started.elapsed() > START_TIMEOUT {
                let _ = child.kill().await;
                return Err(CatalogError::StartTimeout(START_TIMEOUT));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Ok(Self {
            child,
            client,
            port,
            log_path,
            data_dir: config.data_dir.clone(),
        })
    }

    /// Stop the server. Suwayomi has no shutdown API, so this ends the
    /// process; its database tolerates that.
    pub async fn stop(mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }

    /// The last part of the server log, for progress and diagnostics.
    pub fn log_tail(&self, max_bytes: usize) -> String {
        read_tail(&self.log_path, max_bytes)
    }
}

fn java_args(config: &ServerConfig, port: u16, credentials: &Credentials) -> Vec<String> {
    let set = |key: &str, value: &str| format!("{CONFIG_PREFIX}.{key}={value}");
    vec![
        "--enable-native-access=ALL-UNNAMED".into(),
        set("rootDir", &config.data_dir.display().to_string()),
        // Loopback only: never reachable from the network.
        set("ip", "127.0.0.1"),
        set("port", &port.to_string()),
        // The app is the UI; no browser tab, tray icon or bundled web UI.
        set("initialOpenInBrowserEnabled", "false"),
        set("systemTrayEnabled", "false"),
        set("webUIEnabled", "false"),
        set("kcefEnabled", &config.browser_support.to_string()),
        // Without a password any local program or web page could drive it.
        set("authMode", "BASIC_AUTH"),
        set("authUsername", &credentials.username),
        set("authPassword", &credentials.password),
        "-jar".into(),
        config.install.jar.display().to_string(),
    ]
}

/// Ask the OS for an unused loopback port.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn random_password() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|e| CatalogError::Io(std::io::Error::other(e.to_string())))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn read_tail(path: &Path, max_bytes: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes as u64);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(browser: bool) -> ServerConfig {
        ServerConfig {
            install: Install {
                dir: "/i".into(),
                java: "/i/jre/bin/java".into(),
                jar: "/i/bin/Suwayomi-Server.jar".into(),
            },
            data_dir: "/data".into(),
            browser_support: browser,
        }
    }

    fn creds() -> Credentials {
        Credentials {
            username: "u".into(),
            password: "p".into(),
        }
    }

    #[test]
    fn server_is_loopback_only_and_password_protected() {
        let args = java_args(&config(false), 4000, &creds());
        let has = |s: &str| args.iter().any(|a| a.ends_with(s));
        assert!(has("server.ip=127.0.0.1"));
        assert!(has("server.port=4000"));
        assert!(has("server.authMode=BASIC_AUTH"));
        assert!(has("server.webUIEnabled=false"));
        assert!(has("server.systemTrayEnabled=false"));
        assert_eq!(args[args.len() - 2], "-jar");
    }

    #[test]
    fn browser_support_follows_the_setting() {
        let off = java_args(&config(false), 1, &creds());
        let on = java_args(&config(true), 1, &creds());
        assert!(off.iter().any(|a| a.ends_with("kcefEnabled=false")));
        assert!(on.iter().any(|a| a.ends_with("kcefEnabled=true")));
    }

    #[test]
    fn passwords_are_random_and_long() {
        let a = random_password().unwrap();
        let b = random_password().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }

    #[test]
    fn free_port_is_usable() {
        let p = free_port().unwrap();
        assert!(p > 0);
        assert!(std::net::TcpListener::bind(("127.0.0.1", p)).is_ok());
    }
}
