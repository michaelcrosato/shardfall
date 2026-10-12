//! The GPU packing and the CPU reference evaluator (asset plan §6.2-6.5).
//!
//! `Material::pack` writes the storage-buffer row the renderer's uber-shader reads
//! (`GpuMaterial`, 368 bytes: a header and three 112-byte `GpuLayer`s). `eval_packed` is a
//! line-for-line port of `eval_material` in the prototype kernel library
//! (`docs/asset_plan/prototypes/materials/materials.wgsl`) and of the prototype harness's CPU
//! mirror: the same PCG hash and constants, the same f32 operations in the same order, and
//! the same parameters, because it reads the packed row exactly as the shader does. Keep the
//! two in step: the renderer's parity test compares them pixel by pixel.
//!
//! Where it differs from the prototype WGSL (the renderer's copy must match):
//! - voronoi spots: the feature is the spot's diameter, `2·b.x·a.x` (precheck and
//!   `pat_cells`), and the cellular cavity mean divides by that feature;
//! - a layer with cells fades to its cells' mean linear colour, packed in `b.yzw`: an
//!   unresolved layer sets `c = b.yzw` before its cavity mean, and a resolved one sets
//!   `c = mix(b.yzw, cell colour, k)` instead of `mix(c, cell colour, k)`;
//! - no shingles flip (axis code 4) and no debug kernels (11, 12);
//! - `turn` also turns 1D directions, but in the packer (`dir`), so the kernel is unchanged.
//!
//! Anti-aliasing uses an analytic footprint instead of derivatives: lines are box-filtered
//! pulses, a layer fades to its mean between 3 and 1 pixels per feature and is skipped below
//! one, and fractal noise drops the octaves under a pixel.
//!
//! All lengths are metres.

use std::f32::consts::PI;

use glam::{Vec2, Vec3};

use super::resolve::Material;
use super::schema::{LayerDoc, Param};
use crate::color::Color;
use crate::props::compile::DEFAULT_ROLES;
use crate::props::doc::{MapMode, Pattern};
use crate::shape::Look;

/// u32 words in one packed layer (7 vec4).
pub const LAYER_WORDS: usize = 28;
/// u32 words in one packed material: a header, the PBR row and three layers.
pub const MATERIAL_WORDS: usize = 8 + 3 * LAYER_WORDS;
/// Bytes in one packed material (the storage-buffer stride).
pub const MATERIAL_BYTES: usize = MATERIAL_WORDS * 4;

/// Kernel ids in `GpuLayer::head[0] & 0xff` (the 14 patterns share 10 kernels).
pub mod kernel {
    pub const NONE: u32 = 0;
    pub const NOISE: u32 = 1;
    /// voronoi, cobbles (variant: 0 spots, 1 edges, 2 cells).
    pub const CELLS: u32 = 2;
    /// brick, tiles (variant 0), planks (variant 1: random end joints).
    pub const GRID: u32 = 3;
    pub const CHECKER: u32 = 4;
    pub const GRADIENT: u32 = 5;
    /// wave, stripes (variant 1: rings).
    pub const BANDS: u32 = 6;
    pub const WOOD: u32 = 7;
    pub const MAGIC: u32 = 8;
    pub const DOTS: u32 = 9;
    /// variant 0 round, 1 square, 2 point.
    pub const SHINGLES: u32 = 10;
}

/// One pattern layer, laid out as `MatLayer` in the WGSL library.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuLayer {
    /// x: kernel | variant<<8 | axis<<12 | facing<<16 | mask<<20 | paint<<24 | ncells<<28;
    /// y: salt.
    pub head: [u32; 4],
    /// rgb: linear colour (paint 0), or r: linear shade factor (paint 1); a: strength.
    pub paint: [f32; 4],
    pub a: [f32; 4],
    /// x: the kernel's; yzw: the cells' mean linear colour when the layer has cells.
    pub b: [f32; 4],
    /// x vary, y cavity, z glow, w bevel.
    pub c: [f32; 4],
    /// Up to 4 sRGB8 colours, r in the low byte (`unpack4x8unorm`).
    pub cells: [u32; 4],
    /// xyz: unit axis for 1D patterns.
    pub dir: [f32; 4],
}

/// One material row, laid out as `Material` in the WGSL library.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuMaterial {
    /// x layer count, y map (0 object, 1 world, 2 axis), z bit 0: turn.
    pub head: [u32; 4],
    /// x roughness (< 0 = legacy shading), y metallic, z 1/scale.
    pub pbr: [f32; 4],
    pub layers: [GpuLayer; 3],
}

impl GpuLayer {
    fn words(&self, out: &mut Vec<u32>) {
        let bits = |v: [f32; 4]| v.map(f32::to_bits);
        for row in [self.head, bits(self.paint), bits(self.a), bits(self.b), bits(self.c), self.cells, bits(self.dir)] {
            out.extend(row);
        }
    }
}

impl GpuMaterial {
    /// The row as the GPU reads it (f32 as their bits), ready to upload.
    pub fn words(&self) -> [u32; MATERIAL_WORDS] {
        let mut out = Vec::with_capacity(MATERIAL_WORDS);
        out.extend(self.head);
        out.extend(self.pbr.map(f32::to_bits));
        for layer in &self.layers {
            layer.words(&mut out);
        }
        out.try_into().expect("a packed material is 92 words")
    }
}

