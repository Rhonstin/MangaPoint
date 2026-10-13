//! User preferences.

use converter::process::{
    CropMode, Layout, Margins, SpreadMode, StripCut, DEFAULT_TRIM_PADDING, DEFAULT_TRIM_SENSITIVITY,
};
use converter::xtc::{Format, ReadDirection};
use converter::Dither;
use device_profiles::{DeviceProfile, Firmware};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
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
    /// How a long strip is cut where it has no blank gap.
    pub strip_cut: StripCut,
    /// With the older cut, how much of a page repeats on the next.
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
            strip_cut: StripCut::Quiet,
            overlap_percent: 8,
            dither: Dither::FloydSteinberg,
        }
    }
}

/// One setting of [`ConversionSettings`], named, so a series can remember
/// which ones it changed from its preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversionField {
    Format,
    Direction,
    Layout,
    Crop,
    Margins,
    TrimSensitivity,
    TrimPadding,
    Spread,
    Contrast,
    AutoLevels,
    StripCut,
    OverlapPercent,
    Dither,
}

impl ConversionField {
    pub const ALL: [ConversionField; 13] = [
        ConversionField::Format,
        ConversionField::Direction,
        ConversionField::Layout,
        ConversionField::Crop,
        ConversionField::Margins,
        ConversionField::TrimSensitivity,
        ConversionField::TrimPadding,
        ConversionField::Spread,
        ConversionField::Contrast,
        ConversionField::AutoLevels,
        ConversionField::StripCut,
        ConversionField::OverlapPercent,
        ConversionField::Dither,
    ];
}

impl ConversionSettings {
    /// The settings in which `self` and `other` differ.
    pub fn differing(&self, other: &ConversionSettings) -> Vec<ConversionField> {
        ConversionField::ALL
            .into_iter()
            .filter(|&f| {
                let mut probe = self.clone();
                probe.copy_field(other, f);
                probe != *self
            })
            .collect()
    }

    /// Take one setting from `from`.
    pub fn copy_field(&mut self, from: &ConversionSettings, field: ConversionField) {
        match field {
            ConversionField::Format => self.format = from.format,
            ConversionField::Direction => self.direction = from.direction,
            ConversionField::Layout => self.layout = from.layout,
            ConversionField::Crop => self.crop = from.crop,
            ConversionField::Margins => self.margins = from.margins,
            ConversionField::TrimSensitivity => self.trim_sensitivity = from.trim_sensitivity,
            ConversionField::TrimPadding => self.trim_padding = from.trim_padding,
            ConversionField::Spread => self.spread = from.spread,
            ConversionField::Contrast => self.contrast = from.contrast,
            ConversionField::AutoLevels => self.auto_levels = from.auto_levels,
            ConversionField::StripCut => self.strip_cut = from.strip_cut,
            ConversionField::OverlapPercent => self.overlap_percent = from.overlap_percent,
            ConversionField::Dither => self.dither = from.dither,
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
    /// The most chapters and pages at once, all from the one best source, so
    /// every chapter stays from the same translation.
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
    pub const MAX_CHAPTERS: u8 = 10;
    pub const MAX_PAGES: u8 = 100;
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
    /// Download different chapters from different sources at the same time.
    /// Adds the sources' speeds together, but chapters can then come from
    /// different translations. Off unless the user turns it on, in any speed.
    pub spread_sources: bool,
    /// Ask every source that has a chapter at once and keep the first to
    /// finish. Fastest per chapter, and chapters can then come from different
    /// translations. Off unless the user turns it on, in any speed.
    pub race_sources: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            speed: DownloadSpeed::Ultra,
            custom_chapters: 4,
            custom_pages: 24,
            spread_sources: false,
            race_sources: false,
        }
    }
}

impl DownloadSettings {
    /// The numbers behind the choice, from measuring real sources: gains
    /// level off at about 50 requests in flight, some sources gain nothing at
    /// all (they cap themselves), and none pushed back even at 200.
    ///
    /// Mixing sources is only done when the user has switched it on, because it
    /// can mix translations within one series; it works with any speed.
    pub fn limits(&self) -> DownloadLimits {
        let (chapters, pages) = match self.speed {
            DownloadSpeed::Gentle => (1, 2),
            DownloadSpeed::Normal => (2, 6),
            DownloadSpeed::Fast => (4, 24),
            DownloadSpeed::Ultra => (8, 48),
            DownloadSpeed::Custom => (
                self.custom_chapters.clamp(1, DownloadLimits::MAX_CHAPTERS) as usize,
                self.custom_pages.clamp(1, DownloadLimits::MAX_PAGES) as usize,
            ),
        };
        DownloadLimits {
            chapters,
            pages,
            spread_sources: self.spread_sources,
            race_sources: self.race_sources,
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
    /// Id of the device pages are made for: a built-in or one of
    /// `custom_devices`. Files from before devices were data hold the old
    /// enum's name, which is mapped on load.
    #[serde(deserialize_with = "device_id")]
    pub device: String,
    /// Devices the user added.
    pub custom_devices: Vec<DeviceProfile>,
    /// The preset a device starts from, by device id.
    pub device_presets: BTreeMap<String, String>,
    pub firmware: Firmware,
    /// Show every conversion setting, not just the few that matter most.
    pub advanced_conversion: bool,
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
    /// Sources turned off for searching. Everything not listed is searched,
    /// so a source installed later is searched too.
    pub search_excluded_sources: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: device_profiles::X4_PRO_ID.into(),
            custom_devices: Vec::new(),
            device_presets: BTreeMap::new(),
            firmware: Firmware::CrossPoint,
            advanced_conversion: false,
            conversion: ConversionSettings::default(),
            export: ExportSettings::default(),
            browser_support: false,
            shrink_downloads: false,
            download: DownloadSettings::default(),
            default_preset_id: None,
            language: "en".into(),
            preferred_sources: Vec::new(),
            search_excluded_sources: Vec::new(),
        }
    }
}

