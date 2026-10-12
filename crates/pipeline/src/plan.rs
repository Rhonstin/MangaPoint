//! Deciding which source supplies which chapter.
//!
//! A series stays consistent when its chapters come from as few sources as
//! possible, so sources are ranked by **coverage first**: how many of the
//! chosen chapters they have. Among the sources that cover (nearly) as much as
//! the best, the **fastest** wins. The others only fill gaps.

use crate::types::ChapterRequest;

/// A source within this share of the best coverage counts as equally complete,
/// so speed can decide between them.
const SAME_COVERAGE: f32 = 0.9;

/// Where the user wants chapters to come from.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum SourceChoice {
    /// The source with the most of the chapters, then the fastest.
    #[default]
    Adaptive,
    /// Only this source; chapters it does not have are left out.
    Only(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RankedSource {
    pub source_id: String,
    /// How many of the requested chapters the source can supply.
    pub coverage: usize,
    /// Pages per second, if it has been measured.
    pub speed: Option<f32>,
}

/// How many of `requests` each source can supply, in order of first appearance.
pub fn coverage(requests: &[ChapterRequest]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for request in requests {
        let mut seen: Vec<&str> = Vec::new();
        for option in &request.options {
            // A source listing a chapter twice still counts once.
            if seen.contains(&option.source_id.as_str()) {
                continue;
            }
            seen.push(&option.source_id);
            match counts.iter_mut().find(|(id, _)| *id == option.source_id) {
                Some((_, n)) => *n += 1,
                None => counts.push((option.source_id.clone(), 1)),
            }
        }
    }
    counts
}

/// Rank the sources for these chapters, best first.
pub fn rank_sources(
    requests: &[ChapterRequest],
    speed_of: impl Fn(&str) -> Option<f32>,
) -> Vec<RankedSource> {
    let mut ranked: Vec<RankedSource> = coverage(requests)
        .into_iter()
        .map(|(source_id, coverage)| RankedSource {
            speed: speed_of(&source_id),
            source_id,
            coverage,
        })
        .collect();
    let best = ranked.iter().map(|r| r.coverage).max().unwrap_or(0) as f32;
    let top = |r: &RankedSource| r.coverage as f32 >= SAME_COVERAGE * best;
    // A measured source beats an unmeasured one; faster beats slower.
    let faster_first = |a: &RankedSource, b: &RankedSource| match (a.speed, b.speed) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    };
    ranked.sort_by(|a, b| match (top(a), top(b)) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (true, true) => faster_first(a, b)
            .then(b.coverage.cmp(&a.coverage))
            .then_with(|| a.source_id.cmp(&b.source_id)),
        (false, false) => b
            .coverage
            .cmp(&a.coverage)
            .then(faster_first(a, b))
            .then_with(|| a.source_id.cmp(&b.source_id)),
    });
    ranked
}

/// The sources that cover about as much as the best one: the ones worth a
/// speed test.
pub fn top_tier(ranked: &[RankedSource]) -> Vec<&RankedSource> {
    let best = ranked.iter().map(|r| r.coverage).max().unwrap_or(0) as f32;
    ranked
        .iter()
        .filter(|r| r.coverage as f32 >= SAME_COVERAGE * best)
        .collect()
}

/// The requests after applying the user's choice.
#[derive(Debug, PartialEq)]
pub struct Chosen {
    /// What to download, each with its sources in the order to try them.
    pub requests: Vec<ChapterRequest>,
    /// For each request, its position in the list that was passed in.
    pub kept: Vec<usize>,
    /// Positions the chosen source could not supply (only with `Only`).
    pub unavailable: Vec<usize>,
}

