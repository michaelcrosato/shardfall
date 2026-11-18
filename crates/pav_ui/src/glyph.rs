//! Button prompts that follow the device the player used last: PlayStation's symbols on a
//! controller (Xbox's A B X Y when the player picks them in Settings), keycaps and mouse buttons
//! on keyboard and mouse. Every glyph is drawn from shapes, not images or font symbols: it is
//! sharp at any size, reads at ten feet, and needs no font to carry ✕ ○ □ △.
//!
//! - [`draw`] draws one glyph; [`row`] a row of prompts ("✕ Wear   □ More   ○ Close").
//! - [`rich`] draws a line with glyphs inside it: lines carry `{key:action}`, which
//!   `pav_core::lines::rich` fills with a marker ([`marker`]) that this draws as the action's
//!   glyphs (`crate::controls::glyphs`).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2 as EVec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Which controller's names and symbols the prompts use (Settings: button prompts).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadStyle {
    #[default]
    PlayStation,
    Xbox,
}

/// A controller's button or stick, by its place in the standard mapping (named as on a
/// DualSense: Cross is button 0, Xbox's A).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pad {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    Create,
    Options,
    L3,
    R3,
    Up,
    Down,
    Left,
    Right,
    /// The D-pad as a whole (moving through a menu).
    DPad,
    /// The touchpad's press (button 17 on a DualSense).
    Touchpad,
    LeftStick,
    RightStick,
}

/// A mouse button, or its wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Wheel,
    /// The mouse itself (aiming, pointing).
    Move,
}

/// One glyph of a prompt.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Pad(Pad),
    /// A keyboard key, by the name on its cap ("W", "Space", "Esc").
    Key(String),
    Mouse(MouseButton),
}

impl Glyph {
    pub fn key(name: &str) -> Glyph {
        Glyph::Key(name.to_string())
    }
}

/// The name a glyph goes by in plain text (the tools' text readings, a screen reader).
pub fn name(g: &Glyph, style: PadStyle) -> String {
    let ps = style == PadStyle::PlayStation;
    match g {
        Glyph::Key(k) => k.clone(),
        Glyph::Mouse(MouseButton::Left) => "LMB".into(),
        Glyph::Mouse(MouseButton::Right) => "RMB".into(),
        Glyph::Mouse(MouseButton::Wheel) => "Wheel".into(),
        Glyph::Mouse(MouseButton::Move) => "Mouse".into(),
        Glyph::Pad(p) => match p {
            Pad::Cross => {
                if ps {
                    "Cross"
                } else {
                    "A"
                }
            }
            Pad::Circle => {
                if ps {
                    "Circle"
                } else {
                    "B"
                }
            }
            Pad::Square => {
                if ps {
                    "Square"
                } else {
                    "X"
                }
            }
            Pad::Triangle => {
                if ps {
                    "Triangle"
                } else {
                    "Y"
                }
            }
            Pad::L1 => {
                if ps {
                    "L1"
                } else {
                    "LB"
                }
            }
            Pad::R1 => {
                if ps {
                    "R1"
                } else {
                    "RB"
                }
            }
            Pad::L2 => {
                if ps {
                    "L2"
                } else {
                    "LT"
                }
            }
            Pad::R2 => {
                if ps {
                    "R2"
                } else {
                    "RT"
                }
            }
            Pad::L3 => {
                if ps {
                    "L3"
                } else {
                    "LS"
                }
            }
            Pad::R3 => {
                if ps {
                    "R3"
                } else {
                    "RS"
                }
            }
            Pad::Create => {
                if ps {
                    "Create"
                } else {
                    "View"
                }
            }
            Pad::Options => {
                if ps {
                    "Options"
                } else {
                    "Menu"
                }
            }
            Pad::Up => "D-pad up",
            Pad::Down => "D-pad down",
            Pad::Left => "D-pad left",
            Pad::Right => "D-pad right",
            Pad::DPad => "D-pad",
            Pad::Touchpad => "Touchpad",
            Pad::LeftStick => "Left stick",
            Pad::RightStick => "Right stick",
        }
        .into(),
    }
}

