//! Touch screens: phones and tablets.
//!
//! - A thumb anywhere on the open game is the move stick. It is invisible until touched; while
//!   held a faint ring shows where it started, and it follows a thumb that runs past its edge.
//! - The other thumb has a fan of round buttons in the corner: skills (tap: at the nearest foe;
//!   drag: aim by hand and release to cast, drag back onto the button to cancel; hold: keep
//!   casting), dodge, potion, and a "use" button that appears only when something is in reach.
//! - Attacks are automatic: the first skill strikes the nearest foe in reach, and while the
//!   stick is still the hero steps in to one that is fighting it ([`pick_target`]).
//! - Temporary things on screen (messages, banners, notes, the minimap) are swiped away
//!   ([`Swipes`]); a tap on the open game closes windows.
//! - A touch that lands on a window goes to egui as its pointer.
//!
//! Positions are egui points (CSS pixels in the browser).

use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2 as EVec2};
use glam::{Vec2, Vec3};
use pav_core::arpg::GameFrame;
use pav_core::arpg::combat::Team;
use pav_core::input::buttons;
use web_time::Instant;

/// Something on screen a finger can press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    /// A skill slot (0 is the left mouse button's; auto-attack uses it).
    Skill(u8),
    Dodge,
    Potion,
    /// Talk, trade, travel, descend: shown when something is in reach.
    Use,
    /// The pavilion: jump, throw a bomb, crouch, get in and out of vehicles.
    Jump,
    Bomb,
    Crouch,
    Interact,
    /// The hero's panels, the pause menu and the map.
    Bag,
    Menu,
    Map,
}

const SLOT_BITS: [u32; 6] =
    [buttons::PRIMARY, buttons::SECONDARY, buttons::SKILL3, buttons::SKILL4, buttons::SKILL5, buttons::SKILL6];

/// Movement past this (points) turns a skill tap into aiming by hand.
const DRAG_DEAD: f32 = 18.0;
/// A skill held this long without aiming keeps casting.
const HOLD: f32 = 0.32;
/// The stick's reach in points (full speed at its edge).
const STICK_R: f32 = 56.0;
/// How far a skill drag goes for a ground skill's full range.
const AIM_DRAG: f32 = 110.0;
/// A tap: shorter than this and moving less than `TAP_SLOP`.
const TAP_TIME: f32 = 0.3;
const TAP_SLOP: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Stick,
    Control(Control),
    Swipe(egui::Id),
    Egui,
    World,
}

#[derive(Clone, Debug)]
struct Finger {
    owner: Owner,
    start: Pos2,
    pos: Pos2,
    /// The stick's centre: where the thumb landed, dragged along when it runs past the edge.
    anchor: Pos2,
    t0: Instant,
    /// Frames it has been down for (a slow frame should not turn a tap into a press).
    frames: u32,
    /// Furthest it has been from where it started.
    travel: f32,
}

impl Finger {
    fn age(&self) -> f32 {
        self.t0.elapsed().as_secs_f32()
    }
    /// Short: under `TAP_TIME`, or seen by no more than a frame.
    fn quick(&self) -> bool {
        self.age() < TAP_TIME || self.frames <= 1
    }
    fn tap(&self) -> bool {
        self.quick() && self.travel < TAP_SLOP
    }
    /// Held long enough to count as holding (and seen held by a frame).
    fn holding(&self) -> bool {
        self.age() >= HOLD && self.frames >= 2
    }
}

/// A control as drawn this frame (touches test against the last frame's).
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub control: Control,
    pub rect: Rect,
    pub round: bool,
    /// Drawn above windows and pressed before them (the corner buttons).
    pub top: bool,
}

impl Placed {
    fn hit(&self, p: Pos2) -> bool {
        if self.round {
            // A little more forgiving than it looks.
            p.distance(self.rect.center()) <= self.rect.width() * 0.5 + 8.0
        } else {
            self.rect.expand(6.0).contains(p)
        }
    }
}

/// What the fingers want this frame.
#[derive(Clone, Debug, Default)]
pub struct Intent {
    /// Screen-space movement (x right, y up), length <= 1.
    pub move_axis: Vec2,
    /// World-space movement that overrides the stick (auto-attack stepping in).
    pub move_world: Option<Vec2>,
    pub held: u32,
    pub pressed: u32,
    /// Where skills aim this frame (None: straight ahead).
    pub aim: Option<Vec3>,
    /// A skill being aimed by hand: where it would land (for the marker on the ground).
    pub aiming: Option<Vec3>,
    /// Panels and menus asked for.
    pub ui: Vec<Control>,
    /// A tap on the open game (closes windows).
    pub world_tap: bool,
}

/// What the controls show this frame.
pub struct View<'a> {
    pub game: Option<&'a GameFrame>,
    pub auto_attack: bool,
    /// The minimap was swiped away (a map button brings it back).
    pub map_hidden: bool,
    /// Unspent passive points (a badge on the bag).
    pub points: u32,
    /// A window or menu covers the game: only the corner buttons show.
    pub covered: bool,
}

