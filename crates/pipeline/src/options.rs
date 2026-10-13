//! Mapping saved settings onto the converter's options.

use converter::ProcessOptions;
use device_profiles::DeviceProfile;
use library::{ConversionSettings, Settings};

/// Everything the converter needs for pages shown on `device`, converted the
/// way `conversion` says. Previews and exports both call this, so what you
/// see is what gets written.
pub fn process_options_for(
    device: &DeviceProfile,
    conversion: &ConversionSettings,
) -> ProcessOptions {
    ProcessOptions {
        width: device.width,
        height: device.height,
        layout: conversion.layout,
        crop: conversion.crop,
        margins: conversion.margins,
        trim_sensitivity: conversion.trim_sensitivity,
        trim_padding: conversion.trim_padding,
        spread: conversion.spread,
        direction: conversion.direction,
        contrast: conversion.contrast,
        auto_levels: conversion.auto_levels,
        strip_cut: conversion.strip_cut,
        overlap_percent: conversion.overlap_percent,
        dither: conversion.dither,
        depth: conversion.format.depth(),
    }
}

/// The same, using the saved defaults.
pub fn process_options(settings: &Settings) -> ProcessOptions {
    process_options_for(&settings.device_profile(), &settings.conversion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::xtc::Format;
    use converter::Depth;

    #[test]
    fn follows_device_and_format() {
        let mut s = Settings::default();
        let o = process_options(&s);
        assert_eq!((o.width, o.height), (480, 800));
        assert_eq!(o.depth, Depth::Two, "default is XTCH");
        assert_eq!(o.contrast, 55);

        s.conversion.format = Format::Xtc;
        assert_eq!(process_options(&s).depth, Depth::One);
    }

    #[test]
    fn per_series_settings_override_the_defaults() {
        let conversion = ConversionSettings {
            contrast: 80,
            format: Format::Xtc,
            ..Default::default()
        };
        let o = process_options_for(&device_profiles::x4(), &conversion);
        assert_eq!(o.contrast, 80);
        assert_eq!(o.depth, Depth::One);
    }
}
