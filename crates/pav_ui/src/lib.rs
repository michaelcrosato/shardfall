//! Shardfall's game UI, drawn with egui: the HUD (`arpg_ui`), the inventory, shop, stash and
//! character windows (`arpg_items`), the passive tree (`arpg_tree`) and the touch controls
//! (`touch`). The window (`pav_app`) draws it over the game; the tools draw the same code on a
//! headless context (`headless`): a text reading of what is on screen, and the UI over captures.

pub mod arpg_items;
pub mod arpg_tree;
pub mod arpg_ui;
pub mod headless;
pub mod touch;

/// The input device the player is on: hints and the HUD follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
    /// A phone or tablet (touch.rs).
    Touch,
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

/// A `pav_core::games` game's HUD lines (`Game::hud`), top left.
pub fn game_lines(ctx: &egui::Context, h: &pav_core::frame::HudFrame) {
    if h.lines.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("game_lines")).anchor(egui::Align2::LEFT_TOP, [16.0, 60.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for line in &h.lines {
                ui.label(egui::RichText::new(line).monospace().size(18.0).strong());
            }
        });
    });
}
