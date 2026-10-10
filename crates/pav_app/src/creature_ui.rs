//! Creature authoring uses the same revision-checked tools as an LLM. Drafts are local;
//! accepted source, jobs, history, playback, and frame feedback are shared with agents.

use egui::{Color32, RichText};
use pav_core::RenderFrame;
use pav_core::creature_preview::CreaturePreviewInfo;
use pav_core::creatures;
use serde_json::{Value, json};

use crate::animation_ui::Command;

fn command(tool: &'static str, args: Value) -> Command {
    Command { tool, args: args.as_object().expect("tool args are an object").clone() }
}

fn text(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
fn string(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateFrom {
    Template,
    Theme,
    Blueprint,
}

struct Draft {
    name: String,
    revision: String,
    blueprint: Value,
    text: String,
    dirty: bool,
}

struct NamedDraft {
    path: String,
    text: String,
    dirty: bool,
}

pub struct CreatureUi {
    pub open: bool,
    pub status: String,
    pub error: bool,
    catalog: Value,
    source: String,
    preview_seen: Option<String>,
    search: String,
    new_name: String,
    create_from: CreateFrom,
    template: String,
    theme: String,
    seed: u32,
    quality: String,
    create_json: String,
    constraints: String,
    draft: Option<Draft>,
    named: Option<NamedDraft>,
    pending_job: Option<u64>,
    submitted_draft: Option<String>,
}

impl CreatureUi {
    pub fn new(open: bool) -> Self {
        Self {
            open, status: String::new(), error: false,
            catalog: pav_tools::creature_tools::catalog_snapshot(),
            source: String::new(), preview_seen: None, search: String::new(),
            new_name: "my_creature".into(), create_from: CreateFrom::Template,
            template: "ridgeback_stalker".into(), theme: "beast".into(), seed: 51,
            quality: "medium".into(), create_json: "{\n  \"format\": \"spawnforge/0.2\",\n  \"name\": \"My Creature\",\n  \"extends\": \"quadruped\",\n  \"seed\": 51\n}".into(),
            constraints: "{}".into(), draft: None, named: None, pending_job: None, submitted_draft: None,
        }
    }

    fn dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(|draft| draft.dirty) || self.named.as_ref().is_some_and(|draft| draft.dirty)
    }

    pub fn creation_removed(&self) -> bool {
        pav_tools::creature_tools::authored_snapshot(&self.source)
            .is_some_and(|record| record["revision"] == "absent" && record["redo"].as_u64().unwrap_or(0) > 0)
    }

    fn draft_stamp(&self) -> String {
        json!({
            "source": self.source,
            "draft": self.draft.as_ref().map(|draft| (&draft.name, &draft.revision, &draft.text, draft.dirty)),
            "named": self.named.as_ref().map(|draft| (&draft.path, &draft.text, draft.dirty))
        })
        .to_string()
    }

    /// Only a command from this panel can claim its submitted draft. Watcher and agent
    /// reports cannot clear text that the human has not submitted.
    pub fn report_command(&mut self, result: Result<Value, String>) {
        if let Ok(report) = &result {
            if report["state"] == "queued" {
                self.pending_job = report["job"].as_u64();
                self.submitted_draft = Some(self.draft_stamp());
            }
        }
        self.report(result);
    }

    pub fn report(&mut self, result: Result<Value, String>) {
        match result {
            Err(error) => {
                self.error = true;
                self.status = error;
            }
            Ok(report) => {
                let state = report["state"].as_str().unwrap_or("");
                let name = string(&report, "name");
                self.error = state == "failed" || report.get("error").is_some();
                if state == "queued" {
                    self.status = format!("Build #{} queued for {name}. The last valid preview stays visible.", report["job"]);
                } else if self.error {
                    self.status = format!("Build failed: {}", report["error"].as_str().unwrap_or("see job details"));
                    if report["job"].as_u64() == self.pending_job {
                        self.pending_job = None;
                        self.submitted_draft = None;
                    }
                } else if state == "superseded" {
                    self.status = format!("Build #{} was replaced by a newer request.", report["job"]);
                    if report["job"].as_u64() == self.pending_job {
                        self.pending_job = None;
                        self.submitted_draft = None;
                    }
                } else if state == "published" {
                    if report["job"].as_u64() == self.pending_job {
                        if self.submitted_draft.as_ref() == Some(&self.draft_stamp()) {
                            self.draft = None;
                            self.named = None;
                            self.source = name.clone();
                        }
                        self.pending_job = None;
                        self.submitted_draft = None;
                    }
                    self.status = if report["revision"] == "absent" {
                        format!("Creation of {name} undone. Redo can restore it.")
                    } else {
                        format!("Saved {name}. Build {:.0} ms.", report["build_ms"].as_f64().unwrap_or(0.0))
                    };
                } else if report.get("blueprint").is_some() && !self.dirty() {
                    self.source = name;
                }
                if let Some(error) = report["preview_error"].as_str() {
                    self.error = true;
                    self.status.push_str(&format!(" Preview: {error}"));
                }
            }
        }
    }

    fn sync_source(&mut self, record: &Value) {
        let revision = string(record, "revision");
        if revision == "absent" || record["blueprint"].is_null() {
            if !self.dirty() {
                self.draft = None;
                self.named = None;
            }
            return;
        }
        let changed = self.draft.as_ref().is_none_or(|draft| draft.name != self.source || draft.revision != revision);
        if changed && !self.dirty() {
            let blueprint = record["blueprint"].clone();
            self.draft = Some(Draft { name: self.source.clone(), revision, text: text(&blueprint), blueprint, dirty: false });
            self.named = None;
        }
    }

    pub fn window(
        &mut self,
        ctx: &egui::Context,
        frame: &RenderFrame,
        bridge: Option<&str>,
        feedback: Option<&Value>,
    ) -> Vec<Command> {
        let preview_name = frame.creature_preview.as_ref().map(|p| p.name.clone());
        if self.preview_seen != preview_name {
            if !self.dirty() {
                if let Some(name) = &preview_name {
                    self.source = name.clone();
                }
            }
            self.preview_seen = preview_name;
        }
        if !self.open {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let mut open = self.open;
        let jobs = pav_tools::creature_tools::status_snapshot();
        if let Some(job) = self.pending_job {
            if let Some(report) = pav_tools::creature_tools::job_snapshot(job) {
                if matches!(report["state"].as_str(), Some("published" | "failed" | "superseded")) {
                    self.report(Ok(report));
                }
            }
        }
        egui::Window::new("Creature Studio")
            .id(egui::Id::new("creature_studio"))
            .open(&mut open)
            .default_pos([16.0, 80.0])
            .default_width(400.0)
            .default_height(730.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(RichText::new("CREATE AND DIRECT LIVING ASSETS").strong().color(Color32::from_rgb(124, 213, 244)));
                ui.small("Prompt your LLM or use these controls. Both use the same source and history.");
                if let Some(address) = bridge {
                    ui.small(format!("Live bridge: {address}"));
                }
                if cfg!(target_arch = "wasm32") {
                    ui.colored_label(Color32::from_rgb(255, 192, 110), "Use the native app to compile and save creatures.");
                }
                let pending = jobs["pending"].as_u64().unwrap_or(0);
                if pending > 0 {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("{pending} build request(s) in progress"));
                    });
                }
                egui::ScrollArea::vertical().id_salt("creature_controls").show(ui, |ui| {
                    self.library_controls(ui, &jobs, &mut commands);
                    ui.separator();
                    self.create_controls(ui, &mut commands);
                    if let Some(preview) = &frame.creature_preview {
                        ui.separator();
                        ui.strong(&preview.title);
                        ui.small(format!(
                            "{} bones · {} triangles · {} quality",
                            preview.bones, preview.triangles, preview.quality
                        ));
                        preview_controls(ui, preview, &mut commands);
                        ui.collapsing("Surface and motion notes", |ui| {
                            for warning in &preview.warnings {
                                ui.small(warning);
                            }
                        });
                    } else {
                        ui.separator();
                        ui.label("Create a creature or open a saved source to fill this stage.");
                    }
                    if let Some(record) = pav_tools::creature_tools::authored_snapshot(&self.source) {
                        self.sync_source(&record);
                        ui.separator();
                        ui.strong(format!("Editing {}", self.source));
                        if let Some(draft) = &self.draft {
                            ui.small(format!("Source revision {}", draft.revision));
                            if draft.revision != record["revision"].as_str().unwrap_or_default() {
                                ui.colored_label(
                                    Color32::from_rgb(255, 192, 110),
                                    "The source changed. Your draft is kept. Discard it to load the new revision.",
                                );
                            }
                        }
                        if record["compiled"] == true && record["source_revision"] != record["revision"] {
                            ui.colored_label(Color32::from_rgb(255, 192, 110),
                                "This source is newer than the preview. The last valid build stays visible while it builds or needs a fix.");
                        }
                        let revision = string(&record, "revision");
                        ui.horizontal(|ui| {
                            for (label, action, count) in [("Undo", "undo", "undo"), ("Redo", "redo", "redo")] {
                                if ui
                                    .add_enabled(
                                        !self.dirty() && record[count].as_u64().unwrap_or(0) > 0,
                                        egui::Button::new(label),
                                    )
                                    .clicked()
                                {
                                    commands.push(command(
                                        "creature_edit",
                                        json!({"action":action,"name":self.source,"if_revision":revision}),
                                    ));
                                }
                            }
                            if ui.add_enabled(!self.dirty() && revision != "absent", egui::Button::new("Rebuild")).clicked() {
                                commands.push(command(
                                    "creature_edit",
                                    json!({"action":"rebuild","name":self.source,"if_revision":revision}),
                                ));
                            }
                            if self.dirty() && ui.button("Discard draft").clicked() {
                                self.draft = None;
                                self.named = None;
                            }
                        });
                        if revision == "absent" {
                            ui.small("Creation is undone. Use Redo to restore this creature.");
                        }
                        self.named_controls(ui, &mut commands);
                        self.blueprint_controls(ui, &mut commands);
                        ui.small("Each accepted edit saves. Failed or superseded builds keep the last valid creature.");
                    }
                    if !self.status.is_empty() {
                        ui.separator();
                        ui.colored_label(
                            if self.error { Color32::from_rgb(255, 151, 126) } else { Color32::from_rgb(138, 227, 198) },
                            &self.status,
                        );
                    }
                    if let Some(feedback) = feedback {
                        ui.separator();
                        let state = feedback["state"].as_str().unwrap_or("idle");
                        ui.small(format!("Latest frame: {state} · ticket {}", feedback["ticket"]));
                        if let Some(ms) = feedback["submitted_ms"].as_f64() {
                            ui.small(format!("{ms:.0} ms from tool start to native frame submission"));
                        }
                    }
                    ui.collapsing("Build jobs", |ui| {
                        if let Some(jobs) = jobs["jobs"].as_array() {
                            for job in jobs.iter().take(8) {
                                ui.label(format!(
                                    "#{} {}: {}",
                                    job["job"],
                                    job["name"].as_str().unwrap_or(""),
                                    job["state"].as_str().unwrap_or("")
                                ));
                                if let Some(error) = job["error"].as_str() {
                                    ui.colored_label(Color32::from_rgb(255, 151, 126), error);
                                }
                                if let Some(issues) = job["issues"].as_array() {
                                    for issue in issues.iter().take(4) {
                                        ui.small(format!(
                                            "{}: {}",
                                            issue["path"].as_str().unwrap_or(""),
                                            issue["message"].as_str().unwrap_or("")
                                        ));
                                    }
                                }
                            }
                        }
                    });
                });
            });
        self.open = open;
        commands
    }

    fn library_controls(&mut self, ui: &mut egui::Ui, jobs: &Value, commands: &mut Vec<Command>) {
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.text_edit_singleline(&mut self.search);
        });
        let search = self.search.to_lowercase();
        let library = creatures::library();
        let mut names: Vec<String> = library.assets.keys().cloned().collect();
        if let Some(assets) = jobs["assets"].as_object() {
            for name in assets.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        if !self.source.is_empty() && !names.contains(&self.source) {
            names.push(self.source.clone());
        }
        names.sort();
        egui::ScrollArea::vertical().id_salt("creature_library").max_height(105.0).show(ui, |ui| {
            for name in names.iter().filter(|name| name.to_lowercase().contains(&search)) {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.dirty() || self.source == *name, egui::Button::selectable(self.source == *name, name))
                        .clicked()
                        && self.source != *name
                    {
                        self.source = name.clone();
                        self.draft = None;
                        self.named = None;
                    }
                    if ui.add_enabled(library.assets.contains_key(name), egui::Button::new("View")).clicked() {
                        commands.push(command("creature_preview", json!({"name":name})));
                    }
                });
            }
        });
    }

    fn create_controls(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        egui::CollapsingHeader::new("Create a creature").default_open(self.source.is_empty()).show(ui, |ui| {
            ui.horizontal(|ui| { ui.label("Save as"); ui.text_edit_singleline(&mut self.new_name); });
            ui.small("Use lowercase letters, numbers, underscores, or hyphens.");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.create_from, CreateFrom::Template, "Template");
                ui.selectable_value(&mut self.create_from, CreateFrom::Theme, "Theme");
                ui.selectable_value(&mut self.create_from, CreateFrom::Blueprint, "Blueprint JSON");
            });
            match self.create_from {
                CreateFrom::Template => {
                    egui::ComboBox::from_id_salt("creature_template").selected_text(&self.template).show_ui(ui, |ui| {
                        if let Some(templates) = self.catalog["templates"].as_array() {
                            for template in templates {
                                if let Some(id) = template["id"].as_str() { ui.selectable_value(&mut self.template, id.into(), template["name"].as_str().unwrap_or(id)); }
                            }
                        }
                    });
                }
                CreateFrom::Theme => {
                    egui::ComboBox::from_id_salt("creature_theme").selected_text(&self.theme).show_ui(ui, |ui| {
                        if let Some(modules) = self.catalog["modules"].as_array() {
                            for module in modules.iter().filter(|module| module["kind"] == "theme") {
                                if let Some(id) = module["id"].as_str() { ui.selectable_value(&mut self.theme, id.into(), id); }
                            }
                        }
                    });
                    ui.horizontal(|ui| { ui.label("Seed"); ui.add(egui::DragValue::new(&mut self.seed)); });
                    ui.collapsing("Theme constraints", |ui| { ui.small("Example: {\"bodyPlan\":\"quadruped\"}"); ui.text_edit_multiline(&mut self.constraints); });
                }
                CreateFrom::Blueprint => { ui.add(egui::TextEdit::multiline(&mut self.create_json).code_editor().desired_rows(9)); }
            }
            ui.horizontal(|ui| {
                ui.label("Quality");
                for quality in ["low", "medium", "high"] { ui.selectable_value(&mut self.quality, quality.into(), quality); }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.dirty() && !cfg!(target_arch = "wasm32"), egui::Button::new("Create and view")).clicked() {
                    let mut args = json!({"action":"create","name":self.new_name,"quality":self.quality,"if_revision":"absent"});
                    let result = match self.create_from {
                        CreateFrom::Template => { args["template"] = json!(self.template); Ok(()) }
                        CreateFrom::Theme => serde_json::from_str::<Value>(&self.constraints).map(|constraints| {
                            args["theme"] = json!(self.theme); args["seed"] = json!(self.seed); args["constraints"] = constraints;
                        }),
                        CreateFrom::Blueprint => serde_json::from_str::<Value>(&self.create_json).map(|blueprint| { args["blueprint"] = blueprint; }),
                    };
                    match result {
                        Ok(()) => commands.push(command("creature_edit", args)),
                        Err(error) => self.report(Err(format!("Cannot read the creation JSON: {error}"))),
                    }
                }
                if ui.add_enabled(!self.source.is_empty() && !self.dirty() && !cfg!(target_arch = "wasm32"), egui::Button::new("Copy selected")).clicked() {
                    commands.push(command("creature_edit", json!({"action":"copy","from":self.source,"name":self.new_name,"quality":self.quality,"if_revision":"absent"})));
                }
            });
        });
    }

    fn named_controls(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let Some(draft) = &self.draft else { return };
        let nodes = named_nodes(&draft.blueprint);
        if nodes.is_empty() {
            return;
        }
        if self.named.is_none() {
            self.named = Some(NamedDraft { path: nodes[0].0.clone(), text: text(&nodes[0].1), dirty: false });
        }
        ui.collapsing("Named parts and surfaces", |ui| {
            let named = self.named.as_mut().unwrap();
            egui::ComboBox::from_id_salt("creature_named_part").selected_text(&named.path).width(330.0).show_ui(ui, |ui| {
                for (path, value) in &nodes {
                    if ui
                        .add_enabled(!named.dirty || named.path == *path, egui::Button::selectable(named.path == *path, path))
                        .clicked()
                        && named.path != *path
                    {
                        named.path = path.clone();
                        named.text = text(value);
                        named.dirty = false;
                    }
                }
            });
            ui.small("Edit this named node. Apply related changes in one blueprint update below.");
            if ui.add_enabled(!draft.dirty, egui::TextEdit::multiline(&mut named.text).code_editor().desired_rows(8)).changed() {
                named.dirty = true;
            }
            if ui.add_enabled(named.dirty && !draft.dirty, egui::Button::new("Apply named edit")).clicked() {
                match serde_json::from_str::<Value>(&named.text) {
                    Ok(value) => {
                        let before = nodes.iter().find(|(path, _)| *path == named.path).map(|(_, value)| value).unwrap();
                        match named_ops(&named.path, before, &value) {
                            Ok(ops) if ops.is_empty() => {
                                self.status = "No fields changed.".into();
                            }
                            Ok(ops) => commands.push(command(
                                "creature_edit",
                                json!({
                                    "action":"patch","name":draft.name,"if_revision":draft.revision,"ops":ops
                                }),
                            )),
                            Err(error) => {
                                self.error = true;
                                self.status = error;
                            }
                        }
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = format!("Cannot read named edit JSON: {error}");
                    }
                }
            }
        });
    }

    fn blueprint_controls(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        let named_dirty = self.named.as_ref().is_some_and(|named| named.dirty);
        let Some(draft) = &mut self.draft else { return };
        ui.collapsing("Whole blueprint", |ui| {
            if ui.add_enabled(!named_dirty, egui::TextEdit::multiline(&mut draft.text).code_editor().desired_rows(14)).changed() {
                draft.dirty = true;
            }
            if ui.add_enabled(draft.dirty && !named_dirty, egui::Button::new("Apply blueprint")).clicked() {
                match serde_json::from_str::<Value>(&draft.text) {
                    Ok(blueprint) => commands.push(command(
                        "creature_edit",
                        json!({"action":"replace","name":draft.name,"if_revision":draft.revision,"blueprint":blueprint}),
                    )),
                    Err(error) => {
                        self.error = true;
                        self.status = format!("Cannot read blueprint JSON: {error}");
                    }
                }
            }
        });
    }
}