pub fn apply_choice(
    requests: Vec<ChapterRequest>,
    choice: &SourceChoice,
    ranked: &[RankedSource],
) -> Chosen {
    let mut chosen = Chosen {
        requests: Vec::new(),
        kept: Vec::new(),
        unavailable: Vec::new(),
    };
    for (index, mut request) in requests.into_iter().enumerate() {
        match choice {
            SourceChoice::Adaptive => {
                let position = |source: &str| {
                    ranked
                        .iter()
                        .position(|r| r.source_id == source)
                        .unwrap_or(usize::MAX)
                };
                // Stable, so a source listed twice keeps its own order.
                request
                    .options
                    .sort_by_key(|option| position(&option.source_id));
            }
            SourceChoice::Only(source) => {
                request.options.retain(|option| &option.source_id == source);
                if request.options.is_empty() {
                    chosen.unavailable.push(index);
                    continue;
                }
            }
        }
        chosen.kept.push(index);
        chosen.requests.push(request);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChapterRef;

    fn req(number: f64, sources: &[&str]) -> ChapterRequest {
        ChapterRequest {
            number: Some(number),
            name: format!("Ch {number}"),
            key: None,
            options: sources
                .iter()
                .enumerate()
                .map(|(i, s)| ChapterRef {
                    source_id: s.to_string(),
                    chapter_id: number as i64 * 10 + i as i64,
                })
                .collect(),
        }
    }

    fn ids(ranked: &[RankedSource]) -> Vec<&str> {
        ranked.iter().map(|r| r.source_id.as_str()).collect()
    }

    #[test]
    fn coverage_counts_each_source_once_per_chapter() {
        let requests = vec![
            req(1.0, &["a", "b"]),
            req(2.0, &["a"]),
            req(3.0, &["a", "a"]),
        ];
        assert_eq!(
            coverage(&requests),
            [("a".to_string(), 3), ("b".to_string(), 1)]
        );
    }

    #[test]
    fn the_source_with_the_most_chapters_ranks_first_even_if_slower() {
        let requests: Vec<_> = (1..=10)
            .map(|n| {
                if n <= 9 {
                    req(n as f64, &["complete", "partial"])
                } else {
                    req(n as f64, &["complete"])
                }
            })
            .collect();
        let speeds = |s: &str| Some(if s == "complete" { 2.0 } else { 20.0 });
        // "partial" has 9 of 10 chapters: within the tier, so speed decides.
        let r = rank_sources(&requests, speeds);
        assert_eq!(ids(&r), ["partial", "complete"]);

        // With only 5 of 10 it is not close, so the complete source wins.
        let requests: Vec<_> = (1..=10)
            .map(|n| {
                if n <= 5 {
                    req(n as f64, &["complete", "half"])
                } else {
                    req(n as f64, &["complete"])
                }
            })
            .collect();
        let r = rank_sources(&requests, |s| {
            Some(if s == "complete" { 2.0 } else { 20.0 })
        });
        assert_eq!(ids(&r), ["complete", "half"]);
    }

    #[test]
    fn equally_complete_sources_are_split_by_speed_and_unmeasured_ones_go_last() {
        let requests = vec![req(1.0, &["a", "b", "c"]), req(2.0, &["a", "b", "c"])];
        let r = rank_sources(&requests, |s| match s {
            "a" => Some(3.0),
            "b" => Some(9.0),
            _ => None,
        });
        assert_eq!(ids(&r), ["b", "a", "c"]);
        assert_eq!(top_tier(&r).len(), 3);
    }

    #[test]
    fn the_top_tier_leaves_out_sources_that_cover_little() {
        let requests: Vec<_> = (1..=10)
            .map(|n| {
                if n == 1 {
                    req(n as f64, &["big", "tiny"])
                } else {
                    req(n as f64, &["big"])
                }
            })
            .collect();
        let r = rank_sources(&requests, |_| None);
        assert_eq!(
            top_tier(&r)
                .iter()
                .map(|s| s.source_id.as_str())
                .collect::<Vec<_>>(),
            ["big"]
        );
    }

    #[test]
    fn adaptive_orders_each_chapters_sources_by_rank_and_keeps_every_chapter() {
        // Solo Leveling shape: whole numbers on one source, ".1" on another.
        let requests = vec![
            req(1.0, &["buddy", "asura"]),
            req(1.5, &["buddy"]),
            req(2.0, &["asura"]),
        ];
        let ranked = rank_sources(&requests, |s| Some(if s == "asura" { 1.0 } else { 15.0 }));
        let chosen = apply_choice(requests, &SourceChoice::Adaptive, &ranked);
        assert_eq!(chosen.kept, [0, 1, 2]);
        assert!(chosen.unavailable.is_empty());
        let first: Vec<_> = chosen
            .requests
            .iter()
            .map(|r| r.options[0].source_id.as_str())
            .collect();
        assert_eq!(first, ["buddy", "buddy", "asura"]);
        assert_eq!(
            chosen.requests[0].options.len(),
            2,
            "the other source stays as a fallback"
        );
    }

    #[test]
    fn only_one_source_drops_the_others_and_reports_what_it_lacks() {
        let requests = vec![req(1.0, &["a", "b"]), req(2.0, &["b"]), req(3.0, &["a"])];
        let chosen = apply_choice(requests, &SourceChoice::Only("a".into()), &[]);
        assert_eq!(chosen.kept, [0, 2]);
        assert_eq!(chosen.unavailable, [1]);
        assert!(chosen
            .requests
            .iter()
            .all(|r| r.options.len() == 1 && r.options[0].source_id == "a"));
    }

    #[test]
    fn an_empty_request_list_is_fine() {
        assert!(rank_sources(&[], |_| None).is_empty());
        let chosen = apply_choice(Vec::new(), &SourceChoice::Adaptive, &[]);
        assert!(chosen.requests.is_empty());
    }
}
