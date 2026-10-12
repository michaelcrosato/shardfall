//! glTF 2.0, read in-house: the container (`.glb`, or `.gltf` with `.bin` files and `data:`
//! URIs), accessors (any component type, normalised integers, sparse), the node hierarchy, and
//! `scene`: a whole model read into the importer's `Scene` (meshes, materials, textures,
//! images). The motion importer (`mocap::glb`) reads its rigs and clips through the same
//! container, accessors, matrices and hierarchy.
//!
//! Inputs are checked as they are read: chunk types, buffer and view bounds, index ranges, and
//! extensions a model cannot be read without (Draco, meshopt, Basis, WebP) are named in the
//! error.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use glam::{DMat4, DVec3};
use serde_json::Value;

use super::image;
use super::scene::{
    Alpha, Image, Info, MAX_IMAGES, MAX_TRIS, Material, Mesh, Node, Scene, TexRef, XFORM_IDENTITY, normal_matrix,
};

/* ---- column-major 4x4 matrices, quaternions [x, y, z, w] ---- */
pub type M4 = [f64; 16];
pub type V = [f64; 3];

pub const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn mat(t: V, q: [f64; 4], s: V) -> M4 {
    let [x, y, z, w] = q;
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [
        (1.0 - 2.0 * (yy + zz)) * s[0],
        2.0 * (xy + wz) * s[0],
        2.0 * (xz - wy) * s[0],
        0.0,
        2.0 * (xy - wz) * s[1],
        (1.0 - 2.0 * (xx + zz)) * s[1],
        2.0 * (yz + wx) * s[1],
        0.0,
        2.0 * (xz + wy) * s[2],
        2.0 * (yz - wx) * s[2],
        (1.0 - 2.0 * (xx + yy)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}
pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
        }
    }
    o
}
pub fn xf(m: &M4, p: V) -> V {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// A node's translation, rotation and scale.
#[derive(Clone)]
pub struct Trs {
    pub t: V,
    pub r: [f64; 4],
    pub s: V,
}

pub fn arr<const N: usize>(v: &Value, d: [f64; N]) -> [f64; N] {
    match v.as_array() {
        Some(a) if a.len() == N => std::array::from_fn(|i| a[i].as_f64().unwrap_or(d[i])),
        _ => d,
    }
}

/// A node at rest: its translation, rotation and scale (a node placed by a matrix: the matrix
/// taken apart).
pub fn rest_trs(n: &Value) -> Trs {
    let m = arr::<16>(&n["matrix"], IDENTITY);
    if n.get("matrix").is_some() && n.get("translation").is_none() && n.get("rotation").is_none() {
        // A node placed by a matrix: its translation, rotation and scale.
        let (s, r, t) = DMat4::from_cols_array(&m).to_scale_rotation_translation();
        return Trs { t: t.to_array(), r: r.to_array(), s: s.to_array() };
    }
    Trs { t: arr(&n["translation"], [0.0; 3]), r: arr(&n["rotation"], [0.0, 0.0, 0.0, 1.0]), s: arr(&n["scale"], [1.0; 3]) }
}

/// A node's local matrix at rest: its `matrix`, else its translation, rotation and scale.
pub fn local_matrix(n: &Value) -> M4 {
    if n.get("matrix").is_some() {
        return arr::<16>(&n["matrix"], IDENTITY);
    }
    let t = rest_trs(n);
    mat(t.t, t.r, t.s)
}

/// The node tree: each node's parent (`usize::MAX`: none) and an order with parents before
/// children (a cycle is cut where it closes).
pub struct Hierarchy {
    pub parent: Vec<usize>,
    pub order: Vec<usize>,
}

impl Hierarchy {
    pub fn new(nodes: &[Value]) -> Hierarchy {
        let mut parent = vec![usize::MAX; nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            for c in n["children"].as_array().into_iter().flatten() {
                if let Some(c) = c.as_u64().filter(|&c| (c as usize) < nodes.len()) {
                    parent[c as usize] = i;
                }
            }
        }
        // Parents before children.
        let mut order = Vec::with_capacity(nodes.len());
        let mut seen = vec![false; nodes.len()];
        for i in 0..nodes.len() {
            let mut chain = vec![i];
            while let Some(&p) = chain.last().map(|&c| &parent[c]) {
                if p == usize::MAX || seen[p] || chain.contains(&p) {
                    break;
                }
                chain.push(p);
            }
            for &c in chain.iter().rev() {
                if !seen[c] {
                    seen[c] = true;
                    order.push(c);
                }
            }
        }
        Hierarchy { parent, order }
    }

    /// Every node's world matrix from the local ones.
    pub fn world(&self, local: &[M4]) -> Vec<M4> {
        let mut w = vec![[0.0; 16]; local.len()];
        for &i in &self.order {
            w[i] = if self.parent[i] != usize::MAX { mul(&w[self.parent[i]], &local[i]) } else { local[i] };
        }
        w
    }
}

/// The most elements an accessor may have (a model's limit is 2M triangles, 6M indices).
pub const MAX_ELEMENTS: usize = 1 << 24;

const GLB_MAGIC: u32 = 0x4654_6c67;
const CHUNK_JSON: u32 = 0x4e4f_534a;
const CHUNK_BIN: u32 = 0x004e_4942;

/// A glTF file: its JSON and its buffers.
pub struct Gltf {
    pub json: Value,
    buffers: Vec<Vec<u8>>,
    /// The folder its files sit in (`.bin` files, images).
    dir: PathBuf,
    /// Read from a `.glb`.
    pub glb: bool,
}

/// A file a glTF names, relative to its own folder: percent-decoded, kept inside that folder.
fn sibling(dir: &Path, uri: &str) -> Result<PathBuf> {
    let mut bytes = Vec::new();
    let b = uri.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&uri[i + 1..i + 3], 16) {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    let rel = PathBuf::from(String::from_utf8_lossy(&bytes).to_string());
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) || uri.contains("://") {
        bail!("{uri}: a file outside the model's folder is not read");
    }
    Ok(dir.join(rel))
}

