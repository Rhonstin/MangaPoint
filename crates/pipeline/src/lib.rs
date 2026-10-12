//! Turning chosen chapters into files on the e-reader, in three stages:
//!
//! 1. [`download`]: save chapters into the library (resumable, with
//!    fallback to another source)
//! 2. [`preview`]: show how a saved page will look on the reader, using the
//!    very conversion the export uses
//! 3. [`compose`]: build reader files from the saved chapters, with no
//!    network, then send them with [`export`] (folder, SD card, Wi-Fi)

pub mod compose;
pub mod download;
pub mod error;
pub mod export;
pub mod fetch;
pub mod options;
pub mod preview;
pub mod types;

pub use compose::{export_series, ExportJob, ExportOutcome, Grouping};
pub use download::{download_chapters, DownloadOutcome};
pub use error::PipelineError;
pub use export::{ExportTarget, ExportedFile};
pub use fetch::RetryPolicy;
pub use options::{process_options, process_options_for};
pub use preview::{decode_page, levels_to_previews, render_levels, render_original, PreviewImage};
pub use types::{
    requests_from_merged, CancelFlag, ChapterRef, ChapterRequest, JobEvent, SkippedChapter,
    SourceUse,
};
