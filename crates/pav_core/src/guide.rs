//! The field guide: a glossary for the world demo's station guides (`field_guide.toml`).

use std::sync::OnceLock;

use serde::Deserialize;

/// One word of the field guide.
#[derive(Clone, Debug, Deserialize)]
pub struct Term {
    /// What room files call it (lower case).
    pub key: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub see: Vec<String>,
}

#[derive(Deserialize)]
struct Guide {
    term: Vec<Term>,
}

/// Every term, in file order.
pub fn terms() -> &'static [Term] {
    static TERMS: OnceLock<Vec<Term>> = OnceLock::new();
    TERMS.get_or_init(|| toml::from_str::<Guide>(include_str!("field_guide.toml")).expect("field_guide.toml").term)
}

/// A term by key (or name), ignoring case.
pub fn term(key: &str) -> Option<&'static Term> {
    terms().iter().find(|t| t.key.eq_ignore_ascii_case(key) || t.name.eq_ignore_ascii_case(key))
}

/// Terms whose key, name or text mention `query` (ignoring case).
pub fn search(query: &str) -> Vec<&'static Term> {
    let q = query.to_lowercase();
    terms()
        .iter()
        .filter(|t| t.key.contains(&q) || t.name.to_lowercase().contains(&q) || t.text.to_lowercase().contains(&q))
        .collect()
}