/// A material ready to draw: what travels in the instance (base colour, emissive, look) and
/// the row in the material table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packed {
    /// Linear RGB.
    pub base: Vec3,
    pub emissive: f32,
    pub look: Look,
    pub gpu: GpuMaterial,
}

impl Packed {
    pub fn eval(&self, sample: &Sample) -> Shaded {
        eval_packed(&self.gpu, self.base, sample)
    }
}

/// Where a material is evaluated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Part-local metric position and normal (object and axis maps).
    pub local: Vec3,
    pub local_normal: Vec3,
    /// World position and normal (world map; `facing` always uses the world normal).
    pub world: Vec3,
    pub world_normal: Vec3,
    /// The part's half extents (axis map radius, gradients from the part's bottom).
    pub half_extents: Vec3,
    /// Per-instance seed (20 bits used): hash(entity, part) for object and axis maps, 0 for
    /// world maps.
    pub seed: u32,
    /// Metres per pixel on the surface (see `footprint`).
    pub footprint: f32,
}

impl Sample {
    /// A sample whose local frame is the world's (a world-mapped surface, or a test).
    pub fn at(p: Vec3, n: Vec3, footprint: f32) -> Self {
        Sample { local: p, local_normal: n, world: p, world_normal: n, half_extents: Vec3::splat(0.5), seed: 0, footprint }
    }
}

/// The evaluated surface: linear albedo and the layers' glow (the instance's emissive adds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shaded {
    pub rgb: Vec3,
    pub glow: f32,
}

/// Metres per pixel on a surface, as the kernel computes it. `pixel` is `Globals.params.w`,
/// `2 / (proj[1][1] · height_px)`; `eye` is the camera position (`None` for orthographic).
pub fn footprint(pixel: f32, eye: Option<Vec3>, forward: Vec3, world: Vec3, world_normal: Vec3) -> f32 {
    let (dist, vdir) = match eye {
        Some(eye) => {
            let to_eye = eye - world;
            (to_eye.length(), to_eye / to_eye.length().max(1e-4))
        }
        None => (1.0, -forward),
    };
    pixel * dist / world_normal.dot(vdir).abs().max(0.25)
}

// --- Defaults (§6.2, metres) ------------------------------------------------------------------

/// A pattern's default size `[w,h]` (one number: both).
pub fn default_size(pattern: Pattern) -> [f32; 2] {
    use Pattern::*;
    let s = match pattern {
        Brick => return [0.4, 0.2],
        Planks => return [1.2, 0.2],
        Shingles => return [0.3, 0.2],
        Noise | Checker | Magic | Tiles => 0.5,
        Voronoi | Stripes => 0.3,
        Cobbles => 0.25,
        Wave => 0.2,
        Dots => 0.15,
        Wood => 0.08,
        Gradient => 1.0,
    };
    [s, s]
}

/// A pattern's default gap, for the patterns that paint one.
pub fn default_gap(pattern: Pattern) -> Option<f32> {
    use Pattern::*;
    Some(match pattern {
        Brick | Cobbles => 0.03,
        Planks | Shingles => 0.015,
        Tiles | Voronoi => 0.02,
        _ => return None,
    })
}

/// A dot's default radius.
pub fn default_radius() -> f32 {
    0.04
}

// --- Packing ------------------------------------------------------------------------------------

impl Material {
    /// Packs the material for drawing with `map` (the material's own, or the part's auto
    /// choice) and the theme `roles` (`$wall` → `#rrggbb`; `DEFAULT_ROLES` fills any gaps).
    pub fn pack(&self, map: MapMode, roles: &[(&str, &str)]) -> Packed {
        let mut gpu = GpuMaterial::default();
        let count = self.layers.len().min(3);
        gpu.head = [count as u32, map_code(map), self.turn as u32, 0];
        let roughness = self.roughness.unwrap_or(if self.metallic > 0.0 { 0.5 } else { -1.0 });
        gpu.pbr = [roughness, self.metallic, 1.0 / self.scale, 0.0];
        for (index, layer) in self.layers.iter().take(3).enumerate() {
            gpu.layers[index] = pack_layer(index, layer, self.turn, roles);
        }
        Packed { base: linear(&self.color, roles), emissive: self.emissive, look: self.look, gpu }
    }
}

fn map_code(map: MapMode) -> u32 {
    match map {
        MapMode::Object => 0,
        MapMode::World => 1,
        MapMode::Axis => 2,
    }
}

/// `#rrggbb` or a role, as sRGB bytes; magenta when unknown, so mistakes show.
fn srgb8(color: &str, roles: &[(&str, &str)]) -> [u8; 3] {
    let hex = if color.starts_with('$') {
        roles.iter().chain(DEFAULT_ROLES).find(|(role, _)| *role == color).map_or("#ff00ff", |(_, hex)| *hex)
    } else {
        color
    };
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xff00ff);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn linear(color: &str, roles: &[(&str, &str)]) -> Vec3 {
    let [r, g, b] = srgb8(color, roles);
    Vec3::from_array(Color::srgb8(r, g, b).0)
}

/// RGBA8 for `unpack4x8unorm`: r in the low byte, alpha 255.
fn rgba8(color: &str, roles: &[(&str, &str)]) -> u32 {
    let [r, g, b] = srgb8(color, roles);
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xff << 24
}

