//! The HUD at a car's wheel (`pav_core::vehicle`): the automatic gearbox's gear, a big number
//! in a round dial whose rim fills as the car speeds up through the gear and empties at each
//! shift up. Sized to read from a sofa; the window and the tools (`headless`) draw it the same.

use std::f32::consts::PI;

use egui::{Align2, Color32, FontId, Pos2, Shape, Stroke, vec2};
use pav_core::frame::DriveHud;

const TEAL: Color32 = Color32::from_rgb(110, 220, 210);
const AMBER: Color32 = Color32::from_rgb(255, 190, 70);
const RED: Color32 = Color32::from_rgb(255, 90, 70);

/// The gear dial: in the bottom right corner, clear of the prompts along the bottom; on a touch
/// screen under the menu button (the bottom right is the thumb's buttons).
pub fn gear(ctx: &egui::Context, d: &DriveHud, touch: bool) {
    let screen = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("drive_gear")));
    let (r, c) = if touch {
        let (k, m) = crate::arpg_ui::touch_scale(screen);
        let r = 34.0 * k;
        // (The menu button: 19 points round at the corner's margin.)
        (r, Pos2::new(screen.right() - m - r, screen.top() + m + 50.0 * k + r))
    } else {
        let r = 50.0;
        (r, Pos2::new(screen.right() - 30.0 - r, screen.bottom() - 30.0 - r))
    };
    p.circle(c, r, Color32::from_black_alpha(180), Stroke::new(1.5, Color32::from_white_alpha(70)));
    // The rim: three quarters of a turn from the bottom left, round the top, to the bottom right.
    // (Amber, then red, as a shift up nears; top gear has none to come.)
    let rev = d.rpm.clamp(0.0, 1.0);
    let hot = match d.gear {
        1..=4 if rev > 0.9 => RED,
        1..=4 if rev > 0.7 => AMBER,
        _ => TEAL,
    };
    let w = r * 0.11;
    p.add(arc(c, r - w, 0.75 * PI, 1.5 * PI, Stroke::new(w, Color32::from_white_alpha(30))));
    p.add(arc(c, r - w, 0.75 * PI, 1.5 * PI * rev.max(0.03), Stroke::new(w, hot)));
    let (number, color) = if d.gear < 0 { (pav_core::t!("drive.reverse"), AMBER) } else { (d.gear.to_string(), Color32::WHITE) };
    p.text(c - vec2(0.0, r * 0.1), Align2::CENTER_CENTER, number, FontId::proportional(r * 1.1), color);
    p.text(
        c + vec2(0.0, r * 0.58),
        Align2::CENTER_CENTER,
        pav_core::t!("drive.gear"),
        FontId::proportional(r * 0.27),
        Color32::from_white_alpha(200),
    );
}

/// An arc round `c` from angle `from` (radians, clockwise on screen from the right) through
/// `sweep`.
fn arc(c: Pos2, r: f32, from: f32, sweep: f32, stroke: Stroke) -> Shape {
    let n = ((sweep.abs() * r / 4.0).ceil() as usize).clamp(2, 96);
    let points = (0..=n)
        .map(|i| {
            let a = from + sweep * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    Shape::line(points, stroke)
}
