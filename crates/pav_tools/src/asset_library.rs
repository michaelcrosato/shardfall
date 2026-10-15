//! The curated library (docs/ASSET_SYSTEM_PLAN.md §9.3, §9.5): each pack's licence
//! (`PACK.json`), the credits file, the gate a library asset passes, and
//! `asset_edit action=publish`, which writes a checked workshop asset into a pack with its index
//! row and its review; the lineage a fork records, and the store's generated table
//! (`store/STORE.tsv`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::props::doc::{Category, Lineage, Method};
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

/// A pack's `PACK.json` under `root` (`lib/`, else `local/`), read as written: a fork copies
/// its licence and credit whatever the licence is.
fn pack_file(root: &Path, pack: &str) -> Option<Pack> {
    ["lib", "local"].iter().find_map(|tier| {
        let text = std::fs::read_to_string(root.join(tier).join(pack).join("PACK.json")).ok()?;
        serde_json::from_str(&text).ok()
    })
}

/// The lineage `asset_edit action=copy from=X` writes into its fork (docs/AGENT_ASSET_STORE.md
/// §9): parent `X@<revision>`, method `edited`, and X's origin, licence (as an SPDX id) and
/// credit, from X's own lineage or what its document says (`Doc::provenance`: an import's origin
/// is `import`, own work's `agent`). A pack asset's credit is its `PACK.json` line unless X
/// writes a lineage of its own. None for a source format 1 holds: its copy stays format 1,
/// which has no place for one.
pub fn fork_lineage(root: &Path, from: &str, def: &PropDef) -> Option<Lineage> {
    if def.doc.needed_format() == 1 {
        return None;
    }
    let parent = def.doc.provenance();
    let pack = store::pack(from).and_then(|pack| pack_file(root, pack));
    let license = match parent.license {
        Some(text) => Some(pav_core::store::spdx(&text).map(str::to_owned).unwrap_or(text)),
        None => pack.as_ref().map(|pack| pack.license.clone()),
    };
    let credit = match (&def.doc.lineage, pack) {
        (None, Some(pack)) => Some(pack.credit),
        (_, pack) => parent.credit.or(pack.map(|pack| pack.credit)),
    };
    Some(Lineage {
        method: Method::Edited,
        origin: parent.origin,
        parents: vec![format!("{from}@{}", def.revision)],
        license,
        credit,
    })
}

/// What keeps a written lineage from being complete provenance in `lib/<pack>` (§6's gate): an
/// edit names its parents, the licence is the pack's, and a licence that needs a credit has one.
fn provenance_problems(def: &PropDef, pack: &Pack) -> Vec<String> {
    let Some(lineage) = &def.doc.lineage else { return Vec::new() };
    let mut found = Vec::new();
    if lineage.method == Method::Edited && lineage.parents.is_empty() {
        found.push("provenance: an edited asset names its parents (lineage.parents)".to_owned());
    }
    match lineage.license.as_deref() {
        None => found.push("provenance: its lineage names no licence".to_owned()),
        Some(license) if pav_core::store::spdx(license) != Some(pack.license.as_str()) => found.push(format!(
            "provenance: its lineage's licence {license} is not the pack's ({}); keep it in a pack of its own licence",
            pack.license
        )),
        Some(license) => {
            if pav_core::store::admitted(license, "prop").is_ok_and(|licence| licence.credit) && lineage.credit.is_none() {
                found.push(format!("provenance: {license} needs the credit line (lineage.credit)"));
            }
        }
    }
    found
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

/// The columns of `store/STORE.tsv` (docs/AGENT_ASSET_STORE.md §5).
/// `foundation` is the archetype an entry founds (`store/foundations.tsv`), else `-`.
pub const STORE_HEADER: &str = "id\tkind\ttier\tlicence\tmethod\torigin\tsize_cm\ttok\tfoundation\ttags\tdesc";

/// One row of `store/STORE.tsv`.
struct Entry {
    id: String,
    kind: &'static str,
    tier: String,
    licence: &'static str,
    method: String,
    origin: String,
    size_cm: String,
    tok: usize,
    tags: Vec<String>,
    desc: String,
}

/// A JSON value's strings: a list's (at most `max`), or none.
fn strings(value: &Value, max: usize) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).take(max).map(str::to_owned).collect()
}