fn pack_layer(index: usize, l: &LayerDoc, turn: bool, roles: &[(&str, &str)]) -> GpuLayer {
    use Pattern::*;
    let mut o = GpuLayer::default();
    let mut variant = 0u32;
    let mut axis_default = "x";
    // Cellular defaults: vary, cavity, bevel.
    let mut cellular = (0.0f32, 0.0f32, 0.0f32);
    let size = default_size(l.pattern);
    let kind = match l.pattern {
        Noise => {
            let detail = l.int("detail", 3).clamp(1, 4);
            let cover = l.num("cover", 0.5);
            o.a = [l.num("size", size[0]), detail as f32, l.num("distort", 0.0), cover];
            o.b = [lookup(&NOISE_COVER[detail as usize - 1], cover), l.num("soft", 0.3) * 0.08 + 0.002, 0.0, 0.0];
            kernel::NOISE
        }
        Voronoi | Cobbles => {
            let cob = l.pattern == Cobbles;
            variant = match l.word("feature").unwrap_or(if cob { "edges" } else { "spots" }) {
                "spots" => 0,
                "edges" => 1,
                _ => 2,
            };
            let size = l.num("size", size[0]);
            let cover = l.num("cover", 0.3);
            let gap = l.num("gap", default_gap(l.pattern).unwrap_or(0.0));
            o.a = [size, l.num("jitter", if cob { 0.8 } else { 1.0 }), gap, cover];
            o.b = [(cover / PI).sqrt(), 0.0, 0.0, 0.0];
            cellular = if cob { (0.2, 0.5, 0.3 * size) } else { (0.0, 0.0, 0.03) };
            kernel::CELLS
        }
        Brick | Planks | Tiles => {
            let (offset, vary, cavity) = match l.pattern {
                Brick => (0.5, 0.15, 0.3),
                Planks => (0.0, 0.2, 0.2),
                _ => (0.0, 0.1, 0.2),
            };
            variant = (l.pattern == Planks) as u32;
            let s = l.size2(size);
            let gap = l.num("gap", default_gap(l.pattern).unwrap_or(0.0));
            o.a = [s[0], s[1], gap, l.num("offset", offset)];
            cellular = (vary, cavity, gap.max(0.02));
            kernel::GRID
        }
        Checker => {
            o.a = [l.num("size", size[0]), 0.0, 0.0, 0.0];
            kernel::CHECKER
        }
        Gradient => {
            axis_default = "y";
            o.a = [l.num("from", 0.0), l.num("to", 1.0), 0.0, 0.0];
            kernel::GRADIENT
        }
        Wave | Stripes => {
            let stripes = l.pattern == Stripes;
            variant = l.flag("rings") as u32;
            o.a = [l.num("size", size[0]), l.num("cover", 0.5), l.num("distort", 0.0), 0.0];
            o.b = [l.num("soft", if stripes { 0.0 } else { 0.5 }), 0.0, 0.0, 0.0];
            kernel::BANDS
        }
        Wood => {
            axis_default = "y";
            o.a = [l.num("size", size[0]), l.num("cover", 0.35), l.num("distort", 1.5), 0.0];
            o.b = [l.num("soft", 0.4), 0.0, 0.0, 0.0];
            kernel::WOOD
        }
        Magic => {
            let depth = l.int("depth", 2).clamp(1, 4);
            let cover = l.num("cover", 0.5);
            o.a = [l.num("size", size[0]), depth as f32, l.num("distort", 1.0), cover];
            o.b = [lookup(&MAGIC_COVER[depth as usize - 1], cover), l.num("soft", 0.3) * 0.1 + 0.002, 0.0, 0.0];
            kernel::MAGIC
        }
        Dots => {
            o.a = [l.num("size", size[0]), l.num("radius", default_radius()), l.num("offset", 0.5), l.num("jitter", 0.0)];
            kernel::DOTS
        }
        Shingles => {
            variant = match l.word("shape").unwrap_or("round") {
                "round" => 0,
                "square" => 1,
                _ => 2,
            };
            let s = l.size2(size);
            o.a = [s[0], s[1], l.num("gap", default_gap(l.pattern).unwrap_or(0.0)), 0.5];
            cellular = (0.15, 0.4, 0.05);
            kernel::SHINGLES
        }
    };
    let (axis, dir) = match l.axis() {
        Some(Param::Vector(v)) => (3, v.normalize()),
        Some(Param::Word(word)) => named_axis(word),
        _ => named_axis(axis_default),
    };
    // `turn` turns the pattern 90 degrees about local z: 2D patterns swap u and v in the
    // kernel; the direction of 1D patterns (bands, wood, gradient) turns here, x to y.
    let dir = if turn { Vec3::new(-dir.y, dir.x, dir.z) } else { dir };
    o.dir = dir.extend(0.0).to_array();
    let facing = match l.word("facing") {
        Some("up") => 1,
        Some("down") => 2,
        Some("side") => 3,
        _ => 0,
    };
    let mask = match l.word("masked_by") {
        Some("prev") => 1,
        Some("not_prev") => 2,
        _ => 0,
    };
    let cells = l.cells();
    for (k, cell) in cells.iter().take(4).enumerate() {
        o.cells[k] = rgba8(cell, roles);
    }
    if !cells.is_empty() {
        // The cells' mean linear colour: what the layer fades to when its cells are too small.
        let n = cells.len().min(4);
        let sum = o.cells[..n].iter().fold(Vec3::ZERO, |sum, cell| sum + srgb8_to_linear(*cell));
        let mean = sum / n as f32;
        o.b[1..4].copy_from_slice(&mean.to_array());
    }
    let strength = l.num("strength", 1.0);
    let paint = match l.color() {
        Some(color) => {
            o.paint = linear(color, roles).extend(strength).to_array();
            0
        }
        None => {
            o.paint = [l.num("shade", 0.6).powf(2.2), 0.0, 0.0, strength];
            1
        }
    };
    o.c = [l.num("vary", cellular.0), l.num("cavity", cellular.1), l.num("glow", 0.0), l.num("bevel", cellular.2)];
    let ncells = cells.len().min(4) as u32;
    o.head = [
        kind | variant << 8 | axis << 12 | facing << 16 | mask << 20 | paint << 24 | ncells << 28,
        pcg((index as u32).wrapping_mul(7919).wrapping_add(l.int("seed", 0))),
        0,
        0,
    ];
    o
}

