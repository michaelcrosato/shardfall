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
    /// The windows with items last frame: a new one (a menu, a popup) takes the pad.
    layers: Vec<LayerId>,
}

/// Where a canvas wants the D-pad to snap (egui temp data under this id: screen points), such as
/// the passive tree's nodes in view.
pub const SNAP_POINTS: &str = "padnav_snap_points";

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
        self.layers.clear();
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
        let items = items(ctx);
        let screen = ctx.content_rect();
        // A window that just opened takes the pad; otherwise keep the item we are on (its rect
        // may have moved), or find one to start from.
        let mut layers: Vec<LayerId> = Vec::new();
        for i in &items {
            if !layers.contains(&i.layer) {
                layers.push(i.layer);
            }
        }
        let opened = layers.iter().any(|l| !self.layers.contains(l));
        self.layers = layers;
        let mut current = self.snapped.and_then(|s| items.iter().find(|i| i.id == s.id).copied()).filter(|_| !opened);
        // The item went (a menu turned a page): start again at the top, not wherever it was.
        let lost = self.snapped.is_some() && current.is_none();
        if opened || lost {
            self.pointer = None;
        }
        let mouse = mouse.filter(|_| !lost);
        if current.is_none() {
            let from = self.pointer.or(mouse);
            current = from
                .and_then(|p| items.iter().filter(|i| i.rect.contains(p)).min_by(|a, b| area(a).total_cmp(&area(b))).copied())
                .or_else(|| first(&items));
            if let Some(c) = current {
                if !c.canvas || self.pointer.is_none() {
                    self.pointer = Some(c.rect.center());
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
        // On a canvas, the item is whatever the pointer is over now.
        if canvas {
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
        for (now, was, button) in [
            (input.cross && !on_slider, self.buttons.0, PointerButton::Primary),
            (input.square, self.buttons.1, PointerButton::Secondary),
        ] {
            if on_canvas {
                // A canvas (the passive tree) is a mouse's: held is held, so holding Cross and
                // moving drags it.
                if now != was {
                    events.push(Event::PointerButton { pos: pointer, button, pressed: now, modifiers });
                }
            } else if now && !was {
                // Elsewhere a press acts once, on the frame it goes down: a whole click, however
                // long the button stays down (and however slow the frame).
                events.push(Event::PointerButton { pos: pointer, button, pressed: true, modifiers });
                events.push(Event::PointerButton { pos: pointer, button, pressed: false, modifiers });
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
        self.snapped = current;
        events
    }

    /// Lets go of the pointer's buttons (the menus closed with one held).
    pub fn release(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        // (Only a canvas holds a button down; elsewhere a press was a whole click.)
        let canvas = self.snapped.is_some_and(|s| s.canvas);
        if let Some(pos) = self.pointer.filter(|_| canvas) {
            for (held, button) in [(self.buttons.0, PointerButton::Primary), (self.buttons.1, PointerButton::Secondary)] {
                if held {
                    out.push(Event::PointerButton { pos, button, pressed: false, modifiers: Default::default() });
                }
            }
            if self.buttons.2 {
                out.push(Event::ModifiersChanged(Default::default()));
            }
        }
        self.buttons = (false, false, false);
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
    items(ctx).iter().any(|i| i.slider && i.rect.contains(pos))
}

/// The clickable items the last frame drew in the open windows (the topmost popup's alone when
/// one is open: a combo box's list), not the world's labels under them.
fn items(ctx: &Context) -> Vec<Item> {
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
    let dir = d.vec();
    let mut best: Option<(f32, Item)> = None;
    for i in items {
        if Some(i.id) == current || i.canvas && current.is_some() {
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
        assert!(items(&ctx).iter().all(|i| !strips.contains(&i.id)));
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
