//! The store's foundations (docs/AGENT_ASSET_STORE.md §4, `store/foundations.tsv`): every listed
//! foundation resolves, is standard (an embedded LIB/core or BUILTIN row), passes `check` with
//! no findings and the library gate, and each example transform derives a variant: a preset or
//! knob values compile to something other than the default, a roles palette names roles the
//! asset uses, and a seed draws a different variant from seed 0.

use pav_core::props::{self, Params, index};
use pav_core::store;
use pav_tools::asset_library;
use serde_json::{Map, Value};

fn object(json: &str, what: &str) -> Map<String, Value> {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("{what}: {json} is not a JSON object: {e}"))
}

#[test]
fn every_foundation_is_standard_clean_and_its_transforms_derive_variants() {
    let rows = index::embedded();
    let core = asset_library::read_pack(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/props/lib/core"))
        .expect("lib/core has its PACK.json");
    for foundation in store::foundations() {
        let id = foundation.id;
        let row = rows.iter().find(|row| row.name == id).unwrap_or_else(|| panic!("{id} is not embedded"));
        assert_eq!(row.level, "standard", "{id}: a foundation is standard");
        let def = props::resolve_ref(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut found = pav_tools::asset_tools::check_lines(&def);
        if id.starts_with("LIB/core/") {
            found.extend(asset_library::gate(&def, &core));
        }
        assert!(found.is_empty(), "{id}: {found:?}");
        let plain = def.variant(0);
        for transform in &foundation.transforms {
            let what = format!("{id} {transform}");
            if let Some(preset) = transform.strip_prefix(':') {
                let placed = format!("{id}:{preset}");
                let (_, def, values) = props::resolve_placed(&placed, &Params::new()).unwrap_or_else(|e| panic!("{e}"));
                assert!(*def.variant_with(&values, 0) != *plain, "{what} draws the default");
            } else if let Some(json) = transform.strip_prefix("params=") {
                let given: Params = object(json, &what).into_iter().collect();
                let (_, def, values) = props::resolve_placed(id, &given).unwrap_or_else(|e| panic!("{what}: {e}"));
                assert!(*def.variant_with(&values, 0) != *plain, "{what} draws the default");
            } else if let Some(json) = transform.strip_prefix("roles=") {
                let text = def.doc.text();
                for (role, color) in object(json, &what) {
                    assert!(role.starts_with('$') && text.contains(&role), "{what}: {id} uses no {role}");
                    assert!(color.as_str().is_some_and(|c| c.len() == 7 && c.starts_with('#')), "{what}: {color}");
                }
            } else if let Some(seed) = transform.strip_prefix("seed=") {
                let seed: u32 = seed.parse().unwrap_or_else(|_| panic!("{what}: not a seed"));
                assert!(seed != 0 && *def.variant(seed) != *plain, "{what} draws what seed 0 draws");
            } else {
                panic!("{what}: unknown transform");
            }
        }
    }
}

/// At least one foundation is a fork of a committed import, published to standard with its
/// lineage (the S2 exit test, §13): it names the import as its parent and carries its licence
/// and credit.
#[test]
fn an_import_fork_is_a_standard_foundation() {
    let forks: Vec<_> = store::foundations()
        .iter()
        .filter_map(|foundation| {
            let def = props::resolve_ref(foundation.id).ok()?;
            let lineage = def.doc.lineage.clone()?;
            lineage
                .parents
                .iter()
                .any(|parent| parent.starts_with("LIB/kenney_mini_dungeon/"))
                .then_some((foundation.id, lineage))
        })
        .collect();
    let (id, lineage) = forks.iter().find(|(id, _)| *id == "LIB/core/cask").expect("the cask forks Kenney's barrel");
    assert_eq!(lineage.parents.len(), 1, "{id}");
    assert!(lineage.parents[0].starts_with("LIB/kenney_mini_dungeon/barrel@prop2-"), "{id}: {:?}", lineage.parents);
    assert_eq!((lineage.license.as_deref(), lineage.credit.as_deref()), (Some("CC0-1.0"), Some("Kenney (www.kenney.nl)")));
    assert_eq!(lineage.origin.name(), "import", "{id}");
}
