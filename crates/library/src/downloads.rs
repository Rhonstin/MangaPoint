//! Chapters the user has downloaded. The pages stay on disk until the user
//! deletes them, so previewing and exporting never need the network.
//!
//! Layout under the library folder:
//!
//! ```text
//! <library>/<series>/<chapter>/00001.img   the page as downloaded
//!                              00002.img
//!                              source.txt  which source copy these pages came from
//! ```
//!
//! This module also owns deletion, and every delete goes through
//! [`remove_inside`], which refuses to touch anything but a folder strictly
//! inside the library.

use crate::settings::ConversionSettings;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// ---- Identity --------------------------------------------------------------

/// A stable id for a series, from its title: the same title gives the same id
/// whatever the capitalisation or punctuation, so chapters downloaded on
/// different days from different sources land in one place. Also usable as a
/// folder name: `solo-leveling-3fa9c2e1`.
pub fn series_id(title: &str) -> String {
    let squashed: String = title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in squashed.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let words: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let mut slug = words.join("-");
    slug.truncate(40);
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "series" } else { slug };
    format!("{slug}-{:08x}", hash & 0xffff_ffff)
}

/// A stable name for a chapter within its series: `c12`, `c12_5`, or
/// `u<chapter id>` for chapters without a number.
pub fn chapter_key(number: Option<f64>, chapter_id: i64) -> String {
    match number {
        Some(n) if n.fract().abs() < 0.0005 => format!("c{}", n.round() as i64),
        Some(n) => format!(
            "c{}",
            format!("{n:.3}").trim_end_matches('0').replace('.', "_")
        ),
        None => format!("u{chapter_id}"),
    }
}

// ---- The index -------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DownloadedChapter {
    pub key: String,
    pub number: Option<f64>,
    pub name: String,
    /// Where these pages came from (kept so a download can be repeated).
    pub source_id: String,
    pub chapter_id: i64,
    pub pages: usize,
    /// Size of the saved pages on disk.
    pub bytes: u64,
    pub downloaded_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadedSeries {
    pub id: String,
    pub title: String,
    pub chapters: Vec<DownloadedChapter>,
    /// How pages of this series are converted (set while previewing).
    pub settings: ConversionSettings,
    /// The preset these settings started from, if any. The settings may have
    /// been changed since.
    pub preset_id: Option<String>,
    pub updated_at: u64,
}

impl DownloadedSeries {
    pub fn bytes(&self) -> u64 {
        self.chapters.iter().map(|c| c.bytes).sum()
    }

    pub fn page_count(&self) -> usize {
        self.chapters.iter().map(|c| c.pages).sum()
    }

    pub fn chapter(&self, key: &str) -> Option<&DownloadedChapter> {
        self.chapters.iter().find(|c| c.key == key)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryDb {
    pub series: Vec<DownloadedSeries>,
}

impl LibraryDb {
    pub fn find(&self, series_id: &str) -> Option<&DownloadedSeries> {
        self.series.iter().find(|s| s.id == series_id)
    }

    /// Add or replace a downloaded chapter, creating the series if needed
    /// (with `defaults` as its conversion settings). Chapters stay in reading
    /// order, unnumbered ones last.
    pub fn record_chapter(
        &mut self,
        title: &str,
        defaults: &ConversionSettings,
        chapter: DownloadedChapter,
        now: u64,
    ) -> String {
        let id = series_id(title);
        let index = match self.series.iter().position(|s| s.id == id) {
            Some(i) => i,
            None => {
                self.series.push(DownloadedSeries {
                    id: id.clone(),
                    title: title.to_string(),
                    chapters: Vec::new(),
                    settings: defaults.clone(),
                    preset_id: None,
                    updated_at: now,
                });
                self.series.len() - 1
            }
        };
        let series = &mut self.series[index];
        series.chapters.retain(|c| c.key != chapter.key);
        series.chapters.push(chapter);
        series.chapters.sort_by(|a, b| {
            a.number
                .unwrap_or(f64::MAX)
                .total_cmp(&b.number.unwrap_or(f64::MAX))
                .then_with(|| a.name.cmp(&b.name))
        });
        series.updated_at = now;
        id
    }

    /// Forget one chapter; the series goes too when its last chapter does.
    pub fn remove_chapter(&mut self, series_id: &str, key: &str) -> bool {
        let Some(i) = self.series.iter().position(|s| s.id == series_id) else {
            return false;
        };
        let before = self.series[i].chapters.len();
        self.series[i].chapters.retain(|c| c.key != key);
        let removed = self.series[i].chapters.len() != before;
        if self.series[i].chapters.is_empty() {
            self.series.remove(i);
        }
        removed
    }

    pub fn remove_series(&mut self, series_id: &str) -> Option<DownloadedSeries> {
        let i = self.series.iter().position(|s| s.id == series_id)?;
        Some(self.series.remove(i))
    }

    pub fn set_settings(&mut self, series_id: &str, settings: ConversionSettings) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            s.settings = settings;
        }
    }