#[derive(Default)]
pub struct Touch {
    fingers: HashMap<u64, Finger>,
    /// The finger egui sees as its pointer.
    egui_finger: Option<u64>,
    placed: Vec<Placed>,
    /// Controls that went down, and ones tapped (pressed and let go quickly), since last frame.
    downs: Vec<Control>,
    taps: Vec<Control>,
    /// Skills let go after aiming by hand: the slot and the drag (points, screen axes).
    flicks: Vec<(u8, EVec2)>,
    world_tap: bool,
    pub swipes: Swipes,
}

impl Touch {
    /// A new finger. `ui` is the order of the egui layer under it, if any. Returns true when
    /// egui should get this touch.
    pub fn down(&mut self, id: u64, pos: Pos2, ui: Option<egui::Order>) -> bool {
        let stick_free = !self.fingers.values().any(|f| f.owner == Owner::Stick);
        let owner = if let Some(p) = self.placed.iter().find(|p| p.top && p.hit(pos)) {
            self.downs.push(p.control);
            Owner::Control(p.control)
        } else if ui.is_some_and(|o| o >= egui::Order::Middle) {
            Owner::Egui
        } else if let Some(p) = self.placed.iter().rev().find(|p| p.hit(pos)) {
            self.downs.push(p.control);
            Owner::Control(p.control)
        } else if let Some(s) = self.swipes.hit(pos) {
            self.swipes.held = Some((s, EVec2::ZERO));
            Owner::Swipe(s)
        } else if ui.is_some() {
            Owner::Egui
        } else if stick_free {
            Owner::Stick
        } else {
            Owner::World
        };
        self.fingers.insert(id, Finger { owner, start: pos, pos, anchor: pos, t0: Instant::now(), frames: 0, travel: 0.0 });
        if owner == Owner::Egui && self.egui_finger.is_none() {
            self.egui_finger = Some(id);
        }
        owner == Owner::Egui
    }

    /// A finger moved. Returns true when egui should get it.
    pub fn moved(&mut self, id: u64, pos: Pos2) -> bool {
        let Some(f) = self.fingers.get_mut(&id) else { return false };
        f.pos = pos;
        f.travel = f.travel.max(pos.distance(f.start));
        match f.owner {
            Owner::Stick => {
                // A thumb that runs past the edge drags the stick along.
                let d = pos - f.anchor;
                if d.length() > STICK_R {
                    f.anchor = pos - d.normalized() * STICK_R;
                }
            }
            Owner::Swipe(s) => self.swipes.held = Some((s, pos - f.start)),
            _ => {}
        }
        f.owner == Owner::Egui
    }

    /// A finger lifted (or the system took it). Returns true when egui should get it.
    pub fn up(&mut self, id: u64, pos: Pos2, cancelled: bool) -> bool {
        let Some(mut f) = self.fingers.remove(&id) else { return false };
        f.pos = pos;
        f.travel = f.travel.max(pos.distance(f.start));
        if self.egui_finger == Some(id) {
            self.egui_finger = None;
        }
        match f.owner {
            Owner::Control(c) if !cancelled => {
                let placed = self.placed.iter().find(|p| p.control == c).copied();
                match c {
                    Control::Skill(s) if f.travel >= DRAG_DEAD => {
                        // Dragged back onto the button: cancelled.
                        let back = placed.is_some_and(|p| pos.distance(p.rect.center()) < p.rect.width() * 0.3);
                        if !back {
                            self.flicks.push((s, pos - f.start));
                        }
                    }
                    Control::Skill(_) if f.holding() => {}
                    _ if f.travel < TAP_SLOP * 2.0 => self.taps.push(c),
                    _ => {}
                }
            }
            Owner::Swipe(s) => self.swipes.release(s, pos - f.start, f.quick(), f.tap() && !cancelled),
            Owner::Stick | Owner::World if f.tap() && !cancelled => self.world_tap = true,
            _ => {}
        }
        f.owner == Owner::Egui
    }

    /// The finger egui should treat as its pointer (the first one that landed on a window).
    #[cfg(test)]
    fn is_egui_pointer(&self, id: u64) -> bool {
        self.egui_finger == Some(id)
    }

    /// Lets go of everything (the window lost focus).
    pub fn clear(&mut self) {
        self.fingers.clear();
        self.egui_finger = None;
        self.downs.clear();
        self.swipes.held = None;
    }

    fn stick(&self) -> Option<&Finger> {
        self.fingers.values().find(|f| f.owner == Owner::Stick)
    }

    /// The stick: screen-space direction (x right, y up), length <= 1.
    pub fn stick_axis(&self) -> Vec2 {
        let Some(f) = self.stick() else { return Vec2::ZERO };
        let d = (f.pos - f.anchor) / STICK_R;
        let v = Vec2::new(d.x, -d.y);
        let l = v.length();
        // A small dead zone, then straight to full speed over the rest of the ring.
        if l < 0.12 { Vec2::ZERO } else { v / l * ((l - 0.12) / 0.7).min(1.0) }
    }

    fn held_control(&self, c: Control) -> Option<&Finger> {
        self.fingers.values().find(|f| f.owner == Owner::Control(c))
    }

