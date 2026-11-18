//! The passive tree window: a pan-and-zoom canvas of the generated tree. Hover a node for what
//! it does; click to take it (shift-click takes the whole path to it), right-click to give it
//! back for gold; allocated masteries open their options. Search lights up matching nodes.
//!
//! Read from a sofa: it opens on the tree's core (the start and its sectors fill the canvas; the
//! endless Astral rings wait past the edges until the player zooms out), the points to spend
//! are the biggest thing in the header, and what only a mouse uses (the search field, the
//! shapes' legend) shows only with a mouse. Zoomed out, it keeps its shape and the player's
//! choices and lets the ordinary nodes fade (`detail`): an overview, not a field of dots.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2 as EVec2};
use glam::Vec2;
use pav_core::arpg::GameCmd;
use pav_core::arpg::InvView;
use pav_core::arpg::data::data;
use pav_core::arpg::lines;
use pav_core::arpg::tree::{Node, NodeKind, Tree, start_id};
use pav_core::t;

pub struct TreeUi {
    pub open: bool,
    /// Canvas centre in tree units and pixels per tree unit.
    pan: Vec2,
    zoom: f32,
    /// The zoom follows the canvas at this times the zoom that fills it with the tree's core
    /// (`core_zoom`: 1, as the tree opens), until the player zooms (Centre brings it back).
    fit: Option<f32>,
    /// On a gamepad: hints name its buttons.
    pub pad: bool,
    /// On a touch screen: the first tap on a node shows it, the second takes it; two fingers zoom.
    pub touch: bool,
    /// Touch: the node shown.
    picked: Option<u32>,
    search: String,
    /// The search (lower case) and its hits by node index.
    lit: (String, Vec<bool>),
    /// Mastery node whose options are shown.
    mastery: Option<u32>,
}

impl Default for TreeUi {
    fn default() -> Self {
        Self {
            open: false,
            pan: Vec2::ZERO,
            zoom: 21.0,
            fit: Some(1.0),
            search: String::new(),
            lit: Default::default(),
            mastery: None,
            pad: false,
            touch: false,
            picked: None,
        }
    }
}

/// The passive tree window's id (its layer's): a flick across it pans, never closes it.
pub const TREE_WINDOW: &str = "passive_tree";

