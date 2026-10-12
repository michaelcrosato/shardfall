//! Snapshot dedupe (asset plan §5.8): a snapshot carries each definition its placed copies use
//! once, in `SimState::prop_defs`, and each `PropInstance` names its definition by key. The
//! table is written before the entities, so a restore reads the definitions first and the
//! instances find theirs while loading. Snapshots from before the table embed each instance's
//! definition inline, and still load.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::{Doc, PropDef, canonical, embedded, fnv};
use crate::issue;

impl PropDef {
    /// The key a snapshot names this definition by: a hash of its document and of the
    /// definitions its references compiled against.
    pub fn snapshot_key(&self) -> u64 {
        fnv(self.key.as_bytes())
    }
}

/// The definitions placed copies use, with the definitions their references compiled against.
#[derive(Clone, Debug, Default)]
pub struct PropDefTable {
    defs: BTreeMap<u64, Arc<PropDef>>,
}

impl PropDefTable {
    /// Keep `def` (and what its references compiled against) for the next snapshot.
    pub fn insert(&mut self, def: &Arc<PropDef>) {
        if self.defs.insert(def.snapshot_key(), def.clone()).is_none() {
            for dep in def.pinned.values() {
                self.insert(dep);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

thread_local! {
    /// The table being restored on this thread: instances resolve their keys through it.
    static LOADING: RefCell<BTreeMap<u64, Weak<PropDef>>> = RefCell::default();
}

impl Serialize for PropDefTable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        /// A definition without references as `embedded` writes it; one with references as
        /// its document and the keys of what they compiled against.
        struct Entry<'a>(&'a Arc<PropDef>);
        impl Serialize for Entry<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let def = self.0;
                if def.pinned.is_empty() {
                    return embedded::serialize(def, serializer);
                }
                let document = serde_json::from_str::<Value>(&def.doc.text()).map_err(serde::ser::Error::custom)?;
                let refs: BTreeMap<&str, u64> =
                    def.pinned.iter().map(|(name, dep)| (name.as_str(), dep.snapshot_key())).collect();
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("document", &document)?;
                map.serialize_entry("refs", &refs)?;
                map.end()
            }
        }
        let mut map = serializer.serialize_map(Some(self.defs.len()))?;
        for (key, def) in &self.defs {
            map.serialize_entry(key, &Entry(def))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PropDefTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: BTreeMap<u64, Value> = BTreeMap::deserialize(deserializer)?;
        let mut built = BTreeMap::new();
        for key in raw.keys() {
            build(*key, &raw, &mut built, 0).map_err(D::Error::custom)?;
        }
        LOADING.with(|loading| *loading.borrow_mut() = built.iter().map(|(key, def)| (*key, Arc::downgrade(def))).collect());
        // The keys this build computes name the definitions from now on.
        Ok(Self { defs: built.into_values().map(|def| (def.snapshot_key(), def)).collect() })
    }
}

/// One stored definition, after the ones its references name.
fn build(
    key: u64,
    raw: &BTreeMap<u64, Value>,
    built: &mut BTreeMap<u64, Arc<PropDef>>,
    depth: u8,
) -> Result<Arc<PropDef>, String> {
    if let Some(def) = built.get(&key) {
        return Ok(def.clone());
    }
    if depth > super::compile::MAX_REF_DEPTH {
        return Err("the snapshot's definitions reference each other too deeply".into());
    }
    let value = raw.get(&key).ok_or_else(|| format!("the snapshot has no definition {key:016x}"))?;
    let def = match value.as_object() {
        Some(object) if object.contains_key("document") => {
            let refs = object.get("refs").and_then(Value::as_object).cloned().unwrap_or_default();
            let mut deps = BTreeMap::new();
            for (name, dep) in refs {
                let dep = dep.as_u64().ok_or_else(|| format!("{name}: expected a definition key"))?;
                deps.insert(name, build(dep, raw, built, depth + 1)?);
            }
            let lookup = |name: &str| deps.get(&canonical(name).ok()?).cloned();
            let doc = Doc::from_value_in(&object["document"], &lookup).map_err(|issues| issue::render(&issues))?;
            Arc::new(PropDef::with_deps(doc, &lookup, None).map_err(|issues| issue::render(&issues))?)
        }
        _ => embedded::read(value.clone())?,
    };
    built.insert(key, def.clone());
    Ok(def)
}

/// `PropInstance::definition` in a snapshot: its key in the table. Inline definitions, from
/// snapshots before the table, still read.
pub mod by_key {
    use super::*;

    pub fn serialize<S: Serializer>(def: &Arc<PropDef>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(def.snapshot_key())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<PropDef>, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Number(number) => {
                let key = number.as_u64().ok_or_else(|| D::Error::custom("a definition key is a whole number"))?;
                LOADING
                    .with(|loading| loading.borrow().get(&key).and_then(Weak::upgrade))
                    .ok_or_else(|| D::Error::custom(format!("the snapshot has no definition {key:016x}")))
            }
            inline => embedded::read(inline).map_err(D::Error::custom),
        }
    }
}
