//! Menus on a controller, for every egui screen as it is: the D-pad (or the left stick) moves
//! between the clickable items of the open windows, Cross and Square click the item (left and
//! right, as a mouse would), Triangle is Shift, and the right stick scrolls. It reads where the
//! last frame drew its widgets, puts egui's pointer on the chosen one (so hover, tooltips and
//! clicks work as with a mouse) and rings it, big enough to read from the sofa.
//!
//! - A slider takes the D-pad's left and right itself (egui's own keys for a focused slider);
//!   Cross never clicks it, which would jump its value to the middle.
//! - On a canvas (a widget as big as a good part of the screen: the passive tree) the stick
//!   moves the pointer freely and the D-pad nudges it; the canvas reads it like a mouse.
//! - Hover moves the focus: when the player picks up the pad, it starts from the item the mouse
//!   was on.

use egui::{Context, Event, Id, Key, LayerId, Order, PointerButton, Pos2, Rect, Stroke, Vec2 as EVec2, vec2};

/// A direction on the D-pad (or a push of the left stick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn vec(self) -> EVec2 {
        match self {
            Dir::Up => vec2(0.0, -1.0),
            Dir::Down => vec2(0.0, 1.0),
            Dir::Left => vec2(-1.0, 0.0),
            Dir::Right => vec2(1.0, 0.0),
        }
    }
    fn key(self) -> Key {
        match self {
            Dir::Up => Key::ArrowUp,
            Dir::Down => Key::ArrowDown,
            Dir::Left => Key::ArrowLeft,
            Dir::Right => Key::ArrowRight,
        }
    }
}

/// The pad, as menus read it this frame.
#[derive(Clone, Debug, Default)]
pub struct PadIn {
    /// D-pad presses since the last frame.
    pub taps: Vec<Dir>,
    /// The left stick (x right, y up), dead zone applied.
    pub stick: glam::Vec2,
    /// The right stick's vertical push: scrolling.
    pub scroll: f32,
    pub cross: bool,
    pub square: bool,
    pub triangle: bool,
    pub dt: f32,
}

/// One clickable thing the last frame drew.
#[derive(Clone, Copy, Debug)]
struct Item {
    id: Id,
    rect: Rect,
    layer: LayerId,
    slider: bool,
    canvas: bool,
}

#[derive(Default)]
pub struct PadNav {
    /// egui's pointer (points), while the pad drives the menus.
    pub pointer: Option<Pos2>,
    snapped: Option<Item>,
    /// The stick's held direction and the time to its next step.
    held: Option<(Dir, f32)>,
    buttons: (bool, bool, bool),
    /// Cross and Square went down on a canvas and egui still holds them (a drag): each is let go
    /// when its button comes up, wherever the pointer has gone by then.
    grabbed: (bool, bool),
    /// Where the pointer last was on a canvas, and the canvas's rect then: the pad comes back
    /// there (to the node a dialog opened from), not to the canvas's middle, unless the canvas has
    /// changed size or place since (the UI's scale, a turned phone), which moves its nodes.
    canvas_at: Option<(Id, Rect, Pos2)>,
    /// The windows with items last frame: a new one (a menu, a popup) takes the pad.
    layers: Vec<LayerId>,
    /// The pad has had a frame on these menus: false on its first, just picked up or the menus
    /// just come up (`reset`), whose presses only show where it is.
    seen: bool,
}

/// Where a canvas wants the D-pad to snap (egui temp data under this id: screen points), such as
/// the passive tree's nodes in view.
pub const SNAP_POINTS: &str = "padnav_snap_points";

/// Where a screen wants the pad to land when it needs a new place to start (a window just
/// opened, or the item it was on went): a widget's `Id`, in egui temp data under this id. The
/// skill chooser starts on the skill the button has, and its button's row takes the pad back.
pub const FOCUS: &str = "padnav_focus";

/// Stick past this is a push.
const PUSH: f32 = 0.55;
const FIRST_REPEAT: f32 = 0.38;
const REPEAT: f32 = 0.13;

impl PadNav {
    /// The pad left the menus (or the player picked up the mouse): forget the pointer.
    pub fn reset(&mut self) {
        self.pointer = None;
        self.snapped = None;
        self.held = None;
        self.canvas_at = None;
        self.layers.clear();
        self.seen = false;
    }

    /// Whether the pointer sits on a canvas, where the stick moves it freely.
    pub fn free(&self) -> bool {
        self.snapped.is_some_and(|s| s.canvas)
    }

    /// The item the pad is on, if any.
    pub fn focused(&self) -> Option<Rect> {
        self.snapped.filter(|s| !s.canvas).map(|s| s.rect)
    }

