//! Stage 2: preview how a page will look on the reader.
//!
//! This runs the same conversion as the export ([`process_page`]), so the
//! preview shows exactly the pixels that would be written to the file. It
//! shows the ideal four (or two) gray levels; how the real panel renders
//! them is for a hardware test to confirm.

use crate::error::{PipelineError, Result};
use converter::decode::decode_to_gray;
use converter::process::fit;
use converter::{process_page, GrayImage, LevelImage, ProcessOptions};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

/// A page ready to show: PNG bytes plus its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Decode a saved page for repeated previewing. Decode once, then call
/// [`render_levels`] each time a setting changes.
pub fn decode_page(bytes: &[u8]) -> Result<GrayImage> {
    decode_to_gray(bytes).map_err(|e| PipelineError::Decode(e.to_string()))
}

/// The device pages a saved page becomes: one normally, two for a double-page
/// spread that is split. This is the very function the export uses.
pub fn render_levels(gray: &GrayImage, opts: &ProcessOptions) -> Vec<LevelImage> {
    process_page(gray, opts)
}

/// Show converted pages as PNGs. Levels map evenly onto gray: black, dark
/// gray, light gray, white for four levels; black and white for two.
pub fn levels_to_previews(levels: &[LevelImage]) -> Vec<PreviewImage> {
    levels.iter().map(level_png).collect()
}

fn level_png(img: &LevelImage) -> PreviewImage {
    let max = img.depth.max_level() as u32;
    let pixels: Vec<u8> = img
        .levels
        .iter()
        .map(|&l| (l as u32 * 255 / max) as u8)
        .collect();
    PreviewImage {
        png: encode_png(&pixels, img.width, img.height),
        width: img.width,
        height: img.height,
    }
}

/// The downloaded page itself, shrunk to fit the screen without any
/// conversion, for comparing against the converted result.
pub fn render_original(gray: &GrayImage, opts: &ProcessOptions) -> PreviewImage {
    let fitted = fit(gray, opts.width, opts.height);
    PreviewImage {
        png: encode_png(&fitted.pixels, fitted.width, fitted.height),
        width: fitted.width,
        height: fitted.height,
    }
}

fn encode_png(gray: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(gray, width, height, ExtendedColorType::L8)
        .expect("encoding an in-memory gray image cannot fail");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use converter::{Depth, Dither};

    fn opts(depth: Depth) -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, depth);
        o.dither = Dither::Threshold;
        o
    }

    fn page() -> GrayImage {
        GrayImage::from_fn(600, 900, |x, _| (x * 255 / 599) as u8).unwrap()
    }

    #[test]
    fn previews_are_device_sized_pngs() {
        let levels = render_levels(&page(), &opts(Depth::Two));
        let previews = levels_to_previews(&levels);
        assert_eq!(previews.len(), 1);
        assert_eq!((previews[0].width, previews[0].height), (480, 800));
        assert_eq!(&previews[0].png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn four_levels_show_as_four_grays_and_two_as_black_and_white() {
        let shades = |depth| {
            let previews = levels_to_previews(&render_levels(&page(), &opts(depth)));
            let decoded = image::load_from_memory(&previews[0].png)
                .unwrap()
                .to_luma8();
            let mut seen: Vec<u8> = decoded.pixels().map(|p| p.0[0]).collect();
            seen.sort_unstable();
            seen.dedup();
            seen
        };
        assert_eq!(shades(Depth::Two), vec![0, 85, 170, 255]);
        assert_eq!(shades(Depth::One), vec![0, 255]);
    }

    #[test]
    fn a_spread_previews_as_two_pages() {
        let spread = GrayImage::new(1800, 900, 120).unwrap();
        assert_eq!(
            levels_to_previews(&render_levels(&spread, &opts(Depth::Two))).len(),
            2
        );
    }

    #[test]
    fn the_original_is_fitted_not_converted() {
        let original = render_original(&page(), &opts(Depth::Two));
        assert!(original.width <= 480 && original.height <= 800);
        let decoded = image::load_from_memory(&original.png).unwrap().to_luma8();
        let distinct: std::collections::HashSet<u8> = decoded.pixels().map(|p| p.0[0]).collect();
        assert!(
            distinct.len() > 16,
            "all the original's tones remain, not just four"
        );
    }

    #[test]
    fn unreadable_data_is_a_clear_error() {
        assert!(matches!(
            decode_page(b"not an image"),
            Err(PipelineError::Decode(_))
        ));
    }
}
