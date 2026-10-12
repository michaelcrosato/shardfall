//! `assets`: find library and workshop assets (docs/ASSET_SYSTEM_PLAN.md §9.2). Queries read
//! index rows (the embedded ones, then each pack's `index.tsv` on disk); workshop rows are made
//! live from the library. A listing row stays within about 40 tokens.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use pav_core::issue::{self, Code, Issue};
use pav_core::props::doc::Category;
use pav_core::props::index::{self, Row};
use pav_core::props::{self, Tier};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_str};

/// The arguments `assets` takes (`kind` is routed before it arrives).
const ARGS: &[&str] = &["kind", "find", "category", "tags", "tier", "similar", "limit", "page", "reindex"];

/// A listing row's JSON is cut to this many bytes (its description first): at most about 39
/// o200k tokens, as o200k reads these rows at 3.0-3.5 bytes per token.
pub const ROW_BYTES: usize = 116;
/// Tags a row shows (sockets come after them); the index keeps them all for `find`.
const ROW_TAGS: usize = 4;

pub const COLS: &str = "name cat size_cm parts tok tags desc";

/// What a query asks for. Every word of `words` must match; `similar` ranks by likeness.
#[derive(Default)]
pub(crate) struct Query {
    pub words: Vec<String>,
    pub category: Option<String>,
    /// Tags that must all be present; `@sit` names a socket.
    pub tags: Vec<String>,
    pub tier: Option<Tier>,
    pub similar: Option<Row>,
}

/// A row's score for `words`: each word scores where it matches best (in the leaf 5, a tag,
/// socket or the category 3, the name 2, the description 1). None: a word matches nowhere.
fn find_score(row: &Row, words: &[String]) -> Option<u32> {
    let (leaf, name, desc) = (props::leaf(&row.name), row.name.to_ascii_lowercase(), row.desc.to_ascii_lowercase());
    words
        .iter()
        .map(|word| {
            let word = word.as_str();
            if leaf.contains(word) {
                Some(5)
            } else if row.cat.starts_with(word)
                || row.tags.iter().any(|tag| tag.starts_with(word))
                || row.sockets.iter().any(|socket| socket == word)
            {
                Some(3)
            } else if name.contains(word) {
                Some(2)
            } else if desc.contains(word) {
                Some(1)
            } else {
                None
            }
        })
        .sum()
}

/// How alike two assets are, deterministically: the category 3, each shared tag 2 (up to 3),
/// each shared socket 1, then up to 2 for the largest side and 1 for the part count, falling to
/// 0 at 4 times apart.
fn similarity(a: &Row, b: &Row) -> f32 {
    let close = |x: f32, y: f32| if x > 0.0 && y > 0.0 { (1.0 - (x / y).ln().abs() / 4f32.ln()).max(0.0) } else { 0.0 };
    let largest = |row: &Row| row.size().into_iter().fold(0.0f32, f32::max);
    let category = if !a.cat.is_empty() && a.cat == b.cat { 3.0 } else { 0.0 };
    let tags = a.tags.iter().filter(|tag| b.tags.contains(tag)).count().min(3) as f32 * 2.0;
    let sockets = a.sockets.iter().filter(|socket| b.sockets.contains(socket)).count() as f32;
    category + tags + sockets + 2.0 * close(largest(a), largest(b)) + close(a.parts as f32, b.parts as f32)
}