    /// One frame of menus on the pad, between egui passes: what egui should hear next pass.
    /// `mouse` is where the mouse last was (the pad starts from the item under it).
    pub fn frame(&mut self, ctx: &Context, input: &PadIn, mouse: Option<Pos2>) -> Vec<Event> {
        let mut events = Vec::new();
        let woke = !self.seen;
        self.seen = true;
        // Every item, and the ones no other window covers: the pad stays on the item it is on
        // even while something passes over it (a toast), but steps and lands only where it shows.
        let all = collect_items(ctx);
        let items: Vec<Item> = all.iter().filter(|i| shown(ctx, i.rect, i.layer)).copied().collect();
        let screen = ctx.content_rect();
        // A window that just opened takes the pad; otherwise keep the item we are on (its rect
        // may have moved), or find one to start from.
        let mut layers: Vec<LayerId> = Vec::new();
        for i in &all {
            if !layers.contains(&i.layer) {
                layers.push(i.layer);
            }
        }
        let opened = layers.iter().any(|l| !self.layers.contains(l));
        // The pad's first frame on these menus (picked up, or they just came up).
        let fresh = self.layers.is_empty();
        self.layers = layers;
        let mut current = self.snapped.and_then(|s| all.iter().find(|i| i.id == s.id).copied()).filter(|_| !opened);
        // The item went (a menu turned a page): start again at the top, not wherever it was.
        let lost = self.snapped.is_some() && current.is_none();
        if opened || lost {
            self.pointer = None;
        }
        let mouse = mouse.filter(|_| !lost);
        if current.is_none() {
            let from = self.pointer.or(mouse);
            // Where the screen asks, for a window the pad opened or an item it lost (picking
            // the pad up starts from the item under the mouse instead).
            let wanted: Option<Id> = ctx.data(|d| d.get_temp(Id::new(FOCUS)));
            // (Asked for something another window hides, such as a Skills row under the passive
            // tree: whatever shows there, the tree's canvas.)
            let asked = || {
                wanted.and_then(|id| {
                    items.iter().find(|i| i.id == id).copied().or_else(|| {
                        let hid = all.iter().find(|i| i.id == id)?.rect.center();
                        items.iter().filter(|i| i.rect.contains(hid)).min_by(|a, b| area(a).total_cmp(&area(b))).copied()
                    })
                })
            };
            current = asked()
                .filter(|_| (opened || lost) && !fresh)
                .or_else(|| {
                    from.and_then(|p| {
                        items.iter().filter(|i| i.rect.contains(p)).min_by(|a, b| area(a).total_cmp(&area(b))).copied()
                    })
                })
                // (Picked up fresh with nothing under the mouse, the pad starts at the top, unless the
                // screen asks for a canvas: the passive tree's, not the tab above it.)
                .or_else(|| asked().filter(|i| i.canvas))
                .or_else(|| first(&items));
            if let Some(c) = current {
                if !c.canvas {
                    self.pointer = Some(c.rect.center());
                } else if self.pointer.is_none() {
                    // Back on a canvas it was on (a dialog over it closed): where it was. Picked up
                    // with the mouse on it: where the mouse is (the node it points at). Else its
                    // middle.
                    let same = |r: Rect| (r.min - c.rect.min).length() < 1.0 && (r.max - c.rect.max).length() < 1.0;
                    let back = self.canvas_at.filter(|(id, r, _)| *id == c.id && same(*r)).map(|(_, _, p)| p);
                    let under = mouse.filter(|p| woke && c.rect.contains(*p));
                    self.pointer = Some(back.or(under).unwrap_or(c.rect.center()));
                }
            }
        }
        let mut pointer = self.pointer.unwrap_or(screen.center());
        // The directions asked for this frame: D-pad taps, and the stick's pushes with repeat.
        let mut dirs = input.taps.clone();
        let canvas = current.is_some_and(|c| c.canvas);
        if canvas {
            // The stick moves the pointer freely; the D-pad snaps it to the next point the
            // canvas offers that way (a tree's nodes), or nudges it.
            let speed = 260.0 + 900.0 * input.stick.length().powi(2);
            pointer += vec2(input.stick.x, -input.stick.y) * speed * input.dt;
            let points: Vec<Pos2> = ctx.data(|d| d.get_temp(Id::new(SNAP_POINTS))).unwrap_or_default();
            for d in dirs.drain(..) {
                pointer = snap(&points, pointer, d).unwrap_or(pointer + d.vec() * 40.0);
            }
            self.held = None;
        } else {
            let pushed = stick_dir(input.stick);
            match (pushed, self.held) {
                (Some(d), Some((h, t))) if d == h => {
                    let t = t - input.dt;
                    if t <= 0.0 {
                        dirs.push(d);
                        self.held = Some((d, REPEAT));
                    } else {
                        self.held = Some((d, t));
                    }
                }
                (Some(d), _) => {
                    dirs.push(d);
                    self.held = Some((d, FIRST_REPEAT));
                }
                (None, _) => self.held = None,
            }
        }
        for d in dirs {
            if let Some(c) = current.filter(|c| c.slider && matches!(d, Dir::Left | Dir::Right)) {
                // A slider takes left and right as its own keys.
                ctx.memory_mut(|m| m.request_focus(c.id));
                events.push(Event::Key {
                    key: d.key(),
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
                events.push(Event::Key {
                    key: d.key(),
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Default::default(),
                });
                continue;
            }
            let from = current.map(|c| c.rect).unwrap_or(Rect::from_center_size(pointer, EVec2::splat(1.0)));
            if let Some(next) = step(&items, current.map(|c| c.id), from, d) {
                if current.is_some_and(|c| c.slider) {
                    ctx.memory_mut(|m| m.surrender_focus(current.unwrap().id));
                }
                current = Some(next);
                pointer = next.rect.center();
            }
        }
        pointer = pointer.clamp(screen.min, screen.max - vec2(2.0, 2.0));
        // On a canvas, the item is whatever the pointer is over now (dragging, still the canvas,
        // as a mouse's drag is: crossing a button on the way doesn't land on it).
        if canvas && !(self.grabbed.0 || self.grabbed.1) {
            current =
                items.iter().filter(|i| i.rect.contains(pointer)).min_by(|a, b| area(a).total_cmp(&area(b))).copied().or(current);
        }
        events.push(Event::PointerMoved(pointer));
        let modifiers = egui::Modifiers { shift: input.triangle, ..Default::default() };
        if input.triangle != self.buttons.2 {
            events.push(Event::ModifiersChanged(modifiers));
        }
        let on_slider = current.is_some_and(|c| c.slider);
        let on_canvas = current.is_some_and(|c| c.canvas);
        if woke {
            // The press that picks the pad up (or one still held as the menus come up: the Cross
            // that opened a vendor) only shows where the pad is: the screen may have moved under it
            // (a pad's prompts and view differ from a mouse's). The next press acts.
            self.buttons.0 = input.cross && !on_slider;
            self.buttons.1 = input.square;
        }
        for (now, was, button, grabbed) in [
            (input.cross && !on_slider, self.buttons.0, PointerButton::Primary, &mut self.grabbed.0),
            (input.square, self.buttons.1, PointerButton::Secondary, &mut self.grabbed.1),
        ] {
            if *grabbed {
                // Down on a canvas: egui holds it until the button comes up, wherever the pointer
                // is then (a drag that crossed onto the header lets go too).
                if !now {
                    events.push(Event::PointerButton { pos: pointer, button, pressed: false, modifiers });
                    *grabbed = false;
                }
            } else if now && !was {
                // A canvas (the passive tree) is a mouse's: held is held, so holding Cross and
                // moving drags it. Elsewhere a press acts once, on the frame it goes down: a
                // whole click, however long the button stays down (and however slow the frame).
                events.push(Event::PointerButton { pos: pointer, button, pressed: true, modifiers });
                if on_canvas {
                    *grabbed = true;
                } else {
                    events.push(Event::PointerButton { pos: pointer, button, pressed: false, modifiers });
                }
            }
        }
        self.buttons = (input.cross && !on_slider, input.square, input.triangle);
        if input.scroll.abs() > 0.1 {
            events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, input.scroll * 900.0 * input.dt),
                modifiers: Default::default(),
            });
        }
        // With nothing to be on (the window not drawn yet), no pointer is kept: the next frame
        // starts on the window's first item, not on whatever lies under the screen's middle.
        self.pointer = current.is_some().then_some(pointer);
        if let Some(c) = current.filter(|c| c.canvas) {
            self.canvas_at = Some((c.id, c.rect, pointer));
        }
        self.snapped = current;
        events
    }

    /// Lets go of the pointer's buttons (the menus closed with one held).
    pub fn release(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        // (Only a canvas holds a button down; elsewhere a press was a whole click. The window
        // may have gone already: the release goes where the pointer last was on it.)
        if let Some(pos) = self.pointer.or(self.canvas_at.map(|(_, _, p)| p)) {
            for (held, button) in [(self.grabbed.0, PointerButton::Primary), (self.grabbed.1, PointerButton::Secondary)] {
                if held {
                    out.push(Event::PointerButton { pos, button, pressed: false, modifiers: Default::default() });
                }
            }
        }
        // Triangle is Shift on every item: held as the pad leaves, it is let go too (the keyboard
        // says nothing until a key of its own changes, and a stuck Shift takes whole paths).
        if self.buttons.2 {
            out.push(Event::ModifiersChanged(Default::default()));
        }
        self.buttons = (false, false, false);
        self.grabbed = (false, false);
        out
    }

    /// The ring round the item the pad is on (or the pointer, on a canvas): drawn over
    /// everything, thick enough to see from across a room.
    pub fn draw(&self, ctx: &Context, time: f64) {
        let Some(pointer) = self.pointer else { return };
        let p = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("padnav_ring")));
        let gold = egui::Color32::from_rgb(255, 214, 120);
        match self.focused() {
            Some(r) => {
                let pulse = 0.5 + 0.5 * ((time * 4.0).sin() as f32);
                let r = r.expand(3.0);
                p.rect_stroke(
                    r.expand(3.0),
                    8.0,
                    Stroke::new(4.0, gold.gamma_multiply(0.25 + 0.2 * pulse)),
                    egui::StrokeKind::Outside,
                );
                p.rect_stroke(r, 6.0, Stroke::new(3.0, gold), egui::StrokeKind::Outside);
            }
            None => {
                // A free pointer: an arrow.
                let pts = vec![pointer, pointer + vec2(0.0, 22.0), pointer + vec2(6.0, 16.5), pointer + vec2(15.0, 16.0)];
                p.add(egui::Shape::convex_polygon(pts, gold, Stroke::new(1.5, egui::Color32::BLACK)));
            }
        }
    }
}

