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
//! - `libraries`: the open model libraries the importer knows (its directory) and `get`;
//!   `licence`: the gate that routes a pack's translations to `lib/` or `local/`.
//! - `fit`: translation into a format-2 prop (primitives, the mesh part, mirrors and arrays).
//! - `raster`: a CPU rasteriser of orthographic views; `metrics`: the fidelity metrics, the
//!   tiers and the compare image.
//! - `report`: writing what was made, and the replies.
//! - `skeleton`: the bone-name tables that recognise a rig (ported from my-3danimations);
//!   `character`: a rigged model translated into a figure (`pav_core::figure`).

pub mod catalog;
pub mod character;
pub mod fetch;
pub mod fit;
pub mod gltf;
pub mod image;
pub mod ledger;
pub mod libraries;
pub mod licence;
pub mod metrics;
pub mod normalize;
pub mod obj;
pub mod raster;
pub mod report;
pub mod scene;
pub mod skeleton;

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
