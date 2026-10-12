//! The app's decisions that do not depend on the UI, so they can be tested.

use library::{Settings, Verdict};
use pipeline::{ExportTarget, JobEvent};
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

// ---- The chapter table -----------------------------------------------------

/// Which column the chapter table is sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Number,
    Name,
    Sources,
    Date,
}

/// What clicking a column header does: sorting by a new column uses that
/// column's natural direction (newest and biggest first, names A to Z);
/// clicking the current column flips the direction.
pub fn next_sort(current: (SortKey, bool), clicked: SortKey) -> (SortKey, bool) {
    if current.0 == clicked {
        (clicked, !current.1)
    } else {
        let descending = !matches!(clicked, SortKey::Name);
        (clicked, descending)
    }
}

/// Newest upload time among the sources that have the chapter (0 if unknown).
pub fn latest_upload(chapter: &MergedChapter) -> i64 {
    chapter
        .options
        .iter()
        .map(|o| o.upload_date)
        .max()
        .unwrap_or(0)
}

/// Indices of the chapters to show: those matching every word of `filter`
/// (in the name or the number), ordered by `key`. Chapters with no value for
/// the sort column (no number, no date) always come last.
pub fn visible_chapters(
    merged: &[MergedChapter],
    filter: &str,
    key: SortKey,
    descending: bool,
) -> Vec<usize> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<usize> = (0..merged.len())
        .filter(|&i| {
            let ch = &merged[i];
            let haystack = format!(
                "{} {}",
                ch.name.to_lowercase(),
                ch.number.map(format_number).unwrap_or_default()
            );
            words.iter().all(|w| haystack.contains(w.as_str()))
        })
        .collect();

    // Value to sort on; `None` sorts after everything else.
    let value = |i: usize| -> Option<f64> {
        let ch = &merged[i];
        match key {
            SortKey::Number => ch.number,
            SortKey::Sources => Some(ch.options.len() as f64),
            SortKey::Date => {
                let d = latest_upload(ch);
                (d > 0).then_some(d as f64)
            }
            SortKey::Name => Some(0.0),
        }
    };
    let (mut present, absent): (Vec<usize>, Vec<usize>) =
        shown.drain(..).partition(|&i| value(i).is_some());
    present.sort_by(|&a, &b| {
        let ord = if key == SortKey::Name {
            merged[a]
                .name
                .to_lowercase()
                .cmp(&merged[b].name.to_lowercase())
        } else {
            value(a).unwrap().total_cmp(&value(b).unwrap())
        };
        // Ties keep the chapters' own order (by number, then position).
        let ord = ord.then_with(|| {
            merged[a]
                .number
                .unwrap_or(f64::MAX)
                .total_cmp(&merged[b].number.unwrap_or(f64::MAX))
        });
        if descending {
            ord.reverse()
        } else {
            ord
        }
    });
    present.extend(absent);
    present
}

/// `12`, `12.5`: whole numbers without a decimal point.
pub fn format_number(n: f64) -> String {
    if n.fract().abs() < 0.0005 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// `2024-05-03` from milliseconds since the Unix epoch; empty if unknown.
/// (UTC; computed directly so no date library is needed.)
pub fn format_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let days = ms.div_euclid(86_400_000);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
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

// ---- Where files go --------------------------------------------------------

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

// ---- Steps within a series -------------------------------------------------

/// What a tab needs before it can be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    /// Chapters ticked, or something already downloaded.
    SelectionOrDownloads,
    /// At least one downloaded chapter.
    Downloads,
}

/// Why a tab cannot be opened yet, in words for a tooltip; `None` if it can.
pub fn tab_blocked(
    needs: Needs,
    selected: usize,
    downloaded: usize,
    downloading: bool,
) -> Option<&'static str> {
    match needs {
        Needs::Nothing => None,
        Needs::SelectionOrDownloads if selected > 0 || downloaded > 0 || downloading => None,
        Needs::SelectionOrDownloads => Some("Tick some chapters first"),
        Needs::Downloads if downloaded > 0 => None,
        Needs::Downloads => Some("Download a chapter first"),
    }
}

/// Where a chapter stands in the Downloads tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterStatus {
    Saved,
    Downloading,
    Waiting,
    Failed,
}

// ---- Progress --------------------------------------------------------------

