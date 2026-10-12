//! Resolving a material: its `base` chain merged (shallow: each field from the nearest material
//! that gives it; `layers` replaced wholesale), then the budgets and legibility checks of asset
//! plan §6.3, reported as warnings.

use std::collections::BTreeMap;

use super::schema::{Fields, LayerDoc, MatDoc, Param};
use crate::issue::{self, Code, Issue, join};
use crate::props::doc::{MapMode, Pattern};
use crate::shape::Look;

/// `base` links in a chain (a material, its base, … : at most this many hops).
pub const MAX_DEPTH: usize = 4;
/// Cost units above which a material is reported as expensive.
pub const MAX_COST: f32 = 6.0;
/// Features smaller than this (metres) blur at the game camera.
pub const MIN_FEATURE: f32 = 0.05;
/// Gaps narrower than this (metres) vanish at the game camera.
pub const MIN_GAP: f32 = 0.015;
/// Metres per pixel at the Shardfall camera at 540p (§6.3), for the legibility messages.
const PIXEL_540P: f32 = 0.021;

/// A material with its base chain applied: every field decided, lengths in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// `MAT/oak_planks`; empty for an inline material.
    pub name: String,
    /// `#rrggbb` or a role.
    pub color: String,
    pub look: Look,
    pub emissive: f32,
    /// `None` keeps the renderer's legacy shading (metals default to 0.5 when packed).
    pub roughness: Option<f32>,
    pub metallic: f32,
    /// `None` is auto: the part decides (world for static blocks, axis for round parts).
    pub map: Option<MapMode>,
    pub scale: f32,
    pub turn: bool,
    pub layers: Vec<LayerDoc>,
    /// The material whose `layers` these are (itself or a base).
    pub layers_from: String,
}

impl Material {
    /// Cost units (§6.3): the sum over the layers.
    pub fn cost(&self) -> f32 {
        self.layers.iter().map(cost).sum()
    }

    /// Whether a theme palette changes it (a role in a layer colour or cell): such a material
    /// needs one GPU slot per palette. The base colour travels in the instance.
    pub fn layers_use_roles(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.color().is_some_and(|c| c.starts_with('$')) || layer.cells().iter().any(|c| c.starts_with('$')))
    }

    /// Budget and legibility warnings (§6.3), with paths into the material's own document.
    pub fn check(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        let total = self.cost();
        if total > MAX_COST {
            let parts: Vec<String> = self.layers.iter().map(|l| format!("{} {}", l.pattern.name(), cost_text(cost(l)))).collect();
            out.push(
                Issue::warning(
                    "layers",
                    Code::Limit,
                    format!("costs {} units ({}); keep it at {MAX_COST} or less", cost_text(total), parts.join(" + ")),
                )
                .fix("drop a layer, or lower a noise's detail"),
            );
        }
        let inherited =
            if self.layers_from != self.name { format!(" (layers from {})", self.layers_from) } else { String::new() };
        for (index, layer) in self.layers.iter().enumerate() {
            let path = format!("layers[{index}]");
            if let Some((key, feature)) = feature(layer) {
                let feature = feature * self.scale;
                if feature < MIN_FEATURE - 1e-6 {
                    out.push(
                        Issue::warning(
                            join(&path, key),
                            Code::Legibility,
                            format!(
                                "{} cm features are under 5 cm{inherited}: about {:.1} px at the game camera at 540p, where they fade to flat colour",
                                cm(feature),
                                feature / PIXEL_540P
                            ),
                        )
                        .fix("make them 5 cm or larger, or drop the layer"),
                    );
                }
            }
            if let Some(gap) = gap(layer).map(|gap| gap * self.scale).filter(|gap| *gap > 0.0 && *gap < MIN_GAP - 1e-6) {
                out.push(
                    Issue::warning(
                        join(&path, "gap"),
                        Code::Legibility,
                        format!("{} cm gaps are under 1.5 cm{inherited}: they vanish at the game camera at 540p", cm(gap)),
                    )
                    .fix("make gap 1.5 cm or wider, or 0 for none"),
                );
            }
        }
        out
    }
}

