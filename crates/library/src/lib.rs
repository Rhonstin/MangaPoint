//! Everything MangaPoint remembers between runs, stored as small JSON files.
//!
//! * [`paths`]: where files live
//! * [`json`]: crash-safe load/save of one JSON document
//! * [`settings`]: user preferences (device, output, conversion defaults)
//! * [`sources`]: first-run choices and added extension stores
//! * [`health`]: source test results and real-use success/latency, which
//!   feed the search ranking

pub mod downloads;
pub mod exports;
pub mod health;
pub mod json;
pub mod paths;
pub mod settings;
pub mod sources;

pub use downloads::{
    chapter_key, remove_inside, series_id, DownloadedChapter, DownloadedSeries, LibraryDb,
    LibraryFiles,
};
pub use exports::{delete_exported_file, ExportRecord, ExportedFileRecord, ExportsDb};
pub use health::{HealthDb, StepTiming, TestRecord, Verdict};
pub use json::{JsonError, JsonFile};
pub use paths::AppPaths;
pub use settings::{ConversionSettings, ExportSettings, Settings};
pub use sources::{Choice, SourceState};
