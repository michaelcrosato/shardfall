//! CPU renderer: no GPU or drivers needed, so captures work in any container. Boxes and
//! cylinders are rasterised; spheres and tapered capsules are ray-traced per pixel (smooth at
//! any zoom). Then sun shadows, toon or smooth lighting, outlines, fog, and 2D text and
//! rectangles on top. Rows are split across threads.

use glam::{Mat4, Quat, Vec3};

use crate::entity::Look;
use crate::font;
use crate::util::Color;

/// A drawable shape in world space.
#[derive(Clone, Copy, Debug)]
pub enum Prim {
    Box {
        center: Vec3,
        rot: Quat,
        half: Vec3,
    },
    Cylinder {
        center: Vec3,
        rot: Quat,
        half_height: f32,
        radius: f32,
    },
    /// Tapered capsule from `a` (radius `ra`) to `b` (radius `rb`); `a == b` is a sphere.
    Cone {
        a: Vec3,
        b: Vec3,
        ra: f32,
        rb: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub prim: Prim,
    pub color: Color,
    pub look: Look,
    /// Casts a sun shadow.
    pub shadow: bool,
}

/// Camera for one render.
#[derive(Clone, Copy, Debug)]
pub struct Cam {
    pub eye: Vec3,
    pub fwd: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// Perspective: tan of half the vertical field of view. Orthographic: half the view
    /// height in metres.
    pub half: f32,
    pub ortho: bool,
    pub near: f32,
    pub far: f32,
}

impl Cam {
    /// A camera at `eye` looking along `fwd`. `fov_deg` is the vertical field of view; for
    /// orthographic cameras `ortho_half` is half the visible height (m).
    pub fn new(eye: Vec3, fwd: Vec3, fov_deg: f32, ortho: Option<f32>, far: f32) -> Self {
        let fwd = fwd.normalize_or(Vec3::NEG_Z);
        let hint = if fwd.y.abs() > 0.999 { Vec3::NEG_Z } else { Vec3::Y };
        let right = fwd.cross(hint).normalize();
        let up = right.cross(fwd).normalize();
        let half = match ortho {
            Some(h) => h.max(0.01),
            None => (fov_deg.clamp(1.0, 170.0).to_radians() * 0.5).tan(),
        };
        let near = if ortho.is_some() { 0.05 } else { 0.1 };
        Cam { eye, fwd, up, right, half, ortho: ortho.is_some(), near, far: far.max(near + 1.0) }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        use glam::camera::rh::{proj::directx, view::look_to_mat4};
        let view = look_to_mat4(self.eye, self.fwd, self.up);
        let proj = if self.ortho {
            let (hh, hw) = (self.half, self.half * aspect);
            directx::orthographic(-hw, hw, -hh, hh, self.near, self.far)
        } else {
            directx::perspective(2.0 * self.half.atan(), aspect, self.near, self.far)
        };
        proj * view
    }

    /// Ray through normalised screen coordinates (u right, v up, both -1..1).
    pub fn ray(&self, u: f32, v: f32, aspect: f32) -> (Vec3, Vec3) {
        if self.ortho {
            (self.eye + self.right * (u * self.half * aspect) + self.up * (v * self.half), self.fwd)
        } else {
            let d = self.fwd + self.right * (u * self.half * aspect) + self.up * (v * self.half);
            (self.eye, d.normalize())
        }
    }

    /// Pixel position of a world point, if it is in front of the camera.
    pub fn project(&self, p: Vec3, width: usize, height: usize) -> Option<(f32, f32)> {
        let c = self.view_proj(width as f32 / height.max(1) as f32) * p.extend(1.0);
        if c.w <= 1e-4 || c.z < 0.0 {
            return None;
        }
        Some(((c.x / c.w * 0.5 + 0.5) * width as f32, (0.5 - c.y / c.w * 0.5) * height as f32))
    }
}

/// Light and atmosphere for one render.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub sky: Color,
    pub horizon: Color,
    /// Toward the sun.
    pub sun_dir: Vec3,
    /// Sun colour times strength.
    pub sun: Color,
    pub ambient: f32,
    /// Distance fully covered by fog (0 = none).
    pub fog: f32,
    pub shadows: bool,
    pub outlines: bool,
    /// The area that gets shadows (around the camera target).
    pub shadow_center: Vec3,
    pub shadow_radius: f32,
}

