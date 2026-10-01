//! egui overlays: boot diagnostics, stats, crash banner.

use egui::{Color32, RichText};

use crate::boot::{StageStatus, diag};
use crate::input::{Device, guide};

pub struct OverlayInfo<'a> {
    pub fps: f32,
    pub frame_ms: f32,
    pub tps: f32,
    pub tick_ms: f32,
    pub tick: u64,
    pub entities: usize,
    pub gpu_errors: u32,
    pub adapter: &'a str,
    pub crash: Option<String>,
    pub paused: bool,
    pub speed: f32,
    pub rewinding: bool,
}

pub enum MenuAction {
    Resume,
    Reset,
    Rooms,
    RoomCard,
    LoadScene(String),
    Tuning,
    Feel,
    Physics,
    Screenshot,
    Quit,
}

pub fn boot_panel(ctx: &egui::Context, open: &mut bool) {
    let d = diag();
    egui::Window::new("Boot diagnostics").open(open).default_pos([12.0, 12.0]).resizable(false).collapsible(true).show(
        ctx,
        |ui| {
            let stages = d.stages.lock().map(|s| s.clone()).unwrap_or_default();
            let total: f64 = stages.iter().map(|s| s.ms).sum();
            egui::Grid::new("stages").striped(true).show(ui, |ui| {
                for s in &stages {
                    ui.label(RichText::new(format!("{:02}", s.num)).monospace());
                    ui.label(&s.name);
                    ui.label(RichText::new(format!("{:>7.1} ms", s.ms)).monospace());
                    match &s.status {
                        StageStatus::Ok => ui.label(RichText::new("ok").color(Color32::from_rgb(90, 200, 110))),
                        StageStatus::Failed(m) => ui.label(RichText::new(format!("FAILED: {m}")).color(Color32::RED)),
                    };
                    ui.label(RichText::new(&s.detail).small());
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(format!("Total {:.0} ms · log: {}", total, d.log_path.display()));
            ui.label(RichText::new("F3 toggles this panel").small().weak());
        },
    );
}

/// `free` = screen area not covered by side panels.
pub fn stats_panel(ctx: &egui::Context, i: &OverlayInfo, free: egui::Rect) {
    let pos = free.right_top() + egui::vec2(-10.0, 10.0);
    egui::Area::new(egui::Id::new("stats")).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("{:.0} fps  {:.2} ms", i.fps, i.frame_ms)).monospace());
            ui.label(RichText::new(format!("sim {:.0} tps  {:.3} ms/tick", i.tps, i.tick_ms)).monospace());
            ui.label(RichText::new(format!("tick {}  entities {}", i.tick, i.entities)).monospace());
            if i.rewinding {
                ui.label(RichText::new("◀◀ REWINDING").color(Color32::from_rgb(120, 200, 255)).strong());
            } else if i.paused {
                ui.label(RichText::new("PAUSED").color(Color32::YELLOW).strong());
            } else if (i.speed - 1.0).abs() > 0.01 {
                ui.label(RichText::new(format!("speed ×{:.2}", i.speed)).color(Color32::YELLOW));
            }
            if i.gpu_errors > 0 {
                ui.label(RichText::new(format!("{} GPU errors (see log)", i.gpu_errors)).color(Color32::RED));
            }
            ui.label(RichText::new(i.adapter).small().weak());
        });
    });
}

pub fn crash_banner(root: &mut egui::Ui, msg: &str) {
    egui::Panel::top("crash").show(root, |ui| {
        ui.label(RichText::new("The simulation crashed").color(Color32::RED).strong());
        ui.label(msg);
        ui.label(format!("Full report in {}", diag().log_path.display()));
    });
}

pub fn hint_bar(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::new("hints")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -8.0]).show(ctx, |ui| {
        ui.label(
            RichText::new(text).small().color(Color32::from_white_alpha(200)).background_color(Color32::from_black_alpha(110)),
        );
    });
}

pub fn guide_panel(ctx: &egui::Context, device: Device, game: bool) {
    egui::Area::new(egui::Id::new("guide")).anchor(egui::Align2::LEFT_BOTTOM, [10.0, -30.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(
                RichText::new(match device {
                    Device::KeyboardMouse => "Controls (keyboard + mouse)",
                    Device::Gamepad => "Controls (gamepad)",
                })
                .strong(),
            );
            egui::Grid::new("guide_grid").show(ui, |ui| {
                for (a, k) in if game { crate::input::game_guide(device) } else { guide(device) } {
                    ui.label(*a);
                    ui.label(RichText::new(*k).monospace());
                    ui.end_row();
                }
            });
        });
    });
}

pub fn pause_menu(
    ctx: &egui::Context,
    device: Device,
    scene: &str,
    difficulty: Option<&mut pav_core::arpg::Difficulty>,
) -> Option<MenuAction> {
    let mut action = None;
    egui::Window::new("Paused")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(format!("Room: {scene}"));
            ui.horizontal(|ui| {
                if ui.button("Resume (Esc)").clicked() {
                    action = Some(MenuAction::Resume);
                }
                if ui.button("Rooms (F2)").clicked() {
                    action = Some(MenuAction::Rooms);
                }
                if ui.button("Room info").clicked() {
                    action = Some(MenuAction::RoomCard);
                }
                if ui.button("Reset scene").clicked() {
                    action = Some(MenuAction::Reset);
                }
                if ui.button("Tuning (F1)").clicked() {
                    action = Some(MenuAction::Tuning);
                }
                if ui.button("Feel metrics").clicked() {
                    action = Some(MenuAction::Feel);
                }
                if ui.button("Physics stats").clicked() {
                    action = Some(MenuAction::Physics);
                }
                if ui.button("Screenshot (F12)").clicked() {
                    action = Some(MenuAction::Screenshot);
                }
                if ui.button("Quit").clicked() {
                    action = Some(MenuAction::Quit);
                }
            });
            if let Some(d) = difficulty {
                ui.separator();
                crate::arpg_ui::difficulty_ui(ui, d);
            }
            ui.separator();
            ui.label(RichText::new("Load scene").strong());
            for (name, about) in pav_core::scenes::SCENES {
                ui.horizontal(|ui| {
                    if ui.button(*name).clicked() {
                        action = Some(MenuAction::LoadScene(name.to_string()));
                    }
                    ui.label(RichText::new(*about).small());
                });
            }
            ui.separator();
            ui.label(RichText::new("Controls").strong());
            egui::Grid::new("menu_guide").show(ui, |ui| {
                for (a, k) in guide(device) {
                    ui.label(*a);
                    ui.label(RichText::new(*k).monospace());
                    ui.end_row();
                }
            });
            ui.label(RichText::new("System keys: F1 tuning · F2 rooms · F3 diagnostics · F4 leave room · F5 reset room · F6 pause · F7 step · F8/F9 speed · F10 edit mode · F11 fullscreen · F12 screenshot · hold Backspace rewind").small().weak());
        });
    action
}

pub fn toast(ctx: &egui::Context, msg: &str) {
    egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(msg);
        });
    });
}
