//! Terminal grid layer: turns a [`Frame`] into quads.

use crate::color::Rgb;
use crate::frame::{CellFlags, CursorShape, Frame};

use super::boxdraw;
use super::glyphs::{GlyphCache, STYLE_BOLD, STYLE_ITALIC};
use super::scene::Scene;

pub struct GridParams {
    /// Top left of the grid in pixels.
    pub x: f32,
    pub y: f32,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    /// False during the "off" half of a cursor blink.
    pub cursor_on: bool,
    pub focused: bool,
}

pub fn draw(scene: &mut Scene, glyphs: &mut GlyphCache, frame: &Frame, p: &GridParams) {
    let m = glyphs.metrics();
    let (cw, ch) = (m.cell_w, m.cell_h);
    if frame.cols == 0 || frame.rows == 0 || frame.cells.len() < frame.cols * frame.rows {
        return;
    }
    let line = (m.font_px / 14.0).round().max(1.0);

    // Pass 1: cell backgrounds, merged into runs of equal color.
    for row in 0..frame.rows {
        let y = p.y + row as f32 * ch;
        let cells = &frame.cells[row * frame.cols..(row + 1) * frame.cols];
        let mut col = 0;
        while col < frame.cols {
            let Some(bg) = cells[col].bg else {
                col += 1;
                continue;
            };
            let start = col;
            while col < frame.cols && cells[col].bg == Some(bg) {
                col += 1;
            }
            scene.rect(p.x + start as f32 * cw, y, (col - start) as f32 * cw, ch, bg.rgba(1.0));
        }
    }

    // A focused block cursor sits under its character, which is recolored.
    let block_cursor = frame
        .cursor
        .filter(|c| p.focused && p.cursor_on && c.shape == CursorShape::Block)
        .map(|c| (c.col, c.row));

    // Pass 2: characters and text decorations.
    for row in 0..frame.rows {
        let y = p.y + row as f32 * ch;
        for col in 0..frame.cols {
            let cell = &frame.cells[row * frame.cols + col];
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                continue;
            }
            let x = p.x + col as f32 * cw;
            let width = if cell.flags.contains(CellFlags::WIDE) { cw * 2.0 } else { cw };

            let mut fg = cell.fg;
            if block_cursor == Some((col, row)) {
                scene.rect(x, y, width, ch, p.cursor.rgba(1.0));
                fg = p.cursor_text;
            }
            let color = fg.rgba(1.0);

            if cell.c != ' ' && !boxdraw::draw(scene, cell.c, x, y, cw, ch, color) {
                let mut style = 0;
                if cell.flags.contains(CellFlags::BOLD) {
                    style |= STYLE_BOLD;
                }
                if cell.flags.contains(CellFlags::ITALIC) {
                    style |= STYLE_ITALIC;
                }
                for g in glyphs.get(cell.c, cell.zerowidth.as_deref(), style) {
                    scene.glyph(x + g.left, y + g.top, g.w, g.h, g.uv, color, g.colored);
                }
            }

            let under_y = (y + m.baseline + line + 1.0).min(y + ch - line);
            if cell.flags.contains(CellFlags::UNDERLINE) {
                scene.rect(x, under_y, width, line, color);
            }
            if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
                scene.rect(x, under_y - line, width, line, color);
                scene.rect(x, (under_y + line).min(y + ch - line), width, line, color);
            }
            if cell.flags.contains(CellFlags::UNDERCURL) {
                // A small square wave reads as a squiggle at this size.
                let step = (line * 2.0).max(2.0);
                let mut dx = 0.0;
                let mut up = false;
                while dx < width {
                    let dy = if up { -line } else { 0.0 };
                    scene.rect(x + dx, under_y + dy, step.min(width - dx), line, color);
                    up = !up;
                    dx += step;
                }
            }
            if cell.flags.contains(CellFlags::DOTTED_UNDERLINE) {
                let mut dx = 0.0;
                while dx < width {
                    scene.rect(x + dx, under_y, line, line, color);
                    dx += line * 2.0;
                }
            }
            if cell.flags.contains(CellFlags::DASHED_UNDERLINE) {
                let dash = (width / 4.0).floor().max(1.0);
                scene.rect(x, under_y, dash * 1.5, line, color);
                scene.rect(x + dash * 2.5, under_y, dash * 1.5, line, color);
            }
            if cell.flags.contains(CellFlags::STRIKEOUT) {
                scene.rect(x, (y + m.baseline - m.font_px * 0.3).round(), width, line, color);
            }
        }
    }

    // Pass 3: the other cursor shapes, drawn on top.
    let Some(cursor) = frame.cursor else { return };
    let x = p.x + cursor.col as f32 * cw;
    let y = p.y + cursor.row as f32 * ch;
    let width = if cursor.wide { cw * 2.0 } else { cw };
    let color = p.cursor.rgba(1.0);
    let t = (line * 2.0).max(2.0);
    if !p.focused || cursor.shape == CursorShape::Hollow {
        // Outline only, so an unfocused window is obvious at a glance.
        scene.rect(x, y, width, line, color);
        scene.rect(x, y + ch - line, width, line, color);
        scene.rect(x, y, line, ch, color);
        scene.rect(x + width - line, y, line, ch, color);
    } else if p.cursor_on {
        match cursor.shape {
            CursorShape::Beam => scene.rect(x, y, t, ch, color),
            CursorShape::Underline => scene.rect(x, y + ch - t, width, t, color),
            CursorShape::Block | CursorShape::Hollow => {}
        }
    }
}
