//! The camera rig: every parameter is live. "2D top-down" is just tilt 90 + orthographic.

use glam::{Mat4, Vec2, Vec3};
use pav_core::params::{ParamVisitor, Tunable};
use pav_render::CameraData;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraParams {
    /// Degrees above the horizon: 90 = straight down, 0 = side view.
    pub tilt: f32,
    /// Degrees; 0 = looking north (-Z).
    pub yaw: f32,
    /// Distance from the focus point (metres). In orthographic mode it sets the zoom.
    pub distance: f32,
    /// Vertical field of view (degrees). Orthographic uses it to match framing.
    pub fov: f32,
    pub ortho: bool,
    /// Seconds for the camera to catch up with the focus (0 = locked).
    pub follow_lag: f32,
    /// Look slightly above the focus point (metres).
    pub height_offset: f32,
}

impl Default for CameraParams {
    fn default() -> Self {
        Self { tilt: 62.0, yaw: 0.0, distance: 18.0, fov: 40.0, ortho: false, follow_lag: 0.12, height_offset: 0.8 }
    }
}

impl Tunable for CameraParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("tilt", &mut self.tilt, 0.0, 90.0, "Degrees above the horizon (90 = top-down, 0 = side view)");
        v.float("yaw", &mut self.yaw, -180.0, 180.0, "Camera rotation around the focus (degrees)");
        v.float("distance", &mut self.distance, 2.0, 120.0, "Distance / zoom (m)");
        v.float("fov", &mut self.fov, 5.0, 100.0, "Vertical field of view (degrees)");
        v.bool("ortho", &mut self.ortho, "Orthographic projection (flat 2D look)");
        v.float("follow_lag", &mut self.follow_lag, 0.0, 1.0, "Seconds to catch up with the target");
        v.float("height_offset", &mut self.height_offset, -2.0, 4.0, "Aim above the target (m)");
    }
}

impl CameraParams {
    pub const PRESETS: &'static [(&'static str, fn() -> CameraParams)] = &[
        ("classic 62°", || CameraParams::default()),
        ("top-down 90° ortho", || CameraParams { tilt: 90.0, ortho: true, distance: 22.0, ..Default::default() }),
        ("high 80°", || CameraParams { tilt: 80.0, ..Default::default() }),
        ("70°", || CameraParams { tilt: 70.0, ..Default::default() }),
        ("45°", || CameraParams { tilt: 45.0, distance: 16.0, ..Default::default() }),
        ("isometric", || CameraParams { tilt: 35.264, yaw: 45.0, ortho: true, distance: 24.0, ..Default::default() }),
        ("side view", || CameraParams { tilt: 0.0, ortho: true, distance: 20.0, height_offset: 2.0, ..Default::default() }),
        ("close 3rd person", || CameraParams { tilt: 25.0, distance: 7.0, fov: 60.0, height_offset: 1.4, ..Default::default() }),
    ];
}