/// The face buttons' colours: PlayStation's symbols and Xbox's letters.
fn face(p: Pad, style: PadStyle) -> Color32 {
    match (style, p) {
        (PadStyle::PlayStation, Pad::Cross) => Color32::from_rgb(124, 178, 255),
        (PadStyle::PlayStation, Pad::Circle) => Color32::from_rgb(255, 110, 110),
        (PadStyle::PlayStation, Pad::Square) => Color32::from_rgb(238, 143, 208),
        (PadStyle::PlayStation, _) => Color32::from_rgb(63, 214, 160),
        (PadStyle::Xbox, Pad::Cross) => Color32::from_rgb(110, 196, 80),
        (PadStyle::Xbox, Pad::Circle) => Color32::from_rgb(235, 80, 70),
        (PadStyle::Xbox, Pad::Square) => Color32::from_rgb(80, 140, 240),
        (PadStyle::Xbox, _) => Color32::from_rgb(240, 200, 50),
    }
}

const PLATE: Color32 = Color32::from_rgb(28, 30, 36);
const RIM: Color32 = Color32::from_rgb(214, 218, 228);
const INK: Color32 = Color32::from_rgb(240, 242, 246);

/// How wide a glyph is at height `h`.
pub fn width(p: &Painter, g: &Glyph, h: f32, style: PadStyle) -> f32 {
    match g {
        Glyph::Key(k) => {
            let text = p.layout_no_wrap(k.clone(), FontId::proportional(h * 0.52), INK).size().x;
            (text + h * 0.55).max(h)
        }
        Glyph::Mouse(_) => h * 0.75,
        Glyph::Pad(Pad::L1 | Pad::R1 | Pad::L2 | Pad::R2) => h * 1.45,
        Glyph::Pad(Pad::Create | Pad::Options) if style == PadStyle::PlayStation => h * 1.2,
        Glyph::Pad(Pad::Touchpad) => h * 1.7,
        _ => h,
    }
}

/// Draws `g` with its left edge's middle at `at`, `h` tall. Returns where it went.
pub fn draw(p: &Painter, at: Pos2, g: &Glyph, h: f32, style: PadStyle) -> Rect {
    let w = width(p, g, h, style);
    let rect = Rect::from_min_size(pos2(at.x, at.y - h * 0.5), vec2(w, h));
    let c = rect.center();
    let rim = Stroke::new((h * 0.07).max(1.0), RIM);
    match g {
        Glyph::Key(k) => {
            let cap = rect.shrink(h * 0.04);
            p.rect_filled(cap.translate(vec2(0.0, h * 0.06)), h * 0.18, Color32::from_black_alpha(160));
            p.rect_filled(cap, h * 0.18, PLATE);
            p.rect_stroke(cap, h * 0.18, rim, egui::StrokeKind::Inside);
            p.text(c, Align2::CENTER_CENTER, k, FontId::proportional(h * 0.52), INK);
        }
        Glyph::Mouse(b) => {
            let body = Rect::from_center_size(c, vec2(w * 0.92, h * 0.98));
            p.rect_filled(body, w * 0.45, PLATE);
            let mid_y = body.top() + body.height() * 0.42;
            let hl = Color32::from_rgb(255, 205, 110);
            match b {
                MouseButton::Left => {
                    let half = Rect::from_min_max(body.min, pos2(c.x, mid_y));
                    p.rect_filled(half.shrink(h * 0.05), egui::CornerRadius { nw: (w * 0.4) as u8, ..Default::default() }, hl);
                }
                MouseButton::Right => {
                    let half = Rect::from_min_max(pos2(c.x, body.top()), pos2(body.right(), mid_y));
                    p.rect_filled(half.shrink(h * 0.05), egui::CornerRadius { ne: (w * 0.4) as u8, ..Default::default() }, hl);
                }
                MouseButton::Wheel => {
                    let wheel = Rect::from_center_size(pos2(c.x, body.top() + h * 0.22), vec2(w * 0.18, h * 0.26));
                    p.rect_filled(wheel, w * 0.09, hl);
                }
                MouseButton::Move => {}
            }
            p.line_segment([pos2(c.x, body.top() + h * 0.05), pos2(c.x, mid_y)], Stroke::new(rim.width * 0.8, RIM));
            p.line_segment(
                [pos2(body.left() + h * 0.04, mid_y), pos2(body.right() - h * 0.04, mid_y)],
                Stroke::new(rim.width * 0.8, RIM),
            );
            p.rect_stroke(body, w * 0.45, rim, egui::StrokeKind::Inside);
        }
        Glyph::Pad(b) => draw_pad(p, rect, *b, h, style),
    }
    // A glyph drawn without letters (a mouse button, ✕, the D-pad) still says its name to the
    // tools' text reading (`headless`): the same text, invisible.
    let lettered = matches!(g, Glyph::Key(_))
        || matches!(g, Glyph::Pad(Pad::L1 | Pad::R1 | Pad::L2 | Pad::R2 | Pad::L3 | Pad::R3))
        || (style == PadStyle::Xbox && matches!(g, Glyph::Pad(Pad::Cross | Pad::Circle | Pad::Square | Pad::Triangle)));
    if !lettered {
        p.text(rect.center(), Align2::CENTER_CENTER, name(g, style), FontId::proportional(h * 0.3), Color32::TRANSPARENT);
    }
    rect
}