    /// This frame's game input from the fingers. `game` is the Shardfall frame with the hero's
    /// feet and facing; `to_world` turns a screen direction (x right, y up) into the ground's.
    pub fn intent(
        &mut self,
        game: Option<(&GameFrame, Vec3, f32)>,
        to_world: impl Fn(Vec2) -> Vec2,
        auto_attack: bool,
    ) -> Intent {
        for f in self.fingers.values_mut() {
            f.frames += 1;
        }
        let mut it =
            Intent { move_axis: self.stick_axis(), world_tap: std::mem::take(&mut self.world_tap), ..Default::default() };
        let downs = std::mem::take(&mut self.downs);
        let taps = std::mem::take(&mut self.taps);
        let flicks = std::mem::take(&mut self.flicks);
        let press = |it: &mut Intent, b: u32| {
            it.pressed |= b;
            it.held |= b;
        };
        // Act the moment they go down: dodge, potion, use, jump, bomb.
        for c in &downs {
            match c {
                Control::Dodge => press(&mut it, buttons::DODGE),
                Control::Potion => press(&mut it, buttons::POTION),
                Control::Use | Control::Interact => press(&mut it, buttons::INTERACT),
                Control::Jump => press(&mut it, buttons::JUMP),
                Control::Bomb => press(&mut it, buttons::USE),
                _ => {}
            }
        }
        for c in &taps {
            if matches!(c, Control::Bag | Control::Menu | Control::Map) {
                it.ui.push(*c);
            }
        }
        for (c, b) in [(Control::Jump, buttons::JUMP), (Control::Crouch, buttons::CROUCH)] {
            if self.held_control(c).is_some() {
                it.held |= b;
            }
        }
        let Some((g, feet, facing)) = game else { return it };
        let Some(hero) = g.hero.as_ref().filter(|h| !h.dead) else { return it };
        let foes = foes(g);
        let ahead = Vec3::new(facing.sin(), 0.0, facing.cos());
        let slot_of = |s: u8| hero.slots.get(s as usize).filter(|x| !x.key.is_empty());
        // Where a skill goes when tapped: the nearest foe it can reach (or the nearest at all
        // for missiles), else straight ahead.
        let auto_aim = |s: u8| -> Option<Vec3> {
            let sl = slot_of(s)?;
            let near = |limit: f32| {
                foes.iter()
                    .map(|f| (f, flat_dist(f.pos, feet) - f.radius))
                    .filter(|(_, d)| *d <= limit)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(f, _)| f.pos + Vec3::Y * 0.6)
            };
            near(sl.reach + 2.0).or_else(|| near(sl.reach.max(10.0))).or_else(|| Some(feet + ahead * sl.range.clamp(2.0, 6.0)))
        };
        // Where a hand-aimed skill lands for a drag.
        let hand_aim = |s: u8, drag: EVec2| -> Option<Vec3> {
            let sl = slot_of(s)?;
            let d = to_world(Vec2::new(drag.x, -drag.y).normalize_or_zero());
            let dir = Vec3::new(d.x, 0.0, d.y);
            let far = if sl.ground {
                sl.range * ((drag.length() - DRAG_DEAD * 0.5) / AIM_DRAG).clamp(0.1, 1.0)
            } else {
                sl.range.clamp(3.0, 12.0)
            };
            Some(feet + dir * far)
        };
        // Skills.
        for s in 0..6u8 {
            let Some(sl) = slot_of(s) else { continue };
            let bit = SLOT_BITS[s as usize];
            if taps.contains(&Control::Skill(s)) && !sl.channel {
                press(&mut it, bit);
                it.aim = auto_aim(s);
            }
            if let Some(f) = self.held_control(Control::Skill(s)) {
                let drag = f.pos - f.start;
                if f.travel >= DRAG_DEAD {
                    it.aiming = hand_aim(s, drag);
                } else if sl.channel || f.holding() {
                    it.held |= bit;
                    if downs.contains(&Control::Skill(s)) {
                        it.pressed |= bit;
                    }
                    if it.aim.is_none() {
                        it.aim = auto_aim(s);
                    }
                }
            }
        }
        for (s, drag) in flicks {
            if let Some(a) = hand_aim(s, drag) {
                press(&mut it, SLOT_BITS[s as usize]);
                it.aim = Some(a);
            }
        }
        // Auto-attack with the first skill (unless another skill was just cast).
        let others = SLOT_BITS[1..].iter().fold(0, |a, b| a | b);
        if auto_attack && it.pressed & others == 0 {
            if let Some(sl) = slot_of(0) {
                let stick = to_world(it.move_axis);
                match pick_target(feet, stick, &foes, sl.reach) {
                    Some((i, Engage::Strike)) => {
                        it.held |= buttons::PRIMARY;
                        it.aim.get_or_insert(foes[i].pos + Vec3::Y * 0.6);
                    }
                    Some((i, Engage::Approach)) => {
                        let d = flat(foes[i].pos - feet).normalize_or_zero();
                        it.move_world = Some(Vec2::new(d.x, d.z));
                    }
                    None => {}
                }
            }
        }
        it
    }

    /// Lays out and draws the controls for this frame (touches next frame test against them).
    pub fn draw(&mut self, ctx: &egui::Context, v: &View) {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("touch_controls")));
        // The corner buttons stay above windows (and win over them).
        let top = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("touch_corner")));
        let (k, m) = crate::arpg_ui::touch_scale(screen);
        let mut placed = Vec::new();
        let down = |c: Control| self.fingers.values().find(|f| f.owner == Owner::Control(c));
        // Top right: menu, the bag, the map when the minimap is away.
        let r = 19.0 * k;
        let mut at = Pos2::new(screen.right() - m - r, screen.top() + m + r);
        let mut corner = vec![Control::Menu];
        if let Some(g) = v.game {
            corner.push(Control::Bag);
            if v.map_hidden && g.level.is_some() {
                corner.push(Control::Map);
            }
        }
        for c in corner {
            round_button(&top, at, r, down(c).is_some(), 1.0);
            let ic = Color32::from_white_alpha(220);
            match c {
                Control::Menu => {
                    for dx in [-4.5, 4.5] {
                        let bar = Rect::from_center_size(at + EVec2::new(dx * k, 0.0), EVec2::new(4.0 * k, 14.0 * k));
                        top.rect_filled(bar, 1.5, ic);
                    }
                }
                Control::Bag => {
                    bag_icon(&top, at, r, ic);
                    if v.points > 0 {
                        let b = at + EVec2::new(r * 0.75, -r * 0.75);
                        top.circle_filled(b, 8.0 * k, Color32::from_rgb(255, 200, 90));
                        let n = format!("{}", v.points.min(99));
                        top.text(b, Align2::CENTER_CENTER, n, FontId::proportional(10.0 * k), Color32::BLACK);
                    }
                }
                _ => map_icon(&top, at, r, ic),
            }
            placed.push(Placed { control: c, rect: Rect::from_center_size(at, EVec2::splat(r * 2.0)), round: true, top: true });
            at.x -= 46.0 * k;
        }
        if !v.covered {
            // The stick, only while a thumb is on it.
            if let Some(f) = self.stick() {
                let d = f.pos - f.anchor;
                let knob = f.anchor + if d.length() > STICK_R { d.normalized() * STICK_R } else { d };
                p.circle(f.anchor, STICK_R, Color32::from_black_alpha(28), Stroke::new(1.5, Color32::from_white_alpha(42)));
                p.circle_filled(knob, 22.0, Color32::from_white_alpha(46));
            }
            // The fan in the bottom right, round the big button in the corner.
            let c = Pos2::new(screen.right() - m - 40.0 * k, screen.bottom() - m - 40.0 * k);
            let fan = Fan { c, k };
            match v.game {
                Some(g) => self.game_fan(&p, g, v, &fan, &mut placed),
                None => {
                    let buttons = [
                        (Control::Jump, c, 34.0, "JUMP"),
                        (Control::Bomb, fan.inner(180.0), 25.0, "BOMB"),
                        (Control::Crouch, fan.inner(225.0), 24.0, "DUCK"),
                        (Control::Interact, fan.inner(270.0), 24.0, "USE"),
                    ];
                    for (ctl, pos, r, label) in buttons {
                        let r = r * k;
                        round_button(&p, pos, r, down(ctl).is_some(), 1.0);
                        p.text(pos, Align2::CENTER_CENTER, label, FontId::proportional(11.0 * k), Color32::from_white_alpha(220));
                        placed.push(Placed {
                            control: ctl,
                            rect: Rect::from_center_size(pos, EVec2::splat(r * 2.0)),
                            round: true,
                            top: false,
                        });
                    }
                }
            }
        }
        self.placed = placed;
    }

    fn game_fan(&self, p: &egui::Painter, g: &GameFrame, v: &View, fan: &Fan, placed: &mut Vec<Placed>) {
        let Some(h) = &g.hero else { return };
        if h.dead {
            return;
        }
        let (c, k) = (fan.c, fan.k);
        let down = |c: Control| self.fingers.values().find(|f| f.owner == Owner::Control(c));
        let mut place = |ctl: Control, pos: Pos2, r: f32| {
            placed.push(Placed {
                control: ctl,
                rect: Rect::from_center_size(pos, EVec2::splat(r * 2.0)),
                round: true,
                top: false,
            });
        };
        // The big button in the corner dodges; with auto-attack off it attacks and the dodge
        // moves to the bottom row.
        let (dodge, row) = if v.auto_attack { (c, 0.0) } else { (c + EVec2::new(-150.0 * k, 18.0 * k), 55.0 * k) };
        let r = if v.auto_attack { 34.0 * k } else { 22.0 * k };
        let busy = h.dodge_cd > 0.0;
        round_button(p, dodge, r, down(Control::Dodge).is_some(), if busy { 0.5 } else { 1.0 });
        dodge_icon(p, dodge, r, Color32::from_white_alpha(if busy { 110 } else { 230 }));
        place(Control::Dodge, dodge, r);
        // Skills: three on an inner arc, two on an outer one (the first skill, with
        // auto-attack off, in the corner).
        let spots = [fan.inner(180.0), fan.inner(225.0), fan.inner(270.0), fan.outer(205.0), fan.outer(245.0)];
        let mut free = spots.iter();
        for (s, sl) in h.slots.iter().enumerate() {
            if sl.key.is_empty() || (s == 0 && v.auto_attack) {
                continue;
            }
            let (pos, r) = if s == 0 {
                (c, 34.0 * k)
            } else {
                match free.next() {
                    Some(p) => (*p, 24.0 * k),
                    None => break,
                }
            };
            let ctl = Control::Skill(s as u8);
            let finger = down(ctl);
            round_button(p, pos, r, finger.is_some(), 1.0);
            let rect = Rect::from_center_size(pos, EVec2::splat(r * 2.0));
            let col = if sl.spell { crate::arpg_ui::element_color(sl.element, 1.0) } else { Color32::from_rgb(235, 228, 210) };
            let icon = if sl.affordable { col } else { Color32::from_rgb(150, 70, 70) };
            crate::arpg_ui::skill_icon(p, rect.shrink(r * 0.15), &sl.key, icon);
            if sl.cooldown > 0.0 && sl.cooldown_max > 0.0 {
                sweep(p, pos, r, (sl.cooldown / sl.cooldown_max).clamp(0.0, 1.0));
                let secs = format!("{:.0}", sl.cooldown.ceil());
                p.text(pos, Align2::CENTER_CENTER, secs, FontId::proportional(14.0 * k), Color32::WHITE);
            }
            // Aiming by hand: a knob shows the direction, the rim lights up.
            if let Some(f) = finger.filter(|f| f.travel >= DRAG_DEAD) {
                let d = f.pos - f.start;
                let knob = pos + if d.length() > r { d.normalized() * r } else { d };
                let gold = Color32::from_rgb(255, 220, 140);
                p.circle_stroke(pos, r + 3.0, Stroke::new(2.0, gold));
                p.circle_filled(knob, 7.0 * k, gold);
                if f.pos.distance(pos) < r * 0.3 {
                    p.text(
                        pos - EVec2::new(0.0, r + 12.0),
                        Align2::CENTER_CENTER,
                        "cancel",
                        FontId::proportional(11.0),
                        Color32::WHITE,
                    );
                }
            }
            place(ctl, pos, r);
        }
        // Potion, on the bottom row, with its charges.
        let pp = c + EVec2::new(-150.0 * k - row, 18.0 * k);
        let r = 20.0 * k;
        let none = h.potions == 0;
        round_button(p, pp, r, down(Control::Potion).is_some(), if none { 0.5 } else { 1.0 });
        flask_icon(p, pp, r, if none { Color32::from_rgb(90, 50, 50) } else { Color32::from_rgb(220, 50, 60) });
        let b = pp + EVec2::new(r * 0.7, r * 0.7);
        p.circle_filled(b, 8.0 * k, Color32::from_black_alpha(200));
        p.text(b, Align2::CENTER_CENTER, format!("{}", h.potions), FontId::proportional(10.0 * k), Color32::WHITE);
        place(Control::Potion, pp, r);
        // Use: only when something is in reach, named for what it does, next on the row.
        if let Some(s) = g.near.and_then(|i| g.spots.get(i)) {
            use pav_core::arpg::SpotKind as K;
            let what = match s.kind {
                K::Vendor => "Trade",
                K::Stash => "Stash",
                K::Portal => "Travel",
                K::Exhibit => "Examine",
                K::Exit => "Descend",
                K::Gamble => "Gamble",
                K::Alchemist => "Brew",
                K::Chest => "Open",
            };
            let w = (what.len() as f32 * 8.5 + 34.0) * k;
            let rect = Rect::from_center_size(pp + EVec2::new(-34.0 * k - w * 0.5, 0.0), EVec2::new(w, 38.0 * k));
            let alpha = if down(Control::Use).is_some() { 255 } else { 225 };
            p.rect_filled(rect, 19.0 * k, Color32::from_rgba_unmultiplied(255, 214, 140, alpha));
            p.text(rect.center(), Align2::CENTER_CENTER, what, FontId::proportional(15.0 * k), Color32::from_rgb(30, 22, 12));
            placed.push(Placed { control: Control::Use, rect, round: false, top: false });
        }
    }
}