/// Decodes base64 (standard alphabet, padding optional).
pub fn base64(text: &str) -> Result<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in text.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = val(c).ok_or_else(|| anyhow!("bad base64 character {:?}", c as char))?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// A `data:` URI's bytes.
fn data_uri(uri: &str) -> Result<Vec<u8>> {
    let (head, body) = uri.split_once(',').ok_or_else(|| anyhow!("a data URI without a comma"))?;
    if !head.ends_with(";base64") {
        bail!("a data URI that is not base64");
    }
    base64(body)
}

/// The bytes a URI names: a `data:` URI or a file beside the model.
fn uri_bytes(dir: &Path, uri: &str) -> Result<Vec<u8>> {
    if uri.starts_with("data:") {
        return data_uri(uri);
    }
    let p = sibling(dir, uri)?;
    std::fs::read(&p).map_err(|e| anyhow!("{}: {e}", p.display()))
}

impl Gltf {
    /// Reads a `.glb` or `.gltf` file and every buffer it names.
    pub fn open(path: &Path) -> Result<Gltf> {
        let buf = std::fs::read(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Gltf::parse(buf, &dir).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    /// Reads glTF from its bytes (`dir`: where the files it names are).
    pub fn parse(buf: Vec<u8>, dir: &Path) -> Result<Gltf> {
        let u32le = |b: &[u8], o: usize| -> Option<usize> {
            b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        };
        let (json, bin, glb): (Value, Option<Vec<u8>>, bool) = if u32le(&buf, 0) == Some(GLB_MAGIC as usize) {
            if buf.len() < 20 {
                bail!("a truncated .glb");
            }
            let jl = u32le(&buf, 12).unwrap_or(0);
            if u32le(&buf, 16) != Some(CHUNK_JSON as usize) {
                bail!("a .glb whose first chunk is not JSON");
            }
            let json: Value = serde_json::from_slice(buf.get(20..20 + jl).ok_or_else(|| anyhow!("truncated .glb"))?)?;
            let at = 20 + jl;
            let bin = match (u32le(&buf, at), u32le(&buf, at + 4)) {
                (Some(bl), Some(t)) => {
                    if t != CHUNK_BIN as usize {
                        bail!("a .glb whose second chunk is not BIN (type {t:#x})");
                    }
                    Some(buf.get(at + 8..at + 8 + bl).ok_or_else(|| anyhow!("a truncated BIN chunk"))?.to_vec())
                }
                _ => None,
            };
            (json, bin, true)
        } else {
            let text = buf.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&buf);
            if text.iter().find(|c| !c.is_ascii_whitespace()) != Some(&b'{') {
                bail!("not a glTF file (.glb or .gltf)");
            }
            (serde_json::from_slice(text)?, None, false)
        };
        let mut bin = bin;
        let mut buffers = Vec::new();
        for (i, b) in json["buffers"].as_array().into_iter().flatten().enumerate() {
            let data = match b["uri"].as_str() {
                Some(uri) => uri_bytes(dir, uri).map_err(|e| anyhow!("buffer {i}: {e}"))?,
                None if i == 0 && glb => bin.take().ok_or_else(|| anyhow!("buffer 0: the .glb has no BIN chunk"))?,
                None => bail!("buffer {i} has no uri"),
            };
            let want = b["byteLength"].as_u64().unwrap_or(0) as usize;
            if data.len() < want {
                bail!("buffer {i} holds {} bytes, less than its byteLength {want}", data.len());
            }
            buffers.push(data);
        }
        Ok(Gltf { json, buffers, dir: dir.to_path_buf(), glb })
    }

    /// A buffer view's bytes.
    pub fn view(&self, i: usize) -> Result<&[u8]> {
        let bv = self.json["bufferViews"].get(i).ok_or_else(|| anyhow!("bufferView {i} does not exist"))?;
        let b = bv["buffer"].as_u64().unwrap_or(0) as usize;
        let buf = self.buffers.get(b).ok_or_else(|| anyhow!("bufferView {i}: buffer {b} does not exist"))?;
        let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = bv["byteLength"].as_u64().unwrap_or(0) as usize;
        off.checked_add(len)
            .and_then(|end| buf.get(off..end))
            .ok_or_else(|| anyhow!("bufferView {i} reads past the end of buffer {b}"))
    }

    /// An accessor's elements, each as its components.
    pub fn accessor(&self, i: &Value) -> Result<Vec<Vec<f64>>> {
        let i = i.as_u64().ok_or_else(|| anyhow!("an accessor index is missing"))? as usize;
        let (flat, n) = self.read(i)?;
        Ok(flat.chunks(n).map(<[f64]>::to_vec).collect())
    }

    /// An accessor's elements as one list (components in a row) and how many components each
    /// has. Normalised integers are scaled into 0..1 (-1..1 signed); sparse values replace
    /// their elements.
    pub fn read(&self, i: usize) -> Result<(Vec<f64>, usize)> {
        let a = self.json["accessors"].get(i).ok_or_else(|| anyhow!("accessor {i} does not exist"))?;
        let n = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => bail!("accessor type {t} is not read"),
        };
        let count = a["count"].as_u64().unwrap_or(0) as usize;
        if count > MAX_ELEMENTS {
            bail!("accessor {i}: {count} elements, over the {MAX_ELEMENTS} limit");
        }
        let ct = a["componentType"].as_u64().unwrap_or(0);
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let mut out = vec![0.0; count * n];
        if let Some(bvi) = a["bufferView"].as_u64() {
            let bv = &self.json["bufferViews"][bvi as usize];
            let stride = bv["byteStride"].as_u64().map(|s| s as usize).filter(|&s| s > 0);
            read_into(
                &mut out,
                self.view(bvi as usize)?,
                a["byteOffset"].as_u64().unwrap_or(0) as usize,
                stride,
                n,
                ct,
                normalized,
            )
            .map_err(|e| anyhow!("accessor {i}: {e}"))?;
        }
        if let Some(sp) = a.get("sparse") {
            let k = sp["count"].as_u64().unwrap_or(0) as usize;
            if k > count {
                bail!("accessor {i}: {k} sparse values for {count} elements");
            }
            let (ix, vals) = (&sp["indices"], &sp["values"]);
            let mut idx = vec![0.0; k];
            let view = |v: &Value| -> Result<&[u8]> {
                self.view(v["bufferView"].as_u64().ok_or_else(|| anyhow!("accessor {i}: sparse without a bufferView"))? as usize)
            };
            read_into(
                &mut idx,
                view(ix)?,
                ix["byteOffset"].as_u64().unwrap_or(0) as usize,
                None,
                1,
                ix["componentType"].as_u64().unwrap_or(0),
                false,
            )
            .map_err(|e| anyhow!("accessor {i}, sparse indices: {e}"))?;
            let mut v = vec![0.0; k * n];
            read_into(&mut v, view(vals)?, vals["byteOffset"].as_u64().unwrap_or(0) as usize, None, n, ct, normalized)
                .map_err(|e| anyhow!("accessor {i}, sparse values: {e}"))?;
            for (j, &e) in idx.iter().enumerate() {
                let e = e as usize;
                if e >= count {
                    bail!("accessor {i}: sparse index {e} is past its {count} elements");
                }
                out[e * n..e * n + n].copy_from_slice(&v[j * n..j * n + n]);
            }
        }
        Ok((out, n))
    }