/// The store's entries of objects: every row of the templates' and committed packs' indexes
/// under `props_root` (never `local/`), each with its origin from its file's lineage.
fn prop_entries(props_root: &Path) -> Result<Vec<Entry>> {
    let mut folders = vec![props_root.join("templates")];
    folders.extend(committed_packs(props_root).into_iter().map(|(_, folder)| folder));
    let mut out = Vec::new();
    for folder in folders {
        let path = folder.join("index.tsv");
        let text = std::fs::read_to_string(&path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        for row in index::parse(&text, Tier::Disk).map_err(|e| anyhow!("{}: {e}", path.display()))? {
            if pav_core::store::local_path(&row.name) {
                continue;
            }
            let licence = pav_core::store::admitted(&row.licence, "prop").map_err(|e| anyhow!("{}: refused: {e}", row.name))?;
            let file = folder.join(format!("{}.json", props::leaf(&row.name)));
            let value: Value = std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .ok_or_else(|| anyhow!("{} does not read", file.display()))?;
            let origin = match value["lineage"]["origin"].as_str() {
                Some(origin) => origin,
                None if row.method == "imported" => "import",
                None if row.method == "generated" => "generator",
                None => "agent",
            };
            out.push(Entry {
                id: row.name.clone(),
                kind: index::KIND,
                tier: row.level.clone(),
                licence: licence.id,
                method: row.method.clone(),
                origin: origin.to_owned(),
                size_cm: row.size_cm.clone(),
                tok: row.tok,
                tags: row.tags.clone(),
                desc: row.desc.clone(),
            });
        }
    }
    Ok(out)
}

/// The store's entries of clips: every clip of the embedded clip sets whose licence the store
/// admits. CC0 clips are `standard`; CC-BY and CMU's terms top out at `checked` (§7, §14 Q3, Q4).
fn clip_entries() -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for name in pav_core::anim::files("json") {
        let text = pav_core::anim::source(&name).unwrap_or_default();
        let set: Value = serde_json::from_str(&text).map_err(|e| anyhow!("anim/{name}: {e}"))?;
        let set_name = set["set"].as_str().ok_or_else(|| anyhow!("anim/{name}: no set"))?;
        for (clip_name, clip) in set["clips"].as_object().into_iter().flatten() {
            let source = clip["src"].as_str().unwrap_or_default();
            let licence_text = set["sources"][source]["license"].as_str().or(set["credit"].as_str()).unwrap_or_default();
            let Ok(licence) = pav_core::store::admitted(licence_text, "clip") else { continue };
            out.push(Entry {
                id: format!("{set_name}/{clip_name}"),
                kind: "clip",
                tier: if licence.id == "CC0-1.0" { "standard" } else { "checked" }.into(),
                licence: licence.id,
                method: "imported".into(),
                origin: "import".into(),
                size_cm: "-".into(),
                tok: index::tokens(clip.to_string().len()),
                tags: strings(&clip["tags"], 6),
                desc: clip["desc"].as_str().unwrap_or_default().to_owned(),
            });
        }
    }
    Ok(out)
}

