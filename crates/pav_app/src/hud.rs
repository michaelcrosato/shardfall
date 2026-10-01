//! Game HUD: course timer and results, short messages, the feel-metrics overlay (with live
//! input-latency measurement), camera cues from the level, and the hit flash.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use pav_core::character::MovementModel;
use pav_core::frame::HudFrame;
use pav_core::params::{self, ChoiceParam, ParamValue};
use pav_core::sim::TickRate;
use pav_core::zones::CameraCue;
use pav_view::{CameraParams, CameraRig};

/// One measured press: OS event -> simulation tick -> frame on screen.
#[derive(Clone, Copy, Debug)]
pub struct LatencySample {
    pub to_tick: f32,
    pub to_screen: f32,
}

/// Input latency measurement (the app owns the clock of input events).
#[derive(Default)]
pub struct Latency {
    /// Press waiting to be handed to the simulation.
    pub pending: Option<Instant>,
    pub samples: VecDeque<LatencySample>,
}

impl Latency {
    pub fn press(&mut self) {
        self.pending.get_or_insert_with(Instant::now);
    }
    pub fn record(&mut self, s: LatencySample) {
        self.samples.push_back(s);
        while self.samples.len() > 30 {
            self.samples.pop_front();
        }
    }
    fn stats(&self) -> Option<(f32, f32, f32, f32)> {
        let n = self.samples.len();
        if n == 0 {
            return None;
        }
        let tick = self.samples.iter().map(|s| s.to_tick).sum::<f32>() / n as f32;
        let total: Vec<f32> = self.samples.iter().map(|s| s.to_tick + s.to_screen).collect();
        let avg = total.iter().sum::<f32>() / n as f32;
        let min = total.iter().copied().fold(f32::MAX, f32::min);
        let max = total.iter().copied().fold(0.0, f32::max);
        Some((avg, tick, min, max))
    }
}

/// Applies camera cues (pads, camera zones) on top of the room's camera, with sweeps.
#[derive(Default)]
pub struct CameraDirector {
    serial: u64,
    base: Option<CameraParams>,
    cue: Option<Arc<CameraCue>>,
    start: Option<Instant>,
    quarters: u8,
}

fn set_camera_value(p: &mut CameraParams, key: &str, v: f32, quarters: u8) {
    let v = if key == "yaw" { ((v + quarters as f32 * 90.0 + 540.0).rem_euclid(360.0)) - 180.0 } else { v };
    let _ = params::set(p, key, ParamValue::Float(v as f64));
}

impl CameraDirector {
    pub fn update(&mut self, hud: &HudFrame, quarters: u8, rig: &mut CameraRig) {
        if hud.cue_serial != self.serial {
            self.serial = hud.cue_serial;
            match &hud.cue {
                Some(c) => {
                    let base = self.base.get_or_insert_with(|| rig.params.clone()).clone();
                    rig.blend_from_current(0.7);
                    let mut p = base;
                    for (k, v) in &c.set {
                        match v.as_f64() {
                            Some(f) if k != "ortho" => set_camera_value(&mut p, k, f as f32, quarters),
                            _ => {
                                let _ = params::set(&mut p, k, v.clone());
                            }
                        }
                    }
                    rig.params = p;
                    self.cue = Some(c.clone());
                    self.start = Some(Instant::now());
                    self.quarters = quarters;
                }
                None => {
                    if let Some(b) = self.base.take() {
                        rig.blend_from_current(0.7);
                        rig.params = b;
                    }
                    self.cue = None;
                }
            }
        }
        let Some(c) = &self.cue else { return };
        let t = self.start.map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
        for sw in &c.sweep {
            let u = (t / sw.period.max(0.1) + sw.phase) * std::f32::consts::TAU;
            let v = sw.from + (sw.to - sw.from) * (0.5 - 0.5 * u.cos());
            set_camera_value(&mut rig.params, &sw.param, v, self.quarters);
        }
        if c.flip_ortho > 0.0 {
            let base = c.set.get("ortho").and_then(|v| v.as_bool()).unwrap_or(false);
            rig.params.ortho = base ^ (((t / c.flip_ortho) as i64) % 2 == 1);
        }
    }
}

