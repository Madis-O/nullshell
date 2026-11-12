//! The window and event loop. Glue between the pty, the terminal model and
//! the renderer; none of those three know about each other.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::windows::{Color, CornerPreference, IconExtWindows, WindowAttributesExtWindows};
use winit::window::{Fullscreen, Icon, Theme, Window, WindowId};

use crate::color::Rgb;
use crate::config::Config;
use crate::frame::Frame;
use crate::input::{self, MouseButtonCode};
use crate::platform;
use crate::pty::{Pty, PtyEvent};
use crate::render::{FrameFx, HudState, Renderer};
use crate::term::{SelectionKind, TermEvent, TermMode, Terminal};

const DEFAULT_TITLE: &str = "NullShell";
/// Redraw interval while animated effects are running.
const ANIMATION_FRAME: Duration = Duration::from_millis(33);
/// Redraw interval otherwise: cursor blink and the clock.
const IDLE_FRAME: Duration = Duration::from_millis(500);
const BLINK_MS: u128 = 530;
const STARTUP_GLITCH: Duration = Duration::from_millis(450);
const BELL_GLITCH: Duration = Duration::from_millis(220);
/// Longest stretch of output parsing before a frame is drawn.
const PARSE_BUDGET: Duration = Duration::from_millis(12);

pub enum UserEvent {
    Pty(PtyEvent),
}

pub fn run(cfg: Config, warning: Option<String>) -> Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().context("event loop")?;
    let mut app = App { cfg, warning, proxy: event_loop.create_proxy(), state: None, error: None };
    event_loop.run_app(&mut app).context("event loop")?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

struct App {
    cfg: Config,
    warning: Option<String>,
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
    error: Option<anyhow::Error>,
}

#[derive(Default)]
struct Mouse {
    x: f64,
    y: f64,
    /// Left button is held and extending a selection.
    selecting: bool,
    /// Button currently held while a program is receiving mouse reports.
    reporting: Option<MouseButtonCode>,
    last_cell: (usize, usize),
    last_click: Option<Instant>,
    click_cell: (usize, usize),
    clicks: u8,
    wheel: f32,
}

