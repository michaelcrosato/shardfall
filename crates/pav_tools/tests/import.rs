//! The asset importer's reading half: glTF (.glb, .gltf with data URIs and .bin files) and OBJ
//! read into one `Scene`, normalised into the engine's frame, and the `asset_import` tool's
//! read-only surface. Every model here is built in code (tests/common); nothing binary is
//! committed and nothing needs the network.

mod common;

use std::path::Path;

use common::{MeshBuilder, cuboid, png_rgba, temp_dir, zip};
use glam::DVec3;
use pav_tools::import::fetch::RemoteZip;
use pav_tools::import::normalize::{self, Norm, Place};
use pav_tools::import::obj::tex_map;
use pav_tools::import::scene::{Alpha, Scene, Wrap, XFORM_IDENTITY, srgb_to_linear};
use pav_tools::import::{gltf, read_model};
use pav_tools::{Output, Session, call};
use serde_json::{Value, json};

fn write(dir: &Path, name: &str, bytes: impl AsRef<[u8]>) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn tool(s: &mut Session, args: Value) -> anyhow::Result<Value> {
    match call(s, "asset_import", args.as_object().unwrap())? {
        Output::Json(v) => Ok(v),
        Output::Image { .. } => panic!("asset_import returns data"),
    }
}

fn close(a: [f32; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] as f64 - b[k]).abs() < 1e-5)
}

/// Every triangle's winding agrees with its vertex normals.
fn outward(s: &Scene) -> bool {
    normalize::normal_agreement(s) == 1.0
}

/// A table: a top and four legs under a node moved, turned and scaled; a red material.
fn table() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    let red = b.material(
        json!({"name": "red", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.1, 0.1, 1.0], "metallicFactor": 0.0}}),
    );
    let top = b.box_prim([-0.5, 0.7, -0.3], [0.5, 0.75, 0.3], Some(red));
    let mut prims = vec![top];
    for (x, z) in [(-0.45, -0.25), (0.45, -0.25), (-0.45, 0.25), (0.45, 0.25)] {
        prims.push(b.box_prim([x - 0.03, 0.0, z - 0.03], [x + 0.03, 0.7, z + 0.03], Some(red)));
    }
    let m = b.mesh(prims);
    let child = b.node(json!({"name": "table", "mesh": m, "scale": [2.0, 2.0, 2.0]}));
    // A quarter turn about +y and a move, on the parent.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    b.node(json!({"name": "root", "children": [child], "translation": [1.0, 0.0, 0.0], "rotation": [0.0, s, 0.0, s]}));
    b
}

#[test]
fn a_gltf_reads_back_exactly_in_every_container() {
    let dir = temp_dir("containers");
    let b = table();
    let glb = write(&dir, "table.glb", b.glb(&[1]));
    let embedded = write(&dir, "embedded.gltf", b.gltf_embedded(&[1]));
    let (text, bin) = b.gltf_external(&[1], "table%20data.bin");
    write(&dir, "table data.bin", bin);
    let external = write(&dir, "external.gltf", text);
    let scenes: Vec<Scene> = [&glb, &embedded, &external].iter().map(|p| read_model(p).unwrap()).collect();
    let s = &scenes[0];
    assert_eq!((s.info.format.as_str(), scenes[1].info.format.as_str()), ("glb", "gltf"));
    assert_eq!(s.nodes.len(), 2);
    assert_eq!((s.nodes[0].name.as_str(), s.nodes[1].parent), ("root", Some(0)));
    assert_eq!(s.meshes.len(), 5);
    assert_eq!(s.tris(), 60);
    // The top's corner (0.5, 0.75, 0.3): scaled ×2, turned a quarter about +y (x → −z), moved +1 x.
    let top = &s.meshes[0];
    let (p, _, _) = cuboid([-0.5, 0.7, -0.3], [0.5, 0.75, 0.3]);
    let corner = p.iter().position(|c| *c == [0.5, 0.75, 0.3]).unwrap();
    assert!(close(top.pos[corner], [1.0 + 0.6, 1.5, -1.0]), "{:?}", top.pos[corner]);
    let (lo, hi) = s.bounds().unwrap();
    assert!((lo - DVec3::new(0.4, 0.0, -1.0)).length() < 1e-5 && (hi - DVec3::new(1.6, 1.5, 1.0)).length() < 1e-5, "{lo} {hi}");
    assert!(outward(s));
    assert_eq!(s.materials[0].base, [0.8, 0.1, 0.1, 1.0]);
    assert_eq!(s.materials[0].metallic, 0.0);
    // The three containers read the same numbers.
    for o in &scenes[1..] {
        assert_eq!(o.tris(), s.tris());
        for (a, b) in o.meshes.iter().zip(&s.meshes) {
            assert_eq!((&a.pos, &a.nrm, &a.tris), (&b.pos, &b.nrm, &b.tris));
        }
    }
}

