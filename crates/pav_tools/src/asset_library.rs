//! The curated library (docs/ASSET_SYSTEM_PLAN.md §9.3, §9.5): each pack's licence
//! (`PACK.json`), the credits file, the gate a library asset passes, and
//! `asset_edit action=publish`, which writes a checked workshop asset into a pack with its index
//! row and its review.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::props::doc::Category;
use pav_core::props::index;
use pav_core::props::{self, PropDef, Tier, store};
use serde::Deserialize;
use serde_json::{Value, json};

/// Licences `lib/` may hold. `lib/core` holds own work and CC0 only (plan D5); anything else
/// stays under `local/`.
pub const REDISTRIBUTABLE: &[&str] = &["CC0-1.0", "CC-BY-3.0", "CC-BY-4.0"];

/// A pack's `PACK.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub pack: String,
    pub title: String,
    /// An SPDX identifier.
    pub license: String,
    /// The attribution line.
    pub credit: String,
    /// `own`, or where it was downloaded from.
    pub source: String,
    /// The import catalog it came from, if any.
    #[serde(default)]
    pub catalog: Option<String>,
}

/// The `PACK.json` of `folder`, checked: it names its folder and its licence may be committed.
pub fn read_pack(folder: &Path) -> Result<Pack> {
    let path = folder.join("PACK.json");
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow!("{}: {e} (a pack needs its licence)", path.display()))?;
    let pack: Pack = serde_json::from_str(&text).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let folder_name = folder.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if pack.pack != folder_name {
        bail!("{}: pack \"{}\" must name its folder \"{folder_name}\"", path.display(), pack.pack);
    }
    license_allowed(&pack.pack, &pack.license).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    for (field, value) in [("title", &pack.title), ("credit", &pack.credit), ("source", &pack.source)] {
        if value.trim().is_empty() {
            bail!("{}: {field} is empty (the credits need it)", path.display());
        }
    }
    Ok(pack)
}

/// Whether an asset's `source` may sit in a pack licensed `pack_license`: own work has no
/// `source`; anything with one names a licence, and it is the pack's.
pub fn source_allowed(source: Option<&props::doc::Source>, pack_license: &str) -> Result<()> {
    let Some(source) = source else { return Ok(()) };
    match source.license.as_deref() {
        None => bail!("its source names no licence"),
        Some(license) if license != pack_license => {
            bail!("its source's licence {license} is not the pack's ({pack_license}); keep it in a pack of its own licence")
        }
        Some(_) => Ok(()),
    }
}

/// Whether `license` may be committed in `lib/<pack>`.
pub fn license_allowed(pack: &str, license: &str) -> Result<()> {
    let allowed: &[&str] = if pack == store::CORE { &["CC0-1.0"] } else { REDISTRIBUTABLE };
    if !allowed.contains(&license) {
        bail!("licence {license} cannot be committed in lib/{pack} (allowed: {}); keep it under local/", allowed.join(", "));
    }
    Ok(())
}

/// The committed packs under `root`: `lib/core`, then the others on disk.
pub fn committed_packs(root: &Path) -> Vec<(String, PathBuf)> {
    let core = root.join("lib").join(store::CORE);
    let mut packs: Vec<_> = core.is_dir().then(|| (store::CORE.to_owned(), core)).into_iter().collect();
    packs.extend(
        index::disk_packs(root).into_iter().filter(|(_, _, tier)| *tier == Tier::Disk).map(|(pack, folder, _)| (pack, folder)),
    );
    packs
}

/// The credits file (`CREDITS.md` under the props root): each committed pack's licence and
/// credit line. Local packs are not shared, so not listed.
pub fn credits(root: &Path) -> Result<String> {
    let mut text = String::from(
        "# Object asset credits\n\nThe licence and credit of each committed object pack (`assets/props/lib/`), written by\n`pav assets reindex=<pack>` from the packs' `PACK.json`.\n",
    );
    for (_, folder) in committed_packs(root) {
        let pack = read_pack(&folder)?;
        text.push_str(&format!(
            "\n## LIB/{}: {}\n\n- Licence: {}\n- Credit: {}\n- Source: {}\n",
            pack.pack, pack.title, pack.license, pack.credit, pack.source
        ));
    }
    Ok(text)
}