/// The rows that pass the query's filters, best first: by likeness with `similar`, else by the
/// find score; ties go to fewer tokens, then the name. Without either, by name.
pub(crate) fn rank(rows: Vec<Row>, query: &Query) -> Vec<Row> {
    let mut scored: Vec<(f32, Row)> = rows
        .into_iter()
        .filter(|row| query.category.as_ref().is_none_or(|cat| row.cat == *cat))
        .filter(|row| query.tier.is_none_or(|tier| row.tier == tier))
        .filter(|row| {
            query.tags.iter().all(|tag| match tag.strip_prefix('@') {
                Some(socket) => row.sockets.iter().any(|s| s == socket),
                None => row.tags.contains(tag),
            })
        })
        .filter(|row| query.similar.as_ref().is_none_or(|like| like.name != row.name))
        .filter_map(|row| {
            let found = if query.words.is_empty() { 0 } else { find_score(&row, &query.words)? };
            let score = match &query.similar {
                // The find score only breaks ties of likeness.
                Some(like) => similarity(like, &row) + found as f32 / 1000.0,
                None => found as f32,
            };
            Some((score, row))
        })
        .collect();
    let ranked = !query.words.is_empty() || query.similar.is_some();
    scored.sort_by(|(sa, a), (sb, b)| {
        let by_score = if ranked { sb.total_cmp(sa) } else { std::cmp::Ordering::Equal };
        by_score.then(if ranked { a.tok.cmp(&b.tok) } else { std::cmp::Ordering::Equal }).then_with(|| a.name.cmp(&b.name))
    });
    scored.into_iter().map(|(_, row)| row).collect()
}

/// `text` cut at a word to at most `max` bytes, with … when cut.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.into();
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = text[..end].rfind(' ').filter(|at| *at > end / 2).unwrap_or(end);
    format!("{}…", text[..cut].trim_end_matches([',', ';', ':', ' ', '.']))
}

/// One listing row, `COLS`: the first tags, then sockets as `@name`, then as much of the
/// description as keeps the row within `ROW_BYTES`.
pub(crate) fn listing_row(row: &Row) -> Value {
    let sockets = row.sockets.iter().map(|s| format!("@{s}"));
    let mut tags: Vec<String> = row.tags.iter().take(ROW_TAGS).cloned().chain(sockets).collect();
    let fixed = |tags: &[String]| json!([row.name, row.cat, row.size_cm, row.parts, row.tok, tags.join(" "), ""]);
    while fixed(&tags).to_string().len() > ROW_BYTES - 16 && !tags.is_empty() {
        tags.pop();
    }
    let mut value = fixed(&tags);
    let mut room = ROW_BYTES.saturating_sub(value.to_string().len());
    // A few words would say nothing; leave them out. Escaped quotes take more room.
    while room >= 12 {
        value[6] = json!(cut(&row.desc, room));
        let over = value.to_string().len().saturating_sub(ROW_BYTES);
        if over == 0 {
            return value;
        }
        room -= over;
    }
    value[6] = json!("");
    value
}

/// Every row a query reads: the embedded index, the packs on disk under `root`, and the
/// workshop's assets made live. Also the indexes that would not read.
fn all_rows(root: &Path) -> (Vec<Row>, Vec<String>) {
    let mut rows = index::embedded().to_vec();
    let (disk, errors) = index::disk(root);
    rows.extend(disk);
    let library = props::library();
    rows.extend(library.defs.iter().filter(|(name, _)| props::editable(name)).map(|(name, def)| {
        let tier = if name.starts_with("WORKSHOP_LOCAL/") { Tier::Local } else { Tier::Workshop };
        Row::of(name, def, tier)
    }));
    (rows, errors)
}

fn number(a: &Args, key: &str, default: usize, range: std::ops::RangeInclusive<usize>) -> Result<usize> {
    match a.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .map(|value| value as usize)
            .filter(|value| range.contains(value))
            .ok_or_else(|| anyhow!("{key} must be an integer from {} to {}", range.start(), range.end())),
    }
}

