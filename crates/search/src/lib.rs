//! Searching many sources at once and showing each series once.
//!
//! The same series shows up in many sources with slightly different titles,
//! chapter counts and reliability. This crate
//!
//! 1. asks every installed source at the same time ([`fanout`]),
//! 2. groups results that are the same series ([`normalize`], [`cluster`]),
//! 3. picks the best source for each series ([`rank`]), and
//! 4. merges chapter lists so a missing or failing chapter can come from
//!    another source ([`merge`]).

pub mod cluster;
pub mod fanout;
pub mod merge;
pub mod normalize;
pub mod rank;

pub use cluster::{cluster, Candidate, Cluster};
pub use fanout::{enrich, search_stream, SearchEvent, SearchState};
pub use merge::{merge_chapters, missing_numbers, MergedChapter};
pub use rank::{rank_series, Enrichment, RankContext, RankedSeries, ScoredMember};
