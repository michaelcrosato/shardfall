//! Devices -> actions. Keyboard keys are bound by physical position (WASD stays WASD on AZERTY).
//! The fixed system layer (pause, rewind, menus) is handled in `app.rs` and never rebinds.

use std::collections::HashSet;

use glam::Vec2;
use pav_core::input::buttons;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    KeyboardMouse,
    Gamepad,
}

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

/// Human-readable control guide per device.
pub fn guide(device: Device) -> &'static [(&'static str, &'static str)] {
    match device {
        Device::KeyboardMouse => &[
            ("Move", "W A S D"),
            ("Jump", "Space (hold = higher)"),
            ("Crouch", "C / Ctrl (hold)"),
            ("Crawl", "Z (toggle)"),
            ("Throw bomb", "F or left click (at the cursor)"),
            ("Walk slowly", "Shift (instant model)"),
            ("Climb ladder", "walk into it"),
            ("Camera", "right-drag rotate · wheel zoom · 1–8 presets"),
            ("Rewind", "hold Backspace"),
            ("Menu / tuning", "Esc / F1"),
        ],
        Device::Gamepad => &[
            ("Move", "left stick"),
            ("Aim", "right stick"),
            ("Jump", "A (hold = higher)"),
            ("Crouch", "B (hold)"),
            ("Crawl", "Y (toggle)"),
            ("Throw bomb", "X or right trigger"),
            ("Walk slowly", "left trigger (instant model)"),
            ("Camera", "LB / RB rotate · D-pad zoom"),
            ("Rewind", "hold Back/View"),
            ("Menu", "Start"),
        ],
    }
}

#[derive(Default)]
pub struct Pad {
    pub left: Vec2,
    pub right: Vec2,
    pub held: u32,
    pub rotate: f32,
    pub zoom: f32,
    pub start_pressed: bool,
    pub back_held: bool,
    pub any_activity: bool,
}

pub struct Input {
    pub keys: HashSet<KeyCode>,
    pub mouse: HashSet<MouseButton>,
    pub cursor: Vec2,
    pub mouse_active: bool,
    pub gilrs: Option<gilrs::Gilrs>,
    pub pad: Pad,
    pub last_device: Device,
    prev_held: u32,
}

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
            pad: Pad::default(),
            last_device: Device::KeyboardMouse,
            prev_held: 0,
        }
    }

    pub fn key(&mut self, code: KeyCode, state: ElementState) {
        self.last_device = Device::KeyboardMouse;
        match state {
            ElementState::Pressed => {
                self.keys.insert(code);
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

    /// Reads gamepad events and state (first connected pad).
    pub fn poll_gamepad(&mut self) {
        let Some(g) = &mut self.gilrs else { return };
        let mut activity = false;
        let mut start = false;
        while let Some(ev) = g.next_event() {
            match ev.event {
                gilrs::EventType::ButtonPressed(gilrs::Button::Start, _) => {
                    start = true;
                    activity = true;
                }
                gilrs::EventType::ButtonPressed(..) => activity = true,
                gilrs::EventType::AxisChanged(_, v, _) if v.abs() > 0.4 => activity = true,
                gilrs::EventType::Connected => log::info!("gamepad connected"),
                gilrs::EventType::Disconnected => log::info!("gamepad disconnected"),
                _ => {}
            }
        }
        let mut pad = Pad { start_pressed: start, any_activity: activity, ..Default::default() };
        if let Some((_, gp)) = g.gamepads().next() {
            use gilrs::{Axis, Button};
            pad.left = deadzone(Vec2::new(gp.value(Axis::LeftStickX), gp.value(Axis::LeftStickY)), 0.18);
            pad.right = deadzone(Vec2::new(gp.value(Axis::RightStickX), gp.value(Axis::RightStickY)), 0.25);
            let trig = |b: Button| gp.button_data(b).map(|d| d.value()).unwrap_or(0.0) > 0.35;
            let mut held = 0;
            for (btn, action) in [
                (Button::South, buttons::JUMP),
                (Button::East, buttons::CROUCH),
                (Button::North, buttons::CRAWL),
                (Button::West, buttons::USE),
            ] {
                if gp.is_pressed(btn) {
                    held |= action;
                }
            }
            if trig(Button::RightTrigger2) {
                held |= buttons::USE;
            }
            if trig(Button::LeftTrigger2) {
                held |= buttons::FOCUS;
            }
            pad.held = held;
            pad.rotate = (gp.is_pressed(Button::RightTrigger) as i32 - gp.is_pressed(Button::LeftTrigger) as i32) as f32;
            pad.zoom = (gp.is_pressed(Button::DPadDown) as i32 - gp.is_pressed(Button::DPadUp) as i32) as f32;
            pad.back_held = gp.is_pressed(Button::Select);
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
        let kb = Vec2::new(k(KEYS.right) - k(KEYS.left), k(KEYS.up) - k(KEYS.down));
        let v = if kb != Vec2::ZERO { kb.normalize() } else { self.pad.left };
        v.clamp_length_max(1.0)
    }

    /// Held game buttons from all devices.
    pub fn held(&self) -> u32 {
        let mut b = self.pad.held;
        for (bit, codes) in KEYS.buttons {
            if codes.iter().any(|c| self.keys.contains(c)) {
                b |= bit;
            }
        }
        if self.mouse.contains(&MouseButton::Left) {
            b |= buttons::PRIMARY;
        }
        b
    }

    /// Returns (held, newly pressed since the last call).
    pub fn buttons(&mut self) -> (u32, u32) {
        let held = self.held();
        let pressed = held & !self.prev_held;
        self.prev_held = held;
        (held, pressed)
    }

    pub fn rewind_held(&self) -> bool {
        self.keys.contains(&KeyCode::Backspace) || self.pad.back_held
    }
}
