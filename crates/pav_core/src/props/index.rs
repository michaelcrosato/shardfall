//! The asset index (docs/ASSET_SYSTEM_PLAN.md §9.2): one row per asset of a folder, kept in
//! `index.tsv` beside its files, so a query reads the index instead of every asset. Tools write
//! it (`assets reindex=`); a person or agent may edit `note`, and `cat` and `tags` of format-1
//! assets, which their documents cannot hold.

use std::path::Path;
use std::sync::OnceLock;

use super::store::{self, Tier};
use super::{PropDef, compile};
use crate::issue;

pub const HEADER: &str = "name\tcat\tsize_cm\tparts\ttok\tfid\tsrc\tkind\tlicence\tmethod\ttier\ttags\tsockets\tnote\tdesc";

/// The `kind` column of an object index (docs/AGENT_ASSET_STORE.md §5).
pub const KIND: &str = "prop";

/// One asset as queries see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The canonical name, e.g. `LIB/core/barrel`.
    pub name: String,
    /// Its category (`furniture`, `container`, …), or empty.
    pub cat: String,
    /// Width x height x depth in centimetres.
    pub size_cm: String,
    /// Parts after copies.
    pub parts: usize,
    /// Tokens of its canonical file: ceil(bytes / 2.8).
    pub tok: usize,
    /// `authored`, `fit93` (silhouette IoU %) or `mesh40` (% of area in mesh parts).
    pub fid: String,
    /// `own`, or the source's licence and library, e.g. `CC0-1.0 kenney`.
    pub src: String,
    /// Its licence's SPDX id (own work is CC0-1.0, §14 Q1), from its lineage or source.
    pub licence: String,
    /// `authored`, `imported`, `edited` or `generated` (`Doc::provenance`).
    pub method: String,
    /// Its store tier (docs/AGENT_ASSET_STORE.md §7): `standard` (embedded), `checked` (on disk,
    /// passing every check), `disk`; `local` and `workshop` rows are not store entries.
    pub level: String,
    pub tags: Vec<String>,
    pub sockets: Vec<String>,
    /// Free text kept across reindexing.
    pub note: String,
    /// The description on one line.
    pub desc: String,
    /// Where the row was read from (not written).
    pub tier: Tier,
}

/// A row's store tier before the checks that can raise it: embedded rows are `standard`.
pub fn level(tier: Tier) -> &'static str {
    match tier {
        Tier::Embedded => "standard",
        Tier::Disk => "disk",
        Tier::Local => "local",
        Tier::Workshop => "workshop",
    }
}

/// Tokens of a file of `bytes` bytes: about 2.8 bytes per token for asset JSON.
pub fn tokens(bytes: usize) -> usize {
    (bytes * 10).div_ceil(28)
}

/// A row's licence column: the SPDX id a licence text names, else the text itself (a licence
/// the store does not admit, on a local row), else `unknown`.
pub fn licence_id(text: Option<&str>) -> String {
    match text {
        Some(text) => crate::store::spdx(text).map(str::to_owned).unwrap_or_else(|| field(text).replace(' ', "_")),
        None => "unknown".into(),
    }
}

/// One line of text for a TSV field.
fn field(text: &str) -> String {
    text.split(['\t', '\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

impl Row {
    /// The row of a definition.
    pub fn of(name: &str, def: &PropDef, tier: Tier) -> Self {
        let doc = &def.doc;
        let fid = match &doc.fit {
            Some(fit) if fit.mesh_pct.is_some_and(|pct| pct > 0.0) => format!("mesh{}", fit.mesh_pct.unwrap_or(0.0).round()),
            Some(fit) if fit.iou.is_some() => format!("fit{}", (fit.iou.unwrap_or(0.0) * 100.0).round()),
            _ => "authored".into(),
        };
        let src = match &doc.source {
            Some(source) => {
                let words: Vec<&str> = [&source.license, &source.library].into_iter().flatten().map(String::as_str).collect();
                if words.is_empty() { "unknown".into() } else { field(&words.join(" ")) }
            }
            None => "own".into(),
        };
        let lineage = doc.provenance();
        Self {
            name: name.to_owned(),
            cat: doc.category.map(|cat| cat.name().to_owned()).unwrap_or_default(),
            size_cm: def.bounds(1.0).size_cm(),
            parts: def.compiled.parts.len(),
            tok: tokens(doc.text().len()),
            fid,
            src,
            licence: licence_id(lineage.license.as_deref()),
            method: lineage.method.name().into(),
            level: level(tier).into(),
            tags: doc.tags.clone(),
            sockets: doc.sockets.keys().cloned().collect(),
            note: String::new(),
            desc: field(&doc.description),
            tier,
        }
    }

    /// The size in centimetres, [width, height, depth].
    pub fn size(&self) -> [f32; 3] {
        let mut size = [0.0; 3];
        for (slot, axis) in size.iter_mut().zip(self.size_cm.split('x')) {
            *slot = axis.parse().unwrap_or(0.0);
        }
        size
    }

    fn line(&self) -> String {
        [
            self.name.clone(),
            self.cat.clone(),
            self.size_cm.clone(),
            self.parts.to_string(),
            self.tok.to_string(),
            self.fid.clone(),
            self.src.clone(),
            KIND.into(),
            self.licence.clone(),
            self.method.clone(),
            self.level.clone(),
            self.tags.join(" "),
            self.sockets.join(" "),
            field(&self.note),
            self.desc.clone(),
        ]
        .join("\t")
    }
}

/// Read an index. Rows keep the file's order.
pub fn parse(text: &str, tier: Tier) -> Result<Vec<Row>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(format!("the first line must be the header: {}", HEADER.replace('\t', " ")));
    }
    let words = |text: &str| text.split_whitespace().map(String::from).collect::<Vec<_>>();
    lines
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, line)| {
            let cells: Vec<&str> = line.split('\t').collect();
            let [name, cat, size_cm, parts, tok, fid, src, kind, licence, method, level, tags, sockets, note, desc] = cells[..]
            else {
                return Err(format!("line {}: {} fields (expected 15)", i + 2, cells.len()));
            };
            if kind != KIND {
                return Err(format!("line {}: kind \"{kind}\" (an object index holds {KIND} rows)", i + 2));
            }
            let number = |what: &str, text: &str| text.parse::<usize>().map_err(|_| format!("line {}: {what} \"{text}\"", i + 2));
            Ok(Row {
                name: store::canonical(name).map_err(|e| format!("line {}: {e}", i + 2))?,
                cat: cat.into(),
                size_cm: size_cm.into(),
                parts: number("parts", parts)?,
                tok: number("tok", tok)?,
                fid: fid.into(),
                src: src.into(),
                licence: licence.into(),
                method: method.into(),
                level: level.into(),
                tags: words(tags),
                sockets: words(sockets),
                note: note.into(),
                desc: desc.into(),
                tier,
            })
        })
        .collect()
}

