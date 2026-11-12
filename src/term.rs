//! Terminal emulation state. Wraps `alacritty_terminal` and turns it into a
//! renderer-independent [`Frame`].

use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::vte::ansi::{
    Color, CursorShape as AnsiCursorShape, CursorStyle as AnsiCursorStyle, NamedColor, Processor,
};

pub use alacritty_terminal::term::TermMode;

use crate::color::Rgb;
use crate::config::{ColorConfig, Config, CursorStyle};
use crate::frame::{CellFlags, Cursor, CursorShape, Frame};

/// Things the terminal asks its owner to do.
pub enum TermEvent {
    /// Reply bytes for the shell (cursor position reports and friends).
    PtyWrite(Vec<u8>),
    Title(String),
    ResetTitle,
    Bell,
    ClipboardStore(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Character by character.
    Simple,
    /// Word by word (double click).
    Word,
    /// Whole lines (triple click).
    Line,
    /// Rectangular block.
    Block,
}

#[derive(Clone)]
struct Listener(Sender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Number of color slots alacritty tracks (256 indexed + named extras).
const PALETTE_LEN: usize = 269;

struct Palette {
    table: [Rgb; PALETTE_LEN],
    selection_bg: Rgb,
    selection_fg: Rgb,
}

impl Palette {
    fn new(c: &ColorConfig) -> Self {
        let mut t = [Rgb::default(); PALETTE_LEN];
        let normal = c.normal.as_array();
        let bright = c.bright.as_array();
        t[..8].copy_from_slice(&normal);
        t[8..16].copy_from_slice(&bright);
        // 6x6x6 color cube.
        let level = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
        for i in 0..216 {
            t[16 + i] = Rgb::new(level(i / 36), level((i / 6) % 6), level(i % 6));
        }
        // Grayscale ramp.
        for i in 0..24 {
            let v = (8 + 10 * i) as u8;
            t[232 + i] = Rgb::new(v, v, v);
        }
        t[NamedColor::Foreground as usize] = c.foreground;
        t[NamedColor::Background as usize] = c.background;
        t[NamedColor::Cursor as usize] = c.cursor;
        for (i, color) in normal.iter().enumerate() {
            t[NamedColor::DimBlack as usize + i] = color.scale(0.66);
        }
        t[NamedColor::BrightForeground as usize] = c.foreground;
        t[NamedColor::DimForeground as usize] = c.foreground.scale(0.66);
        Self { table: t, selection_bg: c.selection_background, selection_fg: c.selection_foreground }
    }

    fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        let lookup = |i: usize| match overrides[i] {
            Some(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
            None => self.table[i.min(PALETTE_LEN - 1)],
        };
        match color {
            Color::Spec(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
            Color::Named(name) => lookup(name as usize),
            Color::Indexed(i) => lookup(i as usize),
        }
    }
}

pub struct Terminal {
    term: Term<Listener>,
    parser: Processor,
    events: Receiver<Event>,
    palette: Palette,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, cfg: &Config) -> Self {
        let (tx, events) = mpsc::channel();
        let shape = match cfg.cursor.shape {
            CursorStyle::Block => AnsiCursorShape::Block,
            CursorStyle::Beam => AnsiCursorShape::Beam,
            CursorStyle::Underline => AnsiCursorShape::Underline,
        };
        let term_cfg = TermConfig {
            scrolling_history: cfg.scrollback.lines,
            default_cursor_style: AnsiCursorStyle { shape, blinking: cfg.cursor.blink },
            ..TermConfig::default()
        };
        let size = Size { cols: cols.max(2), rows: rows.max(1) };
        Self {
            term: Term::new(term_cfg, &size, Listener(tx)),
            parser: Processor::new(),
            events,
            palette: Palette::new(&cfg.colors),
        }
    }

    /// Feed raw shell output through the escape sequence parser.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(Size { cols: cols.max(2), rows: rows.max(1) });
    }

    pub fn cols(&self) -> usize {
        self.term.columns()
    }

    pub fn rows(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Collect requests raised while parsing. `cell` is the cell size in pixels.
    pub fn drain_events(&mut self, cell: (u16, u16)) -> Vec<TermEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::PtyWrite(text) => out.push(TermEvent::PtyWrite(text.into_bytes())),
                Event::Title(title) => out.push(TermEvent::Title(title)),
                Event::ResetTitle => out.push(TermEvent::ResetTitle),
                Event::Bell => out.push(TermEvent::Bell),
                Event::ClipboardStore(_, text) => out.push(TermEvent::ClipboardStore(text)),
                Event::ColorRequest(index, format) => {
                    let rgb = self.term.colors()[index].unwrap_or_else(|| {
                        let c = self.palette.table[index.min(PALETTE_LEN - 1)];
                        alacritty_terminal::vte::ansi::Rgb { r: c.r, g: c.g, b: c.b }
                    });
                    out.push(TermEvent::PtyWrite(format(rgb).into_bytes()));
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = WindowSize {
                        num_lines: self.rows() as u16,
                        num_cols: self.cols() as u16,
                        cell_width: cell.0,
                        cell_height: cell.1,
                    };
                    out.push(TermEvent::PtyWrite(format(size).into_bytes()));
                }
                // Clipboard reads by programs are refused; the rest is not needed.
                _ => {}
            }
        }
        out
    }

    // ---- scrollback -------------------------------------------------------

    pub fn scroll_lines(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_page_up(&mut self) {
        self.term.scroll_display(Scroll::PageUp);
    }

    pub fn scroll_page_down(&mut self) {
        self.term.scroll_display(Scroll::PageDown);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    // ---- selection --------------------------------------------------------

    fn viewport_point(&self, col: usize, row: usize) -> Point {
        let col = col.min(self.cols().saturating_sub(1));
        let row = row.min(self.rows().saturating_sub(1));
        Point::new(Line(row as i32 - self.display_offset() as i32), Column(col))
    }

    pub fn selection_start(&mut self, col: usize, row: usize, right_half: bool, kind: SelectionKind) {
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        let side = if right_half { Side::Right } else { Side::Left };
        self.term.selection = Some(Selection::new(ty, self.viewport_point(col, row), side));
    }

    pub fn selection_update(&mut self, col: usize, row: usize, right_half: bool) {
        let point = self.viewport_point(col, row);
        let side = if right_half { Side::Right } else { Side::Left };
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn selection_clear(&mut self) {
        self.term.selection = None;
    }

    /// Selected text, or `None` when nothing (or only nothing-ness) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    // ---- snapshot ---------------------------------------------------------

    /// Fill `frame` with the visible screen, colors fully resolved.
    pub fn snapshot(&self, frame: &mut Frame) {
        let cols = self.cols();
        let rows = self.rows();
        let content = self.term.renderable_content();
        let colors = content.colors;
        let default_fg = self.palette.resolve(Color::Named(NamedColor::Foreground), colors);
        frame.reset(cols, rows, default_fg);
        frame.display_offset = content.display_offset;
        frame.history = self.term.grid().history_size();

        let offset = content.display_offset as i32;
        let selection = content.selection;

        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            let col = indexed.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            let cell = indexed.cell;
            let src = cell.flags;

            let mut fg = self.palette.resolve(cell.fg, colors);
            let mut bg = self.palette.resolve(cell.bg, colors);
            let mut default_bg = matches!(cell.bg, Color::Named(NamedColor::Background));
            if src.contains(Flags::DIM) {
                fg = fg.scale(0.66);
            }
            if src.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
                default_bg = false;
            }
            if selection.is_some_and(|s| s.contains(indexed.point)) {
                fg = self.palette.selection_fg;
                bg = self.palette.selection_bg;
                default_bg = false;
            }

            let mut flags = CellFlags::empty();
            let map = [
                (Flags::BOLD, CellFlags::BOLD),
                (Flags::ITALIC, CellFlags::ITALIC),
                (Flags::UNDERLINE, CellFlags::UNDERLINE),
                (Flags::DOUBLE_UNDERLINE, CellFlags::DOUBLE_UNDERLINE),
                (Flags::UNDERCURL, CellFlags::UNDERCURL),
                (Flags::DOTTED_UNDERLINE, CellFlags::DOTTED_UNDERLINE),
                (Flags::DASHED_UNDERLINE, CellFlags::DASHED_UNDERLINE),
                (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
                (Flags::WIDE_CHAR, CellFlags::WIDE),
                (Flags::WIDE_CHAR_SPACER, CellFlags::WIDE_SPACER),
            ];
            for (from, to) in map {
                if src.contains(from) {
                    flags |= to;
                }
            }

            let hidden = src.contains(Flags::HIDDEN);
            let out = &mut frame.cells[row as usize * cols + col];
            out.c = if hidden || cell.c == '\0' { ' ' } else { cell.c };
            out.zerowidth = if hidden { None } else { cell.zerowidth().map(Into::into) };
            out.fg = fg;
            out.bg = if default_bg { None } else { Some(bg) };
            out.flags = flags;
        }

        let cursor = content.cursor;
        let shape = match cursor.shape {
            AnsiCursorShape::Block => Some(CursorShape::Block),
            AnsiCursorShape::Underline => Some(CursorShape::Underline),
            AnsiCursorShape::Beam => Some(CursorShape::Beam),
            AnsiCursorShape::HollowBlock => Some(CursorShape::Hollow),
            AnsiCursorShape::Hidden => None,
        };
        let row = cursor.point.line.0 + offset;
        let col = cursor.point.column.0;
        if let Some(shape) = shape {
            if row >= 0 && (row as usize) < rows && col < cols {
                let wide = frame.cells[row as usize * cols + col].flags.contains(CellFlags::WIDE);
                frame.cursor = Some(Cursor { col, row: row as usize, shape, wide });
            }
        }
    }
}
