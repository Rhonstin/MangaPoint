//! The cat floating belly up on a calm sea at dusk, for the empty Search page.
//! An original scene drawn in code on a 128x64 grid as SVG rectangles. The
//! motion is CSS (see `style.css`): the cat bobs, its tail and paws sway, the
//! waves drift, stars twinkle and little "z"s float up.

use dioxus::prelude::*;
use std::sync::OnceLock;

const W: i32 = 128;
const H: i32 = 64;

fn rect(out: &mut String, x: i32, y: i32, w: i32, h: i32, fill: &str) {
    // Waves reach past the edges so that they can drift without a gap.
    let (x0, x1) = (x.max(-20), (x + w).min(W + 20));
    if x1 > x0 && (0..H).contains(&y) && h > 0 {
        out.push_str(&format!(
            "<rect x=\"{x0}\" y=\"{y}\" width=\"{}\" height=\"{h}\" fill=\"{fill}\"/>",
            x1 - x0
        ));
    }
}

/// A filled ellipse, one rectangle per row.
fn ellipse(out: &mut String, cx: f32, cy: f32, rx: f32, ry: f32, fill: &str) {
    let top = (cy - ry).floor() as i32;
    let bottom = (cy + ry).ceil() as i32;
    for y in top..=bottom {
        let dy = (y as f32 + 0.5 - cy) / ry;
        if dy.abs() > 1.0 {
            continue;
        }
        let half = rx * (1.0 - dy * dy).sqrt();
        let x0 = (cx - half).round() as i32;
        let x1 = (cx + half).round() as i32;
        rect(out, x0, y, x1 - x0, 1, fill);
    }
}

fn pixels(out: &mut String, points: &[(i32, i32)], fill: &str) {
    for &(x, y) in points {
        rect(out, x, y, 1, 1, fill);
    }
}

/// Dashes of one wave row, repeating every 16 pixels from off the left edge
/// to off the right edge, so the row can drift by 16 and loop without a seam.
fn wave_row(out: &mut String, y: i32, shift: i32, fill: &str) {
    for k in -1..=8 {
        let base = k * 16 + shift;
        rect(out, base, y, 5, 1, fill);
        rect(out, base + 9, y, 3, 1, fill);
    }
}

const BODY: &str = "#a9a9a9";
const SHADE: &str = "#7d7d7d";
const LIGHT: &str = "#e8e8e8";
const BELLY: &str = "#f1ece2";
const PINK: &str = "#f0a3b3";
const DARK: &str = "#0a0a0a";

fn sky_and_sea() -> String {
    let mut s = String::new();
    for (y0, y1, c) in [
        (0, 5, "#0b0d1c"),
        (6, 10, "#10152b"),
        (11, 15, "#171d3a"),
        (16, 20, "#212849"),
        (21, 24, "#2e3158"),
        (25, 27, "#443c66"),
        (28, 31, "#27425f"),
        (32, 37, "#203854"),
        (38, 45, "#192f48"),
        (46, 53, "#132639"),
        (54, 63, "#0e1c2f"),
    ] {
        rect(&mut s, 0, y0, W, y1 - y0 + 1, c);
    }
    // The moon, with a soft glow.
    s.push_str("<g opacity=\"0.07\">");
    ellipse(&mut s, 104.0, 11.0, 14.0, 14.0, "#f3ecd0");
    s.push_str("</g><g opacity=\"0.12\">");
    ellipse(&mut s, 104.0, 11.0, 9.5, 9.5, "#f3ecd0");
    s.push_str("</g>");
    ellipse(&mut s, 104.0, 11.0, 5.5, 5.5, "#f3ecd0");
    pixels(
        &mut s,
        &[(102, 9), (103, 9), (102, 10), (106, 13), (105, 14)],
        "#d9d1b2",
    );
    // Stars that twinkle in three groups.
    for (i, group) in [
        &[(8, 4), (30, 9), (58, 3), (88, 7), (118, 20)][..],
        &[(18, 12), (44, 6), (70, 13), (96, 3), (122, 8)][..],
        &[(5, 19), (36, 17), (64, 8), (80, 19), (112, 26)][..],
    ]
    .iter()
    .enumerate()
    {
        s.push_str(&format!("<g class=\"star s{i}\">"));
        pixels(&mut s, group, "#dfe6ff");
        s.push_str("</g>");
    }
    // A cloud that drifts.
    s.push_str("<g class=\"cloud\">");
    rect(&mut s, 14, 17, 14, 2, "#2c3358");
    rect(&mut s, 17, 15, 8, 2, "#2c3358");
    rect(&mut s, 20, 14, 4, 1, "#2c3358");
    s.push_str("</g>");
    // The moon's path on the water.
    s.push_str("<g class=\"glint\">");
    for (i, y) in (29..46).step_by(2).enumerate() {
        let w = 3 + (i as i32 % 3) * 2;
        rect(&mut s, 104 - w / 2, y, w, 1, "#cdd5e6");
    }
    s.push_str("</g>");
    // Far waves.
    s.push_str("<g class=\"waves far\">");
    wave_row(&mut s, 30, 0, "#3a5f86");
    wave_row(&mut s, 34, 6, "#35597f");
    wave_row(&mut s, 40, 3, "#2f5176");
    s.push_str("</g>");
    s
}