fn device_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = String::deserialize(d)?;
    Ok(device_profiles::legacy_id(&raw).map_or(raw, str::to_string))
}

impl Settings {
    /// The chosen device. A device that has since been removed falls back to
    /// the X4 Pro, so export never fails over a stale id.
    pub fn device_profile(&self) -> DeviceProfile {
        device_profiles::find(&self.device, &self.custom_devices)
            .unwrap_or_else(device_profiles::x4_pro)
    }

    /// Every device the user can pick.
    pub fn devices(&self) -> Vec<DeviceProfile> {
        device_profiles::all(&self.custom_devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_plan() {
        let s = Settings::default();
        assert_eq!(s.device, device_profiles::X4_PRO_ID);
        assert_eq!(s.conversion.direction, ReadDirection::RightToLeft);
        assert_eq!(s.conversion.contrast, 55);
        assert_eq!(s.conversion.overlap_percent, 8);
        assert_eq!(s.conversion.dither, Dither::FloydSteinberg);
        assert!(!s.browser_support, "browser support is opt-in");
        assert!(
            s.search_excluded_sources.is_empty(),
            "every source is searched until the user says otherwise"
        );
        assert!(
            !s.shrink_downloads,
            "pages are kept at full quality by default"
        );
    }

    #[test]
    fn download_speeds_map_to_limits_and_custom_numbers_are_held_in_range() {
        let plain = |chapters, pages| DownloadLimits {
            chapters,
            pages,
            spread_sources: false,
            race_sources: false,
        };
        let mut d = DownloadSettings::default();
        assert_eq!(
            d.speed,
            DownloadSpeed::Ultra,
            "the fastest mode is the default"
        );
        assert_eq!(d.limits(), plain(8, 48));
        d.speed = DownloadSpeed::Gentle;
        assert_eq!(d.limits(), plain(1, 2));
        d.speed = DownloadSpeed::Normal;
        assert_eq!(d.limits(), plain(2, 6));
        d.speed = DownloadSpeed::Fast;
        assert_eq!(d.limits(), plain(4, 24));
        d.speed = DownloadSpeed::Custom;
        d.custom_chapters = 0;
        d.custom_pages = 250;
        assert_eq!(d.limits(), plain(1, 100));
        d.custom_chapters = 99;
        d.custom_pages = 0;
        assert_eq!(d.limits(), plain(10, 1));
    }

    #[test]
    fn mixing_sources_follows_its_own_switches_in_every_speed() {
        let speeds = [
            DownloadSpeed::Gentle,
            DownloadSpeed::Normal,
            DownloadSpeed::Fast,
            DownloadSpeed::Ultra,
            DownloadSpeed::Custom,
        ];
        // Off by default, whatever the speed.
        let mut d = DownloadSettings::default();
        for speed in speeds {
            d.speed = speed;
            let l = d.limits();
            assert!(!l.spread_sources && !l.race_sources, "{speed:?}");
        }
        // Switched on, it applies to every speed, not just Advanced.
        d.spread_sources = true;
        d.race_sources = true;
        for speed in speeds {
            d.speed = speed;
            let l = d.limits();
            assert!(l.spread_sources && l.race_sources, "{speed:?}");
        }
        // And each switch is its own.
        d.race_sources = false;
        let l = d.limits();
        assert!(l.spread_sources && !l.race_sources);
    }

    #[test]
    fn differing_names_exactly_the_changed_settings() {
        let a = ConversionSettings::default();
        assert!(a.differing(&a).is_empty());
        let mut b = a.clone();
        b.contrast = 80;
        b.margins.top = 5;
        assert_eq!(
            a.differing(&b),
            vec![ConversionField::Margins, ConversionField::Contrast]
        );
        let mut c = a.clone();
        c.copy_field(&b, ConversionField::Contrast);
        assert_eq!(c.contrast, 80);
        assert_eq!(c.margins, a.margins, "only the named setting is copied");
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
    fn old_device_names_and_missing_devices_are_handled() {
        let s: Settings = serde_json::from_str(r#"{"device":"XteinkX4"}"#).unwrap();
        assert_eq!(s.device, device_profiles::X4_ID);
        assert_eq!(s.device_profile().width, 480);
        let gone: Settings = serde_json::from_str(r#"{"device":"custom-removed"}"#).unwrap();
        assert_eq!(gone.device_profile().id, device_profiles::X4_PRO_ID);
    }

    #[test]
    fn custom_devices_round_trip_and_are_selectable() {
        let mut s = Settings::default();
        let dev = DeviceProfile::custom(&s.devices(), "Kobo Nia", 758, 1024).unwrap();
        s.device = dev.id.clone();
        s.custom_devices.push(dev);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(
            (back.device_profile().width, back.device_profile().height),
            (758, 1024)
        );
        assert!(!back.device_profile().builtin);
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
