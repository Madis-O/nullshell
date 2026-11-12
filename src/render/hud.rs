//! HUD layer: the header, frame and status bar around the terminal.
//!
//! Purely decorative plus a few status readouts. It only pushes quads into
//! the scene, so it is safe to restyle or replace without touching anything
//! else. A custom title bar or tab strip would live next to this file.

use crate::frame::Frame;

use super::glyphs::{GlyphCache, STYLE_BOLD};
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

/// Draw monospace text on the cell grid starting at `(x, y)`. Returns the x
/// position after the last character.
fn text(scene: &mut Scene, glyphs: &mut GlyphCache, x: f32, y: f32, s: &str, color: [f32; 4], style: u8) -> f32 {
    let cw = glyphs.metrics().cell_w;
    let mut cx = x;
    for c in s.chars() {
        if c != ' ' {
            for g in glyphs.get(c, None, style) {
                scene.glyph(cx + g.left, y + g.top, g.w, g.h, g.uv, color, g.colored);
            }
        }
        cx += cw;
    }
    cx
}

/// Shorten `s` to at most `max` characters, marking the cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 2 {
        return String::new();
    }
    let mut out: String = s.chars().take(max - 2).collect();
    out.push_str("..");
    out
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
    let tag_w = cw * (label.len() as f32 + 2.0);
    scene.cut_rect(pad, tag_y, tag_w, tag_h, accent.rgba(dim), px(8.0), corner::BR, 0.0);
    let text_y = tag_y + ((tag_h - ch) / 2.0).round();
    text(scene, glyphs, pad + cw, text_y, label, dark, STYLE_BOLD);

    // Three shrinking bars trailing the tag.
    let mut bx = pad + tag_w + px(5.0);
    for bar in [4.0, 2.0, 1.0] {
        scene.rect(bx, tag_y, px(bar), tag_h, accent.rgba(0.85 * dim));
        bx += px(bar) + px(3.0);
    }

    // Clock on the right, with a pulsing status light.
    let clock_w = cw * hud.clock.chars().count() as f32;
    let clock_x = w - pad - clock_w;
    text(scene, glyphs, clock_x, text_y, hud.clock, accent.rgba(0.9 * dim), 0);
    let pulse = if hud.focused { 0.55 + 0.45 * (time * 2.4).sin() } else { 0.3 };
    let light = px(7.0);
    let light_x = clock_x - cw - light;
    scene.rect(light_x, text_y + ((ch - light) / 2.0).round(), light, light, alert.rgba(pulse));

    // Window title between the two.
    let title_x = bx + cw;
    let room = ((light_x - cw - title_x) / cw).floor().max(0.0) as usize;
    if room > 6 {
        let title = truncate(&hud.title.to_uppercase(), room - 3);
        let x = text(scene, glyphs, title_x, text_y, "//", alert.rgba(0.9 * dim), STYLE_BOLD);
        text(scene, glyphs, x + cw, text_y, &title, alt.rgba(0.85 * dim), 0);
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
    let shell_w = cw * (shell.chars().count() as f32 + 2.0);
    scene.cut_rect(pad, bar_y, shell_w, bar_h, alt.rgba(0.10 * dim), px(7.0), corner::TL, 0.0);
    scene.cut_rect(pad, bar_y, shell_w, bar_h, alt.rgba(0.85 * dim), px(7.0), corner::TL, px(1.0));
    text(scene, glyphs, pad + cw, label_y, &shell, alt.rgba(dim), STYLE_BOLD);

    let mut x = pad + shell_w + cw;
    let size = format!("{}x{}", frame.cols, frame.rows);
    x = text(scene, glyphs, x, label_y, &size, theme.foreground.rgba(0.55 * dim), 0) + cw;

    if frame.display_offset > 0 {
        let hist = format!("HISTORY -{}", frame.display_offset);
        let hw = cw * (hist.len() as f32 + 2.0);
        scene.cut_rect(x, bar_y, hw, bar_h, alert.rgba(1.0), px(7.0), corner::BR, 0.0);
        text(scene, glyphs, x + cw, label_y, &hist, [1.0, 1.0, 1.0, 1.0], STYLE_BOLD);
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
    let readout_x = right - cw - cw * readout.len() as f32;

    if let Some(warning) = hud.warning {
        let room = ((readout_x - cw - x) / cw).floor().max(0.0) as usize;
        if room > 8 {
            let msg = truncate(&format!("! {warning}"), room);
            text(scene, glyphs, x, label_y, &msg, alert.rgba(1.0), STYLE_BOLD);
        }
    }
    if readout_x > x {
        text(scene, glyphs, readout_x, label_y, readout, theme.foreground.rgba(0.35 * dim), 0);
    }
}
