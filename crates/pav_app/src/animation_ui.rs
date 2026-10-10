//! Human controls for the same animation tools that agents use.

use egui::{Color32, RichText};
use pav_core::animation_preview::PreviewInfo;
use pav_core::clips;
use pav_tools::tools::Args;
use serde_json::{Value, json};

pub struct Command {
    pub tool: &'static str,
    pub args: Args,
}

impl Command {
    fn preview(args: Value) -> Self {
        Self { tool: "anim_preview", args: args.as_object().unwrap().clone() }
    }

    fn edit(args: Value) -> Self {
        Self { tool: "anim_edit", args: args.as_object().unwrap().clone() }
    }
}

pub struct AnimationUi {
    pub open: bool,
    pub status: String,
    pub error: bool,
    search: String,
    source: String,
    move_name: String,
    name: String,
    duration: f32,
    pose: String,
    retime: f32,
    removed_edit: Option<(String, String)>,
}

impl Default for AnimationUi {
    fn default() -> Self {
        Self {
            open: false,
            status: String::new(),
            error: false,
            search: String::new(),
            source: "QUATERNIUS/Idle_Loop".into(),
            move_name: "roundhouse".into(),
            name: "NewAnimation".into(),
            duration: 2.0,
            pose: "{\"armR\": [80, 20, 50, 25, 0]}".into(),
            retime: 1.0,
            removed_edit: None,
        }
    }
}

impl AnimationUi {
    pub fn new(open: bool) -> Self {
        Self { open, ..Self::default() }
    }

    pub fn report(&mut self, result: Result<Value, String>) {
        match result {
            Ok(v) => {
                self.error = false;
                if v.get("editable").is_some() {
                    self.removed_edit = if v["clip"].is_null() && v["redo"].as_u64().unwrap_or(0) > 0 {
                        v["name"].as_str().zip(v["revision"].as_str()).map(|(name, revision)| (name.into(), revision.into()))
                    } else {
                        None
                    };
                }
                if let Some(path) = v.get("file").or_else(|| v.get("path")).and_then(Value::as_str) {
                    self.status = format!("Saved: {path}");
                } else {
                    self.status.clear();
                }
            }
            Err(e) => {
                self.status = e;
                self.error = true;
            }
        }
    }