#[test]
fn a_mirroring_node_keeps_faces_outward() {
    let mut b = MeshBuilder::new();
    let p = b.box_prim([0.0, 0.0, 0.0], [1.0, 2.0, 0.5], None);
    let m = b.mesh(vec![p]);
    b.node(json!({"name": "mirrored", "mesh": m, "scale": [-1.0, 1.0, 1.0]}));
    let dir = temp_dir("mirror");
    let s = read_model(&write(&dir, "m.glb", b.glb(&[0]))).unwrap();
    assert!(outward(&s), "the reader reverses a mirrored node's triangles");
    let (lo, hi) = s.bounds().unwrap();
    assert_eq!((lo.x, hi.x), (-1.0, 0.0));
    // And normalisation flips a primitive written inside out.
    let mut b = MeshBuilder::new();
    let (pos, nrm, mut idx) = cuboid([0.0; 3], [1.0; 3]);
    for t in idx.chunks_mut(3) {
        t.swap(1, 2);
    }
    let (pa, na, ia) = (b.vec3(&pos), b.vec3(&nrm), b.indices(&idx));
    let prim = b.prim(pa, Some(ia), &[("NORMAL", na)], None);
    let m = b.mesh(vec![prim]);
    b.node(json!({"mesh": m}));
    let mut s = read_model(&write(&dir, "inside_out.glb", b.glb(&[0]))).unwrap();
    assert!(!outward(&s));
    let done = normalize::normalize(&mut s, &Norm::default());
    assert_eq!(done.flipped, vec![0]);
    assert!(outward(&s));
}

#[test]
fn texels_and_vertex_colours_give_exact_face_colours() {
    // A 2×2 palette texture; each face of a quad samples one texel at its middle.
    let px = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]];
    let png = png_rgba(2, 2, &px);
    let mut b = MeshBuilder::new();
    let tex = b.png_texture(&png, "palette");
    let mat = b.material(json!({"name": "pal", "pbrMetallicRoughness": {"baseColorTexture": {"index": tex}}}));
    // Two triangles side by side, all of each one's corners on one texel.
    let pos = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 1.0, 0.0]];
    let uv = [[0.25, 0.25]; 3].into_iter().chain([[0.75, 0.75]; 3]).collect::<Vec<_>>();
    let (pa, ua) = (b.vec3(&pos), b.vec2(&uv));
    let prim = b.prim(pa, None, &[("TEXCOORD_0", ua)], Some(mat));
    // A third triangle in vertex colours (normalised bytes), no material.
    let (cp, cc) =
        (b.vec3(&[[0.0, 2.0, 0.0], [1.0, 2.0, 0.0], [0.0, 3.0, 0.0]]), b.unorm8(&[128, 64, 255, 255].repeat(3), "VEC4"));
    let vprim = b.prim(cp, None, &[("COLOR_0", cc)], None);
    let m = b.mesh(vec![prim, vprim]);
    b.node(json!({"mesh": m}));
    let dir = temp_dir("colours");
    let s = read_model(&write(&dir, "pal.glb", b.glb(&[0]))).unwrap();
    assert_eq!((s.images[0].w, s.images[0].h, s.images[0].rgba.len()), (2, 2, 16));
    assert_eq!(normalize::face_colour(&s, 0, 0), [1.0, 0.0, 0.0, 1.0], "top-left texel: red");
    assert_eq!(normalize::face_colour(&s, 0, 1), [1.0, 1.0, 1.0, 1.0], "bottom-right texel: white");
    let v = normalize::face_colour(&s, 1, 0);
    let want = [128.0 / 255.0, 64.0 / 255.0, 1.0];
    assert!((0..3).all(|k| (v[k] - want[k]).abs() < 1e-6), "{v:?}");
    // Equal areas: red, white and the vertex colour a third each.
    let pal = normalize::palette(&s, 6);
    assert_eq!(pal.iter().map(|(h, _)| h.as_str()).collect::<Vec<_>>(), ["#bc89ff", "#ff0000", "#ffffff"]);
    assert!(pal.iter().all(|(_, pct)| (pct - 33.3).abs() < 0.1), "{pal:?}");
    let _ = srgb_to_linear(0);
}

