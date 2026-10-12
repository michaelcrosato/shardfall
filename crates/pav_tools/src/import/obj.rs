//! Wavefront OBJ with its MTL materials, read with `tobj` into a `Scene`: one node per named
//! object (`o`/`g`), one mesh per object and material, faces triangulated as fans. `Kd` is read
//! as linear (as Blender writes it), `d` as alpha, `Ke` as emissive and `map_Kd` as the base
//! colour texture (PNG). Texture coordinates are turned to v down, as glTF has them. OBJ has no
//! units or up axis: normalisation's `scale` and `up` say them.

use std::io::BufReader;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use glam::DMat4;

use super::image;
use super::scene::{Alpha, Image, Info, MAX_IMAGES, MAX_TRIS, Material, Mesh, Node, Scene, TexRef, XFORM_IDENTITY};

/// A file an OBJ or MTL names, kept inside the model's folder.
fn inside(dir: &Path, name: &str) -> Option<PathBuf> {
    let rel = PathBuf::from(name.replace('\\', "/"));
    rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)).then(|| dir.join(rel))
}

fn floats(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

pub fn scene(path: &Path) -> Result<Scene> {
    let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let f = std::fs::File::open(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let opts = tobj::LoadOptions { single_index: true, triangulate: true, ignore_points: true, ignore_lines: true };
    let (models, mats) = tobj::load_obj_buf(&mut BufReader::new(f), &opts, |p| {
        let Some(p) = p.to_str().and_then(|p| inside(&dir, p)) else { return Err(tobj::LoadError::OpenFileFailed) };
        let f = std::fs::File::open(p).map_err(|_| tobj::LoadError::OpenFileFailed)?;
        tobj::load_mtl_buf(&mut BufReader::new(f))
    })
    .map_err(|e| anyhow!("{file}: {e}"))?;
    let mut info = Info { file: file.clone(), format: "obj".into(), ..Info::default() };
    let mats = match mats {
        Ok(m) => m,
        Err(e) => {
            info.warnings.push(format!("materials: {e} (the .mtl is missing or unreadable): default grey"));
            Vec::new()
        }
    };

    // Materials and their textures (each texture file read once).
    let mut images: Vec<Image> = Vec::new();
    let mut materials = Vec::new();
    for m in &mats {
        let d = m.dissolve.unwrap_or(1.0).clamp(0.0, 1.0);
        let kd = m.diffuse.unwrap_or([0.8, 0.8, 0.8]);
        let ke = m.unknown_param.get("Ke").map(|v| floats(v)).filter(|v| v.len() >= 3).map_or([0.0; 3], |v| [v[0], v[1], v[2]]);
        let base_tex = match m.diffuse_texture.as_deref().and_then(|t| t.split_whitespace().last()) {
            Some(name) => {
                let at = match images.iter().position(|i| i.name == name) {
                    Some(i) => Some(i),
                    None => match inside(&dir, name).map(|p| std::fs::read(&p).map_err(|e| anyhow!("{}: {e}", p.display()))) {
                        Some(Ok(b)) => {
                            if images.len() >= MAX_IMAGES {
                                bail!("{file}: over {MAX_IMAGES} images");
                            }
                            images.push(image::read(name, &b, true, &mut info.warnings));
                            Some(images.len() - 1)
                        }
                        Some(Err(e)) => {
                            info.warnings.push(format!("material {}: texture {e}: colour factor only", m.name));
                            None
                        }
                        None => {
                            info.warnings.push(format!("material {}: texture {name} is outside the model's folder", m.name));
                            None
                        }
                    },
                };
                at.map(|image| TexRef { image, uv_set: 0, xform: XFORM_IDENTITY })
            }
            None => None,
        };
        materials.push(Material {
            name: m.name.clone(),
            base: [kd[0], kd[1], kd[2], d],
            metallic: 0.0,
            roughness: 1.0,
            emissive: ke,
            alpha: if d < 1.0 { Alpha::Blend } else { Alpha::Opaque },
            double_sided: false,
            unlit: false,
            base_tex,
        });
    }

    // One node per object name, one mesh per object and material.
    let mut nodes: Vec<Node> = Vec::new();
    let mut meshes = Vec::new();
    let mut total = 0;
    for (i, md) in models.iter().enumerate() {
        let m = &md.mesh;
        let nv = m.positions.len() / 3;
        if m.indices.is_empty() || nv == 0 {
            continue;
        }
        let name = if md.name.is_empty() || md.name == "unnamed_object" { format!("object{i}") } else { md.name.clone() };
        let node = match nodes.iter().position(|n| n.name == name) {
            Some(n) => n,
            None => {
                nodes.push(Node { name, parent: None, world: DMat4::IDENTITY });
                nodes.len() - 1
            }
        };
        let mut tris: Vec<[u32; 3]> = m.indices.as_chunks::<3>().0.to_vec();
        if let Some(bad) = m.indices.iter().find(|&&v| v as usize >= nv) {
            bail!("{file}: object {}: index {bad} is past its {nv} vertices", md.name);
        }
        tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        total += tris.len();
        if total > MAX_TRIS {
            bail!("{file}: over {MAX_TRIS} triangles");
        }
        let three = |v: &[f32]| -> Vec<[f32; 3]> { if v.len() == nv * 3 { v.as_chunks::<3>().0.to_vec() } else { Vec::new() } };
        meshes.push(Mesh {
            node,
            material: m.material_id.filter(|&id| id < materials.len()),
            pos: three(&m.positions),
            nrm: three(&m.normals),
            uv: if m.texcoords.len() == nv * 2 {
                m.texcoords.as_chunks::<2>().0.iter().map(|&[u, v]| [u, 1.0 - v]).collect()
            } else {
                Vec::new()
            },
            uv1: Vec::new(),
            col: three(&m.vertex_color).into_iter().map(|c| [c[0], c[1], c[2], 1.0]).collect(),
            tris,
        });
    }
    Ok(Scene { nodes, meshes, materials, images, info })
}
