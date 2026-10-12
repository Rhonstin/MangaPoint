//! The app's decisions that do not depend on the UI, so they can be tested.

use library::{Settings, Verdict};
use pipeline::{ChapterRequest, ExportTarget, JobEvent, JobSpec};
use search::{MergedChapter, ScoredMember};
use std::collections::HashSet;
use std::path::Path;

/// The best entry from each source, at most `limit` of them. `members` must
/// be ordered best first, as search results are.
///
/// A source can list one series twice (a duplicate page, another edition).
/// Loading and merging both adds nothing, and the screens key their lists on
/// the source, so each source appears once.
pub fn one_per_source(members: &[ScoredMember], limit: usize) -> Vec<ScoredMember> {
    let mut seen = HashSet::new();
    members
        .iter()
        .filter(|m| seen.insert(m.candidate.source_id.clone()))
        .take(limit)
        .cloned()
        .collect()
}

// ---- Choosing chapters -----------------------------------------------------

/// Indices of chapters matching text like `1-10, 15, 20-`.
///
/// `numbers` holds each chapter's number (`None` for unnumbered ones, which
/// text can never select). A bare number selects that chapter, `a-b` selects
/// the inclusive range and `a-` selects from `a` to the end.
pub fn select_by_text(text: &str, numbers: &[Option<f64>]) -> Result<Vec<usize>, String> {
    let mut wanted: Vec<(f64, f64)> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let parse = |s: &str| {
            s.trim()
                .parse::<f64>()
                .map_err(|_| format!("'{}' is not a chapter number", s.trim()))
        };
        match part.split_once('-') {
            None => {
                let n = parse(part)?;
                wanted.push((n, n));
            }
            Some((a, "")) => wanted.push((parse(a)?, f64::INFINITY)),
            Some((a, b)) => {
                let (a, b) = (parse(a)?, parse(b)?);
                if a > b {
                    return Err(format!("the range {a}-{b} runs backwards"));
                }
                wanted.push((a, b));
            }
        }
    }
    if wanted.is_empty() {
        return Err("type chapter numbers such as 1-10, 15".into());
    }
    Ok(numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let n = (*n)?;
            wanted
                .iter()
                .any(|&(a, b)| n >= a - 0.0005 && n <= b + 0.0005)
                .then_some(i)
        })
        .collect())
}

/// Indices of the `count` highest-numbered chapters.
pub fn latest(numbers: &[Option<f64>], count: usize) -> Vec<usize> {
    let mut numbered: Vec<(usize, f64)> = numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.map(|n| (i, n)))
        .collect();
    numbered.sort_by(|a, b| b.1.total_cmp(&a.1));
    numbered.into_iter().take(count).map(|(i, _)| i).collect()
}

// ---- Building a job --------------------------------------------------------

/// Where the user chose to send the result.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetChoice {
    /// The configured output folder (or the app's default one).
    Folder,
    /// A mounted reader or SD card.
    Reader(String),
    /// The reader's WebDAV share from Settings.
    WebDav,
}

pub fn export_target(
    choice: &TargetChoice,
    settings: &Settings,
    default_dir: &Path,
) -> Result<ExportTarget, String> {
    match choice {
        TargetChoice::Folder => Ok(ExportTarget::Folder {
            path: settings
                .export
                .output_dir
                .clone()
                .unwrap_or_else(|| default_dir.to_path_buf()),
        }),
        TargetChoice::Reader(mount) => Ok(ExportTarget::Device {
            mount: mount.into(),
            folder: settings.export.device_folder.clone(),
        }),
        TargetChoice::WebDav => match settings.export.webdav_url.as_deref() {
            Some(url) if !url.trim().is_empty() => Ok(ExportTarget::WebDav {
                base_url: url.trim().to_string(),
                folder: settings.export.device_folder.clone(),
            }),
            _ => Err("Set the reader's Wi-Fi address in Settings first.".into()),
        },
    }
}

/// A job for the chosen chapters of a series. `selected` indexes `merged`;
/// chapters go into the file in reading order whatever order they were ticked.
pub fn build_spec(
    title: &str,
    merged: &[MergedChapter],
    selected: &[usize],
    member_sources: &[String],
    settings: &Settings,
    target: ExportTarget,
) -> Option<JobSpec> {
    let mut picked: Vec<usize> = selected
        .iter()
        .copied()
        .filter(|&i| i < merged.len())
        .collect();
    picked.sort_unstable();
    picked.dedup();
    if picked.is_empty() {
        return None;
    }
    let chosen: Vec<MergedChapter> = picked.iter().map(|&i| merged[i].clone()).collect();
    let chapters: Vec<ChapterRequest> = pipeline::requests_from_merged(&chosen, member_sources);
    Some(JobSpec::new(title, chapters, settings.clone(), target))
}

// ---- Progress --------------------------------------------------------------