/// 2D drawing on top of the 3D image, in pixels.
#[derive(Clone, Debug)]
pub enum Overlay {
    Text { x: i32, y: i32, scale: f32, color: Color, text: String },
    Rect { x: i32, y: i32, w: i32, h: i32, color: Color, alpha: f32 },
}

pub struct Scene {
    pub cam: Cam,
    pub light: Lighting,
    pub items: Vec<Item>,
    pub overlay: Vec<Overlay>,
}

/// An sRGB image, one `0x00RRGGBB` per pixel, rows top to bottom.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(width: usize, height: usize, fill: u32) -> Self {
        Self { width, height, px: vec![fill; width * height] }
    }

    pub fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("png header");
            let rgb: Vec<u8> = self.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
            w.write_image_data(&rgb).expect("png data");
        }
        out
    }

    /// Copies `src` into this image at (x, y).
    pub fn blit(&mut self, src: &Image, x: usize, y: usize) {
        for row in 0..src.height {
            let ty = y + row;
            if ty >= self.height {
                break;
            }
            let n = src.width.min(self.width.saturating_sub(x));
            let d = ty * self.width + x;
            self.px[d..d + n].copy_from_slice(&src.px[row * src.width..row * src.width + n]);
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color, alpha: f32) {
        let c = srgb_u32(color.vec());
        let a = alpha.clamp(0.0, 1.0);
        let (x0, y0) = (x.max(0) as usize, y.max(0) as usize);
        let (x1, y1) = ((x + w).clamp(0, self.width as i32) as usize, (y + h).clamp(0, self.height as i32) as usize);
        for yy in y0..y1 {
            for xx in x0..x1 {
                let p = &mut self.px[yy * self.width + xx];
                *p = if a >= 1.0 { c } else { blend(*p, c, a) };
            }
        }
    }

    /// 8x8 bitmap text, `scale` pixels per font pixel (fractions are fine), with a dark
    /// outline so it reads on any background.
    pub fn text(&mut self, x: i32, y: i32, scale: f32, color: Color, text: &str) {
        let s = scale.max(0.5);
        let o = s.round().max(1.0) as i32;
        let dark = Color::hex("#101216");
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (1, 1)] {
            self.glyphs(x + dx * o, y + dy * o, s, dark, text);
        }
        self.glyphs(x, y, s, color, text);
    }

    fn glyphs(&mut self, x: i32, y: i32, s: f32, color: Color, text: &str) {
        let c = srgb_u32(color.vec());
        let cell = 8.0 * s;
        let (h, w) = ((cell.ceil() as i32).max(1), self.width as i32);
        for (i, ch) in text.chars().enumerate() {
            let g = font::glyph(ch);
            let x0 = x + (i as f32 * cell) as i32;
            let x1 = x + ((i + 1) as f32 * cell) as i32;
            for py in y.max(0)..(y + h).min(self.height as i32) {
                let row = (((py - y) as f32 + 0.5) / s) as usize;
                let Some(bits) = g.get(row) else { continue };
                if *bits == 0 {
                    continue;
                }
                for px in x0.max(0)..x1.min(w) {
                    let col = (((px - x0) as f32 + 0.5) / s) as u32;
                    if col < 8 && bits >> col & 1 == 1 {
                        self.px[py as usize * self.width + px as usize] = c;
                    }
                }
            }
        }
    }
}

fn blend(a: u32, b: u32, t: f32) -> u32 {
    let ch = |sh: u32| {
        let (x, y) = (((a >> sh) & 255) as f32, ((b >> sh) & 255) as f32);
        ((x + (y - x) * t).round() as u32).min(255) << sh
    };
    ch(16) | ch(8) | ch(0)
}

/// Linear colour to packed sRGB.
pub fn srgb_u32(c: Vec3) -> u32 {
    let e = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let s = if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u32
    };
    (e(c.x) << 16) | (e(c.y) << 8) | e(c.z)
}

/// Fast sRGB encode through a table (shading pass).
struct Encoder([u8; 4096]);

impl Encoder {
    fn new() -> Self {
        let mut t = [0u8; 4096];
        for (i, v) in t.iter_mut().enumerate() {
            let x = i as f32 / 4095.0;
            let s = if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
            *v = (s * 255.0 + 0.5) as u8;
        }
        Encoder(t)
    }
    fn pack(&self, c: Vec3) -> u32 {
        let e = |x: f32| self.0[(x.clamp(0.0, 1.0) * 4095.0) as usize] as u32;
        (e(c.x) << 16) | (e(c.y) << 8) | e(c.z)
    }
}

