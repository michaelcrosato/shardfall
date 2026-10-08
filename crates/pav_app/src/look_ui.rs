//! The Look & Filters window (pause menu): every filter on the whole scene, only the
//! characters and objects, or only the environment, with sliders and presets per filter,
//! whole looks, and looks of your own. The look is a layer over the scene's own settings
//! (rooms, places, the tuning panel): sections that are on replace them. Saved next to the
//! executable (`shardfall_looks.json`) or in the browser's local storage.

use egui::{RichText, Ui};
use pav_view::ViewSettings;
use pav_view::build::{PaletteChoice, PartChoice, PixelTarget, StyleOverride};
use pav_view::look::{self, Look, Section};
use serde::{Deserialize, Serialize};
use web_time::Instant;

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedLook {
    pub name: String,
    pub look: Look,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct LookFile {
    version: u32,
    current: Look,
    saved: Vec<SavedLook>,
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> std::path::PathBuf {
    crate::boot::exe_dir().join("shardfall_looks.json")
}

#[cfg(target_arch = "wasm32")]
const KEY: &str = "shardfall_looks";

fn read_file() -> LookFile {
    #[cfg(not(target_arch = "wasm32"))]
    let text = std::fs::read_to_string(path()).ok();
    #[cfg(target_arch = "wasm32")]
    let text = web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.get_item(KEY).ok().flatten());
    text.and_then(|t| match serde_json::from_str(&t) {
        Ok(f) => Some(f),
        Err(e) => {
            log::warn!("looks file unreadable ({e}); starting from the scene's own look");
            None
        }
    })
    .unwrap_or_default()
}

fn write_file(f: &LookFile) {
    let Ok(text) = serde_json::to_string_pretty(f) else { return };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = path();
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &p)).is_err() {
            log::warn!("could not write {}", p.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    if web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.set_item(KEY, &text).ok()).is_none() {
        log::warn!("could not save looks to local storage");
    }
}

pub struct LookUi {
    pub open: bool,
    file: LookFile,
    name: String,
    /// When the look last changed without being written yet.
    dirty: Option<Instant>,
}

impl LookUi {
    pub fn load() -> Self {
        let mut file = read_file();
        file.version = 1;
        Self { open: false, file, name: "my look".into(), dirty: None }
    }