/// Where the buttons round the corner go.
struct Fan {
    c: Pos2,
    k: f32,
}

impl Fan {
    fn inner(&self, deg: f32) -> Pos2 {
        self.c + EVec2::angled(deg.to_radians()) * 92.0 * self.k
    }
    fn outer(&self, deg: f32) -> Pos2 {
        self.c + EVec2::angled(deg.to_radians()) * 158.0 * self.k
    }
}

/// A round, see-through button.
fn round_button(p: &egui::Painter, c: Pos2, r: f32, down: bool, alpha: f32) {
    let r = if down { r * 0.94 } else { r };
    let (fill, rim) = if down { (150.0, 120.0) } else { (105.0, 55.0) };
    p.circle(
        c,
        r,
        Color32::from_black_alpha((fill * alpha) as u8),
        Stroke::new(1.5, Color32::from_white_alpha((rim * alpha) as u8)),
    );
}

/// A cooldown: a dark sweep over the button.
fn sweep(p: &egui::Painter, c: Pos2, r: f32, f: f32) {
    let n = 28;
    let pts: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f * k as f32 / n as f32;
            c + EVec2::angled(a) * r
        })
        .collect();
    for w in pts.windows(2) {
        p.add(Shape::convex_polygon(vec![c, w[0], w[1]], Color32::from_black_alpha(150), Stroke::NONE));
    }
}

