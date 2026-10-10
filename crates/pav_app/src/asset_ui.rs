//! Object authoring through the same tools used by live agents. Painting a panel never
//! submits a command; editable drafts keep the revision they were read from.

use egui::{Color32, RichText};
use pav_core::props::{self, PropAsset, PropPart};
use pav_core::{Look, RenderFrame, Shape};
use serde_json::{Value, json};

pub use crate::animation_ui::Command;

fn command(tool: &'static str, args: Value) -> Command {
    Command { tool, args: args.as_object().unwrap().clone() }
}

struct PartDraft {
    asset: String,
    name: String,
    revision: String,
    part: PropPart,
    dirty: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum JsonMode {
    #[default]
    Operations,
    Definition,
}

struct JsonDraft {
    asset: String,
    revision: String,
    text: String,
    dirty: bool,
}

pub struct AssetUi {
    pub open: bool,
    pub status: String,
    pub error: bool,
    search: String,
    source: String,
    preview_seen: Option<String>,
    new_name: String,
    selected_part: String,
    part: Option<PartDraft>,
    json_mode: JsonMode,
    json: Option<JsonDraft>,
    removed: Option<(String, String)>,
    position: [f32; 3],
    yaw: f32,
    scale: f32,
    collide: bool,
}

impl Default for AssetUi {
    fn default() -> Self {
        Self {
            open: false,
            status: String::new(),
            error: false,
            search: String::new(),
            source: "BUILTIN/bench".into(),
            preview_seen: None,
            new_name: "my_object".into(),
            selected_part: String::new(),
            part: None,
            json_mode: JsonMode::Operations,
            json: None,
            removed: None,
            position: [3.0, 0.0, 0.0],
            yaw: 0.0,
            scale: 1.0,
            collide: true,
        }
    }
}

impl AssetUi {
    pub fn new(open: bool) -> Self {
        Self { open, ..Self::default() }
    }

    /// Keep the redo control reachable when undo removes the active definition.
    pub fn creation_removed(&self) -> bool {
        self.removed.is_some()
    }

    /// Reports a command result. Definitions also arrive through the shared library, so
    /// external edits are visible without making an inspect request on every UI frame.
    pub fn report(&mut self, result: Result<Value, String>) {
        match result {
            Err(error) => {
                self.status = error;
                self.error = true;
            }
            Ok(value) => {
                self.error = false;
                if value.get("editable").is_some() {
                    if let Some(name) = value["name"].as_str() {
                        if self.source != name || value["changed"] == true || value.get("asset").is_some() {
                            self.part = None;
                            self.json = None;
                        }
                        self.source = name.into();
                        self.removed = if value["revision"] == "absent" && value["redo"].as_u64().unwrap_or(0) > 0 {
                            Some((name.into(), "absent".into()))
                        } else {
                            None
                        };
                    }
                    self.status = if value["revision"] == "absent" {
                        "Creation undone. Redo restores this object.".into()
                    } else if value["saved"] == true {
                        let refreshed = value["refreshed_instances"].as_u64().unwrap_or(0);
                        format!("Saved {}. {refreshed} placed instances updated.", self.source)
                    } else {
                        format!("{} is ready.", self.source)
                    };
                } else {
                    let instance = value.get("instance").unwrap_or(&value);
                    if let Some(id) = instance.get("id").and_then(Value::as_u64) {
                        self.status = format!("Placed instance #{id}. Return to the world to see it in the level.");
                    } else {
                        self.status.clear();
                    }
                }
                if let Some(error) = value.get("preview_error").and_then(Value::as_str) {
                    self.status.push_str(&format!(" Preview: {error}"));
                    self.error = true;
                }
            }
        }
    }

