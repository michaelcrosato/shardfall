//! The title screen: a short splash, then the start menu over the town. Shardfall comes first
//! (Continue, or Play for a new hero); the engine's tools have the other half of the screen
//! (`tools_panel`, also in the pause menu and at the Pavilion Gate in town). Every word is a
//! line (`t!`), so the screen follows the language; the Language menu sits here, where a
//! restart costs nothing.

use egui::{Align, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use pav_core::t;

/// The engine's tools, as the title screen, the pause menu and the gate offer them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// The pavilion: the engine's tour of rooms (the `world` scene).
    Tour,
    Animation,
    Object,
    Creature,
    /// The starter game (`pav_core::games`), the template a new game copies.
    Starter,
    Look,
    Rooms,
    Tuning,
}

impl Tool {
    /// The big tiles, in order; `Rooms` and `Tuning` sit in the row under them.
    pub const TILES: [Tool; 6] = [Tool::Tour, Tool::Animation, Tool::Object, Tool::Creature, Tool::Starter, Tool::Look];

    pub fn name(self) -> String {
        match self {
            Tool::Tour => t!("tools.tour"),
            Tool::Animation => t!("tools.animation"),
            Tool::Object => t!("tools.object"),
            Tool::Creature => t!("tools.creature"),
            Tool::Starter => t!("tools.starter"),
            Tool::Look => t!("tools.look"),
            Tool::Rooms => t!("tools.rooms"),
            Tool::Tuning => t!("tools.tuning"),
        }
    }

    pub fn about(self) -> String {
        match self {
            Tool::Tour => t!("tools.tour.about"),
            Tool::Animation => t!("tools.animation.about"),
            Tool::Object => t!("tools.object.about"),
            Tool::Creature => t!("tools.creature.about"),
            Tool::Starter => t!("tools.starter.about"),
            Tool::Look => t!("tools.look.about"),
            Tool::Rooms => t!("tools.rooms.about"),
            Tool::Tuning => t!("tools.tuning.about"),
        }
    }

    /// The tile's emblem colour.
    fn color(self) -> Color32 {
        match self {
            Tool::Tour => Color32::from_rgb(232, 112, 74),
            Tool::Animation => Color32::from_rgb(104, 182, 255),
            Tool::Object => Color32::from_rgb(240, 196, 92),
            Tool::Creature => Color32::from_rgb(120, 214, 140),
            Tool::Starter => Color32::from_rgb(196, 140, 255),
            Tool::Look => Color32::from_rgb(255, 128, 196),
            Tool::Rooms | Tool::Tuning => Color32::from_rgb(180, 186, 200),
        }
    }
}

/// What the player picked.
#[derive(Clone, Debug, PartialEq)]
pub enum TitleAction {
    /// Into the game: the saved hero, or a new one.
    Play,
    /// Erase the saved hero and start a new one.
    NewHero,
    Tool(Tool),
    Language(String),
    Fullscreen,
    Quit,
}

/// What the screen shows besides its own lines.
#[derive(Clone, Debug, Default)]
pub struct TitleInfo {
    /// The saved hero: name and level.
    pub hero: Option<(String, u32)>,
    /// The Language menu's choices (empty: no menu, the address names the language).
    pub languages: Vec<String>,
    pub language: String,
    /// The browser can go full screen.
    pub fullscreen: bool,
    /// A native build can quit.
    pub quit: bool,
}

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);
const INK: Color32 = Color32::from_rgb(236, 230, 218);
const DIM: Color32 = Color32::from_rgb(170, 164, 152);
/// Seconds of splash before the menu: fade in, hold, fade over to the menu.
const SPLASH_IN: f32 = 0.7;
const SPLASH_HOLD: f32 = 2.0;
const SPLASH_OUT: f32 = 0.6;

/// The title screen's state.
#[derive(Clone, Debug)]
pub struct TitleScreen {
    /// Seconds since it opened.
    pub age: f32,
    /// The splash is over (or was skipped).
    splash_done: bool,
    /// Asking before erasing the saved hero.
    confirm_new: bool,
    /// Keyboard focus has been put on the first button.
    focused: bool,
}

impl TitleScreen {
    /// `splash`: start with the splash (the first start), or straight on the menu.
    pub fn new(splash: bool) -> Self {
        Self { age: 0.0, splash_done: !splash, confirm_new: false, focused: false }
    }

    /// Any key, click or tap during the splash goes straight to the menu.
    pub fn skip_splash(&mut self) {
        if !self.splash_done {
            self.splash_done = true;
            self.age = SPLASH_IN + SPLASH_HOLD + SPLASH_OUT;
        }
    }

    pub fn in_splash(&self) -> bool {
        !self.splash_done
    }