/// Rewrites `CREDITS.md` under `root` when it changed.
pub fn write_credits(root: &Path) -> Result<()> {
    let text = credits(root)?;
    let path = root.join("CREDITS.md");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&path, text).map_err(|e| anyhow!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Whether a library asset is `checked` (docs/AGENT_ASSET_STORE.md §7): no `check` findings,
/// the library gate passed (description, category, 2-6 tags, token cap, licence) and an admitted
/// licence. The caller checks the mesh share.
pub fn checked(def: &PropDef, pack: &Pack) -> bool {
    pav_core::store::admitted(&pack.license, "prop").is_ok()
        && gate(def, pack).is_empty()
        && crate::asset_tools::check_lines(def).is_empty()
}

/// Every source of the store's entries for its credits text: the clip sets embedded in the build
/// and those in `anim/<folder>/` on disk (never `anim/local`), and the committed object packs.
pub fn credit_sources(props_root: &Path) -> Result<Vec<pav_core::store::Credit>> {
    use pav_core::clips::ClipSet;
    use pav_core::store::{Credit, licence_credit};
    let mut out = Vec::new();
    let mut add_set = |set: &ClipSet, where_: &'static str| {
        let mut seen = BTreeSet::new();
        for source in set.sources.values() {
            let licence = source.get("license").and_then(Value::as_str).unwrap_or("").to_owned();
            let credit = licence_credit(&licence).map(str::to_owned).unwrap_or_else(|| set.credit.clone());
            if seen.insert((licence.clone(), credit.clone())) {
                out.push(Credit { kind: "clip", what: set.set.clone(), where_, licence, credit });
            }
        }
    };
    for name in pav_core::anim::files("json") {
        let text = pav_core::anim::source(&name).unwrap_or_default();
        let set = ClipSet::parse(&text).map_err(|e| anyhow!("anim/{name}: {e}"))?;
        add_set(&set, "embedded");
    }
    if let Some(dir) = pav_core::anim::disk_dir() {
        let mut folders: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| anyhow!("{}: {e}", dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name != "catalogs" && name != "local" && !name.to_string_lossy().starts_with('.'))
            })
            .collect();
        folders.sort();
        for folder in folders {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&folder)
                .map_err(|e| anyhow!("{}: {e}", folder.display()))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect();
            files.sort();
            for file in files {
                let text = std::fs::read_to_string(&file).map_err(|e| anyhow!("{}: {e}", file.display()))?;
                add_set(&ClipSet::parse(&text).map_err(|e| anyhow!("{}: {e}", file.display()))?, "on disk");
            }
        }
    }
    for (name, folder) in committed_packs(props_root) {
        let pack = read_pack(&folder)?;
        let where_ = if name == store::CORE { "embedded" } else { "on disk" };
        out.push(Credit { kind: "prop", what: format!("LIB/{}", pack.pack), where_, licence: pack.license, credit: pack.credit });
    }
    Ok(out)
}

/// `assets reindex=store`: writes the store's generated files (`CREDITS.md`) under the store
/// folder (`$PAV_STORE`, else `./store`).
pub fn write_store(props_root: &Path) -> Result<Value> {
    let dir = crate::asset_search::store_root();
    if !dir.is_dir() {
        bail!("no store folder at {} (run from the repository, or set PAV_STORE)", dir.display());
    }
    let credits = pav_core::store::credits_text(&credit_sources(props_root)?);
    let path = dir.join("CREDITS.md");
    std::fs::write(&path, &credits).map_err(|e| anyhow!("write {}: {e}", path.display()))?;
    Ok(json!({"written": [path], "credits_lines": credits.lines().filter(|line| line.starts_with("- ")).count()}))
}