/// Speed history for the feel overlay graph.
#[derive(Default)]
pub struct FeelOverlay {
    pub open: bool,
    /// Opened automatically by the current room.
    pub auto: bool,
    speed: VecDeque<f32>,
    last_tick: u64,
}

pub struct HudCtx<'a> {
    pub hud: &'a HudFrame,
    pub tick: u64,
    pub dt: f32,
}

/// Course timer, results and level messages (top centre).
pub fn course_hud(ctx: &egui::Context, h: &HudCtx) {
    let ago = |tick: u64| h.tick.saturating_sub(tick) as f32 * h.dt;
    let result = h.hud.last_result.as_ref().filter(|r| ago(r.tick) < 6.0);
    let msg = h.hud.message.as_ref().filter(|(_, t)| ago(*t) < 2.5);
    if h.hud.course.is_none() && result.is_none() && msg.is_none() {
        return;
    }
    egui::Area::new(egui::Id::new("course_hud")).anchor(egui::Align2::CENTER_TOP, [0.0, 10.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                if let Some(c) = &h.hud.course {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(c.course.to_uppercase()).strong());
                        let t = if c.running { format!("{:6.2} s", c.time) } else { "ready".into() };
                        ui.label(RichText::new(t).monospace().size(22.0).strong());
                        if c.gates_total > 0 {
                            ui.label(format!("gates {}/{}", c.gates_passed, c.gates_total));
                        }
                        if c.missed > 0 {
                            ui.label(RichText::new(format!("missed {}", c.missed)).color(Color32::from_rgb(255, 170, 90)));
                        }
                        if c.hits > 0 {
                            ui.label(RichText::new(format!("hits {}", c.hits)).color(Color32::from_rgb(255, 120, 110)));
                        }
                        if c.falls > 0 {
                            ui.label(format!("falls {}", c.falls));
                        }
                        if let Some(b) = c.best {
                            ui.label(RichText::new(format!("best {b:.2} s")).weak());
                        }
                    });
                }
                if let Some(r) = result {
                    let mut line = format!("{}  {:.2} s", r.course.to_uppercase(), r.time);
                    if r.missed > 0 {
                        line += &format!("  (+{:.0} s: {} missed)", r.missed as f32 * pav_core::course::GATE_PENALTY, r.missed);
                    }
                    ui.label(RichText::new(line).size(20.0).strong());
                    let mut sub = Vec::new();
                    if r.hits > 0 {
                        sub.push(format!("{} hits", r.hits));
                    }
                    if r.falls > 0 {
                        sub.push(format!("{} falls", r.falls));
                    }
                    if r.new_best {
                        ui.label(RichText::new("NEW BEST").color(Color32::from_rgb(255, 215, 90)).strong());
                    } else {
                        sub.push(format!("best {:.2} s", r.best));
                    }
                    if !sub.is_empty() {
                        ui.label(sub.join(" · "));
                    }
                }
                if let Some((m, _)) = msg {
                    ui.label(RichText::new(m).color(Color32::from_rgb(255, 220, 140)).strong());
                }
            });
        });
    });
}

/// Red screen-edge flash while the player is invulnerable after a hit.
pub fn hit_flash(ctx: &egui::Context, invuln: f32) {
    if invuln <= 0.0 {
        return;
    }
    let a = (invuln / 0.6).clamp(0.0, 1.0);
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("hit_flash")));
    let w = 18.0 + 30.0 * a;
    let c = Color32::from_rgba_unmultiplied(230, 40, 30, (110.0 * a) as u8);
    painter.rect_stroke(rect.shrink(w * 0.5), 0.0, egui::Stroke::new(w, c), egui::StrokeKind::Middle);
}

pub struct FeelSettings<'a> {
    pub model: &'a mut MovementModel,
    pub tick_rate: &'a mut TickRate,
    pub vsync: &'a mut bool,
    pub smoothing: &'a mut bool,
}

impl FeelOverlay {
    pub fn record(&mut self, tick: u64, speed: f32) {
        if tick != self.last_tick {
            self.last_tick = tick;
            self.speed.push_back(speed);
            while self.speed.len() > 240 {
                self.speed.pop_front();
            }
        }
    }