fn dodge_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    // A roll: an arc with an arrowhead and two speed lines.
    let s = r * 0.42;
    let pts: Vec<Pos2> = (0..=14)
        .map(|i| {
            let a = 3.6 + 4.2 * i as f32 / 14.0;
            c + EVec2::angled(a) * s
        })
        .collect();
    let tip = *pts.last().unwrap();
    p.add(Shape::line(pts, Stroke::new(3.0, col)));
    p.add(Shape::convex_polygon(
        vec![tip + EVec2::new(4.0, -6.0), tip + EVec2::new(5.0, 6.0), tip + EVec2::new(-5.0, 2.0)],
        col,
        Stroke::NONE,
    ));
    for dy in [-0.35, 0.35] {
        p.line_segment([c + EVec2::new(-s * 1.9, dy * s), c + EVec2::new(-s * 1.25, dy * s)], Stroke::new(2.0, col));
    }
}

fn flask_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let s = r * 0.5;
    p.circle_filled(c + EVec2::new(0.0, s * 0.35), s * 0.75, col);
    p.rect_filled(Rect::from_center_size(c + EVec2::new(0.0, -s * 0.55), EVec2::new(s * 0.5, s * 0.7)), 1.0, col);
    p.circle_filled(c + EVec2::new(-s * 0.25, s * 0.15), s * 0.18, Color32::from_white_alpha(90));
}

