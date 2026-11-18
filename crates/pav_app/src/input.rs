//! Devices -> actions. Keyboard keys are bound by physical position (WASD stays WASD on AZERTY).
//! The fixed system layer (pause, rewind, menus) is handled in `app.rs` and never rebinds.

use std::collections::HashSet;

use glam::Vec2;
use pav_core::input::buttons;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

pub use pav_ui::Device;

/// Game-action bindings (defaults; rooms may override in the future).
pub struct KeyBinds {
    pub up: &'static [KeyCode],
    pub down: &'static [KeyCode],
    pub left: &'static [KeyCode],
    pub right: &'static [KeyCode],
    pub buttons: &'static [(u32, &'static [KeyCode])],
}

pub const KEYS: KeyBinds = KeyBinds {
    up: &[KeyCode::KeyW, KeyCode::ArrowUp],
    down: &[KeyCode::KeyS, KeyCode::ArrowDown],
    left: &[KeyCode::KeyA, KeyCode::ArrowLeft],
    right: &[KeyCode::KeyD, KeyCode::ArrowRight],
    buttons: &[
        (buttons::JUMP, &[KeyCode::Space]),
        (buttons::CROUCH, &[KeyCode::KeyC, KeyCode::ControlLeft]),
        (buttons::CRAWL, &[KeyCode::KeyZ]),
        (buttons::USE, &[KeyCode::KeyF]),
        (buttons::FOCUS, &[KeyCode::ShiftLeft]),
        (buttons::INTERACT, &[KeyCode::KeyE]),
    ],
};

/// Physical key names usable in room files (`[keys]`).
pub fn key_from_name(n: &str) -> Option<KeyCode> {
    use KeyCode::*;
    let letters = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
        KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    if let Some(c) = n.strip_prefix("Key").and_then(|l| l.chars().next()).filter(|c| c.is_ascii_uppercase() && n.len() == 4) {
        return Some(letters[(c as u8 - b'A') as usize]);
    }
    let digits = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
    if let Some(d) = n.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()).filter(|d| *d < 10) {
        return Some(digits[d]);
    }
    Some(match n {
        "Space" => Space,
        "Enter" => Enter,
        "Tab" => Tab,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "AltLeft" => AltLeft,
        "AltRight" => AltRight,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "Comma" => Comma,
        "Period" => Period,
        "Slash" => Slash,
        "Semicolon" => Semicolon,
        "Quote" => Quote,
        "BracketLeft" => BracketLeft,
        "BracketRight" => BracketRight,
        "Minus" => Minus,
        "Equal" => Equal,
        "CapsLock" => CapsLock,
        _ => return None,
    })
}

/// Current keyboard bindings: defaults, optionally overridden by the room.
#[derive(Clone)]
pub struct Bindings {
    pub up: Vec<KeyCode>,
    pub down: Vec<KeyCode>,
    pub left: Vec<KeyCode>,
    pub right: Vec<KeyCode>,
    pub buttons: Vec<(u32, Vec<KeyCode>)>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            up: KEYS.up.to_vec(),
            down: KEYS.down.to_vec(),
            left: KEYS.left.to_vec(),
            right: KEYS.right.to_vec(),
            buttons: KEYS.buttons.iter().map(|(b, k)| (*b, k.to_vec())).collect(),
        }
    }
}

impl Bindings {
    /// Shardfall: WASD to move, mouse buttons and Q E R F for the six skill slots, Space to
    /// dodge, 1 for a potion, Shift to attack in place.
    pub fn game() -> Self {
        use KeyCode::*;
        Self {
            up: vec![KeyW, ArrowUp],
            down: vec![KeyS, ArrowDown],
            left: vec![KeyA, ArrowLeft],
            right: vec![KeyD, ArrowRight],
            buttons: vec![
                (buttons::SKILL3, vec![KeyQ]),
                (buttons::SKILL4, vec![KeyE]),
                (buttons::SKILL5, vec![KeyR]),
                (buttons::SKILL6, vec![KeyF]),
                (buttons::DODGE, vec![Space]),
                (buttons::POTION, vec![Digit1]),
                (buttons::FOCUS, vec![ShiftLeft]),
                (buttons::INTERACT, vec![KeyG]),
            ],
        }
    }

