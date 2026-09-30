//! egui overlays: boot diagnostics, stats, crash banner.

use egui::{Color32, RichText};

use crate::boot::{StageStatus, diag};

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

pub fn stats_panel(ctx: &egui::Context, i: &OverlayInfo) {
    egui::Area::new(egui::Id::new("stats")).anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("{:.0} fps  {:.2} ms", i.fps, i.frame_ms)).monospace());
            ui.label(RichText::new(format!("sim {:.0} tps  {:.3} ms/tick", i.tps, i.tick_ms)).monospace());
            ui.label(RichText::new(format!("tick {}  entities {}", i.tick, i.entities)).monospace());
            if i.paused {
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