fn bag_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let s = r * 0.5;
    p.rect_filled(Rect::from_center_size(c + EVec2::new(0.0, s * 0.25), EVec2::new(s * 1.6, s * 1.3)), 3.0, col);
    p.circle_stroke(c + EVec2::new(0.0, -s * 0.5), s * 0.42, Stroke::new(2.0, col));
}

fn map_icon(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let s = r * 0.5;
    let pts = vec![
        c + EVec2::new(-s, -s * 0.7),
        c + EVec2::new(-s * 0.33, -s),
        c + EVec2::new(s * 0.33, -s * 0.7),
        c + EVec2::new(s, -s),
        c + EVec2::new(s, s * 0.7),
        c + EVec2::new(s * 0.33, s),
        c + EVec2::new(-s * 0.33, s * 0.7),
        c + EVec2::new(-s, s),
        c + EVec2::new(-s, -s * 0.7),
    ];
    p.add(Shape::line(pts, Stroke::new(2.0, col)));
}

// ------------------------------------------------------------------------------ auto-attack

/// A foe as targeting sees it.
#[derive(Clone, Copy, Debug)]
pub struct Foe {
    pub pos: Vec3,
    pub radius: f32,
    /// Fighting the hero.
    pub aggro: bool,
}

fn foes(g: &GameFrame) -> Vec<Foe> {
    g.actors
        .iter()
        .filter(|a| a.team == Team::Monster && !a.dead)
        .map(|a| Foe { pos: a.feet, radius: a.radius, aggro: a.aggro })
        .collect()
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    flat(a - b).length()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engage {
    Strike,
    /// Step in toward it (a foe fighting the hero, just out of reach, while the stick is still).
    Approach,
}

/// Which foe auto-attack goes for, if any. `stick` is the world direction the player steers
/// (zero when the stick is still) and `reach` the first skill's.
///
/// - Something in reach is struck, unless the player is running away from it.
/// - With the stick still, a foe fighting the hero a step or two beyond reach is closed in on.
pub fn pick_target(me: Vec3, stick: Vec2, foes: &[Foe], reach: f32) -> Option<(usize, Engage)> {
    let still = stick.length() < 0.15;
    let steer = Vec3::new(stick.x, 0.0, stick.y).normalize_or_zero();
    let mut best: Option<(usize, f32, Engage)> = None;
    for (i, f) in foes.iter().enumerate() {
        let to = flat(f.pos - me);
        let gap = to.length() - f.radius;
        let engage = if gap <= reach + 0.2 {
            // Running away from it: let the player go.
            if !still && to.normalize_or_zero().dot(steer) < -0.25 {
                continue;
            }
            Engage::Strike
        } else if still && f.aggro && gap <= reach + 3.0 {
            Engage::Approach
        } else {
            continue;
        };
        // Something in reach beats anything to walk to; then the nearest.
        let score = gap + if engage == Engage::Approach { 100.0 } else { 0.0 };
        if best.is_none_or(|b| score < b.1) {
            best = Some((i, score, engage));
        }
    }
    best.map(|(i, _, e)| (i, e))
}

// --------------------------------------------------------------------------------- swipes

/// Temporary things on screen that a finger can swipe away. Each one asks [`Swipes::place`]
/// where to draw itself every frame; once swiped it stays away until its content changes.
#[derive(Default)]
pub struct Swipes {
    prev: Vec<(egui::Id, Rect, u64)>,
    now: Vec<(egui::Id, Rect, u64)>,
    /// The one under a finger and how far it has been dragged.
    held: Option<(egui::Id, EVec2)>,
    /// Swiped away: which version of it (its key), when, and how it flew.
    gone: HashMap<egui::Id, (u64, Instant, EVec2)>,
    /// Tapped (not swiped) since last frame.
    tapped: Vec<egui::Id>,
}

/// Where a swipeable thing is drawn this frame: shifted by the finger and faded.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub offset: EVec2,
    pub alpha: f32,
}