    /// Defaults with a room's `[keys]` overrides applied. Returns problems found.
    pub fn with_overrides(map: &std::collections::BTreeMap<String, Vec<String>>) -> (Self, Vec<String>) {
        let mut b = Self::default();
        let mut errs = Vec::new();
        for (action, names) in map {
            let keys: Vec<KeyCode> = names
                .iter()
                .filter_map(|n| {
                    let k = key_from_name(n);
                    if k.is_none() {
                        errs.push(format!("unknown key '{n}'"));
                    }
                    k
                })
                .collect();
            match action.as_str() {
                "move_up" => b.up = keys,
                "move_down" => b.down = keys,
                "move_left" => b.left = keys,
                "move_right" => b.right = keys,
                other => match pav_core::input::buttons::from_name(other) {
                    Some(bit) => match b.buttons.iter_mut().find(|(x, _)| *x == bit) {
                        Some(e) => e.1 = keys,
                        None => b.buttons.push((bit, keys)),
                    },
                    None => errs.push(format!("unknown action '{other}'")),
                },
            }
        }
        (b, errs)
    }
}

/// Shardfall's touch controls, in the language in use: what each does, and the gesture or
/// on-screen control for it (game lines `controls.*`). Keys and pads: `game_rows`.
pub fn touch_guide() -> Vec<(String, String)> {
    let rows: &[(&str, &str)] = &[
        ("controls.move", "controls.touch.move"),
        ("controls.attack", "controls.touch.attack"),
        ("controls.skills", "controls.touch.skills"),
        ("controls.dodge_potion", "controls.touch.dodge_potion"),
        ("controls.use", "controls.touch.use"),
        ("controls.touch_panels", "controls.touch.panels"),
        ("controls.swipe", "controls.touch.swipe"),
        ("controls.close", "controls.touch.close"),
    ];
    rows.iter().map(|(what, how)| (pav_core::t!(what), pav_core::t!(how))).collect()
}

/// Shardfall's controls on a keyboard and mouse or a controller, one action to a row: what to
/// press (drawn as glyphs) and what it does. The pause menu's Controls page and the guide panel
/// show these; they read `pav_ui::controls`, as the input does.
pub fn game_rows(device: Device) -> Vec<(Vec<pav_ui::glyph::Glyph>, String)> {
    let g = |a: &str| pav_ui::controls::glyphs(a, device);
    let all = |acts: &[&str]| acts.iter().flat_map(|a| pav_ui::controls::glyphs(a, device)).collect::<Vec<_>>();
    let t = |id: &str| pav_core::t!(id);
    let mut rows = vec![
        (g("move"), t("controls.move")),
        (g("aim"), t("controls.aim")),
        (all(&["skill1", "skill2", "skill3", "skill4", "skill5", "skill6"]), t("controls.skills")),
        (g("dodge"), t("controls.dodge")),
        (g("potion"), t("controls.potion")),
        (g("use"), t("controls.use")),
        (g("map"), t("controls.map")),
        (g("attack_in_place"), t("controls.attack_in_place")),
    ];
    if device == Device::Gamepad {
        rows.extend([
            (g("inventory"), t("controls.pad_panels")),
            (g("menu"), t("controls.menu")),
            (g("navigate"), t("controls.menu_move")),
            (g("confirm"), t("controls.menu_take")),
            (g("more"), t("controls.menu_more")),
            (g("back"), t("controls.menu_back")),
            (g("tabs"), t("controls.menu_tabs")),
        ]);
    } else {
        rows.extend([
            (all(&["inventory", "tree", "character", "skills"]), t("controls.panels")),
            (g("town"), t("controls.town_portal")),
            (g("menu"), t("controls.menu")),
        ]);
    }
    rows
}