    pub fn window(&mut self, ctx: &egui::Context, frame: &RenderFrame, bridge: Option<&str>) -> Vec<Command> {
        let preview_name = frame.prop_preview.as_ref().map(|preview| preview.name.as_str());
        self.follow_preview(preview_name);
        if !self.open {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let mut open = self.open;
        egui::Window::new("Object Studio")
            .id(egui::Id::new("object_studio"))
            .open(&mut open)
            .default_pos([16.0, 80.0])
            .default_width(360.0)
            .default_height(680.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(RichText::new("CREATE, VIEW, AND PLACE OBJECTS").strong().color(Color32::from_rgb(100, 220, 186)));
                ui.small("Use these controls or prompt your LLM. Both edit the same objects.");
                if let Some(address) = bridge {
                    ui.small(format!("Live bridge: {address}"));
                }
                egui::ScrollArea::vertical().id_salt("object_studio_controls").show(ui, |ui| {
                    self.library_controls(ui, &mut commands);
                    if let Some(preview) = &frame.prop_preview {
                        ui.separator();
                        ui.label(RichText::new(format!("Viewing {}", preview.name)).strong());
                        if !preview.available {
                            ui.colored_label(
                                Color32::from_rgb(255, 165, 110),
                                "This object is unavailable. Choose another object.",
                            );
                        }
                        preview_controls(ui, preview, &mut commands);
                    }
                    let library = props::library();
                    if let Some(asset) = library.assets.get(&self.source) {
                        self.sync_drafts(asset, false);
                        ui.separator();
                        ui.label(RichText::new(format!("Object: {}", self.source)).strong());
                        if !asset.description.is_empty() {
                            ui.label(&asset.description);
                        }
                        let editable = self.source.starts_with("WORKSHOP/") && !cfg!(target_arch = "wasm32");
                        if editable {
                            let revision = asset.revision();
                            ui.horizontal(|ui| {
                                for (label, action) in [("Undo", "undo"), ("Redo", "redo")] {
                                    if ui.button(label).clicked() {
                                        commands.push(command(
                                            "asset_edit",
                                            json!({
                                                "action":action, "name":self.source, "if_revision":revision
                                            }),
                                        ));
                                    }
                                }
                                ui.small("Accepted edits save automatically.");
                            });
                        } else if !cfg!(target_arch = "wasm32") {
                            ui.small("Copy this template to edit its parts.");
                        }
                        self.part_controls(ui, asset, editable, &mut commands);
                        if editable {
                            self.json_controls(ui, asset, &mut commands);
                        }
                        if !cfg!(target_arch = "wasm32") {
                            self.placement_controls(ui, frame, &mut commands);
                        }
                    } else {
                        ui.small("Choose an available object from the library.");
                    }
                    if let Some((name, revision)) = &self.removed {
                        ui.separator();
                        if ui.button(format!("Redo creation of {name}")).clicked() {
                            commands.push(command("asset_edit", json!({"action":"redo", "name":name, "if_revision":revision})));
                        }
                    }
                    if !self.status.is_empty() {
                        ui.separator();
                        let color = if self.error { Color32::from_rgb(255, 145, 120) } else { Color32::from_rgb(130, 225, 190) };
                        ui.colored_label(color, &self.status);
                    }
                });
            });
        self.open = open;
        commands
    }

    fn follow_preview(&mut self, name: Option<&str>) {
        if self.preview_seen.as_deref() == name {
            return;
        }
        let unsaved = self.part.as_ref().is_some_and(|draft| draft.dirty) || self.json.as_ref().is_some_and(|draft| draft.dirty);
        if let Some(name) = name.filter(|_| !unsaved) {
            self.source = name.into();
        }
        self.preview_seen = name.map(str::to_owned);
    }

