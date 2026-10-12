//! Planning a chapter's device pages before any are rendered.
//!
//! The old conversion looked at one downloaded image at a time. That cuts a
//! webtoon that arrives as many short images at the seams and gives the user
//! nothing to correct. Here a chapter is first *prepared* (cropped, and for a
//! webtoon stitched into one continuous strip), then *planned* (a list of the
//! pages it becomes), and only then *rendered*. The plan is plain data, so a
//! screen can show it next to the downloaded pages.
//!
//! * Paged chapters: one page per image. A double-page spread is split at its
//!   spine, rotated, or fitted, as the settings say.
//! * Webtoon chapters: every image is scaled to the screen width and joined.
//!   Pages are cut at the blank gutters between scenes when there is one near
//!   the bottom of the screen, and overlap slightly when there is not.
//!
//! Positions in a strip are also given in *units* (the strip scaled to 1000
//! wide), which do not depend on the device.

use crate::gray::{GrayImage, LevelImage};
use crate::process::{
    border_median, crop_bounds, crop_rect, finish, fit, letterbox_top, resize, rotate_cw,
    trim_bounds, trim_threshold, CropMode, Layout, Margins, ProcessOptions, SpreadMode,
};
use crate::xtc::ReadDirection;

/// The width a strip has in unit coordinates.
pub const UNIT_WIDTH: u32 = 1000;

/// A webtoon chapter is recognised when its images are, typically, at least
/// this many times taller than wide.
const STRIP_ASPECT: f32 = 2.0;

