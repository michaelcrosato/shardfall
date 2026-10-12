//! How each layer of a material reads at the Shardfall camera (asset plan §6.3, §6.6): its
//! feature size, the pixels a feature spans, how much of a surface it visibly paints, and a
//! verdict. Swatches report it beside their image.

use glam::Vec3;

use super::eval::{Sample, eval_packed};
use super::resolve::{Material, feature};
use crate::props::doc::MapMode;

/// Metres per pixel at the game camera (tilt 56°, 15.5 m, fov 40°) at 1080p and at 540p.
pub const PIXEL_1080P: f32 = 0.0104;
pub const PIXEL_540P: f32 = 0.021;

/// One layer at the game camera.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerReport {
    pub pattern: &'static str,
    /// The feature size in metres (after the material's scale); None for gradients.
    pub feature: Option<f32>,
    /// Pixels one feature spans at 1080p.
    pub px: Option<f32>,
    /// The share of a surface (a floor and a wall, 2 m square each) whose colour or glow it
    /// visibly changes, 0-1.
    pub cover: f32,
    pub verdict: &'static str,
}

/// Every layer of `material`, bottom first.
pub fn layers(material: &Material) -> Vec<LayerReport> {
    let packed = material.pack(material.map.unwrap_or(MapMode::Object), &[]);
    // Fine sampling, so the pattern itself is measured rather than its blur.
    const N: usize = 48;
    let mut samples = Vec::with_capacity(2 * N * N);
    for i in 0..N {
        for j in 0..N {
            let (u, v) = ((i as f32 + 0.5) / N as f32 * 2.0 - 1.0, (j as f32 + 0.5) / N as f32 * 2.0 - 1.0);
            samples.push(Sample::at(Vec3::new(u, 0.0, v), Vec3::Y, 0.002));
            samples.push(Sample::at(Vec3::new(u, v + 1.0, 0.0), Vec3::Z, 0.002));
        }
    }
    // The surface with the layers up to each one: a layer covers what it changes over those
    // below it (its paint, and its cells' variation and cavities).
    let upto = |count: usize| -> Vec<_> {
        let mut gpu = packed.gpu;
        gpu.head[0] = count as u32;
        samples.iter().map(|s| eval_packed(&gpu, packed.base, s)).collect()
    };
    let mut below = upto(0);
    material
        .layers
        .iter()
        .enumerate()
        .take(3)
        .map(|(index, layer)| {
            let with = upto(index + 1);
            let changed = with
                .iter()
                .zip(&below)
                .filter(|(a, b)| (a.rgb - b.rgb).abs().max_element() > 2.0 / 255.0 || (a.glow - b.glow).abs() > 0.01)
                .count();
            below = with;
            let cover = changed as f32 / samples.len() as f32;
            let size = feature(layer).map(|(_, size)| size * material.scale);
            let px = size.map(|size| size / PIXEL_1080P);
            let verdict = match size {
                _ if cover < 0.03 => "barely shows",
                None => "ok",
                Some(size) if size / PIXEL_540P >= 3.0 => "clear",
                Some(size) if size / PIXEL_540P >= 1.0 => "soft at 540p",
                Some(size) if size / PIXEL_1080P >= 3.0 => "1080p only",
                Some(size) if size / PIXEL_1080P >= 1.0 => "fades to flat",
                Some(_) => "invisible",
            };
            LayerReport { pattern: layer.pattern.name(), feature: size, px, cover, verdict }
        })
        .collect()
}
