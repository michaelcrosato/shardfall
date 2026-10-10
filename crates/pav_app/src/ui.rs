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
    /// The scene's resolution as a fraction of the screen's.
    pub render_scale: f32,
}

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
    /// The live animation workspace.
    AnimationStudio,
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
                    Device::Touch => "Controls (touch)",
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
    let game = difficulty.is_some();
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
                if ui
                    .button("Look & filters")
                    .on_hover_text("Pixel art, cel shading, outlines, palettes, grading and more, on the whole scene, the characters & objects or the environment")
                    .clicked()
                {
                    action = Some(MenuAction::Look);
                }
                if ui.button("Animation Studio").clicked() {
                    action = Some(MenuAction::AnimationStudio);
                }
                if !game
                    && ui
                        .button("How it works (H)")
                        .on_hover_text("The station guide of the room you are in: how it works, where games use it, how to ask for it")
                        .clicked()
                {
                    action = Some(MenuAction::Guide);
                }
                if !game && ui.button("Field guide").on_hover_text("Every word, and what to ask for in every room").clicked() {
                    action = Some(MenuAction::FieldGuide);
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
            let difficulty_shown = difficulty.is_some();
            if let Some(d) = difficulty {
                ui.separator();
                crate::arpg_ui::difficulty_ui(ui, d);
                ui.horizontal(|ui| {
                    if ui.button("New hero").on_hover_text("Erase the saved hero and start again in Emberwatch").clicked() {
                        action = Some(MenuAction::NewHero);
                    }
                    ui.label(RichText::new("The hero is saved on every trip and on quit.").small().weak());
                });
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
                for (a, k) in if difficulty_shown { crate::input::game_guide(device) } else { guide(device) } {
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

/// Touch screens: bigger targets, a little bigger text, thicker scroll bars, and dragging
/// scrolls. `on = false` puts the desktop sizes back.
pub fn touch_style(ctx: &egui::Context, on: bool) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|s| {
        s.spacing = egui::style::Spacing::default();
        s.text_styles = egui::style::default_text_styles();
        if on {
            let sp = &mut s.spacing;
            sp.interact_size = egui::vec2(44.0, 34.0);
            sp.button_padding = egui::vec2(12.0, 7.0);
            sp.item_spacing = egui::vec2(10.0, 8.0);
            sp.slider_width = 170.0;
            sp.icon_width = 20.0;
            sp.icon_width_inner = 12.0;
            sp.combo_height = 320.0;
            sp.scroll = egui::style::ScrollStyle::solid();
            sp.scroll.bar_width = 8.0;
            for (style, size) in
                [(TextStyle::Body, 15.0), (TextStyle::Button, 15.0), (TextStyle::Small, 12.0), (TextStyle::Heading, 20.0)]
            {
                s.text_styles.insert(style, FontId::proportional(size));
            }
            s.interaction.tooltip_delay = 0.35;
        }
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
                ui.label(RichText::new("Playing by touch").strong().size(17.0));
                let tips: &[(&str, &str)] = if game {
                    &[
                        ("Move", "drag anywhere"),
                        ("Attack", "automatic"),
                        ("Skills", "tap, or drag to aim"),
                        ("Messages", "swipe away"),
                        ("Windows", "tap outside to close"),
                    ]
                } else {
                    &[("Move", "drag anywhere"), ("Jump, bomb, duck", "the corner buttons"), ("Messages", "swipe away")]
                };
                egui::Grid::new("touch_tips_grid").spacing([12.0, 4.0]).show(ui, |ui| {
                    for (a, k) in tips {
                        ui.label(RichText::new(*a).strong());
                        ui.label(RichText::new(*k).weak());
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Swipe this away to play").small().color(Color32::from_rgb(255, 214, 150)));
            });
        },
    );
}

/// Settings the touch menu changes in place.
pub struct TouchMenu<'a> {
    pub prefs: &'a mut crate::save::Prefs,
    pub show_fps: &'a mut bool,
    /// Shardfall's difficulty (None outside the game).
    pub difficulty: Option<&'a mut pav_core::arpg::Difficulty>,
    /// Somewhere a town portal can leave from.
    pub portal: bool,
    /// The scene's resolution now (percent of the screen's).
    pub scale: f32,
}

/// The pause menu on touch screens: a column of big buttons, the settings that matter on a
/// phone, and the rest folded away.
pub fn touch_menu(ctx: &egui::Context, scene: &str, m: TouchMenu) -> Option<MenuAction> {
    let mut action = None;
    let screen = ctx.content_rect();
    let w = (screen.width() - 32.0).min(380.0);
    let game = m.difficulty.is_some();
    egui::Window::new("Paused")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .min_width(w)
        .max_width(w)
        .show(ctx, |ui| {
            // (A scroll area that shrinks to fit: the window's own would keep its full height.)
            egui::ScrollArea::vertical()
                .max_height(screen.height() - 40.0)
                .show(ui, |ui| touch_menu_body(ui, w, scene, game, m, &mut action));
        });
    action
}

fn touch_menu_body(ui: &mut egui::Ui, w: f32, scene: &str, game: bool, m: TouchMenu, action: &mut Option<MenuAction>) {
    ui.set_width(w);
    let big = |ui: &mut egui::Ui, text: &str| ui.add_sized([w, 42.0], egui::Button::new(RichText::new(text).size(16.0)));
    ui.vertical_centered(|ui| ui.label(RichText::new("Paused").heading()));
    if big(ui, "Resume").clicked() {
        *action = Some(MenuAction::Resume);
    }
    if m.portal && big(ui, "Town portal").clicked() {
        *action = Some(MenuAction::TownPortal);
    }
    ui.separator();
    if game {
        let on = m.prefs.auto_attack;
        let label = if on { "Auto-attack: on" } else { "Auto-attack: off" };
        if ui.add_sized([w, 38.0], egui::Button::new(label).selected(on)).clicked() {
            m.prefs.auto_attack = !on;
        }
    }
    ui.label(RichText::new(format!("Graphics (drawing {:.0}% of the screen's pixels)", m.scale * m.scale * 100.0)).small());
    ui.horizontal(|ui| {
        let bw = (w - 3.0 * ui.spacing().item_spacing.x) / 4.0;
        for q in crate::quality::Quality::ALL {
            if ui.add_sized([bw, 36.0], egui::Button::new(q.name()).selected(m.prefs.quality == q)).clicked() {
                m.prefs.quality = q;
            }
        }
    });
    ui.horizontal(|ui| {
        let bw = (w - ui.spacing().item_spacing.x) / 2.0;
        if ui.add_sized([bw, 36.0], egui::Button::new("Show FPS").selected(*m.show_fps)).clicked() {
            *m.show_fps = !*m.show_fps;
        }
        if ui.add_sized([bw, 36.0], egui::Button::new("Full screen")).clicked() {
            *action = Some(MenuAction::Fullscreen);
        }
    });
    ui.separator();
    egui::CollapsingHeader::new("More").show(ui, |ui| {
        if let Some(d) = m.difficulty {
            crate::arpg_ui::difficulty_ui(ui, d);
            if ui.button("New hero").clicked() {
                *action = Some(MenuAction::NewHero);
            }
            ui.separator();
        }
        ui.horizontal_wrapped(|ui| {
            for (label, a) in [
                ("Look & filters", MenuAction::Look),
                ("Rooms", MenuAction::Rooms),
                ("Reset scene", MenuAction::Reset),
                ("Tuning", MenuAction::Tuning),
                ("Feel metrics", MenuAction::Feel),
            ] {
                if ui.button(label).clicked() {
                    *action = Some(a);
                }
            }
            if !game && ui.button("How it works").clicked() {
                *action = Some(MenuAction::Guide);
            }
        });
        ui.label(RichText::new(format!("Load scene (now: {scene})")).strong());
        ui.horizontal_wrapped(|ui| {
            for (name, about) in pav_core::scenes::SCENES {
                if ui.button(*name).on_hover_text(*about).clicked() {
                    *action = Some(MenuAction::LoadScene(name.to_string()));
                }
            }
        });
        ui.label(RichText::new("Controls").strong());
        for (a, k) in if game { crate::input::game_guide(Device::Touch) } else { guide(Device::Touch) } {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(*a).strong());
                ui.label(*k);
            });
        }
    });
}