// ---------------------------------------------------------------------------- geometry

struct Tri {
    p: [Vec3; 3],
    n: Vec3,
    item: u32,
}

fn mesh(item: &Item, idx: u32, out: &mut Vec<Tri>) {
    fn quad(out: &mut Vec<Tri>, c: [Vec3; 4], n: Vec3, item: u32) {
        out.push(Tri { p: [c[0], c[1], c[2]], n, item });
        out.push(Tri { p: [c[0], c[2], c[3]], n, item });
    }
    match item.prim {
        Prim::Box { center, rot, half } => {
            let axes = [Vec3::X, Vec3::Y, Vec3::Z];
            for i in 0..3 {
                let (j, k) = ((i + 1) % 3, (i + 2) % 3);
                for s in [-1.0f32, 1.0] {
                    let corner = |a: f32, b: f32| center + rot * ((axes[i] * s + axes[j] * a + axes[k] * b) * half);
                    let c = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
                    quad(out, c, rot * (axes[i] * s), idx);
                }
            }
        }
        Prim::Cylinder { center, rot, half_height, radius } => {
            const N: usize = 16;
            let at = |k: usize, y: f32| {
                let a = k as f32 / N as f32 * std::f32::consts::TAU;
                center + rot * Vec3::new(a.cos() * radius, y, a.sin() * radius)
            };
            for k in 0..N {
                let mid = (k as f32 + 0.5) / N as f32 * std::f32::consts::TAU;
                let n = rot * Vec3::new(mid.cos(), 0.0, mid.sin());
                quad(out, [at(k, -half_height), at(k + 1, -half_height), at(k + 1, half_height), at(k, half_height)], n, idx);
                for (y, ny) in [(half_height, 1.0f32), (-half_height, -1.0)] {
                    out.push(Tri { p: [center + rot * Vec3::Y * y, at(k, y), at(k + 1, y)], n: rot * (Vec3::Y * ny), item: idx });
                }
            }
        }
        Prim::Cone { .. } => {}
    }
}

fn sphere_hit(ro: Vec3, rd: Vec3, c: Vec3, r: f32) -> Option<(f32, Vec3)> {
    let oc = ro - c;
    let b = oc.dot(rd);
    let h = b * b - (oc.dot(oc) - r * r);
    if h < 0.0 {
        return None;
    }
    let t = -b - h.sqrt();
    (t > 0.0).then(|| (t, (oc + rd * t) / r))
}

/// Ray vs tapered capsule (Inigo Quilez's rounded cone). `rd` must be normalised.
fn cone_hit(ro: Vec3, rd: Vec3, pa: Vec3, pb: Vec3, ra: f32, rb: f32) -> Option<(f32, Vec3)> {
    let ba = pb - pa;
    let m0 = ba.dot(ba);
    let rr = ra - rb;
    if m0 <= rr * rr + 1e-8 {
        // One end's sphere swallows the other.
        return if ra >= rb { sphere_hit(ro, rd, pa, ra) } else { sphere_hit(ro, rd, pb, rb) };
    }
    let (oa, ob) = (ro - pa, ro - pb);
    let (m1, m2, m3) = (ba.dot(oa), ba.dot(rd), rd.dot(oa));
    let (m5, m6, m7) = (oa.dot(oa), ob.dot(rd), ob.dot(ob));
    let d2 = m0 - rr * rr;
    let k2 = d2 - m2 * m2;
    let k1 = d2 * m3 - m1 * m2 + m2 * rr * ra;
    let k0 = d2 * m5 - m1 * m1 + m1 * rr * ra * 2.0 - m0 * ra * ra;
    let h = k1 * k1 - k0 * k2;
    if h < 0.0 {
        return None;
    }
    if k2.abs() > 1e-9 {
        let t = (-h.sqrt() - k1) / k2;
        let y = m1 - ra * rr + t * m2;
        if y > 0.0 && y < d2 {
            return (t > 0.0).then(|| (t, (d2 * (oa + rd * t) - ba * y).normalize()));
        }
    }
    let h1 = m3 * m3 - m5 + ra * ra;
    let h2 = m6 * m6 - m7 + rb * rb;
    let mut best: Option<(f32, Vec3)> = None;
    if h1 > 0.0 {
        let t = -m3 - h1.sqrt();
        if t > 0.0 {
            best = Some((t, (oa + rd * t) / ra));
        }
    }
    if h2 > 0.0 {
        let t = -m6 - h2.sqrt();
        if t > 0.0 && best.is_none_or(|b| t < b.0) {
            best = Some((t, (ob + rd * t) / rb));
        }
    }
    best
}