/// What the progress bar shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobProgress {
    pub chapter_index: usize,
    pub chapter_total: usize,
    pub chapter_name: String,
    pub pages_done: usize,
    pub pages_total: usize,
    pub note: Option<String>,
}

impl JobProgress {
    pub fn apply(&mut self, event: &JobEvent) {
        match event {
            JobEvent::ChapterStarted { index, total, name } => {
                self.chapter_index = *index;
                self.chapter_total = *total;
                self.chapter_name = name.clone();
                self.pages_done = 0;
                self.pages_total = 0;
                self.note = None;
            }
            JobEvent::PageDone { done, total, .. } => {
                self.pages_done = *done;
                self.pages_total = *total;
            }
            JobEvent::UsedFallback { to, .. } => {
                self.note = Some(format!("That source failed, trying {to}."));
            }
            JobEvent::ChapterSkipped { name, .. } => {
                self.note = Some(format!("Skipped {name}: no source could supply it."));
            }
            JobEvent::Writing { .. } => {
                self.pages_done = 0;
                self.pages_total = 0;
                self.note = Some("Writing the file…".into());
            }
        }
    }

    /// Overall progress, 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        if self.chapter_total == 0 {
            return 0.0;
        }
        let within = if self.pages_total > 0 {
            self.pages_done as f32 / self.pages_total as f32
        } else {
            0.0
        };
        ((self.chapter_index as f32 + within) / self.chapter_total as f32).clamp(0.0, 1.0)
    }

    pub fn label(&self) -> String {
        if self.chapter_total == 0 {
            return "Starting…".into();
        }
        let pages = if self.pages_total > 0 {
            format!(", page {} of {}", self.pages_done, self.pages_total)
        } else {
            String::new()
        };
        format!(
            "Chapter {} of {}{pages}",
            self.chapter_index + 1,
            self.chapter_total
        )
    }
}

// ---- Wording ---------------------------------------------------------------