/// Which part of a paged image a page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Whole,
    /// Turned a quarter turn clockwise to use the screen's full height.
    Rotated,
    /// One half of a spread: 0 is the half read first.
    Half(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Image {
        index: usize,
        part: Part,
    },
    /// Rows of the stitched strip, in device pixels.
    Strip {
        top: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPage {
    pub source: Source,
    /// Where the page starts in the strip, in units. 0 for paged chapters.
    pub unit_top: u32,
    /// How tall the page's slice of the strip is, in units. 0 for paged
    /// chapters.
    pub unit_height: u32,
    /// Paged: the image is a landscape double page.
    pub landscape: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterPlan {
    /// A stitched webtoon strip rather than one page per image.
    pub strip: bool,
    pub pages: Vec<PlannedPage>,
    /// Total strip height in units; 0 for paged chapters.
    pub unit_height: u32,
}

/// A chapter, cropped and (for a webtoon) stitched, ready to plan and render.
pub struct Prepared {
    kind: Kind,
}

enum Kind {
    Paged(Vec<GrayImage>),
    Strip(Strip),
}

struct Strip {
    width: u32,
    /// Each image scaled to `width`, top to bottom.
    parts: Vec<GrayImage>,
    /// First row of each part.
    starts: Vec<u32>,
    total: u32,
    /// Per row: nothing but background.
    blank: Vec<bool>,
}

impl Prepared {
    pub fn is_strip(&self) -> bool {
        matches!(self.kind, Kind::Strip(_))
    }

    /// The number of source images.
    pub fn image_count(&self) -> usize {
        match &self.kind {
            Kind::Paged(images) => images.len(),
            Kind::Strip(strip) => strip.parts.len(),
        }
    }
}

/// Whether `opts.layout` makes this chapter a stitched strip.
pub fn is_strip_chapter(images: &[(u32, u32)], opts: &ProcessOptions) -> bool {
    match opts.layout {
        Layout::Webtoon => true,
        Layout::Manga => false,
        Layout::Auto => {
            if images.is_empty() {
                return false;
            }
            let mut ratios: Vec<f32> = images
                .iter()
                .map(|&(w, h)| h as f32 / w.max(1) as f32)
                .collect();
            ratios.sort_by(|a, b| a.total_cmp(b));
            ratios[ratios.len() / 2] >= STRIP_ASPECT
        }
    }
}

/// Crop and, for a webtoon, stitch. `images` are the chapter's pages in order.
pub fn prepare(images: &[GrayImage], opts: &ProcessOptions) -> Prepared {
    let dims: Vec<(u32, u32)> = images.iter().map(|i| (i.width, i.height)).collect();
    if is_strip_chapter(&dims, opts) {
        Prepared {
            kind: Kind::Strip(build_strip(images, opts)),
        }
    } else {
        let cropped = images
            .iter()
            .map(|img| {
                let (x, y, w, h) = crop_bounds(img, opts);
                if (x, y, w, h) == (0, 0, img.width, img.height) {
                    img.clone()
                } else {
                    crop_rect(img, x, y, w, h)
                }
            })
            .collect();
        Prepared {
            kind: Kind::Paged(cropped),
        }
    }
}

fn build_strip(images: &[GrayImage], opts: &ProcessOptions) -> Strip {
    let side = |p: u8| p.min(Margins::MAX_PERCENT) as u64;
    let cut = |len: u32, p: u8| (len as u64 * side(p) / 100) as u32;

    // Fixed margins first. Top and bottom apply to every image (a banner on
    // each slice); left and right are the same cut everywhere.
    let fixed: Vec<GrayImage> = images
        .iter()
        .map(|img| {
            let x = cut(img.width, opts.margins.left);
            let y = cut(img.height, opts.margins.top);
            let w = img
                .width
                .saturating_sub(x + cut(img.width, opts.margins.right))
                .max(1);
            let h = img
                .height
                .saturating_sub(y + cut(img.height, opts.margins.bottom))
                .max(1);
            crop_rect(img, x, y, w, h)
        })
        .collect();

    // Blank side margins: one cut for the whole chapter, so every image keeps
    // the same width and the strip does not jitter sideways.
    let (left, right) = if opts.crop == CropMode::Margins {
        let threshold = trim_threshold(opts.trim_sensitivity);
        let mut left = 1.0f32;
        let mut right = 0.0f32;
        let mut any = false;
        for img in &fixed {
            if let Some((x, _, w, _)) = trim_bounds(img, threshold, opts.trim_padding) {
                left = left.min(x as f32 / img.width as f32);
                right = right.max((x + w) as f32 / img.width as f32);
                any = true;
            } else {
                left = 0.0;
                right = 1.0;
                any = true;
            }
        }
        if any && right > left {
            (left, right)
        } else {
            (0.0, 1.0)
        }
    } else {
        (0.0, 1.0)
    };

    let width = opts.width;
    let threshold = trim_threshold(opts.trim_sensitivity);
    let mut parts = Vec::with_capacity(fixed.len());
    let mut starts = Vec::with_capacity(fixed.len());
    let mut blank = Vec::new();
    let mut total = 0u32;
    for img in &fixed {
        let x = (left * img.width as f32).floor() as u32;
        let w = (((right - left) * img.width as f32).round() as u32).clamp(1, img.width - x);
        let sideways = if (x, w) == (0, img.width) {
            img.clone()
        } else {
            crop_rect(img, x, 0, w, img.height)
        };
        let scaled_h = (sideways.height as f64 * width as f64 / sideways.width as f64)
            .round()
            .max(1.0) as u32;
        let scaled = resize(&sideways, width, scaled_h);
        let bg = border_median(&scaled);
        let row_limit = (width / 200).max(1);
        for y in 0..scaled.height {
            let ink = (0..width)
                .filter(|&x| (scaled.get(x, y) as i16 - bg as i16).abs() > threshold)
                .count() as u32;
            blank.push(ink <= row_limit);
        }
        starts.push(total);
        total += scaled.height;
        parts.push(scaled);
    }
    Strip {
        width,
        parts,
        starts,
        total,
        blank,
    }
}

fn to_units(px: u32, width: u32) -> u32 {
    (px as u64 * UNIT_WIDTH as u64 / width.max(1) as u64) as u32
}

/// Decide the pages of a prepared chapter.
pub fn plan(prepared: &Prepared, opts: &ProcessOptions) -> ChapterPlan {
    match &prepared.kind {
        Kind::Paged(images) => plan_paged(images, opts),
        Kind::Strip(strip) => plan_strip(strip, opts),
    }
}

fn plan_paged(images: &[GrayImage], opts: &ProcessOptions) -> ChapterPlan {
    let mut pages = Vec::new();
    for (index, img) in images.iter().enumerate() {
        let landscape = img.width as f32 > img.height as f32 * 1.1;
        let parts: &[Part] = if !landscape {
            &[Part::Whole]
        } else {
            match opts.spread {
                SpreadMode::Fit => &[Part::Whole],
                SpreadMode::Rotate => &[Part::Rotated],
                SpreadMode::Split => &[Part::Half(0), Part::Half(1)],
            }
        };
        for &part in parts {
            pages.push(PlannedPage {
                source: Source::Image { index, part },
                unit_top: 0,
                unit_height: 0,
                landscape,
            });
        }
    }
    ChapterPlan {
        strip: false,
        pages,
        unit_height: 0,
    }
}

fn plan_strip(strip: &Strip, opts: &ProcessOptions) -> ChapterPlan {
    let screen = opts.height;
    let overlap = screen * opts.overlap_percent.min(50) as u32 / 100;
    let min_fill = screen * 55 / 100;
    let min_gutter = (screen / 60).max(4);
    let pad = (screen / 100).clamp(2, 10);

    let mut spans: Vec<(u32, u32)> = Vec::new(); // top, height
    let mut start = 0u32;
    while start < strip.total {
        let remaining = strip.total - start;
        if remaining <= screen {
            spans.push((start, remaining));
            break;
        }
        let ideal_end = start + screen;
        // The latest blank gutter in the lower part of the screen.
        let (end, next) = match latest_gutter(strip, start + min_fill, ideal_end, min_gutter) {
            Some((a, b)) => {
                let end = (a + pad).min(ideal_end);
                (end, b.saturating_sub(pad).max(end))
            }
            None => (ideal_end, ideal_end - overlap),
        };
        spans.push((start, end - start));
        start = next.max(start + 1);
    }

    let pages = spans
        .into_iter()
        .filter(|&(top, height)| {
            // Pages that are only background are dropped: trailing whitespace.
            (top..top + height).any(|y| !strip.blank.get(y as usize).copied().unwrap_or(true))
        })
        .map(|(top, height)| PlannedPage {
            source: Source::Strip { top, height },
            unit_top: to_units(top, strip.width),
            unit_height: to_units(height, strip.width),
            landscape: false,
        })
        .collect();
    ChapterPlan {
        strip: true,
        pages,
        unit_height: to_units(strip.total, strip.width),
    }
}

/// The last run of at least `min_len` blank rows that lies inside
/// `[from, to)`, as `(first_row, one_past_last_row)`.
fn latest_gutter(strip: &Strip, from: u32, to: u32, min_len: u32) -> Option<(u32, u32)> {
    let to = to.min(strip.total);
    let mut best = None;
    let mut run_start: Option<u32> = None;
    for y in from..=to {
        let blank = y < to && strip.blank.get(y as usize).copied().unwrap_or(false);
        match (blank, run_start) {
            (true, None) => run_start = Some(y),
            (false, Some(a)) => {
                if y - a >= min_len {
                    best = Some((a, y));
                }
                run_start = None;
            }
            _ => {}
        }
    }
    best
}

/// Render one planned page to device pixels and levels.
pub fn render_page(prepared: &Prepared, page: &PlannedPage, opts: &ProcessOptions) -> LevelImage {
    let screen = match (&prepared.kind, page.source) {
        (Kind::Strip(strip), Source::Strip { top, height }) => {
            letterbox_top(&strip_rows(strip, top, height), opts.width, opts.height)
        }
        (Kind::Paged(images), Source::Image { index, part }) => {
            paged_screen(&images[index], part, opts)
        }
        _ => unreachable!("a plan is only rendered with the chapter it was made from"),
    };
    finish(screen, opts)
}

fn paged_screen(img: &GrayImage, part: Part, opts: &ProcessOptions) -> GrayImage {
    match part {
        Part::Whole => fit(img, opts.width, opts.height),
        Part::Rotated => fit(&rotate_cw(img), opts.width, opts.height),
        Part::Half(n) => {
            let mid = spine(img);
            let left = crop_rect(img, 0, 0, mid, img.height);
            let right = crop_rect(img, mid, 0, img.width - mid, img.height);
            let first_is_right = opts.direction == ReadDirection::RightToLeft;
            let half = match (n, first_is_right) {
                (0, true) | (1, false) => right,
                _ => left,
            };
            fit(&half, opts.width, opts.height)
        }
    }
}

/// Where to cut a double page: the blank column nearest the middle, or the
/// middle itself when the art runs across the fold.
fn spine(img: &GrayImage) -> u32 {
    let (w, h) = (img.width, img.height);
    let centre = w / 2;
    let bg = border_median(img);
    let threshold = trim_threshold(crate::process::DEFAULT_TRIM_SENSITIVITY);
    let limit = (h / 200).max(1);
    let blank = |x: u32| {
        (0..h)
            .filter(|&y| (img.get(x, y) as i16 - bg as i16).abs() > threshold)
            .count() as u32
            <= limit
    };
    let lo = w * 2 / 5;
    let hi = (w * 3 / 5).min(w - 1);
    let mut best: Option<(u32, u32)> = None; // (distance of run middle, middle)
    let mut run: Option<u32> = None;
    for x in lo..=hi + 1 {
        let b = x <= hi && blank(x);
        match (b, run) {
            (true, None) => run = Some(x),
            (false, Some(a)) => {
                let mid = (a + x) / 2;
                let d = mid.abs_diff(centre);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, mid));
                }
                run = None;
            }
            _ => {}
        }
    }
    best.map_or(centre, |(_, m)| m)
        .clamp(1, w.saturating_sub(1).max(1))
}