    fn library_controls(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        ui.separator();
        ui.label(RichText::new("Templates and saved objects").strong());
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search objects"));
        let search = self.search.to_lowercase();
        let library = props::library();
        egui::ComboBox::from_id_salt("object_source").selected_text(&self.source).width(300.0).show_ui(ui, |ui| {
            for (name, asset) in &library.assets {
                if search.is_empty()
                    || name.to_lowercase().contains(&search)
                    || asset.description.to_lowercase().contains(&search)
                {
                    ui.selectable_value(&mut self.source, name.clone(), name);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("View selected").clicked() {
                commands.push(command("asset_preview", json!({"action":"open", "name":self.source})));
            }
            if ui.button("Read latest").clicked() {
                if cfg!(target_arch = "wasm32") {
                    self.part = None;
                    self.json = None;
                } else {
                    commands.push(command("asset_edit", json!({"action":"inspect", "name":self.source})));
                }
            }
        });
        if cfg!(target_arch = "wasm32") {
            ui.small("Use the desktop studio to create, edit, and save objects.");
        } else {
            ui.label("New object name");
            ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("garden_bench"));
            ui.horizontal(|ui| {
                if ui.button("Create from box").clicked() {
                    commands.push(command("asset_edit", json!({"action":"create", "name":self.new_name})));
                }
                if ui.button("Copy selected").clicked() {
                    commands.push(command("asset_edit", json!({"action":"copy", "name":self.new_name, "from":self.source})));
                }
            });
            ui.small("Use lowercase letters, numbers, underscores, or hyphens.");
        }
    }

    fn sync_drafts(&mut self, asset: &PropAsset, force: bool) {
        let revision = asset.revision();
        let dirty_selection =
            self.part.as_ref().is_some_and(|draft| draft.asset == self.source && draft.name == self.selected_part && draft.dirty);
        if !asset.parts.contains_key(&self.selected_part) && !dirty_selection {
            self.selected_part = asset.parts.keys().next().cloned().unwrap_or_default();
        }
        let read_part = force
            || self.part.as_ref().is_none_or(|draft| {
                draft.asset != self.source || draft.name != self.selected_part || (!draft.dirty && draft.revision != revision)
            });
        if read_part {
            self.part = asset.parts.get(&self.selected_part).map(|part| PartDraft {
                asset: self.source.clone(),
                name: self.selected_part.clone(),
                revision: revision.clone(),
                part: part.clone(),
                dirty: false,
            });
        }
        let read_json = force
            || self.json.as_ref().is_none_or(|draft| draft.asset != self.source || (!draft.dirty && draft.revision != revision));
        if read_json {
            let value = match self.json_mode {
                JsonMode::Definition => serde_json::to_value(asset).unwrap(),
                JsonMode::Operations => json!([{
                    "op":"set", "part":self.selected_part,
                    "fields":{"color":asset.parts.get(&self.selected_part).map(|part| part.color.as_str()).unwrap_or("#e8704a")}
                }]),
            };
            self.json = Some(JsonDraft {
                asset: self.source.clone(),
                revision,
                text: serde_json::to_string_pretty(&value).unwrap(),
                dirty: false,
            });
        }
    }

    fn part_controls(&mut self, ui: &mut egui::Ui, asset: &PropAsset, editable: bool, commands: &mut Vec<Command>) {
        ui.separator();
        ui.label(RichText::new("Named parts").strong());
        egui::ComboBox::from_id_salt("object_part").selected_text(&self.selected_part).width(280.0).show_ui(ui, |ui| {
            for name in asset.parts.keys() {
                ui.selectable_value(&mut self.selected_part, name.clone(), name);
            }
        });
        self.sync_drafts(asset, false);
        let Some(draft) = &mut self.part else { return };
        let bounds = draft.part.bounds(1.0);
        let size = bounds.size();
        ui.small(format!("Extent: {:.2} × {:.2} × {:.2} m", size.x, size.y, size.z));
        let stale = draft.revision != asset.revision();
        ui.add_enabled_ui(editable, |ui| {
            draft.dirty |= vector(ui, "Local centre", &mut draft.part.pos);
            draft.dirty |= angles(ui, &mut draft.part.yaw, &mut draft.part.pitch, &mut draft.part.roll);
            draft.dirty |= shape_controls(ui, &mut draft.part.shape);
            ui.horizontal(|ui| {
                ui.label("Color");
                draft.dirty |= ui.add(egui::TextEdit::singleline(&mut draft.part.color).desired_width(100.0)).changed();
                let mut rgb = rgb(&draft.part.color);
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    draft.part.color = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    draft.dirty = true;
                }
            });
            egui::ComboBox::from_id_salt("object_part_look").selected_text(look_name(draft.part.look)).show_ui(ui, |ui| {
                for look in [Look::Flat, Look::Cel, Look::Lit, Look::Unlit, Look::Cutout] {
                    draft.dirty |= ui.selectable_value(&mut draft.part.look, look, look_name(look)).changed();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Glow");
                draft.dirty |= ui.add(egui::DragValue::new(&mut draft.part.emissive).speed(0.1)).changed();
                draft.dirty |= ui.checkbox(&mut draft.part.solid, "Solid part").changed();
            });
            if stale {
                ui.colored_label(
                    Color32::from_rgb(255, 190, 110),
                    "This object changed elsewhere. Read latest before applying this draft.",
                );
            }
            if ui.add_enabled(!stale, egui::Button::new("Apply part")).clicked() {
                match draft.part.validate() {
                    Ok(()) => commands.push(command(
                        "asset_edit",
                        json!({
                            "action":"patch", "name":draft.asset, "if_revision":draft.revision,
                            "ops":[{"op":"set", "part":draft.name, "fields":draft.part}]
                        }),
                    )),
                    Err(error) => {
                        self.error = true;
                        self.status = error;
                    }
                }
            }
        });
        ui.small("Positions use metres; rotations use degrees. Changes apply together when you press Apply part.");
    }

    fn json_controls(&mut self, ui: &mut egui::Ui, asset: &PropAsset, commands: &mut Vec<Command>) {
        ui.collapsing("Atomic JSON editor", |ui| {
            let before = self.json_mode;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.json_mode, JsonMode::Operations, "Operations");
                ui.selectable_value(&mut self.json_mode, JsonMode::Definition, "Full definition");
            });
            if self.json_mode != before {
                self.json = None;
                self.sync_drafts(asset, false);
            }
            ui.small(match self.json_mode {
                JsonMode::Operations => {
                    "An array of set, add, or remove operations on named parts. The whole batch succeeds or fails together."
                }
                JsonMode::Definition => "Edit the full object definition. Apply replaces it as one saved edit.",
            });
            if ui.button("Load latest JSON").clicked() {
                self.json = None;
                self.sync_drafts(asset, false);
            }
            let Some(draft) = &mut self.json else { return };
            draft.dirty |= ui
                .add(
                    egui::TextEdit::multiline(&mut draft.text)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            let stale = draft.revision != asset.revision();
            if stale {
                ui.colored_label(
                    Color32::from_rgb(255, 190, 110),
                    "The saved object changed. Load latest JSON before applying this draft.",
                );
            }
            if ui.add_enabled(!stale, egui::Button::new("Apply JSON")).clicked() {
                match serde_json::from_str::<Value>(&draft.text) {
                    Ok(value) if self.json_mode == JsonMode::Operations && value.is_array() => {
                        commands.push(command(
                            "asset_edit",
                            json!({
                                "action":"patch", "name":draft.asset, "if_revision":draft.revision, "ops":value
                            }),
                        ));
                    }
                    Ok(value) if self.json_mode == JsonMode::Definition && value.is_object() => {
                        commands.push(command(
                            "asset_edit",
                            json!({
                                "action":"replace", "name":draft.asset, "if_revision":draft.revision, "asset":value
                            }),
                        ));
                    }
                    Ok(_) => {
                        self.error = true;
                        self.status = match self.json_mode {
                            JsonMode::Operations => "Operations must be a JSON array.".into(),
                            JsonMode::Definition => "A definition must be a JSON object.".into(),
                        };
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = format!("Invalid JSON: {error}");
                    }
                }
            }
            ui.collapsing("Format guide", |ui| {
                ui.label(props::LEGEND);
            });
        });
    }