/// One chapter being worked on right now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveChapter {
    pub name: String,
    pub pages_done: usize,
    pub pages_total: usize,
}

/// What the progress bar shows. Chapters can download together, so it keeps
/// every chapter in progress, not just one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobProgress {
    pub chapter_total: usize,
    /// Chapters completely dealt with.
    pub finished: usize,
    /// Chapters under way, by their position in the job.
    pub active: std::collections::BTreeMap<usize, ActiveChapter>,
    pub note: Option<String>,
    /// When the first chapter began, and how many pages have arrived since.
    pub started: Option<std::time::Instant>,
    pub pages_seen: usize,
    /// The user pressed cancel; the job is winding down.
    pub stopping: bool,
}

impl JobProgress {
    pub fn apply(&mut self, event: &JobEvent) {
        match event {
            JobEvent::ChapterStarted { index, total, name } => {
                self.chapter_total = *total;
                self.started.get_or_insert_with(std::time::Instant::now);
                self.active.insert(
                    *index,
                    ActiveChapter {
                        name: name.clone(),
                        ..Default::default()
                    },
                );
                self.note = None;
            }
            JobEvent::PageDone {
                chapter_index,
                done,
                total,
            } => {
                self.pages_seen += 1;
                let chapter = self.active.entry(*chapter_index).or_default();
                chapter.pages_done = *done;
                chapter.pages_total = *total;
            }
            JobEvent::UsedFallback { to, .. } => {
                self.note = Some(format!("That source failed, trying {to}."));
            }
            JobEvent::ChapterSkipped { name, .. } => {
                self.note = Some(format!("Skipped {name}: no source could supply it."));
            }
            JobEvent::ChapterSaved { .. } => {}
            JobEvent::ChapterDone { index } => {
                if self.active.remove(index).is_some() {
                    self.finished += 1;
                }
            }
            JobEvent::PagesAtOnce { now, chosen } => {
                self.note = (now < chosen).then(|| {
                    format!(
                        "The source is pushing back, so downloads are slowed to {now} at a time."
                    )
                });
            }
            JobEvent::Writing { .. } => {
                self.active.clear();
                self.finished = self.chapter_total;
                self.note = Some("Writing the file…".into());
            }
        }
    }

    /// How fast pages are arriving, once there is enough to say.
    pub fn pages_per_second(&self) -> Option<f32> {
        let seconds = self.started?.elapsed().as_secs_f32();
        (seconds >= 4.0 && self.pages_seen >= 6).then(|| self.pages_seen as f32 / seconds)
    }

    /// Overall progress, 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        if self.chapter_total == 0 {
            return 0.0;
        }
        let partial: f32 = self
            .active
            .values()
            .filter(|c| c.pages_total > 0)
            .map(|c| c.pages_done as f32 / c.pages_total as f32)
            .sum();
        ((self.finished as f32 + partial) / self.chapter_total as f32).clamp(0.0, 1.0)
    }

    pub fn label(&self) -> String {
        if self.stopping {
            return "Stopping…".into();
        }
        if self.chapter_total == 0 {
            return "Starting…".into();
        }
        match self.active.len() {
            0 if self.finished == 0 => "Starting…".into(),
            0 => format!("{} of {} chapters done", self.finished, self.chapter_total),
            1 => {
                let (index, chapter) = self.active.iter().next().expect("one entry");
                let pages = if chapter.pages_total > 0 {
                    format!(", page {} of {}", chapter.pages_done, chapter.pages_total)
                } else {
                    String::new()
                };
                format!("Chapter {} of {}{pages}", index + 1, self.chapter_total)
            }
            n => format!(
                "{} of {} chapters done, {n} in progress",
                self.finished, self.chapter_total
            ),
        }
    }
}

// ---- Export size -----------------------------------------------------------