/// Rows `[top, top + height)` of the strip.
fn strip_rows(strip: &Strip, top: u32, height: u32) -> GrayImage {
    let height = height.min(strip.total.saturating_sub(top)).max(1);
    let mut pixels = Vec::with_capacity(strip.width as usize * height as usize);
    let end = top + height;
    for (part, &first) in strip.parts.iter().zip(&strip.starts) {
        let last = first + part.height;
        if last <= top || first >= end {
            continue;
        }
        let from = top.max(first) - first;
        let to = end.min(last) - first;
        let w = strip.width as usize;
        pixels.extend_from_slice(&part.pixels[from as usize * w..to as usize * w]);
    }
    GrayImage::from_pixels(strip.width, height, pixels).expect("rows from the strip fit its size")
}

/// A strip page's rows as downloaded (scaled to the screen width, tones
/// untouched), to show beside the converted page. `None` for a paged chapter.
pub fn original_rows(prepared: &Prepared, page: &PlannedPage) -> Option<GrayImage> {
    match (&prepared.kind, page.source) {
        (Kind::Strip(strip), Source::Strip { top, height }) => Some(strip_rows(strip, top, height)),
        _ => None,
    }
}

/// Plan and render a whole chapter.
pub fn convert_chapter(images: &[GrayImage], opts: &ProcessOptions) -> Vec<LevelImage> {
    let prepared = prepare(images, opts);
    let plan = plan(&prepared, opts);
    render_all(&prepared, &plan.pages, opts)
}