    /// Draws the screen; `dt` is the frame's seconds.
    pub fn ui(&mut self, ctx: &egui::Context, info: &TitleInfo, dt: f32) -> Option<TitleAction> {
        self.age += dt.clamp(0.0, 0.25);
        let screen = ctx.content_rect();
        let splash_end = SPLASH_IN + SPLASH_HOLD;
        if !self.splash_done && self.age >= splash_end + SPLASH_OUT {
            self.splash_done = true;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("title_backdrop")));
        if !self.splash_done {
            // The splash: black, the emblem and the name fading in, then over to the menu.
            let a = if self.age < SPLASH_IN {
                1.0
            } else if self.age < splash_end {
                1.0
            } else {
                1.0 - ((self.age - splash_end) / SPLASH_OUT).clamp(0.0, 1.0)
            };
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha((255.0 * a) as u8));
            let show = (self.age / SPLASH_IN).clamp(0.0, 1.0) * a;
            let center = screen.center() - Vec2::new(0.0, screen.height() * 0.06);
            emblem(&painter, center - Vec2::new(0.0, screen.height() * 0.16), screen.height() * 0.09, show, self.age);
            wordmark(&painter, center, screen.height() * 0.13, show);
            let small = (screen.height() * 0.028).clamp(13.0, 26.0);
            painter.text(
                center + Vec2::new(0.0, screen.height() * 0.12),
                Align2::CENTER_CENTER,
                t!("title.engine"),
                FontId::proportional(small),
                fade(DIM, show),
            );
            if self.age > SPLASH_IN + 0.6 {
                let pulse = 0.55 + 0.45 * (self.age * 3.0).sin().abs();
                painter.text(
                    Pos2::new(center.x, screen.bottom() - screen.height() * 0.1),
                    Align2::CENTER_CENTER,
                    t!("title.skip"),
                    FontId::proportional(small * 0.8),
                    fade(DIM, show * pulse),
                );
            }
            if self.age < splash_end {
                return None;
            }
        }
        // The menu over the town: a shade from the left and right edges, and two cards.
        let menu_alpha = if self.splash_done { 1.0 } else { ((self.age - splash_end) / SPLASH_OUT).clamp(0.0, 1.0) };
        shade(&painter, screen, menu_alpha);
        let narrow = screen.width() < 900.0 || screen.width() < screen.height();
        let mut action = None;
        let margin = (screen.width() * 0.04).clamp(16.0, 64.0);
        if narrow {
            egui::Area::new(egui::Id::new("title_narrow"))
                .fixed_pos(screen.min + Vec2::splat(margin * 0.5))
                .order(egui::Order::Middle)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(screen.height() - margin).show(ui, |ui| {
                        ui.set_width(screen.width() - margin);
                        card(ui, |ui| crate::directed(ui, |ui| self.game_card(ui, info, screen, &mut action)));
                        ui.add_space(12.0);
                        card(ui, |ui| {
                            if let Some(tool) = crate::directed(ui, |ui| tools_panel(ui, 1)) {
                                action = Some(TitleAction::Tool(tool));
                            }
                        });
                    });
                });
        } else {
            let left = Rect::from_min_size(
                Pos2::new(screen.left() + margin, screen.top() + margin * 1.4),
                Vec2::new(screen.width() * 0.36, screen.height() - margin * 2.8),
            );
            let right_w = screen.width() * 0.46;
            let right = Rect::from_min_size(
                Pos2::new(screen.right() - margin - right_w, screen.top() + margin * 1.4),
                Vec2::new(right_w, screen.height() - margin * 2.8),
            );
            egui::Area::new(egui::Id::new("title_game")).fixed_pos(left.min).order(egui::Order::Middle).show(ctx, |ui| {
                ui.set_max_width(left.width());
                card(ui, |ui| {
                    ui.set_width(left.width() - 36.0);
                    self.game_card(ui, info, screen, &mut action);
                });
            });
            egui::Area::new(egui::Id::new("title_tools")).fixed_pos(right.min).order(egui::Order::Middle).show(ctx, |ui| {
                ui.set_max_width(right.width());
                card(ui, |ui| {
                    ui.set_width(right.width() - 36.0);
                    if let Some(tool) = crate::directed(ui, |ui| tools_panel(ui, 2)) {
                        action = Some(TitleAction::Tool(tool));
                    }
                });
            });
            painter.text(
                Pos2::new(screen.center().x, screen.bottom() - margin * 0.7),
                Align2::CENTER_CENTER,
                t!("title.footer"),
                FontId::proportional(13.0),
                fade(DIM, menu_alpha),
            );
        }
        if self.confirm_new {
            egui::Window::new(t!("title.new_hero"))
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.label(t!("title.new_hero_confirm"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t!("title.yes")).clicked() {
                            self.confirm_new = false;
                            action = Some(TitleAction::NewHero);
                        }
                        if ui.button(t!("title.no")).clicked() {
                            self.confirm_new = false;
                        }
                    });
                });
        }
        action
    }

    fn game_card(&mut self, ui: &mut egui::Ui, info: &TitleInfo, screen: Rect, action: &mut Option<TitleAction>) {
        let rtl = pav_core::lines::rtl();
        let h = screen.height();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), (h * 0.2).clamp(110.0, 220.0)), egui::Sense::hover());
        let painter = ui.painter_at(rect.expand(8.0));
        emblem(&painter, Pos2::new(rect.center().x, rect.top() + rect.height() * 0.26), rect.height() * 0.22, 1.0, self.age);
        wordmark(
            &painter,
            Pos2::new(rect.center().x, rect.top() + rect.height() * 0.68),
            (rect.height() * 0.42).min(rect.width() * 0.16),
            1.0,
        );
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("title.tagline")).color(DIM).size(15.0));
        });
        ui.add_space(18.0);
        let wide = Vec2::new(ui.available_width(), 52.0);
        let first = match &info.hero {
            Some((name, level)) => t!("title.continue_hero", name = name.as_str(), level = *level),
            None => t!("title.play"),
        };
        let play = ui.add_sized(
            wide,
            egui::Button::new(RichText::new(first).size(22.0).color(Color32::from_rgb(30, 22, 12)).strong())
                .fill(GOLD)
                .corner_radius(CornerRadius::same(10)),
        );
        if !self.focused && self.splash_done {
            play.request_focus();
            self.focused = true;
        }
        if play.clicked() {
            *action = Some(TitleAction::Play);
        }
        ui.add_space(8.0);
        if info.hero.is_some()
            && ui
                .add_sized(
                    Vec2::new(ui.available_width(), 34.0),
                    egui::Button::new(RichText::new(t!("title.new_hero")).size(16.0)),
                )
                .clicked()
        {
            self.confirm_new = true;
        }
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        let layout = if rtl { egui::Layout::right_to_left(Align::Center) } else { egui::Layout::left_to_right(Align::Center) };
        ui.with_layout(layout, |ui| {
            if !info.languages.is_empty() {
                ui.label(RichText::new(t!("title.language")).color(DIM));
                let current = pav_core::lines::language_name(&info.language);
                egui::ComboBox::from_id_salt("title_language").selected_text(current).show_ui(ui, |ui| {
                    for code in &info.languages {
                        if ui.selectable_label(*code == info.language, pav_core::lines::language_name(code)).clicked()
                            && *code != info.language
                        {
                            *action = Some(TitleAction::Language(code.clone()));
                        }
                    }
                });
            }
            if info.fullscreen && ui.button(t!("title.fullscreen")).clicked() {
                *action = Some(TitleAction::Fullscreen);
            }
            if info.quit && ui.button(t!("title.quit")).clicked() {
                *action = Some(TitleAction::Quit);
            }
        });
    }
}

