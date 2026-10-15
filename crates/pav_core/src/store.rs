//! The agent asset store's shared rules (docs/AGENT_ASSET_STORE.md): the licences it admits
//! (`store/licences.tsv`), the words a search understands (`store/synonyms.tsv`), the
//! foundations each archetype starts from (`store/foundations.tsv`), and the credits text
//! generated from clip set `sources` and object packs' `PACK.json`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

const LICENCES: &str = include_str!("../../../store/licences.tsv");
const SYNONYMS: &str = include_str!("../../../store/synonyms.tsv");
const FOUNDATIONS: &str = include_str!("../../../store/foundations.tsv");

/// One foundation (a row of `store/foundations.tsv`, §4): a standard prop an archetype starts
/// from, with examples of the transforms that derive its variants.
#[derive(Debug)]
pub struct Foundation {
    /// The archetype it founds: `table`, `seat`, `barrel`, `light`, `crate`, `weapon`, `stall`.
    pub archetype: &'static str,
    /// Its asset name (`LIB/core/x` or `BUILTIN/x`).
    pub id: &'static str,
    /// Example transforms: `:preset`, `params={…}`, `roles={…}` or `seed=N`.
    pub transforms: Vec<&'static str>,
    /// The technique it shows, at most 40 tokens.
    pub why: &'static str,
}

/// The foundations, in the file's order (grouped by archetype).
pub fn foundations() -> &'static [Foundation] {
    static ROWS: OnceLock<Vec<Foundation>> = OnceLock::new();
    ROWS.get_or_init(|| {
        FOUNDATIONS
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .skip(1)
            .map(|line| {
                let cells: Vec<&'static str> = line.split('\t').collect();
                let [archetype, id, transforms, why] = cells[..] else { panic!("store/foundations.tsv: {line}") };
                Foundation { archetype, id, transforms: transforms.split_whitespace().collect(), why }
            })
            .collect()
    })
}

/// The foundation an asset name is, if any.
pub fn foundation(id: &str) -> Option<&'static Foundation> {
    foundations().iter().find(|foundation| foundation.id == id)
}

/// The credit line of own work (`assets/props/lib/core/PACK.json`'s, which a test holds equal).
pub const OWN_CREDIT: &str = "Shardfall contributors (own work)";

/// One admitted licence (a row of `store/licences.tsv`).
#[derive(Debug)]
pub struct Licence {
    /// SPDX id, or a `LicenseRef-` for terms SPDX does not list.
    pub id: &'static str,
    /// The kinds it is admitted for (`prop`, `material`, `clip`).
    pub kinds: Vec<&'static str>,
    /// Whether every copy and fork must carry a credit line.
    pub credit: bool,
    /// The terms in one line: what every row with this licence carries.
    pub terms: &'static str,
}

/// The admitted licences.
pub fn licences() -> &'static [Licence] {
    static ROWS: OnceLock<Vec<Licence>> = OnceLock::new();
    ROWS.get_or_init(|| {
        LICENCES
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .skip(1)
            .map(|line| {
                let cells: Vec<&'static str> = line.split('\t').collect();
                let [id, kinds, credit, terms] = cells[..] else { panic!("store/licences.tsv: {line}") };
                Licence { id, kinds: kinds.split_whitespace().collect(), credit: credit == "yes", terms }
            })
            .collect()
    })
}

/// The licence id a licence text names: SPDX ids, the free-text forms the clip sets and index
/// rows use (`CC0 1.0`, `CC BY 4.0 (credit: …)`, `CC0-1.0 kenney/mini_dungeon`), CMU's terms,
/// and `own` (own work is CC0-1.0, §14 Q1). None: unknown, so not admitted.
pub fn spdx(text: &str) -> Option<&'static str> {
    let lower = text.trim().to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    let squashed: String = lower.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    if first == "own" || squashed.starts_with("cc01.0") || first == "cc0" || first == "cc0-1.0" {
        Some("CC0-1.0")
    } else if lower.contains("-nc") || lower.contains(" nc") || lower.contains("-nd") || lower.contains("-sa") {
        None
    } else if squashed.starts_with("ccby4.0") {
        Some("CC-BY-4.0")
    } else if squashed.starts_with("ccby3.0") {
        Some("CC-BY-3.0")
    } else if lower.starts_with("licenseref-cmu") || lower.starts_with("free for all uses") {
        Some("LicenseRef-CMU")
    } else {
        None
    }
}