/// The pavilion's controls (its rooms, the plaza), as `game_rows`: the engine tour's words stay
/// English (its signs and guides are the engine's demo).
pub fn room_rows(device: Device) -> Vec<(Vec<pav_ui::glyph::Glyph>, String)> {
    let g = |a: &str| pav_ui::controls::glyphs(a, device);
    let mut rows: Vec<(Vec<pav_ui::glyph::Glyph>, String)> = [
        ("move", "Move"),
        ("jump", "Jump (hold for higher)"),
        ("crouch", "Crouch"),
        ("crawl", "Crawl"),
        ("bomb", "Throw a bomb"),
        ("interact", "Use: a plaza station, a vehicle"),
        ("rewind", "Rewind (hold)"),
    ]
    .iter()
    .map(|(a, what)| (g(a), what.to_string()))
    .collect();
    if device == Device::Gamepad {
        rows.push((
            vec![pav_ui::glyph::Glyph::Pad(PadButton::L1), pav_ui::glyph::Glyph::Pad(PadButton::R1)],
            "Turn the camera".into(),
        ));
        rows.push((g("menu"), pav_core::t!("controls.menu")));
    } else {
        rows.push((vec![pav_ui::glyph::Glyph::Mouse(pav_ui::glyph::MouseButton::Wheel)], "Zoom the camera".into()));
        rows.push((g("tuning"), "Tuning panel".into()));
        rows.push((g("rooms"), "Go to any room".into()));
        rows.push((g("menu"), pav_core::t!("controls.menu")));
    }
    rows
}

#[derive(Default)]
pub struct Pad {
    pub left: Vec2,
    pub right: Vec2,
    pub held: u32,
    pub rotate: f32,
    pub zoom: f32,
    /// Options: the pause menu.
    pub start_pressed: bool,
    /// D-pad left, or the touchpad's press: the game's map.
    pub map_pressed: bool,
    /// Buttons pressed since the last poll (menus read these as taps).
    pub taps: Vec<PadButton>,
    /// Cross and Square held: the menus' left and right clicks. Triangle: Shift (take a whole
    /// path in the passive tree).
    pub cross_held: bool,
    pub square_held: bool,
    pub triangle_held: bool,
    pub back_held: bool,
    /// L2 and R2 as far as they are pulled, 0 to 1 (analog on every pad that has them).
    pub l2: f32,
    pub r2: f32,
    pub any_activity: bool,
}

pub use pav_ui::glyph::Pad as PadButton;

/// gilrs's name for a button, as the standard mapping's (and the prompts') `Pad`. The PS
/// button (gilrs's Mode, the standard mapping's 16) is 52in1's: it maps to nothing.
fn pad_button(b: gilrs::Button) -> Option<PadButton> {
    use gilrs::Button as B;
    Some(match b {
        B::South => PadButton::Cross,
        B::East => PadButton::Circle,
        B::West => PadButton::Square,
        B::North => PadButton::Triangle,
        B::LeftTrigger => PadButton::L1,
        B::RightTrigger => PadButton::R1,
        B::LeftTrigger2 => PadButton::L2,
        B::RightTrigger2 => PadButton::R2,
        B::Select => PadButton::Create,
        B::Start => PadButton::Options,
        B::LeftThumb => PadButton::L3,
        B::RightThumb => PadButton::R3,
        B::DPadUp => PadButton::Up,
        B::DPadDown => PadButton::Down,
        B::DPadLeft => PadButton::Left,
        B::DPadRight => PadButton::Right,
        _ => return None,
    })
}

pub struct Input {
    pub keys: HashSet<KeyCode>,
    pub mouse: HashSet<MouseButton>,
    pub cursor: Vec2,
    pub mouse_active: bool,
    pub gilrs: Option<gilrs::Gilrs>,
    /// A scripted gamepad (`--pad-script FILE`): tests and agents drive controller flows.
    pub pad_script: Option<PadScript>,
    /// The other controllers this frame (`ExtraPad`), and the ones local players hold
    /// (`crate::couch` sets it: the player's own pad is the first one not among them).
    pub extra_pads: Vec<ExtraPad>,
    pub couch_pads: Vec<usize>,
    pub pad: Pad,
    pub last_device: Device,
    prev_held: u32,
    /// Keys / mouse buttons pressed since the last `buttons()` call (short taps are never lost).
    key_taps: HashSet<KeyCode>,
    pub mouse_taps: HashSet<MouseButton>,
    pub bindings: Bindings,
    /// Game mode: the right mouse button is a skill and the gamepad uses the game layout.
    pub game: bool,
}

/// A pad's raw state for one frame.
#[derive(Clone, Debug, Default)]
pub struct PadState {
    pub left: Vec2,
    pub right: Vec2,
    pub held: Vec<PadButton>,
    pub l2: f32,
    pub r2: f32,
}

