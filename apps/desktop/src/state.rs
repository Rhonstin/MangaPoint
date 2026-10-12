//! Everything the screens share.

use crate::logic::JobProgress;
use catalog::SuwayomiClient;
use dioxus::prelude::*;
use library::{AppPaths, HealthDb, JsonFile, Settings, SourceState};
use pipeline::CancelFlag;
use search::RankedSeries;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Where the app keeps its files. Set `MANGAPOINT_HOME` to use another folder
/// (handy for trying the app without touching real settings).
pub fn paths() -> &'static AppPaths {
    static PATHS: OnceLock<AppPaths> = OnceLock::new();
    PATHS.get_or_init(|| {
        AppPaths::new(
            std::env::var_os("MANGAPOINT_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(AppPaths::default_root),
        )
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Setup,
    Search,
    Series,
    Sources,
    Jobs,
    Settings,
}

#[derive(Clone, PartialEq, Debug)]
pub enum BootState {
    Starting {
        message: String,
        percent: Option<u8>,
    },
    Ready,
    Failed(String),
}

/// The running catalog server.
#[derive(Clone)]
pub struct Services {
    pub client: Arc<SuwayomiClient>,
    pub port: u16,
    pub data_dir: PathBuf,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

/// What a confirmation dialog does when accepted.
#[derive(Clone, PartialEq, Debug)]
pub enum ModalAction {
    /// Turn on browser support, then install the built-in source with this id (if any).
    EnableBrowser {
        then_install: Option<String>,
    },
    DisableBrowser,
    RemoveExtension(String),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Modal {
    pub title: String,
    pub body: String,
    pub confirm: String,
    pub cancel: String,
    pub danger: bool,
    pub action: ModalAction,
}

#[derive(Clone, Default)]
pub struct SearchView {
    pub query: String,
    pub ranked: Vec<RankedSeries>,
    pub searching: bool,
    pub answered: usize,
    pub total: usize,
    /// Sources that did not answer, by name.
    pub failed: Vec<String>,
    pub no_sources: bool,
}

#[derive(Clone)]
pub struct JobView {
    pub title: String,
    pub progress: JobProgress,
    /// `None` while running, then the files written or the error.
    pub result: Option<Result<Vec<String>, String>>,
    pub skipped: Vec<String>,
    pub cancel: CancelFlag,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub boot: Signal<BootState>,
    pub services: Signal<Option<Services>>,
    pub settings: Signal<Settings>,
    pub sources: Signal<SourceState>,
    pub health: Signal<HealthDb>,
    pub page: Signal<Page>,
    pub series: Signal<Option<RankedSeries>>,
    pub search: Signal<SearchView>,
    /// Cover images as data URLs, keyed by `source:manga`.
    pub covers: Signal<HashMap<String, String>>,
    pub notice: Signal<Option<Notice>>,
    pub modal: Signal<Option<Modal>>,
    pub job: Signal<Option<JobView>>,
    /// Bumped when the job history changes, so the Jobs page reloads.
    pub jobs_version: Signal<u32>,
    /// Bumped when installed sources or test results change.
    pub sources_version: Signal<u32>,
    /// Source ids being tested right now.
    pub testing: Signal<HashSet<String>>,
    /// True while "install all recommended" is running.
    pub installing: Signal<bool>,
}

impl AppState {
    /// Create the state inside the root component, loading saved files.
    pub fn init() -> Self {
        let sources: SourceState = JsonFile::new(paths().sources_file()).load();
        let first_page = if sources.onboarding_done {
            Page::Search
        } else {
            Page::Setup
        };
        Self {
            boot: use_signal(|| BootState::Starting {
                message: "Starting…".into(),
                percent: None,
            }),
            services: use_signal(|| None),
            settings: use_signal(|| JsonFile::<Settings>::new(paths().settings_file()).load()),
            sources: use_signal(move || sources.clone()),
            health: use_signal(|| JsonFile::<HealthDb>::new(paths().health_file()).load()),
            page: use_signal(move || first_page),
            series: use_signal(|| None),
            search: use_signal(SearchView::default),
            covers: use_signal(HashMap::new),
            notice: use_signal(|| None),
            modal: use_signal(|| None),
            job: use_signal(|| None),
            jobs_version: use_signal(|| 0),
            sources_version: use_signal(|| 0),
            testing: use_signal(HashSet::new),
            installing: use_signal(|| false),
        }
    }

    pub fn client(&self) -> Option<Arc<SuwayomiClient>> {
        self.services.read().as_ref().map(|s| s.client.clone())
    }

    pub fn go(&self, page: Page) {
        let mut p = self.page;
        p.set(page);
    }

    pub fn notify(&self, text: impl Into<String>) {
        let mut n = self.notice;
        n.set(Some(Notice {
            text: text.into(),
            error: false,
        }));
    }

    pub fn error(&self, text: impl Into<String>) {
        let mut n = self.notice;
        n.set(Some(Notice {
            text: text.into(),
            error: true,
        }));
    }

    pub fn ask(&self, modal: Modal) {
        let mut m = self.modal;
        m.set(Some(modal));
    }

    pub fn save_settings(&self) {
        let _ = JsonFile::new(paths().settings_file()).save(&*self.settings.read());
    }

    pub fn save_sources(&self) {
        let _ = JsonFile::new(paths().sources_file()).save(&*self.sources.read());
    }

    pub fn save_health(&self) {
        let _ = JsonFile::new(paths().health_file()).save(&*self.health.read());
    }

    pub fn bump_jobs(&self) {
        let mut v = self.jobs_version;
        let next = *v.peek() + 1;
        v.set(next);
    }

    pub fn bump_sources(&self) {
        let mut v = self.sources_version;
        let next = *v.peek() + 1;
        v.set(next);
    }
}

/// Run a task for as long as the app lives, even after the screen that
/// started it is closed. (Ordinary `spawn` tasks stop with their screen.)
pub fn spawn_forever(future: impl std::future::Future<Output = ()> + 'static) {
    dioxus::core::spawn_forever(future);
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
