//! Choosing, installing and testing manga sources.
//!
//! * [`defaults`]: the built-in list offered on first run
//! * [`manager`]: install/skip answers, custom extension stores
//! * [`check`]: the source test behind the Sources page's "Retest" button

pub mod check;
pub mod defaults;
pub mod manager;

pub use check::{check_source, test_many, CheckOptions};
pub use defaults::{DefaultSource, Requires, DEFAULTS};
pub use manager::{InstalledSource, Offer, SourceManager, SourcesError};