fn draw_pad(p: &Painter, rect: Rect, b: Pad, h: f32, style: PadStyle) {
    let c = rect.center();
    let r = h * 0.5;
    let rim = Stroke::new((h * 0.07).max(1.0), RIM);
    let label = |p: &Painter, text: &str, size: f32| {
        p.text(c, Align2::CENTER_CENTER, text, FontId::proportional(size), INK);
    };
    match b {
        Pad::Cross | Pad::Circle | Pad::Square | Pad::Triangle => {
            p.circle_filled(c, r * 0.96, PLATE);
            p.circle_stroke(c, r * 0.96, rim);
            let col = face(b, style);
            match style {
                PadStyle::Xbox => {
                    let letter = match b {
                        Pad::Cross => "A",
                        Pad::Circle => "B",
                        Pad::Square => "X",
                        _ => "Y",
                    };
                    p.text(c, Align2::CENTER_CENTER, letter, FontId::proportional(h * 0.62), col);
                }
                PadStyle::PlayStation => {
                    let s = Stroke::new((h * 0.1).max(1.5), col);
                    let k = r * 0.42;
                    match b {
                        Pad::Cross => {
                            p.line_segment([c + vec2(-k, -k), c + vec2(k, k)], s);
                            p.line_segment([c + vec2(-k, k), c + vec2(k, -k)], s);
                        }
                        Pad::Circle => {
                            p.circle_stroke(c, k * 1.12, s);
                        }
                        Pad::Square => {
                            p.rect_stroke(Rect::from_center_size(c, EVec2::splat(k * 1.9)), 0.0, s, egui::StrokeKind::Middle);
                        }
                        _ => {
                            let t = k * 1.2;
                            let pts = vec![c + vec2(0.0, -t), c + vec2(t * 0.95, t * 0.62), c + vec2(-t * 0.95, t * 0.62)];
                            p.add(Shape::closed_line(pts, s));
                        }
                    }
                }
            }
        }
        Pad::L1 | Pad::R1 | Pad::L2 | Pad::R2 => {
            let trigger = matches!(b, Pad::L2 | Pad::R2);
            let body = rect.shrink2(vec2(h * 0.04, h * 0.1));
            let round = if trigger {
                egui::CornerRadius { nw: (h * 0.45) as u8, ne: (h * 0.45) as u8, sw: (h * 0.12) as u8, se: (h * 0.12) as u8 }
            } else {
                egui::CornerRadius::same((h * 0.16) as u8)
            };
            p.rect_filled(body, round, PLATE);
            p.rect_stroke(body, round, rim, egui::StrokeKind::Inside);
            label(p, &name(&Glyph::Pad(b), style), h * 0.46);
        }
        Pad::L3 | Pad::R3 | Pad::LeftStick | Pad::RightStick => {
            p.circle_filled(c, r * 0.96, PLATE);
            p.circle_stroke(c, r * 0.96, rim);
            p.circle_stroke(c, r * 0.62, Stroke::new(rim.width * 0.7, Color32::from_white_alpha(90)));
            let text = match b {
                Pad::LeftStick => "L",
                Pad::RightStick => "R",
                _ => &name(&Glyph::Pad(b), style),
            };
            label(p, text, h * if text.len() > 1 { 0.4 } else { 0.5 });
            if matches!(b, Pad::LeftStick | Pad::RightStick) {
                // Four ticks: it moves every way.
                for d in [vec2(0.0, -1.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(-1.0, 0.0)] {
                    let tip = c + d * r * 0.86;
                    let side = vec2(-d.y, d.x) * r * 0.13;
                    p.add(Shape::convex_polygon(
                        vec![tip, tip - d * r * 0.18 + side, tip - d * r * 0.18 - side],
                        RIM,
                        Stroke::NONE,
                    ));
                }
            }
        }
        Pad::Up | Pad::Down | Pad::Left | Pad::Right | Pad::DPad => {
            // A cross; the arm in use lit.
            let arm = h * 0.3;
            let reach = h * 0.48;
            let lit = Color32::from_rgb(255, 205, 110);
            for (dir, which) in [
                (vec2(0.0, -1.0), Pad::Up),
                (vec2(0.0, 1.0), Pad::Down),
                (vec2(-1.0, 0.0), Pad::Left),
                (vec2(1.0, 0.0), Pad::Right),
            ] {
                let end = c + dir * reach;
                let r2 = Rect::from_two_pos(c + vec2(-dir.y, dir.x) * arm * 0.5, end - vec2(-dir.y, dir.x) * arm * 0.5);
                let on = b == which;
                p.rect_filled(r2, h * 0.06, if on { lit } else { PLATE });
                p.rect_stroke(r2, h * 0.06, rim, egui::StrokeKind::Inside);
            }
            p.rect_filled(Rect::from_center_size(c, EVec2::splat(arm - rim.width)), 0.0, PLATE);
        }
        Pad::Create | Pad::Options => {
            let options = b == Pad::Options;
            match style {
                PadStyle::PlayStation => {
                    let body = Rect::from_center_size(c, vec2(rect.width() * 0.9, h * 0.62));
                    p.rect_filled(body, h * 0.31, PLATE);
                    p.rect_stroke(body, h * 0.31, rim, egui::StrokeKind::Inside);
                    if options {
                        for k in [-1.0, 0.0, 1.0] {
                            let y = c.y + k * h * 0.11;
                            p.line_segment([pos2(c.x - h * 0.2, y), pos2(c.x + h * 0.2, y)], Stroke::new(h * 0.06, INK));
                        }
                    } else {
                        // Create: a little burst of three strokes.
                        for a in [-0.6f32, 0.0, 0.6] {
                            let d = vec2(a.sin(), -a.cos());
                            p.line_segment([c + d * h * 0.06, c + d * h * 0.22], Stroke::new(h * 0.06, INK));
                        }
                    }
                }
                PadStyle::Xbox => {
                    p.circle_filled(c, r * 0.8, PLATE);
                    p.circle_stroke(c, r * 0.8, rim);
                    if options {
                        for k in [-1.0, 0.0, 1.0] {
                            let y = c.y + k * h * 0.1;
                            p.line_segment([pos2(c.x - h * 0.17, y), pos2(c.x + h * 0.17, y)], Stroke::new(h * 0.06, INK));
                        }
                    } else {
                        let s = Stroke::new(h * 0.05, INK);
                        p.rect_stroke(
                            Rect::from_center_size(c + vec2(-h * 0.05, -h * 0.05), EVec2::splat(h * 0.22)),
                            0.0,
                            s,
                            egui::StrokeKind::Middle,
                        );
                        p.rect_stroke(
                            Rect::from_center_size(c + vec2(h * 0.06, h * 0.06), EVec2::splat(h * 0.22)),
                            0.0,
                            s,
                            egui::StrokeKind::Middle,
                        );
                    }
                }
            }
        }
        Pad::Touchpad => {
            let body = rect.shrink2(vec2(h * 0.04, h * 0.14));
            p.rect_filled(body, h * 0.14, PLATE);
            p.rect_stroke(body, h * 0.14, rim, egui::StrokeKind::Inside);
            p.circle_filled(body.center(), h * 0.09, INK);
        }
    }
}

/// Draws several glyphs side by side (a key and its alternative: "W A S D", "L1 R1"), from `at`
/// (left edge, middle). Returns the space they took.
pub fn draw_all(p: &Painter, at: Pos2, glyphs: &[Glyph], h: f32, style: PadStyle) -> Rect {
    let mut x = at.x;
    let mut out = Rect::from_min_size(pos2(at.x, at.y - h * 0.5), vec2(0.0, h));
    for (i, g) in glyphs.iter().enumerate() {
        if i > 0 {
            x += h * 0.15;
        }
        let r = draw(p, pos2(x, at.y), g, h, style);
        out = out.union(r);
        x = r.right();
    }
    out
}

/// How wide `draw_all` makes `glyphs`.
pub fn width_all(p: &Painter, glyphs: &[Glyph], h: f32, style: PadStyle) -> f32 {
    glyphs.iter().map(|g| width(p, g, h, style)).sum::<f32>() + h * 0.15 * glyphs.len().saturating_sub(1) as f32
}

/// One prompt: what to press, and what it does.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub glyphs: Vec<Glyph>,
    pub label: String,
}