fn area(i: &Item) -> f32 {
    i.rect.width() * i.rect.height()
}

fn stick_dir(s: glam::Vec2) -> Option<Dir> {
    if s.length() < PUSH {
        return None;
    }
    Some(if s.x.abs() > s.y.abs() {
        if s.x > 0.0 { Dir::Right } else { Dir::Left }
    } else if s.y > 0.0 {
        Dir::Up
    } else {
        Dir::Down
    })
}

/// A slider (or anything else a finger drags sideways: it drags, and is wide and short) under
/// `pos` in the last frame: touch doesn't flick its window away (`touch::Touch::on_slider`).
pub fn slider_at(ctx: &Context, pos: Pos2) -> bool {
    collect_items(ctx).iter().any(|i| i.slider && i.rect.contains(pos))
}

/// The clickable items the last frame drew in the open windows (the topmost popup's alone when
/// one is open: a combo box's list), not the world's labels under them.
/// The nine points `shown` looks at: a rect's middle, corners and edges.
const AT: [(f32, f32); 9] =
    [(0.5, 0.5), (0.1, 0.1), (0.9, 0.1), (0.1, 0.9), (0.9, 0.9), (0.5, 0.1), (0.5, 0.9), (0.1, 0.5), (0.9, 0.5)];

/// Whether a widget at `rect` on `layer` shows: no other window covers it at all nine points (its
/// middle, corners and edges). One covered (two panels the keys opened, one over the other) is no
/// stop nor place to land, nor a screen's to ask for: the ring would sit on something hidden and
/// Cross click the window on top. (Seen anywhere, it counts: a big canvas whose middle a window
/// covers still shows.)
pub fn shown(ctx: &Context, rect: Rect, layer: LayerId) -> bool {
    AT.iter().any(|&(x, y)| ctx.layer_id_at(rect.lerp_inside(vec2(x, y))).is_none_or(|top| top == layer))
}

fn collect_items(ctx: &Context) -> Vec<Item> {
    let screen = ctx.content_rect();
    let order: Vec<LayerId> = ctx.memory(|m| m.layer_ids().collect());
    let visible = ctx.memory(|m| m.areas().visible_layer_ids());
    ctx.viewport(|v| {
        let w = &v.prev_pass.widgets;
        let mut by_layer: Vec<(LayerId, Vec<Item>)> = Vec::new();
        for layer in order.iter().filter(|l| visible.contains(l)) {
            if !matches!(layer.order, Order::Middle | Order::Foreground) {
                continue;
            }
            // (An area's own background, what drags or raises it, and a window's title bar are
            // not items.)
            let (background, title) = (layer.id.with("move"), layer.id.with("__title_click"));
            let list: Vec<Item> = w
                .get_layer(*layer)
                .filter(|r| r.id != background && r.id != title)
                // Clickable things, and sliders: egui's slider rail only drags (no click), so it
                // is taken by its shape (a scroll area's drag-only body is tall).
                .filter(|r| r.enabled && r.rect.is_positive() && screen.intersects(r.rect))
                // A scroll bar or a window's resize edge (a thin strip that drags) is no stop: the
                // right stick scrolls.
                .filter(|r| !(r.sense.senses_drag() && r.rect.width().min(r.rect.height()) < 16.0))
                .filter(|r| r.sense.senses_click() || (r.sense.senses_drag() && slider_shaped(r.rect)))
                .map(|r| {
                    let rect = r.rect.intersect(screen);
                    let canvas = rect.area() > screen.area() * 0.2;
                    let slider = r.sense.senses_drag() && !canvas && slider_shaped(rect);
                    Item { id: r.id, rect, layer: *layer, slider, canvas }
                })
                .collect();
            if !list.is_empty() {
                by_layer.push((*layer, list));
            }
        }
        // A popup on top takes the pad alone; otherwise every open window is one menu.
        match by_layer.last() {
            Some((l, list)) if l.order == Order::Foreground => list.clone(),
            _ => by_layer.into_iter().flat_map(|(_, l)| l).collect(),
        }
    })
}

