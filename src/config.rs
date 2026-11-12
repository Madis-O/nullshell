//! TOML configuration, loaded once at startup.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::color::Rgb;

pub const DEFAULT_CONFIG: &str = include_str!("../assets/default_config.toml");

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub shell: ShellConfig,
    pub scrollback: ScrollbackConfig,
    pub cursor: CursorConfig,
    pub colors: ColorConfig,
    pub effects: EffectsConfig,
    pub hud: HudConfig,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self { family: "Cascadia Mono".into(), size: 12.0, line_height: 1.25 }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub opacity: f32,
    pub width: u32,
    pub height: u32,
    pub square_corners: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self { opacity: 0.93, width: 1100, height: 680, square_corners: true }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: String,
    /// Command run once when the shell starts (e.g. "fastfetch").
    pub startup_command: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScrollbackConfig {
    pub lines: usize,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self { lines: 10_000 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct CursorConfig {
    pub shape: CursorStyle,
    pub blink: bool,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self { shape: CursorStyle::Block, blink: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct AnsiColors {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
}

impl AnsiColors {
    pub fn as_array(&self) -> [Rgb; 8] {
        [
            self.black, self.red, self.green, self.yellow, self.blue, self.magenta, self.cyan,
            self.white,
        ]
    }

    fn normal() -> Self {
        Self {
            black: Rgb::new(0x12, 0x10, 0x1a),
            red: Rgb::new(0xff, 0x00, 0x3c),
            green: Rgb::new(0x00, 0xff, 0x9f),
            yellow: Rgb::new(0xfc, 0xee, 0x0a),
            blue: Rgb::new(0x3d, 0x7b, 0xff),
            magenta: Rgb::new(0xd8, 0x33, 0xff),
            cyan: Rgb::new(0x00, 0xf0, 0xff),
            white: Rgb::new(0xc8, 0xd3, 0xd5),
        }
    }

    fn bright() -> Self {
        Self {
            black: Rgb::new(0x7d, 0x78, 0x96),
            red: Rgb::new(0xff, 0x55, 0x78),
            green: Rgb::new(0x6d, 0xff, 0xc4),
            yellow: Rgb::new(0xff, 0xf7, 0x72),
            blue: Rgb::new(0x7a, 0xa5, 0xff),
            magenta: Rgb::new(0xee, 0x7d, 0xff),
            cyan: Rgb::new(0x8a, 0xf8, 0xff),
            white: Rgb::new(0xff, 0xff, 0xff),
        }
    }
}

impl Default for AnsiColors {
    fn default() -> Self {
        Self::normal()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ColorConfig {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    pub selection_background: Rgb,
    pub selection_foreground: Rgb,
    pub accent: Rgb,
    pub accent_alt: Rgb,
    pub alert: Rgb,
    pub normal: AnsiColors,
    pub bright: AnsiColors,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            foreground: Rgb::new(0xc5, 0xf6, 0xfa),
            background: Rgb::new(0x08, 0x07, 0x0d),
            cursor: Rgb::new(0xfc, 0xee, 0x0a),
            cursor_text: Rgb::new(0x08, 0x07, 0x0d),
            selection_background: Rgb::new(0xff, 0x00, 0x3c),
            selection_foreground: Rgb::new(0xff, 0xff, 0xff),
            accent: Rgb::new(0xfc, 0xee, 0x0a),
            accent_alt: Rgb::new(0x00, 0xf0, 0xff),
            alert: Rgb::new(0xff, 0x00, 0x3c),
            normal: AnsiColors::normal(),
            bright: AnsiColors::bright(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct EffectsConfig {
    pub enabled: bool,
    pub animate: bool,
    pub scanlines: f32,
    pub glow: f32,
    pub aberration: f32,
    pub vignette: f32,
    pub scan_beam: f32,
    pub noise: f32,
    pub glitch: f32,
}

impl Default for EffectsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            animate: true,
            scanlines: 0.10,
            glow: 0.45,
            aberration: 0.6,
            vignette: 0.35,
            scan_beam: 0.05,
            noise: 0.02,
            glitch: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct HudConfig {
    pub enabled: bool,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Config {
    /// Clamp values that would otherwise break rendering.
    fn sanitize(mut self) -> Self {
        self.font.size = self.font.size.clamp(5.0, 96.0);
        self.font.line_height = self.font.line_height.clamp(0.8, 3.0);
        self.window.opacity = self.window.opacity.clamp(0.1, 1.0);
        self.window.width = self.window.width.clamp(200, 16_000);
        self.window.height = self.window.height.clamp(120, 16_000);
        self.scrollback.lines = self.scrollback.lines.min(1_000_000);
        self
    }
}

/// `%APPDATA%\NullShell\config.toml`
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("NullShell").join("config.toml"))
}

/// Load the config. Never fails: on any problem the defaults are used and a
/// human readable warning is returned next to them.
pub fn load(path_override: Option<PathBuf>) -> (Config, Option<String>) {
    let explicit = path_override.is_some();
    let Some(path) = path_override.or_else(default_path) else {
        return (Config::default(), Some("no config directory found".into()));
    };

    if !path.exists() {
        if explicit {
            return (Config::default(), Some(format!("config not found: {}", path.display())));
        }
        // First run: drop a commented default config so there is something to edit.
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&path, DEFAULT_CONFIG);
        return (Config::default(), None);
    }

    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return (Config::default(), Some(format!("cannot read config: {e}"))),
    };
    match toml::from_str::<Config>(&text) {
        Ok(cfg) => (cfg.sanitize(), None),
        Err(e) => {
            let first = e.message().lines().next().unwrap_or("parse error").to_string();
            (Config::default(), Some(format!("config error: {first}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_default_struct() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).expect("default config parses");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let cfg: Config = toml::from_str("[font]\nsize = 20.0\n").unwrap();
        assert_eq!(cfg.font.size, 20.0);
        assert_eq!(cfg.font.family, "Cascadia Mono");
        assert_eq!(cfg.colors, ColorConfig::default());
    }

    #[test]
    fn bad_color_is_an_error() {
        assert!(toml::from_str::<Config>("[colors]\nforeground = \"red\"\n").is_err());
    }
}
