//! Built-in box drawing, block element and braille characters.
//!
//! Fonts rarely line these up perfectly from cell to cell, which shows as
//! gaps in TUI borders and graphs. Drawing them as pixel-snapped rectangles
//! makes frames in btop, nvim and friends seamless at any font size.

use super::scene::Scene;

/// Arms of U+2500..U+257F as `left right up down`, one digit each:
/// 0 none, 1 light, 2 heavy, 3 double. Arcs and diagonals are `0000`.
const LINES: &str = "\
1100 2200 0011 0022 1100 2200 0011 0022 1100 2200 0011 0022 0101 0201 0102 0202 \
1001 2001 1002 2002 0110 0210 0120 0220 1010 2010 1020 2020 0111 0211 0121 0112 \
0122 0221 0212 0222 1011 2011 1021 1012 1022 2021 2012 2022 1101 2101 1201 2201 \
1102 2102 1202 2202 1110 2110 1210 2210 1120 2120 1220 2220 1111 2111 1211 2211 \
1121 1112 1122 2121 1221 2112 1212 2221 2212 2122 1222 2222 1100 2200 0011 0022 \
3300 0033 0301 0103 0303 3001 1003 3003 0310 0130 0330 3010 1030 3030 0311 0133 \
0333 3011 1033 3033 3301 1103 3303 3310 1130 3330 3311 1133 3333 0000 0000 0000 \
0000 0000 0000 0000 1000 0010 0100 0001 2000 0020 0200 0002 1200 0012 2100 0021 ";