    pub fn window(&mut self, ctx: &egui::Context, preview: Option<&PreviewInfo>, bridge: Option<&str>) -> Vec<Command> {
        if !self.open && preview.is_none() {
            return Vec::new();
        }
        self.open = true;
        let mut commands = Vec::new();
        egui::Window::new("Animation Studio")
            .id(egui::Id::new("animation_studio"))
            .default_pos([16.0, 44.0])
            .default_width(350.0)
            .default_height(700.0)
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.label(RichText::new("LIVE ANIMATION WORKSPACE").strong().color(Color32::from_rgb(100, 220, 186)));
                ui.label("Keep this window open while you prompt your LLM.");
                match bridge {
                    Some(addr) => {
                        ui.small(format!("Live bridge: {addr}"));
                    }
                    None => {
                        ui.small("Local preview. Start with --animation-studio for live MCP access.");
                    }
                }
                ui.separator();

                egui::ScrollArea::vertical().id_salt("animation_controls").show(ui, |ui| {
                    self.library_controls(ui, &mut commands);
                    if let Some((name, revision)) = &self.removed_edit {
                        ui.separator();
                        ui.label(format!("Undo removed {name}."));
                        if ui.button("Redo removed animation").clicked() {
                            commands.push(Command::edit(json!({"action":"redo", "name":name, "if_revision":revision})));
                        }
                    }
                    if let Some(p) = preview {
                        ui.separator();
                        ui.label(RichText::new(&p.name).strong());
                        if !p.available {
                            ui.colored_label(
                                Color32::from_rgb(250, 150, 100),
                                "The selected animation is unavailable. Select another animation.",
                            );
                        }
                        ui.small(format!("{} · {:.3} s · {} key poses", p.kind, p.duration, p.key_times.len()));
                        playback(ui, p, &mut commands);

                        if let Some(name) = &p.clip {
                            let lib = clips::library();
                            if let Some(clip) = lib.find(name).and_then(|id| lib.get(id)) {
                                if !clip.desc.is_empty() {
                                    ui.label(&clip.desc);
                                }
                                if let Some((set_name, _)) = name.split_once('/') {
                                    if let Some(set) = lib.sets.iter().find(|set| set.set == set_name) {
                                        if !set.credit.is_empty() {
                                            ui.collapsing("Source credits", |ui| {
                                                ui.label(&set.credit);
                                            });
                                        }
                                    }
                                }
                                let editable = name.starts_with("WORKSHOP/") || name.starts_with("WORKSHOP_LOCAL/");
                                if editable && cfg!(target_arch = "wasm32") {
                                    ui.small("Use the desktop studio to edit this saved animation.");
                                } else if editable {
                                    let revision = pav_core::animation_edit::revision(clip);
                                    self.edit_controls(ui, p, &revision, &mut commands);
                                } else {
                                    ui.small("Copy this animation to create an editable version.");
                                }
                            }
                        } else {
                            ui.small("Procedural moves can be viewed here. Create or copy a motion clip to edit key poses.");
                        }
                        ui.separator();
                        if ui.button("Close preview and resume game").clicked() {
                            commands.push(Command::preview(json!({"close": true})));
                            self.open = false;
                        }
                    }
                    if !self.status.is_empty() {
                        ui.separator();
                        let color = if self.error { Color32::from_rgb(255, 145, 120) } else { Color32::from_rgb(130, 225, 190) };
                        ui.colored_label(color, &self.status);
                    }
                });
            });
        commands
    }

    fn library_controls(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        ui.label(RichText::new("Animation library").strong());
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search clips"));
        let search = self.search.to_lowercase();
        let library = clips::library();
        egui::ComboBox::from_id_salt("animation_source").selected_text(&self.source).width(310.0).show_ui(ui, |ui| {
            for set in &library.sets {
                for (key, clip) in &set.clips {
                    let name = format!("{}/{key}", set.set);
                    if search.is_empty()
                        || name.to_lowercase().contains(&search)
                        || clip.desc.to_lowercase().contains(&search)
                        || clip.tags.iter().any(|t| t.to_lowercase().contains(&search))
                    {
                        ui.selectable_value(&mut self.source, name.clone(), name);
                    }
                }
            }
        });
        if ui.button("View selected clip").clicked() {
            commands.push(Command::preview(json!({"clip": self.source})));
        }
        ui.collapsing("Procedural moves", |ui| {
            let table = pav_core::moves::table();
            egui::ComboBox::from_id_salt("animation_move").selected_text(&self.move_name).show_ui(ui, |ui| {
                for name in table.names() {
                    ui.selectable_value(&mut self.move_name, name.to_string(), name);
                }
            });
            if ui.button("View selected move").clicked() {
                commands.push(Command::preview(json!({"move":self.move_name})));
            }
        });
        if cfg!(target_arch = "wasm32") {
            ui.small("Use the desktop studio to save animation files and connect an LLM.");
        } else {
            ui.add_space(6.0);
            ui.label("New animation name");
            ui.add(egui::TextEdit::singleline(&mut self.name).hint_text("Wave"));
            ui.horizontal(|ui| {
                ui.label("Duration");
                ui.add(egui::DragValue::new(&mut self.duration).range(0.05..=600.0).speed(0.05).suffix(" s"));
            });
            ui.horizontal(|ui| {
                if ui.button("Create from rest pose").clicked() {
                    commands
                        .push(Command::edit(json!({"action":"create", "name":self.name, "duration":self.duration, "loop":true})));
                }
                if ui.button("Copy selected clip").clicked() {
                    commands.push(Command::edit(json!({"action":"copy", "name":self.name, "from":self.source})));
                }
            });
        }
    }

    fn edit_controls(&mut self, ui: &mut egui::Ui, p: &PreviewInfo, revision: &str, commands: &mut Vec<Command>) {
        ui.separator();
        ui.label(RichText::new("Edit key poses").strong());
        ui.small("Each accepted edit saves automatically. Undo restores the previous edit.");
        ui.horizontal(|ui| {
            for (label, action) in [("Undo", "undo"), ("Redo", "redo"), ("Mirror clip", "mirror")] {
                if ui.button(label).clicked() {
                    commands.push(Command::edit(json!({"action":action, "name":p.name, "if_revision":revision})));
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Insert key at cursor").clicked() {
                commands.push(Command::edit(
                    json!({"action":"key", "name":p.name, "time":p.time, "pose":{}, "if_revision":revision}),
                ));
            }
            if ui.button("Delete key at cursor").clicked() {
                commands
                    .push(Command::edit(json!({"action":"delete_key", "name":p.name, "time":p.time, "if_revision":revision})));
            }
        });
        ui.collapsing("Pose channels", |ui| {
            ui.small("Pause and set the time. Enter the channels to change.");
            ui.add(
                egui::TextEdit::multiline(&mut self.pose)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Apply pose at cursor").clicked() {
                match serde_json::from_str::<Value>(&self.pose) {
                    Ok(pose) if pose.is_object() => commands.push(Command::edit(
                        json!({"action":"key", "name":p.name, "time":p.time, "pose":pose, "if_revision":revision}),
                    )),
                    Ok(_) => {
                        self.error = true;
                        self.status = "Pose channels must be a JSON object.".into();
                    }
                    Err(e) => {
                        self.error = true;
                        self.status = format!("Invalid pose JSON: {e}");
                    }
                }
            }
            ui.small("Arms and legs: [forward, out, up, bend, twist]. Angles use degrees.");
        });
        ui.horizontal(|ui| {
            ui.label("Duration factor");
            ui.add(egui::DragValue::new(&mut self.retime).range(0.05..=20.0).speed(0.05));
            if ui.button("Retime").clicked() {
                commands
                    .push(Command::edit(json!({"action":"retime", "name":p.name, "factor":self.retime, "if_revision":revision})));
            }
        });
    }
}

