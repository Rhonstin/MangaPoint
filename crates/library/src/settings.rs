//! User preferences.

use converter::process::{
    CropMode, Layout, Margins, SpreadMode, DEFAULT_TRIM_PADDING, DEFAULT_TRIM_SENSITIVITY,
};
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use device_profiles::{Device, Firmware};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionSettings {
    pub format: Format,
    pub direction: ReadDirection,
    pub layout: Layout,
    /// Trim blank borders automatically.
    pub crop: CropMode,
    /// Fixed margins cut from every page before the automatic trim.
    pub margins: Margins,
    /// 0..=100: higher notices fainter marks, so trims less.
    pub trim_sensitivity: u8,
    /// Pixels kept around the content after trimming.
    pub trim_padding: u8,
    pub spread: SpreadMode,
    /// 0..=100, 50 = unchanged.
    pub contrast: u8,
    pub auto_levels: bool,
    pub overlap_percent: u8,
    pub dither: Dither,
}

impl Default for ConversionSettings {
    /// The plan's default profile, with XTCH (4 gray levels) as the format
    /// because manga needs the grays.
    fn default() -> Self {
        Self {
            format: Format::Xtch,
            direction: ReadDirection::RightToLeft,
            layout: Layout::Auto,
            crop: CropMode::Margins,
            margins: Margins::default(),
            trim_sensitivity: DEFAULT_TRIM_SENSITIVITY,
            trim_padding: DEFAULT_TRIM_PADDING,
            spread: SpreadMode::Split,
            contrast: 55,
            auto_levels: true,
            overlap_percent: 8,
            dither: Dither::FloydSteinberg,
        }
    }
}

/// How hard downloads push on a source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadSpeed {
    /// Careful: for sources that block easily.
    Gentle,
    #[default]
    Normal,
    Fast,
    /// Every source that has a chapter is asked at once; the first to finish wins.
    Ultra,
    /// The user's own numbers.
    Custom,
}

/// What a download may do at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadLimits {
    /// Chapters being downloaded at the same time.
    pub chapters: usize,
    /// Page requests in flight at the same time, across all chapters.
    pub pages: usize,
    /// Download different chapters from different sources at the same time
    /// when several offer them.
    pub spread_sources: bool,
    /// Ask every source that has a chapter at once and keep the first to
    /// finish. `pages` is then the allowance of each source.
    pub race_sources: bool,
}

impl DownloadLimits {
    pub const MAX_CHAPTERS: u8 = 4;
    pub const MAX_PAGES: u8 = 16;
}

impl Default for DownloadLimits {
    fn default() -> Self {
        DownloadSettings::default().limits()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    pub speed: DownloadSpeed,
    /// Used when `speed` is Custom.
    pub custom_chapters: u8,
    pub custom_pages: u8,
    /// Download different chapters from different sources at the same time
    /// when several offer them. Some sources cap their own speed, so this
    /// adds the sources' speeds together.
    pub spread_sources: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            speed: DownloadSpeed::Normal,
            custom_chapters: 2,
            custom_pages: 6,
            spread_sources: true,
        }
    }
}

impl DownloadSettings {
    /// The numbers behind the choice. The three named speeds were picked from
    /// measuring real sources: gains level off around 8 to 12 requests at
    /// once, and some sources gain nothing at all.
    pub fn limits(&self) -> DownloadLimits {
        let (chapters, pages) = match self.speed {
            DownloadSpeed::Gentle => (1, 2),
            DownloadSpeed::Normal => (2, 6),
            DownloadSpeed::Fast => (3, 12),
            DownloadSpeed::Ultra => (3, 8),
            DownloadSpeed::Custom => (
                self.custom_chapters.clamp(1, DownloadLimits::MAX_CHAPTERS) as usize,
                self.custom_pages.clamp(1, DownloadLimits::MAX_PAGES) as usize,
            ),
        };
        DownloadLimits {
            chapters,
            pages,
            spread_sources: self.spread_sources,
            race_sources: self.speed == DownloadSpeed::Ultra,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    /// Where finished files are written. `None` means the app's default folder.
    pub output_dir: Option<PathBuf>,
    /// Folder created on the device's storage for copied files.
    pub device_folder: String,
    /// Base URL of the reader's WebDAV share, if the user set one up.
    pub webdav_url: Option<String>,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            output_dir: None,
            device_folder: "Manga".into(),
            webdav_url: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub device: Device,
    pub firmware: Firmware,
    pub conversion: ConversionSettings,
    pub export: ExportSettings,
    /// Allow Suwayomi's embedded Chromium (needed by a few sources).
    pub browser_support: bool,
    /// Shrink downloaded pages to save disk space. Off by default: pages are
    /// kept exactly as downloaded, for the best quality.
    pub shrink_downloads: bool,
    /// How many chapters and pages to download at once.
    pub download: DownloadSettings,
    /// The preset the default conversion settings started from, if any.
    pub default_preset_id: Option<String>,
    /// Preferred source language for search and ranking.
    pub language: String,
    /// Source ids the user prefers when the same series exists in several.
    pub preferred_sources: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: Device::XteinkX4Pro,
            firmware: Firmware::CrossPoint,
            conversion: ConversionSettings::default(),
            export: ExportSettings::default(),
            browser_support: false,
            shrink_downloads: false,
            download: DownloadSettings::default(),
            default_preset_id: None,
            language: "en".into(),
            preferred_sources: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_plan() {
        let s = Settings::default();
        assert_eq!(s.device, Device::XteinkX4Pro);
        assert_eq!(s.conversion.direction, ReadDirection::RightToLeft);
        assert_eq!(s.conversion.contrast, 55);
        assert_eq!(s.conversion.overlap_percent, 8);
        assert_eq!(s.conversion.dither, Dither::FloydSteinberg);
        assert!(!s.browser_support, "browser support is opt-in");
        assert!(
            !s.shrink_downloads,
            "pages are kept at full quality by default"
        );
    }

    #[test]
    fn download_speeds_map_to_limits_and_custom_numbers_are_held_in_range() {
        let both = |chapters, pages| DownloadLimits {
            chapters,
            pages,
            spread_sources: true,
            race_sources: false,
        };
        let mut d = DownloadSettings::default();
        assert_eq!(d.limits(), both(2, 6));
        d.speed = DownloadSpeed::Gentle;
        assert_eq!(d.limits(), both(1, 2));
        d.speed = DownloadSpeed::Fast;
        assert_eq!(d.limits(), both(3, 12));
        d.speed = DownloadSpeed::Ultra;
        assert!(d.limits().race_sources);
        assert_eq!((d.limits().chapters, d.limits().pages), (3, 8));
        d.speed = DownloadSpeed::Custom;
        assert!(!d.limits().race_sources);
        d.custom_chapters = 0;
        d.custom_pages = 200;
        assert_eq!(d.limits(), both(1, 16));
        d.custom_chapters = 9;
        d.custom_pages = 0;
        assert_eq!(d.limits(), both(4, 1));
        d.spread_sources = false;
        assert!(!d.limits().spread_sources);
    }

    #[test]
    fn partial_files_fill_in_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{"language":"ja","conversion":{"contrast":70}}"#).unwrap();
        assert_eq!(s.language, "ja");
        assert_eq!(s.conversion.contrast, 70);
        assert_eq!(s.conversion.overlap_percent, 8);
        assert_eq!(s.export.device_folder, "Manga");
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut s = Settings::default();
        s.conversion.format = Format::Xtc;
        s.export.webdav_url = Some("http://192.168.3.3/".into());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }
}