/// A row of prompts ("✕ Wear   □ More   ○ Close") placed by `align` at `at`, glyphs `h` tall.
/// Returns the row's rect.
pub fn row(p: &Painter, at: Pos2, align: Align2, prompts: &[Prompt], h: f32, style: PadStyle, color: Color32) -> Rect {
    let font = FontId::proportional(h * 0.68);
    let gap = h * 0.35;
    let spacer = h * 1.1;
    let galleys: Vec<_> = prompts.iter().map(|pr| p.layout_no_wrap(pr.label.clone(), font.clone(), color)).collect();
    let total: f32 = prompts
        .iter()
        .zip(&galleys)
        .map(|(pr, g)| width_all(p, &pr.glyphs, h, style) + if g.size().x > 0.0 { gap + g.size().x } else { 0.0 })
        .sum::<f32>()
        + spacer * prompts.len().saturating_sub(1) as f32;
    let rect = align.anchor_size(at, vec2(total, h));
    let mut x = rect.left();
    let y = rect.center().y;
    for (i, (pr, g)) in prompts.iter().zip(galleys).enumerate() {
        if i > 0 {
            x += spacer;
        }
        let r = draw_all(p, pos2(x, y), &pr.glyphs, h, style);
        x = r.right();
        if g.size().x > 0.0 {
            x += gap;
            let tl = pos2(x, y - g.size().y * 0.5);
            p.galley(tl + vec2(1.0, 1.0), g.clone(), Color32::from_black_alpha(200));
            p.galley(tl, g.clone(), color);
            x += g.size().x;
        }
    }
    rect
}