fn col(c: [f32; 3], k: f32) -> Color32 {
    let to = |x: f32| ((x * k).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(to(c[0]), to(c[1]), to(c[2]))
}

/// The zoom's range, in pixels (points) per tree unit.
const MIN_ZOOM: f32 = 4.0;
const MAX_ZOOM: f32 = 90.0;

/// How far from the start (tree units) the canvas's shorter side shows when the tree opens: the
/// start, the sectors' first levels and their masteries.
const CORE: f32 = 8.0;

/// Pixels per tree unit that fill `rect` with the tree's core, with the Astral rings (which start
/// `astral` units out) past the longer side's edges: zooming out reaches them.
fn core_zoom(rect: Rect, astral: f32) -> f32 {
    let half = rect.size() * 0.5;
    (half.min_elem() / CORE).max(half.max_elem() / (astral - 0.6).max(CORE)).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Where the Astral rings start, in tree units from the start.
fn astral_start(t: &Tree) -> f32 {
    let r = t.nodes.iter().filter(|n| n.ring > 0).map(|n| n.pos.length()).fold(f32::INFINITY, f32::min);
    if r.is_finite() { r } else { t.nodes.iter().map(|n| n.pos.length()).fold(CORE, f32::max) + 1.0 }
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

// Level of detail. Zoomed out past the opening view, hundreds of same-sized dots would hide the
// tree's shape, so the view keeps what shapes it and what the player can do at full strength
// (`kept`) and lets the ordinary minor nodes and their links fade (`detail`): the main tree's to
// tiny dots on faint roads, the Astral rings' to nothing, leaving the rings' stars as their
// outline. At the opening zoom and closer every node is drawn as it always was.

/// The share of the opening zoom (`core_zoom`) at which the ordinary detail has faded out.
const FADED: f32 = 0.25;
/// The radius (points) a main-tree minor node shrinks to, faded out.
const DOT: f32 = 1.0;
/// The least radius (points) a node is drawn at for the D-pad to snap to it (`Look::selectable`).
const SNAP_MIN: f32 = 2.0;

/// How much of the ordinary detail the view draws at `zoom`: 1 at the opening zoom `core` and
/// closer, easing down to 0 at `FADED` times it (no step anywhere, so nothing pops).
fn detail(zoom: f32, core: f32) -> f32 {
    let x = ((zoom / core - FADED) / (1.0 - FADED)).clamp(0.0, 1.0);
    1.0 - (1.0 - x) * (1.0 - x)
}

/// Whether a node keeps full strength however far out the view goes: what gives the tree its shape
/// (the start, notables, keystones, masteries, skill upgrades, the Astral stars) and anything
/// `marked` for the player (taken, takeable now, on the path shown, a search hit, under the pointer).
fn kept(n: &Node, marked: bool) -> bool {
    marked || !matches!(n.kind, NodeKind::Minor | NodeKind::Astral) || n.lore == "star"
}

/// The radius (points) a kept node grows to as the ordinary detail fades, to stand out from it,
/// as far as the tree's spacing at `zoom` leaves room (half a tree unit); the Astral stars, an
/// outline, keep their size.
fn least(n: &Node, zoom: f32) -> f32 {
    let r: f32 = match n.kind {
        NodeKind::Start => 6.0,
        NodeKind::Keystone => 5.5,
        NodeKind::Mastery => 4.5,
        NodeKind::Notable | NodeKind::Skill => 4.0,
        NodeKind::Astral if n.lore == "star" => 2.0,
        NodeKind::Minor | NodeKind::Astral => 3.0,
    };
    r.min(0.5 * zoom).max(2.0)
}

/// How a node is drawn at the view's level of detail.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    /// Radius in points.
    r: f32,
    /// 1 at full strength; less fades its fill and outline (0: not drawn).
    strength: f32,
    /// The outline's share of its width.
    outline: f32,
}

impl Look {
    /// Drawn big and strong enough to point at: the D-pad snaps only to these (a pointer still
    /// names any node under it).
    fn selectable(self) -> bool {
        self.r >= SNAP_MIN && self.strength >= 0.5
    }
}

/// How `n` is drawn at `zoom` and `detail` (`marked`: it is the player's now, see `kept`).
fn look(n: &Node, marked: bool, zoom: f32, detail: f32) -> Look {
    let r = (radius(n) * zoom).max(2.0);
    let fade = 1.0 - detail;
    if kept(n, marked) {
        // (An Astral star sheds its grey ring: the rings' outline is their sectors' colours.)
        let outline = if n.ring > 0 && !marked { detail } else { 1.0 };
        Look { r: r.max(2.0 + (least(n, zoom) - 2.0) * fade), strength: 1.0, outline }
    } else if n.ring > 0 {
        // The Astral rings' ordinary nodes go: their stars outline the rings.
        Look { r: r * detail, strength: detail, outline: detail }
    } else {
        // The main tree's shrink to dots, without their outlines, that keep its roads' shape.
        Look { r: r + (DOT - r) * fade, strength: 1.0 - 0.45 * fade, outline: detail }
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
        egui::Window::new(t!("tree.title"))
            .id(egui::Id::new(TREE_WINDOW))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_rect(area)
            .show(ctx, |ui| {
                // Only a mouse comes with a keyboard to search by, and needs the shapes' legend.
                let mouse = !self.pad && !self.touch;
                ui.horizontal_wrapped(|ui| {
                    // The points to spend, big, and framed in gold while there are some: what the
                    // tree is opened for.
                    let some = inv.points > 0;
                    let gold = Color32::from_rgb(255, 215, 120);
                    egui::Frame::new()
                        .fill(if some { Color32::from_rgba_unmultiplied(255, 200, 90, 30) } else { Color32::TRANSPARENT })
                        .stroke(Stroke::new(1.5, if some { gold } else { Color32::from_white_alpha(40) }))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(10, 2))
                        .show(ui, |ui| {
                            let points = RichText::new(t!("tree.points", count = inv.points)).size(24.0).strong();
                            ui.label(points.color(if some { gold } else { Color32::from_white_alpha(170) }));
                        });
                    ui.label(RichText::new(t!("tree.allocated", count = inv.tree.len())).color(Color32::from_white_alpha(170)));
                    ui.separator();
                    if mouse {
                        ui.label(t!("tree.search"));
                        ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(160.0));
                        ui.separator();
                    }
                    if ui.button(t!("tree.reset", count = inv.respec_cost)).clicked() {
                        out.push(GameCmd::Respec);
                    }
                    if ui.button(t!("tree.centre")).clicked() {
                        self.pan = Vec2::ZERO;
                        self.fit = Some(1.0);
                    }
                });
                if self.pad {
                    // What the pad's buttons do here, drawn as its own buttons.
                    let g = |a: &str| crate::controls::glyphs(a, crate::Device::Gamepad);
                    let both = |a: &str, b: &str| [g(a), g(b)].concat();
                    let prompts = [
                        (g("confirm"), t!("tree.pad.take")),
                        (both("alt", "confirm"), t!("tree.pad.path")),
                        (g("more"), t!("tree.pad.refund")),
                        (g("scroll"), t!("tree.pad.zoom")),
                        (g("back"), t!("hint.close")),
                    ]
                    .map(|(glyphs, label)| crate::glyph::Prompt { glyphs, label });
                    crate::glyph::row_ui(ui, &prompts, 20.0, Color32::from_white_alpha(210));
                } else {
                    let help = if self.touch { t!("tree.help.touch") } else { pav_core::t_rich!("tree.help") };
                    crate::glyph::rich_label(ui, &help, 12.0, Color32::from_white_alpha(150));
                }
                let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 4.0, Color32::from_rgb(14, 13, 18));
                let astral = astral_start(t);
                let core = core_zoom(rect, astral);
                if let Some(k) = self.fit {
                    self.zoom = (core * k).clamp(MIN_ZOOM, MAX_ZOOM);
                }
                // Pan and zoom (zooming lets go of the fitted view).
                if resp.dragged() {
                    let dlt = resp.drag_delta();
                    self.pan -= Vec2::new(dlt.x, dlt.y) / self.zoom;
                }
                // Two fingers: pinch to zoom round their middle.
                if let Some(mt) = ui.input(|i| i.multi_touch()).filter(|m| rect.contains(m.center_pos)) {
                    let before = self.to_tree(rect, mt.center_pos);
                    if (mt.zoom_delta - 1.0).abs() > 1e-3 {
                        self.fit = None;
                    }
                    self.zoom = (self.zoom * mt.zoom_delta).clamp(MIN_ZOOM, MAX_ZOOM);
                    self.pan += before - self.to_tree(rect, mt.center_pos);
                }
                if resp.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.fit = None;
                        let before = self.to_tree(rect, ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()));
                        self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(MIN_ZOOM, MAX_ZOOM);
                        let after = self.to_tree(rect, ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()));
                        self.pan += before - after;
                    }
                }
                let alloc = &inv.tree;
                let reached = |id: u32| id == start_id() || alloc.contains(&id);
                // (The search lights nodes up only where its field shows: with a mouse.) Its hits, by
                // node index, are kept while the search stays the same.
                let search = if mouse { self.search.to_lowercase() } else { String::new() };
                if self.lit.0 != search || (!search.is_empty() && self.lit.1.len() != t.nodes.len()) {
                    let matches = |n: &Node| {
                        lines::tree_node_name(t, n).to_lowercase().contains(&search)
                            || t.describe(n).iter().any(|l| l.to_lowercase().contains(&search))
                    };
                    let hits = if search.is_empty() { Vec::new() } else { t.nodes.iter().map(matches).collect() };
                    self.lit = (search, hits);
                }
                let lit = &self.lit.1;
                let hit = |i: usize| lit.get(i).copied().unwrap_or(false);
                // The level of detail, and the nodes that are the player's now (taken, takeable, a
                // search hit), which keep full strength.
                let zoom = self.zoom;
                let detail = detail(zoom, core);
                let marked: Vec<bool> =
                    t.nodes.iter().enumerate().map(|(i, n)| reached(n.id) || t.can_allocate(alloc, n.id) || hit(i)).collect();
                let view = rect.expand(30.0);
                // Hovered node and the path to it (touch: the node tapped, then the one shown).
                let hover_pos =
                    if self.touch { resp.clicked().then(|| resp.interact_pointer_pos()).flatten() } else { resp.hover_pos() };
                // The nearest node within its drawn size and a little more: zoomed out, a faded node
                // the D-pad skips is still named under the pointer.
                let tapped = hover_pos.and_then(|hp| {
                    let at = self.to_tree(rect, hp);
                    t.nodes
                        .iter()
                        .enumerate()
                        .map(|(i, n)| (i, n, (n.pos - at).length() * zoom))
                        .filter(|&(i, n, away)| away <= look(n, marked[i], zoom, detail).r + 6.0)
                        .min_by(|a, b| a.2.total_cmp(&b.2))
                        .map(|(_, n, _)| n)
                });
                let mut take_tap = false;
                if self.touch {
                    if resp.clicked() {
                        // A second tap on the node shown takes it; a tap on nothing hides it.
                        take_tap = tapped.is_some_and(|n| self.picked == Some(n.id));
                        self.picked = tapped.map(|n| n.id);
                    }
                    // A second tap on a mastery that is taken opens its options.
                    if take_tap
                        && self.picked.and_then(|id| t.node(id)).is_some_and(|n| reached(n.id) && n.kind == NodeKind::Mastery)
                    {
                        self.mastery = self.picked;
                    }
                }
                let hovered = if self.touch { self.picked.and_then(|id| t.node(id)) } else { tapped };
                let path: Vec<u32> = hovered.filter(|n| !reached(n.id)).and_then(|n| t.path_to(alloc, n.id)).unwrap_or_default();
                // How each node is drawn (the path and the hovered node keep full strength too).
                let looks: Vec<Look> = t
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let mark = marked[i] || path.contains(&n.id) || hovered.is_some_and(|h| h.id == n.id);
                        look(n, mark, zoom, detail)
                    })
                    .collect();
                // Links.
                for (i, n) in t.nodes.iter().enumerate() {
                    let a = self.to_screen(rect, n.pos);
                    if !view.contains(a)
                        && n.links.iter().all(|l| t.node(*l).is_none_or(|m| !view.contains(self.to_screen(rect, m.pos))))
                    {
                        continue;
                    }
                    for l in &n.links {
                        let Some(&j) = t.index.get(l) else { continue };
                        let m = &t.nodes[j];
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
                            // An ordinary link fades with its fainter end.
                            let s = looks[i].strength.min(looks[j].strength);
                            if s < 0.02 {
                                continue;
                            }
                            (1.2 * (0.6 + 0.4 * s), Color32::from_rgb(52, 48, 56).gamma_multiply(s))
                        };
                        p.line_segment([a, b], Stroke::new(w, c));
                    }
                }
                // Nodes (the selectable ones in view are where a controller's D-pad snaps: `padnav`).
                let mut snaps = Vec::new();
                for (i, n) in t.nodes.iter().enumerate() {
                    let c = self.to_screen(rect, n.pos);
                    if !view.contains(c) {
                        continue;
                    }
                    let lk = looks[i];
                    if rect.contains(c) && lk.selectable() {
                        snaps.push(c);
                    }
                    if lk.strength < 0.02 || lk.r < 0.3 {
                        continue;
                    }
                    let (r, s, o) = (lk.r, lk.strength, lk.outline);
                    let sector = t.sectors.get(n.sector as usize).map(|s| s.color).unwrap_or([0.7; 3]);
                    let taken = reached(n.id);
                    let can = !taken && t.can_allocate(alloc, n.id);
                    let fill = if taken {
                        col(sector, 1.25)
                    } else if can {
                        col(sector, 0.6)
                    } else {
                        // (Brighter as its outline fades, so a faded dot keeps its sector's colour.)
                        col(sector, 0.28 + 0.2 * (1.0 - o))
                    }
                    .gamma_multiply(s);
                    let ring = if taken {
                        Color32::from_rgb(250, 230, 160)
                    } else if can || path.contains(&n.id) {
                        Color32::from_rgb(150, 230, 150)
                    } else {
                        Color32::from_rgb(80, 76, 84)
                    }
                    .gamma_multiply(s);
                    match n.kind {
                        NodeKind::Skill => {
                            let pts = vec![
                                c + EVec2::new(0.0, -r),
                                c + EVec2::new(r, 0.0),
                                c + EVec2::new(0.0, r),
                                c + EVec2::new(-r, 0.0),
                            ];
                            p.add(Shape::convex_polygon(
                                pts,
                                fill,
                                Stroke::new(
                                    1.5 * o,
                                    if taken { ring } else { Color32::from_rgb(120, 200, 255).gamma_multiply(s) },
                                ),
                            ));
                        }
                        NodeKind::Mastery => {
                            let pts: Vec<Pos2> = (0..6)
                                .map(|i| {
                                    let a = i as f32 * std::f32::consts::TAU / 6.0 + 0.5;
                                    c + EVec2::new(a.cos(), a.sin()) * r
                                })
                                .collect();
                            p.add(Shape::convex_polygon(pts, fill, Stroke::new(1.5 * o, ring)));
                            if inv.masteries.contains_key(&n.id) {
                                p.circle_filled(c, r * 0.35, Color32::from_rgb(255, 230, 150));
                            }
                        }
                        NodeKind::Keystone => {
                            p.circle_filled(c, r, fill);
                            p.circle_stroke(c, r, Stroke::new(2.5 * o, ring));
                            p.circle_stroke(c, r * 0.7, Stroke::new(o, ring));
                        }
                        _ => {
                            p.circle_filled(c, r, fill);
                            let w = if n.kind == NodeKind::Notable { 2.0 } else { 1.0 };
                            p.circle_stroke(c, r, Stroke::new(w * o, ring));
                        }
                    }
                    if hit(i) {
                        p.circle_stroke(c, r + 4.0, Stroke::new(2.0, Color32::from_rgb(255, 240, 80)));
                    }
                    if self.zoom > 45.0 && matches!(n.kind, NodeKind::Notable | NodeKind::Keystone) {
                        p.text(
                            c + EVec2::new(0.0, r + 8.0),
                            Align2::CENTER_CENTER,
                            lines::tree_node_name(t, n),
                            FontId::proportional(13.0),
                            Color32::from_white_alpha(170),
                        );
                    }
                }
                // Sector names around the start. Zoomed out they move out over their sectors, to the
                // gap before the Astral rings, on a dark backing: the overview's regions.
                let fade = 1.0 - detail;
                for (i, s) in t.sectors.iter().enumerate() {
                    if self.zoom <= 14.0 && fade == 0.0 {
                        continue;
                    }
                    let a = s.angle.to_radians();
                    let out = 2.0 + (astral - 3.0) * fade;
                    let at = self.to_screen(rect, Vec2::new(a.sin(), -a.cos()) * out);
                    let color = col(s.color, 1.0);
                    let font = FontId::proportional(if self.touch { 13.0 } else { 16.0 });
                    let galley = p.layout_no_wrap(lines::sector_name(t, i), font, color);
                    let label = Align2::CENTER_CENTER.anchor_size(at, galley.size());
                    if fade > 0.0 {
                        p.rect_filled(label.expand2(EVec2::new(5.0, 1.0)), 4.0, Color32::from_black_alpha((170.0 * fade) as u8));
                    }
                    p.galley(label.min, galley, color);
                }
                // Touch: the node shown, in a panel at the bottom, with what can be done.
                if let (true, Some(n)) = (self.touch, hovered) {
                    let taken = reached(n.id);
                    let c = self.to_screen(rect, n.pos);
                    let r = t.index.get(&n.id).map_or(2.0, |i| looks[*i].r);
                    p.circle_stroke(c, r + 5.0, Stroke::new(2.5, Color32::from_rgb(255, 230, 150)));
                    let panel = Rect::from_min_max(
                        Pos2::new(rect.left() + 8.0, rect.bottom() - (rect.height() * 0.42).min(190.0)),
                        Pos2::new(rect.left() + (rect.width() - 16.0).min(420.0), rect.bottom() - 8.0),
                    );
                    egui::Area::new(egui::Id::new("tree_pick")).fixed_pos(panel.min).order(egui::Order::Foreground).show(
                        ctx,
                        |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_width(panel.width() - 16.0);
                                egui::ScrollArea::vertical()
                                    .max_height(panel.height() - 60.0)
                                    .show(ui, |ui| node_info(ui, t, n, inv, taken, &path, false));
                                ui.horizontal_wrapped(|ui| {
                                    if !taken && t.can_allocate(alloc, n.id) && (ui.button(t!("tree.take")).clicked() || take_tap)
                                    {
                                        out.push(GameCmd::Allocate(n.id));
                                    }
                                    if !taken && path.len() > 1 && ui.button(t!("tree.take_path", count = path.len())).clicked() {
                                        for id in path.iter().take(inv.points as usize) {
                                            out.push(GameCmd::Allocate(*id));
                                        }
                                    }
                                    if taken && n.kind == NodeKind::Mastery && ui.button(t!("tree.choose")).clicked() {
                                        self.mastery = Some(n.id);
                                    }
                                    if taken
                                        && n.kind != NodeKind::Start
                                        && ui.button(t!("tree.refund", count = inv.refund_cost)).clicked()
                                    {
                                        out.push(GameCmd::Refund(n.id));
                                    }
                                    if ui.button(t!("ui.close")).clicked() {
                                        self.picked = None;
                                    }
                                });
                            });
                        },
                    );
                }
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(crate::padnav::SNAP_POINTS), snaps));
                // Hover: what it does; clicks. On a controller the node's card is docked at the
                // tree's right edge, big enough to read from the sofa; a mouse gets it at the
                // pointer.
                if let Some(n) = hovered.filter(|_| !self.touch) {
                    let taken = reached(n.id);
                    let mut r2 = resp.clone();
                    if self.pad {
                        // Shown, not touched: an interactable card would take the hover and the
                        // press from the nodes under it.
                        egui::Area::new(egui::Id::new("tree_card"))
                            .order(egui::Order::Tooltip)
                            .interactable(false)
                            .anchor(
                                Align2::RIGHT_TOP,
                                EVec2::new(-(ui.ctx().content_rect().right() - rect.right()) - 12.0, rect.top() + 12.0),
                            )
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                                    ui.set_max_width(380.0);
                                    ui.style_mut().override_text_style = Some(egui::TextStyle::Body);
                                    node_info(ui, t, n, inv, taken, &path, false);
                                });
                            });
                    } else {
                        r2 = r2.on_hover_ui_at_pointer(|ui| {
                            ui.set_max_width(320.0);
                            node_info(ui, t, n, inv, taken, &path, true);
                        });
                    }
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
                            let title = lines::mastery_name(&n.mastery, &m.name);
                            egui::Window::new(title)
                                .id(egui::Id::new("mastery"))
                                .collapsible(false)
                                .resizable(false)
                                .anchor(Align2::CENTER_CENTER, EVec2::ZERO)
                                .show(ctx, |ui| {
                                    for (i, (name, stats, power)) in m.options.iter().enumerate() {
                                        // The option's name, then what it does, a line each.
                                        let mut text = lines::mastery_option(&n.mastery, i, name);
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
                                    if ui.button(t!("ui.close")).clicked() {
                                        keep = false;
                                    }
                                });
                        }
                    }
                    if !keep {
                        self.mastery = None;
                    }
                }
                // The shapes' legend, for a mouse (a pad's docked card and a finger's panel name the
                // node's kind), on a dark strip that keeps it off the nodes under it.
                if mouse {
                    let galley = p.layout(
                        t!("tree.legend"),
                        FontId::proportional(13.0),
                        Color32::from_white_alpha(170),
                        (rect.width() - 40.0).max(80.0),
                    );
                    let at = rect.left_bottom() + EVec2::new(10.0, -10.0);
                    let strip = Align2::LEFT_BOTTOM.anchor_size(at, galley.size() + EVec2::new(16.0, 8.0));
                    p.rect_filled(strip, 4.0, Color32::from_black_alpha(190));
                    p.galley(strip.min + EVec2::new(8.0, 4.0), galley, Color32::WHITE);
                }
            });
        if !open {
            self.open = false;
            self.mastery = None;
        }
    }

    /// Zooms to `scale` times the opening view (1 is the core, 0 the farthest out), kept as the
    /// canvas changes until the player zooms: for the tools and tests.
    pub fn set_zoom(&mut self, scale: f32) {
        self.fit = Some(scale.max(0.0));
    }

    /// Circle or Esc: a mastery's options close first. False when none were open.
    pub fn back(&mut self) -> bool {
        self.mastery.take().is_some()
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
        NodeKind::Start => "tree.kind.start",
        NodeKind::Minor => "tree.kind.minor",
        NodeKind::Notable => "tree.kind.notable",
        NodeKind::Keystone => "tree.kind.keystone",
        NodeKind::Mastery => "tree.kind.mastery",
        NodeKind::Skill => "tree.kind.skill",
        NodeKind::Astral => "tree.kind.astral",
    };
    ui.label(RichText::new(lines::tree_node_name(t, n)).strong().size(16.0));
    let kind = if n.ring > 0 { t!("tree.kind.ring", kind = lines::piece(kind, &[]), ring = n.ring) } else { t!(kind) };
    ui.label(RichText::new(kind).small().color(Color32::from_white_alpha(140)));
    for l in t.describe(n) {
        ui.label(RichText::new(l).color(Color32::from_rgb(150, 180, 255)));
    }
    if n.kind == NodeKind::Mastery {
        if let Some(m) = t.masteries.get(&n.mastery) {
            for (i, (name, stats, power)) in m.options.iter().enumerate() {
                let chosen = inv.masteries.get(&n.id) == Some(&(i as u8));
                // The option's name, then what it does, dash-separated.
                let mut line = lines::mastery_option(&n.mastery, i, name);
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
    let lore = lines::tree_node_lore(n);
    if !lore.is_empty() {
        ui.label(RichText::new(lore).italics().color(Color32::from_rgb(170, 130, 90)));
    }
    if hints {
        ui.separator();
        if taken && n.kind != NodeKind::Start {
            ui.label(RichText::new(t!("tree.refund_hint", count = inv.refund_cost)).small());
            if n.kind == NodeKind::Mastery {
                ui.label(RichText::new(t!("tree.choose_hint")).small());
            }
        }
    }
    if !taken && !path.is_empty() {
        ui.label(RichText::new(t!("tree.points_away", count = path.len())).small());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    /// Every text `shape` draws, and the tree's canvas (its dark backdrop).
    fn collect(shape: &Shape, out: &mut Vec<String>, canvas: &mut Rect) {
        match shape {
            Shape::Vec(list) => list.iter().for_each(|s| collect(s, out, canvas)),
            Shape::Text(t) => out.push(t.galley.text().to_string()),
            Shape::Rect(r) if r.fill == Color32::from_rgb(14, 13, 18) => *canvas = r.rect,
            _ => {}
        }
    }

    /// What the open tree drew on a 720p screen.
    struct Frame {
        texts: Vec<String>,
        zoom: f32,
        canvas: Rect,
        /// Where the D-pad snaps.
        snaps: Vec<Pos2>,
    }

    /// Draws the open tree for `inv`, with the pointer at `pointer` (points).
    fn frame(tree: &mut TreeUi, inv: &InvView, pointer: Option<Pos2>) -> Frame {
        let ctx = egui::Context::default();
        // (No fading in: the window is drawn in its own colours from the first pass.)
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let mut out = Vec::new();
        let mut texts = Vec::new();
        let mut canvas = Rect::NOTHING;
        // (The pointer hovers the canvas once it is laid out, and the node's card shows then.)
        for _ in 0..4 {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, EVec2::new(1280.0, 720.0))),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let mut full = ctx.run_ui(raw, |ui| tree.ui(ui.ctx(), inv, &mut out));
            full.textures_delta.clear();
            texts.clear();
            full.shapes.iter().for_each(|s| collect(&s.shape, &mut texts, &mut canvas));
        }
        let snaps = ctx.data(|d| d.get_temp(egui::Id::new(crate::padnav::SNAP_POINTS))).unwrap_or_default();
        Frame { texts, zoom: tree.zoom, canvas, snaps }
    }

    /// The texts the open tree draws on a 720p screen, and its zoom then.
    fn draw(tree: &mut TreeUi) -> (Vec<String>, f32) {
        let f = frame(tree, &InvView { points: 3, ..Default::default() }, None);
        (f.texts, f.zoom)
    }

    /// A hero's tree: the path to the first keystone taken.
    fn taken(t: &Tree) -> BTreeSet<u32> {
        let ks = t.nodes.iter().find(|n| n.kind == NodeKind::Keystone).expect("a keystone");
        t.path_to(&BTreeSet::new(), ks.id).expect("a path to it").into_iter().collect()
    }

    /// The nodes that are the player's with `alloc` taken: taken or takeable now.
    fn marked(t: &Tree, alloc: &BTreeSet<u32>, n: &Node) -> bool {
        n.id == start_id() || alloc.contains(&n.id) || t.can_allocate(alloc, n.id)
    }

    #[test]
    fn the_detail_is_whole_from_the_opening_zoom_in_and_eases_out_without_a_step() {
        let t = &data().tree;
        // One node of each look.
        let mut sample: Vec<&Node> = Vec::new();
        for n in &t.nodes {
            if !sample.iter().any(|s| s.kind == n.kind && s.lore == n.lore && (s.ring > 0) == (n.ring > 0)) {
                sample.push(n);
            }
        }
        for core in [20.0, 30.0, 41.4, 90.0] {
            assert_eq!(detail(core, core), 1.0);
            assert_eq!(detail(core * 2.0, core), 1.0);
            assert_eq!(detail(core * FADED, core), 0.0);
            // Zooming out a step at a time: the detail only falls, and every node's size and
            // strength change a little each step (nothing pops at a threshold).
            let mut z = core * 1.5;
            let mut last = (1.0, sample.iter().map(|n| look(n, false, z, 1.0)).collect::<Vec<_>>());
            while z > MIN_ZOOM {
                z -= 0.02;
                let d = detail(z, core);
                assert!(d <= last.0 && last.0 - d < 0.01, "a step at zoom {z} of {core}: {} -> {d}", last.0);
                let looks: Vec<Look> = sample.iter().map(|n| look(n, false, z, d)).collect();
                for ((n, a), b) in sample.iter().zip(&last.1).zip(&looks) {
                    let step = (a.r - b.r).abs().max((a.strength - b.strength).abs()).max((a.outline - b.outline).abs());
                    assert!(step < 0.05, "{:?} pops at zoom {z} of {core}: {a:?} -> {b:?}", n.kind);
                }
                last = (d, looks);
            }
        }
    }

    #[test]
    fn at_the_opening_zoom_and_closer_every_node_is_drawn_as_before() {
        let t = &data().tree;
        for zoom in [20.0, 41.4, 90.0] {
            let d = detail(zoom, 20.0);
            for n in &t.nodes {
                for mark in [false, true] {
                    let lk = look(n, mark, zoom, d);
                    assert_eq!(lk, Look { r: (radius(n) * zoom).max(2.0), strength: 1.0, outline: 1.0 }, "{}", n.key);
                    assert!(lk.selectable());
                }
            }
        }
    }

    #[test]
    fn zoomed_out_the_shape_and_the_choices_keep_full_strength_and_the_rest_fades() {
        let t = &data().tree;
        let alloc = taken(t);
        let zoom = MIN_ZOOM;
        let d = detail(zoom, core_zoom(Rect::from_min_size(Pos2::ZERO, EVec2::new(1110.0, 518.0)), astral_start(t)));
        assert_eq!(d, 0.0, "all the way out the ordinary detail is gone");
        let (mut choices, mut dots, mut gone) = (0, 0, 0);
        for n in &t.nodes {
            let mark = marked(t, &alloc, n);
            let lk = look(n, mark, zoom, d);
            let shape = !matches!(n.kind, NodeKind::Minor | NodeKind::Astral) || n.lore == "star";
            if shape || mark {
                // The start, notables, keystones, masteries, skill upgrades, the Astral stars,
                // the path taken and what can be taken now: as strong, no smaller, selectable.
                assert_eq!(lk.strength, 1.0, "{}", n.key);
                assert!(lk.r >= (radius(n) * zoom).max(2.0) && lk.selectable(), "{}: {lk:?}", n.key);
                choices += mark as usize;
            } else if n.ring > 0 {
                // An Astral ring's ordinary node: gone (its stars outline the ring).
                assert!(lk.strength == 0.0 && !lk.selectable(), "{}: {lk:?}", n.key);
                gone += 1;
            } else {
                // A main-tree minor node: a faint dot the D-pad skips.
                assert!(lk.strength > 0.0 && lk.strength < 1.0 && lk.r < SNAP_MIN && !lk.selectable(), "{}: {lk:?}", n.key);
                dots += 1;
            }
        }
        assert!(choices > alloc.len() && dots > 100 && gone > 1000, "{choices} {dots} {gone}");
        // A search hit, the path shown and the node under the pointer are marked: full strength.
        let minor = t.nodes.iter().find(|n| n.kind == NodeKind::Minor && !marked(t, &alloc, n)).unwrap();
        assert_eq!(look(minor, true, zoom, d).strength, 1.0);
        // Where the detail has just gone, the kept nodes stand well clear of the dots.
        let zoom = 41.4 * FADED;
        let dot = look(minor, false, zoom, 0.0).r;
        for kind in [NodeKind::Notable, NodeKind::Keystone, NodeKind::Mastery, NodeKind::Skill] {
            let n = t.nodes.iter().find(|n| n.kind == kind).unwrap();
            assert!(look(n, false, zoom, 0.0).r >= 3.5 * dot, "{kind:?}");
        }
    }

    #[test]
    fn zoomed_out_the_d_pad_snaps_to_what_is_drawn_selectable_and_the_card_names_any_node() {
        let t = &data().tree;
        let alloc = taken(t);
        let inv = InvView { points: 3, tree: alloc.clone(), ..Default::default() };
        let mut tree = TreeUi { open: true, pad: true, ..Default::default() };
        let on_canvas = |f: &Frame, tree: &TreeUi| -> Vec<(&Node, Pos2)> {
            t.nodes.iter().map(|n| (n, tree.to_screen(f.canvas, n.pos))).filter(|(_, p)| f.canvas.contains(*p)).collect()
        };
        // As it opens, the D-pad snaps to every node on the canvas, as it always did.
        let f = frame(&mut tree, &inv, None);
        assert_eq!(f.snaps.len(), on_canvas(&f, &tree).len());
        // All the way out: to every node of the tree's shape and the player's choices on it, and
        // to no faded one.
        tree.set_zoom(0.0);
        let f = frame(&mut tree, &inv, None);
        assert_eq!(f.zoom, MIN_ZOOM);
        let nodes = on_canvas(&f, &tree);
        assert_eq!(nodes.len(), t.nodes.len(), "the whole tree is on the canvas");
        let snapped = |p: Pos2| f.snaps.iter().any(|q| q.distance(p) < 0.01);
        for (n, p) in &nodes {
            assert_eq!(snapped(*p), kept(n, marked(t, &alloc, n)), "{}", n.key);
        }
        assert!(f.snaps.len() * 4 < nodes.len(), "{} of {} nodes", f.snaps.len(), nodes.len());
        // The pointer on a faded minor node (the stick moves it freely): the card names it.
        let card = t!("tree.kind.minor");
        assert!(!f.texts.contains(&card));
        let (minor, at) = nodes.iter().find(|(n, _)| n.kind == NodeKind::Minor && !kept(n, marked(t, &alloc, n))).unwrap();
        let f = frame(&mut tree, &inv, Some(*at));
        assert!(f.texts.contains(&card) && f.texts.contains(&lines::tree_node_name(t, minor)), "{:?}", f.texts);
    }

    #[test]
    fn the_tree_opens_on_its_core_with_the_astral_rings_past_the_edges() {
        let t = &data().tree;
        let astral = astral_start(t);
        assert!(astral > CORE + 4.0, "the Astral rings start at {astral}");
        // A TV's canvas, a phone held upright and one held sideways.
        for size in [EVec2::new(1110.0, 518.0), EVec2::new(360.0, 560.0), EVec2::new(820.0, 200.0)] {
            let rect = Rect::from_center_size(Pos2::ZERO, size);
            let zoom = core_zoom(rect, astral);
            assert!(zoom * CORE >= size.min_elem() * 0.5 - 0.5, "the core fills the shorter side: {size:?} {zoom}");
            let shown = t.nodes.iter().filter(|n| n.ring > 0).filter(|n| rect.contains(Pos2::new(n.pos.x, n.pos.y) * zoom));
            assert_eq!(shown.count(), 0, "no Astral node on a {size:?} canvas at {zoom}");
        }
        // Opened, the tree takes that zoom; once the player has zoomed, it keeps theirs.
        let mut tree = TreeUi { open: true, ..Default::default() };
        let (_, zoom) = draw(&mut tree);
        assert!(zoom > 25.0, "opens zoomed in on the core: {zoom}");
        tree.fit = None;
        tree.zoom = 10.0;
        assert_eq!(draw(&mut tree).1, 10.0);
    }

    #[test]
    fn the_search_field_and_the_legend_are_the_mouses() {
        let legend = t!("tree.legend");
        let mut tree = TreeUi { open: true, ..Default::default() };
        let (mouse, _) = draw(&mut tree);
        assert!(mouse.contains(&t!("tree.search")) && mouse.contains(&legend), "{mouse:?}");
        assert!(mouse.contains(&t!("tree.points", count = 3)), "{mouse:?}");
        for (pad, touch) in [(true, false), (false, true)] {
            let mut tree = TreeUi { open: true, pad, touch, ..Default::default() };
            let (texts, _) = draw(&mut tree);
            assert!(!texts.contains(&t!("tree.search")) && !texts.contains(&legend), "pad {pad}: {texts:?}");
            assert!(texts.contains(&t!("tree.points", count = 3)), "{texts:?}");
        }
    }
}
