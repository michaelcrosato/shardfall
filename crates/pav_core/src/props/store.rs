//! Where assets live (docs/ASSET_SYSTEM_PLAN.md §9.1).
//!
//! Names: `BUILTIN/<leaf>` (the templates), `WORKSHOP/<leaf>` and `WORKSHOP_LOCAL/<leaf>`
//! (saved edits), `LIB/<pack>/<leaf>` (library packs: read-only, copy one to change it). A bare
//! leaf is a workshop asset. Tiers: embedded in every build, the browser's included (the
//! templates and `LIB/core`), disk (other packs under `lib/`: native builds read them on first
//! use), local (`local/`, git-ignored: packs and workshop copies whose sources' licences forbid
//! sharing them) and the workshop.

use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/asset_data.rs"));

/// The embedded library pack.
pub const CORE: &str = "core";

/// Where an asset is kept, and so which builds have it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// In every build, the browser's included: the templates and `LIB/core`.
    Embedded,
    /// Committed packs under `assets/props/lib/`, read on first use by native builds.
    Disk,
    /// On this machine only (git-ignored): packs under `assets/props/local/`, `WORKSHOP_LOCAL`.
    Local,
    /// Saved edits, `WORKSHOP/` (native).
    Workshop,
}

impl Tier {
    pub const NAMES: &'static [&'static str] = &["embedded", "disk", "local", "workshop"];
    const ALL: [Tier; 4] = [Tier::Embedded, Tier::Disk, Tier::Local, Tier::Workshop];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::NAMES.iter().position(|name| *name == text).map(|index| Self::ALL[index])
    }
}

const NAMES: &str = "asset names are BUILTIN/name, WORKSHOP/name, WORKSHOP_LOCAL/name or LIB/pack/name";

/// Normalize a name: a bare leaf is a workshop asset; set prefixes are read in any case; packs
/// and leaves are lowercase.
pub fn canonical(name: &str) -> Result<String, String> {
    let segments: Vec<&str> = name.split('/').collect();
    let (set, leaf) = match segments.as_slice() {
        [leaf] => ("WORKSHOP".to_owned(), *leaf),
        [set, leaf] => match set.to_ascii_uppercase().as_str() {
            set @ ("BUILTIN" | "WORKSHOP" | "WORKSHOP_LOCAL") => (set.to_owned(), *leaf),
            "LIB" => return Err(format!("LIB names a pack too: LIB/pack/name, e.g. LIB/core/{}", leaf.to_ascii_lowercase())),
            _ => return Err(NAMES.into()),
        },
        [set, pack, leaf] if set.eq_ignore_ascii_case("LIB") => {
            let pack = pack.to_ascii_lowercase();
            validate_pack(&pack)?;
            (format!("LIB/{pack}"), *leaf)
        }
        _ => return Err(NAMES.into()),
    };
    let leaf = leaf.to_ascii_lowercase();
    super::validate_name(&leaf)?;
    Ok(format!("{set}/{leaf}"))
}

/// A pack name: a lowercase letter, then up to 31 lowercase letters, digits or _. `workshop` is
/// taken by `WORKSHOP_LOCAL`'s folder and `templates` by `assets reindex=templates`.
pub fn validate_pack(pack: &str) -> Result<(), String> {
    let mut bytes = pack.bytes();
    let ok = pack.len() <= 32
        && bytes.next().is_some_and(|c| c.is_ascii_lowercase())
        && bytes.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
    if !ok {
        return Err(format!("pack \"{pack}\": use a lowercase letter, then up to 31 lowercase letters, digits or _"));
    }
    if pack == "workshop" {
        return Err("pack \"workshop\" is reserved (local/workshop holds WORKSHOP_LOCAL)".into());
    }
    if pack == "templates" {
        return Err("pack \"templates\" is reserved (assets reindex=templates indexes BUILTIN)".into());
    }
    Ok(())
}

/// The last segment of a name: the asset's own `name`.
pub fn leaf(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The pack of a `LIB/<pack>/<leaf>` name.
pub fn pack(name: &str) -> Option<&str> {
    name.strip_prefix("LIB/")?.split_once('/').map(|(pack, _)| pack)
}

/// Assets `asset_edit` saves: the workshop's. Templates and packs are read-only.
pub fn editable(name: &str) -> bool {
    name.starts_with("WORKSHOP/") || name.starts_with("WORKSHOP_LOCAL/")
}

/// The embedded tier: the templates and the core pack, in every build.
pub fn embedded(name: &str) -> bool {
    name.starts_with("BUILTIN/") || pack(name) == Some(CORE)
}

/// Packs whose files every build carries beside the core (the browser and a lone `.exe` have no
/// asset files), so rooms and game data can show them. Their rows keep their own tier: shipping
/// says where the files are, not how far an entry has come. Keep `build.rs`'s list in step.
pub const SHIPPED: &[&str] = &["kaykit_adventurers"];

/// In every build, the browser's included: game data and rooms may name only these (the
/// embedded tier and the shipped packs).
pub fn shipped(name: &str) -> bool {
    embedded(name) || pack(name).is_some_and(|pack| SHIPPED.contains(&pack))
}

/// A shipped pack's file by its path inside the pack (`knight.json`, `rigs/knight.json`).
pub fn shipped_file(name: &str, inner: &str) -> Option<&'static str> {
    let pack = pack(name).filter(|pack| SHIPPED.contains(pack))?;
    embedded_file(&format!("lib/{pack}/{inner}"))
}