fn playback(ui: &mut egui::Ui, p: &PreviewInfo, commands: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        if ui.button(if p.playing { "Pause" } else { "Play" }).clicked() {
            commands.push(Command::preview(json!({"playing":!p.playing})));
        }
        if ui.button("Restart").clicked() {
            commands.push(Command::preview(json!({"action":"restart"})));
        }
        if ui.button("-1 frame").clicked() {
            commands.push(Command::preview(json!({"step":-1})));
        }
        if ui.button("+1 frame").clicked() {
            commands.push(Command::preview(json!({"step":1})));
        }
    });
    let mut time = p.time;
    if ui.add(egui::Slider::new(&mut time, 0.0..=p.duration.max(0.001)).text("seconds").fixed_decimals(3)).changed() {
        commands.push(Command::preview(json!({"time":time, "playing":false})));
    }
    ui.horizontal(|ui| {
        if ui.button("Previous key").clicked() {
            let time = p.key_times.iter().rev().find(|t| **t < p.time - 0.0005).copied().unwrap_or(0.0);
            commands.push(Command::preview(json!({"time":time, "playing":false})));
        }
        if ui.button("Next key").clicked() {
            let time = p.key_times.iter().find(|t| **t > p.time + 0.0005).copied().unwrap_or(p.duration);
            commands.push(Command::preview(json!({"time":time, "playing":false})));
        }
    });
    let mut speed = p.speed;
    if ui.add(egui::Slider::new(&mut speed, 0.05..=4.0).logarithmic(true).text("speed").suffix("x")).changed() {
        commands.push(Command::preview(json!({"speed":speed})));
    }
    ui.horizontal_wrapped(|ui| {
        for (label, name, value) in [
            ("Repeat", "repeat", p.repeat),
            ("Mirror", "mirror", p.mirror),
            ("Upper body", "upper", p.upper),
            ("Root motion", "travel", p.travel),
        ] {
            let mut v = value;
            if ui.checkbox(&mut v, label).changed() {
                let mut args = Args::new();
                args.insert(name.into(), json!(v));
                commands.push(Command { tool: "anim_preview", args });
            }
        }
    });
    ui.small("Rotate: right-drag. Zoom: mouse wheel. F12: screenshot.");
    if ui.button("Fit camera to animation").clicked() {
        commands.push(Command::preview(json!({"action":"fit"})));
    }
}
