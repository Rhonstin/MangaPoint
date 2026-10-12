//! Everything the screens share.

use crate::logic::{JobProgress, SearchSort, SortKey, TargetChoice};
use crate::queue::DownloadQueue;
use catalog::SuwayomiClient;
use dioxus::prelude::*;
use library::{
    AppPaths, ConversionSettings, ExportsDb, HealthDb, JsonFile, LibraryDb, LibraryFiles,
    PresetsDb, Settings, SourceState, SpeedDb,
};
use pipeline::{CancelFlag, SourceChoice};
use search::{MergedChapter, RankedSeries};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
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

/// The downloaded chapters on disk.
pub fn library_files() -> LibraryFiles {
    LibraryFiles::new(paths().library_dir())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Setup,
    Search,
    /// A series the user opened: chapters, downloads, preview, export.
    Series,
    Library,
    /// The download queue and the exported files.
    Downloads,
    /// Choosing what to download from one or more library series.
    Plan,
    Sources,
    Settings,
}

/// The parts of the Settings page, shown in its own sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    #[default]
    Reader,
    Presets,
    Downloads,
    Files,
    Browser,
    About,
}

/// The Presets page: the preset being edited, and the test page it is tried on.
#[derive(Clone, Default)]
pub struct PresetLab {
    /// The preset being edited.
    pub selected: Option<String>,
    /// Its settings as they are now, saved or not.
    pub draft: ConversionSettings,
    /// The series whose pages supply the test page; `None` uses the built-in
    /// sample page.
    pub sample_series: Option<String>,
    /// Which page of that series' first chapter (0-based).
    pub page: usize,
    pub total_pages: usize,
    pub original: Option<Shot>,
    pub kept: Option<[f32; 4]>,
    pub converted: Vec<Shot>,
    pub busy: bool,
    pub error: Option<String>,
    /// Bumped on every change so an older drawing never replaces a newer one.
    pub generation: u64,
}

/// The steps within an open series.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeriesTab {
    Overview,
    Chapters,
    Downloads,
    Preview,
    Export,
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
    /// Take series out of the library, deleting what was downloaded for them
    /// (exported files are kept).
    RemoveSeries {
        series_ids: Vec<String>,
    },
    /// Delete the downloaded pages of some chapters; the series stays.
    DeleteChapters {
        series_id: String,
        keys: Vec<String>,
    },
    /// Export every downloaded chapter of these series, one file each.
    ExportSeries {
        series_ids: Vec<String>,
    },
    /// Delete every downloaded chapter of these series; they stay in the library.
    DeleteDownloads {
        series_ids: Vec<String>,
    },
    /// Delete one of the user's presets, or put a changed built-in back.
    DeletePreset {
        id: String,
    },
    /// Delete an exported file from disk.
    DeleteExportedFile {
        record_id: String,
        path: String,
    },
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
    /// Every installed source is turned off for searching.
    pub none_selected: bool,
    /// Which search these results belong to. A newer search bumps it, and an
    /// older one still running then stops publishing.
    pub generation: u64,
}

/// The series the user is working on.
#[derive(Clone)]
pub struct Workspace {
    /// The library id (from the title), under which downloads are kept.
    pub series_id: String,
    pub title: String,
    /// Present when opened from a search; needed to find more chapters.
    pub ranked: Option<RankedSeries>,
    pub tab: SeriesTab,
    /// Where the back button goes.
    pub origin: Page,
}

/// Chapter lists from the sources, merged.
pub struct LoadedChapters {
    pub merged: Vec<MergedChapter>,
    /// Source id of each member, best first; chapter options point into this.
    pub member_sources: Vec<String>,
    pub member_names: Vec<String>,
    /// The same members as they are kept in the library.
    pub saved: Vec<library::SavedSource>,
    pub unreachable: usize,
}

/// What the download planner knows about one series.
#[derive(Clone)]
pub enum PlanChapters {
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// One series in the download planner, with the chapters ticked for it.
#[derive(Clone)]
pub struct PlanEntry {
    pub series_id: String,
    pub title: String,
    pub chapters: PlanChapters,
    /// Positions in the merged chapter list.
    pub picked: HashSet<usize>,
    pub open: bool,
    /// The words typed in this series' filter box.
    pub filter: String,
    pub sort: (SortKey, bool),
}

#[derive(Clone)]
pub enum ChaptersState {
    /// Nothing to load (opened from the library).
    Idle,
    Loading,
    Ready(Rc<LoadedChapters>),
    Failed(String),
}

/// How the chapter table is filtered and sorted.
#[derive(Clone)]
pub struct TableView {
    pub filter: String,
    /// Column, and whether descending.
    pub sort: (SortKey, bool),
}

impl Default for TableView {
    fn default() -> Self {
        Self {
            filter: String::new(),
            sort: (SortKey::Number, true),
        }
    }
}

/// A rendered page as a data URL.
#[derive(Clone, PartialEq)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// How big the pages are drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Zoom {
    /// As large as fits without scrolling.
    #[default]
    Fit,
    /// As wide as the space allows; scroll down for the rest.
    Width,
    /// One screen pixel per device pixel.
    Actual,
    Double,
}

