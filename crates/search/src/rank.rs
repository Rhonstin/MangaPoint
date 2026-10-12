//! Choosing the best source for a series, and ordering series by relevance.

use crate::cluster::{Candidate, Cluster};
use crate::normalize::normalize_title;
use library::HealthDb;
use std::collections::HashMap;
use strsim::jaro_winkler;

/// What we learned by looking at a candidate's chapter list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Enrichment {
    pub chapter_count: Option<u32>,
    /// Upload time of the newest chapter, milliseconds since the epoch.
    pub latest_upload_ms: Option<i64>,
}

pub struct RankContext<'a> {
    pub health: &'a HealthDb,
    /// Preferred source language, e.g. "en".
    pub language: &'a str,
    /// Source ids the user prefers.
    pub preferred_sources: &'a [String],
    pub now_ms: i64,
    /// What we know about candidates, by `(source id, manga id)`.
    pub enrichment: &'a HashMap<(String, i64), Enrichment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoredMember {
    pub candidate: Candidate,
    /// 0.0 to 1.0; higher is better.
    pub score: f32,
    pub enrichment: Option<Enrichment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RankedSeries {
    pub title: String,
    /// Sources carrying this series, best first.
    pub members: Vec<ScoredMember>,
    /// How well the series title matches the query, 0.0 to 1.0.
    pub relevance: f32,
}

impl RankedSeries {
    pub fn best(&self) -> &ScoredMember {
        &self.members[0]
    }
}

// Signal weights; they add up to 1.0.
const W_COMPLETE: f32 = 0.35;
const W_HEALTH: f32 = 0.25;
const W_FRESH: f32 = 0.15;
const W_LANGUAGE: f32 = 0.10;
const W_PREFERRED: f32 = 0.10;
const W_SPEED: f32 = 0.05;

const DAY_MS: f32 = 86_400_000.0;