impl CameraParams {
    /// Blend between two camera setups (yaw takes the short way round; projection switches
    /// halfway).
    pub fn lerp(&self, o: &CameraParams, t: f32) -> CameraParams {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let dyaw = ((o.yaw - self.yaw + 540.0).rem_euclid(360.0)) - 180.0;
        CameraParams {
            tilt: l(self.tilt, o.tilt),
            yaw: ((self.yaw + dyaw * t + 540.0).rem_euclid(360.0)) - 180.0,
            distance: l(self.distance, o.distance),
            fov: l(self.fov, o.fov),
            ortho: if t < 0.5 { self.ortho } else { o.ortho },
            follow_lag: l(self.follow_lag, o.follow_lag),
            height_offset: l(self.height_offset, o.height_offset),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CameraRig {
    /// Target parameters (what the panel edits).
    pub params: CameraParams,
    /// Smoothed focus point.
    pub target: Vec3,
    initialized: bool,
    /// Blending from earlier parameters: (from, elapsed, duration).
    blend: Option<(CameraParams, f32, f32)>,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new(CameraParams::default())
    }
}

impl CameraRig {
    pub fn new(params: CameraParams) -> Self {
        Self { params, target: Vec3::ZERO, initialized: false, blend: None }
    }

    /// Parameters in effect right now (blended while a transition runs).
    pub fn current(&self) -> CameraParams {
        match &self.blend {
            Some((from, e, d)) => {
                let t = (e / d.max(1e-3)).clamp(0.0, 1.0);
                from.lerp(&self.params, t * t * (3.0 - 2.0 * t))
            }
            None => self.params.clone(),
        }
    }

    /// Call before changing `params` to glide there over `seconds` instead of cutting.
    pub fn blend_from_current(&mut self, seconds: f32) {
        self.blend = Some((self.current(), 0.0, seconds));
    }

    /// Follows `focus` with exponential smoothing.
    pub fn update(&mut self, focus: Vec3, dt: f32) {
        if let Some(b) = &mut self.blend {
            b.1 += dt;
            if b.1 >= b.2 {
                self.blend = None;
            }
        }
        if !self.initialized || self.params.follow_lag <= 0.0 {
            self.target = focus;
            self.initialized = true;
            return;
        }
        let k = 1.0 - (-dt / self.current().follow_lag.max(1e-3)).exp();
        self.target += (focus - self.target) * k;
    }

    pub fn snap(&mut self, focus: Vec3) {
        self.target = focus;
        self.initialized = true;
    }

    /// Horizontal forward (camera looking direction flattened), and right.
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let yaw = self.current().yaw.to_radians();
        let fwd = Vec3::new(yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        (fwd, right)
    }

    pub fn forward(&self) -> Vec3 {
        let (fh, _) = self.ground_axes();
        let t = self.current().tilt.clamp(0.0, 90.0).to_radians();
        (fh * t.cos() + Vec3::NEG_Y * t.sin()).normalize()
    }

    pub fn up(&self) -> Vec3 {
        let (fh, _) = self.ground_axes();
        let t = self.current().tilt.clamp(0.0, 90.0).to_radians();
        (fh * t.sin() + Vec3::Y * t.cos()).normalize()
    }

    pub fn look_at(&self) -> Vec3 {
        self.target + Vec3::Y * self.current().height_offset
    }

    pub fn eye(&self) -> Vec3 {
        self.look_at() - self.forward() * self.current().distance
    }

    pub fn data(&self, aspect: f32) -> CameraData {
        let p = &self.current();
        let fwd = self.forward();
        let eye = self.eye();
        let view = glam::camera::rh::view::look_to_mat4(eye, fwd, self.up());
        let mut fov = p.fov.clamp(1.0, 170.0).to_radians();
        if aspect < 1.0 && aspect > 0.0 {
            // Taller than wide (a phone held upright): the field of view is the width's, so the
            // sides are not cut down to a sliver.
            fov = (2.0 * ((fov * 0.5).tan() / aspect).atan()).min(150f32.to_radians());
        }
        let far = (p.distance * 4.0 + 200.0).max(300.0);
        let proj = if p.ortho {
            let half_h = p.distance * (fov * 0.5).tan();
            let half_w = half_h * aspect;
            glam::camera::rh::proj::directx::orthographic(-half_w, half_w, -half_h, half_h, 0.05, far)
        } else {
            glam::camera::rh::proj::directx::perspective(fov, aspect, 0.1, far)
        };
        CameraData { view, proj, eye, forward: fwd, ortho: p.ortho }
    }

    /// World-space ray through a pixel (origin, direction).
    pub fn screen_ray(&self, px: Vec2, size: Vec2) -> (Vec3, Vec3) {
        let cam = self.data(size.x / size.y.max(1.0));
        let inv = (cam.proj * cam.view).inverse();
        let ndc = Vec2::new(px.x / size.x * 2.0 - 1.0, 1.0 - px.y / size.y * 2.0);
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// Where a pixel's ray hits the horizontal plane at `height`.
    pub fn ground_point(&self, px: Vec2, size: Vec2, height: f32) -> Option<Vec3> {
        let (o, d) = self.screen_ray(px, size);
        if d.y.abs() < 1e-5 {
            return None;
        }
        let t = (height - o.y) / d.y;
        (t > 0.0).then(|| o + d * t)
    }

    /// Converts stick/WASD input (x right, y up) into a world direction on the ground plane.
    pub fn relative_move(&self, input: Vec2) -> Vec2 {
        let (f, r) = self.ground_axes();
        let w = r * input.x + f * input.y;
        Vec2::new(w.x, w.z)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let d = self.data(aspect);
        d.proj * d.view
    }
}
