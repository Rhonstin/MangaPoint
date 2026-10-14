//! Where MangaPoint keeps its files.

use std::path::{Path, PathBuf};

/// ```text
/// <root>/
/// ├── settings.json
/// ├── sources.json
/// ├── health.json
/// ├── jobs.json
/// ├── suwayomi/        the provisioned server (reinstallable)
/// ├── server-data/     Suwayomi's database, extensions, logs
/// ├── cache/pages/     downloaded page images for unfinished jobs
/// └── exports/         default output folder
/// ```
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The per-user data folder for this platform.
    pub fn default_root() -> PathBuf {
        let home = || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
        };
        if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join("AppData").join("Roaming"))
                .join("MangaPoint")
        } else if cfg!(target_os = "macos") {
            home()
                .join("Library")
                .join("Application Support")
                .join("MangaPoint")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local").join("share"))
                .join("mangapoint")
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn sources_file(&self) -> PathBuf {
        self.root.join("sources.json")
    }
    pub fn health_file(&self) -> PathBuf {
        self.root.join("health.json")
    }
    pub fn jobs_file(&self) -> PathBuf {
        self.root.join("jobs.json")
    }
    pub fn suwayomi_dir(&self) -> PathBuf {
        self.root.join("suwayomi")
    }
    pub fn server_data_dir(&self) -> PathBuf {
        self.root.join("server-data")
    }
    pub fn page_cache_dir(&self) -> PathBuf {
        self.root.join("cache").join("pages")
    }
    /// Downloaded chapters (see `downloads`).
    pub fn library_dir(&self) -> PathBuf {
        self.root.join("library")
    }
    pub fn library_file(&self) -> PathBuf {
        self.root.join("library.json")
    }
    pub fn exports_file(&self) -> PathBuf {
        self.root.join("exports.json")
    }
    /// The download queue: what is waiting, and what has finished.
    pub fn queue_file(&self) -> PathBuf {
        self.root.join("queue.json")
    }
    /// An export that has not finished, so it can go on after the app was closed.
    pub fn export_job_file(&self) -> PathBuf {
        self.root.join("export_job.json")
    }
    pub fn presets_file(&self) -> PathBuf {
        self.root.join("presets.json")
    }
    pub fn speeds_file(&self) -> PathBuf {
        self.root.join("speeds.json")
    }
    /// Scratch space for files being assembled.
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("cache").join("spool")
    }
    pub fn default_export_dir(&self) -> PathBuf {
        self.root.join("exports")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_under_the_root() {
        let p = AppPaths::new("/r");
        for f in [
            p.settings_file(),
            p.sources_file(),
            p.health_file(),
            p.jobs_file(),
            p.suwayomi_dir(),
            p.server_data_dir(),
            p.page_cache_dir(),
            p.default_export_dir(),
            p.library_dir(),
            p.library_file(),
            p.exports_file(),
            p.presets_file(),
            p.speeds_file(),
            p.spool_dir(),
        ] {
            assert!(f.starts_with("/r"), "{f:?}");
        }
    }

    #[test]
    fn default_root_ends_in_the_app_name() {
        let root = AppPaths::default_root();
        let name = root.file_name().unwrap().to_string_lossy().to_lowercase();
        assert_eq!(name, "mangapoint");
    }
}
