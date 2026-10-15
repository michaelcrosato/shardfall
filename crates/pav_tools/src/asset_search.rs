//! `assets`: find library and workshop assets (docs/ASSET_SYSTEM_PLAN.md §9.2, the store's
//! discovery in docs/AGENT_ASSET_STORE.md §8). Queries read index rows (the embedded ones, then
//! each pack's `index.tsv` on disk); workshop rows are made live from the library. Words match
//! with their singulars and synonyms (`store/synonyms.tsv`); a query that matches nothing gets
//! `none`, the nearest rows and a route, and is logged in `store/misses.tsv`. `shelf=` lists a
//! kind's best entries. A listing row stays within about 45 tokens.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use pav_core::issue::{self, Code, Issue};
use pav_core::props::doc::Category;
use pav_core::props::index::{self, Row};
use pav_core::props::{self, Tier};
use pav_core::store::{self as rules, Term};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_str};

/// The arguments `assets` takes (`kind` is routed before it arrives).
const ARGS: &[&str] = &["kind", "find", "category", "tags", "tier", "similar", "limit", "page", "reindex", "shelf"];

/// A listing row's JSON is cut to this many bytes (its description first): at most about 45
/// o200k tokens, as o200k reads these rows at 3.0-3.5 bytes per token.
pub const ROW_BYTES: usize = 136;
/// Tags a row shows (sockets come after them); the index keeps them all for `find`.
const ROW_TAGS: usize = 4;

pub const COLS: &str = "name cat size_cm tier mesh tok tags desc";

/// `tier=` values: the store tiers (standard, checked, disk) and where a row lives.
const TIERS: &[&str] = &["standard", "checked", "disk", "embedded", "local", "workshop"];

/// The kinds `shelf=` lists.
pub const SHELVES: &[&str] = &["prop", "clip"];

/// What a query asks for. Rows matching every term come first; `similar` ranks by likeness.
#[derive(Default)]
pub(crate) struct Query {
    pub terms: Vec<Term>,
    pub category: Option<String>,
    /// Tags that must all be present; `@sit` names a socket.
    pub tags: Vec<String>,
    /// A store tier or where the row lives (`TIERS`); None leaves out the local tier.
    pub tier: Option<String>,
    pub similar: Option<Row>,
}

/// A ranked row: how many of the query's terms it matched, and its score.
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub row: Row,
    pub matched: usize,
    pub score: f32,
}

/// Whether `word` starts any word of `text`.
fn starts_a_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w.starts_with(word))
}

/// Where `word` matches a row: the whole leaf 5.5 (`table` before `round_table`), in the leaf 5,
/// a tag, socket or the category 3, the name 2, the description 1; 0 nowhere.
fn field_score(row: &Row, leaf: &str, name: &str, desc: &str, word: &str) -> f32 {
    if leaf == word {
        5.5
    } else if leaf.contains(word) {
        5.0
    } else if row.cat.starts_with(word)
        || row.tags.iter().any(|tag| tag.starts_with(word))
        || row.sockets.iter().any(|socket| socket == word)
    {
        3.0
    } else if starts_a_word(name, word) {
        2.0
    } else if starts_a_word(desc, word) {
        1.0
    } else {
        0.0
    }
}

