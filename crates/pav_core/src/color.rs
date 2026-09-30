use serde::{Deserialize, Serialize};

/// Linear RGB color. Build from sRGB with `Color::hex("#rrggbb")` or `Color::srgb8`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Color(pub [f32; 3]);

fn s2l(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

impl Color {
    pub const WHITE: Color = Color([1.0, 1.0, 1.0]);
    pub fn srgb8(r: u8, g: u8, b: u8) -> Self {
        Color([s2l(r as f32 / 255.0), s2l(g as f32 / 255.0), s2l(b as f32 / 255.0)])
    }
    /// Parses "#rrggbb"; falls back to magenta so mistakes are visible.
    pub fn hex(s: &str) -> Self {
        Self::try_hex(s).unwrap_or(Color::srgb8(255, 0, 255))
    }
    pub fn try_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Self::srgb8((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
    pub fn scale(self, k: f32) -> Self {
        Color([self.0[0] * k, self.0[1] * k, self.0[2] * k])
    }
    pub fn lerp(self, o: Color, t: f32) -> Self {
        Color([0, 1, 2].map(|i| self.0[i] + (o.0[i] - self.0[i]) * t))
    }
}
