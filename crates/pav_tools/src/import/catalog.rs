//! Catalogs, written by hand: JSON with `//` comment lines whose order is the output order
//! (`Json` keeps it). The animation catalogs (anim/catalogs) and the asset catalogs
//! (assets/catalogs) share this grammar: `$sources`, `$skip`, `$pick`, `$library` and one line
//! per item, `name: [tags, description, ...]`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

/// JSON with objects in the order the file writes them (serde_json's own maps sort their keys;
/// a catalog's order is the order its moments are cut in).
#[derive(Clone, Debug)]
pub enum Json {
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
    Val(Value),
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_str<E>(self, v: &str) -> Result<Json, E> {
                Ok(Json::Val(v.into()))
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Val(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = s.next_element()? {
                    v.push(x);
                }
                Ok(Json::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Json, A::Error> {
                let mut v = Vec::new();
                while let Some((k, x)) = m.next_entry::<String, Json>()? {
                    v.push((k, x));
                }
                Ok(Json::Obj(v))
            }
        }
        d.deserialize_any(V)
    }
}

impl Json {
    pub fn entries(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(v) => v,
            _ => &[],
        }
    }
    pub fn value(&self) -> Value {
        match self {
            Json::Obj(v) => Value::Object(v.iter().map(|(k, x)| (k.clone(), x.value())).collect()),
            Json::Arr(a) => Value::Array(a.iter().map(Json::value).collect()),
            Json::Val(v) => v.clone(),
        }
    }
}

/// A catalog, written by hand from watching each clip: what each clip is (`clip: [tags, what
/// the body does, the clip it was made from]`; the tags `loop` and `once` say whether it loops),
/// where each library came from (`$sources`), clips left out on purpose and why (`$skip`), and
/// for a capture database which take and stretch each clip is cut from (`$pick`:
/// `clip: [take, from, to, shortest cycle]`, seconds) and which database (`$library`: `cmu`,
/// the default, or `100style`).
#[derive(Default)]
pub struct Catalog {
    pub sources: Map<String, Value>,
    pub skip: BTreeMap<String, String>,
    pub pick: Vec<(String, Value)>,
    pub library: String,
    clips: BTreeMap<String, Value>,
}

/// A catalog's line for one clip.
pub struct Entry {
    pub tags: Vec<String>,
    pub desc: String,
    pub orig: Option<String>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog> {
        let clean: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let j: Json = serde_json::from_str(&clean)?;
        let mut c = Catalog { library: "cmu".into(), ..Default::default() };
        for (k, v) in j.entries() {
            match k.as_str() {
                "$sources" => c.sources = v.value().as_object().cloned().unwrap_or_default(),
                "$skip" => {
                    for (n, why) in v.entries() {
                        c.skip.insert(n.clone(), why.value().as_str().unwrap_or("").to_string());
                    }
                }
                "$pick" => c.pick = v.entries().iter().map(|(n, p)| (n.clone(), p.value())).collect(),
                "$library" => c.library = v.value().as_str().unwrap_or("cmu").to_lowercase(),
                _ if k.starts_with('$') => {}
                _ => {
                    c.clips.insert(k.clone(), v.value());
                }
            }
        }
        Ok(c)
    }

    pub fn read(path: &Path) -> Result<Catalog> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        Catalog::parse(&text).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    /// Does it say anything about clips (a catalog was given)?
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.sources.is_empty() && self.pick.is_empty()
    }

    /// The names of its item lines, sorted.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.clips.keys()
    }

    /// Whether the catalog has a line for this name (well-formed or not).
    pub fn has(&self, name: &str) -> bool {
        self.clips.contains_key(name)
    }

    pub fn entry(&self, name: &str) -> Option<Entry> {
        let v = self.clips.get(name)?.as_array()?;
        let s = |i: usize| v.get(i).and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(Entry {
            tags: s(0).split_whitespace().map(str::to_string).collect(),
            desc: s(1),
            orig: Some(s(2)).filter(|o| !o.is_empty()),
        })
    }
}
