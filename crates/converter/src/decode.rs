//! Decoding page images (JPEG, PNG, WebP, GIF) into grayscale.
//!
//! Behind the `decode` feature: the desktop app uses it; the website decodes
//! with the browser instead and hands raw pixels to the pipeline.

use crate::gray::GrayImage;
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

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_to_gray(b"definitely not an image").is_err());
        assert!(decode_to_gray(&[]).is_err());
    }
}