    /// The overlay window (right side). Returns true if it is open.
    pub fn ui(&mut self, ctx: &egui::Context, h: &HudCtx, lat: &Latency, s: FeelSettings) {
        if !self.open {
            return;
        }
        let f = &h.hud.feel;
        let mut open = self.open;
        egui::Window::new("Feel metrics")
            .id(egui::Id::new("feel_overlay"))
            .open(&mut open)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -40.0])
            .resizable(false)
            .collapsible(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                egui::Grid::new("feel_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Movement model");
                    let mut i = s.model.to_index();
                    egui::ComboBox::from_id_salt("feel_model").selected_text(MovementModel::NAMES[i]).show_ui(ui, |ui| {
                        for (k, n) in MovementModel::NAMES.iter().enumerate() {
                            ui.selectable_value(&mut i, k, *n);
                        }
                    });
                    *s.model = MovementModel::from_index(i);
                    ui.end_row();
                    ui.label("Tick rate");
                    let mut t = s.tick_rate.to_index();
                    ui.horizontal(|ui| {
                        for (k, n) in TickRate::NAMES.iter().enumerate() {
                            ui.selectable_value(&mut t, k, format!("{n} Hz"));
                        }
                    });
                    *s.tick_rate = TickRate::from_index(t);
                    ui.end_row();
                    ui.label("Display");
                    ui.horizontal(|ui| {
                        ui.checkbox(s.vsync, "vsync");
                        ui.checkbox(s.smoothing, "smoothing");
                    });
                    ui.end_row();
                    ui.label("Input delay");
                    match lat.stats() {
                        Some((avg, tick, min, max)) => {
                            ui.label(
                                RichText::new(format!("{avg:.0} ms  ({min:.0}–{max:.0})"))
                                    .monospace()
                                    .strong(),
                            )
                            .on_hover_text(format!(
                                "Key press → simulation tick {tick:.1} ms, → frame submitted {:.1} ms.\nThe display adds its own scan-out time on top.",
                                avg - tick
                            ));
                        }
                        None => {
                            ui.label(RichText::new("press a key").weak());
                        }
                    }
                    ui.end_row();
                    let ms = h.dt * 1000.0;
                    ui.label("Response");
                    ui.label(format!("{} tick{} ({:.0} ms)", f.response_ticks, if f.response_ticks == 1 { "" } else { "s" }, f.response_ticks as f32 * ms));
                    ui.end_row();
                    ui.label("Speed");
                    ui.label(RichText::new(format!("{:.2} m/s  (top {:.2})", f.speed, f.top_speed)).monospace());
                    ui.end_row();
                    ui.label("To top speed");
                    ui.label(RichText::new(format!("{:.0} ms", f.accel_ms)).monospace());
                    ui.end_row();
                    ui.label("Stop");
                    ui.label(RichText::new(format!("{:.0} ms · {:.2} m", f.stop_ms, f.stop_dist)).monospace());
                    ui.end_row();
                    ui.label("Turnaround");
                    ui.label(RichText::new(format!("{:.0} ms", f.turn_ms)).monospace());
                    ui.end_row();
                    ui.label("Last jump");
                    ui.label(
                        RichText::new(format!("{:.2} m high · {:.0} ms · {:.1} m", f.jump_height, f.air_ms, f.jump_dist)).monospace(),
                    );
                    ui.end_row();
                });
                // Speed over the last few seconds.
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(260.0), 54.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 3.0, Color32::from_black_alpha(60));
                let max = self.speed.iter().copied().fold(1.0, f32::max) * 1.1;
                let n = self.speed.len().max(2);
                let pts: Vec<egui::Pos2> = self
                    .speed
                    .iter()
                    .enumerate()
                    .map(|(i, v)| egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1) as f32, rect.bottom() - rect.height() * v / max))
                    .collect();
                painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, Color32::from_rgb(120, 200, 255))));
                painter.text(rect.left_top() + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, format!("speed (max {max:.1} m/s)"), egui::FontId::proportional(10.0), Color32::from_white_alpha(150));
            });
        if !open {
            self.open = false;
        }
        let _ = Duration::ZERO;
    }
}
