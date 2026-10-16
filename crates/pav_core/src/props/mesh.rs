//! Mesh parts (plan §5.5, D3): small quantized, vertex-coloured meshes that `asset_import`
//! writes as readable text, for the pieces of an imported model no primitive fits. A prop places
//! one with `{"mesh":"<pack>/<name>"}` (position, turn, scale, mirror, copies and `recolor`
//! like any part); nobody edits its vertices.
//!
//! The file, `<props root>/lib/<pack>/meshes/<name>.json` (or `local/<pack>/meshes/`):
//! `{"format":1,"name":..,"unit":"mm","tris":N,"budget":4096,"palette":["#rrggbb",..],
//! "v":[x,y,z,..],"f":[a,b,c,p,..]}`: integer millimetres in the part's frame, and per face three
//! vertex indices (counter-clockwise seen from outside) and a palette index. `"two_sided":true`
//! (after `budget`) marks a mesh with open surfaces seen from both sides (a double-sided source
//! material): its back faces are drawn lit, not as the dark cap of a cut-open solid.
//!
//! Meshes are found in the installed set first (the importer installs what it writes), then for
//! the core pack (`core/<name>`) in the files every build embeds (`lib/core/meshes/`), then on
//! disk under `$PAV_ASSETS` (else `assets/props`; native builds), and kept once read.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

use glam::Vec3;
use serde_json::{Map, Value, json};

use super::{fnv, validate_name};

/// Triangles one mesh file may have.
pub const MAX_TRIS: usize = 4_096;
/// Colours one mesh file may have.
pub const MAX_PALETTE: usize = 64;
/// Mesh parts one asset may have after copies, mirrors and references (each is drawn and
/// collides as a hull of its own).
pub const MAX_MESH_NODES: usize = 16;

/// A mesh file as written.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshFile {
    /// The file's leaf name.
    pub name: String,
    /// The triangle budget it was made within.
    pub budget: u32,
    /// Open surfaces seen from both sides: back faces are drawn lit.
    pub two_sided: bool,
    /// `#rrggbb` colours.
    pub palette: Vec<String>,
    /// Vertices in integer millimetres.
    pub v: Vec<[i32; 3]>,
    /// Faces: three vertex indices and a palette index.
    pub f: Vec<[u32; 4]>,
    /// Where it came from (`{library, title, ...}`), as the prop's `source`.
    pub source: Option<Map<String, Value>>,
}