/// `templates` or a pack: write the index.tsv of its folder under `root`.
fn reindex(root: &Path, target: &str) -> Result<Value> {
    let (folder, set) = match target {
        "templates" => (root.join("templates"), "BUILTIN".to_owned()),
        pack => {
            props::store::validate_pack(pack).map_err(|e| anyhow!("reindex: {e}"))?;
            let folder = ["lib", "local"].iter().map(|tier| root.join(tier).join(pack)).find(|path| path.is_dir());
            let folder =
                folder.ok_or_else(|| anyhow!("reindex: no folder lib/{pack} or local/{pack} under {}", root.display()))?;
            (folder, format!("LIB/{pack}"))
        }
    };
    let path = folder.join("index.tsv");
    let old = match std::fs::read_to_string(&path) {
        Ok(text) => index::parse(&text, Tier::Disk).map_err(|e| anyhow!("{}: {e}", path.display()))?,
        Err(_) => Vec::new(),
    };
    let rows = index::build(&folder, &set, &old, &|name| props::resolve_in(root, name).ok().flatten()).map_err(|e| anyhow!(e))?;
    std::fs::write(&path, index::text(&rows)).map_err(|e| anyhow!("write {}: {e}", path.display()))?;
    Ok(json!({"reindexed": path, "rows": rows.len()}))
}

