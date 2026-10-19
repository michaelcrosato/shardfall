//! The passive tree window: a pan-and-zoom canvas of the generated tree. Hover a node for what
//! it does; click to take it (shift-click takes the whole path to it), right-click to give it
//! back for gold; allocated masteries open their options. Search lights up matching nodes.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2 as EVec2};
use glam::Vec2;
use pav_core::arpg::GameCmd;
use pav_core::arpg::InvView;
use pav_core::arpg::data::data;
use pav_core::arpg::tree::{Node, NodeKind, Tree, start_id};

pub struct TreeUi {
    pub open: bool,
    /// Canvas centre in tree units and pixels per tree unit.
    pan: Vec2,
    zoom: f32,
    /// On a gamepad: hints name its buttons.
    pub pad: bool,
    /// On a touch screen: the first tap on a node shows it, the second takes it; two fingers zoom.
    pub touch: bool,
    /// Touch: the node shown.
    picked: Option<u32>,
    search: String,
    /// Mastery node whose options are shown.
    mastery: Option<u32>,
}

impl Default for TreeUi {
    fn default() -> Self {
        Self {
            open: false,
            pan: Vec2::ZERO,
            zoom: 21.0,
            search: String::new(),
            mastery: None,
            pad: false,
            touch: false,
            picked: None,
        }
    }
}