/// Preview display choices that are not conversion settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PreviewView {
    pub zoom: Zoom,
    /// Hide the settings panel to give the pages the whole width.
    pub focus: bool,
    /// Show the downloaded page beside the converted one.
    pub show_original: bool,
    /// Draw the cropped-away area on the original.
    pub show_crop: bool,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: Zoom::Fit,
            focus: false,
            show_original: true,
            show_crop: true,
        }
    }
}

/// One page of the chapter, rendered for the preview.
#[derive(Clone, PartialEq)]
pub struct PreviewPage {
    /// The page as it will appear on the reader (several for a long strip,
    /// two for a split spread).
    pub converted: Vec<Shot>,
    /// The downloaded page, unconverted, for comparison.
    pub original: Shot,
    /// The part of the original the conversion keeps: left, top, width, height
    /// as fractions of the image.
    pub kept: [f32; 4],
}

/// What the Preview tab is showing: every page of one chapter.
#[derive(Clone, Default)]
pub struct PreviewState {
    /// Downloaded chapter being previewed.
    pub chapter_key: Option<String>,
    /// Every page of the chapter, in order. A page is `None` until it has been
    /// drawn; after a settings change the old drawing stays until the new one
    /// replaces it.
    pub pages: Vec<Option<Rc<PreviewPage>>>,
    /// Pages drawn with the current settings so far.
    pub fresh: usize,
    pub view: PreviewView,
    pub busy: bool,
    pub error: Option<String>,
    /// Bumped on every change so an older render never overwrites a newer one.
    pub generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GroupingChoice {
    Single,
    PerChapter,
    EveryN,
}

/// Choices on the Export tab, kept while the user moves between tabs.
#[derive(Clone)]
pub struct ExportOpts {
    pub target: TargetChoice,
    pub grouping: GroupingChoice,
    /// The "N" of "every N chapters", as typed.
    pub every_n: String,
    pub advanced_open: bool,
    /// Downloaded chapters left out of the export.
    pub excluded: HashSet<String>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            target: TargetChoice::Folder,
            grouping: GroupingChoice::Single,
            every_n: "10".into(),
            advanced_open: false,
            excluded: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobKind {
    Export,
}

#[derive(Clone)]
pub struct JobView {
    pub kind: JobKind,
    pub title: String,
    pub progress: JobProgress,
    /// `None` while running, then a one-line summary or the error.
    pub result: Option<Result<String, String>>,
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
    pub search: Signal<SearchView>,
    /// How the search results are ordered.
    pub search_sort: Signal<SearchSort>,
    pub settings_section: Signal<SettingsSection>,
    pub preset_lab: Signal<PresetLab>,
    /// Cover images as data URLs, keyed by `source:manga`.
    pub covers: Signal<HashMap<String, String>>,
    pub notice: Signal<Option<Notice>>,
    pub modal: Signal<Option<Modal>>,
    /// Downloaded chapters (their pages are on disk).
    pub library: Signal<LibraryDb>,
    /// Files exported so far.
    pub exports: Signal<ExportsDb>,
    /// The user's conversion presets (and changes to the built-in ones).
    pub presets: Signal<PresetsDb>,
    /// How fast each source has been, from short speed tests.
    pub speeds: Signal<SpeedDb>,
    /// Where the next download takes its chapters from.
    pub download_source: Signal<SourceChoice>,
    /// Chapters waiting to download, the batch downloading now, and what finished.
    pub queue: Signal<DownloadQueue>,
    /// The series being planned for download, with their ticked chapters.
    pub plan: Signal<Vec<PlanEntry>>,
    /// Series whose details are being fetched right now.
    pub info_busy: Signal<HashSet<String>>,
    pub workspace: Signal<Option<Workspace>>,
    pub chapters: Signal<ChaptersState>,
    /// Chapters ticked in the table, by position in the merged list.
    pub selected: Signal<HashSet<usize>>,
    pub table: Signal<TableView>,
    pub preview: Signal<PreviewState>,
    pub export_opts: Signal<ExportOpts>,
    /// The running (or just finished) download or export.
    pub job: Signal<Option<JobView>>,
    /// Bumped when installed sources or test results change.
    pub sources_version: Signal<u32>,
    /// Source ids being tested right now.
    pub testing: Signal<HashSet<String>>,
    /// True while "install all recommended" is running.
    pub installing: Signal<bool>,
}

impl AppState {
    /// Create the state once, inside the app's root component, loading saved
    /// files.
    ///
    /// The signals are owned by Dioxus's root scope rather than by the
    /// component that creates them. Background tasks (downloads, searches,
    /// retests) run in the root scope and write to this state, and a signal
    /// may only be used from its owner or the owner's descendants.
    pub fn init() -> Self {
        use_hook(Self::build)
    }