struct Glitch {
    start: Instant,
    length: Duration,
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    term: Terminal,
    pty: Pty,
    frame: Frame,
    clipboard: Option<arboard::Clipboard>,
    mods: ModifiersState,
    mouse: Mouse,
    title: String,
    warning: Option<String>,
    animate: bool,
    blink: bool,
    focused: bool,
    dirty: bool,
    shown: bool,
    start: Instant,
    last_draw: Instant,
    blink_epoch: Instant,
    glitch: Option<Glitch>,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match State::new(event_loop, &self.cfg, self.warning.take(), self.proxy.clone()) {
            Ok(mut state) => {
                // Hidden windows get no paint messages, so draw the first
                // frame by hand; that also makes the window visible.
                state.redraw();
                self.state = Some(state);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(state) = self.state.as_mut() else { return };
        match event {
            UserEvent::Pty(PtyEvent::Output) => {
                if state.pump_output() {
                    // Budget ran out with output still queued: come back after this frame.
                    let _ = self.proxy.send_event(UserEvent::Pty(PtyEvent::Output));
                }
            }
            // The shell is gone, so is the window.
            UserEvent::Pty(PtyEvent::Exit) => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                state.pty.kill();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => state.redraw(),
            WindowEvent::Resized(size) => {
                state.renderer.resize(size.width, size.height);
                state.sync_grid();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.renderer.set_scale(scale_factor as f32);
                state.sync_grid();
            }
            WindowEvent::Focused(focused) => state.on_focus(focused),
            WindowEvent::ModifiersChanged(mods) => state.mods = mods.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    state.on_key(&event);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => state.send_input(text.into_bytes()),
            WindowEvent::CursorMoved { position, .. } => state.on_mouse_move(position.x, position.y),
            WindowEvent::MouseInput { state: button_state, button, .. } => {
                state.on_mouse_button(button, button_state == ElementState::Pressed)
            }
            WindowEvent::MouseWheel { delta, .. } => state.on_wheel(delta),
            _ => {}
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let (StartCause::ResumeTimeReached { .. }, Some(state)) = (cause, self.state.as_mut()) {
            state.dirty = true;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = self.state.as_mut() else { return };
        let interval = state.frame_interval();
        if state.last_draw.elapsed() >= interval {
            state.dirty = true;
        }
        if state.dirty {
            state.dirty = false;
            state.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + interval));
    }
}

impl State {
    fn new(
        event_loop: &ActiveEventLoop,
        cfg: &Config,
        config_warning: Option<String>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self> {
        let translucent = cfg.window.opacity < 1.0;
        let win_color = |c: Rgb| Color::from_rgb(c.r, c.g, c.b);
        let mut attrs = Window::default_attributes()
            .with_title(DEFAULT_TITLE)
            .with_inner_size(LogicalSize::new(cfg.window.width, cfg.window.height))
            .with_min_inner_size(LogicalSize::new(360, 220))
            .with_transparent(translucent)
            .with_theme(Some(Theme::Dark))
            .with_window_icon(app_icon())
            .with_taskbar_icon(app_icon())
            // Shown after the first frame so there is no white flash.
            .with_visible(false)
            // Native title bar, tinted to match (Windows 11).
            .with_title_background_color(Some(win_color(cfg.colors.background)))
            .with_title_text_color(win_color(cfg.colors.accent))
            .with_border_color(Some(win_color(cfg.colors.accent)));
        if cfg.window.square_corners {
            attrs = attrs.with_corner_preference(CornerPreference::DoNotRound);
        }
        let window = Arc::new(event_loop.create_window(attrs).context("failed to create window")?);
        window.set_ime_allowed(true);

        let renderer = Renderer::new(window.clone(), cfg)?;
        if translucent && !renderer.is_transparent() {
            platform::set_window_opacity(&window, cfg.window.opacity);
        }

        let (cols, rows) = (renderer.layout().cols, renderer.layout().rows);
        let term = Terminal::new(cols, rows, cfg);
        let pty = Pty::spawn(&cfg.shell, cols as u16, rows as u16, move |event| {
            let _ = proxy.send_event(UserEvent::Pty(event));
        })?;

        let warning = config_warning.or_else(|| renderer.warning().map(str::to_string));
        let now = Instant::now();
        Ok(Self {
            window,
            renderer,
            term,
            pty,
            frame: Frame::default(),
            clipboard: arboard::Clipboard::new().ok(),
            mods: ModifiersState::empty(),
            mouse: Mouse::default(),
            title: DEFAULT_TITLE.to_string(),
            warning,
            animate: cfg.effects.enabled && cfg.effects.animate,
            blink: cfg.cursor.blink,
            focused: true,
            dirty: true,
            shown: false,
            start: now,
            last_draw: now,
            blink_epoch: now,
            glitch: Some(Glitch { start: now, length: STARTUP_GLITCH }),
        })
    }

    fn frame_interval(&self) -> Duration {
        if self.glitch.is_some() || (self.animate && self.focused) {
            ANIMATION_FRAME
        } else {
            IDLE_FRAME
        }
    }

    // ---- output ------------------------------------------------------------

    /// Parse queued shell output. Returns true if more is still waiting.
    fn pump_output(&mut self) -> bool {
        self.pty.begin_drain();
        let started = Instant::now();
        let mut more = false;
        while let Some(chunk) = self.pty.try_read() {
            self.term.feed(&chunk);
            if started.elapsed() > PARSE_BUDGET {
                more = true;
                break;
            }
        }
        self.handle_term_events();
        self.dirty = true;
        more
    }

    fn handle_term_events(&mut self) {
        let layout = self.renderer.layout();
        let cell = (layout.cell_w as u16, layout.cell_h as u16);
        for event in self.term.drain_events(cell) {
            match event {
                TermEvent::PtyWrite(bytes) => self.pty.write(bytes),
                TermEvent::Title(title) => {
                    self.window.set_title(&title);
                    self.title = title;
                }
                TermEvent::ResetTitle => {
                    self.window.set_title(DEFAULT_TITLE);
                    self.title = DEFAULT_TITLE.to_string();
                }
                TermEvent::Bell => {
                    self.glitch = Some(Glitch { start: Instant::now(), length: BELL_GLITCH });
                }
                TermEvent::ClipboardStore(text) => {
                    if let Some(clipboard) = self.clipboard.as_mut() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
        }
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        self.term.snapshot(&mut self.frame);

        let glitch = match &self.glitch {
            Some(g) if now.duration_since(g.start) < g.length => {
                1.0 - now.duration_since(g.start).as_secs_f32() / g.length.as_secs_f32()
            }
            _ => {
                self.glitch = None;
                0.0
            }
        };
        let cursor_on = !self.blink || (now.duration_since(self.blink_epoch).as_millis() / BLINK_MS) % 2 == 0;
        let (h, m, s) = platform::local_time();
        let clock = format!("{h:02}:{m:02}:{s:02}");

        let hud = HudState {
            title: &self.title,
            shell: &self.pty.shell_name,
            clock: &clock,
            warning: self.warning.as_deref(),
            focused: self.focused,
        };
        let fx = FrameFx {
            time: now.duration_since(self.start).as_secs_f32(),
            glitch,
            cursor_on,
            focused: self.focused,
        };
        self.renderer.render(&self.frame, &hud, &fx);
        self.last_draw = now;

        if !self.shown {
            self.shown = true;
            self.window.set_visible(true);
            self.window.focus_window();
        }
    }

    /// Make the terminal and the pty match the grid the renderer can fit.
    fn sync_grid(&mut self) {
        let (cols, rows) = (self.renderer.layout().cols, self.renderer.layout().rows);
        if cols != self.term.cols() || rows != self.term.rows() {
            self.term.resize(cols, rows);
            self.pty.resize(cols as u16, rows as u16);
        }
        self.dirty = true;
    }

    // ---- keyboard ----------------------------------------------------------

    fn send_input(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.term.selection_clear();
        self.term.scroll_to_bottom();
        self.pty.write(bytes);
        self.blink_epoch = Instant::now();
        self.dirty = true;
    }

    fn on_key(&mut self, event: &KeyEvent) {
        let (ctrl, shift, alt) = (self.mods.control_key(), self.mods.shift_key(), self.mods.alt_key());
        let ch = match &event.logical_key {
            Key::Character(s) => s.chars().next().map(|c| c.to_ascii_lowercase()),
            _ => None,
        };
        let named = match &event.logical_key {
            Key::Named(n) => Some(*n),
            _ => None,
        };

        // Window-level shortcuts first.
        if ctrl && !alt {
            match ch {
                Some('c') if shift => return self.copy(false),
                Some('v') if shift => return self.paste(),
                // Plain Ctrl+C copies when there is a selection, like Windows Terminal.
                Some('c') if self.term.selection_text().is_some() => return self.copy(true),
                Some('=') | Some('+') => return self.zoom(Some(1.0)),
                Some('-') | Some('_') => return self.zoom(Some(-1.0)),
                Some('0') if !shift => return self.zoom(None),
                _ => {}
            }
        }
        if shift && !ctrl && !alt {
            let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
            match named {
                Some(NamedKey::Insert) => return self.paste(),
                Some(NamedKey::PageUp) if !alt_screen => {
                    self.term.scroll_page_up();
                    self.dirty = true;
                    return;
                }
                Some(NamedKey::PageDown) if !alt_screen => {
                    self.term.scroll_page_down();
                    self.dirty = true;
                    return;
                }
                _ => {}
            }
        }
        if named == Some(NamedKey::F11) && !ctrl && !shift && !alt {
            let next = match self.window.fullscreen() {
                Some(_) => None,
                None => Some(Fullscreen::Borderless(None)),
            };
            self.window.set_fullscreen(next);
            return;
        }

        if let Some(bytes) = input::encode_key(event, self.mods, self.term.mode()) {
            self.send_input(bytes);
        }
    }

    fn zoom(&mut self, delta: Option<f32>) {
        self.renderer.zoom(delta);
        self.sync_grid();
    }

    fn copy(&mut self, clear: bool) {
        if let (Some(text), Some(clipboard)) = (self.term.selection_text(), self.clipboard.as_mut()) {
            let _ = clipboard.set_text(text);
        }
        if clear {
            self.term.selection_clear();
        }
        self.dirty = true;
    }

    fn paste(&mut self) {
        let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) else { return };
        if !text.is_empty() {
            let bytes = input::encode_paste(&text, self.term.mode());
            self.send_input(bytes);
        }
    }

    fn on_focus(&mut self, focused: bool) {
        self.focused = focused;
        self.blink_epoch = Instant::now();
        if !focused {
            // Modifier releases are not delivered while unfocused.
            self.mods = ModifiersState::empty();
            self.mouse.selecting = false;
        }
        if self.term.mode().contains(TermMode::FOCUS_IN_OUT) {
            self.pty.write(if focused { &b"\x1b[I"[..] } else { &b"\x1b[O"[..] });
        }
        self.dirty = true;
    }

    // ---- mouse -------------------------------------------------------------

    /// True when mouse events belong to the program in the terminal. Holding
    /// Shift always gives the mouse back to NullShell for selecting.
    fn mouse_reporting(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE) && !self.mods.shift_key()
    }

    fn report_mouse(&mut self, button: MouseButtonCode, pressed: bool, motion: bool) {
        let (col, row, _) = self.renderer.cell_at(self.mouse.x, self.mouse.y);
        if let Some(bytes) = input::encode_mouse(button, pressed, motion, col, row, self.mods, self.term.mode()) {
            self.pty.write(bytes);
        }
    }

    fn on_mouse_move(&mut self, x: f64, y: f64) {
        self.mouse.x = x;
        self.mouse.y = y;
        let (col, row, right) = self.renderer.cell_at(x, y);
        let moved = (col, row) != self.mouse.last_cell;
        self.mouse.last_cell = (col, row);

        if self.mouse.selecting {
            // Dragging past the top or bottom edge scrolls through history.
            let layout = self.renderer.layout();
            let top = layout.grid_y as f64;
            let bottom = top + (layout.rows as f32 * layout.cell_h) as f64;
            if y < top {
                self.term.scroll_lines(1);
            } else if y > bottom {
                self.term.scroll_lines(-1);
            }
            self.term.selection_update(col, row, right);
            self.dirty = true;
            return;
        }

        if moved && self.mouse_reporting() {
            let mode = self.term.mode();
            match self.mouse.reporting {
                Some(button) if mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) => {
                    self.report_mouse(button, true, true)
                }
                None if mode.contains(TermMode::MOUSE_MOTION) => {
                    self.report_mouse(MouseButtonCode::None, true, true)
                }
                _ => {}
            }
        }
    }

    fn on_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        let code = match button {
            MouseButton::Left => MouseButtonCode::Left,
            MouseButton::Middle => MouseButtonCode::Middle,
            MouseButton::Right => MouseButtonCode::Right,
            _ => return,
        };

        // A release always goes to whoever got the press.
        if !pressed {
            if self.mouse.reporting == Some(code) {
                self.mouse.reporting = None;
                self.report_mouse(code, false, false);
            } else if code == MouseButtonCode::Left {
                self.mouse.selecting = false;
            }
            return;
        }

        if self.mouse_reporting() {
            self.mouse.reporting = Some(code);
            self.report_mouse(code, true, false);
            return;
        }

        match code {
            MouseButtonCode::Left => {
                let (col, row, right) = self.renderer.cell_at(self.mouse.x, self.mouse.y);
                let now = Instant::now();
                let repeat = self.mouse.click_cell == (col, row)
                    && self.mouse.last_click.is_some_and(|t| now.duration_since(t) < Duration::from_millis(400));
                self.mouse.clicks = if repeat { self.mouse.clicks % 3 + 1 } else { 1 };
                self.mouse.last_click = Some(now);
                self.mouse.click_cell = (col, row);

                let kind = match self.mouse.clicks {
                    2 => SelectionKind::Word,
                    3 => SelectionKind::Line,
                    _ if self.mods.alt_key() => SelectionKind::Block,
                    _ => SelectionKind::Simple,
                };
                self.term.selection_start(col, row, right, kind);
                self.mouse.selecting = true;
            }
            // Right click: copy the selection if there is one, otherwise paste.
            MouseButtonCode::Right => {
                if self.term.selection_text().is_some() {
                    self.copy(true);
                } else {
                    self.paste();
                }
            }
            MouseButtonCode::Middle => self.paste(),
            _ => {}
        }
        self.dirty = true;
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 3.0,
            MouseScrollDelta::PixelDelta(p) => p.y as f32 / self.renderer.layout().cell_h,
        };
        self.mouse.wheel += lines;
        let steps = self.mouse.wheel.trunc() as i32;
        if steps == 0 {
            return;
        }
        self.mouse.wheel -= steps as f32;

        let mode = self.term.mode();
        if self.mouse_reporting() {
            let button = if steps > 0 { MouseButtonCode::WheelUp } else { MouseButtonCode::WheelDown };
            for _ in 0..steps.unsigned_abs().min(12) {
                self.report_mouse(button, true, false);
            }
        } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) && !self.mods.shift_key() {
            // Full screen programs without mouse support get arrow keys.
            let key: &[u8] = if steps > 0 { b"\x1bOA" } else { b"\x1bOB" };
            self.pty.write(key.repeat(steps.unsigned_abs().min(12) as usize));
        } else {
            self.term.scroll_lines(steps);
            self.dirty = true;
        }
    }
}

/// The icon embedded in the exe by build.rs (assets/nullshell.ico).
fn app_icon() -> Option<Icon> {
    Icon::from_resource(1, None).ok()
}