    /// Checks that nothing the file requires is beyond this reader.
    pub fn check_required(&self) -> Result<()> {
        for e in self.json["extensionsRequired"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let why = match e {
                "KHR_draco_mesh_compression" => "Draco-compressed meshes",
                "EXT_meshopt_compression" | "KHR_meshopt_compression" => "meshopt-compressed buffers",
                "KHR_texture_basisu" => "Basis Universal (KTX2) textures",
                "EXT_texture_webp" => "WebP textures",
                "KHR_texture_transform" | "KHR_materials_emissive_strength" | "KHR_materials_unlit" | "KHR_mesh_quantization" => {
                    continue;
                }
                _ => "an extension this reader does not know",
            };
            bail!("the file requires {e} ({why}), which the importer does not read");
        }
        Ok(())
    }
}

/// Reads `out.len() / n` elements of `n` components of type `ct` from `bytes` at `off`.
fn read_into(out: &mut [f64], bytes: &[u8], off: usize, stride: Option<usize>, n: usize, ct: u64, norm: bool) -> Result<()> {
    let size = match ct {
        5126 | 5125 => 4,
        5123 | 5122 => 2,
        5121 | 5120 => 1,
        _ => bail!("component type {ct} is not read"),
    };
    let div = if norm {
        match ct {
            5123 => 65535.0,
            5121 => 255.0,
            5122 => 32767.0,
            5120 => 127.0,
            _ => 1.0,
        }
    } else {
        1.0
    };
    let count = out.len() / n.max(1);
    let stride = stride.unwrap_or(n * size);
    let need = count.saturating_sub(1).checked_mul(stride).and_then(|v| v.checked_add(off)).and_then(|v| v.checked_add(n * size));
    if count > 0 && need.is_none_or(|need| need > bytes.len()) {
        bail!("it reads past the end of its bufferView");
    }
    let b = bytes;
    let read = |o: usize| -> f64 {
        match ct {
            5126 => f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
            5125 => u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64,
            5123 => u16::from_le_bytes([b[o], b[o + 1]]) as f64,
            5122 => i16::from_le_bytes([b[o], b[o + 1]]) as f64,
            5121 => b[o] as f64,
            _ => b[o] as i8 as f64,
        }
    };
    for k in 0..count {
        for c in 0..n {
            out[k * n + c] = read(off + k * stride + c * size) / div;
        }
    }
    Ok(())
}

