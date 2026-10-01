//! In-world text: glyphs from a font are turned into a signed-distance-field atlas once at
//! startup, so letters stay crisp at any distance or zoom. Layout happens on the CPU; each
//! glyph is one instanced quad.

use std::collections::HashMap;

use ab_glyph::{Font, FontRef, Glyph, PxScale, point};
use glam::Vec3;

/// Pixels per em in the atlas.
const EM_PX: f32 = 56.0;
/// Distance-field spread (pixels each side of the edge).
const SPREAD: f32 = 7.0;
pub const ATLAS_W: u32 = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct GlyphInfo {
    /// Atlas rectangle (0..1).
    pub uv: [f32; 4],
    /// Quad offset from the pen position and size, in em units (y up).
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub advance: f32,
}

pub struct FontAtlas {
    pub width: u32,
    pub height: u32,
    /// R8 distance field (0.5 = edge, > 0.5 inside).
    pub pixels: Vec<u8>,
    pub glyphs: HashMap<char, GlyphInfo>,
    /// Ascent/descent in em units.
    pub ascent: f32,
    pub descent: f32,
}

/// 1D squared distance transform (Felzenszwalb & Huttenlocher).
fn edt_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        let s = loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q as f32 - p as f32));
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break s;
            }
        };
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, dq) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        *dq = (q as f32 - p as f32).powi(2) + f[p];
    }
}

/// Euclidean distance from each pixel to the nearest `true` pixel.
fn edt(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e10;
    let mut grid: Vec<f32> = mask.iter().map(|m| if *m { 0.0 } else { INF }).collect();
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        for x in 0..w {
            grid[y * w + x] = d[x].sqrt();
        }
    }
    grid
}

impl FontAtlas {
    /// Builds the atlas for printable ASCII plus a few symbols.
    pub fn new() -> Self {
        let font = FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("built-in font");
        let upem = font.units_per_em().unwrap_or(1000.0);
        // PxScale is the font's ascent-descent height in pixels; convert from em.
        let height_unscaled = font.height_unscaled();
        let scale = PxScale::from(EM_PX * height_unscaled / upem);
        let px_per_unit = EM_PX / upem;
        let pad = SPREAD.ceil() as i32 + 1;
        let chars: Vec<char> = (32u8..127).map(|c| c as char).chain("°×→←↑↓·".chars()).collect();

        let mut pixels = vec![0u8; (ATLAS_W * 512) as usize];
        let mut height = 512u32;
        let (mut cx, mut cy, mut row_h) = (0i32, 0i32, 0i32);
        let mut glyphs = HashMap::new();
        for ch in chars {
            let id = font.glyph_id(ch);
            let advance = font.h_advance_unscaled(id) * px_per_unit / EM_PX;
            let glyph: Glyph = id.with_scale_and_position(scale, point(0.0, 0.0));
            let Some(outline) = font.outline_glyph(glyph) else {
                glyphs.insert(ch, GlyphInfo { advance, ..Default::default() });
                continue;
            };
            let b = outline.px_bounds();
            let (gw, gh) = (b.width().ceil() as i32, b.height().ceil() as i32);
            let (w, h) = (gw + 2 * pad, gh + 2 * pad);
            let mut cov = vec![0.0f32; (w * h) as usize];
            outline.draw(|x, y, c| {
                let (x, y) = (x as i32 + pad, y as i32 + pad);
                if x < w && y < h {
                    cov[(y * w + x) as usize] = c;
                }
            });
            let inside: Vec<bool> = cov.iter().map(|c| *c >= 0.5).collect();
            let outside: Vec<bool> = inside.iter().map(|i| !i).collect();
            let d_in = edt(&inside, w as usize, h as usize);
            let d_out = edt(&outside, w as usize, h as usize);
            if cx + w > ATLAS_W as i32 {
                cx = 0;
                cy += row_h;
                row_h = 0;
            }
            while (cy + h) as u32 > height {
                height *= 2;
                pixels.resize((ATLAS_W * height) as usize, 0);
            }
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    // > 0 inside: distance to the nearest outside pixel.
                    let sd = if inside[i] { d_out[i] - 0.5 } else { -(d_in[i] - 0.5) };
                    let v = (0.5 + sd / (2.0 * SPREAD)).clamp(0.0, 1.0);
                    pixels[((cy + y) as u32 * ATLAS_W + (cx + x) as u32) as usize] = (v * 255.0).round() as u8;
                }
            }
            let (aw, ah) = (ATLAS_W as f32, 1.0); // v normalised after the final height is known
            glyphs.insert(
                ch,
                GlyphInfo {
                    uv: [cx as f32 / aw, cy as f32 * ah, (cx + w) as f32 / aw, (cy + h) as f32 * ah],
                    offset: [(b.min.x - pad as f32) / EM_PX, -(b.max.y + pad as f32) / EM_PX],
                    size: [w as f32 / EM_PX, h as f32 / EM_PX],
                    advance,
                },
            );
            cx += w;
            row_h = row_h.max(h);
        }
        let hf = height as f32;
        for g in glyphs.values_mut() {
            g.uv[1] /= hf;
            g.uv[3] /= hf;
        }
        Self {
            width: ATLAS_W,
            height,
            pixels,
            glyphs,
            ascent: font.ascent_unscaled() / upem,
            descent: font.descent_unscaled() / upem,
        }
    }

    /// Width of a line of text in em units.
    pub fn line_width(&self, text: &str) -> f32 {
        text.chars().map(|c| self.glyphs.get(&c).or(self.glyphs.get(&'?')).map(|g| g.advance).unwrap_or(0.5)).sum()
    }
}

