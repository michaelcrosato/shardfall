//! Builders for the importer's tests: glTF files, PNGs, OBJ text and zip archives made in code,
//! so no binary is committed.

#![allow(dead_code)]

use serde_json::{Value, json};

/// A binary glTF from its JSON and its BIN chunk.
pub fn glb(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut js = serde_json::to_vec(json).unwrap();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut out = Vec::new();
    for v in [0x4654_6c67u32, 2, (12 + 8 + js.len() + 8 + bin.len()) as u32, js.len() as u32, 0x4e4f_534a] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    out
}

/// A PNG of `w`×`h` RGBA pixels (row 0 at the top).
pub fn png_rgba(w: u32, h: u32, px: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(px.len(), (w * h) as usize);
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&px.iter().flatten().copied().collect::<Vec<u8>>()).unwrap();
    }
    out
}

/// Base64 (standard, padded), for `data:` URIs.
pub fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// A zip archive of `(name, bytes, deflate)` entries; `bad_crc` names an entry whose recorded
/// CRC-32 is wrong.
pub fn zip(entries: &[(&str, &[u8], bool)], bad_crc: Option<&str>) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for &(name, data, deflate) in entries {
        let packed = if deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
        let mut crc = pav_tools::import::fetch::crc32(data);
        if bad_crc == Some(name) {
            crc ^= 1;
        }
        let at = out.len() as u32;
        let method = if deflate { 8 } else { 0 };
        le32(&mut out, 0x0403_4b50);
        for x in [20u16, 0, method, 0, 0] {
            le16(&mut out, x);
        }
        for x in [crc, packed.len() as u32, data.len() as u32] {
            le32(&mut out, x);
        }
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);
        le32(&mut central, 0x0201_4b50);
        for x in [20u16, 20, 0, method, 0, 0] {
            le16(&mut central, x);
        }
        for x in [crc, packed.len() as u32, data.len() as u32] {
            le32(&mut central, x);
        }
        for x in [name.len() as u16, 0, 0, 0, 0] {
            le16(&mut central, x);
        }
        le32(&mut central, 0);
        le32(&mut central, at);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_at = out.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4b50);
    for x in [0u16, 0, entries.len() as u16, entries.len() as u16] {
        le16(&mut out, x);
    }
    le32(&mut out, central.len() as u32);
    le32(&mut out, cd_at);
    le16(&mut out, 0);
    out
}

/// A box from `min` to `max`: 24 vertices (flat normals), 12 triangles facing out.
pub fn cuboid(min: [f32; 3], max: [f32; 3]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let c = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0];
    let h = [(max[0] - min[0]) / 2.0, (max[1] - min[1]) / 2.0, (max[2] - min[2]) / 2.0];
    // (normal axis, sign, u axis, v axis) with u × v = normal.
    let faces = [(0, 1.0, 1, 2), (0, -1.0, 2, 1), (1, 1.0, 2, 0), (1, -1.0, 0, 2), (2, 1.0, 0, 1), (2, -1.0, 1, 0)];
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for (n, s, u, v) in faces {
        let base = pos.len() as u32;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let mut p = c;
            p[n] += s * h[n];
            p[u] += su * h[u];
            p[v] += sv * h[v];
            pos.push(p);
            let mut q = [0.0; 3];
            q[n] = s;
            nrm.push(q);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (pos, nrm, idx)
}