/// How wide `row` makes `prompts`.
pub fn row_width(p: &Painter, prompts: &[Prompt], h: f32, style: PadStyle) -> f32 {
    let font = FontId::proportional(h * 0.68);
    let gap = h * 0.35;
    prompts
        .iter()
        .map(|pr| {
            let text = p.layout_no_wrap(pr.label.clone(), font.clone(), Color32::WHITE).size().x;
            width_all(p, &pr.glyphs, h, style) + if text > 0.0 { gap + text } else { 0.0 }
        })
        .sum::<f32>()
        + h * 1.1 * prompts.len().saturating_sub(1) as f32
}

/// A row of prompts as a widget in a layout (a window's footer).
pub fn row_ui(ui: &mut egui::Ui, prompts: &[Prompt], h: f32, color: Color32) -> egui::Response {
    let style = crate::controls::current().1;
    let prompts: Vec<Prompt> = prompts.iter().filter(|p| !p.glyphs.is_empty()).cloned().collect();
    let w = row_width(ui.painter(), &prompts, h, style);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h + 4.0), egui::Sense::hover());
    row(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, &prompts, h, style, color);
    resp
}

/// A line with glyphs in it (`t_rich!`) as a widget in a layout, for the device in use.
pub fn rich_label(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) -> egui::Response {
    let style = crate::controls::current().1;
    let resolve = &crate::controls::current_glyphs;
    let w = rich_width(ui.painter(), text, size, resolve, style);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, size * 1.3), egui::Sense::hover());
    rich(ui.painter(), rect.left_center(), Align2::LEFT_CENTER, text, size, color, resolve, style);
    resp
}