/// Where to start: in the window with the most items (a menu, not a stray panel), its top-left
/// item of a good size (not a title bar's little close button).
fn first(items: &[Item]) -> Option<Item> {
    let mut counts: Vec<(LayerId, usize)> = Vec::new();
    for i in items {
        match counts.iter_mut().find(|(l, _)| *l == i.layer) {
            Some(c) => c.1 += 1,
            None => counts.push((i.layer, 1)),
        }
    }
    // The most items; a tie goes to the window on top (later in the list).
    let top = counts.iter().rev().max_by_key(|(_, n)| *n)?.0;
    let reading = |i: &&Item| i.rect.top() + i.rect.left() * 0.25;
    let mine = || items.iter().filter(|i| i.layer == top && !i.canvas);
    mine()
        .filter(|i| i.rect.width() >= 24.0 && i.rect.height() >= 24.0)
        .min_by(|a, b| reading(a).total_cmp(&reading(b)))
        .or_else(|| mine().min_by(|a, b| reading(a).total_cmp(&reading(b))))
        .or_else(|| items.iter().find(|i| i.layer == top))
        .copied()
}

/// The next item from `from` in direction `d`: the nearest one ahead, favouring those in line.
fn step(items: &[Item], current: Option<Id>, from: Rect, d: Dir) -> Option<Item> {
    // Ordinary items first; a canvas (the passive tree) only when nothing else lies that way, so
    // the D-pad can always get from a header's buttons back onto it.
    nearest(items, current, from, d, false).or_else(|| nearest(items, current, from, d, current.is_some()))
}

fn nearest(items: &[Item], current: Option<Id>, from: Rect, d: Dir, canvases: bool) -> Option<Item> {
    let dir = d.vec();
    let mut best: Option<(f32, Item)> = None;
    for i in items {
        if Some(i.id) == current || (current.is_some() && i.canvas != canvases) {
            continue;
        }
        // How far ahead (between facing edges) and how far off the line.
        let (ahead, off) = match d {
            Dir::Right => (i.rect.left() - from.right(), gap(i.rect.y_range(), from.y_range())),
            Dir::Left => (from.left() - i.rect.right(), gap(i.rect.y_range(), from.y_range())),
            Dir::Down => (i.rect.top() - from.bottom(), gap(i.rect.x_range(), from.x_range())),
            Dir::Up => (from.top() - i.rect.bottom(), gap(i.rect.x_range(), from.x_range())),
        };
        let centre_ahead = (i.rect.center() - from.center()).dot(dir);
        if ahead < -2.0 && centre_ahead <= 4.0 {
            continue;
        }
        let score = ahead.max(0.0) + off * 2.5 + if ahead < -2.0 { 30.0 } else { 0.0 };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, *i));
        }
    }
    best.map(|(_, i)| i)
}