    fn placement_controls(&mut self, ui: &mut egui::Ui, frame: &RenderFrame, commands: &mut Vec<Command>) {
        ui.collapsing("Place in the current level", |ui| {
            ui.small("The position is the object's origin in world metres. Placement keeps the current preview open.");
            let mut pos = glam::Vec3::from_array(self.position);
            vector(ui, "Position", &mut pos);
            self.position = pos.to_array();
            ui.horizontal(|ui| {
                ui.label("Yaw");
                ui.add(egui::DragValue::new(&mut self.yaw).speed(1.0).suffix("°"));
                ui.label("Scale");
                ui.add(egui::DragValue::new(&mut self.scale).speed(0.05));
            });
            ui.checkbox(&mut self.collide, "Use solid parts for collision");
            if frame.prop_preview.is_none() && frame.animation_preview.is_none() {
                if ui.button("Use current world focus").clicked() {
                    self.position = [frame.focus.x + 2.0, 0.0, frame.focus.z];
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Place object").clicked() {
                    commands.push(command(
                        "asset_spawn",
                        json!({
                            "action":"add", "name":self.source, "pos":self.position,
                            "yaw":self.yaw, "scale":self.scale, "collide":self.collide
                        }),
                    ));
                }
                if ui.button("Return to world").clicked() {
                    commands.push(command("asset_preview", json!({"action":"close"})));
                }
            });
        });
    }
}

