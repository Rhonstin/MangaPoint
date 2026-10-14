//! A small pixel-art cat walking along a sidewalk, seen from the side, with
//! buildings and street lamps drifting past behind it. The whole picture is one
//! SVG built once; the browser plays it from CSS keyframes (see `style.css`).

use dioxus::prelude::*;
use std::sync::OnceLock;

const W: usize = 22;
const H: usize = 13;
type Grid = [[u8; W]; H];

const CLEAR: u8 = b'.';
const BODY: u8 = b'g';
const SHADE: u8 = b's';
const LIGHT: u8 = b'l';
const PINK: u8 = b'p';
const EYE: u8 = b'e';

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

fn put(g: &mut Grid, x: usize, y: usize, c: u8) {
    if x < W && y < H {
        g[y][x] = c;
    }
}

fn fill(g: &mut Grid, x0: usize, y0: usize, x1: usize, y1: usize, c: u8) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            put(g, x, y, c);
        }
    }
}

/// One of the four steps of the walk. The cat faces right.
fn frame(phase: usize) -> Grid {
    let mut g = [[CLEAR; W]; H];
    // The body sits a pixel lower while the legs are spread.
    let dy = usize::from(phase.is_multiple_of(2));
    let tail_up = phase < 2;

    // Tail, swinging.
    let tail: [(usize, usize); 4] = if tail_up {
        [(2, 4), (1, 3), (1, 2), (0, 1)]
    } else {
        [(2, 5), (1, 5), (0, 5), (0, 4)]
    };
    for (x, y) in tail {
        put(&mut g, x, y + dy, SHADE);
    }

    // Legs: far ones darker and behind, near ones in front.
    // (x, lifted) for front-far, rear-far, front-near, rear-near.
    let legs: [[(usize, bool); 4]; 4] = [
        [(11, false), (6, true), (14, false), (3, false)],
        [(12, false), (5, false), (13, false), (4, false)],
        [(14, true), (3, false), (11, false), (6, false)],
        [(12, false), (5, false), (13, false), (4, false)],
    ];
    for (i, (x, lifted)) in legs[phase % 4].iter().enumerate() {
        let c = if i < 2 { SHADE } else { BODY };
        let end = if *lifted { 11 } else { 12 };
        fill(&mut g, *x, 9 + dy, x + 1, end, c);
        fill(&mut g, *x, end, x + 1, end, LIGHT);
    }

    // Body, with stripes along the back and a light belly.
    fill(&mut g, 3, 3 + dy, 14, 8 + dy, BODY);
    put(&mut g, 3, 3 + dy, CLEAR);
    put(&mut g, 3, 8 + dy, CLEAR);
    for x in [6, 9, 12] {
        put(&mut g, x, 3 + dy, SHADE);
    }
    fill(&mut g, 6, 8 + dy, 12, 8 + dy, LIGHT);

    // Head, ears, eye and muzzle.
    fill(&mut g, 13, 1 + dy, 18, 6 + dy, BODY);
    fill(&mut g, 13, dy, 14, dy, BODY);
    fill(&mut g, 17, dy, 18, dy, BODY);
    put(&mut g, 14, 1 + dy, PINK);
    put(&mut g, 17, 1 + dy, PINK);
    put(&mut g, 17, 3 + dy, EYE);
    fill(&mut g, 19, 4 + dy, 19, 5 + dy, LIGHT);
    put(&mut g, 19, 4 + dy, PINK);
    put(&mut g, 18, 5 + dy, LIGHT);
    g
}

/// A grid as SVG rectangles, one per run of the same colour in a row.
fn rects(g: &Grid) -> String {
    let mut out = String::new();
    for (y, row) in g.iter().enumerate() {
        let mut x = 0;
        while x < W {
            let c = row[x];
            if c == CLEAR {
                x += 1;
                continue;
            }
            let start = x;
            while x < W && row[x] == c {
                x += 1;
            }
            out.push_str(&format!(
                "<rect x=\"{start}\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"{}\"/>",
                x - start,
                color(c)
            ));
        }
    }
    out
}

fn rect(x: i32, y: i32, w: i32, h: i32, fill: &str) -> String {
    format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>")
}

/// `count` copies of `unit`, `period` apart.
fn repeat(unit: &str, period: i32, count: i32) -> String {
    (0..count)
        .map(|i| format!("<g transform=\"translate({} 0)\">{unit}</g>", i * period))
        .collect()
}

fn scene() -> &'static str {
    static SCENE: OnceLock<String> = OnceLock::new();
    SCENE.get_or_init(|| {
        // Far away: buildings, with a few lit windows.
        let mut buildings = String::new();
        for (x, w, h) in [(0, 14, 22), (16, 10, 30), (28, 16, 18), (46, 12, 26)] {
            buildings.push_str(&rect(x, 40 - h, w, h, "#101010"));
            let mut wy = 40 - h + 4;
            while wy < 36 {
                buildings.push_str(&rect(x + 3, wy, 2, 2, "#1d1d1d"));
                if w > 11 {
                    buildings.push_str(&rect(x + w - 5, wy, 2, 2, "#1d1d1d"));
                }
                wy += 6;
            }
        }
        // Nearer: street lamps.
        let lamp = [
            rect(20, 14, 1, 26, "#2e2e2e"),
            rect(20, 14, 7, 1, "#2e2e2e"),
            rect(25, 15, 4, 2, "#d8d8d8"),
            rect(24, 17, 6, 1, "#ffffff22"),
            rect(23, 18, 8, 1, "#ffffff14"),
        ]
        .concat();
        // The sidewalk, with a joint between its slabs every 16 pixels.
        let joint = rect(0, 41, 1, 7, "#222222");
        let cat: String = (0..4)
            .map(|p| format!("<g class=\"wf wf{p}\">{}</g>", rects(&frame(p))))
            .collect();
        format!(
            "<svg class=\"walker-svg\" viewBox=\"0 0 120 48\" xmlns=\"http://www.w3.org/2000/svg\" shape-rendering=\"crispEdges\">\
             <g class=\"w-far\">{}</g>\
             <g class=\"w-near\">{}</g>\
             {}{}\
             <g class=\"w-ground\">{}</g>\
             <g transform=\"translate(40 27)\">{cat}</g>\
             </svg>",
            repeat(&buildings, 60, 4),
            repeat(&lamp, 80, 3),
            rect(0, 40, 120, 8, "#151515"),
            rect(0, 40, 120, 1, "#303030"),
            repeat(&joint, 16, 16),
        )
    })
}

/// The cat walking along the street.
#[component]
pub fn WalkingCat() -> Element {
    rsx! { div { class: "walker", dangerous_inner_html: scene() } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_is_drawn_and_they_differ() {
        let frames: Vec<String> = (0..4).map(|p| rects(&frame(p))).collect();
        assert!(frames.iter().all(|f| f.contains("<rect")));
        assert_ne!(frames[0], frames[1]);
        assert_ne!(frames[0], frames[2]);
        assert!(scene().contains("w-ground"));
    }
}