    /// The look (for agent tools over the live bridge).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn look(&self) -> &Look {
        &self.file.current
    }

    /// Replaces the look (agent tools over the live bridge).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_look(&mut self, look: Look) {
        self.file.current = look;
        self.touch();
    }

    /// The settings to draw with: the scene's own, with the look's sections that are on.
    pub fn apply(&self, view: &mut ViewSettings) {
        self.file.current.apply(view);
    }

    /// Writes the look a moment after the last change (or now with `force`).
    pub fn save_if_dirty(&mut self, force: bool) {
        if let Some(t) = self.dirty {
            if force || !self.open || t.elapsed().as_secs_f32() > 2.0 {
                write_file(&self.file);
                self.dirty = None;
            }
        }
    }

    fn touch(&mut self) {
        self.dirty = Some(Instant::now());
    }

    /// Draws the window. `base` is the scene's own view (what a section shows while off).
    /// Returns a message for a toast, if any.
    pub fn window(&mut self, ctx: &egui::Context, base: &ViewSettings) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut msg = None;
        let mut changed = false;
        let height = (ctx.content_rect().height() - 150.0).max(240.0);
        egui::Window::new("Look & Filters")
            .open(&mut open)
            .default_pos([ctx.content_rect().right() - 450.0, 96.0])
            .default_width(410.0)
            .default_height(height.min(720.0))
            .max_height(height)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Mix and match filters: each one works on the whole scene, only the characters & objects, \
                         or only the environment. A filter that is switched on replaces the scene's own setting.",
                    )
                    .small(),
                );
                ui.add_space(4.0);
                changed |= self.looks_section(ui, &mut msg);
                ui.separator();
                let look = &mut self.file.current;
                for sec in Section::ALL {
                    changed |= section(ui, look, sec, base, |ui, v| body(ui, sec, v));
                }
                ui.separator();
                ui.label(
                    RichText::new(
                        "Characters & objects: anything that moves, fights or can be picked up (heroes, monsters, \
                         props, loot, spells). Environment: floors, walls, fixed scenery and the sky.",
                    )
                    .small()
                    .weak(),
                );
            });
        if changed {
            self.touch();
        }
        if !open {
            self.open = false;
            self.save_if_dirty(true);
        }
        msg
    }

    /// Whole looks, looks of your own, compare and reset.
    fn looks_section(&mut self, ui: &mut Ui, msg: &mut Option<String>) -> bool {
        let mut changed = false;
        let look = &mut self.file.current;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Looks").strong());
            let summary = if look.on.is_empty() {
                "the scene's own".to_string()
            } else {
                look.on.iter().map(|s| s.key()).collect::<Vec<_>>().join(" · ")
            };
            ui.label(RichText::new(summary).small().weak());
        });
        ui.horizontal_wrapped(|ui| {
            for p in look::looks() {
                if ui.button(&p.name).on_hover_text(&p.about).clicked() {
                    let compare = look.compare;
                    *look = Look::from_preset(p);
                    look.compare = compare;
                    changed = true;
                    *msg = Some(format!("look: {}", p.name));
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Scene default").on_hover_text("Switch every filter off: the scene's own look").clicked() {
                look.on.clear();
                changed = true;
            }
            ui.label("Compare");
            let r = ui
                .add(egui::Slider::new(&mut look.compare, 0.0..=0.9).fixed_decimals(2))
                .on_hover_text("Filters only right of this line (0 = the whole screen): before and after side by side");
            changed |= r.changed();
        });
        egui::CollapsingHeader::new("My looks").id_salt("my_looks").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(160.0));
                if ui.button("Save").on_hover_text("Save the current look under this name").clicked() {
                    let name = self.name.trim().to_string();
                    if !name.is_empty() {
                        let entry = SavedLook { name: name.clone(), look: self.file.current.clone() };
                        match self.file.saved.iter_mut().find(|s| s.name == name) {
                            Some(s) => *s = entry,
                            None => self.file.saved.push(entry),
                        }
                        write_file(&self.file);
                        *msg = Some(format!("saved look '{name}'"));
                    }
                }
            });
            let mut load = None;
            let mut delete = None;
            for (i, s) in self.file.saved.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.button("Use").clicked() {
                        load = Some(i);
                    }
                    if ui.small_button("✕").on_hover_text("Delete").clicked() {
                        delete = Some(i);
                    }
                    ui.label(&s.name);
                });
            }
            if self.file.saved.is_empty() {
                ui.label(RichText::new("Nothing saved yet.").small().weak());
            }
            if let Some(i) = load {
                self.file.current = self.file.saved[i].look.clone();
                self.name = self.file.saved[i].name.clone();
                changed = true;
                *msg = Some(format!("look: {}", self.name));
            }
            if let Some(i) = delete {
                let gone = self.file.saved.remove(i);
                write_file(&self.file);
                *msg = Some(format!("deleted look '{}'", gone.name));
            }
        });
        changed
    }
}

