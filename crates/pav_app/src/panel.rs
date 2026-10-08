//! The tuning panel (F1): every registered parameter grouped by path, presets, time controls,
//! the rewind timeline and frame/tick timing graphs.

use std::collections::VecDeque;
use std::path::PathBuf;

use egui::{Color32, RichText};
use pav_core::params::{self, ParamInfo, ParamKind, ParamValue, Tunable};

use crate::simhost::{SimStats, TimeControl};

pub enum PanelAction {
    Reset,
    Scrub(u64),
    SaveSnapshot,
    LoadSnapshot,
    SaveReplay,
    Status(String),
    /// Open the Look & Filters window.
    Look,
}

pub struct Panel {
    pub open: bool,
    filter: String,
    preset_name: String,
    presets: Vec<String>,
    pub frame_ms: VecDeque<f32>,
    pub tick_ms: VecDeque<f32>,
    status: String,
    scrub_value: u64,
}

pub fn presets_dir() -> PathBuf {
    crate::boot::exe_dir().join("presets")
}

impl Panel {
    pub fn new() -> Self {
        let mut p = Self {
            open: false,
            filter: String::new(),
            preset_name: "my preset".into(),
            presets: Vec::new(),
            frame_ms: VecDeque::new(),
            tick_ms: VecDeque::new(),
            status: String::new(),
            scrub_value: 0,
        };
        p.refresh_presets();
        p
    }

    fn refresh_presets(&mut self) {
        self.presets = std::fs::read_dir(presets_dir())
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        self.presets.sort();
    }

    pub fn record_timing(&mut self, frame_ms: f32, tick_ms: f32) {
        for (q, v) in [(&mut self.frame_ms, frame_ms), (&mut self.tick_ms, tick_ms)] {
            q.push_back(v);
            while q.len() > 240 {
                q.pop_front();
            }
        }
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    /// Draws the panel. Returns actions for the app to carry out.
    pub fn ui(
        &mut self,
        root: &mut egui::Ui,
        params: &mut dyn Tunable,
        ctl: &mut TimeControl,
        stats: &SimStats,
    ) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        egui::Panel::right("tuning").resizable(true).default_size(360.0).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tuning");
                ui.label(RichText::new("F1").weak());
                if ui
                    .button("Look & filters…")
                    .on_hover_text("Filters on the whole scene, the characters & objects or the environment, with presets")
                    .clicked()
                {
                    actions.push(PanelAction::Look);
                }
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.time_section(ui, ctl, stats, &mut actions);
                ui.separator();
                self.preset_section(ui, params, &mut actions);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Filter");
                    ui.text_edit_singleline(&mut self.filter);
                });
                self.params_section(ui, params);
                ui.separator();
                self.timing_section(ui);
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(RichText::new(&self.status).small());
                }
            });
        });
        actions
    }

    fn time_section(&mut self, ui: &mut egui::Ui, ctl: &mut TimeControl, stats: &SimStats, actions: &mut Vec<PanelAction>) {
        egui::CollapsingHeader::new("Time").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(if ctl.paused { "▶ Play (F6)" } else { "⏸ Pause (F6)" }).clicked() {
                    ctl.paused = !ctl.paused;
                }
                if ui.button("Step (F7)").clicked() {
                    ctl.paused = true;
                    ctl.step_requests += 1;
                }
                if ui.button("Reset (F5)").clicked() {
                    actions.push(PanelAction::Reset);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Speed");
                ui.add(egui::Slider::new(&mut ctl.speed, 0.05..=8.0).logarithmic(true).suffix("×"));
            });
            ui.horizontal(|ui| {
                ui.label("Rewind speed");
                ui.add(egui::Slider::new(&mut ctl.rewind_speed, 1..=8).suffix(" ticks/tick"));
            });
            let (lo, hi) = (stats.oldest, stats.newest.max(stats.oldest));
            if ui.ctx().dragged_id().is_none() {
                self.scrub_value = stats.tick.clamp(lo, hi);
            }
            ui.label(format!(
                "Timeline: ticks {lo}–{hi} ({:.1} s, {:.0} MB){}",
                (hi - lo) as f32 / 60.0,
                stats.history_mb,
                if stats.rewound { " · rewound (play to branch)" } else { "" }
            ));
            let r = ui.add(egui::Slider::new(&mut self.scrub_value, lo..=hi.max(lo + 1)).text("tick"));
            if r.changed() {
                ctl.paused = true;
                actions.push(PanelAction::Scrub(self.scrub_value));
            }
            ui.label(
                RichText::new("Hold Backspace (gamepad: Back) to rewind; acting afterwards starts a new timeline.")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("Save snapshot").clicked() {
                    actions.push(PanelAction::SaveSnapshot);
                }
                if ui.button("Load snapshot").clicked() {
                    actions.push(PanelAction::LoadSnapshot);
                }
                if ui.button("Save replay").clicked() {
                    actions.push(PanelAction::SaveReplay);
                }
            });
        });
    }

    fn preset_section(&mut self, ui: &mut egui::Ui, params: &mut dyn Tunable, actions: &mut Vec<PanelAction>) {
        egui::CollapsingHeader::new("Presets").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.preset_name);
                if ui.button("Save").clicked() {
                    let map = params::to_map(params);
                    let dir = presets_dir();
                    let name: String = self
                        .preset_name
                        .chars()
                        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
                        .collect();
                    let path = dir.join(format!("{name}.json"));
                    let r = std::fs::create_dir_all(&dir)
                        .and_then(|_| std::fs::write(&path, serde_json::to_string_pretty(&map).unwrap_or_default()));
                    actions.push(PanelAction::Status(match r {
                        Ok(_) => format!("saved {}", path.display()),
                        Err(e) => format!("could not save preset: {e}"),
                    }));
                    self.refresh_presets();
                }
            });
            let mut load = None;
            for p in &self.presets {
                ui.horizontal(|ui| {
                    ui.label(p);
                    if ui.small_button("Load").clicked() {
                        load = Some(p.clone());
                    }
                });
            }
            if let Some(name) = load {
                let path = presets_dir().join(format!("{name}.json"));
                match std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()) {
                    Some(map) => {
                        let unknown = params::apply_map(params, &map);
                        actions.push(PanelAction::Status(format!("loaded '{name}' ({} unknown keys)", unknown.len())));
                    }
                    None => actions.push(PanelAction::Status(format!("could not read {}", path.display()))),
                }
            }
            ui.label(RichText::new(format!("Files in {} (share them to export/import)", presets_dir().display())).small().weak());
        });
    }

    fn params_section(&mut self, ui: &mut egui::Ui, params: &mut dyn Tunable) {
        let list = params::list(params);
        let filter = self.filter.to_lowercase();
        let mut changes: Vec<(String, ParamValue)> = Vec::new();
        let mut groups: Vec<(String, Vec<&ParamInfo>)> = Vec::new();
        for p in list.iter().filter(|p| filter.is_empty() || p.path.to_lowercase().contains(&filter)) {
            let top = p.path.split('.').next().unwrap_or("").to_string();
            match groups.iter_mut().find(|g| g.0 == top) {
                Some(g) => g.1.push(p),
                None => groups.push((top, vec![p])),
            }
        }
        for (group, items) in groups {
            egui::CollapsingHeader::new(RichText::new(&group).strong()).default_open(!filter.is_empty()).show(ui, |ui| {
                let mut sub: Vec<(String, Vec<&ParamInfo>)> = Vec::new();
                for p in items {
                    let rest = &p.path[group.len() + 1..];
                    let key = match rest.split_once('.') {
                        Some((s, _)) => s.to_string(),
                        None => String::new(),
                    };
                    match sub.iter_mut().find(|g| g.0 == key) {
                        Some(g) => g.1.push(p),
                        None => sub.push((key, vec![p])),
                    }
                }
                for (key, items) in sub {
                    if key.is_empty() {
                        for p in items {
                            param_widget(ui, p, &mut changes);
                        }
                    } else {
                        egui::CollapsingHeader::new(&key)
                            .id_salt(format!("{group}.{key}"))
                            .default_open(!filter.is_empty())
                            .show(ui, |ui| {
                                for p in items {
                                    param_widget(ui, p, &mut changes);
                                }
                            });
                    }
                }
            });
        }
        for (path, v) in changes {
            let _ = params::set(params, &path, v);
        }
    }

    fn timing_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Timing").default_open(true).show(ui, |ui| {
            graph(ui, "frame ms", &self.frame_ms, Color32::from_rgb(110, 200, 255), 33.3);
            graph(ui, "tick ms", &self.tick_ms, Color32::from_rgb(255, 180, 90), 4.0);
        });
    }
}

