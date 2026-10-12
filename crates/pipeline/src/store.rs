//! Remembering jobs, so a failed or interrupted one can be retried.

use crate::job::{JobOutcome, JobSpec};
use library::{JsonError, JsonFile};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub series_title: String,
    pub chapters: usize,
    pub state: JobState,
    pub error: Option<String>,
    /// Where the finished files went.
    pub files: Vec<String>,
    /// Chapters that could not be downloaded, in plain words.
    pub skipped: Vec<String>,
    /// Seconds since the Unix epoch.
    pub updated_at: u64,
    /// Kept so the job can be run again with the same settings.
    pub spec: JobSpec,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct JobList {
    jobs: Vec<JobRecord>,
}

const KEEP: usize = 100;

pub struct JobStore {
    file: JsonFile<JobList>,
}

impl JobStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            file: JsonFile::new(path),
        }
    }

    pub fn all(&self) -> Vec<JobRecord> {
        self.file.load().jobs
    }

    pub fn get(&self, id: &str) -> Option<JobRecord> {
        self.all().into_iter().find(|j| j.id == id)
    }

    fn put(&self, record: JobRecord) -> Result<(), JsonError> {
        let mut list = self.file.load();
        list.jobs.retain(|j| j.id != record.id);
        list.jobs.push(record);
        let excess = list.jobs.len().saturating_sub(KEEP);
        list.jobs.drain(..excess);
        self.file.save(&list)
    }

    pub fn started(&self, spec: &JobSpec) -> Result<(), JsonError> {
        self.put(record(
            spec,
            JobState::Running,
            None,
            Vec::new(),
            Vec::new(),
        ))
    }

    pub fn finished(&self, spec: &JobSpec, outcome: &JobOutcome) -> Result<(), JsonError> {
        self.put(record(
            spec,
            JobState::Done,
            None,
            outcome.files.iter().map(|f| f.location.clone()).collect(),
            outcome
                .skipped
                .iter()
                .map(|s| format!("{}: {}", s.name, s.reason))
                .collect(),
        ))
    }

    pub fn failed(&self, spec: &JobSpec, error: &str, cancelled: bool) -> Result<(), JsonError> {
        let state = if cancelled {
            JobState::Cancelled
        } else {
            JobState::Failed
        };
        self.put(record(
            spec,
            state,
            Some(error.to_string()),
            Vec::new(),
            Vec::new(),
        ))
    }

    /// Jobs that were running when the app last closed did not finish; mark
    /// them failed so they appear as retryable. Call once at start-up.
    pub fn mark_interrupted(&self) -> Result<usize, JsonError> {
        let mut list = self.file.load();
        let mut n = 0;
        for j in list
            .jobs
            .iter_mut()
            .filter(|j| j.state == JobState::Running)
        {
            j.state = JobState::Failed;
            j.error = Some("the app was closed before this finished".into());
            n += 1;
        }
        if n > 0 {
            self.file.save(&list)?;
        }
        Ok(n)
    }

    /// Jobs worth offering a "Retry" button for, newest first.
    pub fn retryable(&self) -> Vec<JobRecord> {
        let mut v: Vec<_> = self
            .all()
            .into_iter()
            .filter(|j| matches!(j.state, JobState::Failed | JobState::Cancelled))
            .collect();
        v.reverse();
        v
    }
}

fn record(
    spec: &JobSpec,
    state: JobState,
    error: Option<String>,
    files: Vec<String>,
    skipped: Vec<String>,
) -> JobRecord {
    JobRecord {
        id: spec.id(),
        series_title: spec.series_title.clone(),
        chapters: spec.chapters.len(),
        state,
        error,
        files,
        skipped,
        updated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        spec: spec.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ExportTarget, ExportedFile};
    use crate::job::{ChapterRef, ChapterRequest, SkippedChapter};
    use library::Settings;

    fn spec(title: &str) -> JobSpec {
        JobSpec::new(
            title,
            vec![ChapterRequest {
                number: Some(1.0),
                name: "Chapter 1".into(),
                options: vec![ChapterRef {
                    source_id: "s".into(),
                    chapter_id: 1,
                }],
            }],
            Settings::default(),
            ExportTarget::Folder {
                path: "/out".into(),
            },
        )
    }

    fn store(tag: &str) -> (JobStore, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("mangapoint-jobs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (JobStore::new(dir.join("jobs.json")), dir)
    }

    #[test]
    fn jobs_are_recorded_through_their_life() {
        let (s, dir) = store("life");
        let sp = spec("Berserk");
        s.started(&sp).unwrap();
        assert_eq!(s.get(&sp.id()).unwrap().state, JobState::Running);

        let outcome = JobOutcome {
            files: vec![ExportedFile {
                location: "/out/Berserk.xtch".into(),
                bytes: 10,
            }],
            pages: 5,
            chapters_done: 1,
            skipped: vec![SkippedChapter {
                index: 1,
                name: "Chapter 2".into(),
                reason: "404".into(),
            }],
            source_use: vec![],
        };
        s.finished(&sp, &outcome).unwrap();
        let rec = s.get(&sp.id()).unwrap();
        assert_eq!(rec.state, JobState::Done);
        assert_eq!(rec.files, vec!["/out/Berserk.xtch"]);
        assert_eq!(rec.skipped, vec!["Chapter 2: 404"]);
        assert_eq!(s.all().len(), 1, "same job updates in place");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_and_cancelled_jobs_can_be_retried_with_their_original_spec() {
        let (s, dir) = store("retry");
        let a = spec("A");
        let b = spec("B");
        s.failed(&a, "network down", false).unwrap();
        s.failed(&b, "cancelled", true).unwrap();
        let retry = s.retryable();
        assert_eq!(retry.len(), 2);
        assert_eq!(retry[0].series_title, "B", "newest first");
        assert_eq!(retry[0].state, JobState::Cancelled);
        assert_eq!(retry[1].spec, a, "the spec survives a save and load");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn jobs_running_at_shutdown_become_retryable() {
        let (s, dir) = store("interrupted");
        let sp = spec("X");
        s.started(&sp).unwrap();
        assert_eq!(s.mark_interrupted().unwrap(), 1);
        assert_eq!(s.mark_interrupted().unwrap(), 0);
        let rec = s.get(&sp.id()).unwrap();
        assert_eq!(rec.state, JobState::Failed);
        assert!(rec.error.unwrap().contains("closed"));
        assert_eq!(s.retryable().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn history_is_capped() {
        let (s, dir) = store("cap");
        for i in 0..130 {
            s.started(&spec(&format!("Series {i}"))).unwrap();
        }
        let all = s.all();
        assert_eq!(all.len(), KEEP);
        assert_eq!(all.last().unwrap().series_title, "Series 129");
        let _ = std::fs::remove_dir_all(dir);
    }
}
