//! The download queue.
//!
//! Chapters picked for download are added here as a batch for their series and
//! the queue works through the batches one after another, so there is nothing
//! to start by hand: keep picking chapters, from this series or others, and
//! they download in turn. Everything here is plain data and rules; the work of
//! actually downloading is in `ui/work.rs`.

use crate::logic::JobProgress;
use pipeline::{CancelFlag, ChapterRequest, SourceChoice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchStatus {
    Waiting,
    Running,
    /// Set aside by the user; it goes on from the chapters not yet saved when
    /// it is resumed.
    Paused,
    Done,
    /// Stopped by an error, in words.
    Failed(String),
    Cancelled,
}

impl BatchStatus {
    /// Waiting, paused or running: still has work to do.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            BatchStatus::Waiting | BatchStatus::Running | BatchStatus::Paused
        )
    }
}

/// How a finished batch went.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchStats {
    /// How long it took.
    pub secs: u64,
    /// Pages saved.
    pub pages: usize,
}

/// Some chapters of one series, downloaded together.
#[derive(Clone)]
pub struct Batch {
    pub id: u64,
    pub series_id: String,
    pub title: String,
    /// What to download, each chapter with the sources that can supply it.
    pub requests: Vec<ChapterRequest>,
    /// The library key of each request, in the same order.
    pub plan: Vec<String>,
    pub choice: SourceChoice,
    /// Source id to name, for the sources the chapters may come from.
    pub source_names: HashMap<String, String>,
    pub status: BatchStatus,
    pub progress: JobProgress,
    pub cancel: CancelFlag,
    /// Told to stop so that it can be put aside, not given up.
    pub pausing: bool,
    /// One line about how it went, once it has finished.
    pub summary: Option<String>,
    /// How long it took and how much it saved, once it has finished.
    pub stats: Option<BatchStats>,
    /// Chapters that could not be downloaded, in words.
    pub skipped: Vec<String>,
    /// The same chapters, by key.
    pub failed: Vec<String>,
    /// Seconds since the Unix epoch.
    pub added_at: u64,
    /// When it finished, in the same seconds; 0 until it has.
    pub finished_at: u64,
}

impl Batch {
    pub fn new(
        series_id: String,
        title: String,
        requests: Vec<ChapterRequest>,
        plan: Vec<String>,
        choice: SourceChoice,
        source_names: HashMap<String, String>,
        added_at: u64,
    ) -> Self {
        debug_assert_eq!(requests.len(), plan.len());
        Self {
            id: 0,
            series_id,
            title,
            requests,
            plan,
            choice,
            source_names,
            status: BatchStatus::Waiting,
            progress: JobProgress::default(),
            cancel: CancelFlag::new(),
            pausing: false,
            summary: None,
            stats: None,
            skipped: Vec::new(),
            failed: Vec::new(),
            added_at,
            finished_at: 0,
        }
    }

    pub fn chapter_count(&self) -> usize {
        self.plan.len()
    }

    /// Chapters dealt with so far.
    pub fn chapters_done(&self) -> usize {
        match self.status {
            BatchStatus::Done => self.plan.len() - self.failed.len().min(self.plan.len()),
            _ => self.progress.finished,
        }
    }
}

/// Where one chapter stands in the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueuedChapter {
    Waiting,
    Downloading {
        pages_done: usize,
        pages_total: usize,
    },
    Failed,
}

/// What adding a batch did.
#[derive(Debug, PartialEq, Eq)]
pub struct Added {
    /// Chapters that were new to the queue.
    pub chapters: usize,
    /// They joined a batch for the same series that was already waiting.
    pub joined_existing: bool,
}

/// A batch as it is kept on disk.
#[derive(Clone, Serialize, Deserialize)]
struct SavedBatch {
    id: u64,
    series_id: String,
    title: String,
    requests: Vec<ChapterRequest>,
    plan: Vec<String>,
    /// The one source it was limited to, if it was.
    only_source: Option<String>,
    source_names: HashMap<String, String>,
    status: BatchStatus,
    summary: Option<String>,
    stats: Option<BatchStats>,
    skipped: Vec<String>,
    failed: Vec<String>,
    added_at: u64,
    finished_at: u64,
}

