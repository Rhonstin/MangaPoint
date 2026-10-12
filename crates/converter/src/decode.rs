//! Decoding page images (JPEG, PNG, WebP, GIF) into grayscale.
//!
//! Behind the `decode` feature: the desktop app uses it; the website decodes
//! with the browser instead and hands raw pixels to the pipeline.

use crate::gray::GrayImage;
use image::ImageEncoder;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("could not decode image: {0}")]
    Image(#[from] image::ImageError),
    #[error("could not read image data: {0}")]
    Io(#[from] std::io::Error),
    #[error("image is {width}x{height}, which is larger than the {limit} megapixel limit")]
    TooLarge { width: u32, height: u32, limit: u32 },
}

/// Refuse absurd images (decompression bombs) before allocating for them.
const MAX_MEGAPIXELS: u32 = 120;

/// Decode to 8-bit luma. Transparent areas are composited onto white first,
/// since a transparent background would otherwise read as black.
pub fn decode_to_gray(bytes: &[u8]) -> Result<GrayImage, DecodeError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let (w, h) = reader.into_dimensions()?;
    if (w as u64 * h as u64) > MAX_MEGAPIXELS as u64 * 1_000_000 {
        return Err(DecodeError::TooLarge {
            width: w,
            height: h,
            limit: MAX_MEGAPIXELS,
        });
    }
    let img = image::load_from_memory(bytes)?;
    let rgba = img.to_rgba8();
    let pixels = rgba
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            let alpha = a as f32 / 255.0;
            (luma * alpha + 255.0 * (1.0 - alpha)).round() as u8
        })
        .collect();
    Ok(GrayImage::from_pixels(rgba.width(), rgba.height(), pixels).expect("sizes match"))
}

/// Pixel size of an image, read from its header without decoding it.
pub fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32), DecodeError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    Ok(reader.into_dimensions()?)
}

/// Tallest page we keep when the user chooses to save space. Twice the
/// reader's 800-pixel screen height, so nothing visible is lost.
pub const STORAGE_MAX_HEIGHT: u32 = 1600;

/// Re-encode a page smaller for storage, or `None` if it is already small
/// enough (so the original bytes should be kept untouched).
///
/// Portrait pages are limited to `max_height` tall. Landscape double-page
/// spreads are limited to twice that wide, so each half still has the full
/// height it will be split into. The result is a grayscale JPEG, which is
/// all the reader can show.
pub fn shrink_for_storage(
    bytes: &[u8],
    max_height: u32,
    quality: u8,
) -> Result<Option<Vec<u8>>, DecodeError> {
    let (w, h) = image_dimensions(bytes)?;
    let landscape = w as f32 > h as f32 * 1.1;
    let scale = if landscape {
        (2.0 * max_height as f64) / w as f64
    } else {
        max_height as f64 / h as f64
    };
    if scale >= 1.0 {
        return Ok(None);
    }
    let gray = decode_to_gray(bytes)?;
    let new_w = ((gray.width as f64 * scale).round() as u32).max(1);
    let new_h = ((gray.height as f64 * scale).round() as u32).max(1);
    let small = crate::process::resize(&gray, new_w, new_h);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).write_image(
        &small.pixels,
        new_w,
        new_h,
        image::ExtendedColorType::L8,
    )?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        out
    }

    #[test]
    fn decodes_png_to_luma() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
        let g = decode_to_gray(&png(&img)).unwrap();
        assert_eq!((g.width, g.height), (2, 1));
        assert_eq!(g.pixels, vec![0, 255]);
    }

    #[test]
    fn transparent_pixels_become_white() {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        assert_eq!(decode_to_gray(&png(&img)).unwrap().pixels, vec![255]);
    }

    fn plain_png(w: u32, h: u32) -> Vec<u8> {
        png(&RgbaImage::from_pixel(w, h, Rgba([120, 120, 120, 255])))
    }

    #[test]
    fn reads_dimensions_without_decoding() {
        assert_eq!(image_dimensions(&plain_png(37, 91)).unwrap(), (37, 91));
        assert!(image_dimensions(b"nope").is_err());
    }

    #[test]
    fn small_pages_are_kept_exactly_as_downloaded() {
        assert_eq!(
            shrink_for_storage(&plain_png(900, 1600), 1600, 85).unwrap(),
            None
        );
        assert_eq!(
            shrink_for_storage(&plain_png(500, 700), 1600, 85).unwrap(),
            None
        );
    }

    #[test]
    fn tall_pages_shrink_to_the_height_limit_as_gray_jpeg() {
        let out = shrink_for_storage(&plain_png(1200, 3200), 1600, 85)
            .unwrap()
            .expect("taller than the limit, so it must shrink");
        assert_eq!(&out[..3], &[0xFF, 0xD8, 0xFF], "JPEG");
        assert_eq!(
            image_dimensions(&out).unwrap(),
            (600, 1600),
            "aspect ratio kept"
        );
        let back = decode_to_gray(&out).unwrap();
        // Flat mid-gray stays mid-gray.
        assert!(back.pixels.iter().all(|&p| (115..=125).contains(&p)));
    }

    #[test]
    fn spreads_may_be_twice_as_wide_so_each_half_keeps_full_height() {
        // 2400x1500 fits (limit is 3200 wide); 4000x2500 does not.
        assert_eq!(
            shrink_for_storage(&plain_png(2400, 1500), 1600, 85).unwrap(),
            None
        );
        let out = shrink_for_storage(&plain_png(4000, 2500), 1600, 85)
            .unwrap()
            .unwrap();
        assert_eq!(image_dimensions(&out).unwrap(), (3200, 2000));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_to_gray(b"definitely not an image").is_err());
        assert!(decode_to_gray(&[]).is_err());
    }
}