/// A rough size for a reader file: each page is a fixed size in the format
/// (about 48 KB for black and white, 96 KB for four grays). Split spreads make
/// it a little bigger.
pub fn estimate_export_bytes(pages: usize, format: converter::xtc::Format) -> u64 {
    let per_page: u64 = match format {
        converter::xtc::Format::Xtc => 48_022,
        converter::xtc::Format::Xtch => 96_022,
    };
    pages as u64 * per_page
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

/// One line saying what a set of conversion settings does, for preset lists.
pub fn describe_settings(c: &library::ConversionSettings) -> String {
    use converter::process::{CropMode, Layout};
    use converter::xtc::{Format, ReadDirection};
    let direction = match c.direction {
        ReadDirection::RightToLeft => "right to left",
        ReadDirection::LeftToRight => "left to right",
        ReadDirection::TopToBottom => "top to bottom",
    };
    let layout = match c.layout {
        Layout::Auto => "automatic layout",
        Layout::Manga => "pages",
        Layout::Webtoon => "long strips",
    };
    let format = match c.format {
        Format::Xtch => "4 grays",
        Format::Xtc => "black & white",
    };
    let trim = match (c.crop, c.margins.is_none()) {
        (CropMode::Full, true) => "no cropping".to_string(),
        (CropMode::Full, false) => "fixed margins".to_string(),
        (CropMode::Margins, true) => "auto trim".to_string(),
        (CropMode::Margins, false) => "auto trim + fixed margins".to_string(),
    };
    format!(
        "{direction} · {layout} · {trim} · {format} · contrast {}",
        c.contrast
    )
}

/// Turn what a source says about a series into what the Library keeps.
pub fn series_info_from(
    details: &catalog::MangaDetails,
    source_names: Vec<(String, String)>,
    has_cover: bool,
    now: u64,
) -> library::SeriesInfo {
    let clean = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or("").to_string();
    let mut genres: Vec<String> = Vec::new();
    for g in &details.genre {
        let g = g.trim();
        if !g.is_empty() && !genres.iter().any(|x| x.eq_ignore_ascii_case(g)) {
            genres.push(g.to_string());
        }
    }
    // Descriptions often arrive with Windows line breaks and long runs of blank lines.
    let mut description = clean(&details.description).replace("\r\n", "\n");
    while description.contains("\n\n\n") {
        description = description.replace("\n\n\n", "\n\n");
    }
    library::SeriesInfo {
        description,
        author: clean(&details.author),
        artist: clean(&details.artist),
        status: library::status_label(details.status.as_deref().unwrap_or("")),
        genres,
        source_names,
        has_cover,
        saved_at: now,
    }
}

/// "1 chapter", "2 chapters".
pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The first and last chapter numbers of a list, if any are numbered.
pub fn number_range(numbers: impl Iterator<Item = Option<f64>>) -> Option<(f64, f64)> {
    numbers.flatten().fold(None, |range, n| match range {
        None => Some((n, n)),
        Some((lo, hi)) => Some((lo.min(n), hi.max(n))),
    })
}

/// How search results are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSort {
    /// How well the title matches, then how good the sources are.
    #[default]
    BestMatch,
    MostChapters,
    RecentlyUpdated,
    MostSources,
    TitleAz,
}

impl SearchSort {
    pub const ALL: [SearchSort; 5] = [
        SearchSort::BestMatch,
        SearchSort::MostChapters,
        SearchSort::RecentlyUpdated,
        SearchSort::MostSources,
        SearchSort::TitleAz,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "Best match",
            SearchSort::MostChapters => "Most chapters",
            SearchSort::RecentlyUpdated => "Recently updated",
            SearchSort::MostSources => "Most sources",
            SearchSort::TitleAz => "Title A to Z",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SearchSort::BestMatch => "match",
            SearchSort::MostChapters => "chapters",
            SearchSort::RecentlyUpdated => "recent",
            SearchSort::MostSources => "sources",
            SearchSort::TitleAz => "az",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The order to show results in, as positions in `ranked`. Ties keep the
/// original (best match) order, and results that lack the thing being sorted
/// on come last.
pub fn sorted_order(ranked: &[search::RankedSeries], sort: SearchSort) -> Vec<usize> {
    let chapters = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.chapter_count))
            .max()
    };
    let updated = |s: &search::RankedSeries| {
        s.members
            .iter()
            .filter_map(|m| m.enrichment.as_ref().and_then(|e| e.latest_upload_ms))
            .max()
    };
    let mut order: Vec<usize> = (0..ranked.len()).collect();
    match sort {
        SearchSort::BestMatch => {}
        SearchSort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(chapters(&ranked[i]).map(i64::from)))
        }
        SearchSort::RecentlyUpdated => {
            order.sort_by_key(|&i| std::cmp::Reverse(updated(&ranked[i])))
        }
        SearchSort::MostSources => {
            order.sort_by_key(|&i| std::cmp::Reverse(ranked[i].members.len()))
        }
        SearchSort::TitleAz => {
            order.sort_by_key(|&i| ranked[i].title.to_lowercase());
        }
    }
    order
}