/// A triangle in screen space (x, y pixels; z depth 0..1).
struct STri {
    x: [f32; 3],
    y: [f32; 3],
    z: [f32; 3],
    n: Vec3,
    item: u32,
}

/// A ray-traced shape and its screen rectangle.
struct SCone {
    a: Vec3,
    b: Vec3,
    ra: f32,
    rb: f32,
    item: u32,
    rect: [i32; 4],
}

/// Projects, culls and near-clips triangles; finds the screen rectangles of cones.
fn setup(items: &[Item], cam: &Cam, w: usize, h: usize, shadow_pass: bool) -> (Vec<STri>, Vec<SCone>) {
    let aspect = w as f32 / h as f32;
    let vp = cam.view_proj(aspect);
    let mut tris = Vec::new();
    let mut world = Vec::new();
    let mut cones = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if shadow_pass && !it.shadow {
            continue;
        }
        let idx = i as u32 + 1;
        match it.prim {
            Prim::Cone { a, b, ra, rb } => {
                let r = ra.max(rb);
                let (lo, hi) = (a.min(b) - Vec3::splat(r), a.max(b) + Vec3::splat(r));
                let mut rect = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
                let mut behind = false;
                for k in 0..8 {
                    let p = Vec3::new(
                        if k & 1 == 0 { lo.x } else { hi.x },
                        if k & 2 == 0 { lo.y } else { hi.y },
                        if k & 4 == 0 { lo.z } else { hi.z },
                    );
                    let c = vp * p.extend(1.0);
                    if c.w <= 1e-3 {
                        behind = true;
                        break;
                    }
                    let (sx, sy) = ((c.x / c.w * 0.5 + 0.5) * w as f32, (0.5 - c.y / c.w * 0.5) * h as f32);
                    rect = [
                        rect[0].min(sx.floor() as i32),
                        rect[1].min(sy.floor() as i32),
                        rect[2].max(sx.ceil() as i32),
                        rect[3].max(sy.ceil() as i32),
                    ];
                }
                if behind {
                    rect = [0, 0, w as i32, h as i32];
                }
                let rect = [rect[0].max(0), rect[1].max(0), (rect[2] + 1).min(w as i32), (rect[3] + 1).min(h as i32)];
                if rect[0] < rect[2] && rect[1] < rect[3] {
                    cones.push(SCone { a, b, ra, rb, item: idx, rect });
                }
            }
            _ => {
                world.clear();
                mesh(it, idx, &mut world);
                for t in &world {
                    // Back faces never show on closed shapes (the sun's view keeps them: fewer leaks).
                    if !shadow_pass {
                        let to_eye = if cam.ortho { -cam.fwd } else { cam.eye - t.p[0] };
                        if t.n.dot(to_eye) <= 0.0 {
                            continue;
                        }
                    }
                    let clip = t.p.map(|p| vp * p.extend(1.0));
                    // Clip against the near plane (z >= 0 in clip space).
                    let mut poly: Vec<glam::Vec4> = Vec::with_capacity(4);
                    for k in 0..3 {
                        let (a, b) = (clip[k], clip[(k + 1) % 3]);
                        if a.z >= 0.0 {
                            poly.push(a);
                        }
                        if (a.z >= 0.0) != (b.z >= 0.0) {
                            poly.push(a + (b - a) * (a.z / (a.z - b.z)));
                        }
                    }
                    if poly.len() < 3 {
                        continue;
                    }
                    let s: Vec<(f32, f32, f32)> = poly
                        .iter()
                        .map(|c| {
                            let w_ = c.w.max(1e-6);
                            ((c.x / w_ * 0.5 + 0.5) * w as f32, (0.5 - c.y / w_ * 0.5) * h as f32, c.z / w_)
                        })
                        .collect();
                    for k in 1..s.len() - 1 {
                        let v = [s[0], s[k], s[k + 1]];
                        tris.push(STri { x: v.map(|q| q.0), y: v.map(|q| q.1), z: v.map(|q| q.2), n: t.n, item: t.item });
                    }
                }
            }
        }
    }
    (tris, cones)
}