/// A term's score on a row: its best form, or a synonym at 0.8 of its place. 0: no match.
pub(crate) fn term_score(row: &Row, term: &Term) -> f32 {
    let (leaf, name, desc) = (props::leaf(&row.name), row.name.to_ascii_lowercase(), row.desc.to_ascii_lowercase());
    let exact = term.forms.iter().map(|form| field_score(row, leaf, &name, &desc, form)).fold(0.0, f32::max);
    let near = term.synonyms.iter().map(|word| 0.8 * field_score(row, leaf, &name, &desc, word)).fold(0.0, f32::max);
    exact.max(near)
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

/// Whether a row is in `tier=`: a store tier (`row.level`) or where it lives.
fn in_tier(row: &Row, tier: Option<&str>) -> bool {
    match tier {
        None => row.tier != Tier::Local,
        Some(tier) => row.tier.name() == tier || row.level == tier,
    }
}

/// The rows that pass the query's filters and match at least one term (every row without
/// terms), best first: by terms matched, then likeness with `similar` (which needs every term)
/// or the find score; ties go to fewer tokens, then the name. Without terms or `similar`, by name.
pub(crate) fn rank(rows: Vec<Row>, query: &Query) -> Vec<Hit> {
    let n = query.terms.len();
    let mut hits: Vec<Hit> = rows
        .into_iter()
        .filter(|row| query.category.as_ref().is_none_or(|cat| row.cat == *cat))
        .filter(|row| in_tier(row, query.tier.as_deref()))
        .filter(|row| {
            query.tags.iter().all(|tag| match tag.strip_prefix('@') {
                Some(socket) => row.sockets.iter().any(|s| s == socket),
                None => row.tags.contains(tag),
            })
        })
        .filter(|row| query.similar.as_ref().is_none_or(|like| like.name != row.name))
        .filter_map(|row| {
            let scores: Vec<f32> = query.terms.iter().map(|term| term_score(&row, term)).collect();
            let matched = scores.iter().filter(|score| **score > 0.0).count();
            if n > 0 && (matched == 0 || (query.similar.is_some() && matched < n)) {
                return None;
            }
            let found: f32 = scores.iter().sum();
            let score = match &query.similar {
                // The find score only breaks ties of likeness.
                Some(like) => similarity(like, &row) + found / 1000.0,
                None => found,
            };
            Some(Hit { row, matched, score })
        })
        .collect();
    let ranked = n > 0 || query.similar.is_some();
    hits.sort_by(|a, b| {
        if !ranked {
            return a.row.name.cmp(&b.row.name);
        }
        b.matched
            .cmp(&a.matched)
            .then(b.score.total_cmp(&a.score))
            .then(level_rank(&a.row).cmp(&level_rank(&b.row)))
            .then(a.row.tok.cmp(&b.row.tok))
            .then_with(|| a.row.name.cmp(&b.row.name))
    });
    hits
}

/// Store tiers best first: standard, checked, then the rest.
fn level_rank(row: &Row) -> usize {
    match row.level.as_str() {
        "standard" => 0,
        "checked" => 1,
        _ => 2,
    }
}

/// The mesh share of a row in percent: `mesh40` is 40, anything else 0.
fn mesh(row: &Row) -> u32 {
    row.fid.strip_prefix("mesh").and_then(|pct| pct.parse().ok()).unwrap_or(0)
}

/// `text` cut at a word to at most `max` bytes, with … when cut.
pub(crate) fn cut(text: &str, max: usize) -> String {
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
    let fixed = |tags: &[String]| json!([row.name, row.cat, row.size_cm, row.level, mesh(row), row.tok, tags.join(" "), ""]);
    while fixed(&tags).to_string().len() > ROW_BYTES - 16 && !tags.is_empty() {
        tags.pop();
    }
    let mut value = fixed(&tags);
    let mut room = ROW_BYTES.saturating_sub(value.to_string().len());
    // A few words would say nothing; leave them out. Escaped quotes take more room.
    while room >= 12 {
        value[7] = json!(cut(&row.desc, room));
        let over = value.to_string().len().saturating_sub(ROW_BYTES);
        if over == 0 {
            return value;
        }
        if over >= room {
            break;
        }
        room -= over;
    }
    value[7] = json!("");
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

/// The index rows of `folder` (`set` names them), each with its store tier: a row of a
/// committed pack on disk is `checked` when it has no `check` findings, passes the library gate
/// (description, 2-6 tags, token cap, licence) and is at most 30% mesh (§4); else `disk`.
pub fn build_index(root: &Path, folder: &Path, set: &str, old: &[Row]) -> Result<Vec<Row>> {
    let mut rows =
        index::build(folder, set, old, &|name| props::resolve_in(root, name).ok().flatten()).map_err(|e| anyhow!(e))?;
    let committed = folder.parent().and_then(Path::file_name).is_some_and(|tier| tier == "lib");
    let local = folder.parent().and_then(Path::file_name).is_some_and(|tier| tier == "local");
    if !local {
        // A store row must carry an admitted licence (§14 Q2); local packs are not store rows.
        for row in &rows {
            if let Err(error) = rules::admitted(&row.src, "prop").and(rules::admitted(&row.licence, "prop")) {
                bail!("{}: refused: {error}", row.name);
            }
        }
    }
    if committed && rows.iter().any(|row| row.tier == Tier::Disk) {
        let pack = crate::asset_library::read_pack(folder)?;
        for row in rows.iter_mut().filter(|row| row.tier == Tier::Disk) {
            let def =
                props::resolve_in(root, &row.name).map_err(|e| anyhow!(e))?.ok_or_else(|| anyhow!("no asset {}", row.name))?;
            if mesh(row) <= 30 && crate::asset_library::checked(&def, &pack) {
                row.level = "checked".into();
            }
        }
    }
    Ok(rows)
}

/// `templates` or a pack: write the index.tsv of its folder under `root`, and for a committed
/// pack the credits (`CREDITS.md`) too. `store` writes the store's generated files; `hall` the
/// Library Hall room (`crate::hall`).
pub(crate) fn reindex(root: &Path, target: &str) -> Result<Value> {
    if target == "store" {
        return crate::asset_library::write_store(root);
    }
    if target == "hall" {
        return crate::hall::write();
    }
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
    let rows = build_index(root, &folder, &set, &old)?;
    std::fs::write(&path, index::text(&rows)).map_err(|e| anyhow!("write {}: {e}", path.display()))?;
    if folder.parent().and_then(Path::file_name).is_some_and(|tier| tier == "lib") {
        crate::asset_library::write_credits(root)?;
    }
    Ok(json!({"reindexed": path, "rows": rows.len()}))
}

/// The asset benchmark's control arm (`PAV_FIND=builtin`, scripts/asset_bench): `assets find=`
/// sees only the templates and the workshop, without the store's partial and `none` replies,
/// `clips find=` finds nothing, and there is no shelf. Off in every normal build.
pub fn builtin_only() -> bool {
    std::env::var("PAV_FIND").is_ok_and(|value| value == "builtin")
}

/// Where the store's files are: `$PAV_STORE`, else `./store`.
pub fn store_root() -> PathBuf {
    std::env::var_os("PAV_STORE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("store"))
}

/// Appends a query that found nothing to `misses.tsv` under `store`, when that file exists:
/// the date, the kind, the query and the nearest rows offered.
pub fn log_miss(store: &Path, kind: &str, query: &str, nearest: &[String]) {
    use std::io::Write;
    let path = store.join("misses.tsv");
    if cfg!(target_arch = "wasm32") || !path.is_file() {
        return;
    }
    let clean = |text: &str| text.split(['\t', '\n', '\r']).map(str::trim).collect::<Vec<_>>().join(" ");
    let line = format!("{}\t{kind}\t{}\t{}\n", crate::import::libraries::today(), clean(query), nearest.join(" "));
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// A name for something a query asks for and nothing matches: its words, joined by `_`.
pub(crate) fn slug(terms: &[Term]) -> String {
    let words: Vec<String> = terms
        .iter()
        .take(3)
        .map(|term| term.forms[0].chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() { "new_asset".into() } else { words.join("_") }
}

/// What to do when nothing matches: characters and motion have their own tools; anything else
/// is made, from a copy of the nearest row when it shares a word or the hinted category.
fn route(terms: &[Term], nearest: &[Row], related: bool) -> String {
    let words: Vec<&str> = terms.iter().map(|term| term.word.as_str()).collect();
    match terms.iter().find_map(|term| term.hint) {
        Some("character") => "characters are not props: playable looks are game/heroes.toml (hero character=, turntable character=), creatures come from genome and turntable def=; their motion: assets find=<move> kind=clip".into(),
        Some("clip") => format!("motion is a clip: assets find=\"{}\" kind=clip", words.join(" ")),
        _ => {
            let name = slug(terms);
            match nearest.first().filter(|_| related) {
                Some(row) => format!(
                    "make it: asset_edit action=copy from={} name={name} and reshape it, or action=create name={name}; asset_edit action=schema is the field guide",
                    row.name
                ),
                None => format!("make it: asset_edit action=create name={name}; asset_edit action=schema is the field guide"),
            }
        }
    }
}

/// Up to 3 rows nearest a query that matched nothing whole: rows matching some of its words,
/// then rows of the category its words hint at, then the leaves closest in spelling. Also
/// whether the first is related (it shares a word or the hinted category).
fn nearest(hits: &[Hit], rows: &[Row], terms: &[Term]) -> (Vec<Row>, bool) {
    let mut out: Vec<Row> = hits.iter().take(3).map(|hit| hit.row.clone()).collect();
    let usable = |row: &&Row| row.tier != Tier::Local;
    let hint = terms.iter().find_map(|term| term.hint);
    if matches!(hint, Some("character" | "clip")) {
        // Not props: the route names their tools, and no prop is near.
        return (out, false);
    }
    if let Some(hint) = hint {
        let mut same: Vec<&Row> = rows.iter().filter(usable).filter(|row| row.cat == hint).collect();
        same.sort_by(|a, b| a.tok.cmp(&b.tok).then_with(|| a.name.cmp(&b.name)));
        for row in same {
            if out.len() < 3 && !out.iter().any(|o| o.name == row.name) {
                out.push(row.clone());
            }
        }
    }
    if out.len() < 3 {
        let distance = |row: &Row| {
            let leaf = props::leaf(&row.name);
            terms.iter().map(|term| issue::edit_distance(&term.forms[0], leaf)).min().unwrap_or(usize::MAX)
        };
        let mut close: Vec<&Row> = rows.iter().filter(usable).collect();
        close.sort_by(|a, b| distance(a).cmp(&distance(b)).then_with(|| a.name.cmp(&b.name)));
        for row in close {
            if out.len() < 3 && !out.iter().any(|o| o.name == row.name) {
                out.push(row.clone());
            }
        }
    }
    let related = !out.is_empty() && (!hits.is_empty() || hint.is_some_and(|hint| out[0].cat == hint));
    (out, related)
}

/// `shelf=prop`: the foundations first (`store/foundations.tsv`), by archetype, each `name
/// size_cm`, its sockets, knobs (`name=lo-hi` or `name=a/b`) and `:presets`; then the other
/// standard and checked rows with an admitted licence, by category, each `name size_cm` and its
/// sockets; within about 1,500 tokens.
pub fn prop_shelf(rows: &[Row]) -> Value {
    use pav_core::props::doc::Param;
    use std::collections::BTreeMap;
    let sockets = |row: &Row| -> String { row.sockets.iter().map(|s| format!(" @{s}")).collect() };
    let number = |x: f32| if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{x}") };
    let mut foundations: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for foundation in rules::foundations() {
        let mut line = foundation.id.to_owned();
        if let Some(row) = rows.iter().find(|row| row.name == foundation.id) {
            line.push_str(&format!(" {}{}", row.size_cm, sockets(row)));
        }
        if let Some(def) = props::get(foundation.id) {
            for (name, param) in &def.doc.params {
                let values = match param {
                    Param::Range { range: [lo, hi], .. } => format!("{}-{}", number(*lo), number(*hi)),
                    Param::Choices { choices, .. } => choices.join("/"),
                };
                line.push_str(&format!(" {name}={values}"));
            }
            line.extend(def.doc.presets.keys().map(|preset| format!(" :{preset}")));
        }
        foundations.entry(foundation.archetype).or_default().push(line);
    }
    let mut by_cat: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut entries: Vec<&Row> = rows
        .iter()
        .filter(|row| matches!(row.level.as_str(), "standard" | "checked"))
        .filter(|row| rules::admitted(&row.src, "prop").is_ok())
        .filter(|row| rules::foundation(&row.name).is_none())
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for row in entries {
        let cat = if row.cat.is_empty() { "other".to_owned() } else { row.cat.clone() };
        by_cat.entry(cat).or_default().push(format!("{} {}{}", row.name, row.size_cm, sockets(row)));
    }
    json!({
        "foundations": foundations,
        "others": by_cat,
        "shelf": "prop",
        "use": "foundations: where each archetype starts, with knobs and :presets; place a variant with asset_spawn name=X:preset params={...} (rooms: asset = \"X:preset\"); techniques and examples in store/foundations.tsv. standard = embedded (rooms and game data may name it), checked = on disk. asset_edit action=inspect name=X reads one; action=copy from=X name=y forks it; assets find=words searches everything",
    })
}

fn check_text(a: &Args, keys: &[&str]) -> Result<()> {
    for key in keys {
        if a.get(*key).is_some_and(|value| !value.is_string()) {
            bail!("{key} must be text");
        }
    }
    Ok(())
}

/// Search the library and the workshop. Rows are compact; `asset_edit inspect` returns one
/// definition.
pub fn t_assets(_: &mut Session, a: &Args) -> Result<Output> {
    let builtin = builtin_only();
    let args: Vec<&str> = ARGS.iter().copied().filter(|key| !builtin || *key != "shelf").collect();
    for key in a.keys() {
        if ["scene", "seed", "ticks"].contains(&key.as_str()) {
            continue;
        }
        if !args.contains(&key.as_str()) {
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, "not an assets argument")
                    .expected(args.join(", "))
                    .maybe_fix(issue::suggestion(key, args.iter().copied()))
            );
        }
    }
    check_text(a, &["find", "category", "tags", "tier", "similar", "reindex", "shelf"])?;
    let root = props::disk_root();
    if let Some(target) = get_str(a, "reindex") {
        if a.len() > 1 {
            bail!("reindex takes no other arguments");
        }
        return Ok(Output::Json(reindex(&root, target)?));
    }
    if let Some(kind) = get_str(a, "shelf") {
        return match kind {
            "prop" => Ok(Output::Json(prop_shelf(&all_rows(&root).0))),
            "clip" => Ok(Output::Json(crate::anim_tools::clip_shelf())),
            other => bail!(
                "{}",
                Issue::error("shelf", Code::UnknownValue, format!("no shelf \"{other}\""))
                    .expected(SHELVES.join(", "))
                    .maybe_fix(issue::suggestion(other, SHELVES.iter().copied()))
            ),
        };
    }
    let limit = number(a, "limit", 20, 1..=100)?;
    let page = number(a, "page", 1, 1..=usize::MAX)?;
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
    let tier = match get_str(a, "tier").map(str::to_ascii_lowercase) {
        None => None,
        Some(text) if TIERS.contains(&text.as_str()) => Some(text),
        Some(text) => bail!(
            "{}",
            Issue::error("tier", Code::UnknownValue, format!("unknown tier \"{text}\""))
                .expected(TIERS.join(", "))
                .maybe_fix(issue::suggestion(&text, TIERS.iter().copied()))
        ),
    };
    let (mut rows, errors) = all_rows(&root);
    if builtin {
        rows.retain(|row| row.name.starts_with("BUILTIN/") || matches!(row.tier, Tier::Workshop | Tier::Local));
    }
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
    let tags: Vec<String> = get_str(a, "tags")
        .unwrap_or("")
        .to_ascii_lowercase()
        .split([' ', ','])
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let find = get_str(a, "find").unwrap_or("");
    let query = Query { terms: rules::terms(find), category, tags, tier, similar };
    let n = query.terms.len();
    let hits = rank(rows.clone(), &query);
    let whole: Vec<&Hit> = hits.iter().filter(|hit| hit.matched == n).collect();
    let partial = whole.is_empty() && n >= 3 && hits.first().is_some_and(|hit| hit.matched >= 2);
    let mut result = if !whole.is_empty() || n == 0 || query.similar.is_some() || builtin {
        page_of(&whole, page, limit)
    } else if partial {
        let shown: Vec<&Hit> = hits.iter().filter(|hit| hit.matched >= 2).collect();
        let missing: Vec<&str> =
            query.terms.iter().filter(|term| term_score(&shown[0].row, term) == 0.0).map(|term| term.word.as_str()).collect();
        let mut result = page_of(&shown, page, limit);
        result["partial"] =
            json!(format!("no asset matches every word; these miss some (the first misses: {})", missing.join(" ")));
        result
    } else {
        let (near, related) = nearest(&hits, &rows, &query.terms);
        let names: Vec<String> = near.iter().map(|row| row.name.clone()).collect();
        log_miss(&store_root(), "prop", find, &names);
        json!({
            "cols": COLS,
            "none": format!("nothing matches \"{find}\""),
            "nearest": near.iter().map(listing_row).collect::<Vec<_>>(),
            "route": route(&query.terms, &near, related),
        })
    };
    if !errors.is_empty() {
        result["errors"] = json!(errors);
    }
    Ok(Output::Json(result))
}

/// One page of ranked rows, with `matched` and `next`.
fn page_of(hits: &[&Hit], page: usize, limit: usize) -> Value {
    let skip = (page - 1).saturating_mul(limit);
    let rows: Vec<Value> = hits.iter().skip(skip).take(limit).map(|hit| listing_row(&hit.row)).collect();
    let mut result = json!({"cols": COLS, "rows": rows, "matched": hits.len()});
    if skip.saturating_add(limit) < hits.len() {
        result["next"] = json!(page + 1);
    }
    result
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
            licence: "CC0-1.0".into(),
            method: "authored".into(),
            level: if name.starts_with("WORKSHOP/") { "workshop".into() } else { "standard".into() },
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

    /// The rows of hits that match every term, in order.
    fn whole(hits: Vec<Hit>, query: &Query) -> Vec<Row> {
        hits.into_iter().filter(|hit| hit.matched == query.terms.len()).map(|hit| hit.row).collect()
    }

    fn find(words: &str) -> Query {
        Query { terms: rules::terms(words), ..Default::default() }
    }

    fn ranked(rows: Vec<Row>, query: &Query) -> Vec<Row> {
        whole(rank(rows, query), query)
    }

    #[test]
    fn find_ranks_leaf_tags_name_then_description_and_needs_every_word() {
        // "crate": the leaf (5) beats a description (1); equal scores go to fewer tokens.
        assert_eq!(names(&ranked(library(), &find("crate"))), ["BUILTIN/crate", "LIB/core/crate_stack", "WORKSHOP/oak_chest"]);
        // Every word must match: wood (a tag) and dungeon (a tag).
        assert_eq!(names(&ranked(library(), &find("wood dungeon"))), ["LIB/core/barrel"]);
        // A category or tag prefix matches; "light" reaches lighting and the light tags.
        assert_eq!(names(&ranked(library(), &find("light"))), ["BUILTIN/lantern", "LIB/core/wall_torch"]);
        assert_eq!(names(&ranked(library(), &find("core torch"))), ["LIB/core/wall_torch"], "name words count");
        assert!(rank(library(), &find("dragon")).is_empty());
        // No words: everything by name.
        assert_eq!(ranked(library(), &Query::default()).len(), 7);
        assert_eq!(ranked(library(), &Query::default())[0].name, "BUILTIN/crate");
    }

    #[test]
    fn filters_narrow_by_category_tags_sockets_and_tier() {
        let query = |category: Option<&str>, tags: &str, tier: Option<&str>| Query {
            category: category.map(String::from),
            tags: tags.split_whitespace().map(String::from).collect(),
            tier: tier.map(String::from),
            ..Default::default()
        };
        assert_eq!(
            names(&ranked(library(), &query(Some("container"), "", None))),
            ["BUILTIN/crate", "LIB/core/barrel", "LIB/core/crate_stack"]
        );
        assert_eq!(names(&ranked(library(), &query(None, "wood dungeon", None))), ["LIB/core/barrel"]);
        assert_eq!(names(&ranked(library(), &query(None, "@sit", None))), ["LIB/core/stool"]);
        assert_eq!(names(&ranked(library(), &query(None, "", Some("workshop")))), ["WORKSHOP/oak_chest"]);
        let lighting = Query { category: Some("lighting".into()), ..find("metal") };
        // An exact tag first, then a synonym in the description (iron).
        assert_eq!(names(&ranked(library(), &lighting)), ["BUILTIN/lantern", "LIB/core/wall_torch"]);
    }

    #[test]
    fn similar_ranks_by_category_tags_size_and_parts() {
        let barrel = library().into_iter().find(|row| row.name == "LIB/core/barrel").unwrap();
        let likes = ranked(library(), &Query { similar: Some(barrel.clone()), ..Default::default() });
        // Not itself; the containers first, the nearest in size and parts on top.
        // The torch shares a tag and is near in size and parts; the stool only shares a tag.
        assert_eq!(names(&likes)[..4], ["BUILTIN/crate", "LIB/core/crate_stack", "LIB/core/wall_torch", "LIB/core/stool"]);
        assert!(!names(&likes).contains(&"LIB/core/barrel"));
        assert!(similarity(&barrel, &likes[0]) > similarity(&barrel, &likes[2]));
        assert_eq!(similarity(&barrel, &likes[0]), similarity(&likes[0], &barrel), "symmetric");
        // With words, likeness still ranks the rows that match them.
        let wood = Query { similar: Some(barrel), ..find("wood") };
        assert_eq!(names(&ranked(library(), &wood))[0], "BUILTIN/crate");
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
            let licence = r#"{"pack":"PACK","title":"Stakes","license":"CC0-1.0","credit":"Test","source":"own"}"#;
            std::fs::write(folder.join("PACK.json"), licence.replace("PACK", pack)).unwrap();
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
        let local = Query { tier: Some("local".into()), ..find("fence") };
        assert_eq!(names(&ranked(rows.clone(), &local)), ["LIB/searchlocal/stake"]);
        assert_eq!(listing_row(&rows[1])[6], "wood fence @top");
        assert_eq!(rows[1].level, "checked", "a lint-clean, described, tagged editable row");
        assert_eq!((rows[1].licence.as_str(), rows[1].method.as_str()), ("CC0-1.0", "authored"), "own work");
        let text = std::fs::read_to_string(root.join("lib/searchpack/index.tsv")).unwrap();
        assert!(text.contains("\town\tprop\tCC0-1.0\tauthored\tchecked\t"), "{text}");
        assert!(reindex(&root, "core").is_err(), "no lib/core under this root");
        let credits = std::fs::read_to_string(root.join("CREDITS.md")).unwrap();
        assert!(credits.contains("## LIB/searchpack: Stakes") && !credits.contains("searchlocal"), "{credits}");
        // An explicit root's files are read afresh, not from the shared library.
        let stake = || props::resolve_in(&root, "LIB/searchpack/stake").unwrap().unwrap().doc.description.clone();
        assert_eq!(stake(), "A sharpened stake.");
        let file = root.join("lib/searchpack/stake.json");
        std::fs::write(&file, std::fs::read_to_string(&file).unwrap().replace("sharpened", "charred")).unwrap();
        assert_eq!(stake(), "A charred stake.");
        assert!(props::get("LIB/searchpack/stake").is_none(), "the shared library caches disk_root() only");
        // A local pack named like a lib pack is reported, not listed under the lib pack's names.
        std::fs::create_dir_all(root.join("local/searchpack")).unwrap();
        let (rows, errors) = index::disk(&root);
        assert!(errors.iter().any(|e| e.contains("hidden by lib/searchpack")), "{errors:?}");
        assert_eq!(names(&rows), ["LIB/searchlocal/stake", "LIB/searchpack/stake"]);
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
        // Control characters escape to six bytes each, far more than the room they took.
        let control = row("BUILTIN/rune", "signage", "60x120x8", 3, 120, "", &"\u{1}".repeat(60));
        for row in library().iter().chain([&long, &quoted, &control]) {
            let value = listing_row(row);
            assert!(value.to_string().len() <= ROW_BYTES, "{value}");
        }
        let value = listing_row(&long);
        let desc = value[7].as_str().unwrap();
        assert!(desc.is_empty() || desc.ends_with('…'), "{value}");
        assert_eq!(cut("An oak barrel with iron hoops.", 16), "An oak barrel…");
        assert_eq!(cut("short", 16), "short");
    }
}