/// Native asset files: `$PAV_ASSETS`, else `./assets/props`.
pub fn disk_root() -> PathBuf {
    std::env::var_os("PAV_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/props"))
}

/// An embedded file by its path under assets/props, e.g. `templates/bench.json`.
pub fn embedded_file(path: &str) -> Option<&'static str> {
    ASSET_DATA.iter().find(|(key, _)| *key == path).map(|(_, text)| *text)
}

/// Every embedded file: (path under assets/props, text).
pub fn embedded_files() -> &'static [(&'static str, &'static str)] {
    ASSET_DATA
}

/// The embedded assets as (name, file text): `BUILTIN/<leaf>` for `templates/<leaf>.json`,
/// `LIB/core/<leaf>` for `lib/core/<leaf>.json`.
pub fn embedded_assets() -> impl Iterator<Item = (String, &'static str)> {
    ASSET_DATA.iter().filter_map(|(path, text)| {
        let (set, file) = match path.split_once('/')? {
            ("templates", file) => ("BUILTIN", file),
            ("lib", rest) => ("LIB/core", rest.strip_prefix("core/")?),
            _ => return None,
        };
        let leaf = file.strip_suffix(".json").filter(|leaf| !leaf.contains('/'))?;
        Some((format!("{set}/{leaf}"), *text))
    })
}

/// The file an asset is kept in under `root` on this machine: the workshop's, or a pack's
/// (under `lib/`, else `local/`). The embedded tier has none.
pub fn disk_file(root: &Path, name: &str) -> Option<PathBuf> {
    let file = format!("{}.json", leaf(name));
    if name.starts_with("WORKSHOP/") {
        return Some(root.join("workshop").join(file));
    }
    if name.starts_with("WORKSHOP_LOCAL/") {
        return Some(root.join("local").join("workshop").join(file));
    }
    let pack = pack(name).filter(|pack| *pack != CORE)?;
    ["lib", "local"].iter().map(|tier| root.join(tier).join(pack).join(&file)).find(|path| path.is_file())
}

/// A library asset's text: `LIB/core` from the embedded data, other packs from `root`
/// (native builds), a shipped pack's from the build when `root` lacks it. None: no such asset.
pub(super) fn library_text(root: &Path, name: &str) -> Option<Result<String, String>> {
    if pack(name)? == CORE {
        return embedded_file(&format!("lib/core/{}.json", leaf(name))).map(|text| Ok(text.to_owned()));
    }
    let shipped = || shipped_file(name, &format!("{}.json", leaf(name))).map(|text| Ok(text.to_owned()));
    if cfg!(target_arch = "wasm32") {
        return shipped();
    }
    match disk_file(root, name).filter(|path| path.is_file()) {
        Some(path) => Some(std::fs::read_to_string(&path).map_err(|e| format!("{name}: read {}: {e}", path.display()))),
        None => shipped().or_else(|| {
            let path = disk_file(root, name)?;
            Some(std::fs::read_to_string(&path).map_err(|e| format!("{name}: read {}: {e}", path.display())))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_tiers_and_files() {
        assert_eq!(canonical("Lib/Core/Barrel").unwrap(), "LIB/core/barrel");
        assert_eq!(canonical("workshop_local/Stool").unwrap(), "WORKSHOP_LOCAL/stool");
        assert_eq!(canonical("crate").unwrap(), "WORKSHOP/crate");
        assert_eq!(canonical("LIB/barrel").unwrap_err(), "LIB names a pack too: LIB/pack/name, e.g. LIB/core/barrel");
        for bad in
            ["LIB/Core-2/x", "LIB/workshop/x", "LIB/templates/x", "LIB/1pack/x", "MINE/x", "a/b/c", "LIB/core/con", "../../bad"]
        {
            assert!(canonical(bad).is_err(), "{bad}");
        }
        assert_eq!((leaf("LIB/core/barrel"), pack("LIB/core/barrel"), pack("BUILTIN/crate")), ("barrel", Some("core"), None));
        assert!(embedded("BUILTIN/crate") && embedded("LIB/core/barrel") && !embedded("LIB/kenney/barrel"));
        assert!(editable("WORKSHOP_LOCAL/x") && !editable("LIB/core/x") && !editable("BUILTIN/box"));
        assert_eq!(Tier::parse("disk").map(Tier::name), Some("disk"));
        let root = Path::new("/r");
        assert_eq!(disk_file(root, "WORKSHOP_LOCAL/x").unwrap(), Path::new("/r/local/workshop/x.json"));
        assert_eq!(disk_file(root, "LIB/core/x"), None, "the core pack is embedded");
        // The templates are embedded with their index; every embedded asset has a name.
        let names: Vec<String> = embedded_assets().map(|(name, _)| name).collect();
        for template in ["BUILTIN/bench", "BUILTIN/box", "BUILTIN/crate", "BUILTIN/lantern"] {
            assert!(names.iter().any(|name| name == template), "{template}");
        }
        assert!(embedded_file("templates/index.tsv").is_some());
    }
}