fn preview_controls(ui: &mut egui::Ui, preview: &pav_core::prop_preview::PropPreviewInfo, commands: &mut Vec<Command>) {
    let size = preview.bounds.size();
    ui.small(format!("{} parts · {:.2} × {:.2} × {:.2} m", preview.parts, size.x, size.y, size.z));
    ui.horizontal(|ui| {
        if ui.button("Fit camera").clicked() {
            commands.push(command("asset_preview", json!({"action":"fit"})));
        }
        if ui.button("Return to world").clicked() {
            commands.push(command("asset_preview", json!({"action":"close"})));
        }
    });
    ui.horizontal(|ui| {
        let mut turntable = preview.turntable;
        if ui.checkbox(&mut turntable, "Turntable").changed() {
            commands.push(command("asset_preview", json!({"turntable":turntable})));
        }
        if ui.button(if preview.playing { "Pause" } else { "Play" }).clicked() {
            commands.push(command("asset_preview", json!({"playing":!preview.playing})));
        }
    });
    let mut yaw = preview.yaw;
    if ui.add(egui::Slider::new(&mut yaw, -180.0..=180.0).clamping(egui::SliderClamping::Edits).text("Yaw").suffix("°")).changed()
    {
        commands.push(command("asset_preview", json!({"yaw":yaw})));
    }
    let mut speed = preview.speed;
    if ui
        .add(
            egui::Slider::new(&mut speed, 0.05..=8.0)
                .clamping(egui::SliderClamping::Edits)
                .logarithmic(true)
                .text("Speed")
                .suffix("x"),
        )
        .changed()
    {
        commands.push(command("asset_preview", json!({"speed":speed})));
    }
    ui.small("Right-drag to rotate the camera. Use the wheel to zoom.");
}

fn vector(ui: &mut egui::Ui, label: &str, value: &mut glam::Vec3) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for (axis, n) in [("X", &mut value.x), ("Y", &mut value.y), ("Z", &mut value.z)] {
            changed |= ui.add(egui::DragValue::new(n).speed(0.05).prefix(format!("{axis} "))).changed();
        }
    });
    changed
}

fn angles(ui: &mut egui::Ui, yaw: &mut f32, pitch: &mut f32, roll: &mut f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (label, value) in [("Yaw ", yaw), ("Pitch ", pitch), ("Roll ", roll)] {
            changed |= ui.add(egui::DragValue::new(value).speed(1.0).prefix(label).suffix("°")).changed();
        }
    });
    changed
}

fn shape_controls(ui: &mut egui::Ui, shape: &mut Shape) -> bool {
    let mut kind = match shape {
        Shape::Box { .. } => 0,
        Shape::RoundedBox { .. } => 1,
        Shape::Sphere { .. } => 2,
        Shape::Capsule { .. } => 3,
        Shape::Cylinder { .. } => 4,
    };
    let before = kind;
    let labels = ["Box", "Rounded box", "Sphere", "Capsule", "Cylinder"];
    egui::ComboBox::from_id_salt("object_part_shape").selected_text(labels[kind]).show_ui(ui, |ui| {
        for (i, label) in labels.iter().enumerate() {
            ui.selectable_value(&mut kind, i, *label);
        }
    });
    let mut changed = kind != before;
    if changed {
        *shape = match kind {
            0 => Shape::Box { half: glam::Vec3::splat(0.5) },
            1 => Shape::RoundedBox { half: glam::Vec3::splat(0.5), radius: 0.1 },
            2 => Shape::Sphere { radius: 0.5 },
            3 => Shape::Capsule { half_height: 0.5, radius: 0.25 },
            _ => Shape::Cylinder { half_height: 0.5, radius: 0.25 },
        };
    }
    match shape {
        Shape::Box { half } | Shape::RoundedBox { half, .. } => {
            changed |= vector(ui, "Half extents", half);
        }
        Shape::Capsule { half_height, .. } | Shape::Cylinder { half_height, .. } => {
            ui.horizontal(|ui| {
                ui.label("Half height");
                changed |= ui.add(egui::DragValue::new(half_height).speed(0.02)).changed();
            });
        }
        _ => {}
    }
    if let Shape::RoundedBox { radius, .. }
    | Shape::Sphere { radius }
    | Shape::Capsule { radius, .. }
    | Shape::Cylinder { radius, .. } = shape
    {
        ui.horizontal(|ui| {
            ui.label("Radius");
            changed |= ui.add(egui::DragValue::new(radius).speed(0.02)).changed();
        });
    }
    changed
}

