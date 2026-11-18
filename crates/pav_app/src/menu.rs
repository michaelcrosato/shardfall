//! The pause menu, the same on every device: a column of big buttons that reads from across a
//! room, and a page for each thing behind them (settings, controls, difficulty, the engine's
//! tools, the developer's switches). Options, Esc or the corner button opens it. A controller
//! moves through it with the D-pad (`pav_ui::padnav`), takes with Cross and goes back with
//! Circle; the mouse points and clicks; a finger taps. Nothing here is a dense data screen:
//! one thing to a row, big enough to read.

use egui::{Color32, RichText};
use pav_core::t;
use pav_ui::Device;
use pav_ui::glyph::{self, Glyph, PadStyle};

use crate::ui::MenuAction;

/// The menu's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    Settings,
    Controls,
    Difficulty,
    Tools,
    Developer,
}

/// What the menu shows and changes in place.
pub struct Menu<'a> {
    pub page: &'a mut Page,
    pub prefs: &'a mut crate::save::Prefs,
    pub show_fps: &'a mut bool,
    /// Shardfall's difficulty (None outside the game).
    pub difficulty: Option<&'a mut pav_core::arpg::Difficulty>,
    /// Somewhere a town portal can leave from.
    pub portal: bool,
    /// The scene's resolution now (a fraction of the screen's).
    pub scale: f32,
    pub device: Device,
    pub scene: &'a str,
    /// The page runs inside 52in1 (its own settings apply on top of these).
    pub in_hub: bool,
}

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);

/// Draws the menu; returns what the player chose.
pub fn menu(ctx: &egui::Context, m: Menu) -> Option<MenuAction> {
    let Menu { page, prefs, show_fps, difficulty, portal, scale, device, scene, in_hub } = m;
    let mut action = None;
    let screen = ctx.content_rect();
    let w = (screen.width() - 32.0).min(460.0);
    let game = difficulty.is_some();
    let title = match *page {
        Page::Main => t!("menu.paused"),
        Page::Settings => t!("menu.settings"),
        Page::Controls => t!("menu.controls"),
        Page::Difficulty => t!("menu.difficulty"),
        Page::Tools => t!("menu.tools"),
        Page::Developer => t!("menu.developer"),
    };
    egui::Window::new("pause_menu")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .min_width(w)
        .max_width(w)
        .show(ctx, |ui| {
            ui.set_width(w);
            ui.vertical_centered(|ui| ui.label(RichText::new(title).size(26.0).color(GOLD)));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(screen.height() - 120.0).show(ui, |ui| {
                ui.set_width(w);
                match *page {
                    Page::Main => main_page(ui, w, game, portal, &mut action),
                    Page::Settings => settings_page(ui, w, prefs, show_fps, scale, device, in_hub, &mut action),
                    Page::Controls => controls_page(ui, device, prefs.pad_style, game),
                    Page::Difficulty => {
                        if let Some(d) = difficulty {
                            crate::arpg_ui::difficulty_ui(ui, d);
                            ui.add_space(8.0);
                            if big(ui, w, &t!("menu.new_hero")).on_hover_text(t!("menu.new_hero.about")).clicked() {
                                action = Some(MenuAction::NewHero);
                            }
                            ui.label(RichText::new(t!("menu.saved")).weak());
                        }
                    }
                    Page::Tools => {
                        if let Some(tool) = pav_ui::directed(ui, |ui| pav_ui::tools::tools_panel(ui, 2)) {
                            action = Some(MenuAction::Tool(tool));
                        }
                    }
                    Page::Developer => developer_page(ui, w, scene, game, &mut action),
                }
            });
            if *page != Page::Main {
                ui.add_space(6.0);
                if big(ui, w, &t!("menu.back")).clicked() {
                    *page = Page::Main;
                }
            }
        });
    if let Some(MenuAction::Page(p)) = action {
        *page = p;
        return None;
    }
    action
}

