//! The pavilion's plaza on screen (`pav_core::plaza`): each station's name over it, a welcome
//! over the plaza, and a card for the station the player stands at whose button uses it
//! (interact, a click or a tap). Drawn by the window and, headless, by the tools. Every word is
//! a line.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, RichText, Stroke, Vec2};
use glam::Vec3;
use pav_core::frame::PlazaHud;
use pav_core::plaza::{STATIONS, Station};
use pav_core::t;

use crate::Device;
use crate::arpg_ui::Projector;

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);
const INK: Color32 = Color32::from_rgb(236, 230, 218);
const DIM: Color32 = Color32::from_rgb(196, 188, 172);
/// Names show for stations this close to the player (m).
const NAMES_WITHIN: f32 = 24.0;
/// Where the welcome floats: on the floor between the spawn and the fountain.
const WELCOME_AT: Vec3 = Vec3::new(0.0, 0.0, 3.9);

/// What the plaza's text needs besides the frame.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub hud: PlazaHud,
    pub device: Device,
    /// The saved Shardfall hero: name and level.
    pub hero: Option<(String, u32)>,
    /// A window covers the plaza (the pause menu, a studio): only the names show.
    pub covered: bool,
}

/// A station's name.
pub fn name(s: &Station) -> String {
    match s.key {
        "shardfall" => t!("title.name"),
        "animation" => t!("tools.animation"),
        "object" => t!("tools.object"),
        "creature" => t!("tools.creature"),
        "starter" => t!("tools.starter"),
        "looks" => t!("tools.look"),
        "tuning" => t!("tools.tuning"),
        "rooms" => t!("tools.rooms"),
        "guide" => t!("plaza.guide"),
        "language" => t!("title.language"),
        other => other.to_string(),
    }
}

/// What a station is, a sentence.
pub fn about(s: &Station) -> String {
    match s.key {
        "shardfall" => t!("title.tagline"),
        "animation" => t!("tools.animation.about"),
        "object" => t!("tools.object.about"),
        "creature" => t!("tools.creature.about"),
        "starter" => t!("tools.starter.about"),
        "looks" => t!("tools.look.about"),
        "tuning" => t!("tools.tuning.about"),
        "rooms" => t!("tools.rooms.about"),
        "guide" => t!("plaza.guide.about"),
        "language" => t!("plaza.language.about"),
        _ => String::new(),
    }
}

/// What using a station does: its button.
pub fn action(s: &Station, hero: Option<&(String, u32)>) -> String {
    match s.key {
        "shardfall" => match hero {
            Some((name, level)) => t!("plaza.shardfall.continue", name = name.as_str(), level = *level),
            None => t!("plaza.shardfall.use"),
        },
        "animation" | "object" | "creature" => t!("plaza.studio.use"),
        "starter" => t!("plaza.starter.use"),
        "looks" => t!("plaza.looks.use"),
        "tuning" => t!("plaza.tuning.use"),
        "rooms" => t!("plaza.rooms.use"),
        "guide" => t!("plaza.guide.use"),
        "language" => t!("plaza.language.use"),
        _ => String::new(),
    }
}

fn color(s: &Station) -> Color32 {
    Color32::from_hex(s.color).unwrap_or(GOLD)
}