/// Which series the Library shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryView {
    #[default]
    All,
    /// Series with chapters on this computer.
    Downloaded,
    /// Series put in the library but not downloaded yet.
    Saved,
}

impl LibraryView {
    pub const ALL: [LibraryView; 3] = [
        LibraryView::All,
        LibraryView::Downloaded,
        LibraryView::Saved,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibraryView::All => "All",
            LibraryView::Downloaded => "Downloaded",
            LibraryView::Saved => "Saved for later",
        }
    }

    pub fn includes(self, series: &library::DownloadedSeries) -> bool {
        match self {
            LibraryView::All => true,
            LibraryView::Downloaded => !series.chapters.is_empty(),
            LibraryView::Saved => series.chapters.is_empty(),
        }
    }
}

/// How the Library orders its series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibrarySort {
    /// The ones touched most recently first.
    #[default]
    Recent,
    Title,
    MostChapters,
    Size,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 4] = [
        LibrarySort::Recent,
        LibrarySort::Title,
        LibrarySort::MostChapters,
        LibrarySort::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LibrarySort::Recent => "Recently added",
            LibrarySort::Title => "Title A to Z",
            LibrarySort::MostChapters => "Most chapters",
            LibrarySort::Size => "Biggest on disk",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            LibrarySort::Recent => "recent",
            LibrarySort::Title => "title",
            LibrarySort::MostChapters => "chapters",
            LibrarySort::Size => "size",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
}