    /// A series still on the default settings, with no preset of its own yet,
    /// takes the preset those defaults came from.
    pub fn adopt_default_preset(
        &mut self,
        series_id: &str,
        defaults: &ConversionSettings,
        preset_id: &Option<String>,
    ) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            if s.preset_id.is_none() && s.settings == *defaults {
                s.preset_id = preset_id.clone();
            }
        }
    }

    /// Remember which preset a series started from.
    pub fn set_preset(&mut self, series_id: &str, preset_id: Option<String>) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == series_id) {
            s.preset_id = preset_id;
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.series.iter().map(DownloadedSeries::bytes).sum()
    }
}

// ---- Files on disk ---------------------------------------------------------

/// Where downloaded pages live, and the only code that deletes them.
#[derive(Clone, Debug)]
pub struct LibraryFiles {
    root: PathBuf,
}

/// Make a value safe to use as one folder or file name, whatever the index
/// file says: separators, `..` and the like become underscores.
fn component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".into()
    } else {
        cleaned
    }
}

impl LibraryFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn series_dir(&self, series_id: &str) -> PathBuf {
        self.root.join(component(series_id))
    }

    pub fn chapter_dir(&self, series_id: &str, key: &str) -> PathBuf {
        self.series_dir(series_id).join(component(key))
    }

    /// Path of page `index` (0-based), named so that sorting by name gives
    /// reading order.
    pub fn page_path(&self, series_id: &str, key: &str, index: usize) -> PathBuf {
        self.chapter_dir(series_id, key)
            .join(format!("{:05}.img", index + 1))
    }

    /// The saved pages of a chapter, in reading order. Empty files (left by
    /// an interrupted write) are ignored.
    pub fn page_files(&self, series_id: &str, key: &str) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.chapter_dir(series_id, key)) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.path().extension().is_some_and(|x| x == "img")
                    && e.metadata().is_ok_and(|m| m.len() > 0)
            })
            .map(|e| e.path())
            .collect();
        files.sort();
        files
    }

    /// Total size of a chapter's saved pages.
    pub fn chapter_bytes(&self, series_id: &str, key: &str) -> u64 {
        self.page_files(series_id, key)
            .iter()
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn remove_chapter(&self, series_id: &str, key: &str) -> io::Result<()> {
        remove_inside(&self.root, &self.chapter_dir(series_id, key))
    }

    pub fn remove_series(&self, series_id: &str) -> io::Result<()> {
        remove_inside(&self.root, &self.series_dir(series_id))
    }
}

