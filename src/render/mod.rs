//! Rendering. Knows nothing about the pty or the terminal emulation: it is
//! handed a [`Frame`] and draws it.
//!
//! Pipeline, per frame:
//!   1. layers (`grid`, `hud`) push quads into a [`scene::Scene`]
//!   2. the scene pass draws those quads into an offscreen texture
//!   3. the post pass (`shaders/post.wgsl`) copies it to the window
//!
//! To add visuals: push more quads from a new layer in [`Renderer::render`],
//! or edit the post shader.

mod boxdraw;
mod glyphs;
mod gpu;
mod grid;
pub mod hud;
mod post;
pub mod scene;

use std::sync::Arc;

use anyhow::Result;
use winit::window::Window;

use crate::color::Rgb;
use crate::config::{Config, EffectsConfig};
use crate::frame::Frame;

use glyphs::GlyphCache;
use gpu::Gpu;
use post::{PostPipeline, PostUniforms};
use scene::{Scene, ScenePipeline};

pub use hud::HudState;

/// Colors the layers draw with.
#[derive(Clone, Copy)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    pub accent: Rgb,
    pub accent_alt: Rgb,
    pub alert: Rgb,
}

/// Where things are, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    /// Monitor scale factor (1.0 = 96 dpi).
    pub scale: f32,
    pub pad: f32,
    pub header_h: f32,
    pub footer_h: f32,
    /// Decorative frame around the terminal: x, y, w, h.
    pub frame: [f32; 4],
    pub grid_x: f32,
    pub grid_y: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub cols: usize,
    pub rows: usize,
}

/// Values that change from frame to frame.
pub struct FrameFx {
    /// Seconds since startup, drives animated effects.
    pub time: f32,
    /// 0..1, strength of the glitch burst.
    pub glitch: f32,
    pub cursor_on: bool,
    pub focused: bool,
}

pub struct Renderer {
    gpu: Gpu,
    scene: Scene,
    scene_pipeline: ScenePipeline,
    post: PostPipeline,
    glyphs: GlyphCache,
    theme: Theme,
    effects: EffectsConfig,
    hud_enabled: bool,
    opacity: f32,
    base_font_pt: f32,
    scale: f32,
    layout: Layout,
}

impl Renderer {
    pub fn new(window: Arc<Window>, cfg: &Config) -> Result<Self> {
        let scale = window.scale_factor() as f32;
        let gpu = Gpu::new(window, cfg.window.opacity < 1.0)?;
        let glyphs = GlyphCache::new(&gpu.device, &gpu.queue, &cfg.font.family, cfg.font.size, cfg.font.line_height, scale);
        let scene_pipeline = ScenePipeline::new(&gpu.device);
        let post = PostPipeline::new(&gpu.device, gpu.config.format, gpu.config.width, gpu.config.height);
        let theme = Theme {
            foreground: cfg.colors.foreground,
            background: cfg.colors.background,
            cursor: cfg.colors.cursor,
            cursor_text: cfg.colors.cursor_text,
            accent: cfg.colors.accent,
            accent_alt: cfg.colors.accent_alt,
            alert: cfg.colors.alert,
        };
        let mut renderer = Self {
            layout: Layout {
                width: 1.0, height: 1.0, scale, pad: 0.0, header_h: 0.0, footer_h: 0.0,
                frame: [0.0; 4], grid_x: 0.0, grid_y: 0.0, cell_w: 1.0, cell_h: 1.0, cols: 2, rows: 1,
            },
            gpu,
            scene: Scene::default(),
            scene_pipeline,
            post,
            glyphs,
            theme,
            effects: cfg.effects.clone(),
            hud_enabled: cfg.hud.enabled,
            opacity: cfg.window.opacity,
            base_font_pt: cfg.font.size,
            scale,
        };
        renderer.update_layout();
        Ok(renderer)
    }

    fn update_layout(&mut self) {
        let m = self.glyphs.metrics();
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);

        let (pad, header_h, footer_h, frame, inset) = if self.hud_enabled {
            let pad = px(10.0);
            let header_h = m.cell_h + px(14.0);
            let footer_h = m.cell_h + px(14.0);
            let fy = header_h + px(5.0);
            let frame = [pad, fy, w - pad * 2.0, h - fy - footer_h - px(2.0)];
            (pad, header_h, footer_h, frame, px(9.0))
        } else {
            (0.0, 0.0, 0.0, [0.0, 0.0, w, h], px(6.0))
        };