/// The KHR_texture_transform of a texture reference as a 2×3 matrix (translation · rotation ·
/// scale, the extension's order), and the coordinate set it overrides to, if any.
fn tex_xform(t: &Value) -> ([f32; 6], Option<u32>) {
    let x = &t["extensions"]["KHR_texture_transform"];
    if !x.is_object() {
        return (XFORM_IDENTITY, None);
    }
    let [ox, oy] = arr(&x["offset"], [0.0, 0.0]);
    let [sx, sy] = arr(&x["scale"], [1.0, 1.0]);
    let r = x["rotation"].as_f64().unwrap_or(0.0);
    let (s, c) = r.sin_cos();
    // The extension's GLSL: mat3(cos, sin, 0, -sin, cos, 0, 0, 0, 1), column-major.
    let m = [c * sx, -s * sy, ox, s * sx, c * sy, oy];
    (m.map(|v| v as f32), x["texCoord"].as_u64().map(|v| v as u32))
}

/// Reads a whole model: the default scene's nodes at rest, every triangle primitive placed in
/// world space, materials, and the images base colours use (decoded). Skinned meshes are
/// placed in their bind pose.
pub fn scene(path: &Path) -> Result<Scene> {
    let g = Gltf::open(path)?;
    let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    read_scene(&g, &file).map_err(|e| anyhow!("{file}: {e}"))
}