#[test]
fn modes_sparse_accessors_materials_and_transforms_read() {
    let mut b = MeshBuilder::new();
    // A strip of 4 vertices (2 triangles) and a fan of 5 (3 triangles).
    let strip = b.vec3(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]);
    let fan = b.vec3(&[[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [-1.0, 0.5, 1.0]]);
    let lines = b.vec3(&[[0.0; 3], [1.0; 3]]);
    // Sparse: three zeros of which index 1 becomes (5, 6, 7).
    let base = b.vec3(&[[0.0; 3]; 3]);
    let si = b.indices(&[1]);
    let sv = b.vec3(&[[5.0, 6.0, 7.0]]);
    let (siv, svv) = (b.accessors[si]["bufferView"].clone(), b.accessors[sv]["bufferView"].clone());
    b.accessors[base]["sparse"] =
        json!({"count": 1, "indices": {"bufferView": siv, "componentType": 5123}, "values": {"bufferView": svv}});
    let glow = b.material(json!({
        "name": "glow", "emissiveFactor": [1.0, 0.5, 0.0], "alphaMode": "MASK", "alphaCutoff": 0.3, "doubleSided": true,
        "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 4.0}, "KHR_materials_unlit": {}},
    }));
    let png = png_rgba(1, 1, &[[10, 20, 30, 128]]);
    let tex = b.png_texture(&png, "t");
    let moved = b.material(json!({"name": "moved", "alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorTexture": {
        "index": tex, "extensions": {"KHR_texture_transform": {"offset": [0.5, 0.25], "scale": [2.0, 3.0], "rotation": 0.0, "texCoord": 1}}}}}));
    let mut p_strip = b.prim(strip, None, &[], Some(glow));
    p_strip["mode"] = json!(5);
    let mut p_fan = b.prim(fan, None, &[], Some(moved));
    p_fan["mode"] = json!(6);
    let mut p_lines = b.prim(lines, None, &[], None);
    p_lines["mode"] = json!(1);
    let p_sparse = b.prim(base, None, &[], None);
    let m = b.mesh(vec![p_strip, p_fan, p_lines, p_sparse]);
    b.node(json!({"mesh": m}));
    b.extra.insert(
        "extensionsUsed".into(),
        json!(["KHR_texture_transform", "KHR_materials_unlit", "KHR_materials_emissive_strength"]),
    );
    let dir = temp_dir("modes");
    let s = read_model(&write(&dir, "modes.glb", b.glb(&[0]))).unwrap();
    assert_eq!(s.meshes.len(), 3, "lines are skipped");
    assert!(s.info.warnings.iter().any(|w| w.contains("lines")), "{:?}", s.info.warnings);
    assert_eq!(s.meshes[0].tris, vec![[0, 1, 2], [2, 1, 3]], "strip winding alternates");
    assert_eq!(s.meshes[1].tris, vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]]);
    assert_eq!(s.meshes[2].pos, vec![[0.0; 3], [5.0, 6.0, 7.0], [0.0; 3]], "the sparse value replaces element 1");
    let g = &s.materials[0];
    assert_eq!((g.emissive, g.alpha, g.double_sided, g.unlit), ([4.0, 2.0, 0.0], Alpha::Mask(0.3), true, true));
    let t = s.materials[1].base_tex.unwrap();
    assert_eq!((t.uv_set, t.xform), (1, [2.0, 0.0, 0.5, 0.0, 3.0, 0.25]));
    assert_eq!(s.materials[1].alpha, Alpha::Blend);
    assert_eq!(s.info.extensions.len(), 3);
}

#[test]
fn a_skinned_mesh_reads_in_its_bind_pose() {
    let mut b = MeshBuilder::new();
    let (pos, nrm, idx) = cuboid([-0.1, 0.0, -0.1], [0.1, 1.0, 0.1]);
    let (pa, na, ia) = (b.vec3(&pos), b.vec3(&nrm), b.indices(&idx));
    let joints = b.unorm8(&[0u8, 0, 0, 0].repeat(pos.len()), "VEC4");
    b.accessors[joints]["normalized"] = json!(false);
    let weights = b.floats(&[1.0f32, 0.0, 0.0, 0.0].repeat(pos.len()), "VEC4");
    // The joint sits 2 m up; its inverse bind matrix undoes that, so the bind pose is the mesh.
    let ibm = b.floats(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -2.0, 0.0, 1.0], "MAT4");
    let prim = b.prim(pa, Some(ia), &[("NORMAL", na), ("JOINTS_0", joints), ("WEIGHTS_0", weights)], None);
    let m = b.mesh(vec![prim]);
    let joint = b.node(json!({"name": "bone", "translation": [0.0, 2.0, 0.0]}));
    // The skinned node's own transform is ignored by skinning.
    let body = b.node(json!({"name": "body", "mesh": m, "skin": 0, "translation": [9.0, 9.0, 9.0]}));
    b.extra.insert("skins".into(), json!([{"joints": [joint], "inverseBindMatrices": ibm}]));
    let dir = temp_dir("skin");
    let s = read_model(&write(&dir, "skin.glb", b.glb(&[joint, body]))).unwrap();
    assert!(s.info.skinned);
    let (lo, hi) = s.bounds().unwrap();
    assert!((lo - DVec3::new(-0.1, 0.0, -0.1)).length() < 1e-6 && (hi - DVec3::new(0.1, 1.0, 0.1)).length() < 1e-6, "{lo} {hi}");
}

