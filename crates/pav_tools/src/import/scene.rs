//! The one intermediate every model reader fills (glTF, OBJ) and everything after reading uses
//! (normalisation, the census, and from row 5.2 the fitter).
//!
//! Conventions: world space (each mesh's vertices already placed by its node), metres, +y up,
//! front +z, counter-clockwise front faces (a mirroring node's triangles are reversed when
//! read), colours linear, texture coordinates with v down (glTF's: image row 0 at v = 0).

use glam::{DMat3, DMat4, DVec3};

/// The most triangles a model may have.
pub const MAX_TRIS: usize = 2_000_000;
/// The most images a model may have, and the largest side of one that is decoded.
pub const MAX_IMAGES: usize = 64;
pub const MAX_IMAGE_SIDE: u32 = 4096;

#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// Parents before children.
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    pub info: Info,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub world: DMat4,
}

/// One primitive: triangles of one material under one node. Attribute lists are empty when the
/// source has none, else as long as `pos`.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub node: usize,
    pub material: Option<usize>,
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    /// The second texture coordinate set (TEXCOORD_1).
    pub uv1: Vec<[f32; 2]>,
    /// Vertex colours, linear RGBA.
    pub col: Vec<[f32; 4]>,
    pub tris: Vec<[u32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    Opaque,
    /// Cut out below this alpha.
    Mask(f32),
    Blend,
}

/// A texture on a material: which image, which coordinate set, the 2×3 transform applied to
/// the coordinates first (`u' = a·u + b·v + c`, `v' = d·u + e·v + f`, KHR_texture_transform),
/// and how coordinates outside 0..1 wrap in u and v (the glTF sampler's `wrapS`/`wrapT`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexRef {
    pub image: usize,
    pub uv_set: u32,
    pub xform: [f32; 6],
    pub wrap: [Wrap; 2],
}

pub const XFORM_IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// How a texture coordinate outside 0..1 lands on the image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// The image tiles (glTF's default, REPEAT 10497).
    #[default]
    Repeat,
    /// Held at the edge (CLAMP_TO_EDGE 33071).
    Clamp,
    /// Tiles mirrored every other time (MIRRORED_REPEAT 33648).
    Mirror,
}

impl Wrap {
    /// From a glTF sampler's wrap code (missing or unknown: repeat).
    pub fn from_gl(code: Option<u64>) -> Wrap {
        match code {
            Some(33071) => Wrap::Clamp,
            Some(33648) => Wrap::Mirror,
            _ => Wrap::Repeat,
        }
    }

    /// A coordinate brought into 0..=1.
    pub fn apply(self, t: f64) -> f64 {
        match self {
            Wrap::Repeat => t - t.floor(),
            Wrap::Clamp => t.clamp(0.0, 1.0),
            Wrap::Mirror => {
                let m = t.rem_euclid(2.0);
                if m > 1.0 { 2.0 - m } else { m }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Base colour factor, linear RGBA.
    pub base: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear, emissive strength included.
    pub emissive: [f32; 3],
    pub alpha: Alpha,
    pub double_sided: bool,
    pub unlit: bool,
    pub base_tex: Option<TexRef>,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: String::new(),
            base: [1.0; 4],
            metallic: 1.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            alpha: Alpha::Opaque,
            double_sided: false,
            unlit: false,
            base_tex: None,
        }
    }
}

/// An image, decoded to RGBA8, sRGB as stored. `rgba` is empty when it was not decoded (not used
/// as a base colour, a JPEG, or over `MAX_IMAGE_SIDE`); `w` and `h` are still known when the
/// header could be read.
#[derive(Clone, Debug, Default)]
pub struct Image {
    pub name: String,
    /// `png`, `jpeg`, `webp`, `ktx2` or `unknown`.
    pub kind: String,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Info {
    pub file: String,
    /// `glb`, `gltf` or `obj`.
    pub format: String,
    pub generator: String,
    /// Extensions the file uses (glTF), sorted.
    pub extensions: Vec<String>,
    /// Things worth knowing that did not stop reading (skipped primitives, undecoded images).
    pub warnings: Vec<String>,
    /// It has skinned meshes (read in their bind pose).
    pub skinned: bool,
    /// It has animations (nodes are read at rest).
    pub animated: bool,
}

impl Scene {
    pub fn tris(&self) -> usize {
        self.meshes.iter().map(|m| m.tris.len()).sum()
    }

    /// The box around every vertex that a triangle uses (min, max), metres.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut b: Option<(DVec3, DVec3)> = None;
        for m in &self.meshes {
            for t in &m.tris {
                for &i in t {
                    let p = DVec3::from(m.pos[i as usize].map(f64::from));
                    b = Some(match b {
                        Some((lo, hi)) => (lo.min(p), hi.max(p)),
                        None => (p, p),
                    });
                }
            }
        }
        b
    }

    /// Moves everything by `m` (computed in f64): vertices, normals and node frames. A mirroring
    /// `m` reverses every triangle so front faces stay counter-clockwise.
    pub fn transform(&mut self, m: DMat4) {
        let nm = normal_matrix(m);
        let flip = m.determinant() < 0.0;
        for mesh in &mut self.meshes {
            for p in &mut mesh.pos {
                *p = m.transform_point3(DVec3::from(p.map(f64::from))).as_vec3().to_array();
            }
            for n in &mut mesh.nrm {
                *n = (nm * DVec3::from(n.map(f64::from))).normalize_or_zero().as_vec3().to_array();
            }
            if flip {
                for t in &mut mesh.tris {
                    t.swap(1, 2);
                }
            }
        }
        for n in &mut self.nodes {
            n.world = m * n.world;
        }
    }
}

/// The matrix that carries normals through `m` (its inverse transpose).
pub fn normal_matrix(m: DMat4) -> DMat3 {
    let a = DMat3::from_mat4(m);
    if a.determinant().abs() < 1e-300 { a } else { a.inverse().transpose() }
}

/// An sRGB byte as linear light.
pub fn srgb_to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear light as an sRGB byte.
pub fn linear_to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// Linear sRGB as Oklab (L, a, b): distances there track how different colours look (ΔE_OK).
pub fn oklab(c: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = c;
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// A linear colour as `#rrggbb` (sRGB).
pub fn hex(c: [f64; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2]))
}