/// Draws the plaza's text; returns the key of a station whose button was clicked or tapped.
pub fn draw(ctx: &egui::Context, view: &View, proj: &Projector) -> Option<&'static str> {
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("plaza_names")));
    let screen = ctx.content_rect();
    let near = view.hud.near.and_then(|i| STATIONS.get(i));
    // The welcome, while no station's card is up.
    if near.is_none() {
        if let Some(at) = proj.to_screen(WELCOME_AT) {
            let wrap = (screen.width() - 32.0).min(560.0);
            let title = p.layout_job(crate::text_job(t!("plaza.welcome"), FontId::proportional(22.0), GOLD, wrap));
            let body = p.layout_job(crate::text_job(t!("plaza.welcome.about"), FontId::proportional(14.5), INK, wrap));
            let h = title.size().y + 4.0 + body.size().y;
            let top = at.y - h * 0.5;
            let box_w = title.size().x.max(body.size().x) + 28.0;
            let back = egui::Rect::from_center_size(Pos2::new(at.x, at.y), Vec2::new(box_w, h + 18.0));
            p.rect_filled(back, 10.0, Color32::from_black_alpha(120));
            let title_x = at.x - title.size().x * 0.5 - title.rect.min.x;
            let body_x = at.x - body.size().x * 0.5 - body.rect.min.x;
            let title_h = title.size().y;
            p.galley(Pos2::new(title_x, top), title, GOLD);
            p.galley(Pos2::new(body_x, top + title_h + 4.0), body, INK);
        }
    }
    // Every station's name over it, wrapped to the room it has on screen (booths stand 3.2 m
    // apart: a high camera brings them close).
    for (i, s) in STATIONS.iter().enumerate() {
        if s.stand().distance(view.hud.feet) > NAMES_WITHIN {
            continue;
        }
        let Some(at) = proj.to_screen(s.tag()) else { continue };
        let span = proj.to_screen(s.tag() + Vec3::X * 2.9).map_or(200.0, |b| (b - at).length());
        let here = view.hud.near == Some(i);
        let full = if s.key == "shardfall" { 20.0 } else { 15.0 };
        let size = (full * (span / 120.0).clamp(0.75, 1.0)).round();
        let c = if here { Color32::from_rgb(255, 232, 170) } else { color(s).lerp_to_gamma(Color32::WHITE, 0.55) };
        let job = |color: Color32| {
            let mut job = crate::text_job(name(s), FontId::proportional(size), color, span.max(56.0));
            job.halign = egui::Align::Center;
            job
        };
        let galley = p.layout_job(job(c));
        let shadow = p.layout_job(job(Color32::from_black_alpha(210)));
        // A centred galley's lines are laid out around x = 0.
        let top = Pos2::new(at.x, at.y - galley.size().y);
        p.galley(top + Vec2::new(1.0, 1.5), shadow, Color32::from_black_alpha(210));
        p.galley(top, galley, c);
    }
    let s = near.filter(|_| !view.covered)?;
    card(ctx, s, view)
}

/// The card for the station the player stands at: its name, what it is, and its button.
fn card(ctx: &egui::Context, s: &'static Station, view: &View) -> Option<&'static str> {
    let screen = ctx.content_rect();
    let touch = view.device == Device::Touch;
    let width = (screen.width() - 32.0).min(440.0);
    // Touch screens keep the bottom corners for the stick and the buttons.
    let lift = if touch { (screen.height() * 0.2).max(120.0) } else { 28.0 };
    let mut used = false;
    egui::Area::new(egui::Id::new("plaza_card")).anchor(Align2::CENTER_BOTTOM, [0.0, -lift]).order(egui::Order::Middle).show(
        ctx,
        |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(18, 16, 22, 225))
                .stroke(Stroke::new(1.5, color(s).gamma_multiply(0.8)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    crate::directed(ui, |ui| {
                        ui.label(RichText::new(name(s)).size(20.0).color(GOLD).strong());
                        ui.label(RichText::new(about(s)).size(14.0).color(DIM));
                        ui.add_space(8.0);
                        let what = action(s, view.hero.as_ref());
                        let label =
                            if view.device == Device::KeyboardMouse { t!("plaza.hint", action = what.as_str()) } else { what };
                        let button =
                            egui::Button::new(RichText::new(label).size(17.0).color(Color32::from_rgb(30, 22, 12)).strong())
                                .fill(color(s).lerp_to_gamma(GOLD, 0.5))
                                .corner_radius(CornerRadius::same(8));
                        if ui.add_sized(Vec2::new(ui.available_width(), if touch { 48.0 } else { 36.0 }), button).clicked() {
                            used = true;
                        }
                    });
                });
        },
    );
    used.then_some(s.key)
}

/// The languages to choose from, when the address does not name one.
#[derive(Clone, Debug, Default)]
pub struct Languages {
    pub choices: Vec<String>,
    pub current: String,
}

/// The Language window (the plaza's Language station): every language the game has, each in
/// its own language. Returns the code picked; `open` goes false when closed.
pub fn language_window(ctx: &egui::Context, open: &mut bool, langs: &Languages) -> Option<String> {
    let mut picked = None;
    let mut keep = *open;
    egui::Window::new(t!("title.language"))
        .id(egui::Id::new("plaza_language"))
        .open(&mut keep)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            crate::directed(ui, |ui| {
                if langs.choices.is_empty() {
                    ui.label(RichText::new(t!("plaza.language.set")).color(DIM));
                    return;
                }
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 140.0).max(160.0)).show(ui, |ui| {
                    for code in &langs.choices {
                        let here = *code == langs.current;
                        if ui.selectable_label(here, pav_core::lines::language_name(code)).clicked() && !here {
                            picked = Some(code.clone());
                        }
                    }
                });
            });
        });
    *open = keep && picked.is_none();
    picked
}