/// Render pages, using every core of the machine: each page is independent,
/// and resizing, contrast and dithering are most of an export's time. The
/// pages come back in order.
#[cfg(not(target_arch = "wasm32"))]
fn render_all(
    prepared: &Prepared,
    pages: &[PlannedPage],
    opts: &ProcessOptions,
) -> Vec<LevelImage> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(pages.len());
    if workers <= 1 {
        return pages
            .iter()
            .map(|page| render_page(prepared, page, opts))
            .collect();
    }
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<LevelImage>> = (0..pages.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(page) = pages.get(i) else { break };
                        done.push((i, render_page(prepared, page, opts)));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            for (i, image) in handle.join().expect("a page renderer panicked") {
                slots[i] = Some(image);
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.expect("every page was rendered"))
        .collect()
}

/// The website runs on one thread.
#[cfg(target_arch = "wasm32")]
fn render_all(
    prepared: &Prepared,
    pages: &[PlannedPage],
    opts: &ProcessOptions,
) -> Vec<LevelImage> {
    pages
        .iter()
        .map(|page| render_page(prepared, page, opts))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dither::Dither;
    use crate::gray::Depth;

    fn opts() -> ProcessOptions {
        let mut o = ProcessOptions::manga_default(480, 800, Depth::Two);
        o.layout = Layout::Webtoon;
        o.crop = CropMode::Full;
        o.auto_levels = false;
        o.contrast = 50;
        o.dither = Dither::Threshold;
        o
    }

    /// `n` images 480 wide: dark content with a white gutter every `scene`
    /// rows, `height` rows tall each.
    fn scenes(n: usize, height: u32, scene: u32, gutter: u32) -> Vec<GrayImage> {
        (0..n)
            .map(|i| {
                GrayImage::from_fn(480, height, |x, y| {
                    let row = i as u32 * height + y;
                    let in_gutter = row % scene >= scene - gutter;
                    if in_gutter || x < 20 {
                        255
                    } else {
                        40
                    }
                })
                .unwrap()
            })
            .collect()
    }

    fn tops(plan: &ChapterPlan) -> Vec<u32> {
        plan.pages
            .iter()
            .map(|p| match p.source {
                Source::Strip { top, .. } => top,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn many_short_images_are_stitched_into_one_strip() {
        // Six 700-row images: no image alone fills a page, together 4200 rows.
        let images = scenes(6, 700, 100_000, 0);
        let o = opts();
        let prepared = prepare(&images, &o);
        assert!(prepared.is_strip());
        let plan = plan(&prepared, &o);
        assert!(plan.strip);
        // No gutters: 800-row pages overlapping by 64: tops 0, 736, 1472, ...
        assert_eq!(&tops(&plan)[..3], &[0, 736, 1472]);
        // Crossing an image seam changes nothing: the page at 736 starts
        // inside image 1 and is a single continuous slice.
        let rows = original_rows(&prepared, &plan.pages[1]).unwrap();
        assert_eq!(rows.height, 800);
    }

    #[test]
    fn pages_are_cut_in_the_gutters_between_scenes() {
        // A 90-row white gutter ends every 600-row scene.
        let images = scenes(1, 6000, 600, 90);
        let o = opts();
        let prepared = prepare(&images, &o);
        let plan = plan(&prepared, &o);
        let Source::Strip { top, height } = plan.pages[0].source else {
            unreachable!()
        };
        assert_eq!(top, 0);
        // The first page ends where scene 1's gutter begins (rows 510..600),
        // a little into it, not at the 800-row screen limit.
        assert!((510..=530).contains(&height), "height {height}");
        // The second page starts in or just before the next scene's content,
        // after the gutter.
        assert!((590..=600).contains(&tops(&plan)[1]), "{:?}", tops(&plan));
    }

    #[test]
    fn trailing_whitespace_does_not_become_a_page() {
        let mut img = GrayImage::new(480, 5000, 255).unwrap();
        img.fill_rect(20, 0, 440, 1500, 40);
        let o = opts();
        let prepared = prepare(&[img], &o);
        let plan = plan(&prepared, &o);
        assert!(plan.pages.len() <= 3, "{} pages", plan.pages.len());
        assert!(plan.pages.len() >= 2);
    }

    #[test]
    fn every_page_is_exactly_the_device_size_for_any_device() {
        for (w, h) in [(480, 800), (528, 792), (758, 1024), (200, 300)] {
            let mut o = opts();
            o.width = w;
            o.height = h;
            let images = scenes(3, 1200, 700, 80);
            for page in convert_chapter(&images, &o) {
                assert_eq!((page.width, page.height), (w, h));
            }
        }
    }

    #[test]
    fn auto_layout_stitches_tall_chapters_and_pages_wide_ones() {
        let mut o = opts();
        o.layout = Layout::Auto;
        let tall = scenes(2, 1500, 100_000, 0); // 480x1500 = 3.1x
        assert!(prepare(&tall, &o).is_strip());
        let pages: Vec<GrayImage> = (0..2)
            .map(|_| GrayImage::new(1000, 1500, 200).unwrap())
            .collect();
        assert!(!prepare(&pages, &o).is_strip());
    }

    #[test]
    fn a_paged_chapter_is_one_page_per_image_and_spreads_split_in_reading_order() {
        let mut o = opts();
        o.layout = Layout::Manga;
        o.direction = ReadDirection::RightToLeft;
        let page = GrayImage::new(1000, 1500, 200).unwrap();
        let spread = GrayImage::from_fn(1600, 1000, |x, _| if x < 800 { 0 } else { 255 }).unwrap();
        let prepared = prepare(&[page, spread], &o);
        let p = plan(&prepared, &o);
        assert_eq!(p.pages.len(), 3);
        let first = render_page(&prepared, &p.pages[1], &o);
        assert_eq!(first.get(240, 400), 3, "right half (white) is read first");
        let second = render_page(&prepared, &p.pages[2], &o);
        assert_eq!(second.get(240, 400), 0);
    }

    #[test]
    fn spread_fit_and_turn_give_one_page() {
        let spread = GrayImage::new(1600, 1000, 128).unwrap();
        for mode in [SpreadMode::Fit, SpreadMode::Rotate] {
            let mut o = opts();
            o.layout = Layout::Manga;
            o.spread = mode;
            let prepared = prepare(std::slice::from_ref(&spread), &o);
            assert_eq!(plan(&prepared, &o).pages.len(), 1, "{mode:?}");
        }
    }

    #[test]
    fn a_spread_is_cut_at_the_spine_when_there_is_a_blank_one() {
        // Art on both sides with a blank band a little left of centre.
        let img = GrayImage::from_fn(1000, 800, |x, y| {
            let margin = !(30..970).contains(&x) || !(30..770).contains(&y);
            if margin || (440..470).contains(&x) {
                255
            } else {
                30
            }
        })
        .unwrap();
        assert!((450..=460).contains(&spine(&img)), "{}", spine(&img));
        let solid = GrayImage::from_fn(1000, 800, |x, y| {
            if !(30..970).contains(&x) || !(30..770).contains(&y) {
                255
            } else {
                30
            }
        })
        .unwrap();
        assert_eq!(spine(&solid), 500, "no gap: the middle");
    }
}