/// The engine's tools: a heading, a line about the engine, then a tile per tool and a row for
/// the room list and the tuning panel. `columns`: tiles per row. Returns the tool picked.
pub fn tools_panel(ui: &mut egui::Ui, columns: usize) -> Option<Tool> {
    let mut picked = None;
    ui.label(RichText::new(t!("tools.heading")).size(24.0).color(GOLD).strong());
    ui.add_space(2.0);
    ui.label(RichText::new(t!("tools.intro")).color(INK).size(14.5));
    ui.add_space(10.0);
    let columns = columns.max(1);
    let gap = 10.0;
    let tile_w = ((ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32).max(120.0);
    for row in Tool::TILES.chunks(columns) {
        let height = row.iter().map(|t| tile_height(ui, *t, tile_w)).fold(64.0, f32::max);
        let layout = if pav_core::lines::rtl() {
            egui::Layout::right_to_left(Align::Min)
        } else {
            egui::Layout::left_to_right(Align::Min)
        };
        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for tool in row {
                if tile(ui, *tool, tile_w, height) {
                    picked = Some(*tool);
                }
            }
        });
        ui.add_space(gap * 0.6);
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for tool in [Tool::Rooms, Tool::Tuning] {
            if ui.button(RichText::new(tool.name()).size(15.0)).on_hover_text(tool.about()).clicked() {
                picked = Some(tool);
            }
        }
    });
    picked
}

fn about_galley(ui: &egui::Ui, tool: Tool, width: f32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_job(crate::text_job(tool.about(), FontId::proportional(13.0), DIM, width - 58.0))
}

fn tile_height(ui: &egui::Ui, tool: Tool, width: f32) -> f32 {
    about_galley(ui, tool, width).size().y + 42.0
}

