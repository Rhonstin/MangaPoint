//! The cat asleep upside down, hanging by its hind paws from a tree branch,
//! for the empty Downloads page. Drawn in code on a 48x40 grid, big pixels
//! like the other cats; the motion is CSS (see `style.css`).

use super::ocean::{group_plain, Layer};
use dioxus::prelude::*;
use std::sync::OnceLock;

pub const W: i32 = 48;
pub const H: i32 = 40;

const BODY: &str = "#a9a9a9";
const SHADE: &str = "#7d7d7d";
const LIGHT: &str = "#e8e8e8";
const BELLY: &str = "#f1ece2";
const PINK: &str = "#f0a3b3";
const DARK: &str = "#0a0a0a";
const BARK: &str = "#6b4f3a";
const BARK_LIGHT: &str = "#86644a";
const BARK_DARK: &str = "#47321f";
const LEAF: &str = "#6f9472";
const LEAF_DARK: &str = "#4f6f52";

fn tree() -> String {
    let mut out = String::new();
    let mut t = Layer::default();
    // The trunk on the left, and the branch growing out of it.
    t.rect(0, 0, 5, 14, BARK);
    t.rect(0, 0, 1, 14, BARK_DARK);
    t.rect(3, 0, 1, 14, BARK_LIGHT);
    t.rect(4, 3, 39, 1, BARK_LIGHT);
    t.rect(4, 4, 40, 2, BARK);
    t.rect(4, 6, 38, 1, BARK_DARK);
    t.pixels(&[(44, 4), (44, 5)], BARK_DARK);
    // A knot, and a couple of twigs.
    t.pixels(&[(12, 4), (13, 4), (12, 5)], BARK_DARK);
    t.pixels(&[(36, 2), (37, 1), (38, 1)], BARK);
    t.pixels(&[(40, 7), (41, 8)], BARK_DARK);
    out.push_str(&group_plain("", 1.0, &t));

    // Leaves that stir in two poses.
    for (class, dy) in [("leaves leaves-a", 0), ("leaves leaves-b", 1)] {
        let mut l = Layer::default();
        for (x, y) in [(38, 0), (39, 0), (40, 1), (38, 1), (39, 2)] {
            l.px(x, y + dy, LEAF);
        }
        l.pixels(&[(40, dy), (41, 2 + dy)], LEAF_DARK);
        for (x, y) in [(41, 8), (42, 8), (42, 9), (43, 9), (41, 9)] {
            l.px(x, y - dy, LEAF);
        }
        l.pixels(&[(43, 8 - dy), (42, 10 - dy)], LEAF_DARK);
        for (x, y) in [(7, 7), (8, 7), (8, 8), (9, 8)] {
            l.px(x, y + dy, LEAF);
        }
        l.px(7, 8 + dy, LEAF_DARK);
        out.push_str(&group_plain(class, 1.0, &l));
    }
    out
}

fn cat() -> String {
    let mut out = String::new();

    // The tail hangs down beside the body and swings; two poses.
    for (class, dx) in [("tail-a", 0), ("tail-b", 1)] {
        let mut tail = Layer::default();
        for (x, y) in [
            (29, 12),
            (30, 13),
            (30, 14),
            (31, 15),
            (31, 16),
            (31, 17),
            (31, 18),
        ] {
            tail.px(x + dx, y, SHADE);
        }
        tail.rect(31 + dx, 19, 1, 2, "#4a4a4a");
        out.push_str(&group_plain(class, 1.0, &tail));
    }

    let mut s = Layer::default();
    // Hind legs, from the branch down to the body.
    s.rect(22, 7, 2, 4, BODY);
    s.rect(26, 7, 2, 4, BODY);
    // Body, with its cream belly toward us.
    s.ellipse(24.5, 17.0, 5.2, 7.2, BODY);
    s.ellipse(24.5, 17.5, 3.4, 5.2, BELLY);
    s.pixels(&[(23, 15), (26, 19), (24, 21)], "#d9d2c5");
    // Front paws folded on the belly.
    s.rect(22, 18, 2, 2, LIGHT);
    s.rect(26, 18, 2, 2, LIGHT);
    // The head hangs below, upside down: nose above the closed eyes.
    s.ellipse(24.5, 28.5, 5.2, 4.4, BODY);
    s.ellipse(24.5, 26.6, 2.6, 1.6, LIGHT);
    s.rect(24, 25, 2, 1, PINK);
    s.pixels(&[(21, 29), (22, 30), (27, 30), (28, 29)], DARK);
    s.pixels(&[(24, 32), (25, 32)], SHADE);
    // Ears point down.
    for (y, x0, x1) in [(32, 19, 22), (33, 19, 21), (34, 19, 20), (35, 19, 19)] {
        s.rect(x0, y, x1 - x0 + 1, 1, BODY);
        s.rect(49 - x1, y, x1 - x0 + 1, 1, BODY);
    }
    s.pixels(&[(20, 33), (29, 33)], PINK);
    out.push_str(&group_plain("", 1.0, &s));

    // The hind paws hooked over the branch, in front of it.
    let mut paws = Layer::default();
    paws.rect(21, 4, 4, 3, LIGHT);
    paws.rect(25, 4, 4, 3, LIGHT);
    paws.pixels(&[(23, 5), (27, 5)], BODY);
    paws.pixels(&[(22, 6), (26, 6)], PINK);
    out.push_str(&group_plain("", 1.0, &paws));
    out
}

fn zzz() -> String {
    let mut out = String::new();
    for (i, (x, y, big)) in [(33, 27, false), (37, 21, true)].into_iter().enumerate() {
        let rows: &[&str] = if big {
            &["###", "..#", ".#.", "###"]
        } else {
            &["##", ".#", "##"]
        };
        let mut z = Layer::default();
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c == '#' {
                    z.px(x + dx as i32, y + dy as i32, LIGHT);
                }
            }
        }
        out.push_str(&group_plain(&format!("zz z{i}"), 1.0, &z));
    }
    out
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| format!("{}<g class=\"swing\">{}</g>{}", tree(), cat(), zzz()))
}

/// The cat sleeping upside down on a branch, while nothing is downloading.
#[component]
pub fn HangingCat() -> Element {
    rsx! {
        svg {
            class: "hang",
            view_box: "0 0 {W} {H}",
            role: "img",
            "aria-label": "A cat sleeping upside down, hanging from a tree branch",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_drawn_inside_the_canvas() {
        for part in markup().split("<rect ").skip(1) {
            let num = |key: &str| -> i32 {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x >= 0 && x + w <= W && (0..H).contains(&y), "{part}");
        }
    }

    #[test]
    fn it_sleeps_upside_down_with_its_parts_that_move() {
        let m = markup();
        for class in ["swing", "tail-a", "tail-b", "leaves-a", "leaves-b", "zz z1"] {
            assert!(m.contains(class), "{class}");
        }
        assert!(m.contains(BELLY) && m.contains(DARK) && m.contains(BARK));
    }
}