fn named_axis(word: &str) -> (u32, Vec3) {
    match word {
        "x" => (0, Vec3::X),
        "y" => (1, Vec3::Y),
        _ => (2, Vec3::Z),
    }
}

// --- `cover` quantile tables ------------------------------------------------------------------

/// Noise `cover` thresholds per detail 1-4: entry i is the value t with P(fbm > t) = i/20.
/// Generated by `cover_tables` (the test `cover_tables_regenerate` checks them).
#[rustfmt::skip]
pub const NOISE_COVER: [[f32; 21]; 4] = [
    [0.9976312, 0.8049808, 0.74588335, 0.70148057, 0.66573185, 0.6329692, 0.6037633, 0.5760657, 0.5501095, 0.5247667, 0.49986655, 0.47505513, 0.4497515, 0.42360196, 0.39573437, 0.36639732, 0.33462402, 0.29870528, 0.25412998, 0.19306904, 0.0050150156],
    [0.9556841, 0.72622377, 0.68040806, 0.64772636, 0.62117803, 0.59741735, 0.5759751, 0.5557916, 0.5369224, 0.5183652, 0.5001497, 0.48203257, 0.46367836, 0.44466934, 0.42471242, 0.40323642, 0.37973213, 0.35255554, 0.31898475, 0.27234185, 0.044078905],
    [0.9245526, 0.6986529, 0.65823495, 0.62936354, 0.6056375, 0.5847961, 0.5662635, 0.54887855, 0.5321998, 0.51613575, 0.5003529, 0.48420718, 0.4680658, 0.45154434, 0.4342416, 0.41525498, 0.39491296, 0.371008, 0.34134525, 0.30012873, 0.057967458],
    [0.8962738, 0.6867091, 0.64867884, 0.62117654, 0.59909385, 0.5798276, 0.5623007, 0.5459134, 0.5301536, 0.51504195, 0.500064, 0.48531735, 0.4700902, 0.45457405, 0.4383245, 0.4209545, 0.4014993, 0.3788787, 0.35130328, 0.3123848, 0.090390906],
];
/// Magic `cover` thresholds per depth 1-4 (distort 1), as `NOISE_COVER`.
#[rustfmt::skip]
pub const MAGIC_COVER: [[f32; 21]; 4] = [
    [1.0, 0.99376535, 0.97508645, 0.9450542, 0.904055, 0.8526144, 0.7925614, 0.72472465, 0.6520756, 0.5758704, 0.49813503, 0.42028043, 0.34589633, 0.2737062, 0.20698583, 0.14673927, 0.09535897, 0.05451691, 0.02433914, 0.006010741, 0.0],
    [0.9751998, 0.7981832, 0.7018477, 0.6025077, 0.51191807, 0.43445265, 0.37500456, 0.3218882, 0.2775082, 0.23933747, 0.20479277, 0.16965541, 0.14222223, 0.114777386, 0.0898239, 0.06706691, 0.045781374, 0.027724892, 0.013866901, 0.0046052337, 0.0],
    [0.9751998, 0.7981832, 0.7018477, 0.6025077, 0.51191807, 0.43445265, 0.37500456, 0.3218882, 0.2775082, 0.23933747, 0.20479277, 0.16965541, 0.14222223, 0.114777386, 0.0898239, 0.06706691, 0.045781374, 0.027724892, 0.013866901, 0.0046052337, 0.0],
    [1.0, 0.9847772, 0.9523855, 0.919685, 0.9018544, 0.87796354, 0.848596, 0.81466675, 0.77561796, 0.7241497, 0.66318893, 0.5942563, 0.52190495, 0.4468752, 0.38140544, 0.32798624, 0.27905872, 0.23907831, 0.1967392, 0.14422041, 0.08976483],
];

/// The threshold for `cover` (0-1), interpolated between table entries.
pub fn lookup(table: &[f32; 21], cover: f32) -> f32 {
    let x = cover.clamp(0.0, 1.0) * 20.0;
    let i = (x.floor() as usize).min(19);
    mix(table[i], table[i + 1], x - i as f32)
}

/// Recomputes `(NOISE_COVER, MAGIC_COVER)` from 200,000 samples per entry with the reference
/// noise, as the prototype did.
pub fn cover_tables() -> ([[f32; 21]; 4], [[f32; 21]; 4]) {
    let mut noise = [[0.0; 21]; 4];
    let mut magics = [[0.0; 21]; 4];
    for d in 1..=4u32 {
        noise[d as usize - 1] = quantiles(|i| fbm(sample_point(i), d, 1.0, 0.0, 0));
        magics[d as usize - 1] = quantiles(|i| magic(sample_point(i) * 0.3, d, 1.0));
    }
    (noise, magics)
}