impl Swipes {
    pub fn begin_frame(&mut self) {
        self.prev = std::mem::take(&mut self.now);
    }

    /// `id` is about to draw in `rect` (`key` names its content: new content comes back after
    /// a swipe). None once it has been swiped away.
    pub fn place(&mut self, id: egui::Id, key: u64, rect: Rect) -> Option<Place> {
        if let Some((k, t, fly)) = self.gone.get(&id) {
            if *k == key {
                let a = t.elapsed().as_secs_f32() / 0.22;
                if a >= 1.0 {
                    return None;
                }
                let dir = fly.normalized();
                return Some(Place { offset: *fly + dir * 420.0 * a * a, alpha: (1.0 - a) * 0.8 });
            }
            self.gone.remove(&id);
        }
        let offset = match self.held {
            Some((h, d)) if h == id => d,
            _ => EVec2::ZERO,
        };
        self.now.push((id, rect.translate(offset), key));
        Some(Place { offset, alpha: (1.0 - offset.length() / 240.0).clamp(0.25, 1.0) })
    }

    /// Whether `id` (with this content) was swiped away.
    pub fn is_gone(&self, id: egui::Id, key: u64) -> bool {
        self.gone.get(&id).is_some_and(|(k, t, _)| *k == key && t.elapsed().as_secs_f32() >= 0.22)
    }

    /// Brings `id` back.
    pub fn restore(&mut self, id: egui::Id) {
        self.gone.remove(&id);
    }

    /// Whether `id` was tapped since the last call.
    pub fn take_tap(&mut self, id: egui::Id) -> bool {
        let n = self.tapped.len();
        self.tapped.retain(|t| *t != id);
        self.tapped.len() != n
    }

    fn hit(&self, p: Pos2) -> Option<egui::Id> {
        self.prev.iter().rev().find(|(_, r, _)| r.expand(4.0).contains(p)).map(|(id, _, _)| *id)
    }

    fn release(&mut self, id: egui::Id, d: EVec2, quick: bool, tap: bool) {
        self.held = None;
        if tap {
            self.tapped.push(id);
            return;
        }
        let flick = quick && d.length() > 26.0;
        if d.length() > 72.0 || flick {
            let key = self.prev.iter().find(|(i, _, _)| *i == id).map(|x| x.2).unwrap_or(0);
            self.gone.insert(id, (key, Instant::now(), d));
        }
    }
}

/// An egui area that a finger can swipe away (on touch screens, when `swipes` is given). `key`
/// names its content: swiped-away content stays away, new content shows.
#[allow(clippy::too_many_arguments)]
pub fn swipe_area(
    ctx: &egui::Context,
    swipes: Option<&mut Swipes>,
    id: &str,
    key: u64,
    anchor: Align2,
    offset: EVec2,
    order: egui::Order,
    add: impl FnOnce(&mut egui::Ui),
) {
    let area = egui::Id::new(id);
    let mut off = offset;
    let mut alpha = 1.0;
    if let Some(s) = swipes {
        let rect = ctx.memory(|m| m.area_rect(area)).unwrap_or(Rect::NOTHING);
        let Some(pl) = s.place(area, key, rect) else { return };
        off += pl.offset;
        alpha = pl.alpha;
    }
    egui::Area::new(area).anchor(anchor, off).order(order).interactable(false).show(ctx, |ui| {
        ui.multiply_opacity(alpha);
        add(ui);
    });
}