/// Upstream ID selectors address fields, not replacement list items. Keep the stable ID
/// and send changed top-level fields together; validation runs after the entire batch.
fn named_ops(path: &str, before: &Value, after: &Value) -> Result<Vec<Value>, String> {
    if !path.contains("[id=") || !path.ends_with(']') {
        return Ok(vec![json!({"op":"set","path":path,"value":after})]);
    }
    let before = before.as_object().ok_or("The selected node must be an object")?;
    let after = after.as_object().ok_or("The edited node must be an object")?;
    if before.get("id") != after.get("id") {
        return Err("Keep the selected node's id. Use the whole blueprint to rename it.".into());
    }
    let mut ops = Vec::new();
    for (key, value) in after {
        if key != "id" && before.get(key) != Some(value) {
            ops.push(json!({"op":"set","path":format!("{path}.{key}"),"value":value}));
        }
    }
    for key in before.keys() {
        if key != "id" && !after.contains_key(key) {
            ops.push(json!({"op":"remove","path":format!("{path}.{key}")}));
        }
    }
    Ok(ops)
}

fn named_nodes(blueprint: &Value) -> Vec<(String, Value)> {
    let mut nodes = Vec::new();
    for section in ["torso", "neck", "head", "tail"] {
        if let Some(value) = blueprint["body"].get(section) {
            nodes.push((format!("body.{section}"), value.clone()));
        }
    }
    for list in ["limbs", "parts"] {
        if let Some(items) = blueprint[list].as_array() {
            for item in items {
                if let Some(id) = item["id"].as_str() {
                    nodes.push((format!("{list}[id={id}]"), item.clone()));
                }
            }
        }
    }
    if let Some(palette) = blueprint["skin"].get("palette") {
        nodes.push(("skin.palette".into(), palette.clone()));
    }
    if let Some(layers) = blueprint["skin"]["layers"].as_array() {
        for (index, layer) in layers.iter().enumerate() {
            let path = if let Some(id) = layer["id"].as_str() {
                format!("skin.layers[id={id}]")
            } else if let Some(kind) =
                layer["type"].as_str().filter(|kind| layers.iter().filter(|other| other["type"] == *kind).count() == 1)
            {
                format!("skin.layers[type={kind}]")
            } else {
                format!("skin.layers[{index}]")
            };
            nodes.push((path, layer.clone()));
        }
    }
    nodes
}

