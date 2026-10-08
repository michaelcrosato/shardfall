//! The look layer behind the Look & Filters menu: filter sections (pixel art, shading,
//! outlines, palette, grading...) that, when switched on, replace the scene's own view
//! settings (rooms, places, the tuning panel), plus presets for each section and whole looks
//! from `looks.toml`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use pav_core::params::ParamValue;
use serde::{Deserialize, Serialize};

use crate::build::ViewSettings;

/// One filter of the menu and the view settings it owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Pixel,
    Shading,
    Outlines,
    Palette,
    Grading,
    Scanlines,
    Grain,
    Screen,
    Glow,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Pixel,
        Section::Shading,
        Section::Outlines,
        Section::Palette,
        Section::Grading,
        Section::Scanlines,
        Section::Grain,
        Section::Screen,
        Section::Glow,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Section::Pixel => "pixel",
            Section::Shading => "shading",
            Section::Outlines => "outlines",
            Section::Palette => "palette",
            Section::Grading => "grading",
            Section::Scanlines => "scanlines",
            Section::Grain => "grain",
            Section::Screen => "screen",
            Section::Glow => "glow",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Pixel => "Pixel art",
            Section::Shading => "Shading (cel, flat, lit)",
            Section::Outlines => "Outlines",
            Section::Palette => "Palette & colour depth",
            Section::Grading => "Colour grading",
            Section::Scanlines => "Scanlines",
            Section::Grain => "Grain & colour fringe",
            Section::Screen => "Screen (CRT)",
            Section::Glow => "Glow & light",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Section::Pixel => "Turns part of the scene into pixel art while the rest stays sharp.",
            Section::Shading => "How surfaces take light: cel bands, flat colour or smooth lighting, for each part.",
            Section::Outlines => "Lines along silhouettes and creases.",
            Section::Palette => "Fewer colours: a fixed retro palette or a few shades per channel, with dithering.",
            Section::Grading => "Temperature, tint, contrast, brightness and saturation.",
            Section::Scanlines => "Dark horizontal lines, like a CRT.",
            Section::Grain => "Film grain and chromatic aberration.",
            Section::Screen => "Whole-screen effects: tube curvature, vignette and a lower resolution.",
            Section::Glow => "Bloom, exposure and bounce light (whole scene).",
        }
    }

    pub fn from_key(key: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.key().eq_ignore_ascii_case(key))
    }

    /// The view settings this section owns (paths as in `pav params prefix=view`, without
    /// `view.`). The first `*_on` / `*_target` path, if any, is the part of the scene it is on.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            Section::Pixel => {
                &["filter.pixel_target", "filter.pixel_art", "filter.pixel_entity", "filter.pixel_levels", "filter.pixel_outline"]
            }
            Section::Shading => &["style", "style_objects", "style_environment", "cel_bands", "rim", "flat_shadow", "specular"],
            Section::Outlines => &["outlines_on", "outlines", "outline_px", "outline_darken"],
            Section::Palette => &["filter.color_on", "filter.palette", "filter.levels", "filter.dither"],
            Section::Grading => &[
                "filter.grade_on",
                "filter.temperature",
                "filter.tint",
                "filter.contrast",
                "filter.brightness",
                "filter.saturation",
            ],
            Section::Scanlines => &["filter.scanlines_on", "filter.scanlines", "filter.scanline_px"],
            Section::Grain => &["filter.grain_on", "filter.chroma_on", "filter.grain", "filter.chroma"],
            Section::Screen => &["filter.curvature", "filter.vignette", "filter.pixelate"],
            Section::Glow => &["bloom", "bloom_threshold", "exposure", "gi"],
        }
    }

    /// The section that owns a view path.
    pub fn of_path(path: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.paths().contains(&path))
    }

    /// Whether a path says which part of the scene a filter is on (presets leave these alone).
    pub fn is_part_path(path: &str) -> bool {
        path.ends_with("_on") || path.ends_with("_target") || path == "filter.pixel_entity"
    }

    /// Copies this section's settings from `from` to `to`.
    pub fn copy(self, from: &ViewSettings, to: &mut ViewSettings) {
        let (f, t) = (&from.filter, &mut to.filter);
        match self {
            Section::Pixel => {
                t.pixel_target = f.pixel_target;
                t.pixel_art = f.pixel_art;
                t.pixel_entity = f.pixel_entity;
                t.pixel_levels = f.pixel_levels;
                t.pixel_outline = f.pixel_outline;
            }
            Section::Shading => {
                to.style = from.style;
                to.style_objects = from.style_objects;
                to.style_environment = from.style_environment;
                to.cel_bands = from.cel_bands;
                to.rim = from.rim;
                to.flat_shadow = from.flat_shadow;
                to.specular = from.specular;
            }
            Section::Outlines => {
                to.outlines_on = from.outlines_on;
                to.outlines = from.outlines;
                to.outline_px = from.outline_px;
                to.outline_darken = from.outline_darken;
            }
            Section::Palette => {
                t.color_on = f.color_on;
                t.palette = f.palette;
                t.levels = f.levels;
                t.dither = f.dither;
            }
            Section::Grading => {
                t.grade_on = f.grade_on;
                t.temperature = f.temperature;
                t.tint = f.tint;
                t.contrast = f.contrast;
                t.brightness = f.brightness;
                t.saturation = f.saturation;
            }
            Section::Scanlines => {
                t.scanlines_on = f.scanlines_on;
                t.scanlines = f.scanlines;
                t.scanline_px = f.scanline_px;
            }
            Section::Grain => {
                t.grain_on = f.grain_on;
                t.chroma_on = f.chroma_on;
                t.grain = f.grain;
                t.chroma = f.chroma;
            }
            Section::Screen => {
                t.curvature = f.curvature;
                t.vignette = f.vignette;
                t.pixelate = f.pixelate;
            }
            Section::Glow => {
                to.bloom = from.bloom;
                to.bloom_threshold = from.bloom_threshold;
                to.exposure = from.exposure;
                to.gi = from.gi;
            }
        }
    }
}

