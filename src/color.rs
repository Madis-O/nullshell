use serde::{Deserialize, Deserializer};

/// 8-bit sRGB color shared by the config, the terminal model and the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Self::new(p(0)?, p(2)?, p(4)?))
    }

    /// Straight-alpha float color, as the scene pipeline expects.
    pub fn rgba(self, a: f32) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            a,
        ]
    }

    pub fn scale(self, f: f32) -> Self {
        let s = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
        Self::new(s(self.r), s(self.g), s(self.b))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::from_hex(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?}, expected \"#rrggbb\"")))
    }
}