fn col(c: [f32; 3], k: f32) -> Color32 {
    let to = |x: f32| ((x * k).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(to(c[0]), to(c[1]), to(c[2]))
}

fn radius(n: &Node) -> f32 {
    match n.kind {
        NodeKind::Start => 0.55,
        NodeKind::Keystone => 0.5,
        NodeKind::Notable => 0.36,
        NodeKind::Mastery => 0.34,
        NodeKind::Skill => 0.27,
        NodeKind::Astral if n.lore == "star" => 0.3,
        NodeKind::Minor | NodeKind::Astral => 0.2,
    }
}

impl TreeUi {
    pub fn ui(&mut self, ctx: &egui::Context, inv: &InvView, out: &mut Vec<GameCmd>) {
        if !self.open {
            return;
        }
        let d = data();
        let t: &Tree = &d.tree;
        let screen = ctx.content_rect();
        let mut open = true;
        // Touch screens: nearly the whole screen, below the buttons in the top right.
        let area = if self.touch {
            Rect::from_min_max(screen.min + EVec2::new(8.0, 58.0), screen.max - EVec2::splat(8.0))
        } else {
            screen.shrink2(EVec2::new(screen.width() * 0.06, screen.height() * 0.06))
        };
        egui::Window::new("Passive Tree")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_rect(area)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} points to spend", inv.points)).strong().color(if inv.points > 0 {
                        Color32::from_rgb(255, 215, 120)
                    } else {
                        Color32::from_white_alpha(180)
                    }));
                    ui.separator();
                    ui.label(format!("{} allocated", inv.tree.len()));
                    ui.separator();
                    if !self.touch {
                        ui.label("Search:");
                        ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(160.0));
                        ui.separator();
                    }
                    if ui.button(format!("Reset all ({} gold)", inv.respec_cost)).clicked() {
                        out.push(GameCmd::Respec);
                    }
                    if ui.button("Centre").clicked() {
                        self.pan = Vec2::ZERO;
                        self.zoom = 21.0;
                    }
                    let help = if self.touch {
                        "tap: show · tap again: take · drag: pan · pinch: zoom".to_string()
                    } else {
                        crate::arpg_items::hint(self.pad, "click: take · shift-click: take the path · right-click: refund · drag: pan · wheel: zoom · P to close")
                    };
                    ui.label(RichText::new(help).small().color(Color32::from_white_alpha(120)));
                });
                let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 4.0, Color32::from_rgb(14, 13, 18));
                // Pan and zoom.
                if resp.dragged() {
                    let dlt = resp.drag_delta();
                    self.pan -= Vec2::new(dlt.x, dlt.y) / self.zoom;
                }
                // Two fingers: pinch to zoom round their middle.
                if let Some(mt) = ui.input(|i| i.multi_touch()).filter(|m| rect.contains(m.center_pos)) {
                    let before = self.to_tree(rect, mt.center_pos);
                    self.zoom = (self.zoom * mt.zoom_delta).clamp(4.0, 90.0);
                    self.pan += before - self.to_tree(rect, mt.center_pos);
                }
                if resp.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        let before = self.to_tree(rect, ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()));
                        self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(4.0, 90.0);
                        let after = self.to_tree(rect, ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()));
                        self.pan += before - after;
                    }
                }
                let alloc = &inv.tree;
                let reached = |id: u32| id == start_id() || alloc.contains(&id);
                let search = self.search.to_lowercase();
                let matches = |n: &Node| {
                    !search.is_empty()
                        && (n.name.to_lowercase().contains(&search)
                            || t.describe(n).iter().any(|l| l.to_lowercase().contains(&search)))
                };
                let view = rect.expand(30.0);
                // Hovered node and the path to it (touch: the node tapped, then the one shown).
                let hover_pos = if self.touch { resp.clicked().then(|| resp.interact_pointer_pos()).flatten() } else { resp.hover_pos() };
                let tapped = hover_pos.and_then(|hp| {
                    let at = self.to_tree(rect, hp);
                    t.nodes
                        .iter()
                        .filter(|n| (n.pos - at).length() <= radius(n) + 6.0 / self.zoom)
                        .min_by(|a, b| (a.pos - at).length().total_cmp(&(b.pos - at).length()))
                });
                let mut take_tap = false;
                if self.touch {
                    if resp.clicked() {
                        // A second tap on the node shown takes it; a tap on nothing hides it.
                        take_tap = tapped.is_some_and(|n| self.picked == Some(n.id));
                        self.picked = tapped.map(|n| n.id);
                    }
                    // A second tap on a mastery that is taken opens its options.
                    if take_tap && self.picked.and_then(|id| t.node(id)).is_some_and(|n| reached(n.id) && n.kind == NodeKind::Mastery) {
                        self.mastery = self.picked;
                    }
                }
                let hovered = if self.touch { self.picked.and_then(|id| t.node(id)) } else { tapped };
                let path: Vec<u32> = hovered.filter(|n| !reached(n.id)).and_then(|n| t.path_to(alloc, n.id)).unwrap_or_default();
                // Links.
                for n in &t.nodes {
                    let a = self.to_screen(rect, n.pos);
                    if !view.contains(a) && n.links.iter().all(|l| t.node(*l).is_none_or(|m| !view.contains(self.to_screen(rect, m.pos)))) {
                        continue;
                    }
                    for l in &n.links {
                        let Some(m) = t.node(*l) else { continue };
                        if m.id < n.id {
                            continue;
                        }
                        let b = self.to_screen(rect, m.pos);
                        let both = reached(n.id) && reached(m.id);
                        let on_path = (path.contains(&n.id) || reached(n.id)) && (path.contains(&m.id) || reached(m.id)) && !both;
                        let (w, c) = if both {
                            (3.0, Color32::from_rgb(235, 205, 130))
                        } else if on_path {
                            (2.5, Color32::from_rgb(120, 220, 140))
                        } else if reached(n.id) || reached(m.id) {
                            (1.8, Color32::from_rgb(120, 110, 100))
                        } else {
                            (1.2, Color32::from_rgb(52, 48, 56))
                        };
                        p.line_segment([a, b], Stroke::new(w, c));
                    }
                }
                // Nodes.
                for n in &t.nodes {
                    let c = self.to_screen(rect, n.pos);
                    if !view.contains(c) {
                        continue;
                    }
                    let r = (radius(n) * self.zoom).max(2.0);
                    let sector = t.sectors.get(n.sector as usize).map(|s| s.color).unwrap_or([0.7; 3]);
                    let taken = reached(n.id);
                    let can = !taken && t.can_allocate(alloc, n.id);
                    let fill = if taken {
                        col(sector, 1.25)
                    } else if can {
                        col(sector, 0.6)
                    } else {
                        col(sector, 0.28)
                    };
                    let ring = if taken {
                        Color32::from_rgb(250, 230, 160)
                    } else if can || path.contains(&n.id) {
                        Color32::from_rgb(150, 230, 150)
                    } else {
                        Color32::from_rgb(80, 76, 84)
                    };
                    match n.kind {
                        NodeKind::Skill => {
                            let pts = vec![c + EVec2::new(0.0, -r), c + EVec2::new(r, 0.0), c + EVec2::new(0.0, r), c + EVec2::new(-r, 0.0)];
                            p.add(Shape::convex_polygon(pts, fill, Stroke::new(1.5, if taken { ring } else { Color32::from_rgb(120, 200, 255) })));
                        }
                        NodeKind::Mastery => {
                            let pts: Vec<Pos2> = (0..6)
                                .map(|i| {
                                    let a = i as f32 * std::f32::consts::TAU / 6.0 + 0.5;
                                    c + EVec2::new(a.cos(), a.sin()) * r
                                })
                                .collect();
                            p.add(Shape::convex_polygon(pts, fill, Stroke::new(1.5, ring)));
                            if inv.masteries.contains_key(&n.id) {
                                p.circle_filled(c, r * 0.35, Color32::from_rgb(255, 230, 150));
                            }
                        }
                        NodeKind::Keystone => {
                            p.circle_filled(c, r, fill);
                            p.circle_stroke(c, r, Stroke::new(2.5, ring));
                            p.circle_stroke(c, r * 0.7, Stroke::new(1.0, ring));
                        }
                        _ => {
                            p.circle_filled(c, r, fill);
                            p.circle_stroke(c, r, Stroke::new(if n.kind == NodeKind::Notable { 2.0 } else { 1.0 }, ring));
                        }
                    }
                    if matches(n) {
                        p.circle_stroke(c, r + 4.0, Stroke::new(2.0, Color32::from_rgb(255, 240, 80)));
                    }
                    if self.zoom > 45.0 && matches!(n.kind, NodeKind::Notable | NodeKind::Keystone) {
                        p.text(c + EVec2::new(0.0, r + 8.0), Align2::CENTER_CENTER, &n.name, FontId::proportional(11.0), Color32::from_white_alpha(170));
                    }
                }
                // Sector names around the tree.
                for s in &t.sectors {
                    let a = s.angle.to_radians();
                    let at = self.to_screen(rect, Vec2::new(a.sin(), -a.cos()) * 2.0);
                    if self.zoom > 14.0 {
                        p.text(at, Align2::CENTER_CENTER, &s.name, FontId::proportional(13.0), col(s.color, 1.0));
                    }
                }
                // Touch: the node shown, in a panel at the bottom, with what can be done.
                if let (true, Some(n)) = (self.touch, hovered) {
                    let taken = reached(n.id);
                    let c = self.to_screen(rect, n.pos);
                    p.circle_stroke(c, (radius(n) * self.zoom).max(2.0) + 5.0, Stroke::new(2.5, Color32::from_rgb(255, 230, 150)));
                    let panel = Rect::from_min_max(
                        Pos2::new(rect.left() + 8.0, rect.bottom() - (rect.height() * 0.42).min(190.0)),
                        Pos2::new(rect.left() + (rect.width() - 16.0).min(420.0), rect.bottom() - 8.0),
                    );
                    egui::Area::new(egui::Id::new("tree_pick")).fixed_pos(panel.min).order(egui::Order::Foreground).show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_width(panel.width() - 16.0);
                            egui::ScrollArea::vertical().max_height(panel.height() - 60.0).show(ui, |ui| node_info(ui, t, n, inv, taken, &path, false));
                            ui.horizontal_wrapped(|ui| {
                                if !taken && t.can_allocate(alloc, n.id) && (ui.button("Take").clicked() || take_tap) {
                                    out.push(GameCmd::Allocate(n.id));
                                }
                                if !taken && path.len() > 1 && ui.button(format!("Take the path ({} points)", path.len())).clicked() {
                                    for id in path.iter().take(inv.points as usize) {
                                        out.push(GameCmd::Allocate(*id));
                                    }
                                }
                                if taken && n.kind == NodeKind::Mastery && ui.button("Choose an option").clicked() {
                                    self.mastery = Some(n.id);
                                }
                                if taken && n.kind != NodeKind::Start && ui.button(format!("Refund ({} gold)", inv.refund_cost)).clicked() {
                                    out.push(GameCmd::Refund(n.id));
                                }
                                if ui.button("Close").clicked() {
                                    self.picked = None;
                                }
                            });
                        });
                    });
                }
                // Hover: what it does; clicks.
                if let Some(n) = hovered.filter(|_| !self.touch) {
                    let taken = reached(n.id);
                    let mut r2 = resp.clone();
                    r2 = r2.on_hover_ui_at_pointer(|ui| {
                        ui.set_max_width(320.0);
                        node_info(ui, t, n, inv, taken, &path, true);
                    });
                    let shift = ui.input(|i| i.modifiers.shift);
                    if r2.clicked() {
                        if taken && n.kind == NodeKind::Mastery {
                            self.mastery = Some(n.id);
                        } else if !taken && t.can_allocate(alloc, n.id) {
                            out.push(GameCmd::Allocate(n.id));
                        } else if !taken && shift {
                            for id in path.iter().take(inv.points as usize) {
                                out.push(GameCmd::Allocate(*id));
                            }
                        }
                    }
                    if r2.secondary_clicked() && taken && n.kind != NodeKind::Start {
                        out.push(GameCmd::Refund(n.id));
                    }
                }
                // Mastery options.
                if let Some(mid) = self.mastery {
                    let mut keep = true;
                    if let Some(n) = t.node(mid) {
                        if let Some(m) = t.masteries.get(&n.mastery) {
                            egui::Window::new(&m.name).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, EVec2::ZERO).show(ctx, |ui| {
                                for (i, (name, stats, power)) in m.options.iter().enumerate() {
                                    let mut text = name.clone();
                                    for (s, v) in stats {
                                        text += &format!("\n   {}", pav_core::arpg::stats::describe(*s, *v));
                                    }
                                    if let Some(p) = power {
                                        text += &format!("\n   {}", p.describe());
                                    }
                                    let chosen = inv.masteries.get(&mid) == Some(&(i as u8));
                                    let taken_elsewhere = inv.masteries.iter().any(|(nid, o)| {
                                        *nid != mid && *o == i as u8 && t.node(*nid).is_some_and(|x| x.mastery == n.mastery)
                                    });
                                    let b = ui.add_enabled(!taken_elsewhere, egui::Button::selectable(chosen, text));
                                    if b.clicked() {
                                        out.push(GameCmd::Mastery(mid, i as u8));
                                        keep = false;
                                    }
                                }
                                if ui.button("Close").clicked() {
                                    keep = false;
                                }
                            });
                        }
                    }
                    if !keep {
                        self.mastery = None;
                    }
                }
                // Legend.
                let legend = rect.left_bottom() + EVec2::new(12.0, -14.0);
                p.text(
                    legend,
                    Align2::LEFT_BOTTOM,
                    "diamonds: skill upgrades · hexagons: masteries · double rings: keystones · large: notables · outer rings: the endless Astral",
                    FontId::proportional(11.0),
                    Color32::from_white_alpha(120),
                );
            });
        if !open {
            self.open = false;
            self.mastery = None;
        }
    }

    fn to_screen(&self, rect: Rect, p: Vec2) -> Pos2 {
        let c = rect.center();
        Pos2::new(c.x + (p.x - self.pan.x) * self.zoom, c.y + (p.y - self.pan.y) * self.zoom)
    }

    fn to_tree(&self, rect: Rect, p: Pos2) -> Vec2 {
        let c = rect.center();
        Vec2::new((p.x - c.x) / self.zoom + self.pan.x, (p.y - c.y) / self.zoom + self.pan.y)
    }
}

