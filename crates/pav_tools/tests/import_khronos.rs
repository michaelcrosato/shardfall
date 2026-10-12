//! The Khronos glTF-Sample-Assets conformance corpus, read by the importer (plan §8.4, §8.10).
//! It downloads twelve core models at a pinned commit into .cache/assets/khronos (git-ignored),
//! so it is opt-in:
//!
//!   cargo test -p pav_tools --test import_khronos -- --ignored --nocapture
//!
//! The switch rule: if more than 3 core models still fail after one fix round, the in-house
//! reader gives way to the `gltf` crate behind the same `Scene`.

use std::path::{Path, PathBuf};

use pav_tools::import::fetch::{self, ASSET_AGENT};
use pav_tools::import::normalize;
use pav_tools::import::read_model;
use pav_tools::import::scene::{Alpha, Scene, XFORM_IDENTITY};
use serde_json::Value;

const COMMIT: &str = "edc7c9e67c639d230715049ee31f9a96a6babbbe";
const MODELS: [&str; 12] = [
    "Box",
    "BoxTextured",
    "BoxVertexColors",
    "Duck",
    "OrientationTest",
    "NegativeScaleTest",
    "TextureCoordinateTest",
    "TextureTransformTest",
    "VertexColorTest",
    "MultiUVTest",
    "SimpleSparseAccessor",
    "AlphaBlendModeTest",
];

fn cache() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache/assets/khronos").join(COMMIT)
}

/// Downloads a model (its .glb, else its .gltf and the files it names) and returns its path.
fn fetch_model(name: &str) -> anyhow::Result<PathBuf> {
    let base = format!("https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/{COMMIT}/Models/{name}");
    let dir = cache().join(name);
    let glb = dir.join(format!("{name}.glb"));
    if glb.exists() || fetch::get_as(Some(ASSET_AGENT), &format!("{base}/glTF-Binary/{name}.glb"), &glb).is_ok() {
        return Ok(glb);
    }
    let gltf = dir.join("glTF").join(format!("{name}.gltf"));
    fetch::get_as(Some(ASSET_AGENT), &format!("{base}/glTF/{name}.gltf"), &gltf)?;
    let j: Value = serde_json::from_slice(&std::fs::read(&gltf)?)?;
    for k in ["buffers", "images"] {
        for uri in j[k].as_array().into_iter().flatten().filter_map(|b| b["uri"].as_str()) {
            if !uri.starts_with("data:") {
                fetch::get_as(Some(ASSET_AGENT), &format!("{base}/glTF/{uri}"), &dir.join("glTF").join(uri.replace("%20", " ")))?;
            }
        }
    }
    Ok(gltf)
}

/// How far each arrow's tip is from its target: each `ArrowXn` points at `TargetXn`.
fn orientation(s: &Scene) -> Vec<(String, f64)> {
    let node = |n: &str| s.nodes.iter().position(|x| x.name == n);
    let mut out = Vec::new();
    for a in ["X1", "X2", "Y1", "Y2", "Z1", "Z2"] {
        let (Some(arrow), Some(target)) = (node(&format!("Arrow{a}")), node(&format!("Target{a}"))) else {
            out.push((a.to_string(), f64::INFINITY));
            continue;
        };
        let origin = s.nodes[arrow].world.w_axis.truncate();
        let pts = |n: usize| -> Vec<glam::DVec3> {
            s.meshes
                .iter()
                .filter(|m| m.node == n)
                .flat_map(|m| m.pos.iter().map(|p| glam::DVec3::from(p.map(f64::from))))
                .collect()
        };
        // The tip: the arrow's point farthest from its node's origin. The target indicator's
        // vertices are baked in place; the tip touches it when the rotation is read right (a
        // transposed rotation swings the tip ~3 units away).
        let tip = pts(arrow).into_iter().max_by(|p, q| p.distance(origin).total_cmp(&q.distance(origin))).unwrap_or(origin);
        let near = pts(target).into_iter().map(|p| p.distance(tip)).fold(f64::INFINITY, f64::min);
        out.push((a.to_string(), near));
    }
    out
}

#[test]
#[ignore = "downloads the Khronos sample models (network)"]
fn the_khronos_core_models_read() {
    let mut failed = Vec::new();
    for name in MODELS {
        let checked = fetch_model(name).and_then(|p| read_model(&p)).and_then(|s| {
            if s.tris() == 0 {
                anyhow::bail!("no triangles");
            }
            let decoded = s.images.iter().filter(|i| !i.rgba.is_empty()).count();
            let check = match name {
                "Box" => (s.tris() == 12).then_some(()).ok_or(format!("{} triangles", s.tris())),
                "BoxTextured" | "Duck" => (decoded > 0).then_some(()).ok_or("its PNG texture is not decoded".to_string()),
                "BoxVertexColors" | "VertexColorTest" => {
                    s.meshes.iter().any(|m| !m.col.is_empty()).then_some(()).ok_or("no vertex colours".to_string())
                }
                "OrientationTest" => {
                    let o = orientation(&s);
                    println!("OrientationTest tip-to-target distances: {o:?}");
                    o.iter().all(|(_, d)| *d <= 0.2).then_some(()).ok_or(format!("{o:?}"))
                }
                "NegativeScaleTest" => {
                    let agree = normalize::normal_agreement(&s);
                    println!("NegativeScaleTest: {:.4} of the area agrees with its normals", agree);
                    (agree >= 0.99).then_some(()).ok_or(format!("{agree:.4} agree"))
                }
                "TextureTransformTest" => s
                    .materials
                    .iter()
                    .any(|m| m.base_tex.is_some_and(|t| t.xform != XFORM_IDENTITY))
                    .then_some(())
                    .ok_or("no texture transform read".to_string()),
                // Its second set feeds the emissive texture, which the Scene does not keep.
                "MultiUVTest" => s.meshes.iter().any(|m| !m.uv1.is_empty()).then_some(()).ok_or("no second UV set".to_string()),
                "SimpleSparseAccessor" => {
                    let (lo, hi) = s.bounds().unwrap();
                    println!("SimpleSparseAccessor bounds {lo} {hi}");
                    (hi.y - lo.y > 1.5).then_some(()).ok_or(format!("sparse values not applied: {lo} {hi}"))
                }
                "AlphaBlendModeTest" => {
                    let modes = |f: fn(&Alpha) -> bool| s.materials.iter().any(|m| f(&m.alpha));
                    (modes(|a| *a == Alpha::Blend) && modes(|a| matches!(a, Alpha::Mask(_))) && modes(|a| *a == Alpha::Opaque))
                        .then_some(())
                        .ok_or("not every alpha mode".to_string())
                }
                _ => Ok(()),
            };
            check.map_err(|why| anyhow::anyhow!(why)).map(|_| s)
        });
        match checked {
            Ok(s) => println!(
                "{name}: ok ({} tris, {} meshes, {} materials, {} images, warnings {:?})",
                s.tris(),
                s.meshes.len(),
                s.materials.len(),
                s.images.len(),
                s.info.warnings
            ),
            Err(e) => {
                println!("{name}: FAILED {e:#}");
                failed.push(format!("{name}: {e:#}"));
            }
        }
    }
    assert!(failed.is_empty(), "{} of {} Khronos core models failed: {failed:#?}", failed.len(), MODELS.len());
}