impl MeshFile {
    /// Reads a mesh file, checking every index and limit.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let o = value.as_object().ok_or("a mesh file is a JSON object")?;
        for key in o.keys() {
            if !["format", "name", "unit", "tris", "budget", "two_sided", "palette", "v", "f", "source"].contains(&key.as_str()) {
                return Err(format!("unknown key {key}"));
            }
        }
        if o.get("format").and_then(Value::as_u64) != Some(1) {
            return Err("format must be 1".into());
        }
        if o.get("unit").and_then(Value::as_str) != Some("mm") {
            return Err("unit must be \"mm\"".into());
        }
        let name = o.get("name").and_then(Value::as_str).ok_or("missing name")?.to_string();
        validate_name(&name)?;
        let budget = o.get("budget").and_then(Value::as_u64).unwrap_or(MAX_TRIS as u64).min(MAX_TRIS as u64) as u32;
        let two_sided = match o.get("two_sided") {
            None => false,
            Some(v) => v.as_bool().ok_or("two_sided is true or false")?,
        };
        let palette: Vec<String> = o
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("missing palette")?
            .iter()
            .map(|c| c.as_str().filter(|c| super::is_color(c) && c.starts_with('#')).map(str::to_string))
            .collect::<Option<_>>()
            .ok_or("palette entries are #rrggbb")?;
        if palette.is_empty() || palette.len() > MAX_PALETTE {
            return Err(format!("{} palette colours (1 to {MAX_PALETTE})", palette.len()));
        }
        let ints = |key: &str| -> Result<Vec<i64>, String> {
            o.get(key)
                .and_then(Value::as_array)
                .ok_or(format!("missing {key}"))?
                .iter()
                .map(|n| n.as_i64().ok_or(format!("{key} holds integers")))
                .collect()
        };
        let v = ints("v")?;
        let f = ints("f")?;
        if v.len() % 3 != 0 || f.len() % 4 != 0 {
            return Err("v holds x,y,z triples and f a,b,c,palette quads".into());
        }
        let v: Vec<[i32; 3]> = v
            .chunks(3)
            .map(|c| {
                let ok = c.iter().all(|x| x.abs() <= 10_000_000);
                ok.then(|| [c[0] as i32, c[1] as i32, c[2] as i32])
            })
            .collect::<Option<_>>()
            .ok_or("vertices within 10 km")?;
        let f: Vec<[u32; 4]> = f
            .chunks(4)
            .map(|c| {
                let ok = c[..3].iter().all(|&i| i >= 0 && (i as usize) < v.len()) && c[3] >= 0 && (c[3] as usize) < palette.len();
                ok.then(|| [c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32])
            })
            .collect::<Option<_>>()
            .ok_or("a face names a vertex or colour that does not exist")?;
        if f.is_empty() || f.len() > MAX_TRIS {
            return Err(format!("{} triangles (1 to {MAX_TRIS})", f.len()));
        }
        if let Some(t) = o.get("tris").and_then(Value::as_u64)
            && t as usize != f.len()
        {
            return Err(format!("tris says {t} but f has {}", f.len()));
        }
        let source = o.get("source").and_then(Value::as_object).cloned();
        Ok(Self { name, budget, two_sided, palette, v, f, source })
    }

    /// The canonical text: a header line, then the palette, the vertices and the faces, each on
    /// its own line.
    pub fn text(&self) -> String {
        let nums = |it: &mut dyn Iterator<Item = i64>| it.map(|n| n.to_string()).collect::<Vec<_>>().join(",");
        let mut out = format!(
            "{{\"format\":1,\"name\":{},\"unit\":\"mm\",\"tris\":{},\"budget\":{},{}\n",
            json!(self.name),
            self.f.len(),
            self.budget,
            if self.two_sided { "\"two_sided\":true," } else { "" }
        );
        if let Some(source) = &self.source {
            out.push_str(&format!("\"source\":{},\n", Value::Object(source.clone())));
        }
        out.push_str(&format!("\"palette\":{},\n", json!(self.palette)));
        out.push_str(&format!("\"v\":[{}],\n", nums(&mut self.v.iter().flatten().map(|&x| x as i64))));
        out.push_str(&format!("\"f\":[{}]}}\n", nums(&mut self.f.iter().flatten().map(|&x| x as i64))));
        out
    }
}

/// A mesh ready to place: the file, its key, its vertices in metres and its box.
#[derive(Debug)]
pub struct LoadedMesh {
    pub file: MeshFile,
    /// FNV-1a of the canonical text: equal keys draw equal meshes.
    pub key: u64,
    pub positions: Vec<Vec3>,
    pub lo: Vec3,
    pub hi: Vec3,
}

impl LoadedMesh {
    pub fn new(file: MeshFile) -> Self {
        let key = fnv(file.text().as_bytes());
        let positions: Vec<Vec3> = file.v.iter().map(|v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) * 0.001).collect();
        let used = file.f.iter().flat_map(|f| [f[0], f[1], f[2]]).map(|i| positions[i as usize]);
        let (lo, hi) = used.fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Self { file, key, positions, lo, hi }
    }

    /// The palette as linear RGB.
    pub fn colours(&self) -> Vec<Vec3> {
        self.file.palette.iter().map(|hex| linear(hex)).collect()
    }
}

/// `#rrggbb` as linear RGB.
pub fn linear(hex: &str) -> Vec3 {
    Vec3::from_array(crate::color::Color::hex(hex).0)
}

/// Checks a mesh name: `<pack>/<name>`, the pack `[a-z][a-z0-9_]{0,31}`.
pub fn check_name(name: &str) -> Result<(&str, &str), String> {
    let (pack, leaf) = name.split_once('/').ok_or("a mesh is named <pack>/<name>")?;
    let mut chars = pack.bytes();
    if pack.len() > 32
        || !chars.next().is_some_and(|c| c.is_ascii_lowercase())
        || chars.any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != b'_')
    {
        return Err(format!("pack \"{pack}\": 1 to 32 lowercase letters, digits or underscores, starting with a letter"));
    }
    validate_name(leaf)?;
    Ok((pack, leaf))
}

