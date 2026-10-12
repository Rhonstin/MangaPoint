//! Two small pixel-art scenes for empty screens, both starring the gray cat:
//! one searching with a magnifying glass, one fishing in an empty bucket while
//! it waits for downloads. Original art drawn in code as SVG rectangles; the
//! motion is CSS (see `style.css`).

use super::cat::sitting_markup;
use dioxus::prelude::*;
use std::sync::OnceLock;

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    out.push_str(&format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>"
    ));
}

/// The pixels of a ring around (`cx`, `cy`) between two radii.
fn ring(out: &mut String, cx: f32, cy: f32, inner: f32, outer: f32, fill: &str) {
    for y in 0..32 {
        for x in 0..44 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if d >= inner && d <= outer {
                rect(out, x, y, 1, 1, fill);
            }
        }
    }
}

/// The cat, blinking now and then: an open-eyed layer and a blinking one.
fn blinking_cat(paw_top: usize) -> String {
    format!(
        "<g class=\"look-open\">{}</g><g class=\"look-blink\">{}</g>",
        sitting_markup(false, Some(paw_top)),
        sitting_markup(true, Some(paw_top))
    )
}

fn search_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let mut s = blinking_cat(16);
        s.push_str("<g class=\"glass\">");
        // The glass: a pale lens, an amber rim, a handle down to the paw.
        let mut lens = String::new();
        ring(&mut lens, 25.0, 9.0, 0.0, 3.4, "#7f93b5");
        s.push_str(&format!("<g opacity=\"0.4\">{lens}</g>"));
        ring(&mut s, 25.0, 9.0, 3.4, 5.2, "#d4b26a");
        for (x, y) in [(23, 7), (24, 6), (23, 8)] {
            rect(&mut s, x, y, 1, 1, "#e8e8e8");
        }
        for (x, y) in [(22, 13), (21, 14), (20, 15), (21, 13), (20, 14)] {
            rect(&mut s, x, y, 1, 1, "#a8826a");
        }
        s.push_str("</g>");
        s
    })
}

fn waiting_markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let mut s = blinking_cat(17);
        // The rod: from the raised paw up and to the right.
        for x in 21..=40 {
            let y = 17 - (x - 21) * 14 / 19;
            rect(&mut s, x, y, 1, 1, "#a8826a");
        }
        // The line, the hook that bobs, and the empty bucket.
        rect(&mut s, 40, 4, 1, 18, "#cfcfcf");
        s.push_str("<g class=\"bob\">");
        rect(&mut s, 40, 22, 1, 2, "#cfcfcf");
        rect(&mut s, 39, 23, 1, 1, "#cfcfcf");
        rect(&mut s, 40, 24, 1, 1, "#f0a3b3");
        s.push_str("</g>");
        rect(&mut s, 34, 26, 10, 1, "#8a8a8a");
        rect(&mut s, 35, 27, 8, 4, "#2a2a2a");
        rect(&mut s, 35, 30, 8, 1, "#4d4d4d");
        s
    })
}

/// The cat looking through a magnifying glass, for a search with nothing to show.
#[component]
pub fn SearchCat() -> Element {
    rsx! {
        svg {
            class: "scene-cat", width: "160", height: "160", view_box: "0 0 32 32",
            role: "img", "aria-label": "A cat searching with a magnifying glass",
            dangerous_inner_html: "{search_markup()}",
        }
    }
}

/// The cat fishing in an empty bucket, while nothing is downloading.
#[component]
pub fn WaitingCat() -> Element {
    rsx! {
        svg {
            class: "scene-cat wide", width: "220", height: "160", view_box: "0 0 44 32",
            role: "img", "aria-label": "A cat fishing in an empty bucket",
            dangerous_inner_html: "{waiting_markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(markup: &str, width: i32) {
        for part in markup.split("<rect ").skip(1) {
            let num = |key: &str| -> i32 {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x >= 0 && x + w <= width && (0..32).contains(&y), "{part}");
        }
    }

    #[test]
    fn the_searching_cat_holds_a_moving_glass() {
        let m = search_markup();
        assert!(m.contains("class=\"glass\"") && m.contains("look-blink"));
        inside(m, 32);
    }

    #[test]
    fn the_waiting_cat_fishes_over_an_empty_bucket() {
        let m = waiting_markup();
        assert!(m.contains("class=\"bob\""));
        inside(m, 44);
    }
}