/// The queue as it is kept on disk, so a restart does not lose it.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedQueue {
    batches: Vec<SavedBatch>,
    next_id: u64,
    paused: bool,
}

#[derive(Clone, Default)]
pub struct DownloadQueue {
    pub batches: Vec<Batch>,
    next_id: u64,
    /// While paused nothing new starts; a running batch carries on.
    pub paused: bool,
    /// A task is working through the queue right now.
    pub runner_active: bool,
}

impl DownloadQueue {
    pub fn to_saved(&self) -> SavedQueue {
        SavedQueue {
            batches: self
                .batches
                .iter()
                .map(|b| SavedBatch {
                    id: b.id,
                    series_id: b.series_id.clone(),
                    title: b.title.clone(),
                    requests: b.requests.clone(),
                    plan: b.plan.clone(),
                    only_source: match &b.choice {
                        SourceChoice::Adaptive => None,
                        SourceChoice::Only(id) => Some(id.clone()),
                    },
                    source_names: b.source_names.clone(),
                    status: b.status.clone(),
                    summary: b.summary.clone(),
                    stats: b.stats,
                    skipped: b.skipped.clone(),
                    failed: b.failed.clone(),
                    added_at: b.added_at,
                    finished_at: b.finished_at,
                })
                .collect(),
            next_id: self.next_id,
            paused: self.paused,
        }
    }

    /// Bring a saved queue back. A batch that was downloading when the app
    /// closed waits to start again; what it had already saved is kept.
    pub fn from_saved(saved: SavedQueue) -> Self {
        let batches = saved
            .batches
            .into_iter()
            .map(|mut b| {
                // A file edited by hand must not bring the app down.
                let n = b.requests.len().min(b.plan.len());
                b.requests.truncate(n);
                b.plan.truncate(n);
                let mut batch = Batch::new(
                    b.series_id,
                    b.title,
                    b.requests,
                    b.plan,
                    match b.only_source {
                        Some(id) => SourceChoice::Only(id),
                        None => SourceChoice::Adaptive,
                    },
                    b.source_names,
                    b.added_at,
                );
                batch.id = b.id;
                batch.status = match b.status {
                    BatchStatus::Running => BatchStatus::Waiting,
                    other => other,
                };
                batch.summary = b.summary;
                batch.stats = b.stats;
                batch.skipped = b.skipped;
                batch.failed = b.failed;
                batch.finished_at = b.finished_at;
                batch
            })
            .collect::<Vec<_>>();
        let next_id = saved
            .next_id
            .max(batches.iter().map(|b| b.id).max().unwrap_or(0));
        Self {
            batches,
            next_id,
            paused: saved.paused,
            runner_active: false,
        }
    }

    /// Add a batch. Chapters of the same series that are already waiting or
    /// downloading are left out, and what is new joins the series' waiting
    /// batch if it has one.
    pub fn add(&mut self, mut batch: Batch) -> Added {
        let taken: Vec<&str> = self
            .batches
            .iter()
            .filter(|b| b.status.is_active() && b.series_id == batch.series_id)
            .flat_map(|b| b.plan.iter().map(String::as_str))
            .collect();
        let mut fresh_requests = Vec::new();
        let mut fresh_plan = Vec::new();
        for (request, key) in batch.requests.drain(..).zip(batch.plan.drain(..)) {
            if !taken.contains(&key.as_str()) && !fresh_plan.contains(&key) {
                fresh_requests.push(request);
                fresh_plan.push(key);
            }
        }
        let chapters = fresh_plan.len();
        if chapters == 0 {
            return Added {
                chapters: 0,
                joined_existing: false,
            };
        }
        if let Some(existing) = self.batches.iter_mut().find(|b| {
            b.status == BatchStatus::Waiting
                && b.series_id == batch.series_id
                && b.choice == batch.choice
        }) {
            existing.requests.extend(fresh_requests);
            existing.plan.extend(fresh_plan);
            existing.source_names.extend(batch.source_names);
            return Added {
                chapters,
                joined_existing: true,
            };
        }
        batch.requests = fresh_requests;
        batch.plan = fresh_plan;
        self.next_id += 1;
        batch.id = self.next_id;
        self.batches.push(batch);
        Added {
            chapters,
            joined_existing: false,
        }
    }

