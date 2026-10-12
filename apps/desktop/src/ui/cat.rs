//! The logo: a small gray pixel-art cat that licks its paw.
//!
//! An original 32x32 sprite drawn in code. Five frames are built once as SVG
//! rectangles; the browser plays the animation from CSS keyframes (see
//! `style.css`), so it costs almost nothing while it runs.

use dioxus::prelude::*;
use std::sync::OnceLock;

const SIZE: usize = 32;
type Grid = [[u8; SIZE]; SIZE];

const CLEAR: u8 = b'.';
const BODY: u8 = b'g';
const SHADE: u8 = b's';
const LIGHT: u8 = b'l';
const PINK: u8 = b'p';
const EYE: u8 = b'e';

/// Palette: gray accent body, darker shade, light chest, pink nose/tongue.
fn color(c: u8) -> &'static str {
    match c {
        BODY => "#b4b4b4",
        SHADE => "#707070",
        LIGHT => "#e8e8e8",
        PINK => "#f0a3b3",
        EYE => "#050505",
        _ => "none",
    }
}

fn px(g: &mut Grid, x: usize, y: usize, c: u8) {
    g[y][x] = c;
}

/// Fill an inclusive rectangle.
fn rect(g: &mut Grid, x0: usize, y0: usize, x1: usize, y1: usize, c: u8) {
    for row in g.iter_mut().take(y1 + 1).skip(y0) {
        for cell in row.iter_mut().take(x1 + 1).skip(x0) {
            *cell = c;
        }
    }
}

/// What differs between frames.
#[derive(Clone, Copy)]
struct Pose {
    eyes_closed: bool,
    /// Top row of the raised paw, or `None` when both paws rest.
    paw_top: Option<usize>,
    /// 0 = no tongue, 1 = tip, 2 = out.
    tongue: u8,
}

const POSES: [Pose; 5] = [
    // 0: sitting, eyes open
    Pose {
        eyes_closed: false,
        paw_top: None,
        tongue: 0,
    },
    // 1: lifting the paw
    Pose {
        eyes_closed: false,
        paw_top: Some(19),
        tongue: 0,
    },
    // 2: licking, tongue tip
    Pose {
        eyes_closed: true,
        paw_top: Some(16),
        tongue: 1,
    },
    // 3: licking, tongue out, paw bobs down a pixel
    Pose {
        eyes_closed: true,
        paw_top: Some(17),
        tongue: 2,
    },
    // 4: blink
    Pose {
        eyes_closed: true,
        paw_top: None,
        tongue: 0,
    },
];

/// The tail, drawn separately so it can sway on its own.
fn tail() -> Grid {
    let mut g = [[CLEAR; SIZE]; SIZE];
    rect(&mut g, 23, 27, 26, 28, SHADE);
    rect(&mut g, 26, 24, 27, 26, SHADE);
    rect(&mut g, 25, 22, 26, 23, SHADE);
    g
}