/// Another controller than the player's own: a local player's in same-screen co-op
/// (`crate::couch`), or one waiting to join. Sticks with their dead zones.
#[derive(Clone, Debug, Default)]
pub struct ExtraPad {
    /// gilrs's id for it (stable while it stays connected).
    pub id: usize,
    pub state: PadState,
    /// Buttons pressed since the last poll.
    pub taps: Vec<PadButton>,
}

/// One connected pad's state (gilrs).
fn read_state(gp: &gilrs::Gamepad) -> PadState {
    use gilrs::{Axis, Button as B};
    const ALL: [B; 16] = [
        B::South,
        B::East,
        B::North,
        B::West,
        B::LeftTrigger,
        B::RightTrigger,
        B::Select,
        B::Start,
        B::DPadUp,
        B::DPadDown,
        B::DPadLeft,
        B::DPadRight,
        B::LeftThumb,
        B::RightThumb,
        B::LeftTrigger2,
        B::RightTrigger2,
    ];
    let value = |b: B| gp.button_data(b).map(|d| d.value()).unwrap_or(0.0);
    let (l2, r2) = (value(B::LeftTrigger2), value(B::RightTrigger2));
    PadState {
        left: Vec2::new(gp.value(Axis::LeftStickX), gp.value(Axis::LeftStickY)),
        right: Vec2::new(gp.value(Axis::RightStickX), gp.value(Axis::RightStickY)),
        held: ALL
            .into_iter()
            .filter(|b| match b {
                // The triggers count as held once pulled past a third.
                B::LeftTrigger2 => l2 > TRIGGER,
                B::RightTrigger2 => r2 > TRIGGER,
                b => gp.is_pressed(*b),
            })
            .filter_map(pad_button)
            .collect(),
        l2,
        r2,
    }
}

/// One line of a pad script: hold this for so many frames.
#[derive(Clone, Debug, Default)]
pub struct PadStep {
    pub frames: u32,
    /// Which controller: 1 the player's own, 2-4 more (local players in same-screen co-op).
    pub pad: u8,
    pub left: Vec2,
    pub right: Vec2,
    pub hold: Vec<PadButton>,
    pub tap: Vec<PadButton>,
}

/// A scripted gamepad, one step per line:
/// `<frames> [pad=N] [left=x,y] [right=x,y] [hold=Cross,Triangle] [tap=Down]` (`#` comments).
/// `pad=2` to `pad=4` drive more controllers (they stay connected, as last set, for the rest
/// of the script); without it a step is the player's own pad.
/// Buttons by the DualSense's names (Cross, Circle, Square, Triangle, L1, R1, L2, R2, Create,
/// Options, L3, R3, Up, Down, Left, Right, Touchpad) or the older gilrs ones (South or A, East
/// or B, West or X, North or Y, LeftTrigger or LB, RightTrigger or RB, Start, Select,
/// DPadUp/Down/Left/Right).
#[derive(Clone, Debug, Default)]
pub struct PadScript {
    pub steps: Vec<PadStep>,
    at: usize,
    frame: u32,
    /// The other controllers' states (by pad number), and their presses this frame.
    others: std::collections::BTreeMap<u8, PadState>,
    taps: Vec<(u8, Vec<PadButton>)>,
}