/// The index file's text: the header, then a row per asset by name.
pub fn text(rows: &[Row]) -> String {
    let mut rows: Vec<&Row> = rows.iter().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    std::iter::once(HEADER.to_owned()).chain(rows.iter().map(|row| row.line())).map(|line| line + "\n").collect()
}

/// Rows for every asset file directly in `folder` (not its licence, `PACK.json`), named
/// `<set>/<leaf>` (`BUILTIN` for the templates, `LIB/<pack>` for a pack). Each keeps its old row's note, and its category and
/// tags when the document has none: format 1 cannot hold them.
pub fn build(folder: &Path, set: &str, old: &[Row], deps: compile::Deps) -> Result<Vec<Row>, String> {
    let mut paths: Vec<_> = std::fs::read_dir(folder)
        .map_err(|e| format!("read {}: {e}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path.extension().is_some_and(|ext| ext == "json")
                && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.') && name != "PACK.json")
        })
        .collect();
    paths.sort();
    let tier = match set {
        "BUILTIN" => Tier::Embedded,
        set if set == format!("LIB/{}", store::CORE) => Tier::Embedded,
        _ if folder.parent().and_then(Path::file_name).is_some_and(|name| name == "local") => Tier::Local,
        _ => Tier::Disk,
    };
    paths
        .iter()
        .map(|path| {
            let leaf = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
            let name = store::canonical(&format!("{set}/{leaf}"))?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
            let error = |issues: Vec<issue::Issue>| format!("{}: {}", path.display(), issue::render(&issues));
            let doc =
                super::Doc::from_value_in(&super::read::strict_value(&text).map_err(|e| error(vec![e]))?, deps).map_err(error)?;
            if doc.name != leaf {
                return Err(format!("{} must contain name \"{leaf}\", not \"{}\"", path.display(), doc.name));
            }
            let def = PropDef::with_deps(doc, deps, None).map_err(error)?;
            let mut row = Row::of(&name, &def, tier);
            if let Some(previous) = old.iter().find(|previous| previous.name == name) {
                row.note = previous.note.clone();
                if def.doc.category.is_none() {
                    row.cat = previous.cat.clone();
                }
                if def.doc.tags.is_empty() {
                    row.tags = previous.tags.clone();
                }
            }
            Ok(row)
        })
        .collect()
}

/// The embedded rows: the templates' and `LIB/core`'s indexes.
pub fn embedded() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| {
        ["templates/index.tsv", "lib/core/index.tsv"]
            .iter()
            .filter_map(|path| store::embedded_file(path).map(|text| (path, text)))
            .flat_map(|(path, text)| parse(text, Tier::Embedded).unwrap_or_else(|e| panic!("embedded {path}: {e}")))
            .collect()
    })
}

/// The folders of the packs on disk under `root`: (pack, folder, tier). `lib/core` is
/// embedded, and `local/workshop` holds `WORKSHOP_LOCAL`.
pub fn disk_packs(root: &Path) -> Vec<(String, std::path::PathBuf, Tier)> {
    let mut packs = Vec::new();
    for (folder, tier) in [("lib", Tier::Disk), ("local", Tier::Local)] {
        let Ok(entries) = std::fs::read_dir(root.join(folder)) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Some(pack) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else { continue };
            if path.is_dir() && pack != store::CORE && store::validate_pack(&pack).is_ok() {
                packs.push((pack, path, tier));
            }
        }
    }
    packs.sort();
    packs
}

/// The rows of the packs on disk under `root` (native builds), and the indexes that would not
/// read. A `local/` pack that shares a `lib/` pack's name is reported and left out: its names
/// would load the `lib/` files.
pub fn disk(root: &Path) -> (Vec<Row>, Vec<String>) {
    let (mut rows, mut errors) = (Vec::new(), Vec::new());
    if cfg!(target_arch = "wasm32") {
        return (rows, errors);
    }
    let packs = disk_packs(root);
    let shared: Vec<&str> = packs.iter().filter(|(_, _, tier)| *tier == Tier::Disk).map(|(pack, _, _)| pack.as_str()).collect();
    for (pack, folder, tier) in &packs {
        if *tier == Tier::Local && shared.contains(&pack.as_str()) {
            errors.push(format!("{} is hidden by lib/{pack}: rename one of the packs", folder.display()));
            continue;
        }
        let path = folder.join("index.tsv");
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text, *tier) {
                Ok(found) => rows.extend(found),
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            },
            Err(_) => errors.push(format!("{} is missing: assets reindex=<pack> writes it", path.display())),
        }
    }
    (rows, errors)
}