/// A key for swipeable content.
pub fn key(x: impl std::hash::Hash) -> u64 {
    use std::hash::{DefaultHasher, Hasher};
    let mut h = DefaultHasher::new();
    x.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foe(x: f32, z: f32, aggro: bool) -> Foe {
        Foe { pos: Vec3::new(x, 0.0, z), radius: 0.5, aggro }
    }

    #[test]
    fn auto_attack_strikes_the_nearest_foe_in_reach() {
        let foes = [foe(4.0, 0.0, true), foe(0.0, 2.0, false), foe(-2.5, 0.0, true)];
        // The second (gap 1.5) is nearer than the third (gap 2.0); the first is out of reach.
        assert_eq!(pick_target(Vec3::ZERO, Vec2::ZERO, &foes, 2.2), Some((1, Engage::Strike)));
        // Nothing in reach, the stick still: step in to the one fighting the hero.
        let foes = [foe(5.0, 0.0, true), foe(0.0, 4.5, false)];
        assert_eq!(pick_target(Vec3::ZERO, Vec2::ZERO, &foes, 2.2), Some((0, Engage::Approach)));
        // Moving: no stepping in, only striking what is in reach.
        assert_eq!(pick_target(Vec3::ZERO, Vec2::X, &foes, 2.2), None);
        // Far away: nothing.
        assert_eq!(pick_target(Vec3::ZERO, Vec2::ZERO, &[foe(20.0, 0.0, true)], 2.2), None);
    }

    #[test]
    fn running_away_is_not_slowed_by_auto_attack() {
        let foes = [foe(2.0, 0.0, true)];
        assert_eq!(pick_target(Vec3::ZERO, Vec2::new(-1.0, 0.0), &foes, 2.2), None);
        // Strafing past it still strikes.
        assert_eq!(pick_target(Vec3::ZERO, Vec2::new(0.0, 1.0), &foes, 2.2), Some((0, Engage::Strike)));
        assert_eq!(pick_target(Vec3::ZERO, Vec2::new(1.0, 0.0), &foes, 2.2), Some((0, Engage::Strike)));
    }

    #[test]
    fn fingers_find_their_jobs() {
        let mut t = Touch::default();
        t.placed = vec![Placed {
            control: Control::Dodge,
            rect: Rect::from_center_size(Pos2::new(700.0, 340.0), EVec2::splat(60.0)),
            round: true,
            top: false,
        }];
        // A thumb on the open game is the stick; it follows the thumb past its edge.
        assert!(!t.down(1, Pos2::new(100.0, 300.0), None));
        t.moved(1, Pos2::new(100.0 + STICK_R * 3.0, 300.0));
        let a = t.stick_axis();
        assert!((a.x - 1.0).abs() < 1e-4 && a.y.abs() < 1e-4, "{a}");
        // A second thumb on the dodge button presses it the moment it lands.
        assert!(!t.down(2, Pos2::new(705.0, 345.0), None));
        let it = t.intent(None, |v| v, true);
        assert!(it.pressed & buttons::DODGE != 0);
        // A window under a finger: egui gets it.
        assert!(t.down(3, Pos2::new(400.0, 200.0), Some(egui::Order::Middle)));
        assert!(t.is_egui_pointer(3));
        assert!(t.up(3, Pos2::new(400.0, 200.0), false));
        // A quick tap on the open game (the stick lifted at once) closes windows.
        t.up(1, Pos2::new(100.0, 300.0), false);
        assert!(!t.down(4, Pos2::new(300.0, 100.0), None));
        t.up(4, Pos2::new(301.0, 100.0), false);
        assert!(t.intent(None, |v| v, true).world_tap);
    }

    #[test]
    fn swiped_things_stay_away_until_they_change() {
        let mut s = Swipes::default();
        let id = egui::Id::new("toast");
        let r = Rect::from_min_size(Pos2::new(100.0, 100.0), EVec2::new(200.0, 40.0));
        assert!(s.place(id, 1, r).is_some());
        s.begin_frame();
        let hit = s.hit(Pos2::new(150.0, 120.0)).unwrap();
        s.held = Some((hit, EVec2::new(90.0, 0.0)));
        assert!(s.place(id, 1, r).unwrap().offset.x > 80.0);
        s.release(hit, EVec2::new(90.0, 0.0), false, false);
        s.gone.get_mut(&id).unwrap().1 = Instant::now() - std::time::Duration::from_secs(1);
        assert!(s.place(id, 1, r).is_none());
        assert!(s.is_gone(id, 1));
        // New content shows again.
        assert!(s.place(id, 2, r).is_some());
        // A short drag springs back.
        s.begin_frame();
        s.release(id, EVec2::new(20.0, 0.0), false, false);
        assert!(s.place(id, 2, r).is_some());
    }
}

#[cfg(test)]
mod egui_taps {
    use egui::{Event, Pos2, RawInput, Rect};

    fn frame(ctx: &egui::Context, events: Vec<Event>, clicked: &mut bool) {
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 400.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::Window::new("w").fixed_pos([100.0, 100.0]).vscroll(true).max_height(300.0).show(ui.ctx(), |ui| {
                let (_, r) = ui.allocate_exact_size(egui::vec2(50.0, 50.0), egui::Sense::click());
                *clicked |= r.clicked();
                ui.allocate_exact_size(egui::vec2(50.0, 500.0), egui::Sense::hover());
            });
        });
        out.textures_delta.clear();
    }

    #[test]
    fn a_quick_tap_clicks() {
        let ctx = egui::Context::default();
        let mut clicked = false;
        frame(&ctx, vec![], &mut clicked);
        frame(&ctx, vec![], &mut clicked);
        let pos = Pos2::new(130.0, 150.0);
        let touch = |phase| Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(7), phase, pos, force: None };
        let button =
            |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(
            &ctx,
            vec![
                touch(egui::TouchPhase::Start),
                Event::PointerMoved(pos),
                button(true),
                touch(egui::TouchPhase::End),
                button(false),
                Event::PointerGone,
            ],
            &mut clicked,
        );
        frame(&ctx, vec![], &mut clicked);
        assert!(clicked, "press and release in one frame");
        // Press in one frame, release in a later one.
        clicked = false;
        frame(&ctx, vec![touch(egui::TouchPhase::Start), Event::PointerMoved(pos), button(true)], &mut clicked);
        frame(&ctx, vec![], &mut clicked);
        frame(&ctx, vec![touch(egui::TouchPhase::End), button(false), Event::PointerGone], &mut clicked);
        frame(&ctx, vec![], &mut clicked);
        assert!(clicked, "press and release in different frames");
    }
}