impl PadScript {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn parse(text: &str) -> Result<PadScript, String> {
        let button = |n: &str| -> Result<PadButton, String> {
            use PadButton as P;
            Ok(match n {
                "Cross" | "South" | "A" => P::Cross,
                "Circle" | "East" | "B" => P::Circle,
                "Square" | "West" | "X" => P::Square,
                "Triangle" | "North" | "Y" => P::Triangle,
                "L1" | "LeftTrigger" | "LB" => P::L1,
                "R1" | "RightTrigger" | "RB" => P::R1,
                "L2" | "LT" => P::L2,
                "R2" | "RT" => P::R2,
                "Options" | "Start" => P::Options,
                "Create" | "Select" => P::Create,
                "L3" => P::L3,
                "R3" => P::R3,
                "Up" | "DPadUp" => P::Up,
                "Down" | "DPadDown" => P::Down,
                "Left" | "DPadLeft" => P::Left,
                "Right" | "DPadRight" => P::Right,
                "Touchpad" => P::Touchpad,
                o => return Err(format!("unknown button '{o}'")),
            })
        };
        let vec = |v: &str| -> Result<Vec2, String> {
            let (x, y) = v.split_once(',').ok_or(format!("'{v}': want x,y"))?;
            Ok(Vec2::new(x.trim().parse().map_err(|_| format!("'{v}'"))?, y.trim().parse().map_err(|_| format!("'{v}'"))?))
        };
        let mut steps = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut words = line.split_whitespace();
            let frames = words.next().and_then(|w| w.parse().ok()).ok_or(format!("line {}: starts with a frame count", n + 1))?;
            let mut st = PadStep { frames, pad: 1, ..Default::default() };
            for w in words {
                let (k, v) = w.split_once('=').ok_or(format!("line {}: '{w}'", n + 1))?;
                match k {
                    "pad" => {
                        st.pad = v.parse().ok().filter(|n| (1..=4).contains(n)).ok_or(format!("line {}: pad is 1 to 4", n + 1))?
                    }
                    "left" => st.left = vec(v)?,
                    "right" => st.right = vec(v)?,
                    "hold" => st.hold = v.split(',').map(button).collect::<Result<_, _>>()?,
                    "tap" => st.tap = v.split(',').map(button).collect::<Result<_, _>>()?,
                    o => return Err(format!("line {}: unknown '{o}'", n + 1)),
                }
            }
            steps.push(st);
        }
        Ok(PadScript { steps, ..Default::default() })
    }

    /// This frame's step (and whether it's the step's first frame); None when done.
    fn next(&mut self) -> Option<(PadStep, bool)> {
        let st = self.steps.get(self.at)?.clone();
        let first = self.frame == 0;
        self.frame += 1;
        if self.frame >= st.frames.max(1) {
            self.at += 1;
            self.frame = 0;
        }
        Some((st, first))
    }
}

/// What the page reads of a controller that gilrs doesn't (the browser build; empty natively).
#[derive(Clone, Debug, Default)]
pub struct PadExtra {
    /// The DualSense's touchpad was pressed (standard mapping's button 17).
    pub touchpad_pressed: bool,
    /// A swipe across the touchpad: -1 left, 1 right (52in1's DualSense touches).
    pub swipe: i32,
}

/// A trigger counts as held past this much of its travel.
const TRIGGER: f32 = 0.35;

fn deadzone(v: Vec2, dz: f32) -> Vec2 {
    let l = v.length();
    if l < dz { Vec2::ZERO } else { v / l * ((l - dz) / (1.0 - dz)).min(1.0) }
}

impl Input {
    pub fn new() -> Self {
        let gilrs = match gilrs::Gilrs::new() {
            Ok(g) => {
                for (_, gp) in g.gamepads() {
                    log::info!("gamepad: {} ({:?})", gp.name(), gp.power_info());
                }
                Some(g)
            }
            Err(e) => {
                log::warn!("gamepads unavailable: {e}");
                None
            }
        };
        Self {
            keys: HashSet::new(),
            mouse: HashSet::new(),
            cursor: Vec2::ZERO,
            mouse_active: false,
            gilrs,
            pad_script: None,
            extra_pads: Vec::new(),
            couch_pads: Vec::new(),
            pad: Pad::default(),
            last_device: Device::KeyboardMouse,
            prev_held: 0,
            key_taps: HashSet::new(),
            mouse_taps: HashSet::new(),
            bindings: Bindings::default(),
            game: false,
        }
    }

    pub fn key(&mut self, code: KeyCode, state: ElementState) {
        self.last_device = Device::KeyboardMouse;
        match state {
            ElementState::Pressed => {
                self.keys.insert(code);
                self.key_taps.insert(code);
            }
            ElementState::Released => {
                self.keys.remove(&code);
            }
        }
    }

    pub fn mouse_button(&mut self, b: MouseButton, state: ElementState) {
        self.last_device = Device::KeyboardMouse;
        self.mouse_active = true;
        match state {
            ElementState::Pressed => {
                self.mouse.insert(b);
                self.mouse_taps.insert(b);
            }
            ElementState::Released => {
                self.mouse.remove(&b);
            }
        }
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.mouse.clear();
    }

