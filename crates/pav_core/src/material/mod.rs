//! Procedural materials (asset plan §6): surface recipes evaluated per pixel, with no image
//! files and no UVs. A material is a base colour and look plus up to three pattern layers from
//! a closed vocabulary of 14 patterns (10 GPU kernels).
//!
//! - `schema`: reading material files and inline materials strictly, with paths and fixes;
//! - `resolve`: `base` chains (shallow merge, layers replaced wholesale, at most 4 hops, no
//!   cycles), cost units and legibility warnings;
//! - `eval`: the GPU packing (`GpuMaterial`, 368 bytes) and the CPU reference evaluator, a port
//!   of the WGSL kernels that drives swatches, bakes, fitting and the renderer's parity test;
//! - `library`: the embedded core tier, `MAT/<name>`, and with the workshop (`MAT/workshop/x`)
//!   the `current()` library props, statics and views resolve names against;
//! - `write`: material files' canonical text and `mat1-` revisions;
//! - `report`: how each layer reads at the game camera (swatch replies).
//!
//! Lengths are in the document's unit (`cm`, the house unit, or `m`) on disk and metres once
//! read.

pub mod eval;
pub mod library;
pub mod report;
pub mod resolve;
pub mod schema;
#[cfg(test)]
mod tests;
pub mod write;

pub use eval::{GpuLayer, GpuMaterial, MATERIAL_BYTES, Packed, Sample, Shaded, eval_packed, footprint};
pub use library::Library;
pub use resolve::{Material, resolve};
pub use schema::{Fields, LayerDoc, MatDoc, Param, read_doc, read_file, read_inline};

/// Library material names start with this.
pub const PREFIX: &str = "MAT/";

/// Whether `name` is written as a library material name: `MAT/` and lowercase words
/// (`a-z`, `0-9`, `_`) separated by `/`, such as `MAT/pb/gold` or `MAT/workshop/moss`.
pub fn is_library_name(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 96
            && rest
                .split('/')
                .all(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    })
}