fn registry() -> &'static RwLock<BTreeMap<String, Arc<LoadedMesh>>> {
    static MESHES: OnceLock<RwLock<BTreeMap<String, Arc<LoadedMesh>>>> = OnceLock::new();
    MESHES.get_or_init(Default::default)
}

/// Makes `file` the mesh `name` (`<pack>/<name>`), replacing any read before.
pub fn install(name: &str, file: MeshFile) -> Result<Arc<LoadedMesh>, String> {
    check_name(name)?;
    let mesh = Arc::new(LoadedMesh::new(file));
    registry().write().unwrap().insert(name.to_string(), mesh.clone());
    Ok(mesh)
}

/// The files a mesh name may be read from, in order: the shared tier, then the local one.
pub fn files(name: &str) -> Vec<std::path::PathBuf> {
    let Ok((pack, leaf)) = check_name(name) else { return Vec::new() };
    let root = std::env::var_os("PAV_ASSETS").map(std::path::PathBuf::from).unwrap_or_else(|| "assets/props".into());
    ["lib", "local"].iter().map(|tier| root.join(tier).join(pack).join("meshes").join(format!("{leaf}.json"))).collect()
}

/// The text of an embedded mesh: the core pack's (`core/<name>`, from `lib/core/meshes/`).
fn embedded(name: &str) -> Option<&'static str> {
    let (pack, leaf) = check_name(name).ok()?;
    (pack == super::store::CORE).then(|| super::store::embedded_file(&format!("lib/core/meshes/{leaf}.json")))?
}

/// A shipped pack's mesh (`super::store::SHIPPED`), which the build carries.
fn shipped(name: &str) -> Option<&'static str> {
    let (pack, leaf) = check_name(name).ok()?;
    super::store::SHIPPED.contains(&pack).then(|| super::store::embedded_file(&format!("lib/{pack}/meshes/{leaf}.json")))?
}