    /// Reads gamepad events and state (the first connected pad). `extra` is what the page reads
    /// that gilrs doesn't: the DualSense's touchpad press (button 17).
    pub fn poll_gamepad(&mut self, extra: &PadExtra) {
        let mut activity = false;
        let mut taps = Vec::new();
        // One pad state from the first connected pad, or from the pad script while it runs.
        let mut state: Option<PadState> = None;
        self.extra_pads.clear();
        if let Some(g) = &mut self.gilrs {
            // Presses and pushes by pad: the player's own pad drives them and the menus; the
            // others are local players' (same-screen co-op), or waiting to join.
            let mut pad_taps: std::collections::BTreeMap<usize, Vec<PadButton>> = Default::default();
            let mut moved = std::collections::BTreeSet::new();
            while let Some(ev) = g.next_event() {
                let pid = usize::from(ev.id);
                match ev.event {
                    gilrs::EventType::ButtonPressed(b, _) => {
                        if let Some(p) = pad_button(b) {
                            pad_taps.entry(pid).or_default().push(p);
                        }
                    }
                    gilrs::EventType::AxisChanged(_, v, _) if v.abs() > 0.4 => {
                        moved.insert(pid);
                    }
                    gilrs::EventType::Connected => log::info!("gamepad connected"),
                    gilrs::EventType::Disconnected => log::info!("gamepad disconnected"),
                    _ => {}
                }
            }
            let primary = g.gamepads().map(|(id, _)| usize::from(id)).find(|id| !self.couch_pads.contains(id));
            for (id, gp) in g.gamepads() {
                let pid = usize::from(id);
                let mine = pad_taps.remove(&pid).unwrap_or_default();
                if Some(pid) == primary {
                    activity |= !mine.is_empty() || moved.contains(&pid);
                    taps.extend(mine);
                    state = Some(read_state(&gp));
                } else {
                    let mut st = read_state(&gp);
                    st.left = deadzone(st.left, 0.15);
                    st.right = deadzone(st.right, 0.2);
                    self.extra_pads.push(ExtraPad { id: pid, state: st, taps: mine });
                }
            }
        }
        if extra.touchpad_pressed {
            taps.push(PadButton::Touchpad);
            activity = true;
        }
        if let Some(script) = &mut self.pad_script {
            if let Some((st, first)) = script.next() {
                activity = true;
                let trig = |b| if st.hold.contains(&b) { 1.0 } else { 0.0 };
                let (l2, r2) = (trig(PadButton::L2), trig(PadButton::R2));
                let pad = PadState { left: st.left, right: st.right, held: st.hold.clone(), l2, r2 };
                if st.pad <= 1 {
                    if first {
                        taps.extend(st.tap.iter().copied());
                    }
                    state = Some(pad);
                } else {
                    // Another controller: it keeps this state until its next step.
                    script.others.insert(st.pad, pad);
                    if first {
                        script.taps.push((st.pad, st.tap.clone()));
                    }
                    state = Some(PadState::default());
                }
            }
            let taps_now = std::mem::take(&mut script.taps);
            for (n, st) in &script.others {
                let taps: Vec<PadButton> = taps_now.iter().filter(|(k, _)| k == n).flat_map(|(_, t)| t.clone()).collect();
                self.extra_pads.push(ExtraPad { id: 1000 + *n as usize, state: st.clone(), taps });
            }
        }
        // Create and Options held together open 52in1's bar: that pair means nothing here.
        let hub_pair =
            state.as_ref().is_some_and(|st| st.held.contains(&PadButton::Create) && st.held.contains(&PadButton::Options));
        if hub_pair {
            taps.retain(|b| !matches!(b, PadButton::Options | PadButton::Create));
        }
        let start = taps.contains(&PadButton::Options);
        let map = taps.iter().any(|b| matches!(b, PadButton::Left | PadButton::Touchpad));
        let mut pad = Pad { start_pressed: start, map_pressed: map, taps, any_activity: activity, ..Default::default() };
        if let Some(st) = state {
            let is = |b: PadButton| st.held.contains(&b);
            // About 0.15 of the stick's travel is dead (the hub's and every console's habit).
            pad.left = deadzone(st.left, 0.15);
            pad.right = deadzone(st.right, 0.2);
            let table = if self.game { pav_ui::controls::GAME_PAD } else { pav_ui::controls::ROOM_PAD };
            let mut held = 0;
            for (btn, action) in table {
                if is(*btn) {
                    held |= action;
                }
            }
            pad.held = held;
            pad.l2 = st.l2;
            pad.r2 = st.r2;
            pad.rotate = if self.game { 0.0 } else { (is(PadButton::R1) as i32 - is(PadButton::L1) as i32) as f32 };
            // In the game the D-pad is potion / use / map / panels, not zoom.
            pad.zoom = if self.game { 0.0 } else { (is(PadButton::Down) as i32 - is(PadButton::Up) as i32) as f32 };
            pad.cross_held = is(PadButton::Cross);
            pad.square_held = is(PadButton::Square);
            pad.triangle_held = is(PadButton::Triangle);
            pad.back_held = is(PadButton::Create) && !hub_pair;
            if pad.left.length() > 0.3 || pad.right.length() > 0.3 || held != 0 {
                pad.any_activity = true;
            }
        }
        if pad.any_activity {
            self.last_device = Device::Gamepad;
        }
        self.pad = pad;
    }