/// A full-width button, tall enough to hit from the sofa.
fn big(ui: &mut egui::Ui, w: f32, text: &str) -> egui::Response {
    ui.add_sized([w, 46.0], egui::Button::new(RichText::new(text).size(20.0)))
}

fn main_page(ui: &mut egui::Ui, w: f32, game: bool, portal: bool, action: &mut Option<MenuAction>) {
    let resume = egui::Button::new(RichText::new(t!("menu.resume")).size(22.0).strong().color(Color32::BLACK)).fill(GOLD);
    if ui.add_sized([w, 52.0], resume).clicked() {
        *action = Some(MenuAction::Resume);
    }
    if game && portal && big(ui, w, &t!("menu.town_portal")).clicked() {
        *action = Some(MenuAction::TownPortal);
    }
    for (label, page, shown) in [
        (t!("menu.settings"), Page::Settings, true),
        (t!("menu.controls"), Page::Controls, true),
        (t!("menu.difficulty"), Page::Difficulty, game),
        (t!("menu.tools"), Page::Tools, true),
    ] {
        if shown && big(ui, w, &label).clicked() {
            *action = Some(MenuAction::Page(page));
        }
    }
    if big(ui, w, &t!("menu.pavilion")).clicked() {
        *action = Some(MenuAction::Pavilion);
    }
    if big(ui, w, &t!("menu.developer")).clicked() {
        *action = Some(MenuAction::Page(Page::Developer));
    }
    if cfg!(not(target_arch = "wasm32")) && big(ui, w, &t!("menu.quit")).clicked() {
        *action = Some(MenuAction::Quit);
    }
}

/// A row: its name on the left, its choices on the right, one picked.
fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, w: f32, label: &str, value: &mut T, options: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.label(RichText::new(label).size(18.0));
    ui.horizontal(|ui| {
        let n = options.len().max(1) as f32;
        let bw = (w - (n - 1.0) * ui.spacing().item_spacing.x) / n;
        for (v, name) in options {
            if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(name).size(18.0)).selected(*value == *v)).clicked() {
                *value = *v;
                changed = true;
            }
        }
    });
    ui.add_space(6.0);
    changed
}

#[allow(clippy::too_many_arguments)]
fn settings_page(
    ui: &mut egui::Ui,
    w: f32,
    prefs: &mut crate::save::Prefs,
    show_fps: &mut bool,
    scale: f32,
    device: Device,
    in_hub: bool,
    action: &mut Option<MenuAction>,
) {
    let on_off = || [(true, t!("settings.on")), (false, t!("settings.off"))];
    choice(
        ui,
        w,
        &t!("settings.prompts"),
        &mut prefs.pad_style,
        &[(PadStyle::PlayStation, t!("settings.prompts.playstation")), (PadStyle::Xbox, t!("settings.prompts.xbox"))],
    );
    choice(ui, w, &t!("settings.vibration"), &mut prefs.vibration, &on_off());
    ui.label(RichText::new(t!("settings.effects")).size(18.0));
    let mut percent = (prefs.effects * 100.0).round();
    ui.spacing_mut().slider_width = w - 80.0;
    if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).step_by(10.0).suffix("%")).changed() {
        prefs.effects = percent / 100.0;
    }
    ui.add_space(6.0);
    choice(
        ui,
        w,
        &t!("settings.ui_size"),
        &mut prefs.ui_size,
        &[(0.85, t!("settings.ui_size.small")), (1.0, t!("settings.ui_size.medium")), (1.2, t!("settings.ui_size.large"))],
    );
    if device == Device::Touch {
        choice(ui, w, &t!("settings.auto_attack"), &mut prefs.auto_attack, &on_off());
    }
    let percent = (scale * scale * 100.0).round() as i64;
    let qualities: Vec<_> = crate::quality::Quality::ALL.iter().map(|q| (*q, q.name())).collect();
    choice(ui, w, &t!("menu.graphics", percent = percent), &mut prefs.quality, &qualities);
    choice(ui, w, &t!("menu.show_fps"), show_fps, &on_off());
    ui.horizontal(|ui| {
        let bw = (w - ui.spacing().item_spacing.x) / 2.0;
        if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(t!("title.language")).size(18.0))).clicked() {
            *action = Some(MenuAction::Language);
        }
        if cfg!(target_arch = "wasm32")
            && ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(t!("title.fullscreen")).size(18.0))).clicked()
        {
            *action = Some(MenuAction::Fullscreen);
        }
    });
    if in_hub {
        ui.add_space(6.0);
        ui.label(RichText::new(t!("settings.hub")).weak());
    }
}