pub fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "Working",
        Verdict::NeedsBrowser => "Needs browser support",
        Verdict::NeedsFlareSolverr => "Blocked by Cloudflare",
        Verdict::Failed => "Not working",
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} bytes")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// "just now", "5 minutes ago", "3 days ago".
pub fn ago(now_secs: u64, then_secs: u64) -> String {
    let s = now_secs.saturating_sub(then_secs);
    let (n, unit) = match s {
        0..=59 => return "just now".into(),
        60..=3599 => (s / 60, "minute"),
        3600..=86_399 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::chapter;
    use search::merge_chapters;

    fn nums(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|&n| Some(n)).collect()
    }

    fn member(source: &str, manga_id: i64, score: f32) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: manga_id,
                    title: "Solo Leveling".into(),
                    thumbnail_url: None,
                },
            ),
            score,
            enrichment: None,
        }
    }

    #[test]
    fn a_source_listing_a_series_twice_appears_once_with_its_best_entry() {
        // MangaFire lists the series twice; the better-scored entry comes first.
        let members = vec![
            member("fire", 1, 0.9),
            member("dex", 2, 0.8),
            member("fire", 3, 0.7),
            member("pill", 4, 0.6),
        ];
        let picked = one_per_source(&members, 10);
        let ids: Vec<_> = picked
            .iter()
            .map(|m| (m.candidate.source_id.as_str(), m.candidate.manga_id))
            .collect();
        assert_eq!(ids, [("fire", 1), ("dex", 2), ("pill", 4)]);
        // The limit applies after de-duplication.
        assert_eq!(one_per_source(&members, 2).len(), 2);
        assert!(one_per_source(&[], 4).is_empty());
    }

    #[test]
    fn chapter_text_selects_numbers_ranges_and_open_ends() {
        let n = nums(&[1.0, 2.0, 3.0, 4.0, 5.0, 10.0, 11.0, 12.0]);
        assert_eq!(select_by_text("2", &n).unwrap(), [1]);
        assert_eq!(select_by_text("2-4", &n).unwrap(), [1, 2, 3]);
        assert_eq!(select_by_text("1, 5, 11", &n).unwrap(), [0, 4, 6]);
        assert_eq!(select_by_text("10-", &n).unwrap(), [5, 6, 7]);
        assert_eq!(
            select_by_text("1-2, 2-3", &n).unwrap(),
            [0, 1, 2],
            "overlaps are not doubled"
        );
        assert_eq!(select_by_text("6-9", &n).unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn chapter_text_handles_halves_and_ignores_unnumbered() {
        let n = vec![Some(1.0), Some(1.5), Some(2.0), None];
        assert_eq!(select_by_text("1-2", &n).unwrap(), [0, 1, 2]);
        assert_eq!(select_by_text("1.5", &n).unwrap(), [1]);
        assert_eq!(
            select_by_text("0-", &n).unwrap(),
            [0, 1, 2],
            "unnumbered chapters are never matched"
        );
    }

    #[test]
    fn bad_chapter_text_gets_a_helpful_message() {
        let n = nums(&[1.0]);
        assert!(select_by_text("abc", &n).unwrap_err().contains("'abc'"));
        assert!(select_by_text("5-2", &n).unwrap_err().contains("backwards"));
        assert!(select_by_text("  ,  ", &n).is_err());
        assert!(select_by_text("", &n).is_err());
    }

    #[test]
    fn latest_picks_the_highest_numbers() {
        let n = vec![Some(1.0), Some(9.0), None, Some(5.0), Some(7.0)];
        assert_eq!(latest(&n, 2), [1, 4]);
        assert_eq!(latest(&n, 10).len(), 4);
        assert!(latest(&[], 3).is_empty());
    }

    #[test]
    fn export_targets_follow_the_choice_and_settings() {
        let mut s = Settings::default();
        let default_dir = Path::new("/exports");
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/exports".into()
            }
        );
        s.export.output_dir = Some("/mine".into());
        assert_eq!(
            export_target(&TargetChoice::Folder, &s, default_dir).unwrap(),
            ExportTarget::Folder {
                path: "/mine".into()
            }
        );
        assert_eq!(
            export_target(&TargetChoice::Reader("E:\\".into()), &s, default_dir).unwrap(),
            ExportTarget::Device {
                mount: "E:\\".into(),
                folder: "Manga".into()
            }
        );
        assert!(export_target(&TargetChoice::WebDav, &s, default_dir).is_err());
        s.export.webdav_url = Some(" http://192.168.3.3 ".into());
        assert_eq!(
            export_target(&TargetChoice::WebDav, &s, default_dir).unwrap(),
            ExportTarget::WebDav {
                base_url: "http://192.168.3.3".into(),
                folder: "Manga".into()
            }
        );
    }

    #[test]
    fn specs_contain_only_the_ticked_chapters_in_reading_order() {
        let merged = merge_chapters(&[
            vec![chapter(1, 1.0, 0), chapter(2, 2.0, 0), chapter(3, 3.0, 0)],
            vec![chapter(12, 2.0, 0)],
        ]);
        let sources = vec!["best".to_string(), "other".to_string()];
        let spec = build_spec(
            "Series",
            &merged,
            &[2, 1, 1, 99], // out of order, repeated, and out of range
            &sources,
            &Settings::default(),
            ExportTarget::Folder { path: "/o".into() },
        )
        .unwrap();
        let names: Vec<_> = spec.chapters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Chapter 2", "Chapter 3"]);
        // Chapter 2 can come from either source, best first.
        assert_eq!(spec.chapters[0].options.len(), 2);
        assert_eq!(spec.chapters[0].options[0].source_id, "best");
        assert_eq!(spec.chapters[1].options.len(), 1);

        assert!(build_spec(
            "S",
            &merged,
            &[],
            &sources,
            &Settings::default(),
            spec.target.clone()
        )
        .is_none());
    }

    #[test]
    fn progress_combines_chapter_and_page_position() {
        let mut p = JobProgress::default();
        assert_eq!(p.fraction(), 0.0);
        assert_eq!(p.label(), "Starting…");
        p.apply(&JobEvent::ChapterStarted {
            index: 1,
            total: 4,
            name: "Ch 2".into(),
        });
        assert_eq!(p.fraction(), 0.25);
        p.apply(&JobEvent::PageDone {
            chapter_index: 1,
            done: 5,
            total: 10,
        });
        assert!((p.fraction() - 0.375).abs() < 1e-6);
        assert_eq!(p.label(), "Chapter 2 of 4, page 5 of 10");
        p.apply(&JobEvent::UsedFallback {
            chapter_index: 1,
            from: "a".into(),
            to: "b".into(),
            reason: String::new(),
        });
        assert!(p.note.as_deref().unwrap().contains('b'));
        p.apply(&JobEvent::ChapterStarted {
            index: 2,
            total: 4,
            name: "Ch 3".into(),
        });
        assert_eq!(p.note, None, "the note clears with the next chapter");
        assert_eq!(p.pages_done, 0);
    }

    #[test]
    fn progress_never_leaves_zero_to_one() {
        let p = JobProgress {
            chapter_index: 9,
            chapter_total: 2,
            pages_done: 50,
            pages_total: 10,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn wording_helpers() {
        assert_eq!(
            verdict_label(Verdict::NeedsBrowser),
            "Needs browser support"
        );
        assert_eq!(human_bytes(999), "999 bytes");
        assert_eq!(human_bytes(1_500), "1.5 KB");
        assert_eq!(human_bytes(268_200_000), "268 MB");
        assert_eq!(human_bytes(1_200_000_000), "1.2 GB");
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1000, 940), "1 minute ago");
        assert_eq!(ago(10_000, 2_800), "2 hours ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400 * 3), "3 days ago");
        assert_eq!(
            ago(5, 50),
            "just now",
            "a clock set back is not a negative time"
        );
    }
}