        let avail_w = (frame[2] - inset * 2.0).max(0.0);
        let avail_h = (frame[3] - inset * 2.0).max(0.0);
        self.layout = Layout {
            width: w,
            height: h,
            scale: s,
            pad,
            header_h,
            footer_h,
            frame,
            grid_x: frame[0] + inset,
            grid_y: frame[1] + inset,
            cell_w: m.cell_w,
            cell_h: m.cell_h,
            cols: ((avail_w / m.cell_w).floor() as usize).max(2),
            rows: ((avail_h / m.cell_h).floor() as usize).max(1),
        };
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// True when the window background really is see-through. If false and
    /// opacity < 1, the caller can fall back to whole-window opacity.
    pub fn is_transparent(&self) -> bool {
        self.gpu.transparent
    }

    /// A problem worth telling the user about (missing font).
    pub fn warning(&self) -> Option<&str> {
        self.glyphs.font_warning.as_deref()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.gpu.resize(width, height);
        self.post.resize(&self.gpu.device, width, height);
        self.update_layout();
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.glyphs.set_size(self.glyphs.font_pt(), scale);
        self.update_layout();
    }

    /// Grow or shrink the font by `delta` points; `None` resets to the config size.
    pub fn zoom(&mut self, delta: Option<f32>) {
        let size = match delta {
            Some(d) => self.glyphs.font_pt() + d,
            None => self.base_font_pt,
        };
        self.glyphs.set_size(size, self.scale);
        self.update_layout();
    }

    /// Cell under a pixel position: (column, row, in the right half of the cell).
    /// Positions outside the grid are clamped to its edge.
    pub fn cell_at(&self, x: f64, y: f64) -> (usize, usize, bool) {
        let l = &self.layout;
        let fx = ((x as f32 - l.grid_x) / l.cell_w).max(0.0);
        let fy = ((y as f32 - l.grid_y) / l.cell_h).max(0.0);
        let col = (fx as usize).min(l.cols.saturating_sub(1));
        let row = (fy as usize).min(l.rows.saturating_sub(1));
        let right = fx.fract() >= 0.5 || fx as usize >= l.cols;
        (col, row, right)
    }

    pub fn render(&mut self, frame: &Frame, hud: &HudState, fx: &FrameFx) {
        if self.gpu.config.width == 0 || self.gpu.config.height == 0 {
            return;
        }

        // Build the scene. If the glyph atlas fills up half way through it is
        // enlarged and the scene is built again.
        for _ in 0..3 {
            self.scene.clear();
            let params = grid::GridParams {
                x: self.layout.grid_x,
                y: self.layout.grid_y,
                cursor: self.theme.cursor,
                cursor_text: self.theme.cursor_text,
                cursor_on: fx.cursor_on,
                focused: fx.focused,
            };
            grid::draw(&mut self.scene, &mut self.glyphs, frame, &params);
            if self.hud_enabled {
                hud::draw(&mut self.scene, &mut self.glyphs, &self.layout, &self.theme, frame, hud, fx.time);
            }
            if !self.glyphs.recover_overflow() {
                break;
            }
        }

        let (w, h) = (self.gpu.config.width as f32, self.gpu.config.height as f32);
        self.scene_pipeline.upload(
            &self.gpu.device,
            &self.gpu.queue,
            &self.scene,
            [w, h],
            self.glyphs.atlas_view(),
            self.glyphs.generation(),
        );

        let fxc = &self.effects;
        let on = |v: f32| if fxc.enabled { v.max(0.0) } else { 0.0 };
        self.post.set_uniforms(
            &self.gpu.queue,
            PostUniforms {
                resolution: [w, h],
                time: fx.time,
                glitch: on(fx.glitch * fxc.glitch).min(1.0),
                scanlines: on(fxc.scanlines),
                glow: on(fxc.glow),
                aberration: on(fxc.aberration),
                vignette: on(fxc.vignette),
                scan_beam: if fxc.animate && fx.focused { on(fxc.scan_beam) } else { 0.0 },
                noise: if fxc.animate { on(fxc.noise) } else { 0.0 },
                scale: self.scale,
            },
        );

        use wgpu::CurrentSurfaceTexture as Current;
        let (target, suboptimal) = match self.gpu.surface.get_current_texture() {
            Current::Success(t) => (t, false),
            Current::Suboptimal(t) => (t, true),
            Current::Outdated | Current::Lost => {
                self.gpu.reconfigure();
                return;
            }
            _ => return,
        };
        let target_view = target.texture.create_view(&Default::default());

        // Background, premultiplied by the window opacity.
        let a = if self.gpu.transparent { self.opacity as f64 } else { 1.0 };
        let bg = self.theme.background;
        let clear = wgpu::Color {
            r: bg.r as f64 / 255.0 * a,
            g: bg.g as f64 / 255.0 * a,
            b: bg.b as f64 / 255.0 * a,
            a,
        };

        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.post.scene_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            self.scene_pipeline.draw(&mut pass);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.post.draw(&mut pass);
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(target);
        if suboptimal {
            self.gpu.reconfigure();
        }
    }
}