/// Resolves `fields`, the material `name` (`MAT/…`, or empty for an inline one), against the
/// library documents `docs` (keyed `MAT/…`). Errors carry the path `base`.
pub fn resolve(name: &str, fields: &Fields, docs: &BTreeMap<String, MatDoc>) -> Result<Material, Vec<Issue>> {
    let mut chain: Vec<(&str, &Fields)> = vec![(name, fields)];
    let mut current = fields;
    while let Some(base) = current.base.as_deref() {
        let from = chain.last().map(|(n, _)| *n).unwrap_or_default();
        let whose = if chain.len() > 1 { format!("{from}'s ") } else { String::new() };
        if chain.iter().any(|(n, _)| *n == base) {
            let names: Vec<&str> = chain.iter().map(|(n, _)| *n).chain([base]).collect();
            return Err(vec![
                Issue::error("base", Code::Cycle, format!("bases go round in a circle: {}", names.join(" → ")))
                    .fix("one of them must not have a base"),
            ]);
        }
        if chain.len() > MAX_DEPTH {
            let names: Vec<&str> = chain.iter().map(|(n, _)| *n).chain([base]).collect();
            return Err(vec![
                Issue::error("base", Code::Limit, format!("{} levels of base: {}", chain.len(), names.join(" → ")))
                    .expected(format!("at most {MAX_DEPTH}"))
                    .fix("start from a material nearer the root"),
            ]);
        }
        let Some(doc) = docs.get(base) else {
            return Err(vec![
                Issue::error("base", Code::UnknownMaterial, format!("{whose}base \"{base}\" is not in the library"))
                    .maybe_fix(issue::suggestion(base, docs.keys().map(String::as_str))),
            ]);
        };
        chain.push((base, &doc.fields));
        current = &doc.fields;
    }
    // Root first, so each nearer material overrides.
    let mut out = Fields::default();
    let mut layers_from = "";
    for (n, f) in chain.iter().rev() {
        macro_rules! take {
            ($($field:ident),*) => { $(if f.$field.is_some() { out.$field = f.$field.clone(); })* };
        }
        take!(color, look, emissive, roughness, metallic, map, scale, turn);
        if f.layers.is_some() {
            out.layers = f.layers.clone();
            layers_from = n;
        }
    }
    let Some(color) = out.color else {
        return Err(vec![Issue::error("color", Code::Missing, "missing").fix("a material needs color, or a base that has one")]);
    };
    Ok(Material {
        name: name.into(),
        color,
        look: out.look.unwrap_or_default(),
        emissive: out.emissive.unwrap_or(0.0),
        roughness: out.roughness,
        metallic: out.metallic.unwrap_or(0.0),
        map: out.map,
        scale: out.scale.unwrap_or(1.0),
        turn: out.turn.unwrap_or(false),
        layers: out.layers.unwrap_or_default(),
        layers_from: layers_from.into(),
    })
}

/// A layer's cost units (§6.3).
pub fn cost(layer: &LayerDoc) -> f32 {
    use Pattern::*;
    match layer.pattern {
        Brick | Planks | Tiles | Checker | Stripes | Gradient | Dots => 1.0,
        Shingles | Wood | Wave | Magic => 1.5,
        Voronoi | Cobbles => 2.5,
        Noise => 0.5 * layer.int("detail", 3) as f32,
    }
}

fn cost_text(units: f32) -> String {
    crate::props::amount(units as f64)
}

fn cm(metres: f32) -> String {
    crate::props::amount((metres as f64 * 1000.0).round() / 10.0)
}

/// The parameter that sets a layer's feature size, and that size in metres (§6.2: sizes are
/// feature sizes; a dot's is its diameter). Gradients have none.
fn feature(layer: &LayerDoc) -> Option<(&'static str, f32)> {
    match layer.pattern {
        Pattern::Gradient => None,
        Pattern::Dots => Some(("radius", 2.0 * layer.num("radius", super::eval::default_radius()))),
        pattern => {
            let size = layer.size2(super::eval::default_size(pattern));
            Some(("size", size[0].min(size[1])))
        }
    }
}

/// A layer's painted gap in metres, where it draws one.
fn gap(layer: &LayerDoc) -> Option<f32> {
    let draws = match layer.pattern {
        Pattern::Voronoi => layer.word("feature") == Some("edges"),
        pattern => super::eval::default_gap(pattern).is_some(),
    };
    let default = super::eval::default_gap(layer.pattern).unwrap_or(0.0);
    draws.then(|| match layer.params.get("gap") {
        Some(Param::Num(gap)) => *gap,
        _ => default,
    })
}
