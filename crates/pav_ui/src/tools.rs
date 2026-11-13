//! The engine's tools: what they are called, what each one is, and the panel that lists them
//! (the pause menu). On the pavilion's plaza each tool is a place instead (`crate::plaza`), and
//! Emberwatch's Pavilion Gate leads back there.

use egui::{Align, Align2, Color32, FontId, Pos2, RichText, Stroke, Vec2};
use pav_core::t;

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);
const INK: Color32 = Color32::from_rgb(236, 230, 218);
const DIM: Color32 = Color32::from_rgb(170, 164, 152);

/// The engine's tools, as the pause menu lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// The pavilion (the `world` scene): where the game opens, and the way back from a game.
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
    pub fn color(self) -> Color32 {
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
