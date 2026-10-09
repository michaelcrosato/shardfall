//! Graphics quality: how much of the screen's resolution the scene is drawn at and which costly
//! effects run. Auto is High on a desktop (everything, as the game always drew) and Medium on a
//! phone or tablet, where the resolution also follows the frame rate (dynamic resolution).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    Auto,
    Low,
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Auto, Quality::Low, Quality::Medium, Quality::High];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Auto => "Auto",
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }

    /// What Auto means on this device.
    fn resolve(self, touch: bool) -> Quality {
        match self {
            Quality::Auto if touch => Quality::Medium,
            Quality::Auto => Quality::High,
            q => q,
        }
    }

    /// Turns off what this quality cannot afford (on top of the scene's and the look's own
    /// settings).
    pub fn trim(self, touch: bool, v: &mut pav_view::ViewSettings) {
        match self.resolve(touch) {
            Quality::High | Quality::Auto => {}
            Quality::Medium => {
                v.gi = 0.0;
                v.shafts = 0.0;
            }
            Quality::Low => {
                v.gi = 0.0;
                v.shafts = 0.0;
                v.haze = 0.0;
                v.halos = 0.0;
                v.bloom = 0.0;
                v.distortion = false;
            }
        }
    }
}

/// The render scale, following the frame rate when the quality is Auto on a touch screen.
pub struct Scaler {
    /// The dynamic part (0.6..1) and the frames measured since it last changed.
    dynamic: f32,
    ms: f32,
    frames: u32,
    /// Seconds of smooth frames in a row (raising the resolution waits for a while of them).
    calm: f32,
    pub scale: f32,
}

impl Default for Scaler {
    fn default() -> Self {
        Self { dynamic: 1.0, ms: 0.0, frames: 0, calm: 0.0, scale: 1.0 }
    }
}

impl Scaler {
    /// Called once per drawn frame with how long the frame took (ms), the target's size in
    /// pixels and the device pixel ratio. Returns (render scale, point lights with shadows).
    pub fn update(&mut self, q: Quality, touch: bool, frame_ms: f32, size: (u32, u32), dpr: f32) -> (f32, usize) {
        let resolved = q.resolve(touch);
        let pixels = (size.0 as f32 * size.1 as f32).max(1.0);
        // At most this many megapixels (High: on a touch screen, two device pixels to a point
        // at most; a desktop draws every pixel, as it always has).
        let (base, lights) = match resolved {
            Quality::Low => ((0.5e6 / pixels).sqrt(), 0),
            Quality::Medium => ((1.0e6 / pixels).sqrt(), 1),
            _ if touch => (2.0 / dpr.max(1.0), pav_render::shadows::MAX_SHADOW_LIGHTS),
            _ => (1.0, pav_render::shadows::MAX_SHADOW_LIGHTS),
        };
        if q == Quality::Auto && touch {
            // A second at a time: slower than 50 fps drops a step, a few smooth seconds raise one.
            self.ms += frame_ms;
            self.frames += 1;
            if self.ms >= 1000.0 {
                let avg = self.ms / self.frames as f32;
                if avg > 20.0 {
                    self.dynamic = (self.dynamic * 0.88).max(0.6);
                    self.calm = 0.0;
                } else if avg < 17.6 {
                    self.calm += self.ms / 1000.0;
                    if self.calm >= 4.0 {
                        self.dynamic = (self.dynamic * 1.06).min(1.0);
                        self.calm = 0.0;
                    }
                } else {
                    self.calm = 0.0;
                }
                self.ms = 0.0;
                self.frames = 0;
            }
        } else {
            self.dynamic = 1.0;
        }
        self.scale = (base.min(1.0) * self.dynamic).clamp(0.25, 1.0);
        (self.scale, lights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktops_draw_everything_and_phones_draw_less() {
        let mut s = Scaler::default();
        // A 1600x900 desktop window: full resolution, every shadow (at any display scaling).
        assert_eq!(s.update(Quality::Auto, false, 16.0, (1600, 900), 1.0), (1.0, pav_render::shadows::MAX_SHADOW_LIGHTS));
        assert_eq!(s.update(Quality::Auto, false, 16.0, (3840, 2160), 3.0).0, 1.0);
        // A phone held sideways (2532x1170 device pixels): about a megapixel, one shadow.
        let (k, lights) = s.update(Quality::Auto, true, 16.0, (2532, 1170), 3.0);
        assert!((0.55..0.6).contains(&k), "{k}");
        assert_eq!(lights, 1);
        // Slow frames lower it, a while of smooth ones raise it back.
        for _ in 0..40 {
            s.update(Quality::Auto, true, 30.0, (2532, 1170), 3.0);
        }
        let low = s.scale;
        assert!(low < k * 0.9, "{low}");
        for _ in 0..60 * 30 {
            s.update(Quality::Auto, true, 16.0, (2532, 1170), 3.0);
        }
        assert!(s.scale > low, "{} {low}", s.scale);
        let mut v = pav_view::ViewSettings { gi: 0.5, ..Default::default() };
        Quality::Auto.trim(false, &mut v);
        assert_eq!(v.gi, 0.5);
        Quality::Auto.trim(true, &mut v);
        assert_eq!(v.gi, 0.0);
    }
}