/// Tokens a library asset may cost by category (plan §9.3): small things 400, dressing 600,
/// furniture and gear 800, set pieces 2,000.
fn token_cap(category: Option<Category>) -> usize {
    match category {
        Some(Category::Food | Category::Tool | Category::Signage) => 400,
        Some(Category::Container | Category::Lighting | Category::Rock | Category::Vegetation) => 600,
        Some(Category::Town | Category::Dungeon) => 2_000,
        _ => 800,
    }
}

/// What keeps `def` out of `lib/<pack>` besides `check`'s findings: what retrieval needs (a
/// description, a category, 2-6 tags), its token cap, references outside the library, workshop
/// materials, and its source's licence.
pub fn gate(def: &PropDef, pack: &Pack) -> Vec<String> {
    let doc = &def.doc;
    let mut found = Vec::new();
    let chars = doc.description.trim().chars().count();
    if !(1..=120).contains(&chars) {
        found.push(format!("missing: a description of 1-120 characters (has {chars})"));
    }
    if doc.category.is_none() {
        let hint = if def.v1.is_some() { "; format 1 holds none: replace it as format 2" } else { "" };
        found.push(format!("missing: category ({}){hint}", Category::NAMES.join(", ")));
    }
    if !(2..=6).contains(&doc.tags.len()) {
        found.push(format!("missing: 2-6 tags (has {})", doc.tags.len()));
    }
    let tok = index::tokens(doc.text().len());
    let cap = token_cap(doc.category);
    if tok > cap {
        found.push(format!("tokens: {tok} (cap {cap} for its category)"));
    }
    for dep in &def.deps {
        let library = dep.starts_with("BUILTIN/") || matches!(store::pack(dep), Some(p) if p == store::CORE || p == pack.pack);
        if !library {
            found.push(format!("refs: {dep} is outside the library; publish it first and reference its LIB/ name"));
        }
    }
    // Workshop materials stay on the machine that made them; a library asset ships without them.
    // Any mention counts: `mat`, the materials table, an inline material's `base`, `recolor`.
    let text = doc.text();
    let workshop: BTreeSet<&str> = text
        .match_indices("MAT/workshop/")
        .map(|(at, _)| {
            let rest = &text[at..];
            &rest[..rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/')).unwrap_or(rest.len())]
        })
        .collect();
    for name in workshop {
        found.push(format!("materials: {name} is a workshop material; use a core MAT/ name or write it inline"));
    }
    if let Err(error) = source_allowed(doc.source.as_ref(), &pack.license) {
        found.push(format!("license: {error}"));
    }
    found
}

/// One line of the review ledger: no tabs or line breaks.
fn line(text: &str) -> String {
    text.split(['\t', '\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The review ledger of a pack folder: (name, revision, review) per asset, sorted by name.
pub fn reviews(folder: &Path) -> Result<Vec<(String, String, String)>> {
    let path = folder.join("review.tsv");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    text.lines()
        .skip(1)
        .filter(|row| !row.trim().is_empty())
        .enumerate()
        .map(|(i, row)| match row.split('\t').collect::<Vec<_>>()[..] {
            [name, revision, review] if !review.trim().is_empty() => {
                Ok((name.to_owned(), revision.to_owned(), review.to_owned()))
            }
            _ => Err(anyhow!("{} line {}: expected name, revision and a review", path.display(), i + 2)),
        })
        .collect()
}

/// `asset_edit action=publish name=WORKSHOP/x pack=core review="ok: …"`: `def` (the workshop
/// asset, with `findings` from `check`) passes the gate, then goes into `lib/<pack>/<leaf>.json`
/// with its index row, its review at this revision, and the credits rewritten.
pub fn publish(
    root: &Path,
    name: &str,
    def: &PropDef,
    reviewed: &str,
    findings: Vec<String>,
    pack: &str,
    review: &str,
) -> Result<Value> {
    if name.starts_with("WORKSHOP_LOCAL/") {
        bail!("{name} stays on this machine: WORKSHOP_LOCAL holds copies of sources that may not be shared");
    }
    if !name.starts_with("WORKSHOP/") {
        bail!("publish takes a workshop asset; copy {name} to WORKSHOP/ first");
    }
    store::validate_pack(pack).map_err(|e| anyhow!("pack: {e}"))?;
    let review = line(review);
    if review.is_empty() || review.chars().count() > 200 {
        bail!("review= says what the sheet review found (1-200 characters), e.g. \"ok: reads at game distance, front +z\"");
    }
    if reviewed != def.revision {
        bail!("{name} is at revision {}, not the reviewed {reviewed}: look at its sheet again", def.revision);
    }
    let folder = root.join("lib").join(pack);
    let pack_info = read_pack(&folder)?;
    let mut found = findings;
    found.extend(gate(def, &pack_info));
    if !found.is_empty() {
        bail!("{name} does not pass the library gate: {}", found.join("; "));
    }
    let leaf = props::leaf(name);
    let published = format!("LIB/{pack}/{leaf}");
    let mut ledger: Vec<_> = reviews(&folder)?.into_iter().filter(|(row, _, _)| *row != published).collect();
    ledger.push((published.clone(), def.revision.clone(), review));
    ledger.sort();
    let mut text = String::from("name\trevision\treview\n");
    for (row, revision, review) in &ledger {
        text.push_str(&format!("{row}\t{revision}\t{review}\n"));
    }
    // The asset, its review and the index change together: a failure puts the files back.
    let file = folder.join(format!("{leaf}.json"));
    let ledger_file = folder.join("review.tsv");
    let old = [&file, &ledger_file].map(|path| std::fs::read(path).ok());
    let written = std::fs::write(&file, def.doc.text())
        .and_then(|()| std::fs::write(&ledger_file, text))
        .map_err(|e| anyhow!("write {}: {e}", folder.display()))
        .and_then(|()| crate::asset_search::reindex(root, pack));
    if let Err(error) = written {
        for (path, bytes) in [&file, &ledger_file].into_iter().zip(old) {
            let _ = match bytes {
                Some(bytes) => std::fs::write(path, bytes),
                None => std::fs::remove_file(path),
            };
        }
        return Err(error);
    }
    let mut result = json!({"published": published, "revision": def.revision, "file": file});
    if pack == store::CORE {
        result["note"] = json!("LIB/core is embedded: rebuild to load it");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pav_core::props::Doc;

    #[test]
    fn library_assets_name_no_workshop_materials() {
        let text = r##"{"format":2,"name":"post","unit":"cm","description":"A post.","category":"town","tags":["wood","post"],
            "parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"mat":"MAT/workshop/mine"}}}"##;
        let def = PropDef::new(Doc::read(text).unwrap()).unwrap();
        let core = Pack {
            pack: "core".into(),
            title: "Core".into(),
            license: "CC0-1.0".into(),
            credit: "Test".into(),
            source: "own".into(),
            catalog: None,
        };
        let found = gate(&def, &core);
        assert_eq!(found, ["materials: MAT/workshop/mine is a workshop material; use a core MAT/ name or write it inline"]);
        // Through an inline material's base, too.
        let based = text
            .replace(r#""mat":"MAT/workshop/mine""#, r#""mat":"m""#)
            .replace(r#""tags":["wood","post"],"#, r#""tags":["wood","post"],"materials":{"m":{"base":"MAT/workshop/other"}},"#);
        let def = PropDef::new(Doc::read(&based).unwrap()).unwrap();
        assert_eq!(
            gate(&def, &core),
            ["materials: MAT/workshop/other is a workshop material; use a core MAT/ name or write it inline"]
        );
    }
}