/// The licence a row of `kind` may carry, or why it is refused.
pub fn admitted(text: &str, kind: &str) -> Result<&'static Licence, String> {
    let id = spdx(text).ok_or_else(|| format!("licence \"{text}\" is not admitted (store/licences.tsv)"))?;
    let licence = licences().iter().find(|licence| licence.id == id).ok_or_else(|| format!("licence {id} is not admitted"))?;
    if !licence.kinds.contains(&kind) {
        return Err(format!("licence {id} is admitted for {} only, not {kind}", licence.kinds.join(", ")));
    }
    Ok(licence)
}

/// Whether a path names one of Shardfall's local tiers, which the store never indexes (§14 Q2).
pub fn local_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.split('/').any(|segment| segment == "local") || path.starts_with("WORKSHOP_LOCAL/")
}

/// Words a query drops: they match everything or nothing.
const STOP: &[&str] = &[
    "a", "an", "the", "of", "with", "and", "or", "for", "in", "on", "to", "at", "by", "some", "any", "that", "this", "its", "my",
    "our", "from", "into", "one", "two", "three", "four", "five", "pair", "set", "few", "several",
];

/// One word of a query, with the forms that match it.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    /// The word as written (lowercase).
    pub word: String,
    /// The word and its singular: exact matches.
    pub forms: Vec<String>,
    /// Words of its synonym groups: matches ranked a little lower.
    pub synonyms: Vec<String>,
    /// A category its `@` hint names (`vegetation`, `character`, `clip`), for a query that found
    /// nothing.
    pub hint: Option<&'static str>,
}

/// The singular of an English plural, or the word.
pub fn singular(word: &str) -> String {
    let n = word.len();
    if n > 4 && word.ends_with("ies") {
        format!("{}y", &word[..n - 3])
    } else if n > 4 && word.ends_with("lves") {
        format!("{}f", &word[..n - 3])
    } else if n > 4 && ["ches", "shes", "xes", "sses", "zes"].iter().any(|end| word.ends_with(end)) {
        word[..n - 2].to_owned()
    } else if n > 3 && word.ends_with('s') && !["ss", "us", "is"].iter().any(|end| word.ends_with(end)) {
        word[..n - 1].to_owned()
    } else {
        word.to_owned()
    }
}

struct Groups {
    groups: Vec<(Option<&'static str>, Vec<&'static str>)>,
}

fn groups() -> &'static Groups {
    static GROUPS: OnceLock<Groups> = OnceLock::new();
    GROUPS.get_or_init(|| Groups {
        groups: SYNONYMS
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .map(|line| {
                let mut words: Vec<&'static str> = line.split_whitespace().collect();
                let hint = words.first().and_then(|first| first.strip_prefix('@'));
                if hint.is_some() {
                    words.remove(0);
                }
                (hint, words)
            })
            .collect(),
    })
}

/// A query's words: lowercase, without punctuation, stop words or numbers, each with its
/// singular and synonyms.
pub fn terms(query: &str) -> Vec<Term> {
    let mut out: Vec<Term> = Vec::new();
    for raw in query.to_ascii_lowercase().split(|c: char| c.is_whitespace() || c == ',') {
        let word = raw.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/'));
        if word.is_empty() || STOP.contains(&word) || word.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if out.iter().any(|term| term.word == word) {
            continue;
        }
        let one = singular(word);
        let mut forms = vec![one.clone()];
        if one != word {
            forms.push(word.to_owned());
        }
        let (mut synonyms, mut hint) = (Vec::new(), None);
        for (group_hint, words) in &groups().groups {
            if words.iter().any(|w| forms.iter().any(|form| form == w)) {
                if group_hint.is_some() {
                    hint = hint.or(*group_hint);
                    continue;
                }
                for w in words {
                    if !forms.iter().any(|form| form == w) && !synonyms.iter().any(|s| s == w) {
                        synonyms.push((*w).to_owned());
                    }
                }
            }
        }
        out.push(Term { word: word.to_owned(), forms, synonyms, hint });
    }
    out
}