/// Draw `c` into the cell at `(x, y)` if it is a built-in character.
/// Returns false when the font should draw it instead.
pub fn draw(scene: &mut Scene, c: char, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> bool {
    let cp = c as u32;
    match cp {
        0x256D..=0x2570 => {
            arc(scene, cp, x, y, w, h, color);
            true
        }
        0x2571..=0x2573 => false,
        0x2500..=0x257F => {
            let i = (cp - 0x2500) as usize * 5;
            let arms = &LINES.as_bytes()[i..i + 4];
            let d = |b: u8| b - b'0';
            lines(scene, d(arms[0]), d(arms[1]), d(arms[2]), d(arms[3]), x, y, w, h, color);
            true
        }
        0x2580..=0x259F => {
            blocks(scene, cp, x, y, w, h, color);
            true
        }
        0x2800..=0x28FF => {
            braille(scene, cp - 0x2800, x, y, w, h, color);
            true
        }
        _ => false,
    }
}

/// Thickness of a light line for a cell of width `w`.
fn light(w: f32) -> f32 {
    (w / 8.0).round().max(1.0)
}

#[allow(clippy::too_many_arguments)]
fn lines(scene: &mut Scene, l: u8, r: u8, u: u8, d: u8, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let t = light(w);
    let heavy = if t < 2.0 { 3.0 } else { t * 2.0 };
    let th = |weight: u8| match weight {
        1 => t,
        2 => heavy,
        3 => 3.0 * t,
        _ => 0.0,
    };
    let (x1, y1) = (x + w, y + h);
    // Top left of the light stroke through the cell center.
    let cx0 = x + ((w - t) / 2.0).floor();
    let cy0 = y + ((h - t) / 2.0).floor();
    // Double lines are two rails, one stroke away on each side of the center:
    // columns A = [xa0, cx0) and B = [xb0, xb1), rows C = [yc0, cy0) and D = [yd0, yd1).
    let (xa0, xb0, xb1) = (cx0 - t, cx0 + t, cx0 + 2.0 * t);
    let (yc0, yd0, yd1) = (cy0 - t, cy0 + t, cy0 + 2.0 * t);
    let double_v = u == 3 || d == 3;
    let double_h = l == 3 || r == 3;
    // Span of the crossing stroke, so single arms reach through corners.
    let vt = th(u).max(th(d));
    let ht = th(l).max(th(r));
    let vx0 = x + ((w - vt) / 2.0).floor();
    let hy0 = y + ((h - ht) / 2.0).floor();

    let mut rect = |ax: f32, ay: f32, bx: f32, by: f32| {
        scene.rect(ax, ay, bx - ax, by - ay, color);
    };

    // Single (light or heavy) arms.
    if l == 1 || l == 2 {
        let s = th(l);
        let ya = y + ((h - s) / 2.0).floor();
        let end = if double_v {
            if u == 3 && d == 3 { cx0 } else { xb1 }
        } else if vt > 0.0 {
            vx0 + vt
        } else {
            cx0
        };
        rect(x, ya, end, ya + s);
    }
    if r == 1 || r == 2 {
        let s = th(r);
        let ya = y + ((h - s) / 2.0).floor();
        let start = if double_v {
            if u == 3 && d == 3 {
                if l != 0 { cx0 } else { xb0 }
            } else {
                xa0
            }
        } else if vt > 0.0 {
            vx0
        } else {
            cx0
        };
        rect(start, ya, x1, ya + s);
    }
    if u == 1 || u == 2 {
        let s = th(u);
        let xa = x + ((w - s) / 2.0).floor();
        let end = if double_h {
            if l == 3 && r == 3 { cy0 } else { yd1 }
        } else if ht > 0.0 {
            hy0 + ht
        } else {
            cy0
        };
        rect(xa, y, xa + s, end);
    }
    if d == 1 || d == 2 {
        let s = th(d);
        let xa = x + ((w - s) / 2.0).floor();
        let start = if double_h {
            if l == 3 && r == 3 {
                if u != 0 { cy0 } else { yd0 }
            } else {
                yc0
            }
        } else if ht > 0.0 {
            hy0
        } else {
            cy0
        };
        rect(xa, start, xa + s, y1);
    }

    // Double arms: each rail stops where it meets the rail it turns into.
    if r == 3 {
        let top = if u == 3 { xb0 } else if d == 3 || l == 3 { xa0 } else { cx0 };
        let bottom = if d == 3 { xb0 } else if u == 3 || l == 3 { xa0 } else { cx0 };
        rect(top, yc0, x1, cy0);
        rect(bottom, yd0, x1, yd1);
    }
    if l == 3 {
        let top = if u == 3 { cx0 } else if d == 3 || r == 3 { xb1 } else { cx0 + t };
        let bottom = if d == 3 { cx0 } else if u == 3 || r == 3 { xb1 } else { cx0 + t };
        rect(x, yc0, top, cy0);
        rect(x, yd0, bottom, yd1);
    }
    if u == 3 {
        let left = if l == 3 { cy0 } else if r == 3 || d == 3 { yd1 } else { cy0 + t };
        let right = if r == 3 { cy0 } else if l == 3 || d == 3 { yd1 } else { cy0 + t };
        rect(xa0, y, cx0, left);
        rect(xb0, y, xb1, right);
    }
    if d == 3 {
        let left = if l == 3 { yd0 } else if r == 3 || u == 3 { yc0 } else { cy0 };
        let right = if r == 3 { yd0 } else if l == 3 || u == 3 { yc0 } else { cy0 };
        rect(xa0, left, cx0, y1);
        rect(xb0, right, xb1, y1);
    }
}

/// Rounded corners U+256D..U+2570.
fn arc(scene: &mut Scene, cp: u32, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let t = light(w);
    let cx0 = x + ((w - t) / 2.0).floor();
    let cy0 = y + ((h - t) / 2.0).floor();
    // Center line of the strokes.
    let (cxm, cym) = (cx0 + t / 2.0, cy0 + t / 2.0);
    // Direction of the horizontal and vertical arm.
    let (sx, sy) = match cp {
        0x256D => (1.0, 1.0),
        0x256E => (-1.0, 1.0),
        0x256F => (-1.0, -1.0),
        _ => (1.0, -1.0),
    };
    let radius = (w.min(h) / 2.0).floor();
    let center = [cxm + sx * radius, cym + sy * radius];

    let pad = t / 2.0 + 1.0;
    let (ax, bx) = (cxm - sx * pad, center[0]);
    let (ay, by) = (cym - sy * pad, center[1]);
    scene.arc(ax.min(bx), ay.min(by), (ax - bx).abs(), (ay - by).abs(), center, radius, t, color);

    // Straight runs from the ends of the arc to the cell edges.
    if sy > 0.0 {
        scene.rect(cx0, center[1], t, y + h - center[1], color);
    } else {
        scene.rect(cx0, y, t, center[1] - y, color);
    }
    if sx > 0.0 {
        scene.rect(center[0], cy0, x + w - center[0], t, color);
    } else {
        scene.rect(x, cy0, center[0] - x, t, color);
    }
}

fn blocks(scene: &mut Scene, cp: u32, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    let eighths = |n: u32, total: f32| (total * n as f32 / 8.0).round();
    let shade = |a: f32| [color[0], color[1], color[2], color[3] * a];
    match cp {
        0x2580 => scene.rect(x, y, w, eighths(4, h), color),
        0x2581..=0x2588 => {
            let hh = eighths(cp - 0x2580, h);
            scene.rect(x, y + h - hh, w, hh, color);
        }
        0x2589..=0x258F => scene.rect(x, y, eighths(0x2590 - cp, w).max(1.0), h, color),
        0x2590 => {
            let ww = eighths(4, w);
            scene.rect(x + ww, y, w - ww, h, color);
        }
        0x2591 => scene.rect(x, y, w, h, shade(0.25)),
        0x2592 => scene.rect(x, y, w, h, shade(0.50)),
        0x2593 => scene.rect(x, y, w, h, shade(0.75)),
        0x2594 => scene.rect(x, y, w, eighths(1, h).max(1.0), color),
        0x2595 => {
            let ww = eighths(1, w).max(1.0);
            scene.rect(x + w - ww, y, ww, h, color);
        }
        _ => {
            // Quadrants. Bits: 1 upper left, 2 upper right, 4 lower left, 8 lower right.
            let mask: u8 = match cp {
                0x2596 => 4,
                0x2597 => 8,
                0x2598 => 1,
                0x2599 => 1 | 4 | 8,
                0x259A => 1 | 8,
                0x259B => 1 | 2 | 4,
                0x259C => 1 | 2 | 8,
                0x259D => 2,
                0x259E => 2 | 4,
                _ => 2 | 4 | 8,
            };
            let (hw, hh) = (eighths(4, w), eighths(4, h));
            if mask & 1 != 0 {
                scene.rect(x, y, hw, hh, color);
            }
            if mask & 2 != 0 {
                scene.rect(x + hw, y, w - hw, hh, color);
            }
            if mask & 4 != 0 {
                scene.rect(x, y + hh, hw, h - hh, color);
            }
            if mask & 8 != 0 {
                scene.rect(x + hw, y + hh, w - hw, h - hh, color);
            }
        }
    }
}

fn braille(scene: &mut Scene, bits: u32, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    // Bit -> (column, row) of the 2x4 dot matrix.
    const DOTS: [(u32, f32, f32); 8] = [
        (0x01, 0.0, 0.0),
        (0x02, 0.0, 1.0),
        (0x04, 0.0, 2.0),
        (0x40, 0.0, 3.0),
        (0x08, 1.0, 0.0),
        (0x10, 1.0, 1.0),
        (0x20, 1.0, 2.0),
        (0x80, 1.0, 3.0),
    ];
    let dot = (w / 4.0).round().max(1.0);
    for (bit, col, row) in DOTS {
        if bits & bit == 0 {
            continue;
        }
        let cx = x + w * (0.25 + 0.5 * col);
        let cy = y + h * (0.125 + 0.25 * row);
        scene.rect((cx - dot / 2.0).round(), (cy - dot / 2.0).round(), dot, dot, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_the_whole_block() {
        assert_eq!(LINES.len(), 128 * 5);
        assert!(LINES.bytes().all(|b| b == b' ' || (b'0'..=b'3').contains(&b)));
    }

    #[test]
    fn every_builtin_draws_something() {
        for cp in (0x2500..=0x259Fu32).chain(0x2801..=0x28FF) {
            if (0x2571..=0x2573).contains(&cp) {
                continue;
            }
            let mut scene = Scene::default();
            let c = char::from_u32(cp).unwrap();
            assert!(draw(&mut scene, c, 0.0, 0.0, 9.0, 20.0, [1.0; 4]), "U+{cp:04X} not handled");
            assert!(scene.len() > 0, "U+{cp:04X} drew nothing");
        }
    }
}