fn cat() -> String {
    let mut s = String::new();

    // The tail trails off to the right and curls up; two poses.
    for (class, spots, tip) in [
        ("tail-a", [(83, 41), (85, 40), (86, 38), (87, 36)], (87, 34)),
        ("tail-b", [(83, 41), (85, 40), (87, 39), (88, 37)], (89, 35)),
    ] {
        s.push_str(&format!("<g class=\"{class}\">"));
        for (x, y) in spots {
            rect(&mut s, x, y, 2, 2, SHADE);
        }
        rect(&mut s, tip.0, tip.1, 2, 2, "#4a4a4a");
        s.push_str("</g>");
    }

    // Body: gray rim, a cream belly facing the sky.
    ellipse(&mut s, 66.0, 41.5, 18.0, 5.4, BODY);
    ellipse(&mut s, 66.0, 40.6, 15.0, 3.7, BELLY);
    ellipse(&mut s, 66.0, 42.4, 12.0, 1.4, "#ddd5c6");
    pixels(&mut s, &[(60, 39), (66, 38), (71, 40), (63, 41)], "#d6cfc1");
    pixels(&mut s, &[(56, 45), (62, 46), (70, 46), (76, 45)], SHADE);

    // Front paws, up in the air (they twitch now and then).
    s.push_str("<g class=\"paws\">");
    rect(&mut s, 53, 35, 2, 4, BODY);
    rect(&mut s, 52, 32, 4, 3, LIGHT);
    pixels(&mut s, &[(53, 33), (54, 33)], PINK);
    rect(&mut s, 57, 34, 2, 5, BODY);
    rect(&mut s, 56, 31, 4, 3, LIGHT);
    pixels(&mut s, &[(57, 32), (58, 32)], PINK);
    s.push_str("</g>");

    // Hind legs.
    ellipse(&mut s, 77.0, 38.5, 4.2, 3.2, BODY);
    rect(&mut s, 76, 33, 2, 5, BODY);
    rect(&mut s, 75, 30, 4, 3, LIGHT);
    pixels(&mut s, &[(76, 31), (77, 31)], PINK);
    rect(&mut s, 80, 35, 2, 3, BODY);
    rect(&mut s, 79, 32, 4, 3, LIGHT);
    pixels(&mut s, &[(80, 33), (81, 33)], PINK);

    // The head, seen from above and upside down: ears toward the water.
    ellipse(&mut s, 44.0, 41.5, 7.2, 6.3, BODY);
    ellipse(&mut s, 44.0, 39.5, 3.6, 2.4, LIGHT);
    rect(&mut s, 43, 37, 2, 1, PINK);
    pixels(&mut s, &[(44, 38)], "#c98a98");
    // Happy closed eyes.
    pixels(&mut s, &[(39, 42), (40, 43), (41, 43), (42, 42)], DARK);
    pixels(&mut s, &[(46, 42), (47, 43), (48, 43), (49, 42)], DARK);
    // A few forehead stripes.
    pixels(&mut s, &[(43, 47), (44, 47), (45, 47), (44, 46)], SHADE);
    // Ears, mostly under the water.
    for (y, x0, x1) in [(46, 38, 41), (47, 38, 40), (48, 38, 39), (49, 38, 38)] {
        rect(&mut s, x0, y, x1 - x0 + 1, 1, BODY);
        rect(&mut s, 88 - x1, y, x1 - x0 + 1, 1, BODY);
    }
    pixels(&mut s, &[(39, 47), (49, 47)], PINK);

    // The water over the lower half of the cat.
    s.push_str("<g opacity=\"0.78\">");
    rect(&mut s, 0, 44, W, 2, "#192f48");
    rect(&mut s, 0, 46, W, 5, "#132639");
    s.push_str("</g>");
    s.push_str("<g class=\"foam\">");
    for (x, w) in [
        (34, 4),
        (40, 6),
        (49, 3),
        (55, 5),
        (63, 7),
        (73, 4),
        (79, 6),
        (88, 3),
        (93, 5),
    ] {
        rect(&mut s, x, 44, w, 1, "#a6c8e4");
    }
    s.push_str("</g>");
    s
}

