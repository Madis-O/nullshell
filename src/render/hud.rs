//! HUD layer: the header, frame and status bar around the terminal.
//!
//! Purely decorative plus a few status readouts. It only pushes quads into
//! the scene, so it is safe to restyle or replace without touching anything
//! else. A custom title bar or tab strip would live next to this file.
//!
//! HUD text uses the HUD typeface (Rajdhani), which is proportional: text is
//! laid out by each character's advance, not on the terminal's cell grid.

use crate::frame::Frame;

use super::glyphs::{GlyphCache, UI_BOLD, UI_MEDIUM, UI_REGULAR, UI_SEMIBOLD};
use super::scene::{corner, Scene};
use super::{Layout, Theme};

pub struct HudState<'a> {
    pub title: &'a str,
    pub shell: &'a str,
    /// Wall clock, already formatted.
    pub clock: &'a str,
    /// Shown in the status bar in the alert color (config problems etc).
    pub warning: Option<&'a str>,
    pub focused: bool,
}

/// Extra space between HUD characters, as a fraction of the cell width.
/// Wide tracking is a big part of the game's UI look.
const TRACKING: f32 = 0.12;

/// Width of `s` in the HUD typeface.
fn measure(glyphs: &mut GlyphCache, s: &str, weight: u16) -> f32 {
    let track = (glyphs.metrics().cell_w * TRACKING).round();
    s.chars().map(|c| glyphs.ui(c, weight).1 + track).sum()
}

/// Draw HUD text with its top at `y` (same vertical box as a terminal row).
/// Returns the x position after the last character.
fn text(scene: &mut Scene, glyphs: &mut GlyphCache, x: f32, y: f32, s: &str, color: [f32; 4], weight: u16) -> f32 {
    let track = (glyphs.metrics().cell_w * TRACKING).round();
    let mut cx = x;
    for c in s.chars() {
        let (images, advance) = glyphs.ui(c, weight);
        for g in images {
            scene.glyph(cx.round() + g.left, y + g.top, g.w, g.h, g.uv, color, g.colored);
        }
        cx += advance + track;
    }
    cx
}

/// Like [`text`], but every digit gets the same slot width, so a ticking
/// clock does not jiggle as its digits change.
fn text_fixed(scene: &mut Scene, glyphs: &mut GlyphCache, x: f32, y: f32, s: &str, color: [f32; 4], weight: u16) -> f32 {
    let (slot, track) = fixed_slot(glyphs, weight);
    let mut cx = x;
    for c in s.chars() {
        let (images, advance) = glyphs.ui(c, weight);
        let width = if c.is_ascii_digit() { slot } else { (advance + track).ceil() };
        let inset = ((width - advance) / 2.0).round();
        for g in images {
            scene.glyph(cx + inset + g.left, y + g.top, g.w, g.h, g.uv, color, g.colored);
        }
        cx += width;
    }
    cx
}

/// Width [`text_fixed`] will use for `s`.
fn measure_fixed(glyphs: &mut GlyphCache, s: &str, weight: u16) -> f32 {
    let (slot, track) = fixed_slot(glyphs, weight);
    s.chars()
        .map(|c| if c.is_ascii_digit() { slot } else { (glyphs.ui(c, weight).1 + track).ceil() })
        .sum()
}

/// Digit slot width (the widest digit plus tracking) and the tracking itself.
fn fixed_slot(glyphs: &mut GlyphCache, weight: u16) -> (f32, f32) {
    let track = (glyphs.metrics().cell_w * TRACKING).round();
    let widest = ('0'..='9').map(|c| glyphs.ui(c, weight).1).fold(0.0, f32::max);
    ((widest + track).ceil(), track)
}

