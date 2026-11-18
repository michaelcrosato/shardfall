//! The controls in one place: which pad button, key or mouse button does what, on each device.
//! The window's input (`pav_app::input`) maps the pad with these tables and every prompt the
//! player sees reads them too, so the two never disagree.
//!
//! The controller comes first (a DualSense, or any pad the browser reports in the standard
//! mapping, on a TV at ten feet); the keyboard keeps the keys PC players expect of the genre
//! and the mouse aims and clicks. In menus, on every pad: the D-pad (or the left stick) moves
//! between items, Cross takes, Square is the item's other action, Circle goes back, L1/R1 switch
//! tabs and Options pauses. PS (button 16) and Create + Options held together are the 52in1
//! hub's: nothing here uses them.

use std::sync::atomic::{AtomicU8, Ordering};

use pav_core::input::buttons;

use crate::Device;
use crate::glyph::{Glyph, MouseButton, Pad, PadStyle};

/// Shardfall in play on a controller: the pad button for each game button bit.
pub const GAME_PAD: &[(Pad, u32)] = &[
    (Pad::Square, buttons::PRIMARY),
    (Pad::Triangle, buttons::SECONDARY),
    (Pad::Circle, buttons::SKILL3),
    (Pad::R1, buttons::SKILL4),
    (Pad::L1, buttons::SKILL5),
    (Pad::R2, buttons::SKILL6),
    (Pad::Cross, buttons::DODGE),
    (Pad::Up, buttons::POTION),
    (Pad::Right, buttons::INTERACT),
    (Pad::L2, buttons::FOCUS),
];

/// The pavilion and its rooms on a controller.
pub const ROOM_PAD: &[(Pad, u32)] = &[
    (Pad::Cross, buttons::JUMP),
    (Pad::Circle, buttons::CROUCH),
    (Pad::Triangle, buttons::CRAWL),
    (Pad::Square, buttons::USE),
    (Pad::R2, buttons::USE),
    (Pad::Right, buttons::INTERACT),
    (Pad::L2, buttons::FOCUS),
];

/// The skill bar's six slots, as game button bits (slot 0 is the basic attack).
pub const SLOT_BITS: [u32; 6] =
    [buttons::PRIMARY, buttons::SECONDARY, buttons::SKILL3, buttons::SKILL4, buttons::SKILL5, buttons::SKILL6];

/// The pad button for a game button bit, in play (`game`) or in the pavilion.
pub fn pad_for(bit: u32, game: bool) -> Option<Pad> {
    let table = if game { GAME_PAD } else { ROOM_PAD };
    table.iter().find(|(_, b)| *b == bit).map(|(p, _)| *p)
}

/// What to press for `action` on `device`. Actions are the words lines use in `{key:action}`:
/// the game's (`skill1`..`skill6`, `dodge`, `potion`, `use`, `map`, `inventory`, `character`,
/// `skills`, `tree`, `town`, `menu`, `attack_in_place`, `move`, `aim`), the pavilion's
/// (`interact`, `jump`, `crouch`, `crawl`, `bomb`, `rewind`, `tuning`, `rooms`) and the menus'
/// (`confirm`, `back`, `more`, `alt`, `tabs`, `navigate`, `scroll`). Empty on a touch screen
/// (its controls are on screen) and for an action the device has no control for.
pub fn glyphs(action: &str, device: Device) -> Vec<Glyph> {
    match device {
        Device::KeyboardMouse => keys(action),
        Device::Gamepad => pad(action).into_iter().map(Glyph::Pad).collect(),
        Device::Touch => Vec::new(),
    }
}

fn keys(action: &str) -> Vec<Glyph> {
    let k = |n: &str| vec![Glyph::key(n)];
    match action {
        "skill1" | "confirm" => vec![Glyph::Mouse(MouseButton::Left)],
        "skill2" | "more" => vec![Glyph::Mouse(MouseButton::Right)],
        "skill3" => k("Q"),
        "skill4" => k("E"),
        "skill5" => k("R"),
        "skill6" => k("F"),
        "dodge" | "jump" => k("Space"),
        "potion" => k("1"),
        "use" => k("G"),
        "interact" => k("E"),
        "map" => k("M"),
        "inventory" => k("I"),
        "character" => k("C"),
        "skills" => k("K"),
        "tree" => k("P"),
        "town" => k("T"),
        "menu" | "back" => k("Esc"),
        "attack_in_place" | "alt" => k("Shift"),
        "move" => k("WASD"),
        "aim" => vec![Glyph::Mouse(MouseButton::Move)],
        "crouch" => k("C"),
        "crawl" => k("Z"),
        "bomb" => k("F"),
        "rewind" => k("Backspace"),
        "tuning" => k("F1"),
        "rooms" => k("F2"),
        "tabs" => k("Tab"),
        "scroll" => vec![Glyph::Mouse(MouseButton::Wheel)],
        _ => Vec::new(),
    }
}