/// Calls `f(x, y, z)` for every pixel centre of `t` inside rows `y0..y1`.
#[inline(always)]
fn raster(t: &STri, w: usize, y0: usize, y1: usize, mut f: impl FnMut(usize, usize, f32)) {
    let (mut x, mut y, mut z) = (t.x, t.y, t.z);
    let edge = |ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32| (bx - ax) * (py - ay) - (by - ay) * (px - ax);
    let mut area = edge(x[0], y[0], x[1], y[1], x[2], y[2]);
    if area.abs() < 1e-12 {
        return;
    }
    if area < 0.0 {
        x.swap(1, 2);
        y.swap(1, 2);
        z.swap(1, 2);
        area = -area;
    }
    let min_y = (y[0].min(y[1]).min(y[2]).floor().max(0.0) as usize).max(y0);
    let max_y = ((y[0].max(y[1]).max(y[2]).ceil()).max(0.0) as usize).min(y1);
    let min_x = x[0].min(x[1]).min(x[2]).floor().max(0.0) as usize;
    let max_x = (x[0].max(x[1]).max(x[2]).ceil().max(0.0) as usize).min(w);
    if min_x >= max_x || min_y >= max_y {
        return;
    }
    let inv = 1.0 / area;
    // Per-edge: value at the row start and step per pixel.
    let e = [(1usize, 2usize), (2, 0), (0, 1)];
    for py in min_y..max_y {
        let fy = py as f32 + 0.5;
        let fx0 = min_x as f32 + 0.5;
        let mut wv = [0.0f32; 3];
        let mut dx = [0.0f32; 3];
        // Narrow the span analytically (with a pixel of margin), then test each pixel.
        let (mut lo, mut hi) = (min_x as f32, max_x as f32);
        for (k, (a, b)) in e.iter().enumerate() {
            wv[k] = edge(x[*a], y[*a], x[*b], y[*b], fx0, fy);
            dx[k] = -(y[*b] - y[*a]);
            if dx[k] > 1e-12 {
                lo = lo.max(fx0 - wv[k] / dx[k] - 1.0);
            } else if dx[k] < -1e-12 {
                hi = hi.min(fx0 - wv[k] / dx[k] + 1.0);
            } else if wv[k] < 0.0 {
                lo = hi;
            }
        }
        let sx = (lo.max(min_x as f32).floor() as usize).max(min_x);
        let ex = (hi.min(max_x as f32).ceil() as usize).min(max_x);
        for px in sx..ex {
            let d = (px - min_x) as f32;
            let (w0, w1, w2) = (wv[0] + dx[0] * d, wv[1] + dx[1] * d, wv[2] + dx[2] * d);
            if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                f(px, py, (w0 * z[0] + w1 * z[1] + w2 * z[2]) * inv);
            }
        }
    }
}

/// Calls `f(x, y, z, normal)` for every pixel of a ray-traced cone inside rows `y0..y1`.
#[inline(always)]
fn trace(c: &SCone, cam: &Cam, vp: &Mat4, w: usize, h: usize, y0: usize, y1: usize, mut f: impl FnMut(usize, usize, f32, Vec3)) {
    let aspect = w as f32 / h as f32;
    let ys = (c.rect[1].max(0) as usize).max(y0);
    let ye = (c.rect[3].max(0) as usize).min(y1);
    for py in ys..ye {
        let v = 1.0 - (py as f32 + 0.5) / h as f32 * 2.0;
        for px in c.rect[0] as usize..c.rect[2] as usize {
            let u = (px as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let (ro, rd) = cam.ray(u, v, aspect);
            let hit =
                if c.a == c.b && c.ra == c.rb { sphere_hit(ro, rd, c.a, c.ra) } else { cone_hit(ro, rd, c.a, c.b, c.ra, c.rb) };
            if let Some((t, n)) = hit {
                let p = vp * (ro + rd * t).extend(1.0);
                if p.w > 1e-6 {
                    f(px, py, p.z / p.w, n);
                }
            }
        }
    }
}

/// Runs `f` on every job using one worker per core (jobs are handed out as workers free up).
fn par<T: Send>(jobs: Vec<T>, f: impl Fn(T) + Sync) {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 16).min(jobs.len().max(1));
    if threads <= 1 {
        jobs.into_iter().for_each(f);
        return;
    }
    let queue = std::sync::Mutex::new(jobs);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                while let Some(job) = queue.lock().ok().and_then(|mut q| q.pop()) {
                    f(job);
                }
            });
        }
    });
}

/// Rows per band: a few bands per core so uneven work evens out.
fn band_rows(rows: usize) -> usize {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 16);
    rows.div_ceil(threads * 4).max(4)
}