fn read_scene(g: &Gltf, file: &str) -> Result<Scene> {
    g.check_required()?;
    let j = &g.json;
    let mut info = Info {
        file: file.to_string(),
        format: if g.glb { "glb" } else { "gltf" }.into(),
        generator: j["asset"]["generator"].as_str().unwrap_or("").to_string(),
        ..Info::default()
    };
    let used: BTreeSet<String> =
        j["extensionsUsed"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    info.extensions = used.into_iter().collect();
    info.animated = j["animations"].as_array().is_some_and(|a| !a.is_empty());

    // Nodes at rest, in world space.
    let nodes = j["nodes"].as_array().cloned().unwrap_or_default();
    let h = Hierarchy::new(&nodes);
    let local: Vec<M4> = nodes.iter().map(local_matrix).collect();
    let world = h.world(&local);
    // The default scene's nodes, parents first and children in their listed order.
    let roots: Vec<usize> = match j["scenes"].as_array() {
        Some(sc) if !sc.is_empty() => {
            let s = j["scene"].as_u64().unwrap_or(0) as usize;
            sc.get(s).ok_or_else(|| anyhow!("scene {s} does not exist"))?["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .map(|v| v as usize)
                .filter(|&v| v < nodes.len())
                .collect()
        }
        _ => (0..nodes.len()).filter(|&i| h.parent[i] == usize::MAX).collect(),
    };
    let mut ours = vec![usize::MAX; nodes.len()];
    let mut out_nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<(usize, Option<usize>)> = roots.iter().rev().map(|&r| (r, None)).collect();
    while let Some((i, parent)) = stack.pop() {
        if ours[i] != usize::MAX {
            continue;
        }
        ours[i] = out_nodes.len();
        out_nodes.push(Node {
            name: nodes[i]["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("node{i}")),
            parent,
            world: DMat4::from_cols_array(&world[i]),
        });
        let kids: Vec<usize> = nodes[i]["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|c| c as usize)
            .filter(|&c| c < nodes.len())
            .collect();
        for &c in kids.iter().rev() {
            stack.push((c, Some(ours[i])));
        }
    }

    // Materials, one per glTF material, and the images base colours use.
    let textures = j["textures"].as_array().cloned().unwrap_or_default();
    let mut base_images = BTreeSet::new();
    let mut materials = Vec::new();
    for (mi, m) in j["materials"].as_array().into_iter().flatten().enumerate() {
        let pbr = &m["pbrMetallicRoughness"];
        let base = arr(&pbr["baseColorFactor"], [1.0; 4]).map(|v| v as f32);
        let strength = m["extensions"]["KHR_materials_emissive_strength"]["emissiveStrength"].as_f64().unwrap_or(1.0);
        let emissive = arr(&m["emissiveFactor"], [0.0; 3]).map(|v| (v * strength) as f32);
        let alpha = match m["alphaMode"].as_str().unwrap_or("OPAQUE") {
            "MASK" => Alpha::Mask(m["alphaCutoff"].as_f64().unwrap_or(0.5) as f32),
            "BLEND" => Alpha::Blend,
            _ => Alpha::Opaque,
        };
        let bt = &pbr["baseColorTexture"];
        let base_tex = match bt["index"].as_u64() {
            Some(t) => {
                let tex = textures.get(t as usize).ok_or_else(|| anyhow!("material {mi}: texture {t} does not exist"))?;
                match tex["source"].as_u64() {
                    Some(img) => {
                        let (xform, over) = tex_xform(bt);
                        base_images.insert(img as usize);
                        Some(TexRef {
                            image: img as usize,
                            uv_set: over.unwrap_or(bt["texCoord"].as_u64().unwrap_or(0) as u32),
                            xform,
                        })
                    }
                    None => {
                        info.warnings.push(format!(
                            "material {mi}: its base texture has no PNG or JPEG source (an extension's format): colour factors only"
                        ));
                        None
                    }
                }
            }
            None => None,
        };
        materials.push(Material {
            name: m["name"].as_str().map(str::to_string).unwrap_or_else(|| format!("material{mi}")),
            base,
            metallic: pbr["metallicFactor"].as_f64().unwrap_or(1.0) as f32,
            roughness: pbr["roughnessFactor"].as_f64().unwrap_or(1.0) as f32,
            emissive,
            alpha,
            double_sided: m["doubleSided"].as_bool().unwrap_or(false),
            unlit: m["extensions"].get("KHR_materials_unlit").is_some(),
            base_tex,
        });
    }
    let img_list = j["images"].as_array().cloned().unwrap_or_default();
    if img_list.len() > MAX_IMAGES {
        bail!("{} images, over the {MAX_IMAGES} limit", img_list.len());
    }
    let mut images: Vec<Image> = Vec::new();
    for (ii, im) in img_list.iter().enumerate() {
        let name = im["name"]
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                im["uri"].as_str().filter(|u| !u.starts_with("data:")).map(|u| u.rsplit('/').next().unwrap_or(u).to_string())
            })
            .unwrap_or_else(|| format!("image{ii}"));
        let bytes = match (im["bufferView"].as_u64(), im["uri"].as_str()) {
            (Some(v), _) => g.view(v as usize).map(<[u8]>::to_vec),
            (None, Some(uri)) => uri_bytes(&g.dir, uri),
            _ => Err(anyhow!("no bufferView or uri")),
        };
        match bytes {
            Ok(b) => images.push(image::read(&name, &b, base_images.contains(&ii), &mut info.warnings)),
            Err(e) => {
                info.warnings.push(format!("image {name}: {e}"));
                images.push(Image { name, kind: "missing".into(), ..Image::default() });
            }
        }
    }
    for m in &mut materials {
        if m.base_tex.is_some_and(|t| t.image >= images.len()) {
            info.warnings.push(format!("material {}: its base texture names a missing image", m.name));
            m.base_tex = None;
        }
    }

    // Meshes, one per triangle primitive of each scene node, in world space.
    let mut meshes = Vec::new();
    let mut tris_total = 0usize;
    let mut skipped_modes = BTreeSet::new();
    let skins = j["skins"].as_array().cloned().unwrap_or_default();
    for (gi, n) in nodes.iter().enumerate() {
        let (Some(mi), si) = (n["mesh"].as_u64(), ours[gi]) else { continue };
        if si == usize::MAX {
            continue;
        }
        let mesh = j["meshes"].get(mi as usize).ok_or_else(|| anyhow!("node {gi}: mesh {mi} does not exist"))?;
        let skin = n["skin"].as_u64().and_then(|s| skins.get(s as usize));
        if skin.is_some() {
            info.skinned = true;
        }
        for (pi, prim) in mesh["primitives"].as_array().into_iter().flatten().enumerate() {
            let at = format!("mesh {mi} primitive {pi}");
            let mode = prim["mode"].as_u64().unwrap_or(4);
            if !(4..=6).contains(&mode) {
                skipped_modes.insert(mode);
                continue;
            }
            let attrs = &prim["attributes"];
            let Some(pa) = attrs["POSITION"].as_u64() else {
                info.warnings.push(format!("{at}: no POSITION, skipped"));
                continue;
            };
            let (pos, _) = g.read(pa as usize).map_err(|e| anyhow!("{at}: {e}"))?;
            let nv = pos.len() / 3;
            let idx: Vec<u32> = match prim["indices"].as_u64() {
                Some(ia) => g.read(ia as usize).map_err(|e| anyhow!("{at}: {e}"))?.0.iter().map(|&v| v as u32).collect(),
                None => (0..nv as u32).collect(),
            };
            if let Some(bad) = idx.iter().find(|&&v| v as usize >= nv) {
                bail!("{at}: index {bad} is past its {nv} vertices");
            }
            let mut tris: Vec<[u32; 3]> = match mode {
                4 => idx.as_chunks::<3>().0.to_vec(),
                5 => (2..idx.len())
                    .map(|k| if k % 2 == 0 { [idx[k - 2], idx[k - 1], idx[k]] } else { [idx[k - 1], idx[k - 2], idx[k]] })
                    .collect(),
                _ => (2..idx.len()).map(|k| [idx[0], idx[k - 1], idx[k]]).collect(),
            };
            tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
            tris_total += tris.len();
            if tris_total > MAX_TRIS {
                bail!("over {MAX_TRIS} triangles");
            }
            let get = |k: &str, want: usize| -> Result<Option<Vec<f64>>> {
                match attrs[k].as_u64() {
                    Some(a) => {
                        let (v, n) = g.read(a as usize).map_err(|e| anyhow!("{at} {k}: {e}"))?;
                        if v.len() / n.max(1) != nv || (want > 0 && n != want) {
                            bail!("{at}: {k} has {} elements of {n}, for {nv} vertices", v.len() / n.max(1));
                        }
                        Ok(Some(v))
                    }
                    None => Ok(None),
                }
            };
            let nrm = get("NORMAL", 3)?;
            let uv = get("TEXCOORD_0", 2)?;
            let uv1 = get("TEXCOORD_1", 2)?;
            let col = match attrs["COLOR_0"].as_u64() {
                Some(a) => {
                    let (v, n) = g.read(a as usize).map_err(|e| anyhow!("{at} COLOR_0: {e}"))?;
                    if v.len() / n.max(1) != nv || !(3..=4).contains(&n) {
                        bail!("{at}: COLOR_0 does not match its vertices");
                    }
                    Some(
                        v.chunks(n)
                            .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32, if n == 4 { c[3] as f32 } else { 1.0 }])
                            .collect(),
                    )
                }
                None => None,
            };
            // Where each vertex goes: the node's world matrix, or for a skinned mesh its joints'
            // bind matrices blended by weight.
            let node_m = DMat4::from_cols_array(&world[gi]);
            let blend = match skin {
                Some(sk) => {
                    skin_matrices(g, sk, &world, &attrs["JOINTS_0"], &attrs["WEIGHTS_0"], nv).map_err(|e| anyhow!("{at}: {e}"))?
                }
                None => None,
            };
            let m_of = |v: usize| -> DMat4 { blend.as_ref().map_or(node_m, |b| b[v]) };
            let node_nm = normal_matrix(node_m);
            let nm_of = |v: usize| blend.as_ref().map_or(node_nm, |b| normal_matrix(b[v]));
            let mut out = Mesh {
                node: si,
                material: prim["material"].as_u64().map(|m| m as usize).filter(|&m| m < materials.len()),
                ..Mesh::default()
            };
            out.pos = (0..nv)
                .map(|v| m_of(v).transform_point3(DVec3::new(pos[3 * v], pos[3 * v + 1], pos[3 * v + 2])).as_vec3().to_array())
                .collect();
            if let Some(nr) = nrm {
                out.nrm = (0..nv)
                    .map(|v| {
                        (nm_of(v) * DVec3::new(nr[3 * v], nr[3 * v + 1], nr[3 * v + 2])).normalize_or_zero().as_vec3().to_array()
                    })
                    .collect();
            }
            out.uv = uv.map(|u| u.chunks(2).map(|c| [c[0] as f32, c[1] as f32]).collect()).unwrap_or_default();
            out.uv1 = uv1.map(|u| u.chunks(2).map(|c| [c[0] as f32, c[1] as f32]).collect()).unwrap_or_default();
            out.col = col.unwrap_or_default();
            // A mirroring placement turns triangles inside out: reverse them.
            if blend.is_none() && node_m.determinant() < 0.0 {
                for t in &mut tris {
                    t.swap(1, 2);
                }
            }
            out.tris = tris;
            meshes.push(out);
        }
    }
    if !skipped_modes.is_empty() {
        info.warnings.push(format!("points and lines are skipped (modes {skipped_modes:?})"));
    }
    if info.skinned {
        info.warnings.push("skinned: read in its bind pose (the creature and character domain; skipped by surveys' fits)".into());
    }
    Ok(Scene { nodes: out_nodes, meshes, materials, images, info })
}