fn pad(action: &str) -> Vec<Pad> {
    let game = |bit| pad_for(bit, true).into_iter().collect::<Vec<_>>();
    match action {
        "skill1" => game(buttons::PRIMARY),
        "skill2" => game(buttons::SECONDARY),
        "skill3" => game(buttons::SKILL3),
        "skill4" => game(buttons::SKILL4),
        "skill5" => game(buttons::SKILL5),
        "skill6" => game(buttons::SKILL6),
        "dodge" => game(buttons::DODGE),
        "potion" => game(buttons::POTION),
        "use" | "interact" => vec![Pad::Right],
        "attack_in_place" => game(buttons::FOCUS),
        "map" => vec![Pad::Left],
        // The hero's panels (bag, character, skills, tree) open with D-pad down; L1/R1 turn them.
        "inventory" | "character" | "skills" | "tree" | "town" => vec![Pad::Down],
        "menu" => vec![Pad::Options],
        "move" => vec![Pad::LeftStick],
        "aim" | "scroll" => vec![Pad::RightStick],
        "confirm" => vec![Pad::Cross],
        "back" => vec![Pad::Circle],
        "more" => vec![Pad::Square],
        "alt" => vec![Pad::Triangle],
        "tabs" => vec![Pad::L1, Pad::R1],
        "navigate" => vec![Pad::DPad],
        "jump" => vec![Pad::Cross],
        "crouch" => vec![Pad::Circle],
        "crawl" => vec![Pad::Triangle],
        "bomb" => vec![Pad::Square],
        "rewind" => vec![Pad::Create],
        _ => Vec::new(),
    }
}

static DEVICE: AtomicU8 = AtomicU8::new(0);
static STYLE: AtomicU8 = AtomicU8::new(0);

/// The device the player used last and the controller's style, for prompts drawn where no one
/// passes them (the line book's `{key:action}`). The window sets them every frame.
pub fn set_current(device: Device, style: PadStyle) {
    DEVICE.store(
        match device {
            Device::KeyboardMouse => 0,
            Device::Gamepad => 1,
            Device::Touch => 2,
        },
        Ordering::Relaxed,
    );
    STYLE.store((style == PadStyle::Xbox) as u8, Ordering::Relaxed);
    pav_core::lines::set_control_label(label);
}

pub fn current() -> (Device, PadStyle) {
    let device = match DEVICE.load(Ordering::Relaxed) {
        1 => Device::Gamepad,
        2 => Device::Touch,
        _ => Device::KeyboardMouse,
    };
    let style = if STYLE.load(Ordering::Relaxed) == 1 { PadStyle::Xbox } else { PadStyle::PlayStation };
    (device, style)
}

/// The current device's glyphs for `action` (what `glyph::rich` resolves markers with).
pub fn current_glyphs(action: &str) -> Vec<Glyph> {
    glyphs(action, current().0)
}

/// `{key:action}` as plain text, for lines drawn as plain text: the keys' or buttons' names.
pub fn label(action: &str) -> String {
    let (device, style) = current();
    let g = glyphs(action, device);
    if g.is_empty() {
        return pav_core::lines::keyboard_label(action);
    }
    g.iter().map(|g| crate::glyph::name(g, style)).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_bit_has_one_pad_button_and_prompts_match_it() {
        let mut seen = std::collections::BTreeSet::new();
        for (p, bit) in GAME_PAD {
            assert!(seen.insert(*bit), "bit {bit} bound twice");
            assert_ne!(*p, Pad::Options, "Options is the menu");
            assert_ne!(*p, Pad::Create, "Create + Options is the hub's");
        }
        for (i, bit) in SLOT_BITS.iter().enumerate() {
            let want = pad_for(*bit, true).map(Glyph::Pad);
            assert_eq!(glyphs(&format!("skill{}", i + 1), Device::Gamepad).first().cloned(), want);
        }
        assert_eq!(glyphs("dodge", Device::Gamepad), vec![Glyph::Pad(Pad::Cross)]);
        assert_eq!(glyphs("dodge", Device::KeyboardMouse), vec![Glyph::key("Space")]);
        assert!(glyphs("dodge", Device::Touch).is_empty());
    }

    #[test]
    fn line_labels_follow_the_device_and_style() {
        set_current(Device::Gamepad, PadStyle::PlayStation);
        assert_eq!(label("confirm"), "Cross");
        set_current(Device::Gamepad, PadStyle::Xbox);
        assert_eq!(label("confirm"), "A");
        assert_eq!(label("tabs"), "LB/RB");
        set_current(Device::KeyboardMouse, PadStyle::Xbox);
        assert_eq!(label("tree"), "P");
        set_current(Device::KeyboardMouse, PadStyle::PlayStation);
    }
}