/// glTF made in code: accessors over one buffer, meshes, nodes, materials, textures and
/// embedded images; written as .glb, .gltf with a data URI, or .gltf with a .bin beside it.
#[derive(Default)]
pub struct MeshBuilder {
    pub bin: Vec<u8>,
    pub views: Vec<Value>,
    pub accessors: Vec<Value>,
    pub meshes: Vec<Value>,
    pub nodes: Vec<Value>,
    pub materials: Vec<Value>,
    pub textures: Vec<Value>,
    pub images: Vec<Value>,
    /// Extra top-level members (extensionsUsed, extensionsRequired, skins...).
    pub extra: serde_json::Map<String, Value>,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.views.push(json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()}));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    /// Floats as an accessor of `ty` (VEC3, VEC2, SCALAR, MAT4...).
    pub fn floats(&mut self, data: &[f32], ty: &str) -> usize {
        let n = match ty {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 16,
        };
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessors.push(json!({"bufferView": v, "componentType": 5126, "count": data.len() / n, "type": ty}));
        self.accessors.len() - 1
    }

    pub fn vec3(&mut self, data: &[[f32; 3]]) -> usize {
        self.floats(&data.iter().flatten().copied().collect::<Vec<_>>(), "VEC3")
    }

    pub fn vec2(&mut self, data: &[[f32; 2]]) -> usize {
        self.floats(&data.iter().flatten().copied().collect::<Vec<_>>(), "VEC2")
    }

    /// Indices as unsigned shorts.
    pub fn indices(&mut self, idx: &[u32]) -> usize {
        let bytes: Vec<u8> = idx.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect();
        let v = self.view(&bytes);
        self.accessors.push(json!({"bufferView": v, "componentType": 5123, "count": idx.len(), "type": "SCALAR"}));
        self.accessors.len() - 1
    }

    /// Normalised unsigned bytes (colours).
    pub fn unorm8(&mut self, data: &[u8], ty: &str) -> usize {
        let n = if ty == "VEC4" { 4 } else { 3 };
        let v = self.view(data);
        self.accessors
            .push(json!({"bufferView": v, "componentType": 5121, "normalized": true, "count": data.len() / n, "type": ty}));
        self.accessors.len() - 1
    }

    /// A primitive's JSON from its position and index accessors and extra attributes.
    pub fn prim(&self, pos: usize, idx: Option<usize>, attrs: &[(&str, usize)], material: Option<usize>) -> Value {
        let mut a = json!({"POSITION": pos});
        for (k, v) in attrs {
            a[*k] = json!(v);
        }
        let mut p = json!({"attributes": a});
        if let Some(i) = idx {
            p["indices"] = json!(i);
        }
        if let Some(m) = material {
            p["material"] = json!(m);
        }
        p
    }

    /// A box primitive (positions, normals, indices).
    pub fn box_prim(&mut self, min: [f32; 3], max: [f32; 3], material: Option<usize>) -> Value {
        let (p, n, i) = cuboid(min, max);
        let (pa, na, ia) = (self.vec3(&p), self.vec3(&n), self.indices(&i));
        self.prim(pa, Some(ia), &[("NORMAL", na)], material)
    }

    pub fn mesh(&mut self, prims: Vec<Value>) -> usize {
        self.meshes.push(json!({"primitives": prims}));
        self.meshes.len() - 1
    }

    pub fn node(&mut self, n: Value) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    pub fn material(&mut self, m: Value) -> usize {
        self.materials.push(m);
        self.materials.len() - 1
    }

    /// A PNG embedded in the buffer, and a texture of it.
    pub fn png_texture(&mut self, png: &[u8], name: &str) -> usize {
        let v = self.view(png);
        self.images.push(json!({"bufferView": v, "mimeType": "image/png", "name": name}));
        self.textures.push(json!({"source": self.images.len() - 1}));
        self.textures.len() - 1
    }

    /// The glTF JSON with `buffer` as its one buffer, the scene's roots being `roots`.
    pub fn json(&self, roots: &[usize], buffer: Value) -> Value {
        let mut j = json!({
            "asset": {"version": "2.0", "generator": "pav_tools tests"},
            "scene": 0,
            "scenes": [{"nodes": roots}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": [buffer],
        });
        for (k, v) in [("materials", &self.materials), ("textures", &self.textures), ("images", &self.images)] {
            if !v.is_empty() {
                j[k] = json!(v);
            }
        }
        for (k, v) in &self.extra {
            j[k] = v.clone();
        }
        j
    }

    pub fn glb(&self, roots: &[usize]) -> Vec<u8> {
        glb(&self.json(roots, json!({"byteLength": self.bin.len()})), &self.bin)
    }

    /// A .gltf whose buffer is a `data:` URI.
    pub fn gltf_embedded(&self, roots: &[usize]) -> String {
        let uri = format!("data:application/octet-stream;base64,{}", base64(&self.bin));
        serde_json::to_string_pretty(&self.json(roots, json!({"byteLength": self.bin.len(), "uri": uri}))).unwrap()
    }

    /// A .gltf naming `bin` beside it, and the bytes of that file.
    pub fn gltf_external(&self, roots: &[usize], bin: &str) -> (String, Vec<u8>) {
        let j = self.json(roots, json!({"byteLength": self.bin.len(), "uri": bin}));
        (serde_json::to_string_pretty(&j).unwrap(), self.bin.clone())
    }
}

/// A fresh empty folder under the system temp folder.
pub fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pav_import_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
