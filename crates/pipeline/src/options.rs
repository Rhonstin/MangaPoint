//! Mapping saved settings onto the converter's options.

use converter::{Depth, ProcessOptions};
use library::Settings;

/// Everything the converter needs for pages shown on the chosen device.
pub fn process_options(settings: &Settings) -> ProcessOptions {
    let profile = settings.device.profile();
    let c = &settings.conversion;
    ProcessOptions {
        width: profile.width,
        height: profile.height,
        layout: c.layout,
        crop: c.crop,
        spread: c.spread,
        direction: c.direction,
        contrast: c.contrast,
        auto_levels: c.auto_levels,
        overlap_percent: c.overlap_percent,
        dither: c.dither,
        depth: depth_of(c.format),
    }
}

fn depth_of(format: converter::xtc::Format) -> Depth {
    format.depth()
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::xtc::Format;

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
}