struct ShadowMap {
    vp: Mat4,
    size: usize,
    depth: Vec<f32>,
    /// Depth units per metre (for biasing).
    per_m: f32,
}

fn shadow_map(items: &[Item], light: &Lighting, size: usize) -> ShadowMap {
    let r = light.shadow_radius.max(4.0);
    let back = r * 2.0 + 60.0;
    let cam = Cam::new(light.shadow_center + light.sun_dir * back, -light.sun_dir, 40.0, Some(r), back * 2.0);
    let vp = cam.view_proj(1.0);
    let (tris, cones) = setup(items, &cam, size, size, true);
    let mut depth = vec![f32::INFINITY; size * size];
    let per = band_rows(size);
    let jobs: Vec<(usize, &mut [f32])> = depth.chunks_mut(per * size).enumerate().collect();
    par(jobs, |(bi, chunk)| {
        let (y0, y1) = (bi * per, bi * per + chunk.len() / size);
        for t in &tris {
            raster(t, size, y0, y1, |x, y, z| {
                let d = &mut chunk[(y - y0) * size + x];
                if z < *d {
                    *d = z;
                }
            });
        }
        for c in &cones {
            trace(c, &cam, &vp, size, size, y0, y1, |x, y, z, _| {
                let d = &mut chunk[(y - y0) * size + x];
                if z < *d {
                    *d = z;
                }
            });
        }
    });
    ShadowMap { vp, size, depth, per_m: 1.0 / (cam.far - cam.near) }
}