/// Delete `target` and everything in it, but only if it is a folder strictly
/// inside `root`. Refuses the root itself, anything outside it, and symbolic
/// links. A folder that is already gone counts as success.
pub fn remove_inside(root: &Path, target: &Path) -> io::Result<()> {
    let refuse = |why: &str| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing to delete {}: {why}", target.display()),
        ))
    };
    if target == root || !target.starts_with(root) {
        return refuse("it is not inside the library folder");
    }
    if target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return refuse("the path contains '..'");
    }
    match fs::symlink_metadata(target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.file_type().is_symlink() => refuse("it is a symbolic link"),
        Ok(meta) if !meta.is_dir() => refuse("it is not a folder"),
        Ok(_) => fs::remove_dir_all(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-lib-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn chapter(number: Option<f64>, id: i64, name: &str) -> DownloadedChapter {
        DownloadedChapter {
            key: chapter_key(number, id),
            number,
            name: name.into(),
            source_id: "s".into(),
            chapter_id: id,
            pages: 10,
            bytes: 1000,
            downloaded_at: 5,
        }
    }

    #[test]
    fn ids_ignore_case_and_punctuation_but_not_the_title() {
        assert_eq!(series_id("Solo Leveling"), series_id("SOLO-leveling!"));
        assert_ne!(
            series_id("Solo Leveling"),
            series_id("Solo Leveling: Ragnarok")
        );
        let id = series_id("Solo Leveling");
        assert!(id.starts_with("solo-leveling-"), "{id}");
        assert_eq!(id.len(), "solo-leveling-".len() + 8);
        // Titles with no ASCII letters still get a usable, distinct id.
        assert!(series_id("進撃の巨人").starts_with("series-"));
        assert_ne!(series_id("進撃の巨人"), series_id("鬼滅の刃"));
    }

    #[test]
    fn chapter_keys_are_stable_and_filename_safe() {
        assert_eq!(chapter_key(Some(12.0), 99), "c12");
        assert_eq!(chapter_key(Some(12.5), 99), "c12_5");
        assert_eq!(chapter_key(Some(0.01), 99), "c0_01");
        assert_eq!(chapter_key(None, 99), "u99");
    }

    #[test]
    fn chapters_are_kept_in_reading_order_and_replaced_not_duplicated() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        db.record_chapter("Berserk", &d, chapter(Some(2.0), 2, "Two"), 1);
        db.record_chapter("berserk", &d, chapter(None, 7, "Note"), 2);
        let id = db.record_chapter("Berserk", &d, chapter(Some(1.0), 1, "One"), 3);
        db.record_chapter("Berserk", &d, chapter(Some(2.0), 22, "Two again"), 4);

        assert_eq!(
            db.series.len(),
            1,
            "same title, different spelling: one series"
        );
        let s = db.find(&id).unwrap();
        let names: Vec<_> = s.chapters.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["One", "Two again", "Note"]);
        assert_eq!(s.updated_at, 4);
        assert_eq!(s.bytes(), 3000);
        assert_eq!(s.page_count(), 30);
    }

    #[test]
    fn a_new_series_starts_with_the_users_default_settings() {
        let mut db = LibraryDb::default();
        let defaults = ConversionSettings {
            contrast: 70,
            ..Default::default()
        };
        let id = db.record_chapter("X", &defaults, chapter(Some(1.0), 1, "a"), 1);
        assert_eq!(db.find(&id).unwrap().settings.contrast, 70);
        // Later chapters do not overwrite settings the user tuned for the series.
        let mut tuned = defaults.clone();
        tuned.contrast = 40;
        db.set_settings(&id, tuned);
        db.record_chapter("X", &defaults, chapter(Some(2.0), 2, "b"), 2);
        assert_eq!(db.find(&id).unwrap().settings.contrast, 40);
    }

    #[test]
    fn removing_the_last_chapter_removes_the_series() {
        let mut db = LibraryDb::default();
        let d = ConversionSettings::default();
        let id = db.record_chapter("X", &d, chapter(Some(1.0), 1, "a"), 1);
        db.record_chapter("X", &d, chapter(Some(2.0), 2, "b"), 1);
        assert!(db.remove_chapter(&id, "c1"));
        assert!(!db.remove_chapter(&id, "c1"), "already gone");
        assert!(db.find(&id).is_some());
        assert!(db.remove_chapter(&id, "c2"));
        assert!(db.find(&id).is_none());
        assert_eq!(db.total_bytes(), 0);
    }

    #[test]
    fn the_index_survives_a_json_round_trip() {
        let mut db = LibraryDb::default();
        db.record_chapter(
            "X",
            &ConversionSettings::default(),
            chapter(Some(1.5), 1, "a"),
            1,
        );
        let json = serde_json::to_string(&db).unwrap();
        assert_eq!(serde_json::from_str::<LibraryDb>(&json).unwrap(), db);
    }

    #[test]
    fn paths_stay_inside_the_library_even_for_hostile_names() {
        let root = PathBuf::from("/lib");
        let files = LibraryFiles::new(&root);
        for evil in ["../../etc", "..", "a/b", r"a\b", "/abs", ""] {
            let p = files.chapter_dir(evil, evil);
            assert!(p.starts_with(&root), "{evil:?} -> {p:?}");
            assert!(
                !p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
                "{p:?}"
            );
        }
    }

    #[test]
    fn pages_are_listed_in_reading_order_and_empty_files_are_ignored() {
        let root = temp("pages");
        let files = LibraryFiles::new(&root);
        for (i, data) in [(2usize, b"c".as_slice()), (0, b"a"), (1, b"b"), (3, b"")] {
            let p = files.page_path("s", "c1", i);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        fs::write(files.chapter_dir("s", "c1").join("source.txt"), "x").unwrap();
        let listed = files.page_files("s", "c1");
        assert_eq!(
            listed.len(),
            3,
            "the empty page and source.txt are not pages"
        );
        let contents: Vec<_> = listed.iter().map(|p| fs::read(p).unwrap()).collect();
        assert_eq!(contents, [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        assert_eq!(files.chapter_bytes("s", "c1"), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_removes_only_the_chosen_chapter_or_series() {
        let root = temp("delete");
        let files = LibraryFiles::new(&root);
        for (s, c) in [("a", "c1"), ("a", "c2"), ("b", "c1")] {
            let p = files.page_path(s, c, 0);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        files.remove_chapter("a", "c1").unwrap();
        assert!(!files.chapter_dir("a", "c1").exists());
        assert!(files.chapter_dir("a", "c2").exists());
        files.remove_series("a").unwrap();
        assert!(!files.series_dir("a").exists());
        assert!(
            files.chapter_dir("b", "c1").exists(),
            "other series untouched"
        );
        files.remove_series("a").unwrap(); // already gone is fine
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_inside_refuses_the_root_outsiders_and_files() {
        let base = temp("guard");
        let root = base.join("library");
        let outside = base.join("precious");
        fs::create_dir_all(root.join("series")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep").unwrap();
        fs::write(root.join("note.txt"), "x").unwrap();

        assert!(
            remove_inside(&root, &root).is_err(),
            "never the library itself"
        );
        assert!(remove_inside(&root, &outside).is_err(), "never a sibling");
        assert!(remove_inside(&root, &base).is_err(), "never a parent");
        assert!(remove_inside(
            &root,
            &root.join("series").join("..").join("..").join("precious")
        )
        .is_err());
        assert!(
            remove_inside(&root, &root.join("note.txt")).is_err(),
            "not a plain file"
        );
        assert!(outside.join("keep.txt").exists());
        assert!(root.join("series").exists());

        assert!(remove_inside(&root, &root.join("series")).is_ok());
        assert!(!root.join("series").exists());
        let _ = fs::remove_dir_all(base);
    }
}