fn extras() -> String {
    let mut s = String::new();
    // Ripples spreading from the cat.
    for i in 0..2 {
        s.push_str(&format!("<g class=\"ripple r{i}\">"));
        rect(&mut s, 46, 50, 40, 1, "#7fa6c8");
        rect(&mut s, 40, 51, 6, 1, "#7fa6c8");
        rect(&mut s, 86, 51, 6, 1, "#7fa6c8");
        s.push_str("</g>");
    }
    // A manga bobbing nearby.
    s.push_str("<g class=\"book\">");
    rect(&mut s, 104, 47, 10, 5, "#6f86ad");
    rect(&mut s, 105, 48, 8, 3, "#e8e8e8");
    rect(&mut s, 104, 51, 10, 1, "#4a5d82");
    pixels(&mut s, &[(108, 49), (109, 49), (110, 49)], "#9a9a9a");
    s.push_str("</g>");
    // Sleepy "z"s drifting up from the cat.
    for (i, (x, y, big)) in [(34, 33, false), (29, 26, true)].into_iter().enumerate() {
        s.push_str(&format!("<g class=\"zz z{i}\">"));
        let rows: &[&str] = if big {
            &["#####", "...#.", "..#..", ".#...", "#####"]
        } else {
            &["####", "..#.", ".#..", "####"]
        };
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if c == '#' {
                    rect(&mut s, x + dx as i32, y + dy as i32, 1, 1, "#e8e8e8");
                }
            }
        }
        s.push_str("</g>");
    }
    s
}

fn near_waves() -> String {
    let mut s = String::new();
    s.push_str("<g class=\"waves mid\">");
    wave_row(&mut s, 53, 2, "#2a4d73");
    wave_row(&mut s, 57, 9, "#27476c");
    s.push_str("</g><g class=\"waves near\">");
    wave_row(&mut s, 60, 5, "#335a82");
    wave_row(&mut s, 62, 12, "#2f5379");
    s.push_str("</g>");
    s
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        format!(
            "{}<g class=\"floater\">{}</g>{}{}",
            sky_and_sea(),
            cat(),
            extras(),
            near_waves()
        )
    })
}

/// The cat floating on its back, filling most of the empty Search page.
#[component]
pub fn OceanCat() -> Element {
    rsx! {
        svg {
            class: "ocean",
            view_box: "0 0 {W} {H}",
            role: "img",
            "aria-label": "A cat floating on its back in a calm sea at dusk",
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
            let (x, y, w, h) = (num("x=\""), num("y=\""), num("width=\""), num("height=\""));
            assert!(
                x >= -20 && x + w <= W + 20 && y >= 0 && y + h <= H,
                "{part}"
            );
        }
    }

    #[test]
    fn the_scene_has_its_moving_parts() {
        let m = markup();
        for class in [
            "floater",
            "tail-a",
            "tail-b",
            "paws",
            "foam",
            "ripple r1",
            "star s2",
            "waves far",
            "waves near",
            "zz z1",
            "book",
            "glint",
            "cloud",
        ] {
            assert!(m.contains(&format!("class=\"{class}\"")), "{class}");
        }
    }
}