/// One source of a credits text: a clip set or an object pack.
#[derive(Clone, Debug)]
pub struct Credit {
    /// `clip` or `prop`.
    pub kind: &'static str,
    /// The set or pack, e.g. `STYLE100` or `LIB/core`; `where_` says where it lives.
    pub what: String,
    pub where_: &'static str,
    /// Its licence text, as the set or pack writes it.
    pub licence: String,
    /// The credit line.
    pub credit: String,
}

/// The credit line a licence text carries (`CC BY 4.0 (credit: …)`), if any.
pub fn licence_credit(licence: &str) -> Option<&str> {
    let start = licence.find("(credit:")? + "(credit:".len();
    let rest = &licence[start..];
    Some(rest[..rest.rfind(')').unwrap_or(rest.len())].trim())
}

/// The credits text: one line per licence and credit, naming every set or pack that carries
/// it. Sources whose licence is not admitted are listed as refused, so nothing goes unnamed.
pub fn credits_text(sources: &[Credit]) -> String {
    let mut text = String::from(
        "# Credits\n\nEvery source the store's entries come from (docs/AGENT_ASSET_STORE.md §9): the motion clip sets\n\
         (their `sources`) and the object packs (their `PACK.json`). Generated by `pav assets reindex=store`;\n\
         do not edit. A CC-BY credit travels with every copy and fork.\n",
    );
    for (kind, title) in [("clip", "Motion clips"), ("prop", "Objects")] {
        let mut groups: BTreeMap<(String, String, &str), Vec<&str>> = BTreeMap::new();
        for source in sources.iter().filter(|source| source.kind == kind) {
            let id = spdx(&source.licence).map(str::to_owned).unwrap_or_else(|| format!("refused: {}", source.licence));
            groups.entry((id, source.credit.clone(), source.where_)).or_default().push(&source.what);
        }
        if groups.is_empty() {
            continue;
        }
        text.push_str(&format!("\n## {title}\n\n"));
        for ((id, credit, where_), mut names) in groups {
            names.sort();
            names.dedup();
            let named = if names.len() <= 4 {
                format!("{} ({where_})", names.join(", "))
            } else {
                format!("{} … {} ({} sets, {where_})", names[0], names[names.len() - 1], names.len())
            };
            let terms = licences().iter().find(|licence| licence.id == id).map(|licence| licence.terms).unwrap_or("");
            text.push_str(&format!("- {named}: {id}. Credit: {}.", credit.trim_end_matches('.')));
            if !terms.is_empty() && id.starts_with("LicenseRef") {
                text.push_str(&format!(" Terms: {terms}"));
            }
            text.push('\n');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licences_map_to_admitted_ids_and_the_rest_are_refused() {
        for (text, id) in [
            ("CC0-1.0", "CC0-1.0"),
            ("CC0 1.0", "CC0-1.0"),
            ("CC0-1.0 kenney/mini_dungeon", "CC0-1.0"),
            ("own", "CC0-1.0"),
            ("CC BY 4.0 (credit: 100STYLE, Mason, Starke and Komura)", "CC-BY-4.0"),
            ("CC-BY-3.0", "CC-BY-3.0"),
            ("free for all uses (may be copied, modified, or redistributed without permission)", "LicenseRef-CMU"),
        ] {
            assert_eq!(spdx(text), Some(id), "{text}");
        }
        for text in
            ["CC-BY-NC-4.0", "CC BY-SA 4.0", "CC-BY-ND-4.0", "GPL-3.0", "Mixamo", "Fab Standard", "UNLICENSED", "", "unknown"]
        {
            assert!(admitted(text, "prop").is_err(), "{text}");
        }
        assert!(admitted("free for all uses", "clip").is_ok());
        assert!(admitted("free for all uses", "prop").is_err(), "CMU's terms admit clips only");
        assert!(admitted("CC-BY-4.0", "material").unwrap().credit);
    }

    #[test]
    fn foundations_are_a_few_per_archetype_with_transforms_and_a_short_why() {
        let rows = foundations();
        let mut per: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            *per.entry(row.archetype).or_default() += 1;
            assert!(row.id.starts_with("LIB/core/") || row.id.starts_with("BUILTIN/"), "{}: standard only", row.id);
            assert_eq!(rows.iter().filter(|other| other.id == row.id).count(), 1, "{} is listed twice", row.id);
            assert!((2..=3).contains(&row.transforms.len()), "{}: 2-3 transforms", row.id);
            for transform in &row.transforms {
                let known = transform.starts_with(':')
                    || ["params={", "roles={", "seed="].iter().any(|prefix| transform.starts_with(prefix));
                assert!(known, "{}: unknown transform {transform}", row.id);
            }
            let tokens = crate::props::index::tokens(row.why.len());
            assert!(!row.why.is_empty() && tokens <= 40, "{}: why is {tokens} tokens", row.id);
        }
        assert_eq!(
            per.keys().copied().collect::<Vec<_>>(),
            [
                "barrel", "bed", "container", "crate", "fire", "gate", "light", "seat", "shelter", "stall", "table", "tool",
                "vegetation", "vehicle", "weapon"
            ]
        );
        // S2's seven archetypes hold 3-5 each; S3's new ones start from the misses that founded them.
        assert!(per.values().all(|n| (1..=5).contains(n)), "at most 5 foundations per archetype: {per:?}");
        assert!(["barrel", "crate", "light", "seat", "stall", "table", "weapon"].iter().all(|a| per[a] >= 3), "{per:?}");
        assert_eq!(foundation("LIB/core/cask").map(|row| row.archetype), Some("barrel"));
        assert!(foundation("LIB/core/urn").is_none());
    }

    #[test]
    fn local_tiers_are_recognised() {
        assert!(local_path("assets/props/local/kenney_castle_kit/wall.json"));
        assert!(local_path("anim/local/mixamo.json"));
        assert!(local_path("WORKSHOP_LOCAL/copy"));
        assert!(!local_path("assets/props/lib/core/chair.json"));
        assert!(!local_path("LIB/kenney_mini_dungeon/table"));
    }

    #[test]
    fn query_words_drop_stop_words_and_gain_singulars_and_synonyms() {
        let terms = terms("Two wooden tables, with the benches");
        let words: Vec<&str> = terms.iter().map(|t| t.word.as_str()).collect();
        assert_eq!(words, ["wooden", "tables", "benches"]);
        assert!(terms[0].synonyms.contains(&"wood".to_owned()));
        assert_eq!(terms[1].forms, ["table", "tables"]);
        assert!(terms[1].synonyms.contains(&"desk".to_owned()));
        assert_eq!(terms[2].forms[0], "bench");
        assert_eq!(singular("shelves"), "shelf");
        assert_eq!(singular("boxes"), "box");
        assert_eq!(singular("glass"), "glass");
        assert_eq!(singular("stairs"), "stair");
        assert_eq!(super::terms("tree")[0].hint, Some("vegetation"));
        assert_eq!(super::terms("characters")[0].hint, Some("character"));
    }

    #[test]
    fn credits_group_sources_by_licence_and_credit() {
        let credit = |kind, what: &str, where_, licence: &str, credit: &str| Credit {
            kind,
            what: what.into(),
            where_,
            licence: licence.into(),
            credit: credit.into(),
        };
        let text = credits_text(&[
            credit("clip", "STYLE100", "embedded", "CC BY 4.0 (credit: 100STYLE)", "100STYLE, Mason, Starke and Komura"),
            credit("clip", "CMU", "embedded", "free for all uses", "CMU Graphics Lab"),
            credit("prop", "LIB/core", "embedded", "CC0-1.0", "Shardfall contributors (own work)"),
            credit("prop", "LIB/odd", "on disk", "CC-BY-NC-4.0", "Someone"),
        ]);
        assert!(text.contains("- STYLE100 (embedded): CC-BY-4.0. Credit: 100STYLE, Mason, Starke and Komura.\n"), "{text}");
        assert!(text.contains("may not be resold"), "CMU rows carry the no-resale term: {text}");
        assert!(text.contains("- LIB/odd (on disk): refused: CC-BY-NC-4.0."), "{text}");
    }
}
