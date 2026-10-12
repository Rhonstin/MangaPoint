//! User preferences.

use converter::process::{CropMode, Layout, SpreadMode};
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
    pub crop: CropMode,
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
            spread: SpreadMode::Split,
            contrast: 55,
            auto_levels: true,
            overlap_percent: 8,
            dither: Dither::FloydSteinberg,
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