/// The library's series to show, as positions in `series`: those in the view
/// that match the filter text (in the title, the author or a genre), in order.
pub fn library_order(
    series: &[library::DownloadedSeries],
    view: LibraryView,
    sort: LibrarySort,
    filter: &str,
) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    let matches = |s: &library::DownloadedSeries| {
        needle.is_empty()
            || s.title.to_lowercase().contains(&needle)
            || s.info.as_ref().is_some_and(|i| {
                i.author.to_lowercase().contains(&needle)
                    || i.genres.iter().any(|g| g.to_lowercase().contains(&needle))
            })
    };
    let mut order: Vec<usize> = (0..series.len())
        .filter(|&i| view.includes(&series[i]) && matches(&series[i]))
        .collect();
    match sort {
        LibrarySort::Recent => {
            order.sort_by_key(|&i| std::cmp::Reverse(series[i].updated_at.max(series[i].added_at)))
        }
        LibrarySort::Title => order.sort_by_key(|&i| series[i].title.to_lowercase()),
        LibrarySort::MostChapters => {
            order.sort_by_key(|&i| std::cmp::Reverse(series[i].chapters.len()))
        }
        LibrarySort::Size => order.sort_by_key(|&i| std::cmp::Reverse(series[i].bytes())),
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::chapter;
    use search::merge_chapters;

    fn nums(v: &[f64]) -> Vec<Option<f64>> {
        v.iter().map(|&n| Some(n)).collect()
    }

    fn merged_list() -> Vec<MergedChapter> {
        let mut a = chapter(1, 1.0, 1_000_000_000_000);
        a.name = "Chapter 1: Start".into();
        let mut b = chapter(2, 2.0, 1_500_000_000_000);
        b.name = "Chapter 2: Middle".into();
        let mut c = chapter(3, 10.0, 0);
        c.name = "Chapter 10: Finale".into();
        let mut extra = chapter(4, -1.0, 1_700_000_000_000);
        extra.name = "Author's note".into();
        // A second source also has chapter 2, newer.
        let other = vec![chapter(12, 2.0, 1_600_000_000_000)];
        merge_chapters(&[vec![a, b, c, extra], other])
    }

    fn names(merged: &[MergedChapter], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| merged[i].name.clone()).collect()
    }

    #[test]
    fn the_table_sorts_by_each_column_in_both_directions() {
        let m = merged_list();
        let by = |key, desc| names(&m, &visible_chapters(&m, "", key, desc));

        // Number: numbered first; the unnumbered note is last in either direction.
        assert_eq!(
            by(SortKey::Number, false),
            [
                "Chapter 1: Start",
                "Chapter 2: Middle",
                "Chapter 10: Finale",
                "Author's note"
            ]
        );
        assert_eq!(
            by(SortKey::Number, true),
            [
                "Chapter 10: Finale",
                "Chapter 2: Middle",
                "Chapter 1: Start",
                "Author's note"
            ]
        );
        // Name: A to Z, case-insensitive.
        assert_eq!(by(SortKey::Name, false)[0], "Author's note");
        assert_eq!(by(SortKey::Name, true)[0], "Chapter 2: Middle");
        // Sources: chapter 2 has two sources.
        assert_eq!(by(SortKey::Sources, true)[0], "Chapter 2: Middle");
        // Date: newest first; chapter 10 has no date, so it is last either way.
        assert_eq!(by(SortKey::Date, true)[0], "Author's note");
        assert_eq!(
            *by(SortKey::Date, true).last().unwrap(),
            "Chapter 10: Finale"
        );
        assert_eq!(
            *by(SortKey::Date, false).last().unwrap(),
            "Chapter 10: Finale"
        );
    }

    #[test]
    fn the_filter_matches_every_word_in_names_and_numbers() {
        let m = merged_list();
        let hit = |f: &str| names(&m, &visible_chapters(&m, f, SortKey::Number, false));
        assert_eq!(hit("finale"), ["Chapter 10: Finale"]);
        assert_eq!(
            hit("CHAPTER middle"),
            ["Chapter 2: Middle"],
            "all words, any case"
        );
        assert_eq!(hit("10"), ["Chapter 10: Finale"], "by number");
        assert_eq!(hit("note"), ["Author's note"]);
        assert!(hit("nothing like this").is_empty());
        assert_eq!(hit("   ").len(), 4, "a blank filter shows everything");
    }

    #[test]
    fn a_chapter_with_several_sources_shows_its_newest_upload() {
        let m = merged_list();
        let two = m.iter().find(|c| c.name == "Chapter 2: Middle").unwrap();
        assert_eq!(latest_upload(two), 1_600_000_000_000);
        let ten = m.iter().find(|c| c.name == "Chapter 10: Finale").unwrap();
        assert_eq!(latest_upload(ten), 0);
    }

    #[test]
    fn clicking_a_header_picks_a_sensible_direction() {
        // New column: numbers and dates start newest/biggest first, names A to Z.
        assert_eq!(
            next_sort((SortKey::Name, false), SortKey::Number),
            (SortKey::Number, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Date),
            (SortKey::Date, true)
        );
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Name),
            (SortKey::Name, false)
        );
        // Same column: flip.
        assert_eq!(
            next_sort((SortKey::Number, true), SortKey::Number),
            (SortKey::Number, false)
        );
        assert_eq!(
            next_sort((SortKey::Number, false), SortKey::Number),
            (SortKey::Number, true)
        );
    }

    #[test]
    fn numbers_and_dates_are_shown_plainly() {
        assert_eq!(format_number(12.0), "12");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(0.01), "0.01");
        assert_eq!(format_number(3.10), "3.1");
        assert_eq!(format_date(0), "");
        assert_eq!(format_date(-5), "");
        assert_eq!(format_date(1_700_000_000_000), "2023-11-14");
        assert_eq!(format_date(951_782_400_000), "2000-02-29", "leap day");
        assert_eq!(format_date(1_000_000_000_000), "2001-09-09");
    }

    #[test]
    fn tabs_open_only_when_their_step_can_work() {
        // Chapters is always open.
        assert_eq!(tab_blocked(Needs::Nothing, 0, 0, false), None);
        // Downloads needs something ticked, something downloaded, or a download running.
        assert!(tab_blocked(Needs::SelectionOrDownloads, 0, 0, false).is_some());
        assert_eq!(tab_blocked(Needs::SelectionOrDownloads, 3, 0, false), None);
        assert_eq!(tab_blocked(Needs::SelectionOrDownloads, 0, 2, false), None);
        assert_eq!(tab_blocked(Needs::SelectionOrDownloads, 0, 0, true), None);
        // Preview and Export need a downloaded chapter, however many are ticked.
        assert_eq!(
            tab_blocked(Needs::Downloads, 50, 0, true),
            Some("Download a chapter first")
        );
        assert_eq!(tab_blocked(Needs::Downloads, 0, 1, false), None);
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

    fn started(index: usize, total: usize) -> JobEvent {
        JobEvent::ChapterStarted {
            index,
            total,
            name: format!("Ch {}", index + 1),
        }
    }

    fn pages(chapter_index: usize, done: usize, total: usize) -> JobEvent {
        JobEvent::PageDone {
            chapter_index,
            done,
            total,
        }
    }

    #[test]
    fn progress_combines_chapter_and_page_position() {
        let mut p = JobProgress::default();
        assert_eq!(p.fraction(), 0.0);
        assert_eq!(p.label(), "Starting…");
        p.apply(&started(1, 4));
        assert_eq!(p.fraction(), 0.0);
        p.apply(&pages(1, 5, 10));
        assert!((p.fraction() - 0.125).abs() < 1e-6);
        assert_eq!(p.label(), "Chapter 2 of 4, page 5 of 10");
        p.apply(&JobEvent::UsedFallback {
            chapter_index: 1,
            from: "a".into(),
            to: "b".into(),
            reason: String::new(),
        });
        assert!(p.note.as_deref().unwrap().contains('b'));
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.fraction(), 0.25);
        p.apply(&started(2, 4));
        assert_eq!(p.note, None, "the note clears with the next chapter");
        assert_eq!(p.label(), "Chapter 3 of 4");
    }

    #[test]
    fn chapters_downloading_together_share_the_bar() {
        let mut p = JobProgress::default();
        for i in 0..3 {
            p.apply(&started(i, 6));
        }
        p.apply(&pages(0, 5, 10));
        p.apply(&pages(1, 10, 10));
        p.apply(&pages(2, 0, 10));
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(p.label(), "0 of 6 chapters done, 3 in progress");
        assert!((p.fraction() - 1.5 / 6.0).abs() < 1e-6);

        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.active.keys().copied().collect::<Vec<_>>(), [0, 2]);
        assert_eq!(p.label(), "1 of 6 chapters done, 2 in progress");
        assert!(
            (p.fraction() - 1.5 / 6.0).abs() < 1e-6,
            "one chapter's 100% moved from partial to finished"
        );
        // A second "done" for the same chapter is not counted twice.
        p.apply(&JobEvent::ChapterDone { index: 1 });
        assert_eq!(p.finished, 1);
    }

    #[test]
    fn easing_off_is_explained_and_clears_when_the_source_recovers() {
        let mut p = JobProgress::default();
        p.apply(&started(0, 1));
        p.apply(&JobEvent::PagesAtOnce { now: 3, chosen: 12 });
        assert!(p.note.as_deref().unwrap().contains('3'));
        p.apply(&JobEvent::PagesAtOnce {
            now: 12,
            chosen: 12,
        });
        assert_eq!(p.note, None);
    }

    #[test]
    fn progress_never_leaves_zero_to_one() {
        let p = JobProgress {
            chapter_total: 2,
            finished: 9,
            ..Default::default()
        };
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn export_size_estimates_follow_the_format() {
        use converter::xtc::Format;
        assert_eq!(estimate_export_bytes(0, Format::Xtch), 0);
        assert_eq!(estimate_export_bytes(100, Format::Xtch), 9_602_200);
        assert_eq!(estimate_export_bytes(100, Format::Xtc), 4_802_200);
        // Four grays take about twice the room of black and white.
        assert!(
            estimate_export_bytes(10, Format::Xtch)
                > 2 * estimate_export_bytes(10, Format::Xtc) - 1000
        );
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

#[cfg(test)]
mod preset_tests {
    use super::*;
    use library::ConversionSettings;

    #[test]
    fn settings_are_described_in_one_plain_line() {
        let manga = describe_settings(&ConversionSettings::default());
        assert_eq!(
            manga,
            "right to left · automatic layout · auto trim · 4 grays · contrast 55"
        );
        let c = ConversionSettings {
            crop: converter::process::CropMode::Full,
            margins: converter::process::Margins {
                top: 5,
                ..Default::default()
            },
            layout: converter::process::Layout::Webtoon,
            direction: converter::xtc::ReadDirection::LeftToRight,
            format: converter::xtc::Format::Xtc,
            ..Default::default()
        };
        assert_eq!(
            describe_settings(&c),
            "left to right · long strips · fixed margins · black & white · contrast 55"
        );
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[test]
    fn a_stopping_job_says_so_whatever_it_was_doing() {
        let mut p = JobProgress::default();
        p.apply(&JobEvent::ChapterStarted {
            index: 0,
            total: 3,
            name: "Ch 1".into(),
        });
        assert_eq!(p.label(), "Chapter 1 of 3");
        p.stopping = true;
        assert_eq!(p.label(), "Stopping…");
    }
}

#[cfg(test)]
mod info_tests {
    use super::*;

    fn details() -> catalog::MangaDetails {
        catalog::MangaDetails {
            id: 1,
            title: "Series".into(),
            description: Some("  First.\r\nSecond.\n\n\n\nThird.  ".into()),
            author: Some(" A. Writer ".into()),
            artist: None,
            genre: vec![
                "Action".into(),
                " action ".into(),
                "".into(),
                "Fantasy".into(),
            ],
            status: Some("ONGOING".into()),
            thumbnail_url: Some("/cover".into()),
        }
    }

    #[test]
    fn series_details_are_tidied_for_display() {
        let info = series_info_from(&details(), vec![("1".into(), "Src".into())], true, 99);
        assert_eq!(info.description, "First.\nSecond.\n\nThird.");
        assert_eq!(info.author, "A. Writer");
        assert_eq!(info.artist, "");
        assert_eq!(info.status, "Ongoing");
        assert_eq!(info.genres, ["Action", "Fantasy"], "no blanks or repeats");
        assert!(info.has_cover);
        assert_eq!(info.saved_at, 99);
        assert_eq!(info.source_name("1"), Some("Src"));
    }

    #[test]
    fn missing_details_become_empty_not_errors() {
        let info = series_info_from(&catalog::MangaDetails::default(), Vec::new(), false, 0);
        assert_eq!(info.description, "");
        assert_eq!(info.status, "");
        assert!(info.genres.is_empty());
        assert!(!info.has_cover);
    }

    #[test]
    fn counts_are_worded_in_the_singular_and_plural() {
        assert_eq!(plural(0, "chapter"), "0 chapters");
        assert_eq!(plural(1, "chapter"), "1 chapter");
        assert_eq!(plural(235, "page"), "235 pages");
    }

    #[test]
    fn the_chapter_range_ignores_unnumbered_chapters() {
        let nums = [Some(3.0), None, Some(1.5), Some(10.0)];
        assert_eq!(number_range(nums.into_iter()), Some((1.5, 10.0)));
        assert_eq!(number_range([None, None].into_iter()), None);
        assert_eq!(number_range(std::iter::empty()), None);
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use search::{Enrichment, RankedSeries, ScoredMember};

    fn member(source: &str, chapters: Option<u32>, updated: Option<i64>) -> ScoredMember {
        ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: source.into(),
                    name: format!("Source {source}"),
                    lang: "en".into(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: 1,
                    title: "x".into(),
                    thumbnail_url: None,
                },
            ),
            score: 0.5,
            enrichment: Some(Enrichment {
                chapter_count: chapters,
                latest_upload_ms: updated,
            }),
        }
    }

    fn series(title: &str, members: Vec<ScoredMember>) -> RankedSeries {
        RankedSeries {
            title: title.into(),
            members,
            relevance: 1.0,
        }
    }

    fn results() -> Vec<RankedSeries> {
        vec![
            series("Bravo", vec![member("a", Some(10), Some(300))]),
            series(
                "alpha",
                vec![
                    member("a", Some(50), Some(100)),
                    member("b", Some(80), None),
                ],
            ),
            series("Charlie", vec![member("a", None, None)]),
            series("Delta", vec![member("a", Some(50), Some(900))]),
        ]
    }

    #[test]
    fn best_match_keeps_the_order_the_search_gave() {
        assert_eq!(
            sorted_order(&results(), SearchSort::BestMatch),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn most_chapters_uses_the_biggest_count_any_source_has_and_puts_unknown_last() {
        // alpha has 80 (from its second source), Delta 50, Bravo 10, Charlie unknown.
        assert_eq!(
            sorted_order(&results(), SearchSort::MostChapters),
            [1, 3, 0, 2]
        );
    }

    #[test]
    fn recently_updated_puts_the_newest_first_and_unknown_last() {
        assert_eq!(
            sorted_order(&results(), SearchSort::RecentlyUpdated),
            [3, 0, 1, 2]
        );
    }

    #[test]
    fn most_sources_breaks_ties_by_the_original_order() {
        assert_eq!(
            sorted_order(&results(), SearchSort::MostSources),
            [1, 0, 2, 3]
        );
    }

    #[test]
    fn titles_sort_without_regard_to_case() {
        assert_eq!(sorted_order(&results(), SearchSort::TitleAz), [1, 0, 2, 3]);
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in SearchSort::ALL {
            assert_eq!(SearchSort::from_key(sort.key()), sort);
        }
        assert_eq!(SearchSort::from_key("nonsense"), SearchSort::BestMatch);
        assert_eq!(
            sorted_order(&[], SearchSort::MostChapters),
            Vec::<usize>::new()
        );
    }
}

#[cfg(test)]
mod library_view_tests {
    use super::*;
    use library::{DownloadedChapter, DownloadedSeries, SeriesInfo};

    fn chapter(bytes: u64) -> DownloadedChapter {
        DownloadedChapter {
            key: "c1".into(),
            number: Some(1.0),
            name: "Chapter 1".into(),
            source_id: "s".into(),
            chapter_id: 1,
            pages: 10,
            bytes,
            downloaded_at: 0,
        }
    }

    fn series(
        title: &str,
        chapters: usize,
        bytes: u64,
        added: u64,
        updated: u64,
    ) -> DownloadedSeries {
        DownloadedSeries {
            id: title.to_lowercase(),
            title: title.into(),
            chapters: (0..chapters).map(|_| chapter(bytes)).collect(),
            added_at: added,
            updated_at: updated,
            ..Default::default()
        }
    }

    fn shelf() -> Vec<DownloadedSeries> {
        let mut a = series("Berserk", 3, 100, 10, 50);
        a.info = Some(SeriesInfo {
            author: "Kentaro Miura".into(),
            genres: vec!["Dark Fantasy".into()],
            ..Default::default()
        });
        vec![
            a,
            series("alpha", 0, 0, 40, 40), // saved, not downloaded
            series("Zeta", 10, 5, 30, 30),
            series("Delta", 0, 0, 5, 5),
        ]
    }

    #[test]
    fn the_views_split_downloaded_from_saved() {
        let s = shelf();
        let order = |v| library_order(&s, v, LibrarySort::Title, "");
        assert_eq!(order(LibraryView::All), [1, 0, 3, 2]);
        assert_eq!(order(LibraryView::Downloaded), [0, 2]);
        assert_eq!(order(LibraryView::Saved), [1, 3]);
    }

    #[test]
    fn the_sorts_order_as_they_say() {
        let s = shelf();
        let by = |sort| library_order(&s, LibraryView::All, sort, "");
        assert_eq!(
            by(LibrarySort::Recent),
            [0, 1, 2, 3],
            "latest touch first (50, 40, 30, 5)"
        );
        assert_eq!(
            by(LibrarySort::Title),
            [1, 0, 3, 2],
            "A to Z, ignoring case"
        );
        assert_eq!(by(LibrarySort::MostChapters), [2, 0, 1, 3]);
        assert_eq!(
            by(LibrarySort::Size),
            [0, 2, 1, 3],
            "300 bytes, 50, then the empty ones"
        );
    }

    #[test]
    fn the_filter_looks_at_the_title_author_and_genres() {
        let s = shelf();
        let find = |text: &str| library_order(&s, LibraryView::All, LibrarySort::Title, text);
        assert_eq!(find("  ZET "), [2]);
        assert_eq!(find("miura"), [0], "author");
        assert_eq!(find("dark fantasy"), [0], "genre");
        assert!(find("nothing like this").is_empty());
        assert_eq!(
            library_order(&s, LibraryView::Saved, LibrarySort::Title, "berserk"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn sort_choices_round_trip_through_their_keys() {
        for sort in LibrarySort::ALL {
            assert_eq!(LibrarySort::from_key(sort.key()), sort);
        }
        assert_eq!(LibrarySort::from_key("???"), LibrarySort::Recent);
    }
}