    pub fn get(&self, id: u64) -> Option<&Batch> {
        self.batches.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Batch> {
        self.batches.iter_mut().find(|b| b.id == id)
    }

    /// The batch to start next, unless the queue is paused or one is running.
    pub fn next_to_start(&self) -> Option<u64> {
        if self.paused || self.running().is_some() {
            return None;
        }
        self.batches
            .iter()
            .find(|b| b.status == BatchStatus::Waiting)
            .map(|b| b.id)
    }

    pub fn running(&self) -> Option<&Batch> {
        self.batches
            .iter()
            .find(|b| b.status == BatchStatus::Running)
    }

    pub fn start(&mut self, id: u64) {
        if let Some(b) = self.get_mut(id) {
            b.status = BatchStatus::Running;
        }
    }

    pub fn finish(
        &mut self,
        id: u64,
        status: BatchStatus,
        summary: String,
        skipped: Vec<String>,
        failed: Vec<String>,
    ) {
        if let Some(b) = self.get_mut(id) {
            b.status = status;
            b.finished_at = now_secs();
            b.summary = Some(summary);
            b.skipped = skipped;
            b.failed = failed;
        }
    }

    pub fn set_stats(&mut self, id: u64, stats: BatchStats) {
        if let Some(b) = self.get_mut(id) {
            b.stats = Some(stats);
        }
    }

    /// Stop a batch. One that has not started is simply dropped from the
    /// queue; one that is running is told to stop and ends as cancelled.
    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(at) = self.batches.iter().position(|b| b.id == id) else {
            return false;
        };
        match self.batches[at].status {
            BatchStatus::Waiting | BatchStatus::Paused => {
                self.batches.remove(at);
                true
            }
            BatchStatus::Running => {
                let b = &mut self.batches[at];
                b.cancel.cancel();
                b.progress.stopping = true;
                true
            }
            _ => false,
        }
    }

    /// Stop everything for one series: its waiting batches go, and its running
    /// one is told to stop.
    pub fn cancel_series(&mut self, series_id: &str) {
        self.batches.retain(|b| {
            b.series_id != series_id
                || !matches!(b.status, BatchStatus::Waiting | BatchStatus::Paused)
        });
        let running = self
            .running()
            .filter(|b| b.series_id == series_id)
            .map(|b| b.id);
        if let Some(id) = running {
            self.cancel(id);
        }
    }

    /// Stop everything: waiting batches go, the running one is told to stop.
    pub fn cancel_all(&mut self) {
        self.batches
            .retain(|b| !matches!(b.status, BatchStatus::Waiting | BatchStatus::Paused));
        if let Some(id) = self.running().map(|b| b.id) {
            self.cancel(id);
        }
    }

