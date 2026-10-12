//! The material library. The core tier (`assets/materials/*.json` and `pb/*.json`, names
//! `MAT/<name>` and `MAT/pb/<name>`) is embedded at build time (`MATERIALS`, from build.rs) and
//! read-only.
//!
//! Seam for the later tiers (asset plan §6.1): workshop (`assets/materials/workshop/`, editable,
//! revisions `mat1-<fnv>`), fitted (`assets/materials/fit/<lib>/`, on disk) and local
//! (git-ignored) materials are more `(name, text)` sources under their own name prefixes,
//! read with `Library::from_sources` like the core ones; a base may then name any loaded tier.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use super::PREFIX;
use super::resolve::{Material, resolve};
use super::schema::{MatDoc, read_file};
use crate::issue::{Code, Issue, Severity};

include!(concat!(env!("OUT_DIR"), "/materials.rs"));

/// Loaded materials: the documents, their resolved forms, and every problem found.
#[derive(Debug, Default)]
pub struct Library {
    docs: BTreeMap<String, MatDoc>,
    materials: BTreeMap<String, Arc<Material>>,
    issues: BTreeMap<String, Vec<Issue>>,
}

impl Library {
    /// The embedded core tier, read once.
    pub fn core() -> &'static Library {
        static CORE: OnceLock<Library> = OnceLock::new();
        CORE.get_or_init(|| Library::from_sources(MATERIALS))
    }

    /// Reads `(name, JSON text)` pairs, names without `MAT/` (`oak_planks`, `pb/gold`). A
    /// material with errors is left out and its errors kept; warnings are kept beside the
    /// materials that load.
    pub fn from_sources(sources: &[(&str, &str)]) -> Library {
        let mut lib = Library::default();
        for (name, text) in sources {
            let full = format!("{PREFIX}{name}");
            match read_file(name, text) {
                Ok(doc) => {
                    lib.docs.insert(full, doc);
                }
                Err(issues) => {
                    lib.issues.insert(full, issues);
                }
            }
        }
        for (full, doc) in &lib.docs {
            match resolve(full, &doc.fields, &lib.docs) {
                Ok(material) => {
                    let warnings = material.check();
                    if !warnings.is_empty() {
                        lib.issues.insert(full.clone(), warnings);
                    }
                    lib.materials.insert(full.clone(), Arc::new(material));
                }
                Err(mut issues) => {
                    for issue in &mut issues {
                        let broken = doc.fields.base.as_ref().filter(|base| lib.issues.contains_key(*base));
                        if let (Code::UnknownMaterial, Some(base)) = (issue.code, broken) {
                            issue.message = format!("base \"{base}\" does not load");
                            issue.fix = Some(format!("fix the errors in {base} first"));
                        }
                    }
                    lib.issues.insert(full.clone(), issues);
                }
            }
        }
        lib
    }

    /// A material by its full name, `MAT/oak_planks`.
    pub fn get(&self, name: &str) -> Option<&Arc<Material>> {
        self.materials.get(name)
    }

    /// The names of the materials that load.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.materials.keys().map(String::as_str)
    }

    /// The documents that read, for resolving inline materials whose `base` is in the library.
    pub fn docs(&self) -> &BTreeMap<String, MatDoc> {
        &self.docs
    }

    /// Errors (the material is left out) and warnings, per material.
    pub fn issues(&self) -> &BTreeMap<String, Vec<Issue>> {
        &self.issues
    }

    /// The materials left out because of errors.
    pub fn broken(&self) -> impl Iterator<Item = &str> {
        self.issues.iter().filter(|(_, issues)| issues.iter().any(|i| i.severity == Severity::Error)).map(|(n, _)| n.as_str())
    }
}