/// One filter's preset (`[[preset]]` in looks.toml).
#[derive(Clone, Debug, Deserialize)]
pub struct Preset {
    pub section: Section,
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub set: BTreeMap<String, ParamValue>,
}

/// A whole look (`[[look]]` in looks.toml).
#[derive(Clone, Debug, Deserialize)]
pub struct LookPreset {
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub set: BTreeMap<String, ParamValue>,
}

#[derive(Deserialize)]
struct LookData {
    #[serde(default)]
    preset: Vec<Preset>,
    #[serde(default)]
    look: Vec<LookPreset>,
}

fn data() -> &'static LookData {
    static DATA: OnceLock<LookData> = OnceLock::new();
    DATA.get_or_init(|| toml::from_str(include_str!("looks.toml")).expect("looks.toml"))
}

/// The presets of one section, in file order.
pub fn presets(section: Section) -> impl Iterator<Item = &'static Preset> {
    data().preset.iter().filter(move |p| p.section == section)
}

pub fn preset(section: Section, name: &str) -> Option<&'static Preset> {
    presets(section).find(|p| p.name.eq_ignore_ascii_case(name))
}

/// The whole looks, in file order.
pub fn looks() -> &'static [LookPreset] {
    &data().look
}

pub fn look_preset(name: &str) -> Option<&'static LookPreset> {
    looks().iter().find(|l| l.name.eq_ignore_ascii_case(name))
}

/// The look layer: the sections that are on replace the scene's own settings with `values`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub on: BTreeSet<Section>,
    pub values: ViewSettings,
    /// Filters only right of this screen fraction (0 = whole screen): before / after.
    pub compare: f32,
}

impl Look {
    /// A whole look from looks.toml: the sections it names are on (from the defaults plus its
    /// settings), every other section is off.
    pub fn from_preset(p: &LookPreset) -> Look {
        let mut look = Look::default();
        look.values.apply(&p.set);
        look.on = p.set.keys().filter_map(|k| Section::of_path(k)).collect();
        look
    }

    pub fn is_on(&self, s: Section) -> bool {
        self.on.contains(&s)
    }

    /// Switches a section on (starting from `current`, what is on screen now, so nothing
    /// jumps) or off (the scene's own settings show again).
    pub fn set_on(&mut self, s: Section, on: bool, current: &ViewSettings) {
        if on && self.on.insert(s) {
            s.copy(current, &mut self.values);
        } else if !on {
            self.on.remove(&s);
        }
    }

    /// Applies one section preset (keeping the part of the scene the filter is on) and
    /// switches the section on.
    pub fn apply_preset(&mut self, p: &Preset, current: &ViewSettings) {
        self.set_on(p.section, true, current);
        let set: BTreeMap<String, ParamValue> =
            p.set.iter().filter(|(k, _)| !Section::is_part_path(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        self.values.apply(&set);
    }

    /// The preset of a section that matches the current values, if any.
    pub fn matching_preset(&self, s: Section) -> Option<&'static Preset> {
        let mut values = self.values.clone();
        let now = pav_core::params::to_map(&mut values);
        presets(s).find(|p| {
            p.set.iter().filter(|(k, _)| !Section::is_part_path(k)).all(|(k, v)| match (now.get(k), v) {
                (Some(a), b) => match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) if !matches!(a, ParamValue::Text(_)) => (x - y).abs() < 1e-3,
                    _ => a == b,
                },
                (None, _) => false,
            })
        })
    }

    /// The settings to draw with: `base` (the scene's own) with the sections that are on
    /// replaced.
    pub fn apply(&self, view: &mut ViewSettings) {
        for s in &self.on {
            s.copy(&self.values, view);
        }
        if self.compare > 0.0 {
            view.filter.split = self.compare;
        }
    }

    /// The same as a list of view paths and values (for tools and room files).
    pub fn to_map(&self) -> BTreeMap<String, ParamValue> {
        let mut values = self.values.clone();
        let all = pav_core::params::to_map(&mut values);
        let mut out: BTreeMap<String, ParamValue> =
            all.into_iter().filter(|(k, _)| Section::of_path(k).is_some_and(|s| self.is_on(s))).collect();
        if self.compare > 0.0 {
            out.insert("filter.split".into(), ParamValue::Float(self.compare as f64));
        }
        out
    }
}