/// Search the library and the workshop. Rows are compact; `asset_edit inspect` returns one
/// definition.
pub fn t_assets(_: &mut Session, a: &Args) -> Result<Output> {
    for (key, value) in a {
        if ["scene", "seed", "ticks"].contains(&key.as_str()) {
            continue;
        }
        if !ARGS.contains(&key.as_str()) {
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, "not an assets argument")
                    .expected(ARGS.join(", "))
                    .maybe_fix(issue::suggestion(key, ARGS.iter().copied()))
            );
        }
        if ["find", "category", "tags", "tier", "similar", "reindex"].contains(&key.as_str()) && !value.is_string() {
            bail!("{key} must be text");
        }
    }
    let root = props::disk_root();
    if let Some(target) = get_str(a, "reindex") {
        if a.len() > 1 {
            bail!("reindex takes no other arguments");
        }
        return Ok(Output::Json(reindex(&root, target)?));
    }
    let limit = number(a, "limit", 20, 1..=100)?;
    let page = number(a, "page", 1, 1..=usize::MAX)?;
    let words = |key: &str| -> Vec<String> {
        get_str(a, key).unwrap_or("").to_ascii_lowercase().split([' ', ',']).filter(|w| !w.is_empty()).map(String::from).collect()
    };
    let category = match get_str(a, "category") {
        None => None,
        Some(text) => Some(
            Category::parse(&text.to_ascii_lowercase())
                .ok_or_else(|| {
                    anyhow!(
                        "{}",
                        Issue::error("category", Code::UnknownValue, format!("unknown category \"{text}\""))
                            .expected(Category::NAMES.join(", "))
                            .maybe_fix(issue::suggestion(text, Category::NAMES.iter().copied()))
                    )
                })?
                .name()
                .to_owned(),
        ),
    };
    let tier = match get_str(a, "tier") {
        None => None,
        Some(text) => Some(Tier::parse(&text.to_ascii_lowercase()).ok_or_else(|| {
            anyhow!(
                "{}",
                Issue::error("tier", Code::UnknownValue, format!("unknown tier \"{text}\""))
                    .expected(Tier::NAMES.join(", "))
                    .maybe_fix(issue::suggestion(text, Tier::NAMES.iter().copied()))
            )
        })?),
    };
    let (rows, errors) = all_rows(&root);
    let similar = match get_str(a, "similar") {
        None => None,
        Some(name) => {
            let wanted = props::canonical(name).map_err(|e| anyhow!("similar: {e}"))?;
            let found = rows.iter().find(|row| row.name == wanted).cloned();
            Some(found.ok_or_else(|| {
                let fix = issue::suggestion(&wanted, rows.iter().map(|row| row.name.as_str()));
                anyhow!("{}", Issue::error("similar", Code::UnknownAsset, format!("no asset \"{name}\"")).maybe_fix(fix))
            })?)
        }
    };
    let query = Query { words: words("find"), category, tags: words("tags"), tier, similar };
    let matched = rank(rows, &query);
    let skip = (page - 1).saturating_mul(limit);
    let rows: Vec<Value> = matched.iter().skip(skip).take(limit).map(listing_row).collect();
    let mut result = json!({"cols": COLS, "rows": rows, "matched": matched.len()});
    if skip.saturating_add(limit) < matched.len() {
        result["next"] = json!(page + 1);
    }
    if !errors.is_empty() {
        result["errors"] = json!(errors);
    }
    Ok(Output::Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, cat: &str, size: &str, parts: usize, tok: usize, tags: &str, desc: &str) -> Row {
        Row {
            name: name.into(),
            cat: cat.into(),
            size_cm: size.into(),
            parts,
            tok,
            fid: "authored".into(),
            src: "own".into(),
            tags: tags.split_whitespace().map(String::from).collect(),
            sockets: Vec::new(),
            note: String::new(),
            desc: desc.into(),
            tier: if name.starts_with("WORKSHOP/") { Tier::Workshop } else { Tier::Embedded },
        }
    }

    fn library() -> Vec<Row> {
        let mut sit = row("LIB/core/stool", "furniture", "40x45x40", 4, 90, "wood seat tavern", "A three-legged stool.");
        sit.sockets = vec!["sit".into()];
        vec![
            row("LIB/core/barrel", "container", "56x90x56", 5, 180, "wood dungeon town", "An oak barrel with iron hoops."),
            row("LIB/core/crate_stack", "container", "120x140x110", 24, 520, "wood crate stack", "Three crates stacked."),
            row("BUILTIN/crate", "container", "100x100x100", 5, 140, "wood box storage", "A wooden crate with braces."),
            row("LIB/core/wall_torch", "lighting", "12x60x30", 6, 300, "light torch wall dungeon", "An iron bracket and torch."),
            row("BUILTIN/lantern", "lighting", "40x180x40", 6, 210, "lamp light metal", "A standing metal lantern."),
            row("WORKSHOP/oak_chest", "", "80x50x50", 9, 260, "", "A chest of oak planks, a crate's cousin."),
            sit,
        ]
    }

    fn names(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    fn find(words: &str) -> Query {
        Query { words: words.split_whitespace().map(String::from).collect(), ..Default::default() }
    }

    #[test]
    fn find_ranks_leaf_tags_name_then_description_and_needs_every_word() {
        // "crate": the leaf (5) beats a description (1); equal scores go to fewer tokens.
        assert_eq!(names(&rank(library(), &find("crate"))), ["BUILTIN/crate", "LIB/core/crate_stack", "WORKSHOP/oak_chest"]);
        // Every word must match: wood (a tag) and dungeon (a tag).
        assert_eq!(names(&rank(library(), &find("wood dungeon"))), ["LIB/core/barrel"]);
        // A category or tag prefix matches; "light" reaches lighting and the light tags.
        assert_eq!(names(&rank(library(), &find("light"))), ["BUILTIN/lantern", "LIB/core/wall_torch"]);
        assert_eq!(names(&rank(library(), &find("core torch"))), ["LIB/core/wall_torch"], "name words count");
        assert!(rank(library(), &find("dragon")).is_empty());
        // No words: everything by name.
        assert_eq!(rank(library(), &Query::default()).len(), 7);
        assert_eq!(rank(library(), &Query::default())[0].name, "BUILTIN/crate");
    }

    #[test]
    fn filters_narrow_by_category_tags_sockets_and_tier() {
        let query = |category: Option<&str>, tags: &str, tier: Option<Tier>| Query {
            category: category.map(String::from),
            tags: tags.split_whitespace().map(String::from).collect(),
            tier,
            ..Default::default()
        };
        assert_eq!(
            names(&rank(library(), &query(Some("container"), "", None))),
            ["BUILTIN/crate", "LIB/core/barrel", "LIB/core/crate_stack"]
        );
        assert_eq!(names(&rank(library(), &query(None, "wood dungeon", None))), ["LIB/core/barrel"]);
        assert_eq!(names(&rank(library(), &query(None, "@sit", None))), ["LIB/core/stool"]);
        assert_eq!(names(&rank(library(), &query(None, "", Some(Tier::Workshop)))), ["WORKSHOP/oak_chest"]);
        let lighting = Query { category: Some("lighting".into()), ..find("metal") };
        assert_eq!(names(&rank(library(), &lighting)), ["BUILTIN/lantern"]);
    }

    #[test]
    fn similar_ranks_by_category_tags_size_and_parts() {
        let barrel = library().into_iter().find(|row| row.name == "LIB/core/barrel").unwrap();
        let ranked = rank(library(), &Query { similar: Some(barrel.clone()), ..Default::default() });
        // Not itself; the containers first, the nearest in size and parts on top.
        // The torch shares a tag and is near in size and parts; the stool only shares a tag.
        assert_eq!(names(&ranked)[..4], ["BUILTIN/crate", "LIB/core/crate_stack", "LIB/core/wall_torch", "LIB/core/stool"]);
        assert!(!names(&ranked).contains(&"LIB/core/barrel"));
        assert!(similarity(&barrel, &ranked[0]) > similarity(&barrel, &ranked[2]));
        assert_eq!(similarity(&barrel, &ranked[0]), similarity(&ranked[0], &barrel), "symmetric");
        // With words, likeness still ranks the rows that match them.
        let wood = Query { similar: Some(barrel), ..find("wood") };
        assert_eq!(names(&rank(library(), &wood))[0], "BUILTIN/crate");
    }

    #[test]
    fn packs_on_disk_are_indexed_and_found_by_tier() {
        let root = std::env::temp_dir().join(format!("pav-asset-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (tier, pack) in [("lib", "searchpack"), ("local", "searchlocal")] {
            let folder = root.join(tier).join(pack);
            std::fs::create_dir_all(&folder).unwrap();
            let text = r##"{"format":2,"name":"stake","unit":"cm","category":"architecture","description":"A sharpened stake.","tags":["wood","fence"],"parts":{"stake":{"type":"cylinder","radius":5,"height":90,"pos":[0,45,0],"color":"#806040"}},"sockets":{"top":{"pos":[0,90,0]}}}"##;
            std::fs::write(folder.join("stake.json"), text).unwrap();
            assert_eq!(reindex(&root, pack).unwrap()["rows"], 1);
        }
        // A pack without an index is reported, not read.
        std::fs::create_dir_all(root.join("lib").join("unindexed")).unwrap();
        let (rows, errors) = index::disk(&root);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(names(&rows), ["LIB/searchlocal/stake", "LIB/searchpack/stake"]);
        assert_eq!(
            (rows[1].cat.as_str(), rows[1].tags.join(" "), rows[1].sockets.join(" "), rows[1].tier),
            ("architecture", "wood fence".into(), "top".into(), Tier::Disk)
        );
        let local = Query { tier: Some(Tier::Local), ..find("fence") };
        assert_eq!(names(&rank(rows.clone(), &local)), ["LIB/searchlocal/stake"]);
        assert_eq!(listing_row(&rows[1])[5], "wood fence @top");
        assert!(reindex(&root, "core").is_err(), "no lib/core under this root");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rows_stay_within_their_budget_and_cut_descriptions_at_words() {
        let long = row(
            "LIB/core/market_stall_with_awning",
            "town",
            "320x290x210",
            48,
            1900,
            "market stall awning wood cloth town fair trade",
            "A wooden market stall with a striped cloth awning, crates of fruit and a hanging sign.",
        );
        let quoted =
            row("BUILTIN/sign", "signage", "60x120x8", 3, 120, "", r#"A sign that reads "\"\"\"\"\"\"\"\"\"\"\"\"\"\"" in red."#);
        for row in library().iter().chain([&long, &quoted]) {
            let value = listing_row(row);
            assert!(value.to_string().len() <= ROW_BYTES, "{value}");
        }
        let value = listing_row(&long);
        let desc = value[6].as_str().unwrap();
        assert!(desc.is_empty() || desc.ends_with('…'), "{value}");
        assert_eq!(cut("An oak barrel with iron hoops.", 16), "An oak barrel…");
        assert_eq!(cut("short", 16), "short");
    }
}
