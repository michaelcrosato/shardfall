//! What the motion importer (`mocap`, `clip_import`) and the asset importer (`asset_import`)
//! share, and the asset importer's reading half.
//!
//! - `fetch`: downloads (curl), single files out of zip archives on the web, CRC-32, SHA-256,
//!   the caches (`.cache/mocap`, `.cache/assets`).
//! - `catalog`: the hand-written catalog grammar; `ledger`: tab-separated ledgers.
//! - `gltf`: glTF 2.0 (.glb, .gltf): container, accessors, matrices, node hierarchy, and a whole
//!   model read into a `Scene`.
//! - `obj`: Wavefront OBJ with its MTL colours, into a `Scene`.
//! - `image`: texture headers, PNG decoding.
//! - `scene`: the one intermediate every reader fills.
//! - `normalize`: units, up axis, facing, origin, winding; welds and parts for the census.
//! - `libraries`: the open model libraries the importer knows (its directory) and `get`.
//! - `report`: writing what was made, and the replies.

pub mod catalog;
pub mod fetch;
pub mod gltf;
pub mod image;
pub mod ledger;
pub mod libraries;
pub mod normalize;
pub mod obj;
pub mod report;
pub mod scene;

use std::path::Path;

use anyhow::{Result, bail};

/// Reads a model file (.glb, .gltf, .obj) into a `Scene`.
pub fn read_model(path: &Path) -> Result<scene::Scene> {
    match path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("glb" | "gltf") => gltf::scene(path),
        Some("obj") => obj::scene(path),
        _ => bail!("{}: a .glb, .gltf or .obj file", path.display()),
    }
}