fn frame(pose: Pose) -> Grid {
    let mut g = [[CLEAR; SIZE]; SIZE];

    // Ears (left, then mirrored), with pink insides.
    for (y, from, to) in [(4, 8, 8), (5, 8, 9), (6, 8, 10), (7, 8, 11), (8, 8, 12)] {
        rect(&mut g, from, y, to, y, BODY);
        rect(&mut g, 31 - to, y, 31 - from, y, BODY);
    }
    for (x, y) in [(9, 6), (9, 7), (10, 7)] {
        px(&mut g, x, y, PINK);
        px(&mut g, 31 - x, y, PINK);
    }

    // Head, rounded at the corners.
    rect(&mut g, 9, 9, 22, 9, BODY);
    rect(&mut g, 8, 10, 23, 17, BODY);
    rect(&mut g, 9, 18, 22, 18, BODY);
    // Forehead stripes.
    px(&mut g, 12, 10, SHADE);
    px(&mut g, 19, 10, SHADE);
    rect(&mut g, 15, 10, 16, 11, SHADE);

    // Body.
    rect(&mut g, 10, 19, 21, 19, BODY);
    rect(&mut g, 9, 20, 22, 27, BODY);
    rect(&mut g, 10, 28, 21, 28, BODY);
    rect(&mut g, 11, 29, 20, 29, BODY);
    rect(&mut g, 13, 20, 18, 27, LIGHT);
    for (x, y) in [(10, 22), (10, 25), (21, 22), (21, 25)] {
        px(&mut g, x, y, SHADE);
    }

    // Resting paws.
    rect(&mut g, 11, 27, 14, 30, LIGHT);
    rect(&mut g, 17, 27, 20, 30, LIGHT);
    px(&mut g, 15, 29, SHADE);
    px(&mut g, 16, 29, SHADE);

    // Muzzle and nose.
    rect(&mut g, 13, 14, 18, 17, LIGHT);
    rect(&mut g, 15, 14, 16, 14, PINK);

    // Eyes.
    if pose.eyes_closed {
        rect(&mut g, 11, 13, 12, 13, EYE);
        rect(&mut g, 19, 13, 20, 13, EYE);
        px(&mut g, 10, 14, EYE);
        px(&mut g, 21, 14, EYE);
    } else {
        rect(&mut g, 11, 12, 12, 14, EYE);
        rect(&mut g, 19, 12, 20, 14, EYE);
        px(&mut g, 12, 12, LIGHT);
        px(&mut g, 20, 12, LIGHT);
    }

    // Raised paw: the right paw leaves the ground and comes up to the mouth.
    if let Some(top) = pose.paw_top {
        rect(&mut g, 17, 27, 20, 30, CLEAR);
        rect(&mut g, 17, 27, 20, 29, LIGHT);
        // Arm from the body up to the paw.
        rect(&mut g, 19, top + 3, 21, 26, BODY);
        // Paw with a pink pad.
        rect(&mut g, 18, top, 21, top + 3, LIGHT);
        rect(&mut g, 19, top + 2, 20, top + 2, PINK);
    }

    // Tongue, reaching toward the paw.
    match pose.tongue {
        1 => px(&mut g, 17, 17, PINK),
        2 => {
            px(&mut g, 17, 17, PINK);
            px(&mut g, 17, 18, PINK);
        }
        _ => {}
    }
    g
}

/// One `<rect>` per horizontal run of equal color.
fn rects(g: &Grid) -> String {
    let mut out = String::new();
    for (y, row) in g.iter().enumerate() {
        let mut x = 0;
        while x < SIZE {
            let c = row[x];
            let mut end = x;
            while end + 1 < SIZE && row[end + 1] == c {
                end += 1;
            }
            if c != CLEAR {
                out.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{}\"/>",
                    end - x + 1,
                    color(c)
                ));
            }
            x = end + 1;
        }
    }
    out
}

fn markup() -> &'static str {
    static MARKUP: OnceLock<String> = OnceLock::new();
    MARKUP.get_or_init(|| {
        let mut s = format!("<g class=\"tail\">{}</g>", rects(&tail()));
        for (i, pose) in POSES.iter().enumerate() {
            s.push_str(&format!("<g class=\"f f{i}\">{}</g>", rects(&frame(*pose))));
        }
        s
    })
}

#[component]
pub fn Cat() -> Element {
    rsx! {
        svg {
            class: "cat",
            width: "32",
            height: "32",
            view_box: "0 0 32 32",
            "aria-hidden": "true",
            dangerous_inner_html: "{markup()}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pose_is_drawn_and_stays_on_the_canvas() {
        let m = markup();
        for i in 0..POSES.len() {
            assert!(m.contains(&format!("class=\"f f{i}\"")), "frame {i}");
        }
        assert!(m.contains("class=\"tail\""));
        // Every rectangle lies inside the 32x32 canvas.
        for part in m.split("<rect ").skip(1) {
            let num = |key: &str| -> usize {
                let rest = &part[part.find(key).unwrap() + key.len()..];
                rest[..rest.find('"').unwrap()].parse().unwrap()
            };
            let (x, y, w) = (num("x=\""), num("y=\""), num("width=\""));
            assert!(x + w <= SIZE && y < SIZE, "{part}");
        }
    }

    #[test]
    fn the_frames_actually_differ() {
        let frames: Vec<Grid> = POSES.iter().map(|p| frame(*p)).collect();
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "frames {i} and {j} are identical");
            }
        }
    }

    #[test]
    fn licking_poses_show_the_tongue_and_closed_eyes() {
        let tongue_pixels = |g: &Grid| g.iter().flatten().filter(|&&c| c == PINK).count();
        let rest = tongue_pixels(&frame(POSES[0]));
        assert!(tongue_pixels(&frame(POSES[2])) > rest);
        assert!(tongue_pixels(&frame(POSES[3])) > tongue_pixels(&frame(POSES[2])));
        // Eyes closed: the open-eye highlight pixel is gone.
        assert_eq!(frame(POSES[2])[12][12], BODY);
        assert_eq!(frame(POSES[0])[12][12], LIGHT);
    }
}