    /// Take a finished batch out of the list.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.batches.len();
        self.batches.retain(|b| b.id != id || b.status.is_active());
        self.batches.len() != before
    }

    pub fn clear_finished(&mut self) {
        self.batches.retain(|b| b.status.is_active());
    }

    /// Put a batch aside. A waiting one is paused at once; a running one is
    /// told to stop and is paused when it has (see [`DownloadQueue::park`]).
    pub fn pause(&mut self, id: u64) -> bool {
        let Some(b) = self.get_mut(id) else {
            return false;
        };
        match b.status {
            BatchStatus::Waiting => {
                b.status = BatchStatus::Paused;
                true
            }
            BatchStatus::Running => {
                b.pausing = true;
                b.cancel.cancel();
                b.progress.stopping = true;
                true
            }
            _ => false,
        }
    }

    /// A running batch that was told to pause has stopped: what is saved
    /// stays, and the chapters still to do wait to be resumed.
    pub fn park(&mut self, id: u64, saved: &std::collections::HashSet<String>) {
        let Some(b) = self.get_mut(id) else {
            return;
        };
        let keep: Vec<usize> = (0..b.plan.len())
            .filter(|&i| !saved.contains(&b.plan[i]))
            .collect();
        b.requests = keep.iter().map(|&i| b.requests[i].clone()).collect();
        b.plan = keep.iter().map(|&i| b.plan[i].clone()).collect();
        b.status = BatchStatus::Paused;
        b.pausing = false;
        b.progress = JobProgress::default();
        b.cancel = CancelFlag::new();
    }

    /// Let a paused batch take its turn again.
    pub fn resume(&mut self, id: u64) -> bool {
        match self.get_mut(id) {
            Some(b) if b.status == BatchStatus::Paused => {
                b.status = BatchStatus::Waiting;
                b.cancel = CancelFlag::new();
                true
            }
            _ => false,
        }
    }

    /// Move a waiting batch to the front of the line.
    pub fn prioritize(&mut self, id: u64) {
        let Some(at) = self.batches.iter().position(|b| b.id == id) else {
            return;
        };
        if self.batches[at].status != BatchStatus::Waiting {
            return;
        }
        let batch = self.batches.remove(at);
        let first_waiting = self
            .batches
            .iter()
            .position(|b| b.status == BatchStatus::Waiting)
            .unwrap_or(self.batches.len());
        self.batches.insert(first_waiting, batch);
    }

    /// Queue the chapters of a finished batch that could not be downloaded,
    /// to try again. Returns how many were queued.
    pub fn retry(&mut self, id: u64, now: u64) -> usize {
        let Some(old) = self.get(id) else {
            return 0;
        };
        if old.status.is_active() || old.failed.is_empty() {
            return 0;
        }
        let (mut requests, mut plan) = (Vec::new(), Vec::new());
        for key in &old.failed {
            if let Some(at) = old.plan.iter().position(|k| k == key) {
                requests.push(old.requests[at].clone());
                plan.push(key.clone());
            }
        }
        let batch = Batch::new(
            old.series_id.clone(),
            old.title.clone(),
            requests,
            plan,
            old.choice.clone(),
            old.source_names.clone(),
            now,
        );
        let queued = self.add(batch).chapters;
        // The retried chapters are no longer failures of the old batch.
        if let Some(old) = self.get_mut(id) {
            old.failed.clear();
        }
        queued
    }

    /// Batches running or waiting, and how many chapters are still to come
    /// (not counting those already finished in the running one).
    pub fn counts(&self) -> (usize, usize, usize) {
        let running = usize::from(self.running().is_some());
        let waiting = self
            .batches
            .iter()
            .filter(|b| b.status == BatchStatus::Waiting)
            .count();
        let chapters = self
            .batches
            .iter()
            .filter(|b| b.status.is_active())
            .map(|b| b.chapter_count().saturating_sub(b.progress.finished))
            .sum();
        (running, waiting, chapters)
    }

    /// Is there anything left to do?
    #[cfg(test)]
    pub fn has_work(&self) -> bool {
        self.batches.iter().any(|b| b.status.is_active())
    }

    /// Does the queue have work for this series?
    pub fn series_has_work(&self, series_id: &str) -> bool {
        self.batches
            .iter()
            .any(|b| b.status.is_active() && b.series_id == series_id)
    }

    /// Where a chapter of a series stands in the queue, if it is in it.
    /// Work still to do wins over an old failure.
    pub fn chapter_state(&self, series_id: &str, key: &str) -> Option<QueuedChapter> {
        let mut failed = false;
        for batch in self
            .batches
            .iter()
            .rev()
            .filter(|b| b.series_id == series_id)
        {
            let Some(index) = batch.plan.iter().position(|k| k == key) else {
                continue;
            };
            match &batch.status {
                BatchStatus::Waiting | BatchStatus::Paused => {
                    return Some(QueuedChapter::Waiting)
                }
                BatchStatus::Running => {
                    return Some(match batch.progress.active.get(&index) {
                        Some(a) => QueuedChapter::Downloading {
                            pages_done: a.pages_done,
                            pages_total: a.pages_total,
                        },
                        None => QueuedChapter::Waiting,
                    })
                }
                _ => failed |= batch.failed.iter().any(|k| k == key),
            }
        }
        failed.then_some(QueuedChapter::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipeline::ChapterRef;

    fn request(n: u32) -> ChapterRequest {
        ChapterRequest {
            number: Some(n as f64),
            name: format!("Chapter {n}"),
            key: Some(format!("c{n}")),
            options: vec![ChapterRef {
                source_id: "s".into(),
                chapter_id: n as i64,
            }],
        }
    }

    fn batch(series: &str, chapters: &[u32]) -> Batch {
        Batch::new(
            series.into(),
            series.to_uppercase(),
            chapters.iter().map(|&n| request(n)).collect(),
            chapters.iter().map(|n| format!("c{n}")).collect(),
            SourceChoice::Adaptive,
            HashMap::new(),
            0,
        )
    }

    #[test]
    fn batches_run_one_after_another_without_anyone_starting_them() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1, 2]));
        q.add(batch("b", &[1]));
        let first = q.next_to_start().unwrap();
        q.start(first);
        assert_eq!(q.next_to_start(), None, "one at a time");
        q.finish(first, BatchStatus::Done, "ok".into(), vec![], vec![]);
        let second = q.next_to_start().unwrap();
        assert_ne!(first, second);
        assert_eq!(q.get(second).unwrap().series_id, "b");
    }

    #[test]
    fn chapters_added_while_a_series_waits_join_its_batch_without_repeats() {
        let mut q = DownloadQueue::default();
        let first = q.add(batch("a", &[1, 2]));
        assert_eq!(
            first,
            Added {
                chapters: 2,
                joined_existing: false
            }
        );
        let more = q.add(batch("a", &[2, 3, 3, 4]));
        assert_eq!(
            more,
            Added {
                chapters: 2,
                joined_existing: true
            },
            "2 is already queued"
        );
        assert_eq!(q.batches.len(), 1);
        assert_eq!(q.batches[0].plan, ["c1", "c2", "c3", "c4"]);
        assert_eq!(
            q.batches[0].requests.len(),
            4,
            "requests stay aligned with the plan"
        );
        assert_eq!(q.add(batch("a", &[1, 4])).chapters, 0);
    }

    #[test]
    fn chapters_added_while_the_series_is_running_wait_in_a_new_batch() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1, 2]));
        let id = q.next_to_start().unwrap();
        q.start(id);
        let added = q.add(batch("a", &[2, 3]));
        assert_eq!(
            added,
            Added {
                chapters: 1,
                joined_existing: false
            },
            "2 is being downloaded"
        );
        assert_eq!(q.batches.len(), 2);
        assert_eq!(q.batches[1].plan, ["c3"]);
    }

    #[test]
    fn pausing_stops_new_batches_from_starting_but_not_the_running_one() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1]));
        q.add(batch("b", &[1]));
        let id = q.next_to_start().unwrap();
        q.start(id);
        q.paused = true;
        assert!(q.running().is_some());
        q.finish(id, BatchStatus::Done, String::new(), vec![], vec![]);
        assert_eq!(q.next_to_start(), None);
        q.paused = false;
        assert!(q.next_to_start().is_some());
    }

    #[test]
    fn cancelling_drops_a_waiting_batch_and_stops_a_running_one() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1]));
        q.add(batch("b", &[1]));
        let a = q.batches[0].id;
        let b = q.batches[1].id;
        q.start(a);
        assert!(q.cancel(b));
        assert!(q.get(b).is_none(), "the waiting one is gone");
        assert!(q.cancel(a));
        assert!(q.get(a).unwrap().cancel.is_cancelled());
        assert!(q.get(a).unwrap().progress.stopping);
        assert!(!q.cancel(999));
    }

    #[test]
    fn cancel_all_clears_the_line_and_stops_what_is_running() {
        let mut q = DownloadQueue::default();
        for s in ["a", "b", "c"] {
            q.add(batch(s, &[1]));
        }
        let a = q.batches[0].id;
        q.start(a);
        q.cancel_all();
        assert_eq!(q.batches.len(), 1);
        assert!(q.batches[0].cancel.is_cancelled());
    }

    #[test]
    fn stopping_one_series_leaves_the_others_alone() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1]));
        q.add(batch("b", &[1]));
        q.add(batch("a", &[5]));
        let first = q.batches[0].id;
        q.start(first); // "a", running; the joined chapter 5 is in the same batch
        q.add(batch("a", &[9])); // a second, waiting batch for "a"
        q.cancel_series("a");
        let left: Vec<&str> = q.batches.iter().map(|b| b.series_id.as_str()).collect();
        assert_eq!(
            left,
            ["a", "b"],
            "the running one stays (it is stopping), the waiting one went"
        );
        assert!(q.batches[0].cancel.is_cancelled());
        assert!(!q.batches[1].cancel.is_cancelled());
        assert!(q.series_has_work("b"));
    }

    #[test]
    fn a_batch_can_be_moved_to_the_front_of_the_line() {
        let mut q = DownloadQueue::default();
        for s in ["a", "b", "c"] {
            q.add(batch(s, &[1]));
        }
        let (a, c) = (q.batches[0].id, q.batches[2].id);
        q.start(a);
        q.prioritize(c);
        let order: Vec<&str> = q.batches.iter().map(|b| b.series_id.as_str()).collect();
        assert_eq!(
            order,
            ["a", "c", "b"],
            "after the running one, before the rest"
        );
        q.prioritize(a);
        assert_eq!(q.batches[0].series_id, "a", "a running batch is not moved");
    }

    #[test]
    fn finished_batches_can_be_cleared_and_retried() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1, 2, 3]));
        let id = q.next_to_start().unwrap();
        q.start(id);
        q.finish(
            id,
            BatchStatus::Done,
            "2 of 3".into(),
            vec!["Chapter 2: no source".into()],
            vec!["c2".into()],
        );
        assert_eq!(q.get(id).unwrap().chapters_done(), 2);
        assert!(!q.remove(999));

        let queued = q.retry(id, 5);
        assert_eq!(queued, 1);
        assert_eq!(q.batches.len(), 2);
        assert_eq!(q.batches[1].plan, ["c2"]);
        assert_eq!(q.batches[1].status, BatchStatus::Waiting);
        assert!(q.get(id).unwrap().failed.is_empty());
        assert_eq!(q.retry(id, 6), 0, "nothing left to retry");

        q.clear_finished();
        assert_eq!(
            q.batches.len(),
            1,
            "the finished one is gone, the waiting one stays"
        );
        assert!(q.has_work());
    }

    #[test]
    fn a_chapters_state_follows_the_queue_and_active_work_beats_an_old_failure() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1, 2]));
        assert_eq!(q.chapter_state("a", "c1"), Some(QueuedChapter::Waiting));
        assert_eq!(q.chapter_state("a", "c9"), None);
        assert_eq!(q.chapter_state("other", "c1"), None);

        let id = q.next_to_start().unwrap();
        q.start(id);
        q.get_mut(id)
            .unwrap()
            .progress
            .apply(&pipeline::JobEvent::ChapterStarted {
                index: 0,
                total: 2,
                name: "Chapter 1".into(),
            });
        q.get_mut(id)
            .unwrap()
            .progress
            .apply(&pipeline::JobEvent::PageDone {
                chapter_index: 0,
                done: 4,
                total: 10,
            });
        assert_eq!(
            q.chapter_state("a", "c1"),
            Some(QueuedChapter::Downloading {
                pages_done: 4,
                pages_total: 10
            })
        );
        assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Waiting));

        q.finish(
            id,
            BatchStatus::Done,
            String::new(),
            vec![],
            vec!["c2".into()],
        );
        assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Failed));
        // Trying again puts it back in the queue: that wins over the old failure.
        q.retry(id, 1);
        assert_eq!(q.chapter_state("a", "c2"), Some(QueuedChapter::Waiting));
    }

    #[test]
    fn counts_say_how_much_is_left() {
        let mut q = DownloadQueue::default();
        assert!(!q.has_work());
        q.add(batch("a", &[1, 2, 3]));
        q.add(batch("b", &[1, 2]));
        assert_eq!(q.counts(), (0, 2, 5));
        let id = q.next_to_start().unwrap();
        q.start(id);
        assert_eq!(q.counts(), (1, 1, 5));
        assert!(q.series_has_work("a") && q.series_has_work("b"));
        q.finish(id, BatchStatus::Cancelled, String::new(), vec![], vec![]);
        assert!(!q.series_has_work("a"));
        assert_eq!(q.counts(), (0, 1, 2));
    }

    #[test]
    fn a_saved_queue_comes_back_with_its_finished_batches_and_resumes_the_rest() {
        let mut q = DownloadQueue::default();
        for title in ["Done one", "Waits", "Was running"] {
            q.add(Batch::new(
                title.to_lowercase(),
                title.into(),
                vec![ChapterRequest {
                    number: Some(1.0),
                    name: "c".into(),
                    key: Some(format!("{title}-1")),
                    options: Vec::new(),
                }],
                vec![format!("{title}-1")],
                SourceChoice::Only("src".into()),
                HashMap::new(),
                10,
            ));
        }
        let (a, c) = (q.batches[0].id, q.batches[2].id);
        q.finish(
            a,
            BatchStatus::Done,
            "Downloaded 1 chapter".into(),
            Vec::new(),
            Vec::new(),
        );
        q.set_stats(a, BatchStats { secs: 9, pages: 40 });
        q.start(c);

        let json = serde_json::to_string(&q.to_saved()).unwrap();
        let back = DownloadQueue::from_saved(serde_json::from_str(&json).unwrap());
        assert_eq!(back.batches.len(), 3);
        assert_eq!(back.batches[0].status, BatchStatus::Done);
        assert_eq!(
            back.batches[0].stats,
            Some(BatchStats { secs: 9, pages: 40 })
        );
        assert!(back.batches[0].finished_at > 0);
        assert_eq!(
            back.batches[2].status,
            BatchStatus::Waiting,
            "it starts again"
        );
        assert_eq!(back.batches[1].choice, SourceChoice::Only("src".into()));
        assert!(!back.runner_active);

        // Lists that do not match are cut to the shorter, not trusted.
        let mut odd = q.to_saved();
        odd.batches[0].plan.push("extra".into());
        let odd = DownloadQueue::from_saved(odd);
        assert_eq!(odd.batches[0].plan.len(), odd.batches[0].requests.len());
    }

    #[test]
    fn a_batch_can_be_put_aside_and_goes_on_with_what_is_left() {
        let mut q = DownloadQueue::default();
        q.add(batch("a", &[1, 2, 3]));
        q.add(batch("b", &[1]));
        let (a, b) = (q.batches[0].id, q.batches[1].id);
        // A waiting batch is paused at once and is skipped by the line.
        assert!(q.pause(b));
        assert_eq!(q.next_to_start(), Some(a));
        // A running one is told to stop, and is parked with what is not saved.
        q.start(a);
        assert!(q.pause(a));
        assert!(q.get(a).unwrap().pausing);
        let saved: std::collections::HashSet<String> = [q.get(a).unwrap().plan[0].clone()].into();
        q.park(a, &saved);
        let parked = q.get(a).unwrap();
        assert_eq!(parked.status, BatchStatus::Paused);
        assert_eq!(parked.plan.len(), 2);
        assert!(!parked.pausing && !parked.cancel.is_cancelled());
        assert!(parked.status.is_active(), "it still counts as work to do");
        assert_eq!(q.next_to_start(), None, "nothing runs until one is resumed");
        assert!(q.resume(a));
        assert_eq!(q.next_to_start(), Some(a));
        // Giving a paused one up drops it like a waiting one.
        assert!(q.cancel(b));
        assert!(q.get(b).is_none());
    }
}