/// The nearest point from `from` in direction `d`, favouring those in line.
fn snap(points: &[Pos2], from: Pos2, d: Dir) -> Option<Pos2> {
    let dir = d.vec();
    points
        .iter()
        .filter_map(|q| {
            let to = *q - from;
            let ahead = to.dot(dir);
            (ahead > 4.0).then(|| (*q, ahead + (to - dir * ahead).length() * 2.5))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(q, _)| q)
}

/// Wide and short: a slider's rail (or a single-line text field).
fn slider_shaped(r: Rect) -> bool {
    r.width() > r.height() * 3.0 && r.height() <= 64.0
}

/// How far apart two ranges are (0 when they overlap).
fn gap(a: egui::Rangef, b: egui::Rangef) -> f32 {
    if a.max < b.min {
        b.min - a.max
    } else if b.max < a.min {
        a.min - b.max
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn item(id: u32, x: f32, y: f32) -> Item {
        Item {
            id: Id::new(id),
            rect: Rect::from_min_size(pos2(x, y), vec2(40.0, 40.0)),
            layer: LayerId::new(Order::Middle, Id::new("w")),
            slider: false,
            canvas: false,
        }
    }

    #[test]
    fn steps_go_to_the_nearest_item_in_line() {
        // A grid of 3 x 2 cells, 50 apart.
        let items: Vec<Item> = (0..6).map(|k| item(k, (k % 3) as f32 * 50.0, (k / 3) as f32 * 50.0)).collect();
        let at = |k: usize| items[k].rect;
        assert_eq!(step(&items, Some(items[0].id), at(0), Dir::Right).unwrap().id, items[1].id);
        assert_eq!(step(&items, Some(items[0].id), at(0), Dir::Down).unwrap().id, items[3].id);
        assert_eq!(step(&items, Some(items[4].id), at(4), Dir::Up).unwrap().id, items[1].id);
        assert_eq!(step(&items, Some(items[5].id), at(5), Dir::Left).unwrap().id, items[4].id);
        assert!(step(&items, Some(items[2].id), at(2), Dir::Right).is_none(), "nothing past the edge");
        // A wide button under the grid is reached from any column.
        let mut with_button = items.clone();
        with_button.push(Item { rect: Rect::from_min_size(pos2(0.0, 110.0), vec2(140.0, 30.0)), ..item(9, 0.0, 0.0) });
        assert_eq!(step(&with_button, Some(items[5].id), at(5), Dir::Down).unwrap().id, Id::new(9u32));
        assert_eq!(first(&with_button).unwrap().id, items[0].id);
    }

    #[test]
    fn a_header_button_steps_down_into_its_canvas() {
        // "Reset all" over the tree's canvas: down goes onto the canvas (nothing else is there),
        // while a button beside it still wins sideways.
        let reset = item(1, 0.0, 0.0);
        let centre = item(2, 50.0, 0.0);
        let canvas = Item { rect: Rect::from_min_size(pos2(0.0, 60.0), vec2(600.0, 400.0)), canvas: true, ..item(3, 0.0, 0.0) };
        let items = [reset, centre, canvas];
        assert_eq!(step(&items, Some(reset.id), reset.rect, Dir::Down).unwrap().id, canvas.id);
        assert_eq!(step(&items, Some(reset.id), reset.rect, Dir::Right).unwrap().id, centre.id);
        assert!(step(&items, Some(reset.id), reset.rect, Dir::Up).is_none());
    }

    #[test]
    fn the_dpad_snaps_to_points_on_a_canvas() {
        let pts = [pos2(100.0, 100.0), pos2(160.0, 104.0), pos2(150.0, 40.0), pos2(40.0, 100.0)];
        assert_eq!(snap(&pts, pos2(100.0, 100.0), Dir::Right), Some(pos2(160.0, 104.0)));
        assert_eq!(snap(&pts, pos2(100.0, 100.0), Dir::Up), Some(pos2(150.0, 40.0)));
        assert_eq!(snap(&pts, pos2(100.0, 100.0), Dir::Left), Some(pos2(40.0, 100.0)));
        assert_eq!(snap(&pts, pos2(100.0, 100.0), Dir::Down), None);
    }

    #[test]
    fn the_stick_pushes_and_repeats() {
        assert_eq!(stick_dir(glam::Vec2::new(0.9, 0.1)), Some(Dir::Right));
        assert_eq!(stick_dir(glam::Vec2::new(0.1, -0.8)), Some(Dir::Down));
        assert_eq!(stick_dir(glam::Vec2::new(0.2, 0.2)), None);
    }
}

#[cfg(test)]
mod egui_tests {
    use super::*;

    /// One pass of a window with three buttons in a column; returns which was clicked.
    fn pass(ctx: &Context, events: Vec<Event>) -> Option<usize> {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut clicked = None;
        let mut out = ctx.run_ui(raw, |ui| {
            egui::Window::new("menu").show(ui.ctx(), |ui| {
                for i in 0..3 {
                    if ui.add_sized([200.0, 40.0], egui::Button::new(format!("item {i}"))).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
        out.textures_delta.clear();
        clicked
    }

    #[test]
    fn the_dpad_walks_a_real_window_and_cross_clicks() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        pass(&ctx, vec![]);
        pass(&ctx, vec![]);
        // Nothing chosen yet: the pad starts on the first item.
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
        assert!(nav.focused().is_some(), "no item found in the window");
        pass(&ctx, ev);
        let first = nav.focused().unwrap();
        // Down: the second item.
        let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Down], dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev);
        let second = nav.focused().unwrap();
        assert!(second.top() > first.top(), "{first:?} -> {second:?}");
        // Cross goes down: the click lands at once, however long it is held.
        let ev = nav.frame(&ctx, &PadIn { cross: true, dt: 0.016, ..Default::default() }, None);
        assert_eq!(pass(&ctx, ev), Some(1));
        let ev = nav.frame(&ctx, &PadIn { cross: true, dt: 2.0, ..Default::default() }, None);
        assert_eq!(pass(&ctx, ev), None, "held, it does not click again");
        let ev = nav.frame(&ctx, &PadIn { cross: false, dt: 0.016, ..Default::default() }, None);
        assert_eq!(pass(&ctx, ev), None);
    }

    #[test]
    fn the_press_that_picks_the_pad_up_only_shows_where_it_is() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        pass(&ctx, vec![]);
        pass(&ctx, vec![]);
        // Cross picks the pad up (or is still held as the menu comes up): no click, held or not.
        for _ in 0..2 {
            let ev = nav.frame(&ctx, &PadIn { cross: true, dt: 0.016, ..Default::default() }, None);
            assert_eq!(pass(&ctx, ev), None, "the first press only shows the pad");
        }
        assert!(nav.focused().is_some(), "it shows where it is");
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev);
        // The next press acts.
        let ev = nav.frame(&ctx, &PadIn { cross: true, dt: 0.016, ..Default::default() }, None);
        assert_eq!(pass(&ctx, ev), Some(0));
    }
}

#[cfg(test)]
mod slider_tests {
    use super::*;

    /// A window like Settings: a choice button, then a slider (with its value box), then a button.
    fn pass(ctx: &Context, events: Vec<Event>, value: &mut f32) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::Window::new("settings").show(ui.ctx(), |ui| {
                let _ = ui.add_sized([300.0, 40.0], egui::Button::new("vibration"));
                ui.spacing_mut().slider_width = 300.0;
                ui.add(egui::Slider::new(value, 0.0..=100.0).step_by(10.0));
                let _ = ui.add_sized([300.0, 40.0], egui::Button::new("size"));
                // A list that scrolls: its scroll bar is no stop.
                egui::ScrollArea::vertical().max_height(90.0).show(ui, |ui| {
                    for i in 0..6 {
                        let _ = ui.add_sized([300.0, 40.0], egui::Button::new(format!("row {i}")));
                    }
                });
            });
        });
        out.textures_delta.clear();
    }

    #[test]
    fn the_dpad_reaches_a_slider_and_moves_it_and_a_finger_on_it_slides() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let mut v = 50.0;
        pass(&ctx, vec![], &mut v);
        pass(&ctx, vec![], &mut v);
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev, &mut v);
        // Down from the first button: the slider's rail (egui's rail only drags, never clicks).
        let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Down], dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev, &mut v);
        let rail = nav.focused().expect("an item");
        assert!(rail.width() > rail.height() * 3.0, "the ring is on the slider: {rail:?}");
        // Right: the slider takes it, a step up.
        for _ in 0..2 {
            let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Right], dt: 0.016, ..Default::default() }, None);
            pass(&ctx, ev, &mut v);
        }
        assert!(v > 50.0, "right moves the slider: {v}");
        // From the top, left three times, frames apart (as a pad's taps come): it comes down.
        v = 100.0;
        for _ in 0..3 {
            let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Left], dt: 0.016, ..Default::default() }, None);
            pass(&ctx, ev, &mut v);
            for _ in 0..3 {
                let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
                pass(&ctx, ev, &mut v);
            }
        }
        assert_eq!(v, 70.0, "three steps left");
        assert_eq!(nav.focused(), Some(rail), "the ring stays on the slider");
        // A finger landing on the rail slides it: touch won't flick the window away.
        assert!(slider_at(&ctx, rail.center()));
        assert!(!slider_at(&ctx, rail.center() + vec2(0.0, 30.0)), "not on the button below");
        // The window's resize edges and the list's scroll bar (thin strips that drag) are neither
        // sliders nor stops.
        let strips: Vec<Id> = ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, w)| w.iter())
                .filter(|r| r.sense.senses_drag() && r.rect.is_positive() && r.rect.width().min(r.rect.height()) < 16.0)
                .map(|r| r.id)
                .collect()
        });
        assert!(!strips.is_empty(), "the window has thin strips that drag");
        assert!(collect_items(&ctx).iter().all(|i| !strips.contains(&i.id)));
    }
}