/// What a node does (its kind, stats, mastery options, lore); `hints` adds the mouse's actions.
fn node_info(ui: &mut egui::Ui, t: &Tree, n: &Node, inv: &InvView, taken: bool, path: &[u32], hints: bool) {
    let kind = match n.kind {
        NodeKind::Start => "Start",
        NodeKind::Minor => "Passive",
        NodeKind::Notable => "Notable",
        NodeKind::Keystone => "Keystone",
        NodeKind::Mastery => "Mastery",
        NodeKind::Skill => "Skill upgrade",
        NodeKind::Astral => "Astral (endless ring)",
    };
    ui.label(RichText::new(&n.name).strong().size(16.0));
    ui.label(
        RichText::new(if n.ring > 0 { format!("{kind} · ring {}", n.ring) } else { kind.to_string() })
            .small()
            .color(Color32::from_white_alpha(140)),
    );
    for l in t.describe(n) {
        ui.label(RichText::new(l).color(Color32::from_rgb(150, 180, 255)));
    }
    if n.kind == NodeKind::Mastery {
        if let Some(m) = t.masteries.get(&n.mastery) {
            for (i, (name, stats, power)) in m.options.iter().enumerate() {
                let chosen = inv.masteries.get(&n.id) == Some(&(i as u8));
                let mut line = name.clone();
                for (s, v) in stats {
                    line += &format!(" — {}", pav_core::arpg::stats::describe(*s, *v));
                }
                if let Some(p) = power {
                    line += &format!(" — {}", p.describe());
                }
                ui.label(RichText::new(line).color(if chosen {
                    Color32::from_rgb(255, 220, 120)
                } else {
                    Color32::from_white_alpha(170)
                }));
            }
        }
    }
    if !n.lore.is_empty() && n.lore != "star" {
        ui.label(RichText::new(&n.lore).italics().color(Color32::from_rgb(170, 130, 90)));
    }
    if hints {
        ui.separator();
        if taken && n.kind != NodeKind::Start {
            ui.label(RichText::new(format!("Right-click: refund ({} gold)", inv.refund_cost)).small());
            if n.kind == NodeKind::Mastery {
                ui.label(RichText::new("Click: choose an option").small());
            }
        }
    }
    if !taken && !path.is_empty() {
        ui.label(RichText::new(format!("{} point{} away", path.len(), if path.len() == 1 { "" } else { "s" })).small());
    }
}