/// Order the members of each cluster by usefulness, then the clusters by
/// how well they answer `query`.
pub fn rank_series(clusters: Vec<Cluster>, query: &str, ctx: &RankContext) -> Vec<RankedSeries> {
    let q = normalize_title(query);
    let mut out: Vec<RankedSeries> = clusters
        .into_iter()
        .map(|c| {
            let max_chapters = c
                .members
                .iter()
                .filter_map(|m| ctx.enrichment.get(&m.key())?.chapter_count)
                .max();
            let mut members: Vec<ScoredMember> = c
                .members
                .into_iter()
                .map(|m| {
                    let enrichment = ctx.enrichment.get(&m.key()).cloned();
                    let score = score_member(&m, enrichment.as_ref(), max_chapters, ctx);
                    ScoredMember {
                        candidate: m,
                        score,
                        enrichment,
                    }
                })
                .collect();
            // Stable: equal scores keep search order.
            members.sort_by(|a, b| b.score.total_cmp(&a.score));
            RankedSeries {
                relevance: relevance(&q, &c.norm),
                title: c.title,
                members,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.relevance
            .total_cmp(&a.relevance)
            // More sources carrying it means more popular.
            .then(b.members.len().cmp(&a.members.len()))
            .then(b.best().score.total_cmp(&a.best().score))
    });
    out
}

fn score_member(
    m: &Candidate,
    enrichment: Option<&Enrichment>,
    max_chapters: Option<u32>,
    ctx: &RankContext,
) -> f32 {
    let complete = match (enrichment.and_then(|e| e.chapter_count), max_chapters) {
        (Some(n), Some(max)) if max > 0 => n as f32 / max as f32,
        _ => 0.5,
    };
    let health = ctx.health.score(&m.source_id);
    let fresh = match enrichment.and_then(|e| e.latest_upload_ms) {
        Some(ms) if ms > 0 => {
            let age_days = ((ctx.now_ms - ms) as f32 / DAY_MS).max(0.0);
            // Within a month is fully fresh; a year or older counts for nothing.
            (1.0 - (age_days - 30.0).max(0.0) / 335.0).clamp(0.0, 1.0)
        }
        _ => 0.5,
    };
    let language = if m.lang == ctx.language {
        1.0
    } else if m.lang == "all" {
        0.8
    } else {
        0.0
    };
    let preferred = if ctx.preferred_sources.contains(&m.source_id) {
        1.0
    } else {
        0.0
    };
    let speed = match ctx.health.typical_ms(&m.source_id) {
        // 3 s or less is full marks, 30 s or more is none.
        Some(ms) => (1.0 - (ms - 3_000.0) / 27_000.0).clamp(0.0, 1.0),
        None => 0.5,
    };
    let score = W_COMPLETE * complete
        + W_HEALTH * health
        + W_FRESH * fresh
        + W_LANGUAGE * language
        + W_PREFERRED * preferred
        + W_SPEED * speed;
    // A source we know cannot be used right now sinks to the bottom.
    if health == 0.0 {
        score * 0.1
    } else {
        score
    }
}

/// Exact match 1.0, prefix 0.9, every query word present 0.75, otherwise a
/// fuzzy score capped at 0.5.
fn relevance(query: &str, title: &str) -> f32 {
    if query.is_empty() {
        return 0.5;
    }
    if query == title {
        return 1.0;
    }
    if title.starts_with(query) {
        return 0.9;
    }
    let title_words: Vec<&str> = title.split(' ').collect();
    if query.split(' ').all(|w| title_words.contains(&w)) {
        return 0.75;
    }
    (jaro_winkler(query, title) as f32 * 0.5).min(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{cluster, test_support::cand};
    use library::{StepTiming, TestRecord, Verdict};

    const NOW: i64 = 1_800_000_000_000;

    fn ctx<'a>(
        health: &'a HealthDb,
        enrichment: &'a HashMap<(String, i64), Enrichment>,
        preferred: &'a [String],
    ) -> RankContext<'a> {
        RankContext {
            health,
            language: "en",
            preferred_sources: preferred,
            now_ms: NOW,
            enrichment,
        }
    }

    fn en(chapters: u32, days_old: i64) -> Enrichment {
        Enrichment {
            chapter_count: Some(chapters),
            latest_upload_ms: Some(NOW - days_old * 86_400_000),
        }
    }

    fn test_record(verdict: Verdict, ms: u64) -> TestRecord {
        TestRecord {
            at: 0,
            verdict,
            steps: vec![StepTiming {
                name: "all".into(),
                ms,
                ok: true,
                detail: None,
            }],
            message: String::new(),
        }
    }

    /// Source ids carrying "Berserk", best first. Sources `a`, `b` and `c`
    /// are offered; any without an entry in `enr` have no chapter data.
    fn order(
        health: &HealthDb,
        enr: &[((&str, i64), Enrichment)],
        preferred: &[String],
    ) -> Vec<String> {
        let map = enr
            .iter()
            .map(|((s, id), e)| ((s.to_string(), *id), e.clone()))
            .collect();
        let list = vec![
            cand("a", 1, "Berserk"),
            cand("b", 2, "Berserk"),
            cand("c", 3, "Berserk"),
        ];
        let ranked = rank_series(cluster(&list), "berserk", &ctx(health, &map, preferred));
        ranked[0]
            .members
            .iter()
            .map(|m| m.candidate.source_id.clone())
            .collect()
    }

    fn best_source(
        health: &HealthDb,
        enr: &[((&str, i64), Enrichment)],
        preferred: &[String],
    ) -> String {
        order(health, enr, preferred).remove(0)
    }

    #[test]
    fn the_most_complete_source_wins_when_all_else_is_equal() {
        let h = HealthDb::default();
        let winner = best_source(
            &h,
            &[
                (("a", 1), en(100, 5)),
                (("b", 2), en(405, 5)),
                (("c", 3), en(300, 5)),
            ],
            &[],
        );
        assert_eq!(winner, "b");
    }

    #[test]
    fn a_failing_source_loses_to_a_slightly_less_complete_healthy_one() {
        let mut h = HealthDb::default();
        for _ in 0..5 {
            h.record_test("a", "A", test_record(Verdict::Failed, 100));
            h.record_test("b", "B", test_record(Verdict::Pass, 100));
        }
        let winner = best_source(&h, &[(("a", 1), en(405, 5)), (("b", 2), en(380, 5))], &[]);
        assert_eq!(winner, "b");
    }

    #[test]
    fn sources_needing_browser_support_sink_to_the_bottom() {
        let mut h = HealthDb::default();
        h.record_test("a", "A", test_record(Verdict::NeedsBrowser, 100));
        // `a` has by far the most chapters and is the freshest, yet is unusable.
        let ranked = order(&h, &[(("a", 1), en(900, 1)), (("b", 2), en(10, 300))], &[]);
        assert_eq!(
            ranked.last().unwrap(),
            "a",
            "an unusable source must come last: {ranked:?}"
        );
        assert_ne!(ranked[0], "a");
    }

    #[test]
    fn freshness_and_user_preference_break_close_calls() {
        let h = HealthDb::default();
        // Same chapter counts; b was updated recently, a a year ago.
        let w = best_source(&h, &[(("a", 1), en(200, 400)), (("b", 2), en(200, 2))], &[]);
        assert_eq!(w, "b");
        // A stated preference tips equal sources.
        let w = best_source(
            &h,
            &[(("a", 1), en(200, 2)), (("b", 2), en(200, 2))],
            &["a".to_string()],
        );
        assert_eq!(w, "a");
    }

    #[test]
    fn language_matters() {
        let h = HealthDb::default();
        let enr = HashMap::new();
        let mut french = cand("fr", 1, "Berserk");
        french.lang = "fr".into();
        let english = cand("en", 2, "Berserk");
        let ranked = rank_series(cluster(&[french, english]), "berserk", &ctx(&h, &enr, &[]));
        assert_eq!(ranked[0].best().candidate.source_id, "en");
    }

    #[test]
    fn scores_stay_within_zero_and_one() {
        let h = HealthDb::default();
        let enr: HashMap<_, _> = [(("a".to_string(), 1), en(500, 0))].into();
        let list = vec![cand("a", 1, "X Series")];
        let r = rank_series(
            cluster(&list),
            "x series",
            &ctx(&h, &enr, &["a".to_string()]),
        );
        let s = r[0].best().score;
        assert!((0.0..=1.0).contains(&s), "{s}");
    }

    #[test]
    fn series_are_ordered_by_relevance_then_by_how_many_sources_carry_them() {
        let h = HealthDb::default();
        let enr = HashMap::new();
        let list = vec![
            cand("a", 1, "Dragon Ball Super"), // prefix of the query? no: query is shorter
            cand("a", 2, "Super Dragon Ball Heroes"), // contains all words
            cand("a", 3, "Dragon Ball"),       // exact
            cand("b", 4, "Dragon Ball"),
            cand("a", 5, "Dragonar Academy"), // fuzzy only
        ];
        let ranked = rank_series(cluster(&list), "dragon ball", &ctx(&h, &enr, &[]));
        let titles: Vec<_> = ranked.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles[0], "Dragon Ball");
        assert_eq!(ranked[0].members.len(), 2);
        // Prefix match beats all-words-present, which beats fuzzy.
        assert_eq!(titles[1], "Dragon Ball Super");
        assert_eq!(titles[2], "Super Dragon Ball Heroes");
        assert_eq!(titles[3], "Dragonar Academy");
    }

    #[test]
    fn relevance_levels() {
        assert_eq!(relevance("one piece", "one piece"), 1.0);
        assert_eq!(relevance("one", "one piece"), 0.9);
        assert_eq!(relevance("piece", "one piece"), 0.75);
        assert!(relevance("zzz", "one piece") <= 0.5);
        assert_eq!(relevance("", "anything"), 0.5);
    }
}