#[test]
fn parts_are_connected_pieces_welded_across_split_normals() {
    let mut b = MeshBuilder::new();
    let (mut p, mut n, mut i) = cuboid([0.0; 3], [1.0; 3]);
    let (p2, n2, i2) = cuboid([2.0, 0.0, 0.0], [3.0, 1.0, 1.0]);
    i.extend(i2.iter().map(|k| k + p.len() as u32));
    p.extend(p2);
    n.extend(n2);
    let (pa, na, ia) = (b.vec3(&p), b.vec3(&n), b.indices(&i));
    let prim = b.prim(pa, Some(ia), &[("NORMAL", na)], None);
    let m = b.mesh(vec![prim]);
    b.node(json!({"mesh": m}));
    let dir = temp_dir("parts");
    let s = read_model(&write(&dir, "two.glb", b.glb(&[0]))).unwrap();
    // Each box's 24 vertices (4 a face) weld into 8 corners: one part each.
    let parts = normalize::parts(&s);
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|p| p.tris.len() == 12 && (p.area - 6.0).abs() < 1e-9), "{parts:?}");
}

#[test]
fn an_obj_reads_with_its_mtl_colours_and_texture() {
    let dir = temp_dir("obj");
    write(&dir, "wood.png", png_rgba(1, 1, &[[200, 120, 40, 255]]));
    write(&dir, "crate.mtl", "newmtl paint\nKd 0.2 0.4 0.6\nKe 0.1 0 0\nd 0.5\n\nnewmtl wood\nKd 1 1 1\nmap_Kd wood.png\n");
    // A quad (two triangles) in paint and a triangle in wood, two objects; v up as OBJ has it.
    let obj = "mtllib crate.mtl\no lid\nv 0 0 0\nv 1 0 0\nv 1 0 1\nv 0 0 1\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 1 0\nusemtl paint\nf 1/1/1 4/4/1 3/3/1 2/2/1\no side\nv 0 0 0\nv 0 1 0\nv 0 0 1\nusemtl wood\nf 5/1 7/2 6/3\n";
    let s = read_model(&write(&dir, "crate.obj", obj)).unwrap();
    assert_eq!(s.info.format, "obj");
    assert_eq!(s.nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["lid", "side"]);
    assert_eq!(s.tris(), 3);
    let paint = &s.materials[s.meshes[0].material.unwrap()];
    assert_eq!((paint.base, paint.emissive, paint.alpha), ([0.2, 0.4, 0.6, 0.5], [0.1, 0.0, 0.0], Alpha::Blend));
    assert_eq!(s.meshes[0].uv[0], [0.0, 1.0], "v turned down");
    assert!(outward(&s), "the quad faces +y like its normal");
    let c = normalize::face_colour(&s, 1, 0);
    assert!((c[0] - srgb_to_linear(200)).abs() < 1e-9 && (c[2] - srgb_to_linear(40)).abs() < 1e-9, "{c:?}");
    // A missing .mtl and a library outside the folder are warnings, not failures.
    let s = read_model(&write(&dir, "bare.obj", "mtllib ../../etc/passwd\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n")).unwrap();
    assert_eq!(s.tris(), 1);
    assert!(s.info.warnings.iter().any(|w| w.contains("materials")), "{:?}", s.info.warnings);
}

#[test]
fn normalisation_turns_scales_and_places_an_asymmetric_model() {
    // A post 1 m tall with a nose toward −x (its front) and a cap on +z (its top: z up), 100
    // units a metre.
    let dir = temp_dir("norm");
    let mut obj = String::new();
    let add = |min: [f32; 3], max: [f32; 3], o: &mut String| {
        let (p, _, i) = cuboid(min, max);
        let base = o.lines().filter(|l| l.starts_with("v ")).count();
        for v in &p {
            o.push_str(&format!("v {} {} {}\n", v[0], v[1], v[2]));
        }
        for t in i.chunks(3) {
            o.push_str(&format!("f {} {} {}\n", t[0] as usize + base + 1, t[1] as usize + base + 1, t[2] as usize + base + 1));
        }
    };
    add([-10.0, -10.0, 20.0], [10.0, 10.0, 120.0], &mut obj); // the post, floating 20 up
    add([-40.0, -5.0, 50.0], [-10.0, 5.0, 60.0], &mut obj); // the nose, toward −x
    let path = write(&dir, "post.obj", obj);
    let mut s = read_model(&path).unwrap();
    let n = Norm::parse(Some("z"), Some(90.0), Some(0.01), Some("floor")).unwrap();
    let done = normalize::normalize(&mut s, &n);
    let (lo, hi) = s.bounds().unwrap();
    // 1 m tall standing on the floor, centred; the nose now points toward +z.
    assert!((hi.y - 1.0).abs() < 1e-6 && lo.y.abs() < 1e-9, "{lo} {hi}");
    assert!((lo.x + hi.x).abs() < 1e-6 && (lo.z + hi.z).abs() < 1e-6, "{lo} {hi}");
    assert!((hi.z - lo.z - 0.5).abs() < 1e-6 && (hi.x - lo.x - 0.2).abs() < 1e-6, "{lo} {hi}");
    let nose_tip = s.meshes[0].pos.iter().chain(&s.meshes.last().unwrap().pos).map(|p| p[2]).fold(f32::MIN, f32::max);
    assert!((nose_tip as f64 - hi.z).abs() < 1e-6);
    assert!(done.moved[1] < 0.0, "lifted down to the floor: {:?}", done.moved);
    assert!(Norm::parse(None, Some(30.0), None, None).is_err());
    // center puts the middle of the bounds at the origin.
    let mut s = read_model(&path).unwrap();
    normalize::normalize(&mut s, &Norm { place: Place::Center, ..Norm::default() });
    let (lo, hi) = s.bounds().unwrap();
    assert!((lo + hi).length() < 1e-6);
}

