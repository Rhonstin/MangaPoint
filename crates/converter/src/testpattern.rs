//! Calibration pages for checking a file on real hardware.
//!
//! None of these depend on the converter's dither or crop logic being right,
//! so when a pattern looks wrong on the device the fault is in the encoding
//! (bit order, column order, gray mapping), not in image processing.
//!
//! How to read the result is described next to each pattern.

use crate::gray::GrayImage;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 800;

const BLACK: u8 = 0;
const DARK: u8 = 85;
const LIGHT: u8 = 170;
const WHITE: u8 = 255;

/// A named calibration page.
pub struct Pattern {
    pub name: &'static str,
    pub image: GrayImage,
}

/// All calibration pages, in the order they appear in the test file.
pub fn all() -> Vec<Pattern> {
    vec![
        Pattern {
            name: "1 Orientation",
            image: orientation(),
        },
        Pattern {
            name: "2 Gray bands",
            image: gray_bands(),
        },
        Pattern {
            name: "3 Checkerboards",
            image: checkerboards(),
        },
        Pattern {
            name: "4 Ramps",
            image: ramps(),
        },
    ]
}

fn blank() -> GrayImage {
    GrayImage::new(WIDTH, HEIGHT, WHITE).expect("constant size is non-zero")
}

/// Detects mirrored, flipped or rotated output (wrong column order, wrong
/// bit order, swapped width/height).
///
/// Expected on the device: a thin black border; a large black square in the
/// TOP-LEFT corner, a medium one in the TOP-RIGHT, a small one in the
/// BOTTOM-LEFT and none in the bottom-right; and a letter "F" in the middle
/// reading normally (stem on the left, arms pointing right).
fn orientation() -> GrayImage {
    let mut img = blank();
    let (w, h) = (WIDTH, HEIGHT);
    img.fill_rect(0, 0, w, 6, BLACK);
    img.fill_rect(0, h - 6, w, 6, BLACK);
    img.fill_rect(0, 0, 6, h, BLACK);
    img.fill_rect(w - 6, 0, 6, h, BLACK);
    img.fill_rect(6, 6, 80, 80, BLACK);
    img.fill_rect(w - 6 - 40, 6, 40, 40, BLACK);
    img.fill_rect(6, h - 6 - 20, 20, 20, BLACK);
    // "F": stem plus two arms, shorter lower arm.
    img.fill_rect(180, 250, 40, 300, BLACK);
    img.fill_rect(180, 250, 160, 40, BLACK);
    img.fill_rect(180, 380, 110, 40, BLACK);
    img
}

/// Checks the 2-bit gray mapping. Four horizontal bands.
///
/// Expected on the device, top to bottom: WHITE, LIGHT gray, DARK gray,
/// BLACK. The white strip on the left of each band carries 1, 2, 3 and 4
/// black squares so the bands cannot be confused. If the order of shades is
/// reversed or the two grays are swapped, only [`crate::xtc::GRAY_MAP`] needs
/// changing. In a 1-bit (.xtc) file only white and black are possible.
fn gray_bands() -> GrayImage {
    let mut img = blank();
    let shades = [WHITE, LIGHT, DARK, BLACK];
    for (i, shade) in shades.into_iter().enumerate() {
        let top = i as u32 * 150;
        img.fill_rect(60, top, WIDTH - 60, 150, shade);
        for n in 0..=i as u32 {
            img.fill_rect(15, top + 15 + n * 30, 20, 20, BLACK);
        }
    }
    // Bottom 200 px: horizontal gradient, to eyeball dithering.
    for y in 600..HEIGHT {
        for x in 0..WIDTH {
            img.set(x, y, (x * 255 / (WIDTH - 1)) as u8);
        }
    }
    img
}

/// Checks pixel-exact addressing. Top half 1-pixel checkerboard, bottom half
/// 8-pixel checkerboard.
///
/// Expected on the device: a fine even gray texture on top (no stripes, no
/// moire) and a coarse board of 8 px squares below, starting black in the
/// top-left of each half.
fn checkerboards() -> GrayImage {
    GrayImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let cell = if y < HEIGHT / 2 { 1 } else { 8 };
        let on = ((x / cell) + (y / cell)) % 2 == 0;
        if on {
            BLACK
        } else {
            WHITE
        }
    })
    .expect("constant size is non-zero")
}

/// Smooth tone ramps for judging dithering. Top half: black to white left to
/// right. Bottom half: black at the top of the half to white at the bottom.
fn ramps() -> GrayImage {
    GrayImage::from_fn(WIDTH, HEIGHT, |x, y| {
        if y < HEIGHT / 2 {
            (x * 255 / (WIDTH - 1)) as u8
        } else {
            ((y - HEIGHT / 2) * 255 / (HEIGHT / 2 - 1)) as u8
        }
    })
    .expect("constant size is non-zero")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_device_sized_and_distinct() {
        let all = all();
        assert_eq!(all.len(), 4);
        for p in &all {
            assert_eq!((p.image.width, p.image.height), (WIDTH, HEIGHT));
        }
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i].image, all[j].image);
            }
        }
    }

    #[test]
    fn orientation_page_is_not_symmetric() {
        let img = orientation();
        // Corner markers differ in size, so no flip or rotation maps the page onto itself.
        assert_eq!(img.get(10, 10), BLACK);
        assert_eq!(img.get(WIDTH - 10, 10), BLACK);
        assert_eq!(img.get(10, HEIGHT - 10), BLACK);
        assert_eq!(img.get(WIDTH - 10, HEIGHT - 10), WHITE);
        assert_eq!(img.get(WIDTH - 50, 60), WHITE); // outside the 40px top-right square
        assert_eq!(img.get(80, 80), BLACK); // inside the 80px top-left square
        assert_eq!(img.get(WIDTH - 20, 60), WHITE);
    }

    #[test]
    fn bands_use_four_distinct_shades_in_order() {
        let img = gray_bands();
        let at = |band: u32| img.get(300, band * 150 + 75);
        assert_eq!([at(0), at(1), at(2), at(3)], [WHITE, LIGHT, DARK, BLACK]);
    }
}