fn preview_controls(ui: &mut egui::Ui, preview: &CreaturePreviewInfo, commands: &mut Vec<Command>) {
    let mut args = serde_json::Map::new();
    ui.horizontal(|ui| {
        if ui.button(if preview.playing { "Pause" } else { "Play" }).clicked() {
            args.insert("playing".into(), json!(!preview.playing));
        }
        if ui.button("Restart").clicked() {
            args.insert("time".into(), json!(0.0));
            args.insert("playing".into(), json!(true));
        }
        if ui.button("Step").clicked() {
            args.insert("step".into(), json!(1));
        }
        if ui.button("Fit").clicked() {
            args.insert("action".into(), json!("fit"));
        }
    });
    let mut clip = preview.clip.clone();
    egui::ComboBox::from_id_salt("creature_clip").selected_text(&clip).show_ui(ui, |ui| {
        for name in &preview.clips {
            ui.selectable_value(&mut clip, name.clone(), name);
        }
    });
    if clip != preview.clip {
        args.insert("clip".into(), json!(clip));
    }
    let mut time = preview.time;
    // Painting must not round an agent's fractional playhead and write it back.
    // Only human input can turn this display value into a preview command.
    if preview.duration > 0.0
        && ui
            .add(
                egui::Slider::new(&mut time, 0.0..=preview.duration)
                    .clamping(egui::SliderClamping::Edits)
                    .text("seconds")
                    .fixed_decimals(3),
            )
            .changed()
    {
        args.insert("time".into(), json!(time));
    }
    ui.horizontal(|ui| {
        let mut looping = preview.looping;
        let mut turntable = preview.turntable;
        if ui.checkbox(&mut looping, "Loop").changed() {
            args.insert("looping".into(), json!(looping));
        }
        if ui.checkbox(&mut turntable, "Turntable").changed() {
            args.insert("turntable".into(), json!(turntable));
        }
    });
    let mut speed = preview.speed;
    if ui
        .add(
            egui::Slider::new(&mut speed, 0.05..=8.0)
                .clamping(egui::SliderClamping::Edits)
                .logarithmic(true)
                .text("speed")
                .suffix("x"),
        )
        .changed()
    {
        args.insert("speed".into(), json!(speed));
    }
    ui.small("F6 play/pause · F7 step · F8/F9 speed. Right-drag to orbit.");
    if !args.is_empty() {
        commands.push(Command { tool: "creature_preview", args });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painting_controls_never_changes_an_agent_playhead_or_speed() {
        let mut preview = CreaturePreviewInfo {
            name: "CREATURE/paint".into(),
            revision: "compiled".into(),
            source_revision: "source".into(),
            available: true,
            title: "Paint".into(),
            quality: "low".into(),
            bones: 1,
            vertices: 3,
            triangles: 1,
            bounds: creatures::Bounds { min: glam::Vec3::ZERO, max: glam::Vec3::ONE },
            clip: "walk".into(),
            clips: vec!["rest".into(), "walk".into()],
            duration: 0.358_333_32,
            time: 0.0,
            playing: false,
            speed: 8.0,
            looping: true,
            turntable: false,
            yaw: 0.0,
            scale: 1.0,
            warnings: Vec::new(),
        };
        let ctx = egui::Context::default();
        for playing in [true, false] {
            preview.playing = playing;
            for time in [1.0 / 60.0, 0.215_000_02, 0.318_333_3] {
                preview.time = time;
                let mut commands = Vec::new();
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(raw, |ui| preview_controls(ui, &preview, &mut commands));
                output.textures_delta.clear();
                assert!(commands.is_empty(), "displaying time {time} at speed 8 must not emit a tool command");
            }
        }
    }

    fn draft_ui() -> CreatureUi {
        let mut ui = CreatureUi::new(true);
        ui.source = "CREATURE/draft".into();
        ui.draft = Some(Draft {
            name: ui.source.clone(),
            revision: "source-a".into(),
            blueprint: json!({"name":"Draft"}),
            text: "{\"name\":\"Submitted\"}".into(),
            dirty: true,
        });
        ui
    }

    #[test]
    fn published_build_keeps_text_typed_after_submission() {
        let mut ui = draft_ui();
        ui.report_command(Ok(json!({"job":1,"name":"CREATURE/draft","state":"queued"})));
        ui.draft.as_mut().unwrap().text = "{\"name\":\"New direction\"}".into();
        ui.report(Ok(json!({"job":1,"name":"CREATURE/draft","state":"published","revision":"source-b"})));
        assert_eq!(ui.draft.as_ref().unwrap().text, "{\"name\":\"New direction\"}");
        assert!(ui.dirty());
    }

    #[test]
    fn background_build_cannot_claim_or_discard_a_manual_draft() {
        let mut ui = draft_ui();
        let before = ui.draft_stamp();
        ui.report(Ok(json!({"job":7,"name":"CREATURE/external","state":"queued"})));
        ui.report(Ok(json!({"job":7,"name":"CREATURE/external","state":"published","revision":"source-b"})));
        assert_eq!(ui.draft_stamp(), before);
        assert!(ui.pending_job.is_none());
    }

    #[test]
    fn successful_panel_build_releases_only_the_submitted_draft() {
        let mut ui = draft_ui();
        ui.report_command(Ok(json!({"job":3,"name":"CREATURE/draft","state":"queued"})));
        ui.report(Ok(json!({"job":3,"name":"CREATURE/draft","state":"published","revision":"source-b"})));
        assert!(ui.draft.is_none());
        assert!(!ui.dirty());
        assert_eq!(ui.source, "CREATURE/draft");
    }
}