#[cfg(test)]
mod opening_tests {
    use super::*;

    #[test]
    fn a_menu_drawn_a_frame_late_still_starts_on_its_first_item() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let run = |ctx: &Context, events: Vec<Event>, show: bool| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                if show {
                    egui::Window::new("m").title_bar(false).fixed_pos([300.0, 200.0]).show(ui.ctx(), |ui| {
                        for i in 0..5 {
                            let _ = ui.add_sized([200.0, 40.0], egui::Button::new(format!("b{i}")));
                        }
                    });
                }
            });
            out.textures_delta.clear();
        };
        run(&ctx, vec![], false);
        // The menu opens: the pad's first frame sees nothing yet.
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
        assert!(nav.pointer.is_none());
        run(&ctx, ev, true);
        // (egui keeps a new window invisible for its first pass, while it sizes it.)
        for _ in 0..2 {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
            run(&ctx, ev, true);
        }
        let top = nav.focused().expect("an item");
        assert!(top.top() < 215.0, "starts on the first button, not the middle one: {top:?}");
    }
}

#[cfg(test)]
mod canvas_focus_tests {
    use super::*;

    /// A window with a tab over a big canvas that asks for the pad (the passive tree).
    fn run(ctx: &Context, events: Vec<Event>) -> Id {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut canvas = Id::NULL;
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("tree").title_bar(false).fixed_pos([20.0, 20.0]).show(&ctx, |ui| {
                let _ = ui.add_sized([120.0, 40.0], egui::Button::new("Bag"));
                let (_, r) = ui.allocate_exact_size(vec2(900.0, 560.0), egui::Sense::click_and_drag());
                canvas = r.id;
                ui.ctx().data_mut(|d| d.insert_temp(Id::new(FOCUS), r.id));
            });
        });
        out.textures_delta.clear();
        canvas
    }

    #[test]
    fn a_pad_picked_up_lands_on_the_canvas_the_screen_asks_for() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let mut canvas = run(&ctx, vec![]);
        for _ in 0..3 {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
            canvas = run(&ctx, ev);
        }
        // On the canvas (no item ring: the pointer is the pad's there), not on the tab above it.
        assert_eq!(nav.focused(), None, "not on the tab");
        let rect = ctx.viewport(|v| v.prev_pass.widgets.get(canvas).map(|w| w.rect)).expect("drawn");
        assert!(nav.pointer.is_some_and(|p| rect.contains(p)), "the pointer is on the canvas: {:?} in {rect:?}", nav.pointer);
    }

    /// What one pass of `scene` drew: the canvas's and the tab's ids, the dialog's button, and
    /// whether the tab was clicked.
    struct Drawn {
        canvas: Id,
        tab: Id,
        dialog: Option<Id>,
        tab_clicked: bool,
    }

    /// The tree's shape: a tab over a canvas that asks for the pad; while `dialog`, a mastery's
    /// options over it instead, which ask for their option.
    fn scene(ctx: &Context, events: Vec<Event>, dialog: bool) -> Drawn {
        sized(ctx, events, dialog, vec2(900.0, 560.0))
    }

    /// The same with the canvas `size` (the UI's scale changed, a phone turned).
    fn sized(ctx: &Context, events: Vec<Event>, dialog: bool, size: EVec2) -> Drawn {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut drawn = Drawn { canvas: Id::NULL, tab: Id::NULL, dialog: None, tab_clicked: false };
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("tree").title_bar(false).fixed_pos([20.0, 20.0]).show(&ctx, |ui| {
                let tab = ui.add_sized([120.0, 40.0], egui::Button::new("Bag"));
                drawn.tab = tab.id;
                drawn.tab_clicked = tab.clicked();
                let (_, r) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
                drawn.canvas = r.id;
                if !dialog {
                    ui.ctx().data_mut(|d| d.insert_temp(Id::new(FOCUS), r.id));
                }
            });
            if dialog {
                egui::Window::new("options").fixed_pos([400.0, 300.0]).show(&ctx, |ui| {
                    let _ = ui.button("first");
                    let chosen = ui.button("chosen");
                    drawn.dialog = Some(chosen.id);
                    ui.ctx().data_mut(|d| d.insert_temp(Id::new(FOCUS), chosen.id));
                });
            }
        });
        out.textures_delta.clear();
        drawn
    }

    fn rect(ctx: &Context, id: Id) -> Rect {
        ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect)).expect("drawn")
    }

    #[test]
    fn a_drag_that_leaves_the_canvas_lets_go_when_cross_comes_up() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        scene(&ctx, vec![], false);
        let go = |nav: &mut PadNav, input: PadIn| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..input }, None);
            scene(&ctx, ev, false)
        };
        let mut drawn = go(&mut nav, PadIn::default());
        for _ in 0..2 {
            drawn = go(&mut nav, PadIn::default());
        }
        // Under the tab, still on the canvas.
        let tab = rect(&ctx, drawn.tab);
        for _ in 0..80 {
            if nav.pointer.is_some_and(|p| p.x < tab.center().x) {
                break;
            }
            go(&mut nav, PadIn { stick: glam::Vec2::new(-1.0, 0.0), ..Default::default() });
        }
        assert!(nav.free(), "still on the canvas");
        // Cross held, the stick up: the drag carries the pointer onto the tab.
        let mut clicked = false;
        for _ in 0..80 {
            if nav.pointer.is_some_and(|p| tab.contains(p)) {
                break;
            }
            drawn = go(&mut nav, PadIn { cross: true, stick: glam::Vec2::new(0.0, 1.0), ..Default::default() });
            clicked |= drawn.tab_clicked;
        }
        assert!(nav.pointer.is_some_and(|p| tab.contains(p)), "the drag reached the tab: {:?} {tab:?}", nav.pointer);
        assert!(ctx.input(|i| i.pointer.primary_down()), "held while dragging");
        // Cross comes up over the tab: egui lets go, and the tab isn't clicked on the way.
        for _ in 0..2 {
            drawn = go(&mut nav, PadIn::default());
            clicked |= drawn.tab_clicked;
        }
        assert!(!ctx.input(|i| i.pointer.any_down()), "let go");
        assert!(!clicked, "a drag across the tab doesn't click it");
    }

    #[test]
    fn a_pad_comes_back_to_where_it_was_on_the_canvas_when_a_dialog_closes() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        scene(&ctx, vec![], false);
        let go = |nav: &mut PadNav, input: PadIn, dialog: bool| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..input }, None);
            scene(&ctx, ev, dialog)
        };
        let mut drawn = go(&mut nav, PadIn::default(), false);
        for _ in 0..2 {
            drawn = go(&mut nav, PadIn::default(), false);
        }
        // Off the canvas's middle, to a node.
        for _ in 0..12 {
            drawn = go(&mut nav, PadIn { stick: glam::Vec2::new(1.0, -0.5), ..Default::default() }, false);
        }
        let node = nav.pointer.expect("on the canvas");
        assert!(node.distance(rect(&ctx, drawn.canvas).center()) > 100.0, "moved off the middle: {node:?}");
        // The node's options open over the tree: the pad is on the option they ask for.
        for _ in 0..3 {
            drawn = go(&mut nav, PadIn::default(), true);
        }
        assert_eq!(nav.focused(), Some(rect(&ctx, drawn.dialog.expect("drawn"))), "on the option asked for");
        // They close: back on the node, not the canvas's middle.
        for _ in 0..3 {
            go(&mut nav, PadIn::default(), false);
        }
        assert!(nav.free(), "on the canvas");
        assert!(nav.pointer.is_some_and(|p| p.distance(node) < 1.0), "back at {node:?}: {:?}", nav.pointer);
    }

    #[test]
    fn a_canvas_that_changed_size_under_a_dialog_takes_the_pad_to_its_middle() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        scene(&ctx, vec![], false);
        let go = |nav: &mut PadNav, input: PadIn, dialog: bool, size: EVec2| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..input }, None);
            sized(&ctx, ev, dialog, size)
        };
        let (big, small) = (vec2(900.0, 560.0), vec2(700.0, 480.0));
        for _ in 0..3 {
            go(&mut nav, PadIn::default(), false, big);
        }
        for _ in 0..12 {
            go(&mut nav, PadIn { stick: glam::Vec2::new(-0.6, -0.5), ..Default::default() }, false, big);
        }
        let node = nav.pointer.expect("on the canvas");
        // The dialog is up while the UI's scale changes: the node is drawn elsewhere now.
        for _ in 0..3 {
            go(&mut nav, PadIn::default(), true, big);
        }
        let mut drawn = go(&mut nav, PadIn::default(), true, small);
        for _ in 0..3 {
            drawn = go(&mut nav, PadIn::default(), false, small);
        }
        let middle = rect(&ctx, drawn.canvas).center();
        assert!(
            nav.pointer.is_some_and(|p| p.distance(middle) < 1.0),
            "at the middle {middle:?}, not the old {node:?}: {:?}",
            nav.pointer
        );
    }

    #[test]
    fn a_pad_picked_up_with_the_mouse_on_the_canvas_starts_where_it_points() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let drawn = scene(&ctx, vec![], false);
        scene(&ctx, vec![], false);
        let at = rect(&ctx, drawn.canvas).center() + vec2(-210.0, 130.0);
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, Some(at));
        scene(&ctx, ev, false);
        assert!(nav.free(), "on the canvas");
        assert_eq!(nav.pointer, Some(at), "where the mouse was, not the canvas's middle");
    }

    #[test]
    fn shift_held_on_a_button_is_let_go_when_the_pad_leaves() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let drawn = scene(&ctx, vec![], false);
        scene(&ctx, vec![], false);
        let tab = rect(&ctx, drawn.tab);
        let go = |nav: &mut PadNav, input: PadIn, mouse: Option<Pos2>| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..input }, mouse);
            scene(&ctx, ev, false);
        };
        // Picked up with the mouse on the tab, then Triangle (Shift) held there.
        go(&mut nav, PadIn::default(), Some(tab.center()));
        go(&mut nav, PadIn::default(), None);
        assert_eq!(nav.focused(), Some(tab), "on the tab");
        go(&mut nav, PadIn { triangle: true, ..Default::default() }, None);
        assert!(ctx.input(|i| i.modifiers.shift), "Shift down");
        let out = nav.release();
        assert!(
            out.iter().any(|e| matches!(e, Event::ModifiersChanged(m) if !m.shift)),
            "the pad lets go of Shift as it leaves: {out:?}"
        );
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    /// A window of rows; while `popup`, a chooser over it (on top) that wants the pad on its third
    /// card. Each names where the pad should land (`FOCUS`), as the Skills window does.
    fn run(ctx: &Context, events: Vec<Event>, popup: bool) {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("rows").title_bar(false).fixed_pos([50.0, 50.0]).show(&ctx, |ui| {
                for i in 0..5 {
                    let (_, r) = ui.allocate_space(vec2(240.0, 44.0));
                    let _ = ui.interact(r, Id::new(("row", i)), egui::Sense::click());
                }
            });
            ctx.data_mut(|d| d.insert_temp(Id::new(FOCUS), Id::new(("row", 3))));
            if popup {
                egui::Window::new("chooser").title_bar(false).order(Order::Foreground).fixed_pos([400.0, 80.0]).show(
                    &ctx,
                    |ui| {
                        for i in 0..6 {
                            let (_, r) = ui.allocate_space(vec2(240.0, 44.0));
                            let _ = ui.interact(r, Id::new(("card", i)), egui::Sense::click());
                        }
                    },
                );
                ctx.data_mut(|d| d.insert_temp(Id::new(FOCUS), Id::new(("card", 2))));
            }
        });
        out.textures_delta.clear();
    }

    fn rect_of(ctx: &Context, id: Id) -> Rect {
        ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect)).expect("drawn")
    }

    #[test]
    fn a_screen_names_where_the_pad_lands_and_comes_back() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        let step = |nav: &mut PadNav, popup: bool, mouse: Option<Pos2>| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, mouse);
            run(&ctx, ev, popup);
        };
        for _ in 0..3 {
            step(&mut nav, false, None);
        }
        // Picked up, the pad starts at the top (or under the mouse), whatever the screen asks.
        assert_eq!(nav.focused(), Some(rect_of(&ctx, Id::new(("row", 0)))));
        // The chooser opens over it: the pad lands on the card it names, not its first.
        for _ in 0..3 {
            step(&mut nav, true, None);
        }
        assert_eq!(nav.focused(), Some(rect_of(&ctx, Id::new(("card", 2)))));
        // Closed: back on the row it was opened from.
        for _ in 0..3 {
            step(&mut nav, false, None);
        }
        assert_eq!(nav.focused(), Some(rect_of(&ctx, Id::new(("row", 3)))));
        // Put down and picked up again with the mouse on the second row: it starts there.
        nav.reset();
        let second = rect_of(&ctx, Id::new(("row", 1)));
        step(&mut nav, false, Some(second.center()));
        assert_eq!(nav.focused(), Some(second));
    }
}

