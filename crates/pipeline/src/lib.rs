//! Turning chosen chapters into files on the e-reader.
//!
//! * [`fetch`]: downloading pages with retries and a disk cache, so an
//!   interrupted job resumes without downloading everything again
//! * [`job`]: the whole run: fetch, convert, assemble XTC/XTCH, hand to export
//! * [`export`]: writing to a folder, a mounted SD card or a WebDAV share
//! * [`store`]: remembering jobs so failed ones can be retried

pub mod error;
pub mod export;
pub mod fetch;
pub mod job;
pub mod options;
pub mod store;

pub use error::PipelineError;
pub use export::{ExportTarget, ExportedFile};
pub use fetch::{PageCache, RetryPolicy};
pub use job::{
    requests_from_merged, run_job, CancelFlag, ChapterRef, ChapterRequest, JobEvent, JobOutcome,
    JobSpec, OnMissing,
};
pub use options::process_options;
pub use store::{JobRecord, JobState, JobStore};