/// The controls for the device in use, one action to a row, its glyphs drawn.
fn controls_page(ui: &mut egui::Ui, device: Device, style: PadStyle, game: bool) {
    let label = match device {
        Device::KeyboardMouse => t!("menu.controls.keyboard"),
        Device::Gamepad => t!("menu.controls.gamepad"),
        Device::Touch => t!("menu.controls.touch"),
    };
    ui.label(RichText::new(label).size(18.0).strong());
    if device == Device::Touch {
        for (a, k) in crate::input::touch_guide() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(a).size(17.0).strong());
                ui.label(RichText::new(k).size(17.0));
            });
        }
        return;
    }
    let rows = if game { crate::input::game_rows(device) } else { crate::input::room_rows(device) };
    let h = 26.0;
    egui::Grid::new("controls_grid").spacing([18.0, 10.0]).show(ui, |ui| {
        for (glyphs, what) in rows {
            glyph_cell(ui, &glyphs, h, style);
            ui.label(RichText::new(what).size(18.0));
            ui.end_row();
        }
    });
    if device == Device::KeyboardMouse {
        ui.add_space(6.0);
        ui.label(RichText::new(t!("dev.menu.system_keys")).weak());
    }
}

/// A cell holding glyphs side by side.
pub fn glyph_cell(ui: &mut egui::Ui, glyphs: &[Glyph], h: f32, style: PadStyle) {
    let w = glyph::width_all(ui.painter(), glyphs, h, style);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.max(h), h), egui::Sense::hover());
    glyph::draw_all(ui.painter(), rect.left_center(), glyphs, h, style);
}

fn developer_page(ui: &mut egui::Ui, w: f32, scene: &str, game: bool, action: &mut Option<MenuAction>) {
    let row = |ui: &mut egui::Ui, items: &[(String, MenuAction)], action: &mut Option<MenuAction>| {
        ui.horizontal(|ui| {
            let n = items.len().max(1) as f32;
            let bw = (w - (n - 1.0) * ui.spacing().item_spacing.x) / n;
            for (label, a) in items {
                if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(label).size(17.0))).clicked() {
                    *action = Some(a.clone());
                }
            }
        });
    };
    row(ui, &[(t!("dev.menu.look"), MenuAction::Look), (t!("dev.menu.tuning"), MenuAction::Tuning)], action);
    row(ui, &[(t!("dev.menu.rooms"), MenuAction::Rooms), (t!("dev.menu.room_info"), MenuAction::RoomCard)], action);
    row(ui, &[(t!("dev.menu.feel"), MenuAction::Feel), (t!("dev.menu.physics"), MenuAction::Physics)], action);
    row(ui, &[(t!("dev.menu.reset_scene"), MenuAction::Reset), (t!("menu.screenshot"), MenuAction::Screenshot)], action);
    if !game {
        row(
            ui,
            &[(t!("dev.menu.how_it_works"), MenuAction::Guide), (t!("dev.menu.field_guide"), MenuAction::FieldGuide)],
            action,
        );
    }
    ui.add_space(6.0);
    ui.label(RichText::new(t!("dev.menu.load_scene.now", scene = scene)).size(17.0).strong());
    for (name, about) in pav_core::scenes::menu() {
        if ui.add_sized([w, 38.0], egui::Button::new(RichText::new(name).size(17.0))).on_hover_text(about).clicked() {
            *action = Some(MenuAction::LoadScene(name.to_string()));
        }
    }
}