/// How wide `rich` draws `text`.
pub fn rich_width(p: &Painter, text: &str, size: f32, resolve: &dyn Fn(&str) -> Vec<Glyph>, style: PadStyle) -> f32 {
    let font = FontId::proportional(size);
    let h = size * 1.25;
    runs(text)
        .into_iter()
        .map(|(action, t)| {
            if action {
                width_all(p, &resolve(t), h, style) + size * 0.3
            } else {
                p.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x
            }
        })
        .sum()
}

/// The marker `pav_core::lines::rich` writes for `{key:action}`: the action between two
/// private-use characters.
pub const MARK_OPEN: char = '\u{F8F0}';
pub const MARK_CLOSE: char = '\u{F8F1}';

/// `text` as runs of plain text and actions (from `{key:action}` markers).
pub fn runs(text: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(MARK_OPEN) {
        if open > 0 {
            out.push((false, &rest[..open]));
        }
        let after = &rest[open + MARK_OPEN.len_utf8()..];
        match after.find(MARK_CLOSE) {
            Some(close) => {
                out.push((true, &after[..close]));
                rest = &after[close + MARK_CLOSE.len_utf8()..];
            }
            None => {
                rest = after;
                break;
            }
        }
    }
    if !rest.is_empty() {
        out.push((false, rest));
    }
    out
}

/// `text` with its markers as plain names (the tools' text readings).
pub fn plain(text: &str, resolve: &dyn Fn(&str) -> Vec<Glyph>, style: PadStyle) -> String {
    runs(text)
        .into_iter()
        .map(
            |(action, t)| {
                if action { resolve(t).iter().map(|g| name(g, style)).collect::<Vec<_>>().join("/") } else { t.to_string() }
            },
        )
        .collect()
}

/// Draws a line with glyphs in it (`{key:action}` markers, resolved by `resolve`), placed by
/// `align` at `at`, in `size` text with a shadow. Returns its rect.
pub fn rich(
    p: &Painter,
    at: Pos2,
    align: Align2,
    text: &str,
    size: f32,
    color: Color32,
    resolve: &dyn Fn(&str) -> Vec<Glyph>,
    style: PadStyle,
) -> Rect {
    let font = FontId::proportional(size);
    let h = size * 1.25;
    enum Piece {
        Text(std::sync::Arc<egui::Galley>),
        Glyphs(Vec<Glyph>),
    }
    let pieces: Vec<Piece> =
        runs(text)
            .into_iter()
            .map(|(action, t)| {
                if action { Piece::Glyphs(resolve(t)) } else { Piece::Text(p.layout_no_wrap(t.to_string(), font.clone(), color)) }
            })
            .collect();
    let pad = size * 0.15;
    let total: f32 = pieces
        .iter()
        .map(|pc| match pc {
            Piece::Text(g) => g.size().x,
            Piece::Glyphs(gs) => width_all(p, gs, h, style) + 2.0 * pad,
        })
        .sum();
    let rect = align.anchor_size(at, vec2(total, h));
    let y = rect.center().y;
    let mut x = rect.left();
    for pc in pieces {
        match pc {
            Piece::Text(g) => {
                let tl = pos2(x, y - g.size().y * 0.5);
                p.galley(tl + vec2(1.0, 1.0), g.clone(), Color32::from_black_alpha(200));
                p.galley(tl, g.clone(), color);
                x += g.size().x;
            }
            Piece::Glyphs(gs) => {
                x += pad;
                let r = draw_all(p, pos2(x, y), &gs, h, style);
                x = r.right() + pad;
            }
        }
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_split_into_runs_and_read_as_names() {
        let t = format!("+3 passive points  ({MARK_OPEN}tree{MARK_CLOSE})");
        let r = runs(&t);
        assert_eq!(r, vec![(false, "+3 passive points  ("), (true, "tree"), (false, ")")]);
        let resolve = |a: &str| if a == "tree" { vec![Glyph::Pad(Pad::Triangle)] } else { vec![] };
        assert_eq!(plain(&t, &resolve, PadStyle::PlayStation), "+3 passive points  (Triangle)");
        assert_eq!(plain(&t, &resolve, PadStyle::Xbox), "+3 passive points  (Y)");
        assert_eq!(runs("plain"), vec![(false, "plain")]);
    }
}