#[test]
fn malformed_input_is_an_error_not_a_panic() {
    let dir = temp_dir("bad");
    let good = table().glb(&[1]);
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("empty.glb", Vec::new(), "not a glTF"),
        ("text.gltf", b"hello".to_vec(), "not a glTF"),
        ("json.gltf", b"{\"asset\":".to_vec(), "EOF"),
        ("short.glb", good[..16].to_vec(), "truncated"),
        (
            "chunk.glb",
            {
                let mut b = good.clone();
                b[16] = b'X';
                b
            },
            "first chunk is not JSON",
        ),
    ];
    for (name, bytes, want) in &cases {
        let e = read_model(&write(&dir, name, bytes)).unwrap_err();
        assert!(format!("{e:#}").contains(want), "{name}: {e:#}");
    }
    // Every truncation and many single-byte changes: an answer, never a panic.
    let path = dir.join("cut.glb");
    for n in (0..good.len()).step_by(7) {
        std::fs::write(&path, &good[..n]).unwrap();
        let _ = read_model(&path);
    }
    for i in (0..good.len()).step_by(3) {
        let mut b = good.clone();
        b[i] = b[i].wrapping_add(97);
        std::fs::write(&path, &b).unwrap();
        let _ = read_model(&path);
    }
    // Counts and offsets far past the data: errors, not huge allocations or overflow.
    for (key, value, want) in [("count", 1u64 << 40, "limit"), ("byteOffset", u64::MAX - 4, "past the end")] {
        let mut b = MeshBuilder::new();
        let p = b.vec3(&[[0.0; 3]; 3]);
        b.accessors[p][key] = json!(value);
        let prim = b.prim(p, None, &[], None);
        let m = b.mesh(vec![prim]);
        b.node(json!({"mesh": m}));
        let e = read_model(&write(&dir, "huge.glb", b.glb(&[0]))).unwrap_err();
        assert!(format!("{e:#}").contains(want), "{key}: {e:#}");
    }
    // A skin whose inverse bind matrices are not MAT4, or fewer than its joints.
    for (ty, floats, joints, want) in [("VEC3", 18, 1, "not MAT4"), ("MAT4", 16, 2, "inverse bind matrices")] {
        let mut b = MeshBuilder::new();
        let p = b.vec3(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let j = b.unorm8(&[0u8; 12], "VEC4");
        b.accessors[j]["normalized"] = json!(false);
        let w = b.floats(&[1.0, 0.0, 0.0, 0.0].repeat(3), "VEC4");
        let ibm = b.floats(&vec![0.0; floats], ty);
        let prim = b.prim(p, None, &[("JOINTS_0", j), ("WEIGHTS_0", w)], None);
        let m = b.mesh(vec![prim]);
        let bone = b.node(json!({"name": "bone"}));
        let body = b.node(json!({"mesh": m, "skin": 0}));
        b.extra.insert("skins".into(), json!([{"joints": vec![bone; joints], "inverseBindMatrices": ibm}]));
        let e = read_model(&write(&dir, "skin.glb", b.glb(&[bone, body]))).unwrap_err();
        assert!(format!("{e:#}").contains(want), "{ty}: {e:#}");
    }
    // Indices past the vertices, a required Draco mesh, a file outside the folder.
    let mut b = MeshBuilder::new();
    let p = b.vec3(&[[0.0; 3]; 3]);
    let i = b.indices(&[0, 1, 9]);
    let prim = b.prim(p, Some(i), &[], None);
    let m = b.mesh(vec![prim]);
    b.node(json!({"mesh": m}));
    let e = read_model(&write(&dir, "idx.glb", b.glb(&[0]))).unwrap_err();
    assert!(format!("{e:#}").contains("index 9 is past its 3 vertices"), "{e:#}");
    b.extra.insert("extensionsRequired".into(), json!(["KHR_draco_mesh_compression"]));
    let e = read_model(&write(&dir, "draco.glb", b.glb(&[0]))).unwrap_err();
    assert!(format!("{e:#}").contains("KHR_draco_mesh_compression"), "{e:#}");
    b.extra.clear();
    let (text, _) = b.gltf_external(&[0], "../secret.bin");
    let e = read_model(&write(&dir, "escape.gltf", text)).unwrap_err();
    assert!(format!("{e:#}").contains("outside the model's folder"), "{e:#}");
    let e = read_model(&write(&dir, "model.fbx", b"x")).unwrap_err();
    assert!(format!("{e:#}").contains(".glb, .gltf or .obj"), "{e:#}");
    assert!(gltf::base64("@@").is_err());
}