#[cfg(test)]
mod hover_tests {
    use super::*;

    #[test]
    fn the_pad_picks_up_where_the_mouse_points() {
        let ctx = Context::default();
        let run = |ctx: &Context, events: Vec<Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::Window::new("m").title_bar(false).fixed_pos([300.0, 100.0]).show(ui.ctx(), |ui| {
                    for i in 0..5 {
                        let _ = ui.add_sized([200.0, 40.0], egui::Button::new(format!("b{i}")));
                    }
                });
            });
            out.textures_delta.clear();
        };
        for _ in 0..3 {
            run(&ctx, vec![]);
        }
        // The mouse rests on the third button (hover); then the pad takes over.
        let third = ctx.viewport(|v| {
            let mut rects: Vec<Rect> = v
                .prev_pass
                .widgets
                .layer_ids()
                .flat_map(|l| v.prev_pass.widgets.get_layer(l).map(|w| w.rect).collect::<Vec<_>>())
                .collect();
            rects.retain(|r| r.width() > 150.0 && r.height() < 60.0);
            rects.sort_by(|a, b| a.top().total_cmp(&b.top()));
            rects[2]
        });
        let mut nav = PadNav::default();
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, Some(third.center()));
        run(&ctx, ev);
        assert_eq!(nav.focused(), Some(third), "the pad starts on the hovered item");
        let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Down], dt: 0.016, ..Default::default() }, Some(third.center()));
        run(&ctx, ev);
        assert!(nav.focused().unwrap().top() > third.top(), "and steps on from it");
    }
}

