//! Plain-data snapshot of what the terminal wants on screen.
//!
//! This is the only thing the terminal side hands to the renderer, so the
//! renderer never needs to know which terminal emulation library is in use.

use bitflags::bitflags;

use crate::color::Rgb;

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CellFlags: u16 {
        const BOLD             = 1 << 0;
        const ITALIC           = 1 << 1;
        const UNDERLINE        = 1 << 2;
        const DOUBLE_UNDERLINE = 1 << 3;
        const UNDERCURL        = 1 << 4;
        const DOTTED_UNDERLINE = 1 << 5;
        const DASHED_UNDERLINE = 1 << 6;
        const STRIKEOUT        = 1 << 7;
        /// First half of a double-width character.
        const WIDE             = 1 << 8;
        /// Second half of a double-width character; nothing is drawn here.
        const WIDE_SPACER      = 1 << 9;
    }
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub c: char,
    /// Combining characters attached to `c`, if any.
    pub zerowidth: Option<Box<[char]>>,
    pub fg: Rgb,
    /// `None` means the default background, which is left to the renderer
    /// (so window opacity shows through).
    pub bg: Option<Rgb>,
    pub flags: CellFlags,
}

impl Cell {
    pub fn blank(fg: Rgb) -> Self {
        Self { c: ' ', zerowidth: None, fg, bg: None, flags: CellFlags::empty() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
    Hollow,
}

#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
    pub shape: CursorShape,
    pub wide: bool,
}

#[derive(Default)]
pub struct Frame {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `cols * rows` cells.
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    /// How many lines the view is scrolled back into history.
    pub display_offset: usize,
    /// Lines of scrollback currently stored.
    pub history: usize,
}

impl Frame {
    pub fn reset(&mut self, cols: usize, rows: usize, fg: Rgb) {
        self.cols = cols;
        self.rows = rows;
        self.cells.clear();
        self.cells.resize(cols * rows, Cell::blank(fg));
        self.cursor = None;
    }
}