/// Shorten `s` until it fits in `max_width`. `keep_end` drops characters from
/// the start instead (for a path, the end is the part that matters).
fn fit(glyphs: &mut GlyphCache, s: &str, weight: u16, max_width: f32, keep_end: bool) -> String {
    if measure(glyphs, s, weight) <= max_width {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    for keep in (1..chars.len()).rev() {
        let candidate: String = if keep_end {
            "..".chars().chain(chars[chars.len() - keep..].iter().copied()).collect()
        } else {
            chars[..keep].iter().copied().chain("..".chars()).collect()
        };
        if measure(glyphs, &candidate, weight) <= max_width {
            return candidate;
        }
    }
    String::new()
}

pub fn draw(
    scene: &mut Scene,
    glyphs: &mut GlyphCache,
    layout: &Layout,
    theme: &Theme,
    frame: &Frame,
    hud: &HudState,
    time: f32,
) {
    let s = layout.scale;
    let px = |v: f32| (v * s).round();
    let m = glyphs.metrics();
    let (cw, ch) = (m.cell_w, m.cell_h);
    let (w, h) = (layout.width, layout.height);
    let pad = layout.pad;

    let accent = theme.accent;
    let alt = theme.accent_alt;
    let alert = theme.alert;
    let dark = theme.background.rgba(1.0);
    let dim = if hud.focused { 1.0 } else { 0.45 };

    // ---- header ----------------------------------------------------------
    let header_h = layout.header_h;
    scene.rect(0.0, 0.0, w, header_h, alt.rgba(0.045));

    // Name tag with the signature cut corner.
    let tag_y = px(4.0);
    let tag_h = header_h - px(8.0);
    let label = "NULLSHELL";
    let tag_w = (measure(glyphs, label, UI_BOLD) + cw * 2.0).round();
    scene.cut_rect(pad, tag_y, tag_w, tag_h, accent.rgba(dim), px(8.0), corner::BR, 0.0);
    let text_y = tag_y + ((tag_h - ch) / 2.0).round();
    text(scene, glyphs, pad + cw, text_y, label, dark, UI_BOLD);

    // Three shrinking bars trailing the tag.
    let mut bx = pad + tag_w + px(5.0);
    for bar in [4.0, 2.0, 1.0] {
        scene.rect(bx, tag_y, px(bar), tag_h, accent.rgba(0.85 * dim));
        bx += px(bar) + px(3.0);
    }

    // Clock on the right, with a pulsing status light.
    let clock_w = measure_fixed(glyphs, hud.clock, UI_SEMIBOLD);
    let clock_x = (w - pad - clock_w).round();
    text_fixed(scene, glyphs, clock_x, text_y, hud.clock, accent.rgba(0.9 * dim), UI_SEMIBOLD);
    let pulse = if hud.focused { 0.55 + 0.45 * (time * 2.4).sin() } else { 0.3 };
    let light = px(7.0);
    let light_x = clock_x - cw - light;
    scene.rect(light_x, text_y + ((ch - light) / 2.0).round(), light, light, alert.rgba(pulse));

    // Window title between the two.
    let title_x = bx + cw;
    let slashes = "//";
    let title_start = title_x + measure(glyphs, slashes, UI_BOLD) + cw;
    let room = light_x - cw - title_start;
    if room > cw * 4.0 {
        let title = fit(glyphs, &hud.title.to_uppercase(), UI_MEDIUM, room, true);
        text(scene, glyphs, title_x, text_y, slashes, alert.rgba(0.9 * dim), UI_BOLD);
        text(scene, glyphs, title_start, text_y, &title, alt.rgba(0.9 * dim), UI_MEDIUM);
    }

    // Rule under the header, heavier under the tag.
    scene.rect(pad, header_h - px(1.0), w - pad * 2.0, px(1.0), alt.rgba(0.30 * dim));
    scene.rect(pad, header_h - px(2.0), tag_w, px(2.0), accent.rgba(dim));

    // ---- frame around the terminal ---------------------------------------
    let [fx, fy, fw, fh] = layout.frame;
    let cut = px(12.0);
    let frame_color = if hud.focused { alert.rgba(0.55) } else { theme.foreground.rgba(0.18) };
    scene.cut_rect(fx, fy, fw, fh, frame_color, cut, corner::TR | corner::BL, px(1.0));

    // Bright brackets on the two uncut corners.
    let arm = px(18.0);
    let thick = px(2.0);
    scene.rect(fx, fy, arm, thick, accent.rgba(dim));
    scene.rect(fx, fy, thick, arm, accent.rgba(dim));
    scene.rect(fx + fw - arm, fy + fh - thick, arm, thick, accent.rgba(dim));
    scene.rect(fx + fw - thick, fy + fh - arm, thick, arm, accent.rgba(dim));

    // Tick marks riding the top edge, left of the cut corner.
    let mut tx = fx + fw - cut - px(10.0);
    for i in 0..6 {
        let tall = if i % 3 == 0 { 5.0 } else { 3.0 };
        scene.rect(tx, fy - px(tall) + px(1.0), px(1.0), px(tall), alt.rgba(0.6 * dim));
        tx -= px(5.0);
    }

    // Scrollback position as a thin gauge on the right edge.
    if frame.display_offset > 0 && frame.history > 0 {
        let track = fh - cut * 2.0;
        let visible = frame.rows as f32;
        let total = frame.history as f32 + visible;
        let thumb = (track * visible / total).max(px(12.0));
        // 0 = oldest line at the top of the view, 1 = back at the live screen.
        let pos = 1.0 - frame.display_offset.min(frame.history) as f32 / frame.history as f32;
        let thumb_y = fy + cut + (track - thumb).max(0.0) * pos;
        scene.rect(fx + fw - px(4.0), thumb_y.round(), px(2.0), thumb, accent.rgba(0.9));
    }

    // ---- status bar -------------------------------------------------------
    let footer_y = h - layout.footer_h;
    let bar_h = layout.footer_h - px(8.0);
    let bar_y = footer_y + px(3.0);
    let label_y = bar_y + ((bar_h - ch) / 2.0).round();

    // Shell name in an outlined tag.
    let shell = format!(">_ {}", hud.shell.to_uppercase());
    let shell_w = (measure(glyphs, &shell, UI_BOLD) + cw * 2.0).round();
    scene.cut_rect(pad, bar_y, shell_w, bar_h, alt.rgba(0.10 * dim), px(7.0), corner::TL, 0.0);
    scene.cut_rect(pad, bar_y, shell_w, bar_h, alt.rgba(0.85 * dim), px(7.0), corner::TL, px(1.0));
    text(scene, glyphs, pad + cw, label_y, &shell, alt.rgba(dim), UI_BOLD);

    let mut x = pad + shell_w + cw;
    let size = format!("{} x {}", frame.cols, frame.rows);
    x = text(scene, glyphs, x, label_y, &size, theme.foreground.rgba(0.6 * dim), UI_MEDIUM) + cw;

    if frame.display_offset > 0 {
        let hist = format!("HISTORY -{}", frame.display_offset);
        let hw = (measure(glyphs, &hist, UI_BOLD) + cw * 2.0).round();
        scene.cut_rect(x, bar_y, hw, bar_h, alert.rgba(1.0), px(7.0), corner::BR, 0.0);
        text(scene, glyphs, x + cw, label_y, &hist, [1.0, 1.0, 1.0, 1.0], UI_BOLD);
        x += hw + cw;
    }

    // Right side: a little barcode and a static readout.
    let mut right = w - pad;
    let code = [2.0, 1.0, 3.0, 1.0, 1.0, 2.0, 1.0, 3.0, 2.0, 1.0];
    for (i, bar) in code.iter().enumerate() {
        right -= px(*bar);
        if i % 2 == 0 {
            scene.rect(right, bar_y + px(2.0), px(*bar), bar_h - px(4.0), accent.rgba(0.8 * dim));
        }
        right -= px(1.0);
    }
    let readout = "UTF-8 // CONPTY";
    let readout_x = (right - cw - measure(glyphs, readout, UI_REGULAR)).round();

    if let Some(warning) = hud.warning {
        let room = readout_x - cw - x;
        if room > cw * 8.0 {
            let msg = fit(glyphs, &format!("! {warning}"), UI_SEMIBOLD, room, false);
            text(scene, glyphs, x, label_y, &msg, alert.rgba(1.0), UI_SEMIBOLD);
        }
    }
    if readout_x > x {
        text(scene, glyphs, readout_x, label_y, readout, theme.foreground.rgba(0.4 * dim), UI_REGULAR);
    }
}