impl ShadowMap {
    /// 0 (in shadow) .. 1 (lit), softened over a 2x2 texel neighbourhood.
    fn lit(&self, p: Vec3, bias_m: f32) -> f32 {
        // The sun's projection is orthographic, so an affine transform is enough.
        let c = self.vp.transform_point3(p);
        let (u, v) = ((c.x * 0.5 + 0.5) * self.size as f32 - 0.5, (0.5 - c.y * 0.5) * self.size as f32 - 0.5);
        if u < 0.0 || v < 0.0 || u >= (self.size - 1) as f32 || v >= (self.size - 1) as f32 {
            return 1.0;
        }
        let z = c.z - bias_m * self.per_m;
        let (x0, y0) = (u as usize, v as usize);
        let (fx, fy) = (u - x0 as f32, v - y0 as f32);
        let i = y0 * self.size + x0;
        let at = |j: usize| if z <= self.depth[j] { 1.0 } else { 0.0 };
        let top = at(i) * (1.0 - fx) + at(i + 1) * fx;
        let bottom = at(i + self.size) * (1.0 - fx) + at(i + self.size + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// Per-pixel scratch buffers, kept between frames (the window renders 60 times a second).
#[derive(Default)]
struct Buffers {
    depth: Vec<f32>,
    normal: Vec<[f32; 3]>,
    ids: Vec<u32>,
    lin: Vec<f32>,
}

thread_local! {
    static BUFFERS: std::cell::RefCell<Buffers> = std::cell::RefCell::new(Buffers::default());
}

/// Renders a scene. `ssaa` > 1 renders that many times larger and averages (smoother edges).
pub fn render(scene: &Scene, width: usize, height: usize, ssaa: usize) -> Image {
    let k = ssaa.clamp(1, 3);
    let (w, h) = (width.max(1) * k, height.max(1) * k);
    let mut img = BUFFERS.with(|b| render_into(scene, w, h, &mut b.borrow_mut()));
    if k > 1 {
        img = downsample(&img, k);
    }
    for o in &scene.overlay {
        match o {
            Overlay::Text { x, y, scale, color, text } => img.text(*x, *y, *scale, *color, text),
            Overlay::Rect { x, y, w, h, color, alpha } => img.rect(*x, *y, *w, *h, *color, *alpha),
        }
    }
    img
}

fn render_into(scene: &Scene, w: usize, h: usize, buf: &mut Buffers) -> Image {
    let cam = &scene.cam;
    let light = &scene.light;
    let items = &scene.items;
    let aspect = w as f32 / h as f32;
    let vp = cam.view_proj(aspect);
    let shadows = (light.shadows && items.iter().any(|i| i.shadow)).then(|| shadow_map(items, light, 1024));
    let (tris, cones) = setup(items, cam, w, h, false);
    let n = w * h;
    buf.depth.resize(n, f32::INFINITY);
    buf.normal.resize(n, [0.0; 3]);
    buf.ids.resize(n, 0);
    buf.lin.resize(n, 0.0);
    let mut color = vec![0u32; n];
    let enc = Encoder::new();
    let per = band_rows(h);
    let (sky, horizon) = (light.sky.vec(), light.horizon.vec());
    let sun = light.sun.vec();
    let sun_dir = light.sun_dir.normalize_or(Vec3::Y);
    let fog = light.fog;
    // Depth back to distance along the view direction (DirectX-style 0..1 depth).
    let (near, far) = (cam.near, cam.far);
    let (pa, pb) = (far / (near - far), near * far / (near - far));
    let (tx, ty) = (cam.half * aspect, cam.half);

    let jobs: Vec<_> = buf
        .depth
        .chunks_mut(per * w)
        .zip(buf.normal.chunks_mut(per * w))
        .zip(buf.ids.chunks_mut(per * w))
        .zip(buf.lin.chunks_mut(per * w))
        .zip(color.chunks_mut(per * w))
        .enumerate()
        .collect();
    par(jobs, |(bi, ((((dep, nor), idb), lnb), col))| {
        let (y0, y1) = (bi * per, bi * per + dep.len() / w);
        dep.fill(f32::INFINITY);
        idb.fill(0);
        // G-buffer: nearest depth, its normal and item.
        for t in &tris {
            raster(t, w, y0, y1, |x, y, z| {
                let i = (y - y0) * w + x;
                if z >= 0.0 && z < dep[i] {
                    dep[i] = z;
                    nor[i] = t.n.to_array();
                    idb[i] = t.item;
                }
            });
        }
        for c in &cones {
            trace(c, cam, &vp, w, h, y0, y1, |x, y, z, n| {
                let i = (y - y0) * w + x;
                if z >= 0.0 && z < dep[i] {
                    dep[i] = z;
                    nor[i] = n.to_array();
                    idb[i] = c.item;
                }
            });
        }
        // Shading.
        for y in y0..y1 {
            let v = 1.0 - (y as f32 + 0.5) / h as f32 * 2.0;
            let row = (y - y0) * w;
            for x in 0..w {
                let i = row + x;
                let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
                // The view ray, scaled so its forward component is 1.
                let side = cam.right * (u * tx) + cam.up * (v * ty);
                if idb[i] == 0 {
                    let dy = if cam.ortho { cam.fwd.y } else { (cam.fwd + side).normalize().y };
                    let t = (dy * 1.6 + 0.2).clamp(0.0, 1.0);
                    col[i] = enc.pack(horizon.lerp(sky, t));
                    continue;
                }
                let z = dep[i];
                let (p, dist) = if cam.ortho {
                    let d = near + z * (far - near);
                    (cam.eye + side + cam.fwd * d, d)
                } else {
                    let d = pb / (z + pa);
                    (cam.eye + (cam.fwd + side) * d, d)
                };
                let it = &items[idb[i] as usize - 1];
                let mut nrm = Vec3::from_array(nor[i]);
                let view_dir = if cam.ortho { cam.fwd } else { cam.fwd + side };
                if nrm.dot(view_dir) > 0.0 {
                    nrm = -nrm;
                }
                let base = it.color.vec();
                let ndl = nrm.dot(sun_dir).max(0.0);
                let sh = match &shadows {
                    Some(sm) if it.look != Look::Glow && ndl > 0.0 => sm.lit(p + nrm * 0.04, 0.03 + 0.12 * (1.0 - ndl)),
                    _ => 1.0,
                };
                let amb = (horizon * 0.55).lerp(sky, nrm.y * 0.5 + 0.5) * light.ambient;
                let mut c = match it.look {
                    Look::Cel => {
                        let l = ndl * sh;
                        let band = if l > 0.22 {
                            1.0
                        } else if l > 0.08 {
                            0.55
                        } else {
                            0.0
                        };
                        base * (amb + sun * band * 0.9)
                    }
                    Look::Lit => base * (amb + sun * ndl * sh),
                    Look::Flat => base * (0.6 + 0.4 * sh) * (0.65 + 0.35 * light.ambient.min(1.5)),
                    Look::Glow => base * 1.05,
                };
                if fog > 0.0 {
                    let f = ((p - cam.eye).length() / fog).clamp(0.0, 1.0);
                    c = c.lerp(horizon, f * f);
                }
                lnb[i] = dist;
                col[i] = enc.pack(c);
            }
        }
    });

    if light.outlines {
        // Darken pixels on the near side of a depth jump (silhouettes, overlaps).
        let (ids, lin) = (&buf.ids, &buf.lin);
        let jobs: Vec<_> = color.chunks_mut(per * w).enumerate().collect();
        par(jobs, |(bi, out)| {
            let y0 = bi * per;
            for (r, row) in out.chunks_mut(w).enumerate() {
                let y = y0 + r;
                for (x, px) in row.iter_mut().enumerate() {
                    let i = y * w + x;
                    let id = ids[i];
                    if id == 0 {
                        continue;
                    }
                    let d = lin[i];
                    let far = |j: usize| ids[j] != id && (ids[j] == 0 || lin[j] > d * 1.02 + 0.06);
                    if (x > 0 && far(i - 1)) || (x + 1 < w && far(i + 1)) || (y > 0 && far(i - w)) || (y + 1 < h && far(i + w)) {
                        let c = *px;
                        let dark = |sh: u32| (((c >> sh) & 255) * 30 / 100) << sh;
                        *px = dark(16) | dark(8) | dark(0);
                    }
                }
            }
        });
    }
    Image { width: w, height: h, px: color }
}

fn downsample(img: &Image, k: usize) -> Image {
    let (w, h) = (img.width / k, img.height / k);
    let mut out = Image::new(w, h, 0);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 3];
            for dy in 0..k {
                for dx in 0..k {
                    let p = img.px[(y * k + dy) * img.width + x * k + dx];
                    acc[0] += (p >> 16) & 255;
                    acc[1] += (p >> 8) & 255;
                    acc[2] += p & 255;
                }
            }
            let n = (k * k) as u32;
            out.px[y * w + x] = ((acc[0] / n) << 16) | ((acc[1] / n) << 8) | (acc[2] / n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(items: Vec<Item>) -> Scene {
        Scene {
            cam: Cam::new(Vec3::new(0.0, 6.0, 8.0), Vec3::new(0.0, -6.0, -8.0), 40.0, None, 100.0),
            light: Lighting {
                sky: Color::hex("#7fb2e5"),
                horizon: Color::hex("#dfe9f2"),
                sun_dir: Vec3::new(0.3, 1.0, 0.2).normalize(),
                sun: Color::WHITE,
                ambient: 0.4,
                fog: 0.0,
                shadows: true,
                outlines: true,
                shadow_center: Vec3::ZERO,
                shadow_radius: 10.0,
            },
            items,
            overlay: vec![Overlay::Text { x: 2, y: 2, scale: 1.0, color: Color::WHITE, text: "Hi".into() }],
        }
    }

    #[test]
    fn draws_boxes_spheres_and_shadows() {
        let red = Color::hex("#ff0000");
        let items = vec![
            Item {
                prim: Prim::Box { center: Vec3::new(0.0, -0.5, 0.0), rot: Quat::IDENTITY, half: Vec3::new(5.0, 0.5, 5.0) },
                color: Color::WHITE,
                look: Look::Lit,
                shadow: true,
            },
            Item { prim: Prim::Cone { a: Vec3::Y, b: Vec3::Y, ra: 1.0, rb: 1.0 }, color: red, look: Look::Cel, shadow: true },
        ];
        let img = render(&scene(items), 160, 90, 1);
        assert_eq!(img.px.len(), 160 * 90);
        // The middle is the red ball.
        let mid = img.px[45 * 160 + 80];
        assert!(mid >> 16 & 255 > 100 && mid >> 8 & 255 < 40, "{mid:06x}");
        // Some floor pixels are lit, some shadowed (darker).
        let floor: Vec<u32> = (0..160).map(|x| img.px[85 * 160 + x] & 255).collect();
        assert!(floor.iter().max().unwrap() > &150);
        let png = img.png();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn cone_hits_its_sides_and_caps() {
        let (pa, pb) = (Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0));
        let hit = cone_hit(Vec3::new(-5.0, 1.0, 0.0), Vec3::X, pa, pb, 0.5, 0.25).unwrap();
        assert!((hit.0 - (5.0 - 0.375)).abs() < 0.02, "{hit:?}");
        let cap = cone_hit(Vec3::new(0.0, -5.0, 0.0), Vec3::Y, pa, pb, 0.5, 0.25).unwrap();
        assert!((cap.0 - 4.5).abs() < 1e-3 && cap.1.y < -0.99, "{cap:?}");
        assert!(cone_hit(Vec3::new(-5.0, 5.0, 0.0), Vec3::X, pa, pb, 0.5, 0.25).is_none());
    }
}