/// The bind-pose matrix of each vertex of a skinned primitive: its joints' world · inverse bind
/// matrices blended by weight. None when the primitive has no joints or weights.
fn skin_matrices(g: &Gltf, skin: &Value, world: &[M4], joints: &Value, weights: &Value, nv: usize) -> Result<Option<Vec<DMat4>>> {
    let (Some(ja), Some(wa)) = (joints.as_u64(), weights.as_u64()) else { return Ok(None) };
    let list: Vec<usize> =
        skin["joints"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|v| v as usize).collect();
    let ibm: Vec<M4> = match skin["inverseBindMatrices"].as_u64() {
        Some(a) => g.read(a as usize)?.0.chunks(16).map(|c| std::array::from_fn(|i| c[i])).collect(),
        None => vec![IDENTITY; list.len()],
    };
    let jm: Vec<DMat4> =
        list.iter().zip(&ibm).map(|(&j, ib)| DMat4::from_cols_array(&mul(world.get(j).unwrap_or(&IDENTITY), ib))).collect();
    let (jv, jn) = g.read(ja as usize)?;
    let (wv, wn) = g.read(wa as usize)?;
    if jv.len() / jn.max(1) != nv || wv.len() / wn.max(1) != nv {
        bail!("JOINTS_0 or WEIGHTS_0 does not match its vertices");
    }
    let mut out = Vec::with_capacity(nv);
    for v in 0..nv {
        let mut m = DMat4::ZERO;
        let mut total = 0.0;
        for k in 0..jn.min(wn) {
            let w = wv[v * wn + k];
            if w > 0.0 {
                let j = jv[v * jn + k] as usize;
                let jmat = jm.get(j).ok_or_else(|| anyhow!("joint {j} is not in the skin"))?;
                m += *jmat * w;
                total += w;
            }
        }
        out.push(if total > 0.0 { m * (1.0 / total) } else { DMat4::IDENTITY });
    }
    Ok(Some(out))
}