impl Default for FontAtlas {
    fn default() -> Self {
        Self::new()
    }
}

/// Horizontal anchor of a text block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    Center,
    Left,
}

/// Text in the world, on the plane spanned by `right` and `up` (unit vectors).
#[derive(Clone, Debug)]
pub struct Text3d {
    pub text: String,
    /// Anchor point: the centre (or left end) of the text block's middle.
    pub origin: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    /// Capital-letter height is about 0.7 of this (m).
    pub size: f32,
    /// Linear RGB.
    pub color: Vec3,
    pub anchor: Anchor,
    /// Render flags (see `scene::flags`).
    pub flags: u32,
    /// Stroke weight adjustment: 0 = font weight, positive = bolder.
    pub weight: f32,
}

/// One glyph quad, ready for the GPU.
#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    pub corner: Vec3,
    pub ax: Vec3,
    pub ay: Vec3,
    pub uv: [f32; 4],
}

/// Lays out (possibly multi-line) text into glyph quads.
pub fn layout(atlas: &FontAtlas, t: &Text3d, out: &mut Vec<GlyphQuad>) {
    let lines: Vec<&str> = t.text.lines().collect();
    let line_h = 1.15;
    let total_h = line_h * lines.len() as f32;
    let cap_mid = (atlas.ascent * 0.72) * 0.5;
    for (li, line) in lines.iter().enumerate() {
        let w = atlas.line_width(line);
        let mut x = match t.anchor {
            Anchor::Center => -w * 0.5,
            Anchor::Left => 0.0,
        };
        // Baseline so the block is vertically centred on the origin.
        let y = total_h * 0.5 - line_h * (li as f32 + 1.0) + (line_h - 1.0) * 0.5 + 0.5 - cap_mid;
        for c in line.chars() {
            let g = atlas.glyphs.get(&c).or(atlas.glyphs.get(&'?')).copied().unwrap_or_default();
            if g.size[0] > 0.0 {
                let gx = x + g.offset[0];
                let gy = y + g.offset[1];
                out.push(GlyphQuad {
                    corner: t.origin + (t.right * gx + t.up * gy) * t.size,
                    ax: t.right * g.size[0] * t.size,
                    ay: t.up * g.size[1] * t.size,
                    uv: g.uv,
                });
            }
            x += g.advance;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_has_letters() {
        let a = FontAtlas::new();
        let g = a.glyphs[&'A'];
        assert!(g.size[0] > 0.3 && g.size[1] > 0.5, "{g:?}");
        assert!(a.line_width("HELLO") > 2.0);
        // Distance field: the centre of 'I' is inside.
        let gi = a.glyphs[&'I'];
        let cx = ((gi.uv[0] + gi.uv[2]) * 0.5 * a.width as f32) as usize;
        let cy = ((gi.uv[1] + gi.uv[3]) * 0.5 * a.height as f32) as usize;
        assert!(a.pixels[cy * a.width as usize + cx] > 140);
    }
}
