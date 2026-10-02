//! Small shared pieces: colours, the deterministic RNG, angle helpers.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Linear RGB colour. Write colours as sRGB hex: `Color::hex("#e8704a")`. In JSON and level
/// files a colour is that hex string.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color(pub [f32; 3]);

fn s2l(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn l2s(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

impl Color {
    pub const WHITE: Color = Color([1.0, 1.0, 1.0]);
    pub const BLACK: Color = Color([0.0, 0.0, 0.0]);

    /// Parses "#rrggbb" (sRGB). Mistakes come out magenta so they are easy to spot.
    pub fn hex(s: &str) -> Self {
        Self::try_hex(s).unwrap_or(Color([1.0, 0.0, 1.0]))
    }
    pub fn try_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        let c = |sh: u32| s2l(((v >> sh) & 255) as f32 / 255.0);
        Some(Color([c(16), c(8), c(0)]))
    }
    pub fn to_hex(self) -> String {
        let b = |x: f32| (l2s(x.clamp(0.0, 1.0)) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", b(self.0[0]), b(self.0[1]), b(self.0[2]))
    }
    /// Brightness multiplier (values above 1 are fine: glow).
    pub fn scale(self, k: f32) -> Self {
        Color(self.0.map(|c| c * k))
    }
    pub fn lerp(self, o: Color, t: f32) -> Self {
        Color([0, 1, 2].map(|i| self.0[i] + (o.0[i] - self.0[i]) * t))
    }
    pub fn vec(self) -> Vec3 {
        Vec3::from_array(self.0)
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::try_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour '{s}' (want \"#rrggbb\")")))
    }
}

/// Small deterministic RNG (PCG32). Game code must use `world.rng` (never the system clock or
/// thread RNG), so runs, rewinds and replays repeat exactly.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng { state: 0, inc: (seed << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed ^ 0x853c_49e6_748f_ea9b);
        r.next_u32();
        r
    }
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }
    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    /// Integer in [0, n).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next_u32() % n }
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
    /// A random unit direction on the ground plane (x, z).
    pub fn dir2(&mut self) -> Vec2 {
        let a = self.range(0.0, std::f32::consts::TAU);
        Vec2::new(a.cos(), a.sin())
    }
    /// A random point in the unit sphere.
    pub fn in_sphere(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.range(-1.0, 1.0), self.range(-1.0, 1.0), self.range(-1.0, 1.0));
            if v.length_squared() <= 1.0 {
                return v;
            }
        }
    }
}

/// Yaw (radians) of a direction on the ground plane: 0 = facing +Z (south), PI/2 = +X (east).
pub fn yaw_of(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

/// Unit direction on the ground plane for a yaw.
pub fn dir_of(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

/// Wraps an angle to [-PI, PI).
pub fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Moves `cur` toward `target` by at most `max_step`.
pub fn approach(cur: f32, target: f32, max_step: f32) -> f32 {
    cur + (target - cur).clamp(-max_step, max_step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_hex_roundtrip() {
        for h in ["#000000", "#ffffff", "#e8704a", "#2f4a7a"] {
            assert_eq!(Color::hex(h).to_hex(), h);
        }
        assert!(Color::try_hex("#12345").is_none());
    }

    #[test]
    fn rng_is_repeatable() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        assert!((0..1000).map(|_| a.f32()).all(|x| (0.0..1.0).contains(&x)));
    }
}