fn quantiles(sample: impl FnMut(u32) -> f32) -> [f32; 21] {
    let mut v: Vec<f32> = (0..200_000u32).map(sample).collect();
    v.sort_by(|a, b| a.total_cmp(b));
    let mut t = [0.0; 21];
    for (i, x) in t.iter_mut().enumerate() {
        let cover = i as f32 / 20.0;
        let q = ((1.0 - cover) * (v.len() - 1) as f32) as usize;
        *x = v[q];
    }
    t
}

fn sample_point(i: u32) -> Vec3 {
    let h = pcg(i.wrapping_mul(3).wrapping_add(11));
    Vec3::new(rnd(h) * 97.0, rnd(pcg(h)) * 97.0, rnd(pcg(h ^ 0xabc)) * 97.0)
}

// --- The kernels (materials.wgsl) -------------------------------------------------------------

/// The kernels' π for the dots' mean coverage, as written in the WGSL.
#[allow(clippy::approx_constant)]
const PI_WGSL: f32 = 3.14159;
// u32 arithmetic wraps as WGSL's does; f32 → i32 saturates in both.

pub fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277803737);
    (w >> 22) ^ w
}

fn hash3i(x: i32, y: i32, z: i32, salt: u32) -> u32 {
    pcg((x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841) ^ (z as u32).wrapping_mul(0xcb1ab31f) ^ salt)
}

fn rnd_lo(h: u32) -> f32 {
    (h & 0xffff) as f32 * (1.0 / 65536.0)
}

fn rnd(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16777216.0)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// WGSL's `fract`: x - floor(x) (Rust's `f32::fract` truncates toward zero).
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// WGSL's `step(edge, x)`.
fn step(edge: f32, x: f32) -> f32 {
    if edge <= x { 1.0 } else { 0.0 }
}

fn vnoise(p: Vec3, salt: u32) -> f32 {
    let fl = p.floor();
    let (ix, iy, iz) = (fl.x as i32, fl.y as i32, fl.z as i32);
    let f = p - fl;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let h = |dx: i32, dy: i32, dz: i32| rnd(hash3i(ix.wrapping_add(dx), iy.wrapping_add(dy), iz.wrapping_add(dz), salt));
    let a = mix(h(0, 0, 0), h(1, 0, 0), u.x);
    let b = mix(h(0, 1, 0), h(1, 1, 0), u.x);
    let c = mix(h(0, 0, 1), h(1, 0, 1), u.x);
    let d = mix(h(0, 1, 1), h(1, 1, 1), u.x);
    mix(mix(a, b, u.y), mix(c, d, u.y), u.z)
}

/// Fractal noise in [0,1]; octaves smaller than ~2 pixels fade to their mean (0.5).
fn fbm(p: Vec3, octaves: u32, size: f32, fw: f32, salt: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q, mut s) = (0.0, 0.5, 0.0, p, size);
    for o in 0..octaves {
        let k = smoothstep(1.0, 3.0, s / fw.max(1e-5));
        if k <= 0.0 {
            // This and all finer octaves are below a pixel: they average to 0.5.
            for _ in o..octaves {
                sum += amp * 0.5;
                norm += amp;
                amp *= 0.5;
            }
            break;
        }
        sum += amp * mix(0.5, vnoise(q, salt.wrapping_add(o.wrapping_mul(7))), k);
        norm += amp;
        amp *= 0.5;
        q *= 2.03;
        s /= 2.03;
    }
    sum / norm
}

/// Box-filtered coverage of a band of width w centred at d = 0, seen through a pixel of width fw.
fn pulse(d: f32, w: f32, fw: f32) -> f32 {
    let f = fw.max(1e-5);
    let lo = (d - 0.5 * f).max(-0.5 * w);
    let hi = (d + 0.5 * f).min(0.5 * w);
    ((hi - lo) / f).clamp(0.0, 1.0)
}

fn fade(feature: f32, fw: f32) -> f32 {
    smoothstep(1.0, 3.0, feature / fw.max(1e-5))
}