#[test]
fn zip_entries_are_crc_checked_and_kept_inside_their_folder() {
    let dir = temp_dir("zip");
    let body = b"glTF model bytes".repeat(40);
    let z = zip(
        &[("Models/a.glb", &body, true), ("../../evil.txt", b"x", false), ("Models/b.glb", b"bbb", false)],
        Some("Models/b.glb"),
    );
    let path = write(&dir, "pack.zip", z);
    let rz = RemoteZip::open_file(&path).unwrap();
    assert_eq!(rz.entries.len(), 3);
    assert_eq!(rz.read(&rz.entries[0]).unwrap(), body);
    assert_eq!(rz.entries[1].rel_path().unwrap(), Path::new("evil.txt"));
    let e = rz.read(&rz.entries[2]).unwrap_err();
    assert!(e.to_string().contains("CRC-32"), "{e}");
}

/// The tool: the directory, a pinned pack fetched from a local archive, its census written twice
/// to identical bytes with the agent's notes kept, one model summarised, and argument errors.
#[test]
fn the_tool_gets_lists_and_surveys() {
    let root = temp_dir("tool");
    let (cats, cache, pack) = (root.join("catalogs"), root.join("cache"), root.join("upstream"));
    std::fs::create_dir_all(&cats).unwrap();
    std::fs::create_dir_all(&pack).unwrap();
    // SAFETY: only this test reads these variables.
    unsafe {
        std::env::set_var("PAV_CATALOGS", &cats);
        std::env::set_var("PAV_ASSET_CACHE", &cache);
    }
    let table = table().glb(&[1]);
    let mut b = MeshBuilder::new();
    let p = b.box_prim([0.0; 3], [0.5, 0.5, 0.5], None);
    let m = b.mesh(vec![p]);
    b.node(json!({"name": "cube", "mesh": m}));
    let cube = b.glb(&[0]);
    let licence = b"Test pack. License: Creative Commons Zero, CC0\n";
    let archive = zip(
        &[
            ("Models/GLB/table.glb", &table, true),
            ("Models/GLB/cube.glb", &cube, false),
            ("Models/OBJ/cube.obj", b"v 0 0 0\n", false),
            ("License.txt", licence, false),
        ],
        None,
    );
    let zpath = write(&pack, "pack.zip", &archive);
    let sha = pav_tools::import::fetch::sha256_hex(&archive);
    let lsha = pav_tools::import::fetch::sha256_hex(licence);
    let catalog = |sha: &str| {
        format!(
            "// test catalog\n{{\"$library\":\"test\",\"$sources\":{{\"mini\":{{\"label\":\"Test\",\"license\":\"CC0-1.0\",\"url\":\"file://{}\",\"sha256\":\"{sha}\",\"licence_file\":\"License.txt\",\"licence_sha256\":\"{lsha}\",\"models\":\"Models/GLB/*.glb\",\"place\":\"floor\"}}}}}}",
            zpath.display()
        )
    };
    write(&cats, "test.json", catalog(&"0".repeat(64)));
    let mut s = Session::new("empty", 1).unwrap();
    let e = tool(&mut s, json!({"get": "test/mini"})).unwrap_err();
    assert!(format!("{e:#}").contains("not the pinned"), "{e:#}");
    write(&cats, "test.json", catalog(&sha));
    let got = tool(&mut s, json!({"get": "test/mini"})).unwrap();
    assert_eq!((got["got"][0]["models"].as_u64(), got["got"][0]["files"].as_u64()), (Some(2), Some(3)), "{got}");
    let source: Value = serde_json::from_str(&std::fs::read_to_string(cache.join("test/mini/SOURCE.json")).unwrap()).unwrap();
    assert_eq!((source["sha256"].as_str(), source["licence_sha256"].as_str()), (Some(sha.as_str()), Some(lsha.as_str())));
    assert!(!cache.join("test/mini/Models/OBJ").exists(), "only the model folder is unpacked");

    let dir = tool(&mut s, json!({})).unwrap();
    assert!(dir["libraries"].as_array().unwrap().iter().any(|l| l["lib"] == "kenney"));

    let survey = tool(&mut s, json!({"survey": "test/mini"})).unwrap();
    assert_eq!((survey["models"].as_u64(), survey["read"].as_u64()), (Some(2), Some(2)), "{survey}");
    let ledger = cats.join("test/mini.tsv");
    let first = std::fs::read_to_string(&ledger).unwrap();
    // The table stands on the floor (place=floor from the catalog): 3.0 x 1.5 x 1.2 m... rounded cm.
    assert!(first.contains("table\tModels/GLB/table.glb\t60\t1\t0\t5\t120x150x200\t"), "{first}");
    // An agent's note survives a re-survey; the bytes are otherwise identical.
    std::fs::write(&ledger, first.replace("\t\n", "\tlooks like a desk\n").replacen("looks like a desk\n", "\n", 1)).unwrap();
    let noted = std::fs::read_to_string(&ledger).unwrap();
    tool(&mut s, json!({"survey": "test/mini"})).unwrap();
    assert_eq!(std::fs::read_to_string(&ledger).unwrap(), noted);
    assert!(noted.contains("looks like a desk"));

    let one = tool(
        &mut s,
        json!({"from": cache.join("test/mini/Models/GLB/table.glb").to_str().unwrap(), "list": true, "place": "floor"}),
    )
    .unwrap();
    assert_eq!((one["tris"].as_u64(), one["parts"].as_u64()), (Some(60), Some(5)), "{one}");
    assert_eq!(one["size_cm"], json!([120, 150, 200]));
    assert_eq!(one["colours"][0][0], "#e75959");
    assert_eq!(one["materials"][0]["name"], "red");
    let folder = tool(&mut s, json!({"from": cache.join("test/mini/Models/GLB").to_str().unwrap(), "list": true})).unwrap();
    assert_eq!(folder["models"], 2);

    let e = tool(&mut s, json!({"form": "x"})).unwrap_err();
    assert!(e.to_string().contains("did you mean \"from\""), "{e}");
    let e = tool(&mut s, json!({"from": "x.glb"})).unwrap_err();
    assert!(e.to_string().contains("lib/pack"), "{e}");
    let e = tool(&mut s, json!({"from": "x.glb", "list": true, "yaw": 45})).unwrap_err();
    assert!(e.to_string().contains("multiple of 90"), "{e}");
    let e = tool(&mut s, json!({"survey": cache.join("test/mini/Models/GLB").to_str().unwrap()})).unwrap_err();
    assert!(e.to_string().contains("out=<file>.tsv"), "{e}");

    // A changed pin replaces the pack whole: the table, gone upstream, goes here too. A wrong
    // pin is refused first and leaves the old pack as it was.
    let archive = zip(&[("Models/GLB/cube.glb", &cube, false), ("License.txt", licence, false)], None);
    write(&pack, "pack.zip", &archive);
    let sha2 = pav_tools::import::fetch::sha256_hex(&archive);
    write(&cats, "test.json", catalog(&"1".repeat(64)));
    assert!(tool(&mut s, json!({"get": "test/mini"})).is_err());
    assert!(cache.join("test/mini/Models/GLB/table.glb").exists());
    write(&cats, "test.json", catalog(&sha2));
    let got = tool(&mut s, json!({"get": "test/mini"})).unwrap();
    assert_eq!(got["got"][0]["models"], 1, "{got}");
    assert!(!cache.join("test/mini/Models/GLB/table.glb").exists(), "no old file under the new pin");
    let source: Value = serde_json::from_str(&std::fs::read_to_string(cache.join("test/mini/SOURCE.json")).unwrap()).unwrap();
    assert_eq!(source["sha256"].as_str(), Some(sha2.as_str()));
    // The same pin again completes in place.
    let again = tool(&mut s, json!({"get": "test/mini"})).unwrap();
    assert_eq!(
        (again["got"][0]["models"].as_u64(), again["got"][0]["notes"].as_array().map(Vec::len)),
        (Some(1), Some(0)),
        "{again}"
    );
}