fn look_name(look: Look) -> &'static str {
    match look {
        Look::Flat => "Flat",
        Look::Cel => "Cel",
        Look::Lit => "Lit",
        Look::Unlit => "Unlit",
        Look::Cutout => "Cutout",
    }
}

fn rgb(color: &str) -> [u8; 3] {
    if color.len() == 7 && color.starts_with('#') && color.is_ascii() {
        let part = |at| u8::from_str_radix(&color[at..at + 2], 16).unwrap_or(0);
        [part(1), part(3), part(5)]
    } else {
        [0xe8, 0x70, 0x4a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painting_object_controls_never_emits_a_live_edit() {
        let mut sim = pav_core::Sim::empty(1);
        let frame = sim.frame();
        let ctx = egui::Context::default();
        let mut panel = AssetUi::new(true);
        let preview = pav_core::prop_preview::PropPreviewInfo {
            name: "BUILTIN/bench".into(),
            revision: "test".into(),
            available: true,
            parts: 6,
            bounds: props::Bounds { min: glam::Vec3::ZERO, max: glam::Vec3::ONE },
            time: 0.450_000_05,
            playing: false,
            speed: 8.0,
            turntable: true,
            yaw: 270.0,
            scale: 1.0,
        };
        for _ in 0..3 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                let mut commands = panel.window(ui.ctx(), &frame, None);
                preview_controls(ui, &preview, &mut commands);
                assert!(commands.is_empty(), "painting a panel must not send a tool request");
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn a_remote_revision_does_not_overwrite_an_unsaved_part_draft() {
        let library = props::library();
        let original = library.assets.get("BUILTIN/bench").unwrap();
        let mut panel = AssetUi::new(true);
        panel.sync_drafts(original, false);
        let draft = panel.part.as_mut().unwrap();
        draft.part.color = "#123456".into();
        draft.dirty = true;
        let revision = draft.revision.clone();
        let mut remote = original.as_ref().clone();
        remote.parts.get_mut(&panel.selected_part).unwrap().color = "#654321".into();
        assert_ne!(remote.revision(), revision);
        panel.sync_drafts(&remote, false);
        let kept = panel.part.as_ref().unwrap();
        assert_eq!(kept.part.color, "#123456");
        assert_eq!(kept.revision, revision, "Apply must keep the original concurrency guard");
        assert_eq!(panel.json.as_ref().unwrap().revision, remote.revision(), "an untouched view follows live edits");
        remote.parts.remove(&panel.selected_part);
        panel.sync_drafts(&remote, false);
        assert_eq!(panel.part.as_ref().unwrap().part.color, "#123456", "a removed part keeps its unsaved draft");
        assert_eq!(panel.part.as_ref().unwrap().revision, revision);
    }

    #[test]
    fn remote_preview_selection_follows_once_and_preserves_unsaved_work() {
        let mut panel = AssetUi::new(true);
        panel.follow_preview(Some("BUILTIN/bench"));
        panel.source = "BUILTIN/crate".into();
        panel.follow_preview(Some("BUILTIN/bench"));
        assert_eq!(panel.source, "BUILTIN/crate", "browsing must not be reset each frame");
        panel.follow_preview(Some("BUILTIN/lantern"));
        assert_eq!(panel.source, "BUILTIN/lantern");
        let library = props::library();
        panel.sync_drafts(library.assets.get("BUILTIN/lantern").unwrap(), false);
        panel.part.as_mut().unwrap().dirty = true;
        panel.follow_preview(Some("BUILTIN/bench"));
        assert_eq!(panel.source, "BUILTIN/lantern", "external preview switches must keep unsaved drafts");
    }
}