fn srgb8_to_linear(v: u32) -> Vec3 {
    let channel = |byte: u32| {
        let c = (byte & 0xff) as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Vec3::new(channel(v), channel(v >> 8), channel(v >> 16))
}

/// 2D coordinates on the dominant plane, with the layer's long axis along u.
fn plane_uv(p: Vec3, n: Vec3, axis: u32) -> Vec2 {
    let a = n.abs();
    let mut vaxis = 1;
    let mut uv = if a.x >= a.y && a.x >= a.z {
        Vec2::new(p.z, p.y)
    } else if a.y >= a.z {
        vaxis = 2;
        Vec2::new(p.x, p.z)
    } else {
        Vec2::new(p.x, p.y)
    };
    if axis == vaxis {
        uv = Vec2::new(uv.y, uv.x);
    }
    uv
}

#[derive(Clone, Copy, Debug)]
struct Pat {
    /// Paint mask (anti-aliased).
    m: f32,
    /// Mask when unresolved.
    mean: f32,
    /// Feature size for fading.
    feat: f32,
    /// Per-cell hash (0 = not cellular).
    cell: u32,
    /// Distance to the nearest cell border, for cavity.
    d: f32,
    gap: f32,
}

fn pat_grid(uv: Vec2, l: &GpuLayer, fw: f32, salt: u32, random_offset: bool) -> Pat {
    let (w, h, gap) = (l.a[0], l.a[1], l.a[2]);
    let row = (uv.y / h).floor();
    let mut off = l.a[3] * w * (row - 2.0 * (row * 0.5).floor());
    if random_offset {
        off = rnd(hash3i(row as i32, 7, 0, salt)) * w;
    }
    let x = (uv.x - off) / w;
    let col = x.floor();
    let fu = x - col;
    let fv = uv.y / h - row;
    let du = fu.min(1.0 - fu) * w;
    let dv = fv.min(1.0 - fv) * h;
    Pat {
        m: pulse(du, gap, fw).max(pulse(dv, gap, fw)),
        mean: 1.0 - (1.0 - (gap / w).min(1.0)) * (1.0 - (gap / h).min(1.0)),
        feat: w.min(h),
        cell: hash3i(col as i32, row as i32, 1, salt) | 1,
        d: du.min(dv),
        gap,
    }
}

fn pat_cells(uv: Vec2, l: &GpuLayer, variant: u32, fw: f32, salt: u32) -> Pat {
    let (size, jit, gap) = (l.a[0], l.a[1], l.a[2]);
    let q = uv / size;
    let ic = q.floor();
    let (mut d1, mut d2) = (1e9f32, 1e9f32);
    let mut bid = (0i32, 0i32);
    for j in -1..=1 {
        for i in -1..=1 {
            let c = ic + Vec2::new(i as f32, j as f32);
            let ci = (c.x as i32, c.y as i32);
            let h = hash3i(ci.0, ci.1, 2, salt);
            let fp = c + Vec2::splat(0.5) + jit * (Vec2::new(rnd(h), rnd_lo(h)) - Vec2::splat(0.5));
            let dd = (fp - q).length();
            if dd < d1 {
                d2 = d1;
                d1 = dd;
                bid = ci;
            } else if dd < d2 {
                d2 = dd;
            }
        }
    }
    let cell = hash3i(bid.0, bid.1, 3, salt) | 1;
    // Approximate distance to the cell border.
    let edge = 0.5 * (d2 - d1) * size;
    let mut o = Pat { m: 0.0, mean: 0.0, feat: size, cell, d: edge, gap };
    if variant == 0 {
        // spots: discs of radius r (a fraction of a cell) around the feature points
        let r = l.b[0] * size;
        o.m = pulse(d1 * size, 2.0 * r, fw);
        o.mean = l.a[3];
        o.feat = 2.0 * r;
    } else if variant == 1 {
        o.m = pulse(edge, gap, fw);
        o.mean = (2.2 * gap / size).min(1.0);
    } else {
        o.m = step(rnd(pcg(o.cell)), l.a[3]);
        o.mean = l.a[3];
    }
    o
}

fn pat_shingles(uv: Vec2, l: &GpuLayer, variant: u32, fw: f32, salt: u32) -> Pat {
    let (w, h, gap) = (l.a[0], l.a[1], l.a[2]);
    let mut row = (uv.y / h).floor();
    let fv = uv.y / h - row;
    let mut off = l.a[3] * w * (row - 2.0 * (row * 0.5).floor());
    let mut x = (uv.x - off) / w;
    let mut col = x.floor();
    let mut fu = x - col - 0.5;
    // Tongue: the tile's lower edge rises from its centre to its sides.
    let edge = match variant {
        0 => 0.5 - (0.25 - fu * fu).max(0.0).sqrt(),
        2 => fu.abs(),
        _ => 0.0,
    };
    let rise = edge * w;
    let mut dy = fv * h - rise;
    if dy < 0.0 {
        // Below this tile's tongue: the exposed part of the tile in the row below.
        row -= 1.0;
        off = l.a[3] * w * (row - 2.0 * (row * 0.5).floor());
        x = (uv.x - off) / w;
        col = x.floor();
        fu = x - col - 0.5;
        dy += h;
    }
    let dside = (0.5 - fu.abs()) * w;
    Pat {
        // Outline along the tongue edge (distance taken vertically) and the tile sides.
        m: pulse(dy.min(h - dy), gap, fw).max(pulse(dside, gap, fw)),
        mean: (gap / h + gap / w).min(1.0),
        feat: w.min(h),
        cell: hash3i(col as i32, row as i32, 4, salt) | 1,
        // Cavity: the shadow just under the overlapping tile edge above.
        d: h - dy,
        gap,
    }
}

fn magic(p: Vec3, depth: u32, distort: f32) -> f32 {
    let mut x = ((p.x + p.y + p.z) * 5.0).sin();
    let mut y = ((-p.x + p.y - p.z) * 5.0).cos();
    let mut z = -((-p.x - p.y + p.z) * 5.0).cos();
    if depth > 0 {
        x *= distort;
        y = -(x - y - z).cos() * distort;
    }
    if depth > 1 {
        z = (-x - y - z).sin() * distort;
        x = -(-x + y - z).cos() * distort;
    }
    if depth > 2 {
        y = -(-x + y + z).sin() * distort;
        z = (x + y + z).cos() * distort;
    }
    if depth > 3 {
        x = (-x - y + z).cos() * distort;
    }
    if distort != 0.0 {
        x /= distort;
    }
    0.5 + 0.5 * x
}

/// Evaluates a packed material over `base` (the instance's linear colour): the port of
/// `eval_material` in the WGSL library.
pub fn eval_packed(mat: &GpuMaterial, base: Vec3, s: &Sample) -> Shaded {
    let count = mat.head[0].min(3) as usize;
    let map = mat.head[1];
    let inv_scale = mat.pbr[2];
    let (mut p, mut n) = (s.local, s.local_normal);
    if map == 1 {
        p = s.world;
        n = s.world_normal;
    }
    let wn = s.world_normal;
    let ext = s.half_extents;
    let seed = s.seed & 0xfffff;
    let fw0 = s.footprint * inv_scale;
    p *= inv_scale;
    // Inside the surface, so lattice faces never land on it.
    let p3 = p - n * 1e-3;
    let mut c = base;
    let mut glow = 0.0;
    let mut prev = 0.0;
    for l in &mat.layers[..count] {
        let kind = l.head[0] & 0xff;
        let variant = (l.head[0] >> 8) & 0xf;
        let axis = (l.head[0] >> 12) & 0xf;
        let facing = (l.head[0] >> 16) & 0xf;
        let maskby = (l.head[0] >> 20) & 0xf;
        let paint = (l.head[0] >> 24) & 0xf;
        let ncells = (l.head[0] >> 28) & 0xf;
        let salt = l.head[1].wrapping_add(seed.wrapping_mul(0x9e3779b9));
        let fw = fw0;
        let dir = Vec3::new(l.dir[0], l.dir[1], l.dir[2]);
        let mut uv = if map == 2 && n.y.abs() < 0.7 {
            // around the part's local y axis: u = arc length, v = height
            let r = ext.x.max(1e-3) * inv_scale;
            let uv = Vec2::new(p.x.atan2(p.z) * r, p.y);
            if axis == 1 { Vec2::new(uv.y, uv.x) } else { uv }
        } else {
            plane_uv(p, n, axis)
        };
        if mat.head[2] & 1 != 0 {
            // material `turn`: 2D patterns run the other way
            uv = Vec2::new(uv.y, uv.x);
        }
        let mut pt = Pat { m: 0.0, mean: 0.0, feat: 0.0, cell: 0, d: 1e9, gap: 0.0 };
        // Feature size and mean coverage from the parameters alone: a layer whose features
        // are below a pixel is replaced by its mean without evaluating the pattern.
        let mut feat = 1e9;
        let mut mean = 0.0;
        let mut cav_mean = 0.0;
        match kind {
            kernel::NOISE | kernel::MAGIC => {
                feat = l.a[0] * if kind == kernel::MAGIC { 0.3 } else { 0.5 };
                mean = l.a[3];
            }
            kernel::CELLS => {
                // spots: the spot's diameter
                feat = if variant == 0 { 2.0 * l.b[0] * l.a[0] } else { l.a[0] };
                mean = if variant == 0 {
                    l.a[3]
                } else if variant == 1 {
                    (2.2 * l.a[2] / l.a[0]).min(1.0)
                } else {
                    l.a[3]
                };
                cav_mean = l.c[1] * (l.c[3] / feat).min(1.0) * 0.5;
            }
            kernel::GRID | kernel::SHINGLES => {
                feat = l.a[0].min(l.a[1]);
                mean = 1.0 - (1.0 - (l.a[2] / l.a[0]).min(1.0)) * (1.0 - (l.a[2] / l.a[1]).min(1.0));
                cav_mean = l.c[1] * (l.c[3] / feat).min(1.0) * 0.5;
            }
            kernel::CHECKER => {
                feat = l.a[0];
                mean = 0.5;
            }
            kernel::BANDS | kernel::WOOD => {
                feat = l.a[0] * l.a[1].min(1.0 - l.a[1]);
                mean = l.a[1];
            }
            kernel::DOTS => {
                feat = 2.0 * l.a[1];
                mean = (PI_WGSL * l.a[1] * l.a[1] / (l.a[0] * l.a[0])).min(1.0);
            }
            _ => {}
        }
        let resolved = fade(feat, fw) > 0.0;
        if !resolved {
            pt.m = mean;
            pt.mean = mean;
            pt.feat = feat;
            if ncells > 0 {
                // The cells' mean colour.
                c = Vec3::new(l.b[1], l.b[2], l.b[3]);
            }
            c *= (1.0 - cav_mean) * (1.0 - cav_mean);
        }
        match if resolved { kind } else { kernel::NONE } {
            kernel::NOISE => {
                let mut q = p3 / l.a[0];
                if l.a[2] > 0.0 {
                    q += Vec3::splat(l.a[2] * (vnoise(q * 0.5 + Vec3::splat(17.3), salt.wrapping_add(99)) - 0.5));
                }
                let v = fbm(q, l.a[1] as u32, l.a[0], fw, salt);
                pt.m = smoothstep(l.b[0] - l.b[1], l.b[0] + l.b[1], v);
                pt.mean = l.a[3];
                pt.feat = l.a[0] * 0.5;
            }
            kernel::CELLS => pt = pat_cells(uv, l, variant, fw, salt),
            kernel::GRID => pt = pat_grid(uv, l, fw, salt, variant == 1),
            kernel::CHECKER => {
                // 3D
                let q = (p3 / l.a[0]).floor();
                let k = q.x + q.y + q.z;
                pt.m = k - 2.0 * (k * 0.5).floor();
                pt.mean = 0.5;
                pt.feat = l.a[0];
            }
            kernel::GRADIENT => {
                // along an axis: 1 at `from`, 0 at `to`
                let mut t = p.dot(dir);
                if map != 1 {
                    t += (ext * inv_scale).dot(dir.abs());
                }
                pt.m = smoothstep(0.0, 1.0, ((t - l.a[1]) / (l.a[0] - l.a[1])).clamp(0.0, 1.0));
                pt.mean = pt.m;
                pt.feat = 1e9;
            }
            kernel::BANDS => {
                // wave, stripes (bands or rings)
                let mut t = if variant == 1 { (p - dir * p.dot(dir)).length() } else { p.dot(dir) };
                if l.a[2] > 0.0 {
                    t += l.a[2] * l.a[0] * (fbm(p3 / (l.a[0] * 4.0), 2, l.a[0] * 4.0, fw, salt) - 0.5);
                }
                // distance to the band centre
                let dd = (fract(t / l.a[0]) - 0.5).abs() * l.a[0];
                let half = l.a[1] * l.a[0] * 0.5;
                let sw = (l.b[0] * l.a[0] * 0.25).max(0.0);
                pt.m = if sw > 0.0 { 1.0 - smoothstep(half - sw, half + sw, dd) } else { pulse(dd, 2.0 * half, fw) };
                pt.mean = l.a[1];
                pt.feat = l.a[0] * l.a[1].min(1.0 - l.a[1]);
            }
            kernel::WOOD => {
                // rings around the axis plus grain along it
                let r = (p - dir * p.dot(dir)).length();
                let along = p.dot(dir);
                let warp =
                    l.a[2] * (vnoise(Vec3::new(along / (l.a[0] * 12.0), r / (l.a[0] * 3.0), 0.0) + Vec3::splat(5.1), salt) - 0.5);
                let t = r / l.a[0] + warp;
                let dd = (fract(t) - 0.5).abs() * l.a[0];
                let half = l.a[1] * l.a[0] * 0.5;
                let sw = (l.b[0] * l.a[0] * 0.25).max(0.0);
                pt.m = 1.0 - smoothstep(half - sw, half + sw, dd);
                pt.mean = l.a[1];
                pt.feat = l.a[0] * l.a[1].min(1.0 - l.a[1]);
            }
            kernel::MAGIC => {
                let v = magic(p3 / l.a[0], l.a[1] as u32, l.a[2]);
                pt.m = smoothstep(l.b[0] - l.b[1], l.b[0] + l.b[1], v);
                pt.mean = l.a[3];
                pt.feat = l.a[0] * 0.3;
            }
            kernel::DOTS => {
                let s = l.a[0];
                let row = (uv.y / s).floor();
                let off = l.a[2] * s * (row - 2.0 * (row * 0.5).floor());
                let x = (uv.x - off) / s;
                let col = x.floor();
                let h = hash3i(col as i32, row as i32, 5, salt);
                let ctr = Vec2::splat(0.5) + l.a[3] * (Vec2::new(rnd(h), rnd_lo(h)) - Vec2::splat(0.5));
                let local = Vec2::new(x - col, uv.y / s - row);
                let dd = (local - ctr).length() * s;
                pt.m = pulse(dd, 2.0 * l.a[1], fw);
                pt.mean = (PI_WGSL * l.a[1] * l.a[1] / (s * s)).min(1.0);
                pt.feat = 2.0 * l.a[1];
                pt.cell = h | 1;
            }
            kernel::SHINGLES => pt = pat_shingles(uv, l, variant, fw, salt),
            _ => {
                if resolved {
                    pt.m = 0.0;
                    pt.mean = 0.0;
                    pt.feat = 1e9;
                }
            }
        }
        let k = fade(pt.feat, fw);
        let mut m = mix(pt.mean, pt.m, k);
        match facing {
            1 => m *= smoothstep(0.3, 0.7, wn.y),
            2 => m *= smoothstep(0.3, 0.7, -wn.y),
            3 => m *= 1.0 - smoothstep(0.3, 0.7, wn.y.abs()),
            _ => {}
        }
        match maskby {
            1 => m *= prev,
            2 => m *= 1.0 - prev,
            _ => {}
        }
        // Cells: colour choice, variation and cavity under the painted gaps.
        if pt.cell != 0 {
            if ncells > 0 {
                let pick = ((rnd(pcg(pt.cell ^ 0x51)) * ncells as f32) as u32).min(ncells - 1);
                // Fades to the cells' mean colour, as an unresolved layer shows it.
                c = Vec3::new(l.b[1], l.b[2], l.b[3]).lerp(srgb8_to_linear(l.cells[pick as usize]), k);
            }
            let vary = l.c[0] * (2.0 * rnd(pt.cell) - 1.0) * k;
            c *= (1.0 + vary) * (1.0 + vary);
            if l.c[1] > 0.0 {
                let bev = l.c[3].max(1e-4);
                let prof = 1.0 - smoothstep(pt.gap * 0.5, pt.gap * 0.5 + bev, pt.d);
                let s = 1.0 - l.c[1] * mix((bev / pt.feat.max(1e-4)).min(1.0) * 0.5, prof, fade(bev, fw));
                c *= s * s;
            }
        }
        let col = if paint == 1 { c * l.paint[0] } else { Vec3::new(l.paint[0], l.paint[1], l.paint[2]) };
        let a = m * l.paint[3];
        c = c.lerp(col, a);
        glow += l.c[2] * a;
        prev = m;
    }
    Shaded { rgb: c, glow }
}