/// Ledger ids come from each file's own path, so they hold as other files come and go, and an
/// agent's note follows its model (by id, else by file).
#[test]
fn ledger_ids_and_notes_hold_as_files_come_and_go() {
    let dir = temp_dir("ids");
    let mut b = MeshBuilder::new();
    let p = b.box_prim([0.0; 3], [1.0; 3], None);
    let m = b.mesh(vec![p]);
    b.node(json!({"mesh": m}));
    let cube = b.glb(&[0]);
    std::fs::create_dir_all(dir.join("models/props")).unwrap();
    write(&dir.join("models/props"), "chair.glb", &cube);
    let out = dir.join("census.tsv");
    let mut s = Session::new("empty", 1).unwrap();
    let mut survey = || {
        tool(&mut s, json!({"survey": dir.join("models").to_str().unwrap(), "out": out.to_str().unwrap()})).unwrap();
        std::fs::read_to_string(&out).unwrap()
    };
    let text = survey();
    assert!(text.contains("\nprops/chair\tprops/chair.glb\t"), "{text}");
    std::fs::write(&out, text.replace("\t\n", "\twobbly leg\n")).unwrap();
    // A second chair elsewhere: the first keeps its id and its note.
    std::fs::create_dir_all(dir.join("models/other")).unwrap();
    write(&dir.join("models/other"), "chair.glb", &cube);
    let text = survey();
    assert!(text.contains("\nprops/chair\tprops/chair.glb\t") && text.contains("\nother/chair\tother/chair.glb\t"), "{text}");
    assert_eq!(text.matches("wobbly leg").count(), 1, "{text}");
    // A ledger written under other ids keeps its notes through the file column.
    std::fs::write(&out, text.replace("props/chair\tprops/chair.glb", "chair\tprops/chair.glb")).unwrap();
    let text = survey();
    assert!(text.contains("\nprops/chair\tprops/chair.glb\t") && text.contains("wobbly leg"), "{text}");
    // Same name, another format, same folder: those two keep their extension, and the note.
    write(&dir.join("models/props"), "chair.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    let text = survey();
    assert!(text.contains("\nprops/chair.glb\tprops/chair.glb\t") && text.contains("\nprops/chair.obj\t"), "{text}");
    assert!(text.lines().any(|l| l.starts_with("props/chair.glb") && l.ends_with("wobbly leg")), "{text}");
}

/// Texel lookups follow each texture's sampler: repeat, clamp to edge, mirrored repeat.
#[test]
fn samplers_wrap_texture_coordinates() {
    let png = png_rgba(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 255]]);
    let mut b = MeshBuilder::new();
    let t0 = b.png_texture(&png, "red_blue");
    b.textures[t0]["sampler"] = json!(0);
    b.textures.push(json!({"source": 0, "sampler": 1}));
    b.textures.push(json!({"source": 0, "sampler": 2}));
    b.extra.insert(
        "samplers".into(),
        json!([{"wrapS": 10497, "wrapT": 10497}, {"wrapS": 33071, "wrapT": 33071}, {"wrapS": 33648, "wrapT": 33648}]),
    );
    let mut prims = Vec::new();
    for (t, u) in [(0, 1.25f32), (1, 1.25), (2, 1.75)] {
        let mat = b.material(json!({"pbrMetallicRoughness": {"baseColorTexture": {"index": t}}}));
        let (pa, ua) = (b.vec3(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]), b.vec2(&[[u, 0.5]; 3]));
        prims.push(b.prim(pa, None, &[("TEXCOORD_0", ua)], Some(mat)));
    }
    let m = b.mesh(prims);
    b.node(json!({"mesh": m}));
    let dir = temp_dir("wrap");
    let s = read_model(&write(&dir, "wrap.glb", b.glb(&[0]))).unwrap();
    let wraps: Vec<[Wrap; 2]> = s.materials.iter().map(|m| m.base_tex.unwrap().wrap).collect();
    assert_eq!(wraps, [[Wrap::Repeat; 2], [Wrap::Clamp; 2], [Wrap::Mirror; 2]]);
    let (red, blue) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    // u 1.25 repeats to 0.25 (red) and clamps to 1 (blue); 1.75 mirrors to 0.25 (red).
    assert_eq!([0, 1, 2].map(|k| normalize::face_colour(&s, k, 0)), [red, blue, red]);
}