    /// Screen-space movement (x right, y up) from keys and stick, length <= 1.
    pub fn move_axis(&self) -> Vec2 {
        let k = |codes: &[KeyCode]| codes.iter().any(|c| self.keys.contains(c)) as i32 as f32;
        let b = &self.bindings;
        let kb = Vec2::new(k(&b.right) - k(&b.left), k(&b.up) - k(&b.down));
        let v = if kb != Vec2::ZERO { kb.normalize() } else { self.pad.left };
        v.clamp_length_max(1.0)
    }

    /// Held game buttons from all devices.
    pub fn held(&self) -> u32 {
        let mut b = self.pad.held;
        for (bit, codes) in &self.bindings.buttons {
            if codes.iter().any(|c| self.keys.contains(c)) {
                b |= bit;
            }
        }
        if self.mouse.contains(&MouseButton::Left) {
            b |= buttons::PRIMARY;
        }
        if self.game && self.mouse.contains(&MouseButton::Right) {
            b |= buttons::SECONDARY;
        }
        b
    }

    /// Returns (held, newly pressed since the last call).
    pub fn buttons(&mut self) -> (u32, u32) {
        let held = self.held();
        let mut pressed = held & !self.prev_held;
        for (bit, codes) in &self.bindings.buttons {
            if codes.iter().any(|c| self.key_taps.contains(c)) {
                pressed |= bit;
            }
        }
        if self.mouse_taps.contains(&MouseButton::Left) {
            pressed |= buttons::PRIMARY;
        }
        if self.game && self.mouse_taps.contains(&MouseButton::Right) {
            pressed |= buttons::SECONDARY;
        }
        self.key_taps.clear();
        self.mouse_taps.clear();
        self.prev_held = held;
        // A tap shorter than a frame still counts as held for that frame.
        (held | pressed, pressed)
    }

    /// True if `b` was pressed since the last call (consumes the tap).
    pub fn take_mouse_tap(&mut self, b: MouseButton) -> bool {
        self.mouse_taps.remove(&b)
    }

    pub fn rewind_held(&self) -> bool {
        self.keys.contains(&KeyCode::Backspace) || self.pad.back_held
    }
}

#[cfg(test)]
mod pad_script_tests {
    use super::*;

    #[test]
    fn pad_scripts_parse_and_play() {
        let mut p = PadScript::parse("# open the bag\n2\n1 tap=DPadDown\n3 left=0.5,-1 hold=A,Y\n").unwrap();
        assert_eq!(p.steps.len(), 3);
        let frames: Vec<(PadStep, bool)> = std::iter::from_fn(|| p.next()).collect();
        assert_eq!(frames.len(), 6);
        assert!(frames[2].1 && frames[2].0.tap == vec![PadButton::Down]);
        assert_eq!(frames[3].0.hold, vec![PadButton::Cross, PadButton::Triangle]);
        let named = PadScript::parse("1 hold=Cross,R2 tap=Options\n2 pad=2 tap=Cross\n").unwrap();
        assert_eq!(named.steps[0].hold, vec![PadButton::Cross, PadButton::R2]);
        assert_eq!((named.steps[0].pad, named.steps[1].pad), (1, 2));
        assert!(PadScript::parse("1 pad=7").is_err());
        assert_eq!(frames[5].0.left, Vec2::new(0.5, -1.0));
        assert!(PadScript::parse("x tap=A").is_err());
        assert!(PadScript::parse("1 tap=Q").is_err());
    }
}