/// One tool's tile: a coloured emblem, its name and what it is. True when clicked.
fn tile(ui: &mut egui::Ui, tool: Tool, width: f32, height: f32) -> bool {
    let name = tool.name();
    let about_galley = about_galley(ui, tool, width);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let hovered = response.hovered() || response.has_focus();
    let p = ui.painter();
    let fill =
        if hovered { Color32::from_rgba_unmultiplied(70, 66, 78, 235) } else { Color32::from_rgba_unmultiplied(40, 38, 46, 225) };
    p.rect(
        rect,
        10.0,
        fill,
        Stroke::new(if hovered { 1.5 } else { 1.0 }, if hovered { tool.color() } else { Color32::from_gray(70) }),
        egui::StrokeKind::Inside,
    );
    let rtl = pav_core::lines::rtl();
    let mark =
        if rtl { Pos2::new(rect.right() - 26.0, rect.top() + 26.0) } else { Pos2::new(rect.left() + 26.0, rect.top() + 26.0) };
    p.circle_filled(mark, 13.0, tool.color().gamma_multiply(0.9));
    p.circle_stroke(mark, 13.0, Stroke::new(2.0, Color32::from_black_alpha(90)));
    let (x, align) = if rtl { (rect.right() - 50.0, Align2::RIGHT_TOP) } else { (rect.left() + 50.0, Align2::LEFT_TOP) };
    p.text(Pos2::new(x, rect.top() + 10.0), align, name, FontId::proportional(17.0), INK);
    // A right-aligned galley's lines end at its right edge, which sits at x.
    let galley_pos = if rtl {
        Pos2::new(x - about_galley.rect.width().max(0.0) - about_galley.rect.min.x, rect.top() + 32.0)
    } else {
        Pos2::new(x, rect.top() + 32.0)
    };
    p.galley(galley_pos, about_galley, DIM);
    response.clicked()
}

/// A dark card behind a block of the menu.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(18, 16, 22, 215))
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(240, 198, 112, 60)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, add);
}

/// Darkens the town toward both edges so the cards read.
fn shade(p: &egui::Painter, screen: Rect, alpha: f32) {
    let steps = 16;
    let mut mesh = egui::Mesh::default();
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let edge = (f - 0.5).abs() * 2.0;
        let c = Color32::from_black_alpha(((40.0 + 150.0 * edge.powf(1.6)) * alpha) as u8);
        let x = screen.left() + f * screen.width();
        mesh.colored_vertex(Pos2::new(x, screen.top()), c);
        mesh.colored_vertex(Pos2::new(x, screen.bottom()), c);
        if i > 0 {
            let k = i * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// The game's name, wide and gold with a dark edge.
fn wordmark(p: &egui::Painter, center: Pos2, size: f32, alpha: f32) {
    let name = t!("title.name");
    let font = FontId::proportional(size.max(18.0));
    for off in [Vec2::new(0.0, size * 0.04), Vec2::new(size * 0.02, size * 0.02)] {
        p.text(center + off, Align2::CENTER_CENTER, &name, font.clone(), Color32::from_black_alpha((200.0 * alpha) as u8));
    }
    p.text(center, Align2::CENTER_CENTER, name, font, fade(GOLD, alpha));
}

/// The emblem: three crystal shards falling, each a lit and a shadowed face, turning slowly.
fn emblem(p: &egui::Painter, center: Pos2, size: f32, alpha: f32, time: f32) {
    let shards = [(-0.62, 0.18, 0.55, -0.35), (0.0, -0.1, 1.0, 0.0), (0.6, 0.22, 0.6, 0.38)];
    for (k, (dx, dy, scale, tilt)) in shards.iter().enumerate() {
        let wobble = (time * 0.8 + k as f32 * 1.7).sin() * 0.04;
        let c = center + Vec2::new(dx * size, dy * size + wobble * size);
        let s = size * scale;
        let angle = tilt + wobble;
        let rot = |v: Vec2| Vec2::new(v.x * angle.cos() - v.y * angle.sin(), v.x * angle.sin() + v.y * angle.cos());
        let top = c + rot(Vec2::new(0.0, -s * 0.62));
        let bottom = c + rot(Vec2::new(0.0, s * 0.62));
        let left = c + rot(Vec2::new(-s * 0.26, -s * 0.05));
        let right = c + rot(Vec2::new(s * 0.26, s * 0.05));
        let lit = Color32::from_rgb(150, 220, 255);
        let dark = Color32::from_rgb(70, 110, 190);
        p.add(egui::Shape::convex_polygon(vec![top, right, bottom], fade(dark, alpha), Stroke::NONE));
        p.add(egui::Shape::convex_polygon(vec![top, bottom, left], fade(lit, alpha), Stroke::NONE));
        p.add(egui::Shape::closed_line(
            vec![top, right, bottom, left],
            Stroke::new(1.5, fade(Color32::from_rgb(220, 245, 255), alpha * 0.8)),
        ));
    }
}
