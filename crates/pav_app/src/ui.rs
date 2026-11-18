//! egui overlays: boot diagnostics, stats, crash banner.

use egui::{Color32, RichText};

use crate::boot::{StageStatus, diag};
use crate::input::Device;

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
    /// The scene's resolution as a fraction of the screen's.
    pub render_scale: f32,
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    Resume,
    Reset,
    Rooms,
    RoomCard,
    LoadScene(String),
    /// Shardfall: erase the save, start over in town.
    NewHero,
    Tuning,
    /// The Look & Filters window.
    Look,
    /// How the room you are in works (the station guide; outside rooms the field guide).
    Guide,
    /// Every word and every room's ask-for-it phrases.
    FieldGuide,
    Feel,
    Physics,
    Screenshot,
    Quit,
    /// Shardfall: home to town (touch screens have no T key).
    TownPortal,
    /// The browser's full screen.
    Fullscreen,
    /// One of the engine's tools (the plaza's stations as a panel).
    Tool(pav_ui::tools::Tool),
    /// To the pavilion's plaza, where the game opens and the tools stand.
    Pavilion,
    /// Another page of the pause menu.
    Page(crate::menu::Page),
    /// The Language window.
    Language,
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

/// `free` = screen area not covered by side panels. `compact`: one line at the bottom (touch
/// screens, whose top corners hold the controls).
pub fn stats_panel(ctx: &egui::Context, i: &OverlayInfo, free: egui::Rect, compact: bool) {
    if compact {
        egui::Area::new(egui::Id::new("stats")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -4.0]).interactable(false).show(
            ctx,
            |ui| {
                ui.label(
                    RichText::new(format!(
                        "{:.0} fps  {:.1} ms  ·  sim {:.2} ms  ·  {:.0}% res",
                        i.fps,
                        i.frame_ms,
                        i.tick_ms,
                        i.render_scale * 100.0
                    ))
                    .monospace()
                    .size(11.0)
                    .color(Color32::from_white_alpha(200))
                    .background_color(Color32::from_black_alpha(120)),
                );
            },
        );
        return;
    }
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
            if i.render_scale < 0.999 {
                ui.label(RichText::new(format!("scene at {:.0}% resolution", i.render_scale * 100.0)).small());
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

/// Prompts as the device's glyphs: along the bottom (the pavilion), or in play a column in the
/// bottom right corner, clear of the orbs, the skill bar and the experience bar.
pub fn hint_bar(ctx: &egui::Context, prompts: &[pav_ui::glyph::Prompt], style: pav_ui::glyph::PadStyle, in_play: bool) {
    if prompts.is_empty() {
        return;
    }
    let screen = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("hints")));
    let color = Color32::from_white_alpha(215);
    if in_play {
        let h = 18.0;
        for (i, prompt) in prompts.iter().rev().enumerate() {
            let at = screen.right_bottom() - egui::vec2(14.0, 14.0 + i as f32 * (h + 8.0));
            pav_ui::glyph::row(&p, at, egui::Align2::RIGHT_BOTTOM, std::slice::from_ref(prompt), h, style, color);
        }
    } else {
        pav_ui::glyph::row(
            &p,
            screen.center_bottom() - egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_BOTTOM,
            prompts,
            18.0,
            style,
            color,
        );
    }
}

/// The controls, bottom left, for the first seconds: the device's own glyphs.
pub fn guide_panel(ctx: &egui::Context, device: Device, game: bool, style: pav_ui::glyph::PadStyle) {
    egui::Area::new(egui::Id::new("guide")).anchor(egui::Align2::LEFT_BOTTOM, [10.0, -40.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(
                RichText::new(match device {
                    Device::KeyboardMouse => pav_core::t!("menu.controls.keyboard"),
                    Device::Gamepad => pav_core::t!("menu.controls.gamepad"),
                    Device::Touch => pav_core::t!("menu.controls.touch"),
                })
                .strong(),
            );
            let rows = if game { crate::input::game_rows(device) } else { crate::input::room_rows(device) };
            egui::Grid::new("guide_grid").spacing([12.0, 6.0]).show(ui, |ui| {
                for (glyphs, what) in rows {
                    crate::menu::glyph_cell(ui, &glyphs, 20.0, style);
                    ui.label(what);
                    ui.end_row();
                }
            });
        });
    });
}

pub fn toast(ctx: &egui::Context, msg: &str) {
    egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_TOP, [0.0, 110.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.label(msg);
        });
    });
}

/// Touch screens: a message that swipes away (top centre).
pub fn toast_touch(ctx: &egui::Context, msg: &str, swipes: &mut crate::touch::Swipes) {
    crate::touch::swipe_area(
        ctx,
        Some(swipes),
        "toast",
        crate::touch::key(msg),
        egui::Align2::CENTER_TOP,
        egui::vec2(0.0, 64.0),
        egui::Order::Foreground,
        |ui| {
            egui::Frame::popup(ui.style()).corner_radius(18.0).inner_margin(egui::vec2(14.0, 8.0)).show(ui, |ui| {
                ui.label(msg);
            });
        },
    );
}

/// Touch screens: the first-time tips, until swiped away.
pub fn touch_tips(ctx: &egui::Context, game: bool, swipes: &mut crate::touch::Swipes) {
    crate::touch::swipe_area(
        ctx,
        Some(swipes),
        "touch_tips",
        0,
        egui::Align2::CENTER_CENTER,
        egui::vec2(0.0, -20.0),
        egui::Order::Foreground,
        |ui| {
            egui::Frame::popup(ui.style()).corner_radius(14.0).inner_margin(14.0).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(RichText::new(pav_core::t!("touch.tips")).strong().size(17.0));
                let tips: &[(&str, &str)] = if game {
                    &[
                        ("controls.move", "touch.tips.move"),
                        ("controls.attack", "touch.tips.attack"),
                        ("controls.skills", "touch.tips.skills"),
                        ("touch.tips.messages", "touch.tips.messages.how"),
                        ("touch.tips.windows", "touch.tips.windows.how"),
                    ]
                } else {
                    &[
                        ("controls.move", "touch.tips.move"),
                        ("touch.tips.jump", "touch.tips.jump.how"),
                        ("touch.tips.messages", "touch.tips.messages.how"),
                    ]
                };
                egui::Grid::new("touch_tips_grid").spacing([12.0, 4.0]).show(ui, |ui| {
                    for (a, k) in tips {
                        ui.label(RichText::new(pav_core::lines::text(a, &[])).strong());
                        ui.label(RichText::new(pav_core::lines::text(k, &[])).weak());
                        ui.end_row();
                    }
                });
                ui.label(RichText::new(pav_core::t!("touch.tips.dismiss")).small().color(Color32::from_rgb(255, 214, 150)));
            });
        },
    );
}