    fn build() -> Self {
        fn sig<T: 'static>(value: T) -> Signal<T> {
            Signal::new_in_scope(value, ScopeId::ROOT)
        }
        let sources: SourceState = JsonFile::new(paths().sources_file()).load();
        let first_page = if sources.onboarding_done {
            Page::Search
        } else {
            Page::Setup
        };
        Self {
            boot: sig(BootState::Starting {
                message: "Starting…".into(),
                percent: None,
            }),
            services: sig(None),
            settings: sig(JsonFile::<Settings>::new(paths().settings_file()).load()),
            sources: sig(sources),
            health: sig(JsonFile::<HealthDb>::new(paths().health_file()).load()),
            page: sig(first_page),
            search: sig(SearchView::default()),
            search_sort: sig(SearchSort::default()),
            settings_section: sig(SettingsSection::default()),
            preset_lab: sig(PresetLab::default()),
            covers: sig(HashMap::new()),
            notice: sig(None),
            modal: sig(None),
            library: sig(JsonFile::<LibraryDb>::new(paths().library_file()).load()),
            exports: sig(JsonFile::<ExportsDb>::new(paths().exports_file()).load()),
            presets: sig(JsonFile::<PresetsDb>::new(paths().presets_file()).load()),
            speeds: sig(JsonFile::<SpeedDb>::new(paths().speeds_file()).load()),
            download_source: sig(SourceChoice::Adaptive),
            queue: sig(DownloadQueue::default()),
            plan: sig(Vec::new()),
            info_busy: sig(HashSet::new()),
            workspace: sig(None),
            chapters: sig(ChaptersState::Idle),
            selected: sig(HashSet::new()),
            table: sig(TableView::default()),
            preview: sig(PreviewState::default()),
            export_opts: sig(ExportOpts::default()),
            job: sig(None),
            sources_version: sig(0),
            testing: sig(HashSet::new()),
            installing: sig(false),
        }
    }

    pub fn client(&self) -> Option<Arc<SuwayomiClient>> {
        self.services.read().as_ref().map(|s| s.client.clone())
    }

    pub fn go(&self, page: Page) {
        let mut p = self.page;
        p.set(page);
    }

    /// Switch tab within the open series.
    pub fn open_tab(&self, tab: SeriesTab) {
        let mut ws = self.workspace;
        if let Some(w) = ws.write().as_mut() {
            w.tab = tab;
        };
    }

    pub fn notify(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::info!("{text}");
        self.show_notice(text, false);
    }

    /// Show a message, and take it away again after a little while.
    fn show_notice(&self, text: String, error: bool) {
        let mut n = self.notice;
        n.set(Some(Notice {
            text: text.clone(),
            error,
        }));
        let seconds = if error { 10 } else { 4 };
        let this = *self;
        spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
            let same = this.notice.peek().as_ref().is_some_and(|n| n.text == text);
            if same {
                let mut n = this.notice;
                n.set(None);
            }
        });
    }

    pub fn error(&self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.show_notice(text, true);
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

    pub fn save_library(&self) {
        let _ = JsonFile::new(paths().library_file()).save(&*self.library.read());
    }

    /// Change the presets. (A signal can only be written through a mutable
    /// binding, which `&self` does not give.)
    pub fn edit_presets<R>(&self, edit: impl FnOnce(&mut PresetsDb) -> R) -> R {
        let mut presets = self.presets;
        let mut guard = presets.write();
        edit(&mut guard)
    }

    /// Change the download queue.
    pub fn edit_queue<R>(&self, edit: impl FnOnce(&mut DownloadQueue) -> R) -> R {
        let mut queue = self.queue;
        let mut guard = queue.write();
        edit(&mut guard)
    }

    /// Change the saved source speeds.
    pub fn edit_speeds<R>(&self, edit: impl FnOnce(&mut SpeedDb) -> R) -> R {
        let mut speeds = self.speeds;
        let mut guard = speeds.write();
        edit(&mut guard)
    }

    pub fn save_speeds(&self) {
        let _ = JsonFile::new(paths().speeds_file()).save(&*self.speeds.read());
    }

    pub fn save_presets(&self) {
        let _ = JsonFile::new(paths().presets_file()).save(&*self.presets.read());
    }

    pub fn save_exports(&self) {
        let _ = JsonFile::new(paths().exports_file()).save(&*self.exports.read());
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
