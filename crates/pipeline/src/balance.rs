//! Spreading chapters over the sources that offer them.
//!
//! Some sources cap their own speed (MangaFire hands out about two pages a
//! second however many are requested at once). When a chapter is offered by
//! several sources, downloading different chapters from different sources at
//! the same time adds their speeds together instead of queueing behind one.

use crate::types::ChapterRef;
use std::collections::HashMap;
use std::sync::Mutex;

/// A source this much slower than the fastest one seen in this job is not
/// given extra chapters: one chapter stuck on it would outlast all the rest.
const TOO_SLOW_FACTOR: f64 = 4.0;

#[derive(Default)]
struct Stats {
    /// Chapters being downloaded from the source right now.
    active: usize,
    pages: usize,
    seconds: f64,
}

impl Stats {
    fn speed(&self) -> Option<f64> {
        (self.seconds > 0.0 && self.pages > 0).then(|| self.pages as f64 / self.seconds)
    }
}

pub struct Balancer {
    enabled: bool,
    stats: Mutex<HashMap<String, Stats>>,
}

impl Balancer {
    /// With `enabled` off, chapters always try their sources in the given
    /// order (best first).
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            stats: Mutex::new(HashMap::new()),
        }
    }

    /// The order to try a chapter's sources in: the least busy first, best
    /// ranked on ties, and sources that proved far slower than the rest last.
    pub fn order<'a>(&self, options: &'a [ChapterRef]) -> Vec<&'a ChapterRef> {
        if !self.enabled || options.len() < 2 {
            return options.iter().collect();
        }
        let stats = self.stats.lock().unwrap();
        let fastest = options
            .iter()
            .filter_map(|o| stats.get(&o.source_id).and_then(Stats::speed))
            .fold(0.0_f64, f64::max);
        let mut ranked: Vec<(bool, usize, usize, &ChapterRef)> = options
            .iter()
            .enumerate()
            .map(|(position, option)| {
                let known = stats.get(&option.source_id);
                let too_slow = known
                    .and_then(Stats::speed)
                    .is_some_and(|speed| speed * TOO_SLOW_FACTOR < fastest);
                let active = known.map_or(0, |s| s.active);
                (too_slow, active, position, option)
            })
            .collect();
        ranked.sort_by_key(|(too_slow, active, position, _)| (*too_slow, *active, *position));
        ranked.into_iter().map(|(_, _, _, option)| option).collect()
    }

    /// A chapter started downloading from `source`.
    pub fn begin(&self, source: &str) {
        self.stats
            .lock()
            .unwrap()
            .entry(source.to_string())
            .or_default()
            .active += 1;
    }

    /// It stopped: `pages` arrived in `seconds` (zero pages if it failed).
    pub fn finish(&self, source: &str, pages: usize, seconds: f64) {
        let mut stats = self.stats.lock().unwrap();
        let entry = stats.entry(source.to_string()).or_default();
        entry.active = entry.active.saturating_sub(1);
        entry.pages += pages;
        entry.seconds += seconds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(sources: &[&str]) -> Vec<ChapterRef> {
        sources
            .iter()
            .enumerate()
            .map(|(i, s)| ChapterRef {
                source_id: s.to_string(),
                chapter_id: i as i64,
            })
            .collect()
    }

    fn first(b: &Balancer, opts: &[ChapterRef]) -> String {
        b.order(opts)[0].source_id.clone()
    }

    #[test]
    fn the_best_source_goes_first_until_it_is_busy() {
        let b = Balancer::new(true);
        let opts = options(&["a", "b", "c"]);
        assert_eq!(first(&b, &opts), "a");
        b.begin("a");
        assert_eq!(first(&b, &opts), "b", "a is busy, b is free");
        b.begin("b");
        assert_eq!(first(&b, &opts), "c");
        b.begin("c");
        assert_eq!(first(&b, &opts), "a", "all equally busy: best ranked wins");
        b.finish("a", 10, 5.0);
        assert_eq!(first(&b, &opts), "a", "a is free again");
    }

    #[test]
    fn a_chapter_with_one_source_has_no_choice() {
        let b = Balancer::new(true);
        b.begin("a");
        assert_eq!(b.order(&options(&["a"])).len(), 1);
    }

    #[test]
    fn switched_off_it_always_keeps_the_given_order() {
        let b = Balancer::new(false);
        let opts = options(&["a", "b"]);
        b.begin("a");
        assert_eq!(first(&b, &opts), "a");
    }

    #[test]
    fn every_source_stays_in_the_list_so_fallback_still_works() {
        let b = Balancer::new(true);
        b.begin("a");
        let order: Vec<String> = b
            .order(&options(&["a", "b", "c"]))
            .iter()
            .map(|o| o.source_id.clone())
            .collect();
        assert_eq!(order, ["b", "c", "a"]);
    }

    #[test]
    fn a_source_far_slower_than_the_rest_is_not_given_extra_chapters() {
        let b = Balancer::new(true);
        let opts = options(&["fast", "slow"]);
        b.begin("fast");
        b.finish("fast", 80, 10.0); // 8 pages/s
        b.begin("slow");
        b.finish("slow", 10, 25.0); // 0.4 pages/s
        b.begin("fast"); // the fast one is busy now...
        assert_eq!(
            first(&b, &opts),
            "fast",
            "...but waiting on it beats the slow one"
        );
    }

    #[test]
    fn a_source_only_somewhat_slower_still_shares_the_load() {
        let b = Balancer::new(true);
        let opts = options(&["a", "b"]);
        b.begin("a");
        b.finish("a", 80, 10.0); // 8 pages/s
        b.begin("b");
        b.finish("b", 40, 10.0); // 4 pages/s: within the factor
        b.begin("a");
        assert_eq!(first(&b, &opts), "b");
    }

    #[test]
    fn failed_downloads_do_not_make_a_source_look_fast_or_slow() {
        let b = Balancer::new(true);
        b.begin("a");
        b.finish("a", 0, 3.0);
        assert!(b.stats.lock().unwrap().get("a").unwrap().speed().is_none());
    }
}
