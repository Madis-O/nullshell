//! Font loading, cell metrics and the glyph atlas.
//!
//! Each terminal cell is shaped on its own with cosmic-text (so font fallback,
//! emoji and combining marks work), rasterized once, and cached in a texture
//! atlas. Glyphs are then placed on the cell grid by the caller, which keeps
//! every character exactly aligned no matter which font it came from.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight};

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;

/// Font weights for HUD text ([`GlyphCache::ui`]).
pub const UI_REGULAR: u16 = 400;
pub const UI_MEDIUM: u16 = 500;
pub const UI_SEMIBOLD: u16 = 600;
pub const UI_BOLD: u16 = 700;

/// HUD typeface, bundled so the frame looks the same on every machine.
const UI_FAMILY: &str = "Rajdhani";
const UI_FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Rajdhani-Regular.ttf"),
    include_bytes!("../../assets/fonts/Rajdhani-Medium.ttf"),
    include_bytes!("../../assets/fonts/Rajdhani-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf"),
];
/// HUD text size relative to the terminal font. Rajdhani runs small.
const UI_SCALE: f32 = 1.18;

const ATLAS_START: u32 = 1024;
const ATLAS_MAX: u32 = 4096;

/// Fonts tried, in order, when the configured family is not installed.
const FALLBACK_FAMILIES: &[&str] = &["Cascadia Mono", "Cascadia Code", "Consolas", "Courier New"];

#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    /// Distance from the top of a cell to the text baseline.
    pub baseline: f32,
    pub font_px: f32,
}

/// One rasterized glyph image, positioned relative to its cell's top left.
#[derive(Clone, Copy)]
pub struct Glyph {
    pub uv: [f32; 4],
    pub left: f32,
    pub top: f32,
    pub w: f32,
    pub h: f32,
    pub colored: bool,
}

struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: u32,
    x: u32,
    y: u32,
    row_h: u32,
}

impl Atlas {
    fn new(device: &wgpu::Device, size: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view, size, x: 1, y: 1, row_h: 0 }
    }

    /// Simple shelf packer with one pixel of padding around each glyph.
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 2 > self.size || h + 2 > self.size {
            return None;
        }
        if self.x + w + 1 > self.size {
            self.x = 1;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.y + h + 1 > self.size {
            return None;
        }
        let pos = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }
}

pub struct GlyphCache {
    fonts: FontSystem,
    swash: SwashCache,
    family: String,
    /// Installed Nerd Font used for private-use (icon) code points.
    symbol_family: Option<String>,
    /// Family used for HUD text.
    ui_family: String,
    font_pt: f32,
    line_height: f32,
    scale: f32,
    metrics: CellMetrics,
    chars: HashMap<(char, u8), (u32, u32)>,
    clusters: HashMap<(Box<[char]>, u8), (u32, u32)>,
    /// HUD glyphs by (char, weight): glyph range plus advance width.
    ui_chars: HashMap<(char, u16), (u32, u32, f32)>,
    glyphs: Vec<Glyph>,
    atlas: Atlas,
    generation: u64,
    overflow: bool,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Set when the configured font was not found and another one was used.
    pub font_warning: Option<String>,
}