/// One filter: a header with its switch and current preset, then presets and sliders.
fn section(
    ui: &mut Ui,
    look: &mut Look,
    sec: Section,
    base: &ViewSettings,
    body: impl FnOnce(&mut Ui, &mut ViewSettings) -> bool,
) -> bool {
    let mut changed = false;
    let id = ui.make_persistent_id(("look_section", sec.key()));
    let on = look.is_on(sec);
    let badge = if !on {
        "scene default".to_string()
    } else {
        let part = part_label(sec, &look.values);
        let preset = look.matching_preset(sec).map(|p| p.name.clone()).unwrap_or_else(|| "custom".into());
        if part.is_empty() { preset } else { format!("{preset} · {part}") }
    };
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let (_, header, _) = state
        .show_header(ui, |ui| {
            let mut o = on;
            let r =
                ui.checkbox(&mut o, "").on_hover_text("On: these settings replace the scene's own. Off: the scene's own show.");
            ui.label(RichText::new(sec.title()).strong());
            ui.label(RichText::new(badge).small().weak());
            r.changed().then_some(o)
        })
        .body(|ui| {
            ui.label(RichText::new(sec.about()).small().weak());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Presets").small());
                let current = look.is_on(sec).then(|| look.matching_preset(sec)).flatten().map(|p| p.name.as_str());
                for p in look::presets(sec) {
                    if ui.selectable_label(current == Some(p.name.as_str()), &p.name).on_hover_text(&p.about).clicked() {
                        look.apply_preset(p, base);
                        changed = true;
                    }
                }
            });
            let mut v = if look.is_on(sec) { look.values.clone() } else { base.clone() };
            if body(ui, &mut v) {
                look.set_on(sec, true, base);
                sec.copy(&v, &mut look.values);
                changed = true;
            }
        });
    if let Some(o) = header.inner {
        look.set_on(sec, o, base);
        changed = true;
    }
    changed
}

/// Where a section's filter is, for its header.
fn part_label(sec: Section, v: &ViewSettings) -> String {
    let f = &v.filter;
    let p = |c: PartChoice| part_name(c).to_string();
    match sec {
        Section::Pixel => match f.pixel_target {
            PixelTarget::All => p(PartChoice::All),
            PixelTarget::Objects => p(PartChoice::Objects),
            PixelTarget::Environment => p(PartChoice::Environment),
            t => pixel_extra_name(t).to_string(),
        },
        Section::Shading => {
            if v.style != StyleOverride::PerObject {
                format!("everything {}", style_name(v.style))
            } else {
                format!("{} / {}", style_name(v.style_objects), style_name(v.style_environment))
            }
        }
        Section::Outlines => p(v.outlines_on),
        Section::Palette => p(f.color_on),
        Section::Grading => p(f.grade_on),
        Section::Scanlines => p(f.scanlines_on),
        Section::Grain => {
            if f.grain_on == f.chroma_on {
                p(f.grain_on)
            } else {
                format!("{} / {}", p(f.grain_on), p(f.chroma_on))
            }
        }
        Section::Screen | Section::Glow => String::new(),
    }
}

fn part_name(c: PartChoice) -> &'static str {
    match c {
        PartChoice::All => "whole scene",
        PartChoice::Objects => "characters & objects",
        PartChoice::Environment => "environment",
    }
}

fn pixel_extra_name(t: PixelTarget) -> &'static str {
    match t {
        PixelTarget::Characters => "characters only",
        PixelTarget::Hero => "the hero",
        PixelTarget::Others => "other characters",
        PixelTarget::World => "all but characters",
        PixelTarget::Entity => "one entity",
        PixelTarget::All => "whole scene",
        PixelTarget::Objects => "characters & objects",
        PixelTarget::Environment => "environment",
    }
}

fn style_name(s: StyleOverride) -> &'static str {
    match s {
        StyleOverride::PerObject => "as authored",
        StyleOverride::Flat => "flat",
        StyleOverride::Cel => "cel",
        StyleOverride::Lit => "lit",
    }
}

/// Whole scene / characters & objects / environment.
fn part_picker(ui: &mut Ui, label: &str, c: &mut PartChoice) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        for o in [PartChoice::All, PartChoice::Objects, PartChoice::Environment] {
            changed |= ui.selectable_value(c, o, part_name(o)).changed();
        }
    });
    changed
}

fn style_picker(ui: &mut Ui, label: &str, s: &mut StyleOverride) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        for o in [StyleOverride::PerObject, StyleOverride::Flat, StyleOverride::Cel, StyleOverride::Lit] {
            changed |= ui.selectable_value(s, o, style_name(o)).changed();
        }
    });
    changed
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, text: &str, help: &str) -> bool {
    ui.add(egui::Slider::new(v, range).text(text).clamping(egui::SliderClamping::Always)).on_hover_text(help).changed()
}