/// The store's entries of materials: the core materials (`assets/materials`, embedded), own
/// work or imported from a CC0 source, all `standard`.
fn material_entries() -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for (name, text) in pav_core::material::library::MATERIALS {
        let doc: Value = serde_json::from_str(text).map_err(|e| anyhow!("MAT/{name}: {e}"))?;
        let source = &doc["source"];
        let own = source.is_null() || source["library"] == "own";
        let licence_text = source["license"].as_str().unwrap_or("own");
        let licence = pav_core::store::admitted(licence_text, "material").map_err(|e| anyhow!("MAT/{name}: refused: {e}"))?;
        out.push(Entry {
            id: format!("{}{name}", pav_core::material::PREFIX),
            kind: "material",
            tier: "standard".into(),
            licence: licence.id,
            method: if own { "authored" } else { "imported" }.into(),
            origin: if own { "agent" } else { "import" }.into(),
            size_cm: "-".into(),
            tok: index::tokens(text.len()),
            tags: strings(&doc["tags"], 6),
            desc: doc["description"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Ok(out)
}

/// `store/STORE.tsv`: one row per store entry (docs/AGENT_ASSET_STORE.md §5), objects from the
/// committed indexes under `props_root`, then the embedded clips and the core materials, each
/// kind by id. The header says it is generated and reports each kind's share by origin (§13 S6);
/// the `foundation` column marks the foundations (`store/foundations.tsv`, §4).
pub fn store_table(props_root: &Path) -> Result<String> {
    let mut kinds = [prop_entries(props_root)?, clip_entries()?, material_entries()?];
    let mut shares = Vec::new();
    for entries in &mut kinds {
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        let Some(first) = entries.first() else { continue };
        let mut origins: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in entries.iter() {
            *origins.entry(entry.origin.as_str()).or_default() += 1;
        }
        let imported = origins.get("import").copied().unwrap_or(0);
        let counts: Vec<String> = origins.iter().map(|(origin, n)| format!("{origin} {n}")).collect();
        shares.push(format!(
            "{} {} ({}% imported: {})",
            first.kind,
            entries.len(),
            imported * 100 / entries.len(),
            counts.join(", ")
        ));
    }
    let mut text = format!(
        "# Every store entry, one row each (docs/AGENT_ASSET_STORE.md §5). Generated by `pav assets reindex=store`; do not edit.\n\
         # By origin: {}. foundation: the archetype a prop founds (store/foundations.tsv), else -.\n{STORE_HEADER}\n",
        shares.join("; ")
    );
    for entry in kinds.iter().flatten() {
        let tok = entry.tok.to_string();
        let foundation = match entry.kind {
            index::KIND => pav_core::store::foundation(&entry.id).map_or("-", |foundation| foundation.archetype),
            _ => "-",
        };
        let cells: [&str; 9] =
            [&entry.id, entry.kind, &entry.tier, entry.licence, &entry.method, &entry.origin, &entry.size_cm, &tok, foundation];
        let fixed = cells.join("\t");
        // As many tags as leave the description a few words, then as much of it as fits the row.
        let mut tags = entry.tags.clone();
        while fixed.len() + tags.join(" ").len() + 2 > STORE_ROW_BYTES - 16 && !tags.is_empty() {
            tags.pop();
        }
        let head = format!("{fixed}\t{}\t", tags.join(" "));
        let room = STORE_ROW_BYTES.saturating_sub(head.len()).min(48);
        let desc = entry.desc.split_whitespace().collect::<Vec<_>>().join(" ");
        text.push_str(&head);
        if room >= 12 {
            text.push_str(&crate::asset_search::cut(&desc, room));
        }
        text.push('\n');
    }
    Ok(text)
}

/// A `STORE.tsv` row's budget: about 50 tokens (§5), at `index::tokens`' 2.8 bytes a token.
pub const STORE_ROW_BYTES: usize = 140;

/// `assets reindex=store`: writes the store's generated files (`CREDITS.md`, `STORE.tsv`) under
/// the store folder (`$PAV_STORE`, else `./store`).
pub fn write_store(props_root: &Path) -> Result<Value> {
    let dir = crate::asset_search::store_root();
    if !dir.is_dir() {
        bail!("no store folder at {} (run from the repository, or set PAV_STORE)", dir.display());
    }
    let credits = pav_core::store::credits_text(&credit_sources(props_root)?);
    let table = store_table(props_root)?;
    let mut rows: BTreeMap<&str, usize> = BTreeMap::new();
    for line in table.lines().filter(|line| !line.starts_with('#')).skip(1) {
        *rows.entry(line.split('\t').nth(1).unwrap_or_default()).or_default() += 1;
    }
    let (path, store) = (dir.join("CREDITS.md"), dir.join("STORE.tsv"));
    std::fs::write(&path, &credits).map_err(|e| anyhow!("write {}: {e}", path.display()))?;
    std::fs::write(&store, &table).map_err(|e| anyhow!("write {}: {e}", store.display()))?;
    let lines = credits.lines().filter(|line| line.starts_with("- ")).count();
    Ok(json!({"written": [path, store], "credits_lines": lines, "store_rows": rows}))
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
/// materials, its source's licence and a written lineage's provenance.
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
    found.extend(provenance_problems(def, pack));
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

    #[test]
    fn a_written_lineage_must_be_complete_provenance() {
        let text = r##"{"format":2,"name":"post","unit":"cm","description":"A post.","category":"town","tags":["wood","post"],
            "lineage":{"method":"edited","origin":"import","license":"CC-BY-4.0"},
            "parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040"}}}"##;
        let def = PropDef::new(Doc::read(text).unwrap()).unwrap();
        let pack = |license: &str| Pack {
            pack: "p".into(),
            title: "P".into(),
            license: license.into(),
            credit: "Test".into(),
            source: "own".into(),
            catalog: None,
        };
        let parents = "provenance: an edited asset names its parents (lineage.parents)";
        assert_eq!(
            gate(&def, &pack("CC0-1.0")),
            [
                parents,
                "provenance: its lineage's licence CC-BY-4.0 is not the pack's (CC0-1.0); keep it in a pack of its own licence"
            ]
        );
        assert_eq!(gate(&def, &pack("CC-BY-4.0")), [parents, "provenance: CC-BY-4.0 needs the credit line (lineage.credit)"]);
        let complete = text.replace(
            r#""license":"CC-BY-4.0""#,
            r#""license":"CC-BY-4.0","credit":"Maker","parents":["LIB/maker/post@prop2-1"]"#,
        );
        let def = PropDef::new(Doc::read(&complete).unwrap()).unwrap();
        assert!(gate(&def, &pack("CC-BY-4.0")).is_empty());
    }
}