impl GlyphCache {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        family: &str,
        ui_family: &str,
        font_pt: f32,
        line_height: f32,
        scale: f32,
    ) -> Self {
        let mut fonts = FontSystem::new();
        for data in UI_FONTS {
            fonts.db_mut().load_font_data(data.to_vec());
        }
        let installed = |name: &str| {
            fonts.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
        };
        let mut font_warning = None;
        let family = if installed(family) {
            family.to_string()
        } else {
            let fallback = FALLBACK_FAMILIES.iter().copied().find(|f| installed(f)).unwrap_or("monospace");
            font_warning = Some(format!("font \"{family}\" not found, using {fallback}"));
            fallback.to_string()
        };

        // The HUD font can be swapped for any installed family in the config.
        let ui_family = if !ui_family.is_empty() && installed(ui_family) { ui_family } else { UI_FAMILY }.to_string();

        let mut symbol_family: Option<String> = None;
        for face in fonts.db().faces() {
            for (name, _) in &face.families {
                // Nerd Fonts ship under long names ("... Nerd Font") and short
                // Windows-friendly ones ("... NF", "... NFM", "... NFP").
                let short = [" NF", " NFM", " NFP"].iter().any(|s| name.ends_with(s));
                if !name.contains("Nerd Font") && !short {
                    continue;
                }
                let better = match &symbol_family {
                    None => true,
                    // Prefer the symbols-only font, it has the cleanest metrics.
                    Some(current) => name.starts_with("Symbols Nerd Font") && !current.starts_with("Symbols Nerd Font"),
                };
                if better {
                    symbol_family = Some(name.clone());
                }
            }
        }

        let mut cache = Self {
            fonts,
            swash: SwashCache::new(),
            family,
            symbol_family,
            ui_family,
            font_pt,
            line_height,
            scale,
            metrics: CellMetrics { cell_w: 8.0, cell_h: 16.0, baseline: 12.0, font_px: 13.0 },
            chars: HashMap::new(),
            clusters: HashMap::new(),
            ui_chars: HashMap::new(),
            glyphs: Vec::new(),
            atlas: Atlas::new(device, ATLAS_START),
            generation: 0,
            overflow: false,
            device: device.clone(),
            queue: queue.clone(),
            font_warning,
        };
        cache.measure();
        cache
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn font_pt(&self) -> f32 {
        self.font_pt
    }

    pub fn atlas_view(&self) -> &wgpu::TextureView {
        &self.atlas.view
    }

    /// Changes whenever the atlas texture is replaced.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Change font size and/or display scale. Drops all cached glyphs.
    pub fn set_size(&mut self, font_pt: f32, scale: f32) {
        self.font_pt = font_pt.clamp(5.0, 96.0);
        self.scale = scale;
        self.measure();
        self.reset(self.atlas.size);
    }

    fn measure(&mut self) {
        // Points to pixels at 96 dpi, times the monitor scale factor.
        let font_px = (self.font_pt * 96.0 / 72.0 * self.scale).max(4.0);
        let cell_h = (font_px * self.line_height).ceil().max(2.0);
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_px, cell_h));
        let attrs = Attrs::new().family(Family::Name(&self.family));
        buffer.set_size(None, None);
        buffer.set_text("M", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut cell_w = (font_px * 0.6).round();
        let mut baseline = (cell_h * 0.8).round();
        if let Some(run) = buffer.layout_runs().next() {
            baseline = run.line_y.round();
            if let Some(g) = run.glyphs.first() {
                cell_w = g.w.round();
            }
        }
        self.metrics = CellMetrics { cell_w: cell_w.max(1.0), cell_h, baseline, font_px };
    }

    fn reset(&mut self, atlas_size: u32) {
        self.chars.clear();
        self.clusters.clear();
        self.ui_chars.clear();
        self.glyphs.clear();
        self.atlas = Atlas::new(&self.device, atlas_size);
        self.generation += 1;
        self.overflow = false;
    }

    /// If the atlas filled up while building the last scene, make room and
    /// return true: the caller must rebuild the scene.
    pub fn recover_overflow(&mut self) -> bool {
        if !self.overflow {
            return false;
        }
        let size = (self.atlas.size * 2).min(ATLAS_MAX);
        self.reset(size);
        true
    }

    /// Glyph images for one cell: a base character plus optional combining
    /// characters. Empty for blanks and anything that cannot be drawn.
    pub fn get(&mut self, c: char, zerowidth: Option<&[char]>, style: u8) -> &[Glyph] {
        let range = match zerowidth {
            None | Some([]) => match self.chars.get(&(c, style)) {
                Some(r) => *r,
                None => {
                    let mut tmp = [0u8; 4];
                    let r = self.rasterize(c.encode_utf8(&mut tmp), c, style, None).map(|(a, b, _)| (a, b));
                    if let Some(r) = r {
                        self.chars.insert((c, style), r);
                    }
                    r.unwrap_or((0, 0))
                }
            },
            Some(extra) => {
                let mut key = Vec::with_capacity(extra.len() + 1);
                key.push(c);
                key.extend_from_slice(extra);
                let key = (key.into_boxed_slice(), style);
                match self.clusters.get(&key) {
                    Some(r) => *r,
                    None => {
                        let text: String = key.0.iter().collect();
                        let r = self.rasterize(&text, c, style, None).map(|(a, b, _)| (a, b));
                        if let Some(r) = r {
                            self.clusters.insert(key, r);
                        }
                        r.unwrap_or((0, 0))
                    }
                }
            }
        };
        &self.glyphs[range.0 as usize..(range.0 + range.1) as usize]
    }

    /// One HUD character in the HUD typeface at the given weight: its glyph
    /// images and how far to advance. HUD text is proportional, so callers lay
    /// characters out by advance instead of by cell.
    pub fn ui(&mut self, c: char, weight: u16) -> (&[Glyph], f32) {
        let (start, len, advance) = match self.ui_chars.get(&(c, weight)) {
            Some(r) => *r,
            None => {
                let mut tmp = [0u8; 4];
                let r = self.rasterize(c.encode_utf8(&mut tmp), c, 0, Some(weight));
                if let Some(r) = r {
                    self.ui_chars.insert((c, weight), r);
                }
                r.unwrap_or((0, 0, 0.0))
            }
        };
        (&self.glyphs[start as usize..(start + len) as usize], advance)
    }

    /// Shape and rasterize `text` into the atlas, returning the glyph range
    /// and the total advance. `ui` selects the HUD typeface at that weight.
    /// `None` means the atlas is full; the result must not be cached then.
    fn rasterize(&mut self, text: &str, first: char, style: u8, ui: Option<u16>) -> Option<(u32, u32, f32)> {
        let m = self.metrics;
        let family = match &self.symbol_family {
            _ if ui.is_some() => self.ui_family.as_str(),
            Some(symbols) if is_private_use(first) => symbols.as_str(),
            _ => self.family.as_str(),
        };
        let font_px = if ui.is_some() { m.font_px * UI_SCALE } else { m.font_px };
        let mut attrs = Attrs::new().family(Family::Name(family));
        if let Some(weight) = ui {
            attrs = attrs.weight(Weight(weight));
        }
        if style & STYLE_BOLD != 0 {
            attrs = attrs.weight(Weight::BOLD);
        }
        if style & STYLE_ITALIC != 0 {
            attrs = attrs.style(Style::Italic);
        }
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(font_px, m.cell_h));
        buffer.set_size(None, None);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);

        let start = self.glyphs.len();
        let mut advance = 0.0;
        let atlas_size = self.atlas.size as f32;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                advance += glyph.w;
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let Some(image) = self.swash.get_image_uncached(&mut self.fonts, physical.cache_key) else {
                    continue;
                };
                let (w, h) = (image.placement.width, image.placement.height);
                if w == 0 || h == 0 {
                    continue;
                }
                let (rgba, colored): (Vec<u8>, bool) = match image.content {
                    SwashContent::Mask => (image.data.iter().flat_map(|a| [255, 255, 255, *a]).collect(), false),
                    SwashContent::Color => (image.data, true),
                    SwashContent::SubpixelMask => (
                        image
                            .data
                            .chunks_exact(4)
                            .flat_map(|p| [255, 255, 255, ((p[0] as u16 + p[1] as u16 + p[2] as u16) / 3) as u8])
                            .collect(),
                        false,
                    ),
                };
                if rgba.len() != (w * h * 4) as usize {
                    continue;
                }
                let Some((ax, ay)) = self.atlas.alloc(w, h) else {
                    self.overflow = true;
                    self.glyphs.truncate(start);
                    return None;
                };
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.atlas.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: ax, y: ay, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                self.glyphs.push(Glyph {
                    uv: [
                        ax as f32 / atlas_size,
                        ay as f32 / atlas_size,
                        (ax + w) as f32 / atlas_size,
                        (ay + h) as f32 / atlas_size,
                    ],
                    left: (physical.x + image.placement.left) as f32,
                    top: m.baseline + (physical.y - image.placement.top) as f32,
                    w: w as f32,
                    h: h as f32,
                    colored,
                });
            }
        }
        Some((start as u32, (self.glyphs.len() - start) as u32, advance))
    }
}

/// Private Use Area code points, where Nerd Fonts keep their icons.
fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}