/// `map_Kd` options are read, and the rest of the line is the file, spaces and all.
#[test]
fn mtl_texture_options_and_spaced_names_read() {
    let dir = temp_dir("mtl");
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    write(&dir.join("textures"), "wood floor.png", png_rgba(2, 1, &[[255, 0, 0, 255], [0, 0, 255, 255]]));
    write(&dir, "floor.mtl", "newmtl floor\nKd 1 1 1\nmap_Kd -s 1 1 1 -clamp on textures/wood floor.png\n");
    let obj = "mtllib floor.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 1.25 0\nusemtl floor\nf 1/1 2/1 3/1\n";
    let s = read_model(&write(&dir, "floor.obj", obj)).unwrap();
    assert!(s.info.warnings.is_empty(), "{:?}", s.info.warnings);
    assert_eq!((s.images[0].name.as_str(), s.images[0].rgba.len()), ("textures/wood floor.png", 8));
    let t = s.materials[0].base_tex.unwrap();
    assert_eq!((t.wrap, t.xform), ([Wrap::Clamp; 2], XFORM_IDENTITY));
    assert_eq!(normalize::face_colour(&s, 0, 0), [0.0, 0.0, 1.0, 1.0], "u 1.25 clamps to the right texel");
    let m = tex_map("-o 0.5 0.25 -s 2 2 1 -bm 0.3 -blendu off a b.png").unwrap();
    assert_eq!((m.file.as_str(), m.offset, m.scale, m.clamp), ("a b.png", [0.5, 0.25, 0.0], [2.0, 2.0, 1.0], false));
    assert!(tex_map("-clamp on").is_none());
}