/// The mesh `name`: installed, else read once from the embedded core pack or (native) disk, and
/// a shipped pack's from the build when the disk lacks it.
pub fn get(name: &str) -> Option<Arc<LoadedMesh>> {
    if let Some(mesh) = registry().read().unwrap().get(name) {
        return Some(mesh.clone());
    }
    let text = match embedded(name) {
        Some(text) => text.to_string(),
        None if cfg!(target_arch = "wasm32") => shipped(name)?.to_string(),
        None => match files(name).into_iter().find_map(|path| std::fs::read_to_string(path).ok()) {
            Some(text) => text,
            None => shipped(name)?.to_string(),
        },
    };
    let file = MeshFile::parse(&text).ok()?;
    install(name, file).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> MeshFile {
        MeshFile {
            name: "tetra".into(),
            budget: 4096,
            two_sided: false,
            palette: vec!["#ff0000".into(), "#00ff00".into()],
            v: vec![[0, 0, 0], [100, 0, 0], [0, 100, 0], [0, 0, 100]],
            f: vec![[0, 2, 1, 0], [0, 1, 3, 1], [0, 3, 2, 0], [1, 2, 3, 1]],
            source: None,
        }
    }

    #[test]
    fn files_round_trip_and_bad_ones_are_refused() {
        let m = tetra();
        let text = m.text();
        assert_eq!(MeshFile::parse(&text).unwrap(), m);
        assert!(text.lines().count() == 4, "{text}");
        let bad = text.replace("[0,2,1,0,", "[0,2,9,0,");
        assert!(MeshFile::parse(&bad).unwrap_err().contains("does not exist"));
        assert!(MeshFile::parse(&text.replace("\"mm\"", "\"cm\"")).is_err());
        assert!(check_name("Kenney/barrel").is_err());
        assert!(check_name("kenney_md/barrel").is_ok());
        let two = MeshFile { two_sided: true, ..m.clone() };
        assert!(two.text().lines().next().unwrap().ends_with(r#""budget":4096,"two_sided":true,"#), "{}", two.text());
        assert_eq!(MeshFile::parse(&two.text()).unwrap(), two);
        let loaded = install("test_pack/tetra", m).unwrap();
        assert_eq!(loaded.hi, Vec3::splat(0.1));
        assert_eq!(get("test_pack/tetra").unwrap().key, loaded.key);
        assert!(get("test_pack/missing").is_none());
    }

    #[test]
    fn mesh_parts_read_compile_mirror_and_recolour() {
        use crate::props::geom::{Generated, Primitive};
        use crate::props::{Doc, PropDef, lint};
        install("test_parts/tetra", tetra()).unwrap();
        let text = r##"{"format":2,"name":"m","unit":"cm","materials":{"gold":{"color":"#ffcc00"}},
"parts":{"body":{"mesh":"test_parts/tetra","pos":[20,0,0],"mirror":"x","recolor":{"1":"gold"}},
"cap":{"type":"box","size":[2,2,2],"pos":[0,12,0],"color":"#808080"}}}"##;
        let doc = Doc::read(text).unwrap_or_else(|e| panic!("{}", crate::issue::render(&e)));
        assert!(doc.text().contains(r#""body":{"mesh":"test_parts/tetra""#), "{}", doc.text());
        let def = PropDef::new(doc).unwrap();
        assert!(def.key.starts_with("propk-"), "a mesh part keys its definition by the file");
        let parts = &def.compiled.parts;
        let (body, copy) = (parts.iter().find(|p| p.id == "body").unwrap(), parts.iter().find(|p| p.id == "body~x").unwrap());
        assert!(matches!(body.prim, Primitive::Mesh { .. }) && copy.mirrored);
        // The tetrahedron spans 0..10 cm: at x 20 cm and mirrored to -30..-20 cm.
        assert!((body.bounds(1.0).max.x - 0.3).abs() < 1e-6 && (copy.bounds(1.0).min.x + 0.3).abs() < 1e-6);
        let (plain, mirrored) = (Generated::of(&body.prim, false).unwrap(), Generated::of(&copy.prim, true).unwrap());
        assert_ne!(plain.key, mirrored.key);
        let mesh = plain.build();
        assert_eq!((mesh.triangles(), mesh.colors.len()), (4, 12));
        assert!(mesh.signed_volume() > 0.0 && mirrored.build().signed_volume() > 0.0, "both wind outward");
        // Palette entry 1 (green) became gold; entry 0 stays red.
        assert!(mesh.colors.contains(&linear("#ffcc00")) && mesh.colors.contains(&linear("#ff0000")));
        assert!(!mesh.colors.contains(&linear("#00ff00")));
        let _ = lint::lint(&def.compiled, &lint::Options::default());
        // Unknown meshes and bad recolours are errors that say what to do.
        let missing = Doc::read(&text.replace("test_parts/tetra", "test_parts/nope")).unwrap_err();
        assert!(crate::issue::render(&missing).contains("no mesh \"test_parts/nope\""));
        let bad = Doc::read(&text.replace(r#""1":"gold""#, r#""7":"gold""#)).unwrap_err();
        assert!(crate::issue::render(&bad).contains("recolor.7"), "{}", crate::issue::render(&bad));
        // Copies count: 9 × 2 mirrored is over the 16 mesh parts an asset may have.
        let many = Doc::read(&text.replace(r#""mirror":"x","#, r#""mirror":"x","array":{"count":9,"step":[0,0,20]},"#));
        let many = crate::issue::render(&many.unwrap_err());
        assert!(many.contains("18 mesh parts after copies") && many.contains("at most 16"), "{many}");
        let eight = text.replace(r#""mirror":"x","#, r#""mirror":"x","array":{"count":8,"step":[0,0,20]},"#);
        assert!(Doc::read(&eight).is_ok());
        // A decorative mesh part stays out of the collider.
        let loose = Doc::read(&text.replace(r#""mirror":"x","#, r#""mirror":"x","solid":false,"#)).unwrap();
        let loose = PropDef::new(loose).unwrap();
        assert!(loose.compiled.parts.iter().filter(|p| p.id.starts_with("body")).all(|p| !p.solid));
    }
}