#[cfg(test)]
mod cover_tests {
    use super::*;

    /// A window of three items, and a panel without items drawn over the middle one (two panels
    /// the keys opened, one over the other).
    fn pass(ctx: &Context, events: Vec<Event>) -> [Id; 3] {
        drawn(ctx, events, true)
    }

    /// The same, the panel over it only while `front` (a toast that comes and goes).
    fn drawn(ctx: &Context, events: Vec<Event>, front: bool) -> [Id; 3] {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut ids = [Id::NULL; 3];
        let mut out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("back").title_bar(false).fixed_pos([20.0, 20.0]).show(&ctx, |ui| {
                for (i, id) in ids.iter_mut().enumerate() {
                    *id = ui.add_sized([200.0, 40.0], egui::Button::new(format!("item {i}"))).id;
                }
            });
            if front {
                let panel = egui::Window::new("front").id(Id::new("front")).title_bar(false).resizable(false);
                panel.fixed_pos([10.0, 66.0]).show(&ctx, |ui| {
                    ui.set_min_size(vec2(260.0, 36.0));
                    ui.label("over");
                });
            }
        });
        out.textures_delta.clear();
        ids
    }

    fn rect(ctx: &Context, id: Id) -> Rect {
        ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect)).expect("drawn")
    }

    #[test]
    fn an_item_another_window_covers_is_stepped_over() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        pass(&ctx, vec![]);
        let ids = pass(&ctx, vec![]);
        let front = ctx.memory(|m| m.area_rect(Id::new("front"))).expect("drawn");
        assert!(front.contains_rect(rect(&ctx, ids[1])), "the panel covers the middle item: {front:?}");
        let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev);
        assert_eq!(nav.focused(), Some(rect(&ctx, ids[0])), "on the first item");
        let ev = nav.frame(&ctx, &PadIn { taps: vec![Dir::Down], dt: 0.016, ..Default::default() }, None);
        pass(&ctx, ev);
        assert_eq!(nav.focused(), Some(rect(&ctx, ids[2])), "over the covered one, to the third");
    }
    #[test]
    fn a_toast_over_the_item_the_pad_is_on_leaves_it_there() {
        let ctx = Context::default();
        let mut nav = PadNav::default();
        drawn(&ctx, vec![], false);
        let ids = drawn(&ctx, vec![], false);
        let go = |nav: &mut PadNav, input: PadIn, front: bool| {
            let ev = nav.frame(&ctx, &PadIn { dt: 0.016, ..input }, None);
            drawn(&ctx, ev, front);
        };
        go(&mut nav, PadIn::default(), false);
        go(&mut nav, PadIn { taps: vec![Dir::Down], ..Default::default() }, false);
        let middle = rect(&ctx, ids[1]);
        assert_eq!(nav.focused(), Some(middle), "on the middle item");
        // Something passes over it (a toast), then goes: the pad stays on it throughout.
        for front in [true, true, true, false, false] {
            go(&mut nav, PadIn::default(), front);
            assert_eq!(nav.focused(), Some(middle), "still on the middle item (front: {front})");
        }
    }
}