fn label_of(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn param_widget(ui: &mut egui::Ui, p: &ParamInfo, changes: &mut Vec<(String, ParamValue)>) {
    let name = label_of(&p.path).replace('_', " ");
    ui.horizontal(|ui| {
        let resp = match (&p.kind, &p.value) {
            (ParamKind::Float { min, max }, v) => {
                let mut x = v.as_f64().unwrap_or(0.0) as f32;
                let r = ui.add(egui::Slider::new(&mut x, *min..=*max).text(&name).clamping(egui::SliderClamping::Always));
                if r.changed() {
                    changes.push((p.path.clone(), ParamValue::Float(x as f64)));
                }
                r
            }
            (ParamKind::Int { min, max }, v) => {
                let mut x = v.as_f64().unwrap_or(0.0) as i32;
                let r = ui.add(egui::Slider::new(&mut x, *min..=*max).text(&name));
                if r.changed() {
                    changes.push((p.path.clone(), ParamValue::Int(x as i64)));
                }
                r
            }
            (ParamKind::Bool, v) => {
                let mut b = v.as_bool().unwrap_or(false);
                let r = ui.checkbox(&mut b, &name);
                if r.changed() {
                    changes.push((p.path.clone(), ParamValue::Bool(b)));
                }
                r
            }
            (ParamKind::Choice { options }, v) => {
                let cur = match v {
                    ParamValue::Text(t) => t.clone(),
                    _ => String::new(),
                };
                let mut picked = None;
                let r = egui::ComboBox::from_label(&name)
                    .selected_text(&cur)
                    .show_ui(ui, |ui| {
                        for o in options {
                            if ui.selectable_label(*o == cur, o).clicked() {
                                picked = Some(o.clone());
                            }
                        }
                    })
                    .response;
                if let Some(o) = picked {
                    changes.push((p.path.clone(), ParamValue::Text(o)));
                }
                r
            }
        };
        if !p.help.is_empty() {
            resp.on_hover_text(format!("{}\n{}", p.help, p.path));
        }
    });
}

fn graph(ui: &mut egui::Ui, label: &str, data: &VecDeque<f32>, color: Color32, scale_max: f32) {
    let last = data.back().copied().unwrap_or(0.0);
    let max = data.iter().copied().fold(0.0, f32::max);
    ui.label(RichText::new(format!("{label}: {last:.2} (max {max:.2})")).small());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_black_alpha(90));
    let top = max.max(scale_max * 0.25).max(1e-3);
    let n = data.len().max(2);
    let pts: Vec<egui::Pos2> = data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1) as f32, rect.bottom() - rect.height() * (v / top).min(1.0))
        })
        .collect();
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, color)));
}