/// The sliders and choices of one section.
fn body(ui: &mut Ui, sec: Section, v: &mut ViewSettings) -> bool {
    let mut c = false;
    match sec {
        Section::Pixel => {
            let f = &mut v.filter;
            let mut part = match f.pixel_target {
                PixelTarget::All => Some(PartChoice::All),
                PixelTarget::Objects => Some(PartChoice::Objects),
                PixelTarget::Environment => Some(PartChoice::Environment),
                _ => None,
            };
            ui.horizontal_wrapped(|ui| {
                ui.label("On");
                for o in [PartChoice::All, PartChoice::Objects, PartChoice::Environment] {
                    if ui.selectable_label(part == Some(o), part_name(o)).clicked() {
                        part = Some(o);
                        f.pixel_target = match o {
                            PartChoice::All => PixelTarget::All,
                            PartChoice::Objects => PixelTarget::Objects,
                            PartChoice::Environment => PixelTarget::Environment,
                        };
                        c = true;
                    }
                }
                egui::ComboBox::from_id_salt("pixel_more")
                    .selected_text(if part.is_none() { pixel_extra_name(f.pixel_target) } else { "more…" })
                    .show_ui(ui, |ui| {
                        for t in [
                            PixelTarget::Characters,
                            PixelTarget::Hero,
                            PixelTarget::Others,
                            PixelTarget::World,
                            PixelTarget::Entity,
                        ] {
                            c |= ui.selectable_value(&mut f.pixel_target, t, pixel_extra_name(t)).changed();
                        }
                    });
            });
            if f.pixel_target == PixelTarget::Entity {
                ui.horizontal(|ui| {
                    ui.label("Entity id");
                    c |= ui.add(egui::DragValue::new(&mut f.pixel_entity).range(0..=i32::MAX)).changed();
                });
            }
            c |= slider(ui, &mut f.pixel_art, 1.0..=16.0, "block size (px)", "Size of a pixel-art block on screen (1 = off)");
            c |= slider(
                ui,
                &mut f.pixel_levels,
                0.0..=32.0,
                "colour levels",
                "Shades per colour channel in the pixel art (below 2 = all)",
            );
            c |= ui
                .checkbox(&mut f.pixel_outline, "dark outline")
                .on_hover_text("A one-block dark line around pixel-art silhouettes")
                .changed();
            if f.pixel_art <= 1.0 {
                ui.label(RichText::new("Block size 1 = off: drag it up or pick a preset.").small().weak());
            }
        }
        Section::Shading => {
            if v.style != StyleOverride::PerObject {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("The scene forces {} everywhere.", style_name(v.style))).small());
                    if ui.small_button("Use the choices below").clicked() {
                        v.style = StyleOverride::PerObject;
                        c = true;
                    }
                });
            }
            c |= style_picker(ui, "Characters & objects", &mut v.style_objects);
            c |= style_picker(ui, "Environment", &mut v.style_environment);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Both:").small());
                for o in [StyleOverride::Flat, StyleOverride::Cel, StyleOverride::Lit] {
                    if ui.small_button(style_name(o)).clicked() {
                        v.style_objects = o;
                        v.style_environment = o;
                        c = true;
                    }
                }
            });
            c |= slider(ui, &mut v.cel_bands, 2.0..=6.0, "cel bands", "Light bands in cel shading");
            c |= slider(ui, &mut v.rim, 0.0..=1.0, "rim light", "Bright edge on cel-shaded things");
            c |= slider(ui, &mut v.flat_shadow, 0.0..=1.0, "flat shadow", "Brightness of shadows in flat shading");
            c |= slider(ui, &mut v.specular, 0.0..=2.0, "highlights", "Specular highlights in lit shading");
        }
        Section::Outlines => {
            c |= ui.checkbox(&mut v.outlines, "outlines").changed();
            c |= part_picker(ui, "On", &mut v.outlines_on);
            c |= slider(ui, &mut v.outline_px, 1.0..=4.0, "thickness (px)", "Outline thickness");
            c |= slider(
                ui,
                &mut v.outline_darken,
                0.0..=1.0,
                "brightness",
                "Outline brightness relative to the surface (0 = black)",
            );
        }
        Section::Palette => {
            let f = &mut v.filter;
            c |= part_picker(ui, "On", &mut f.color_on);
            ui.horizontal_wrapped(|ui| {
                ui.label("Palette");
                for (p, name) in [
                    (PaletteChoice::None, "none"),
                    (PaletteChoice::GameBoy, "Game Boy"),
                    (PaletteChoice::Pico8, "PICO-8"),
                    (PaletteChoice::Cga, "CGA"),
                    (PaletteChoice::OneBit, "1-bit"),
                    (PaletteChoice::Amber, "amber"),
                ] {
                    c |= ui.selectable_value(&mut f.palette, p, name).changed();
                }
            });
            c |= slider(
                ui,
                &mut f.levels,
                0.0..=32.0,
                "shades per channel",
                "Colour levels per channel without a palette (below 2 = off)",
            );
            c |= slider(ui, &mut f.dither, 0.0..=1.0, "dither", "Ordered dithering between the colours");
        }
        Section::Grading => {
            let f = &mut v.filter;
            c |= part_picker(ui, "On", &mut f.grade_on);
            c |= slider(ui, &mut f.temperature, -1.0..=1.0, "temperature", "Warm (+) or cool (-)");
            c |= slider(ui, &mut f.tint, -1.0..=1.0, "tint", "Green (+) or magenta (-)");
            c |= slider(ui, &mut f.contrast, 0.3..=2.0, "contrast", "Contrast");
            c |= slider(ui, &mut f.brightness, 0.3..=2.0, "brightness", "Brightness");
            c |= slider(ui, &mut f.saturation, 0.0..=2.0, "saturation", "Saturation (0 = grey)");
        }
        Section::Scanlines => {
            let f = &mut v.filter;
            c |= part_picker(ui, "On", &mut f.scanlines_on);
            c |= slider(ui, &mut f.scanlines, 0.0..=1.0, "darkness", "Scanline darkness (0 = off)");
            c |= slider(ui, &mut f.scanline_px, 1.0..=8.0, "spacing (px)", "Pixels from one line to the next");
        }
        Section::Grain => {
            let f = &mut v.filter;
            c |= part_picker(ui, "Grain on", &mut f.grain_on);
            c |= slider(ui, &mut f.grain, 0.0..=1.0, "grain", "Film grain (0 = off)");
            c |= part_picker(ui, "Fringe on", &mut f.chroma_on);
            c |= slider(ui, &mut f.chroma, 0.0..=1.0, "colour fringe", "Chromatic aberration (0 = off)");
        }
        Section::Screen => {
            let f = &mut v.filter;
            ui.label(RichText::new("Whole screen.").small().weak());
            c |= slider(ui, &mut f.curvature, 0.0..=0.5, "tube curvature", "CRT curvature (0 = flat)");
            c |= slider(ui, &mut f.vignette, 0.0..=1.0, "vignette", "Darker corners");
            c |= slider(
                ui,
                &mut f.pixelate,
                1.0..=16.0,
                "low resolution (px)",
                "The whole screen in blocks of this size (1 = off)",
            );
        }
        Section::Glow => {
            ui.label(RichText::new("Whole scene.").small().weak());
            c |= slider(ui, &mut v.bloom, 0.0..=2.0, "bloom", "Glow around bright things (0 = off)");
            c |= slider(ui, &mut v.bloom_threshold, 0.2..=4.0, "bloom threshold", "Brightness where glow starts");
            c |= slider(ui, &mut v.exposure, 0.2..=3.0, "exposure", "Overall exposure");
            c |= slider(ui, &mut v.gi, 0.0..=2.0, "bounce light", "Screen-space bounce light and soft occlusion (0 = off)");
        }
    }
    c
}
