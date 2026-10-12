//! Reducing 8-bit grayscale to the 2 or 4 levels the e-paper panel shows.

use crate::gray::{Depth, GrayImage, LevelImage};

/// How to hide the loss of gray levels.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dither {
    /// Nearest level, no dithering. Banding, but crisp line art.
    Threshold,
    /// Floyd-Steinberg error diffusion. Smooth tones; the usual manga default.
    FloydSteinberg,
    /// Atkinson error diffusion. Higher contrast, loses some shadow detail.
    Atkinson,
    /// 4x4 Bayer matrix. Regular pattern, no error propagation.
    Ordered,
}

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Quantize a grayscale image to `depth`, using `dither`.
pub fn quantize(img: &GrayImage, depth: Depth, dither: Dither) -> LevelImage {
    let max = depth.max_level();
    let levels = match dither {
        Dither::Threshold => img.pixels.iter().map(|&p| nearest(p as f32, max)).collect(),
        Dither::FloydSteinberg => diffuse(img, max, FLOYD_STEINBERG),
        Dither::Atkinson => diffuse(img, max, ATKINSON),
        Dither::Ordered => ordered(img, max),
    };
    LevelImage {
        width: img.width,
        height: img.height,
        depth,
        levels,
    }
}

/// Error-diffusion kernel: (dx, dy, weight) with weights over `divisor`.
struct Kernel {
    taps: &'static [(i32, i32, f32)],
    divisor: f32,
}

const FLOYD_STEINBERG: Kernel = Kernel {
    taps: &[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)],
    divisor: 16.0,
};

// Atkinson only diffuses 6/8 of the error, which is what gives it contrast.
const ATKINSON: Kernel = Kernel {
    taps: &[
        (1, 0, 1.0),
        (2, 0, 1.0),
        (-1, 1, 1.0),
        (0, 1, 1.0),
        (1, 1, 1.0),
        (0, 2, 1.0),
    ],
    divisor: 8.0,
};

#[inline]
fn level_value(level: u8, max: u8) -> f32 {
    level as f32 * 255.0 / max as f32
}

#[inline]
fn nearest(value: f32, max: u8) -> u8 {
    let level = (value.clamp(0.0, 255.0) * max as f32 / 255.0).round();
    level as u8
}

fn diffuse(img: &GrayImage, max: u8, kernel: Kernel) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut buf: Vec<f32> = img.pixels.iter().map(|&p| p as f32).collect();
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let old = buf[y * w + x].clamp(0.0, 255.0);
            let level = nearest(old, max);
            out[y * w + x] = level;
            let err = old - level_value(level, max);
            if err == 0.0 {
                continue;
            }
            for &(dx, dy, weight) in kernel.taps {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && (nx as usize) < w && (ny as usize) < h {
                    buf[ny as usize * w + nx as usize] += err * weight / kernel.divisor;
                }
            }
        }
    }
    out
}

fn ordered(img: &GrayImage, max: u8) -> Vec<u8> {
    let w = img.width as usize;
    img.pixels
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let (x, y) = (i % w, i / w);
            let scaled = p as f32 * max as f32 / 255.0;
            let base = scaled.floor();
            let threshold = (BAYER_4X4[y % 4][x % 4] as f32 + 0.5) / 16.0;
            let level = if scaled - base > threshold {
                base + 1.0
            } else {
                base
            };
            level.min(max as f32) as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(value: u8) -> GrayImage {
        GrayImage::new(16, 16, value).unwrap()
    }

    const ALL: [Dither; 4] = [
        Dither::Threshold,
        Dither::FloydSteinberg,
        Dither::Atkinson,
        Dither::Ordered,
    ];

    #[test]
    fn pure_black_and_white_survive_every_method() {
        for dither in ALL {
            for depth in [Depth::One, Depth::Two] {
                let black = quantize(&flat(0), depth, dither);
                let white = quantize(&flat(255), depth, dither);
                assert!(black.levels.iter().all(|&l| l == 0), "{dither:?}/{depth:?}");
                assert!(
                    white.levels.iter().all(|&l| l == depth.max_level()),
                    "{dither:?}/{depth:?}"
                );
            }
        }
    }

    #[test]
    fn two_bit_threshold_picks_nearest_of_four_levels() {
        // Exact level values are 0, 85, 170, 255.
        let img = GrayImage::from_pixels(4, 1, vec![0, 85, 170, 255]).unwrap();
        let q = quantize(&img, Depth::Two, Dither::Threshold);
        assert_eq!(q.levels, vec![0, 1, 2, 3]);
    }

    #[test]
    fn dithering_preserves_average_brightness() {
        // A flat mid-gray should dither to roughly half white in 1-bit.
        for dither in [Dither::FloydSteinberg, Dither::Atkinson, Dither::Ordered] {
            let img = GrayImage::new(64, 64, 128).unwrap();
            let q = quantize(&img, Depth::One, dither);
            let white = q.levels.iter().filter(|&&l| l == 1).count() as f32;
            let ratio = white / q.levels.len() as f32;
            // Atkinson discards part of the error, so allow a wider band.
            assert!((0.35..=0.65).contains(&ratio), "{dither:?}: {ratio}");
        }
    }

    #[test]
    fn output_levels_never_exceed_depth() {
        let img = GrayImage::from_fn(50, 50, |x, y| ((x * 5 + y * 3) % 256) as u8).unwrap();
        for dither in ALL {
            let q = quantize(&img, Depth::Two, dither);
            assert!(q.levels.iter().all(|&l| l <= 3), "{dither:?}");
        }
    }
}
