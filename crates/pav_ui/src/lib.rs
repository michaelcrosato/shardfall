//! Shardfall's game UI, drawn with egui: the HUD (`arpg_ui`), the inventory, shop, stash and
//! character windows (`arpg_items`), the passive tree (`arpg_tree`) and the touch controls
//! (`touch`). The window (`pav_app`) draws it over the game; the tools draw the same code on a
//! headless context (`headless`): a text reading of what is on screen, and the UI over captures.

pub mod arpg_items;
pub mod arpg_tree;
pub mod arpg_ui;
pub mod controls;
pub mod drive;
pub mod fonts;
pub mod glyph;
pub mod headless;
pub mod padnav;
pub mod plaza;
pub mod splash;
pub mod tools;
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

/// The UI's zoom (egui's `zoom_factor`) for a screen `height` logical pixels tall: the game is
/// laid out for 720 and grows with the screen, so a 1080p TV across the room shows it half as
/// big again and a 4K one more (up to three times). `size` is the player's setting (Settings:
/// menu and HUD size); a touch screen keeps its own layout's scale (`touch_scale`).
pub fn ten_foot_zoom(height: f32, size: f32, touch: bool) -> f32 {
    let screen = if touch { 1.0 } else { (height / 720.0).clamp(1.0, 3.0) };
    (screen * size.clamp(0.7, 1.5) * 100.0).round() / 100.0
}

/// Touch screens: bigger targets, a little bigger text, thicker scroll bars, and dragging
/// scrolls. `on = false` puts the screen's sizes back (`game_style`).
pub fn touch_style(ctx: &egui::Context, on: bool) {
    game_style(ctx, on, false);
}

/// The game UI's style. Touch screens: bigger targets and text for fingers. Otherwise, text
/// sized to read from a sofa ten feet from a TV (with `ten_foot_zoom` on top: body text about
/// 22 pixels at 1080p), small print included; the studios (`studio`) keep a desk's sizes. No
/// label is selectable: a game's text is read, never a click target or a stop for the pad.
pub fn game_style(ctx: &egui::Context, touch: bool, studio: bool) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|s| {
        s.spacing = egui::style::Spacing::default();
        s.text_styles = egui::style::default_text_styles();
        s.interaction.selectable_labels = false;
        if touch {
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
        } else if !studio {
            let sp = &mut s.spacing;
            sp.interact_size = egui::vec2(40.0, 24.0);
            sp.button_padding = egui::vec2(8.0, 4.0);
            sp.item_spacing = egui::vec2(9.0, 6.0);
            for (style, size) in [
                (TextStyle::Body, 15.5),
                (TextStyle::Button, 15.5),
                (TextStyle::Small, 12.5),
                (TextStyle::Monospace, 14.0),
                (TextStyle::Heading, 22.0),
            ] {
                s.text_styles.insert(style, FontId::proportional(size));
            }
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        }
    });
}

/// A `pav_core::games` game's HUD lines (`Game::hud`), top left, and its prompts
/// (`Game::prompts`) along the bottom in the device's own glyphs.
pub fn game_lines(ctx: &egui::Context, h: &pav_core::frame::HudFrame) {
    let (device, style) = controls::current();
    let prompts: Vec<glyph::Prompt> = h
        .prompts
        .iter()
        .map(|(action, what)| glyph::Prompt { glyphs: controls::glyphs(action, device), label: what.clone() })
        .filter(|p| !p.glyphs.is_empty())
        .collect();
    if !prompts.is_empty() {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("game_prompts")));
        glyph::row(
            &p,
            screen.center_bottom() - egui::vec2(0.0, 44.0),
            egui::Align2::CENTER_BOTTOM,
            &prompts,
            24.0,
            style,
            egui::Color32::WHITE,
        );
    }
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

/// Lays `add` out for the language's direction: right-aligned, top to bottom, in a
/// right-to-left language; as it is otherwise.
pub fn directed<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if pav_core::lines::rtl() { ui.with_layout(egui::Layout::top_down(egui::Align::Max), add).inner } else { add(ui) }
}

/// A wrapped block of text laid out for the language's direction (right-aligned lines in a
/// right-to-left language).
pub fn text_job(text: String, font: egui::FontId, color: egui::Color32, wrap: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(text, font, color, wrap);
    if pav_core::lines::rtl() {
        job.halign = egui::Align::RIGHT;
    }
    job
}
