//! Shardfall menus: the inventory with a paper doll, item tooltips that compare with what is
//! worn, the smith's shop, the stash, the portal's destinations, the character sheet and the
//! skill bar picker, plus clickable loot labels on the ground. Everything the player does here
//! becomes a `GameCmd` sent to the simulation (so it records and replays like any input).

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2 as EVec2};
use glam::Vec3;
use pav_core::arpg::combat::Rarity;
use pav_core::arpg::data::{Data, data};
use pav_core::arpg::items::{EquipSlot, Item, Slot};
use pav_core::arpg::lines;
use pav_core::arpg::stats::{Mods, Stat, describe};
use pav_core::arpg::{GameCmd, GameFrame, InvView, Place, SpotKind};
use pav_core::t;

use crate::arpg_ui::Projector;

/// Which windows are open.
#[derive(Default)]
pub struct GameUi {
    pub inventory: bool,
    pub character: bool,
    pub skills: bool,
    /// The vendor, stash or portal window (opened by interacting).
    pub panel: Option<SpotKind>,
    /// Which spot the panel belongs to (exhibits).
    pub panel_spot: Option<usize>,
    pub tree: crate::arpg_tree::TreeUi,
    /// The big level map (M).
    pub map: bool,
    /// The player is on a gamepad: hints name its buttons.
    pub pad: bool,
    /// The player is on a touch screen: a tap picks an item and shows what to do with it.
    pub touch: bool,
    /// Touch: the item picked, and where it is.
    picked: Option<(u32, Picked)>,
    /// A tool for the app to open: the Pavilion Gate leads to the pavilion (`Tool::Tour`).
    pub tool: Option<crate::tools::Tool>,
}

/// Where a picked item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picked {
    Bag,
    Worn(usize),
    Vendor,
    Buyback,
    Stash,
}

/// Touch screens: windows fit the screen and scroll (a finger drags the content).
fn fit<'a>(w: egui::Window<'a>, ctx: &egui::Context, touch: bool) -> egui::Window<'a> {
    if !touch {
        return w;
    }
    let s = ctx.content_rect();
    w.vscroll(true).max_height((s.height() - 66.0).max(120.0)).max_width(s.width() - 16.0)
}

/// Touch screens: windows sit below the buttons in the top right (`TOUCH_DROP` down from where
/// they would be centred, or this far from the top).
const TOUCH_DROP: f32 = 25.0;
const TOUCH_TOP: f32 = 58.0;

pub fn rarity_color(r: Rarity) -> Color32 {
    match r {
        Rarity::Normal => Color32::from_rgb(220, 220, 220),
        Rarity::Magic => Color32::from_rgb(120, 150, 255),
        Rarity::Rare => Color32::from_rgb(255, 220, 90),
        Rarity::Unique => Color32::from_rgb(255, 140, 50),
    }
}

fn hex(c: &str) -> Color32 {
    pav_core::Color::try_hex(c)
        .map(|c| {
            let to = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            Color32::from_rgb(to(c.0[0]), to(c.0[1]), to(c.0[2]))
        })
        .unwrap_or(Color32::GRAY)
}

/// A small vector picture of an item kind.
fn item_icon(p: &egui::Painter, rect: Rect, slot: Slot, kind: &str, color: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.32;
    let st = Stroke::new(2.5, color);
    let v = |x: f32, y: f32| c + EVec2::new(x * r, y * r);
    match slot {
        Slot::Weapon => match kind {
            "staff" | "spear" => {
                p.line_segment([v(-0.9, 0.9), v(0.9, -0.9)], st);
                if kind == "staff" {
                    p.circle_filled(v(0.9, -0.9), r * 0.22, color);
                } else {
                    p.add(Shape::convex_polygon(vec![v(0.95, -0.95), v(0.55, -0.75), v(0.75, -0.55)], color, Stroke::NONE));
                }
            }
            "axe" => {
                p.line_segment([v(-0.7, 0.9), v(0.4, -0.6)], st);
                p.add(Shape::convex_polygon(
                    vec![v(0.15, -0.9), v(0.85, -0.45), v(0.55, -0.05), v(0.2, -0.4)],
                    color,
                    Stroke::NONE,
                ));
            }
            "mace" | "maul" => {
                p.line_segment([v(-0.8, 0.8), v(0.35, -0.35)], st);
                p.circle_filled(v(0.5, -0.5), r * if kind == "maul" { 0.45 } else { 0.32 }, color);
            }
            "wand" => {
                p.line_segment([v(-0.6, 0.6), v(0.5, -0.5)], st);
                p.circle_filled(v(0.6, -0.6), r * 0.15, color);
            }
            "claw" => {
                for k in 0..3 {
                    let x = -0.5 + k as f32 * 0.5;
                    p.line_segment([v(x, 0.6), v(x + 0.25, -0.7)], st);
                }
            }
            _ => {
                // Swords and daggers.
                let len = if kind == "dagger" {
                    0.6
                } else if kind == "greatsword" {
                    1.05
                } else {
                    0.9
                };
                p.line_segment([v(-0.7, 0.7), v(-0.7 + 1.6 * len, 0.7 - 1.6 * len)], Stroke::new(3.0, color));
                p.line_segment([v(-0.75, 0.25), v(-0.25, 0.75)], st);
            }
        },
        Slot::Offhand if kind == "focus" => {
            p.circle_filled(c, r * 0.6, color);
            p.circle_stroke(c, r * 0.85, Stroke::new(1.5, color));
        }
        Slot::Offhand => {
            p.add(Shape::convex_polygon(
                vec![v(-0.75, -0.8), v(0.75, -0.8), v(0.7, 0.2), v(0.0, 0.95), v(-0.7, 0.2)],
                color,
                Stroke::NONE,
            ));
        }
        Slot::Helmet => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    v(a.cos() * 0.8, 0.3 + a.sin() * 0.9)
                })
                .collect();
            p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            p.rect_filled(Rect::from_min_max(v(-0.9, 0.25), v(0.9, 0.5)), 1.0, color);
        }
        Slot::Body => {
            p.add(Shape::convex_polygon(
                vec![v(-0.9, -0.7), v(-0.35, -0.85), v(0.35, -0.85), v(0.9, -0.7), v(0.6, 0.9), v(-0.6, 0.9)],
                color,
                Stroke::NONE,
            ));
        }
        Slot::Gloves => {
            p.rect_filled(Rect::from_min_max(v(-0.55, -0.3), v(0.45, 0.85)), 3.0, color);
            p.rect_filled(Rect::from_min_max(v(-0.55, -0.85), v(-0.25, -0.2)), 2.0, color);
            p.rect_filled(Rect::from_min_max(v(0.45, -0.1), v(0.85, 0.3)), 2.0, color);
        }
        Slot::Boots => {
            p.rect_filled(Rect::from_min_max(v(-0.5, -0.85), v(0.1, 0.6)), 2.0, color);
            p.rect_filled(Rect::from_min_max(v(-0.5, 0.3), v(0.85, 0.85)), 3.0, color);
        }
        Slot::Belt => {
            p.rect_filled(Rect::from_min_max(v(-0.95, -0.25), v(0.95, 0.25)), 2.0, color);
            p.rect_stroke(
                Rect::from_min_max(v(-0.25, -0.4), v(0.25, 0.4)),
                2.0,
                Stroke::new(2.0, Color32::from_rgb(230, 200, 110)),
                StrokeKind::Middle,
            );
        }
        Slot::Amulet => {
            p.circle_stroke(v(0.0, -0.3), r * 0.7, Stroke::new(1.5, color));
            p.circle_filled(v(0.0, 0.55), r * 0.35, color);
        }
        Slot::Ring => {
            p.circle_stroke(c, r * 0.6, Stroke::new(3.5, color));
            p.circle_filled(v(0.0, -0.6), r * 0.2, color);
        }
    }
}

/// One item cell: frame tinted by rarity, the item's picture.
fn cell(ui: &mut egui::Ui, d: &Data, item: Option<&Item>, size: f32, empty: &str, highlight: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(EVec2::splat(size), Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    match item {
        Some(it) => {
            let rc = rarity_color(it.rarity);
            let mix = |a: u8, b: u8| (a as f32 * 0.2 + b as f32 * 0.8) as u8;
            p.rect_filled(rect, 4.0, Color32::from_rgb(mix(rc.r(), 26), mix(rc.g(), 24), mix(rc.b(), 28)));
            let b = it.base_def(d);
            let col = b.map(|b| hex(&b.color)).unwrap_or(Color32::GRAY);
            item_icon(p, rect, b.map(|b| b.slot).unwrap_or_default(), b.map(|b| b.kind.as_str()).unwrap_or(""), col);
            p.rect_stroke(rect, 4.0, Stroke::new(if hovered { 2.0 } else { 1.2 }, rc), StrokeKind::Inside);
        }
        None => {
            p.rect_filled(rect, 4.0, Color32::from_rgb(22, 20, 24));
            p.rect_stroke(rect, 4.0, Stroke::new(1.0, Color32::from_rgb(70, 60, 50)), StrokeKind::Inside);
            if !empty.is_empty() {
                p.text(rect.center(), Align2::CENTER_CENTER, empty, FontId::proportional(10.0), Color32::from_white_alpha(70));
            }
        }
    }
    if highlight {
        p.rect_stroke(rect.expand(1.0), 4.0, Stroke::new(2.0, Color32::from_rgb(120, 230, 120)), StrokeKind::Outside);
    }
    resp
}

/// Tooltip body for an item.
pub fn item_tooltip(ui: &mut egui::Ui, d: &Data, it: &Item, title: Option<&str>) {
    ui.set_max_width(330.0);
    let t = it.describe(d);
    if let Some(s) = title {
        ui.label(RichText::new(s).small().color(Color32::from_white_alpha(140)));
    }
    ui.label(RichText::new(&t.name).size(17.0).strong().color(rarity_color(it.rarity)));
    if !t.base.is_empty() {
        ui.label(RichText::new(&t.base).color(rarity_color(it.rarity).gamma_multiply(0.8)));
    }
    ui.label(RichText::new(t!("item.kind_level", kind = &t.kind, level = t.level)).small().color(Color32::from_white_alpha(150)));
    ui.separator();
    for h in &t.header {
        ui.label(RichText::new(h).color(Color32::from_rgb(235, 228, 210)));
    }
    if !t.implicit.is_empty() {
        if !t.header.is_empty() {
            ui.separator();
        }
        for l in &t.implicit {
            ui.label(RichText::new(l).color(Color32::from_rgb(200, 200, 215)));
        }
    }
    if !t.mods.is_empty() {
        ui.separator();
        for (l, tier, local) in &t.mods {
            let mut txt = RichText::new(l).color(Color32::from_rgb(140, 170, 255));
            if it.rarity == Rarity::Unique {
                txt = RichText::new(l).color(Color32::from_rgb(255, 190, 120));
            }
            ui.horizontal(|ui| {
                ui.label(txt);
                if it.rarity != Rarity::Unique {
                    let tier = if *local { t!("item.tier.local", tier = tier + 1) } else { t!("item.tier", tier = tier + 1) };
                    ui.label(RichText::new(tier).small().color(Color32::from_white_alpha(90)));
                }
            });
        }
    }
    if !t.power.is_empty() {
        ui.separator();
        ui.label(RichText::new(&t.power).color(Color32::from_rgb(255, 150, 60)).strong());
    }
    if !t.lore.is_empty() {
        ui.label(RichText::new(&t.lore).italics().color(Color32::from_rgb(170, 130, 90)));
    }
    ui.label(RichText::new(t!("item.sells_for", count = it.value())).small().color(Color32::from_rgb(220, 180, 80)));
}

/// What wearing `new` instead of `old` changes (stat by stat, plus weapon damage and armour).
fn compare(ui: &mut egui::Ui, d: &Data, new: &Item, old: Option<&Item>) {
    let ns = new.stats(d);
    let os = old.map(|o| o.stats(d)).unwrap_or_default();
    let mut diff = Mods::default();
    diff.merge(&ns.mods);
    for (s, v) in &os.mods.0 {
        diff.add(*s, -v);
    }
    ui.separator();
    ui.label(
        RichText::new(if old.is_some() { t!("compare.instead") } else { t!("compare.wear") })
            .small()
            .color(Color32::from_white_alpha(150)),
    );
    if new.slot(d) == Slot::Weapon {
        let nd = (ns.phys[0] + ns.phys[1]) * 0.5 * ns.aps;
        let od = (os.phys[0] + os.phys[1]) * 0.5 * os.aps;
        let value = ((nd - od).abs() * 10.0).round() / 10.0;
        line(ui, nd - od, &if nd >= od { t!("compare.dps_more", value = value) } else { t!("compare.dps_less", value = value) });
    }
    let mut any = false;
    for (s, v) in &diff.0 {
        if v.abs() < 0.05 {
            continue;
        }
        any = true;
        let text = if *v < 0.0 {
            t!("compare.lose", what = pav_core::arpg::stats::describe_piece(*s, v.abs()))
        } else {
            describe(*s, v.abs())
        };
        line(ui, *v, &text);
    }
    if !any && new.slot(d) != Slot::Weapon {
        ui.label(RichText::new(t!("compare.none")).small());
    }
    if ns.power != os.power {
        if let Some(p) = os.power {
            line(ui, -1.0, &t!("compare.lose", what = p.describe_piece()));
        }
        if let Some(p) = ns.power {
            line(ui, 1.0, &p.describe());
        }
    }
}

fn line(ui: &mut egui::Ui, v: f32, text: &str) {
    let c = if v >= 0.0 { Color32::from_rgb(110, 220, 110) } else { Color32::from_rgb(230, 90, 80) };
    ui.label(RichText::new(text).color(c));
}

/// The worn item an item would replace.
fn worn_for<'a>(inv: &'a InvView, d: &Data, it: &Item) -> Option<&'a Item> {
    let slot = it.slot(d);
    let candidates: Vec<&Item> = inv
        .equipment
        .iter()
        .enumerate()
        .filter(|(i, _)| EquipSlot::from_index(*i).is_some_and(|e| e.slot() == slot))
        .filter_map(|(_, e)| e.as_ref())
        .collect();
    // Rings: compare with the weaker (lower value) one.
    candidates.into_iter().min_by_key(|i| i.value())
}

/// Paper doll positions (column, row) of the worn slots.
const DOLL: [(EquipSlot, f32, f32); 10] = [
    (EquipSlot::Helmet, 1.5, 0.0),
    (EquipSlot::Amulet, 2.7, 0.0),
    (EquipSlot::Weapon, 0.3, 1.0),
    (EquipSlot::Body, 1.5, 1.0),
    (EquipSlot::Offhand, 2.7, 1.0),
    (EquipSlot::Gloves, 0.3, 2.0),
    (EquipSlot::Belt, 1.5, 2.0),
    (EquipSlot::Ring1, 2.7, 2.0),
    (EquipSlot::Ring2, 3.7, 2.0),
    (EquipSlot::Boots, 1.5, 3.0),
];

impl GameUi {
    pub fn any_open(&self) -> bool {
        self.inventory || self.character || self.skills || self.panel.is_some() || self.tree.open
    }

    pub fn close_all(&mut self) {
        self.inventory = false;
        self.character = false;
        self.skills = false;
        self.panel = None;
        self.tree.open = false;
    }

    /// Interact pressed: open whatever the hero stands at, or use it (the way down, a chest).
    pub fn interact(&mut self, g: &GameFrame) -> Option<GameCmd> {
        if let Some(s) = g.near.and_then(|i| g.spots.get(i)) {
            if matches!(s.kind, SpotKind::Exit | SpotKind::Chest) {
                return g.near.map(|i| GameCmd::Use(i as u32));
            }
            if s.kind == SpotKind::Gate {
                // Back through the gate to the pavilion's plaza, where the tools are.
                self.tool = Some(crate::tools::Tool::Tour);
                return None;
            }
            let same = self.panel == Some(s.kind) && self.panel_spot == g.near;
            self.panel = if same { None } else { Some(s.kind) };
            self.panel_spot = g.near;
            if matches!(s.kind, SpotKind::Vendor | SpotKind::Stash | SpotKind::Gamble) && self.panel.is_some() {
                self.inventory = true;
            }
        }
        None
    }

    /// Draws the windows and labels; returns the commands the player gave.
    pub fn ui(&mut self, ctx: &egui::Context, g: &GameFrame, proj: &Projector) -> Vec<GameCmd> {
        let mut out = Vec::new();
        let d = data();
        // Walking away closes the vendor, the stash and exhibit cards.
        if let Some(k) = self.panel {
            if g.near.and_then(|i| g.spots.get(i)).map(|s| s.kind) != Some(k)
                || (matches!(k, SpotKind::Exhibit | SpotKind::Hero) && g.near != self.panel_spot)
            {
                self.panel = None;
            }
        }
        labels(ctx, g, proj, &mut out, self.touch);
        let Some(inv) = g.inv.clone() else { return out };
        if self.inventory {
            self.inventory_window(ctx, &d, &inv, &mut out);
        }
        if self.character {
            character_window(ctx, &mut self.character, &inv, g, self.touch);
        }
        if self.skills {
            skills_window(ctx, &mut self.skills, &d, &inv, &mut out, self.touch);
        }
        self.tree.ui(ctx, &inv, &mut out);
        if inv.points > 0 && !self.tree.open && !self.touch {
            // A nudge above the experience bar.
            let screen = ctx.content_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("points_hint")));
            let at = Pos2::new(screen.center().x, screen.bottom() - 28.0);
            p.text(
                at,
                Align2::CENTER_BOTTOM,
                t!("hud.points", count = inv.points),
                FontId::proportional(14.0),
                Color32::from_rgb(255, 215, 120),
            );
        }
        match self.panel {
            Some(SpotKind::Vendor) => self.vendor_window(ctx, &d, &inv, &mut out),
            Some(SpotKind::Stash) => self.stash_window(ctx, &d, &inv, &mut out),
            Some(SpotKind::Portal) => self.portal_window(ctx, g, &mut out),
            Some(SpotKind::Exhibit) => self.exhibit_window(ctx, g, &mut out),
            Some(SpotKind::Gamble) => self.gamble_window(ctx, &inv, &mut out),
            Some(SpotKind::Alchemist) => self.alchemist_window(ctx, &inv, &mut out),
            Some(SpotKind::Hero) => self.hero_window(ctx, &d, &inv, g, &mut out),
            Some(SpotKind::Exit | SpotKind::Chest | SpotKind::Gate) | None => {}
        }
        if self.touch {
            self.picked_card(ctx, &d, &inv, &mut out);
        }
        out
    }

    /// Touch: the item tapped, what it is and what can be done with it.
    fn picked_card(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let Some((id, from)) = self.picked else { return };
        let vendor = self.panel == Some(SpotKind::Vendor);
        let stash = self.panel == Some(SpotKind::Stash);
        let find = |list: &[Item]| list.iter().find(|i| i.id == id).cloned();
        let item = match from {
            Picked::Bag if self.inventory => find(&inv.inventory),
            Picked::Worn(slot) if self.inventory => inv.equipment.get(slot).and_then(|e| e.clone()).filter(|i| i.id == id),
            Picked::Vendor if vendor => find(&inv.vendor),
            Picked::Buyback if vendor => find(&inv.buyback),
            Picked::Stash if stash => find(&inv.stash),
            _ => None,
        };
        let Some(it) = item else {
            self.picked = None;
            return;
        };
        let screen = ctx.content_rect();
        let mut keep = true;
        let mut act = |c: GameCmd, keep: &mut bool| {
            out.push(c);
            *keep = false;
        };
        egui::Window::new("item")
            .id(egui::Id::new("picked_item"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, EVec2::ZERO)
            .max_width((screen.width() - 24.0).min(380.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(screen.height() - 40.0).show(ui, |ui| {
                    let worn = worn_for(inv, d, &it);
                    let title = if let Picked::Worn(s) = from { EquipSlot::from_index(s).map(slot_title) } else { None };
                    item_tooltip(ui, d, &it, title.as_deref());
                    if !matches!(from, Picked::Worn(_)) {
                        compare(ui, d, &it, worn);
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        match from {
                            Picked::Bag => {
                                if ui.button(t!("item.wear")).clicked() {
                                    act(GameCmd::Equip(id), &mut keep);
                                }
                                if it.slot(d) == Slot::Ring && ui.button(t!("item.wear_right")).clicked() {
                                    act(GameCmd::EquipTo(id, EquipSlot::Ring2.index() as u8), &mut keep);
                                }
                                if vendor && ui.button(t!("item.sell_for", count = it.value())).clicked() {
                                    act(GameCmd::Sell(id), &mut keep);
                                }
                                if stash && ui.button(t!("item.stash")).clicked() {
                                    act(GameCmd::Stash(id), &mut keep);
                                }
                                if ui.button(t!("item.drop")).clicked() {
                                    act(GameCmd::Drop(id), &mut keep);
                                }
                            }
                            Picked::Worn(slot) => {
                                if ui.button(t!("item.take_off")).clicked() {
                                    act(GameCmd::Unequip(slot as u8), &mut keep);
                                }
                            }
                            Picked::Vendor => {
                                let price = pav_core::arpg::cmd::buy_price(&it);
                                if ui
                                    .add_enabled(inv.gold >= price, egui::Button::new(t!("item.buy_for", count = price)))
                                    .clicked()
                                {
                                    act(GameCmd::Buy(id), &mut keep);
                                }
                            }
                            Picked::Buyback => {
                                if ui
                                    .add_enabled(
                                        inv.gold >= it.value(),
                                        egui::Button::new(t!("item.buy_back_for", count = it.value())),
                                    )
                                    .clicked()
                                {
                                    act(GameCmd::Buy(id), &mut keep);
                                }
                            }
                            Picked::Stash => {
                                if ui.button(t!("item.take")).clicked() {
                                    act(GameCmd::Take(id), &mut keep);
                                }
                            }
                        }
                        if ui.button(t!("ui.close")).clicked() {
                            keep = false;
                        }
                    });
                    if let Some(w) = worn.filter(|_| matches!(from, Picked::Bag | Picked::Vendor | Picked::Stash)) {
                        ui.separator();
                        item_tooltip(ui, d, w, Some(&t!("item.wearing")));
                    }
                })
            });
        if !keep {
            self.picked = None;
        }
    }

    fn inventory_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let vendor = self.panel == Some(SpotKind::Vendor);
        let stash = self.panel == Some(SpotKind::Stash);
        let touch = self.touch;
        fit(egui::Window::new(t!("inventory.title")).id(egui::Id::new("inventory")), ctx, touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::RIGHT_CENTER, if touch { EVec2::new(-8.0, TOUCH_DROP) } else { EVec2::new(-16.0, -40.0) })
            .show(ctx, |ui| {
                // Paper doll.
                let size = 54.0;
                let (area, _) = ui.allocate_exact_size(EVec2::new(4.7 * (size + 8.0), 4.0 * (size + 6.0)), Sense::hover());
                for (slot, cx, cy) in DOLL {
                    let r = Rect::from_min_size(area.min + EVec2::new(cx * (size + 8.0), cy * (size + 6.0)), EVec2::splat(size));
                    let item = inv.equipment.get(slot.index()).and_then(|i| i.as_ref());
                    let label = match slot {
                        EquipSlot::Ring1 | EquipSlot::Ring2 => t!("doll.ring"),
                        s => t!(&format!("doll.{}", s.key())),
                    };
                    let picked = item.is_some_and(|it| self.picked == Some((it.id, Picked::Worn(slot.index()))));
                    let resp = ui.put(r, |ui: &mut egui::Ui| cell(ui, d, item, size, &label, picked));
                    if let Some(it) = item {
                        if touch {
                            if resp.clicked() {
                                self.picked = Some((it.id, Picked::Worn(slot.index())));
                            }
                        } else if resp.clicked() || resp.secondary_clicked() {
                            out.push(GameCmd::Unequip(slot.index() as u8));
                        }
                        resp.on_hover_ui(|ui| {
                            item_tooltip(ui, d, it, Some(&slot_title(slot)));
                            let take_off = if self.pad { t!("hint.take_off.pad") } else { t!("hint.take_off") };
                            ui.label(RichText::new(take_off).small().color(Color32::from_white_alpha(120)));
                        });
                    }
                }
                ui.label(RichText::new(t!("hud.gold", count = inv.gold)).color(Color32::from_rgb(255, 205, 70)));
                ui.separator();
                // The bag: 8 x 5.
                let pad = self.pad;
                let hint_text = match (touch, pad) {
                    (true, _) => t!("hint.bag.touch"),
                    (_, true) if vendor => t!("hint.bag.sell.pad"),
                    (_, true) if stash => t!("hint.bag.stash.pad"),
                    (_, true) => t!("hint.bag.wear.pad"),
                    _ if vendor => t!("hint.bag.sell"),
                    _ if stash => t!("hint.bag.stash"),
                    _ => t!("hint.bag.wear"),
                };
                egui::Grid::new("bag").spacing(EVec2::splat(4.0)).show(ui, |ui| {
                    for i in 0..pav_core::arpg::hero::INVENTORY_SIZE {
                        let it = inv.inventory.get(i);
                        let better = it.is_some_and(|it| is_upgrade(d, inv, it));
                        let resp = cell(ui, d, it, if touch { 40.0 } else { 42.0 }, "", better);
                        if let Some(it) = it {
                            if touch && resp.clicked() {
                                self.picked = Some((it.id, Picked::Bag));
                            } else if resp.clicked() {
                                out.push(if vendor {
                                    GameCmd::Sell(it.id)
                                } else if stash {
                                    GameCmd::Stash(it.id)
                                } else {
                                    GameCmd::Equip(it.id)
                                });
                            }
                            let id = it.id;
                            let ring = it.slot(d) == Slot::Ring;
                            resp.context_menu(|ui| {
                                if ui.button(t!("item.wear")).clicked() {
                                    out.push(GameCmd::Equip(id));
                                    ui.close();
                                }
                                if ring && ui.button(t!("item.wear_right")).clicked() {
                                    out.push(GameCmd::EquipTo(id, EquipSlot::Ring2.index() as u8));
                                    ui.close();
                                }
                                if vendor && ui.button(t!("item.sell")).clicked() {
                                    out.push(GameCmd::Sell(id));
                                    ui.close();
                                }
                                if stash && ui.button(t!("item.stash")).clicked() {
                                    out.push(GameCmd::Stash(id));
                                    ui.close();
                                }
                                if ui.button(t!("item.drop")).clicked() {
                                    out.push(GameCmd::Drop(id));
                                    ui.close();
                                }
                            });
                            if !touch {
                                resp.on_hover_ui(|ui| {
                                    ui.horizontal_top(|ui| {
                                        ui.vertical(|ui| {
                                            item_tooltip(ui, d, it, None);
                                            compare(ui, d, it, worn_for(inv, d, it));
                                        });
                                        if let Some(w) = worn_for(inv, d, it) {
                                            ui.separator();
                                            ui.vertical(|ui| item_tooltip(ui, d, w, Some(&t!("item.wearing"))));
                                        }
                                    });
                                });
                            }
                        }
                        if (i + 1) % 8 == 0 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(t!("inventory.sort")).clicked() {
                        out.push(GameCmd::Sort);
                    }
                    ui.label(t!("inventory.auto_pickup"));
                    let names =
                        ["everything", "magic", "rare", "unique", "off"].map(|n| t!(&format!("inventory.auto_pickup.{n}")));
                    let mut sel = inv.auto_loot.min(4) as usize;
                    egui::ComboBox::from_id_salt("autoloot").selected_text(names[sel].as_str()).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if ui.selectable_value(&mut sel, i, n.as_str()).clicked() {
                                out.push(GameCmd::AutoLoot(i as u8));
                            }
                        }
                    });
                });
                let footer = match (touch, pad) {
                    (true, _) => t!("inventory.footer.touch", hint = hint_text),
                    (_, true) => t!("inventory.footer.pad", hint = hint_text),
                    _ => t!("inventory.footer", hint = hint_text),
                };
                ui.label(RichText::new(footer).small().color(Color32::from_white_alpha(120)));
            });
        if !open {
            self.inventory = false;
        }
    }

    fn vendor_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let touch = self.touch;
        fit(egui::Window::new(t!("spot.smith")).id(egui::Id::new("vendor")), ctx, touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_CENTER, if self.touch { EVec2::new(8.0, TOUCH_DROP) } else { EVec2::new(16.0, -40.0) })
            .show(ctx, |ui| {
                ui.label(RichText::new(t!("vendor.greeting")).italics().color(Color32::from_rgb(200, 170, 130)));
                ui.separator();
                egui::Grid::new("wares").spacing(EVec2::splat(6.0)).show(ui, |ui| {
                    for (i, it) in inv.vendor.iter().enumerate() {
                        let price = pav_core::arpg::cmd::buy_price(it);
                        ui.vertical(|ui| {
                            let resp = cell(ui, d, Some(it), 50.0, "", is_upgrade(d, inv, it));
                            let afford = inv.gold >= price;
                            ui.label(RichText::new(format!("{price}")).small().color(if afford {
                                Color32::from_rgb(255, 205, 70)
                            } else {
                                Color32::from_rgb(160, 80, 70)
                            }));
                            if touch {
                                if resp.clicked() {
                                    self.picked = Some((it.id, Picked::Vendor));
                                }
                                return;
                            }
                            if resp.clicked() {
                                out.push(GameCmd::Buy(it.id));
                            }
                            resp.on_hover_ui(|ui| {
                                item_tooltip(ui, d, it, None);
                                compare(ui, d, it, worn_for(inv, d, it));
                                let buy = if self.pad {
                                    t!("vendor.buy_hint.pad", count = price)
                                } else {
                                    t!("vendor.buy_hint", count = price)
                                };
                                ui.label(RichText::new(buy).color(Color32::from_rgb(255, 205, 70)));
                            });
                        });
                        if (i + 1) % 6 == 0 {
                            ui.end_row();
                        }
                    }
                });
                if !inv.buyback.is_empty() {
                    ui.separator();
                    ui.label(t!("vendor.buy_back"));
                    ui.horizontal(|ui| {
                        for it in &inv.buyback {
                            let resp = cell(ui, d, Some(it), 40.0, "", false);
                            if touch {
                                if resp.clicked() {
                                    self.picked = Some((it.id, Picked::Buyback));
                                }
                                continue;
                            }
                            if resp.clicked() {
                                out.push(GameCmd::Buy(it.id));
                            }
                            resp.on_hover_ui(|ui| {
                                item_tooltip(ui, d, it, None);
                                ui.label(
                                    RichText::new(t!("vendor.buy_back_for", count = it.value()))
                                        .color(Color32::from_rgb(255, 205, 70)),
                                );
                            });
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t!("vendor.sell_normal")).clicked() {
                        out.push(GameCmd::SellAll(0));
                    }
                    if ui.button(t!("vendor.sell_magic")).clicked() {
                        out.push(GameCmd::SellAll(1));
                    }
                });
            });
        if !open {
            self.panel = None;
        }
    }

    fn stash_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let touch = self.touch;
        let title = t!("stash.title", stored = inv.stash.len(), size = pav_core::arpg::hero::STASH_SIZE);
        fit(egui::Window::new(title).id(egui::Id::new("stash")), ctx, touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_CENTER, if self.touch { EVec2::new(8.0, TOUCH_DROP) } else { EVec2::new(16.0, -40.0) })
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("stash").spacing(EVec2::splat(4.0)).show(ui, |ui| {
                        for i in 0..pav_core::arpg::hero::STASH_SIZE {
                            let it = inv.stash.get(i);
                            let resp = cell(ui, d, it, 40.0, "", false);
                            if let Some(it) = it {
                                if touch {
                                    if resp.clicked() {
                                        self.picked = Some((it.id, Picked::Stash));
                                    }
                                } else if resp.clicked() {
                                    out.push(GameCmd::Take(it.id));
                                }
                                resp.on_hover_ui(|ui| {
                                    item_tooltip(ui, d, it, None);
                                    compare(ui, d, it, worn_for(inv, d, it));
                                });
                            }
                            if (i + 1) % 10 == 0 {
                                ui.end_row();
                            }
                        }
                    });
                });
                let help = match (touch, self.pad) {
                    (true, _) => t!("hint.stash.touch"),
                    (_, true) => t!("hint.stash.pad"),
                    _ => t!("hint.stash"),
                };
                ui.label(RichText::new(help).small().color(Color32::from_white_alpha(120)));
            });
        if !open {
            self.panel = None;
        }
    }

    fn gamble_window(&mut self, ctx: &egui::Context, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        fit(egui::Window::new(t!("spot.gambler")).id(egui::Id::new("gamble")), ctx, self.touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_CENTER, if self.touch { EVec2::new(8.0, TOUCH_DROP) } else { EVec2::new(16.0, -40.0) })
            .show(ctx, |ui| {
                ui.label(RichText::new(t!("gamble.pitch")).italics());
                ui.label(
                    RichText::new(t!("gamble.odds", magic = 60, rare = 22, unique = 3, levels = 2))
                        .small()
                        .color(Color32::from_white_alpha(150)),
                );
                ui.add_space(4.0);
                egui::Grid::new("gamble").num_columns(2).spacing(EVec2::new(10.0, 6.0)).show(ui, |ui| {
                    for (i, slot) in Slot::ALL.iter().enumerate() {
                        let price = pav_core::arpg::scene::gamble_price(inv.level, *slot);
                        let ok = inv.gold >= price && inv.inventory.len() < pav_core::arpg::hero::INVENTORY_SIZE;
                        let b = ui.add_enabled(
                            ok,
                            egui::Button::new(RichText::new(lines::slot_name(*slot)).size(15.0))
                                .min_size(EVec2::new(130.0, 26.0)),
                        );
                        if b.clicked() {
                            out.push(GameCmd::Gamble(i as u8));
                        }
                        let c = if inv.gold >= price { Color32::from_rgb(255, 205, 70) } else { Color32::from_rgb(170, 90, 80) };
                        ui.label(RichText::new(t!("hud.gold", count = price)).color(c));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new(t!("shop.you_have", count = inv.gold)).color(Color32::from_rgb(255, 205, 70)));
            });
        if !open {
            self.panel = None;
        }
    }

    fn alchemist_window(&mut self, ctx: &egui::Context, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        fit(egui::Window::new(t!("alchemist.title")).id(egui::Id::new("alchemist")), ctx, self.touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, if self.touch { EVec2::new(0.0, TOUCH_DROP) } else { EVec2::ZERO })
            .show(ctx, |ui| {
                ui.label(RichText::new(t!("alchemist.pitch")).italics());
                ui.add_space(4.0);
                let rows = [
                    ("flask", t!("alchemist.flask.now", count = inv.potion_max, most = 6)),
                    ("brew", t!("alchemist.brew.now", percent = (45.0 * inv.sheet.potion).round() as i64)),
                ];
                for (k, (name, now)) in rows.iter().enumerate() {
                    ui.horizontal(|ui| {
                        match inv.brew[k] {
                            Some(price) => {
                                let b = ui.add_enabled(
                                    inv.gold >= price,
                                    egui::Button::new(
                                        RichText::new(t!(&format!("alchemist.{name}.buy"), count = price)).size(15.0),
                                    )
                                    .min_size(EVec2::new(240.0, 28.0)),
                                );
                                if b.clicked() {
                                    out.push(GameCmd::Brew(k as u8));
                                }
                            }
                            None => {
                                ui.add_enabled(
                                    false,
                                    egui::Button::new(t!(&format!("alchemist.{name}.maxed"))).min_size(EVec2::new(240.0, 28.0)),
                                );
                            }
                        }
                        ui.label(RichText::new(now).small().color(Color32::from_white_alpha(170)));
                    });
                }
                ui.label(RichText::new(t!("shop.you_have", count = inv.gold)).color(Color32::from_rgb(255, 205, 70)));
            });
        if !open {
            self.panel = None;
        }
    }

    fn exhibit_window(&mut self, ctx: &egui::Context, g: &GameFrame, out: &mut Vec<GameCmd>) {
        let Some(i) = self.panel_spot else { return };
        let Some(s) = g.spots.get(i) else { return };
        let mut open = true;
        fit(egui::Window::new(lines::spot_name(&s.name)).id(egui::Id::new(("exhibit", i))), ctx, self.touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::RIGHT_TOP, EVec2::new(-16.0, 80.0))
            .show(ctx, |ui| {
                for l in &s.info {
                    ui.label(l);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t!("exhibit.release")).clicked() {
                        out.push(GameCmd::Release(i as u32));
                        self.panel = None;
                    }
                    if ui.button(t!("exhibit.reroll")).clicked() {
                        out.push(GameCmd::Reroll);
                        self.panel = None;
                    }
                });
            });
        if !open {
            self.panel = None;
        }
    }

    /// A pedestal in the Hall of Heroes: who they are, how they move, and a button to play as
    /// them (the n-th pedestal is the n-th character).
    fn hero_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, g: &GameFrame, out: &mut Vec<GameCmd>) {
        let Some(i) = self.panel_spot else { return };
        let Some(s) = g.spots.get(i) else { return };
        let n = g.spots[..i].iter().filter(|s| s.kind == SpotKind::Hero).count();
        let Some(c) = d.characters.get(n) else { return };
        let mut open = true;
        fit(egui::Window::new(lines::spot_name(&s.name)).id(egui::Id::new(("hero", i))), ctx, self.touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(360.0)
            .anchor(Align2::RIGHT_TOP, EVec2::new(-16.0, 80.0))
            .show(ctx, |ui| {
                for l in &s.info {
                    ui.label(l);
                }
                ui.separator();
                if inv.character == c.key {
                    ui.label(RichText::new(t!("hall.you_are", name = lines::hero_name(c))).italics());
                } else if ui.button(RichText::new(t!("hall.play_as", name = lines::hero_name(c))).size(16.0)).clicked() {
                    out.push(GameCmd::Character(n as u8));
                    self.panel = None;
                }
            });
        if !open {
            self.panel = None;
        }
    }

    fn portal_window(&mut self, ctx: &egui::Context, g: &GameFrame, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let deepest = g.inv.as_ref().map(|i| i.max_depth).unwrap_or(0).max(1);
        let d = data();
        fit(egui::Window::new(t!("spot.portal")).id(egui::Id::new("portal")), ctx, self.touch)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, if self.touch { EVec2::new(0.0, TOUCH_DROP) } else { EVec2::ZERO })
            .show(ctx, |ui| {
                ui.label(RichText::new(t!("portal.where")).strong());
                for p in [Place::Town, Place::Arena, Place::Lab] {
                    let here = p == g.place;
                    let b = ui.add_enabled(
                        !here,
                        egui::Button::new(RichText::new(p.name()).size(16.0)).min_size(EVec2::new(300.0, 30.0)),
                    );
                    if b.clicked() {
                        out.push(GameCmd::Travel(p.code()));
                        self.panel = None;
                    }
                }
                ui.separator();
                ui.label(RichText::new(t!("portal.waypoints")).strong());
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for n in (1..=deepest).rev() {
                        let plan = pav_core::arpg::world::plan(&d, n);
                        let p = Place::Level(n);
                        let here = p == g.place;
                        let mut text =
                            RichText::new(t!("level.card", label = plan.label(), name = lines::plan_name(&plan))).size(15.0);
                        if plan.endless {
                            text = text.color(Color32::from_rgb(200, 170, 255));
                        }
                        let b = ui.add_enabled(!here, egui::Button::new(text).min_size(EVec2::new(300.0, 26.0)));
                        let b = b.on_hover_ui(|ui| {
                            ui.label(RichText::new(lines::plan_about(&plan)).italics());
                            let mech: Vec<String> = plan.mechanics.iter().map(|m| lines::mechanic_name(*m)).collect();
                            ui.label(t!("portal.mechanics", mechanics = lines::list_comma(&mech)));
                            ui.label(t!("portal.monster_level", level = plan.monster_level));
                            if let Some(b) = &plan.boss {
                                let name = lines::boss_name_of(&d, b, plan.monster_level);
                                ui.label(RichText::new(t!("portal.boss", name = name)).color(Color32::from_rgb(255, 150, 80)));
                            }
                        });
                        if b.clicked() {
                            out.push(GameCmd::Travel(p.code()));
                            self.panel = None;
                        }
                    }
                });
            });
        if !open {
            self.panel = None;
        }
    }
}

/// An item that beats what's worn in its slot (by value of its stats: a rough guide).
fn is_upgrade(d: &Data, inv: &InvView, it: &Item) -> bool {
    let score = |i: &Item| i.score(d);
    match worn_for(inv, d, it) {
        Some(w) => score(it) > score(w) * 1.08,
        None => true,
    }
}

fn character_window(ctx: &egui::Context, open: &mut bool, inv: &InvView, g: &GameFrame, touch: bool) {
    let s = &inv.sheet;
    let d = data();
    fit(egui::Window::new(t!("character.title")).id(egui::Id::new("character")), ctx, touch)
        .open(open)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::LEFT_TOP, if touch { EVec2::new(8.0, TOUCH_TOP) } else { EVec2::new(16.0, 60.0) })
        .show(ctx, |ui| {
            if let Some(h) = &g.hero {
                ui.label(RichText::new(t!("character.heading", name = &h.name, level = h.level)).size(17.0).strong());
            }
            // Which playable character (others wait in the Hall of Heroes, in town).
            if let Some(c) = d.character(&inv.character).filter(|c| !c.title.is_empty()) {
                ui.label(RichText::new(lines::hero_title(c)).italics().color(Color32::from_rgb(200, 190, 170)));
            }
            let w = &inv.weapon;
            // The weapon worn (by its name in the language in use), or bare hands.
            let weapon = match inv.equipment.get(EquipSlot::Weapon.index()).and_then(|e| e.as_ref()) {
                Some(it) => lines::item_name(&d, it),
                None => lines::piece("character.fists", &[]),
            };
            // Signed whole percentages ("+12", "-5") and numbers to one or two places.
            let pct = |v: f32| format!("{:+.0}", v);
            let places = |v: f32, n: i32| ((v as f64) * 10f64.powi(n)).round() / 10f64.powi(n);
            egui::Grid::new("sheet").num_columns(2).striped(true).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(t!(k));
                    ui.label(RichText::new(v).color(Color32::from_rgb(235, 225, 200)));
                    ui.end_row();
                };
                row("character.life", t!("character.regen", value = s.life_max.round() as i64, regen = places(s.life_regen, 1)));
                row("character.mana", t!("character.regen", value = s.mana_max.round() as i64, regen = places(s.mana_regen, 1)));
                row(
                    "character.weapon",
                    t!(
                        "character.weapon.value",
                        name = weapon,
                        min = w.phys[0].round() as i64,
                        max = w.phys[1].round() as i64,
                        speed = places(w.aps, 2)
                    ),
                );
                row("character.attack_speed", t!("character.percent", value = pct((s.attack_speed - 1.0) * 100.0)));
                row("character.cast_speed", t!("character.percent", value = pct((s.cast_speed - 1.0) * 100.0)));
                row(
                    "character.crit",
                    t!(
                        "character.crit.value",
                        chance = places(w.crit * s.crit_inc + s.crit_flat, 1),
                        multiplier = (s.crit_multi * 100.0).round() as i64
                    ),
                );
                row("character.armour", (s.armor.round() as i64).to_string());
                row(
                    "character.evade_block",
                    t!("character.evade_block.value", evade = s.evasion.round() as i64, block = s.block.round() as i64),
                );
                row(
                    "character.resists",
                    t!(
                        "character.resists.value",
                        fire = s.res[1].round() as i64,
                        cold = s.res[2].round() as i64,
                        lightning = s.res[3].round() as i64,
                        poison = s.res[4].round() as i64
                    ),
                );
                row("character.movement", t!("character.percent", value = pct((s.move_speed - 1.0) * 100.0)));
                row("character.cooldowns", t!("character.faster", value = pct((s.cooldown - 1.0) * 100.0)));
                row("character.area", t!("character.percent", value = pct((s.area - 1.0) * 100.0)));
                row(
                    "character.rarity_gold",
                    t!("character.rarity_gold.value", rarity = pct(s.item_rarity), gold = pct(s.gold_find)),
                );
                row("character.experience", t!("character.percent", value = pct((s.xp_gain - 1.0) * 100.0)));
            });
            let m = &s.mods;
            let incs: Vec<String> = [
                (Stat::DamageInc, "all"),
                (Stat::PhysInc, "physical"),
                (Stat::FireInc, "fire"),
                (Stat::ColdInc, "cold"),
                (Stat::LightningInc, "lightning"),
                (Stat::PoisonInc, "poison"),
                (Stat::ElementalInc, "elemental"),
                (Stat::AttackInc, "attack"),
                (Stat::SpellInc, "spell"),
                (Stat::MeleeInc, "melee"),
                (Stat::DamageMore, "more"),
            ]
            .iter()
            .filter(|(st, _)| m.get(*st).abs() > 0.05)
            .map(|(st, n)| t!(&format!("character.damage.{n}"), value = pct(m.get(*st))))
            .collect();
            if !incs.is_empty() {
                ui.separator();
                ui.label(RichText::new(t!("character.damage")).strong());
                ui.label(incs.join("   "));
            }
            if !inv.powers.is_empty() {
                ui.separator();
                ui.label(RichText::new(t!("character.powers")).strong());
                for p in &inv.powers {
                    ui.label(RichText::new(p.describe()).color(Color32::from_rgb(255, 150, 60)));
                }
            }
        });
}

fn skills_window(ctx: &egui::Context, open: &mut bool, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>, touch: bool) {
    fit(egui::Window::new(t!("skills.title")).id(egui::Id::new("skills")), ctx, touch)
        .open(open)
        .resizable(false)
        .collapsible(false)
        .anchor(
            if touch { Align2::CENTER_CENTER } else { Align2::CENTER_BOTTOM },
            EVec2::new(0.0, if touch { TOUCH_DROP } else { -140.0 }),
        )
        .show(ctx, |ui| {
            ui.label(t!("skills.choose"));
            let keys: [String; 6] = std::array::from_fn(|i| match (touch, i) {
                (true, 0) => t!("skills.key.auto"),
                (true, i) => t!("skills.key.button", number = i),
                (false, 0) => t!("skills.key.left"),
                (false, 1) => t!("skills.key.right"),
                (false, i) => ["Q", "E", "R", "F"][i - 2].to_string(),
            });
            let skills = d.hero_skills();
            egui::Grid::new("bar").num_columns(2).show(ui, |ui| {
                for (slot, key) in keys.iter().enumerate() {
                    ui.label(key);
                    let cur = inv.bar[slot].clone();
                    let cur_name = d.skill_id(&cur).map(|i| lines::skill_name(d.skill(i))).unwrap_or_default();
                    egui::ComboBox::from_id_salt(("bar", slot)).selected_text(cur_name).width(200.0).show_ui(ui, |ui| {
                        for id in &skills {
                            let s = d.skill(*id);
                            let locked = s.unlock > inv.level;
                            let name = lines::skill_name(s);
                            let label = if locked { t!("skills.locked", skill = name, level = s.unlock) } else { name };
                            let r = ui.add_enabled(!locked, egui::Button::selectable(s.key == cur, label));
                            if r.clicked() {
                                out.push(GameCmd::Bar(slot as u8, *id));
                            }
                            r.on_hover_text(lines::skill_about(s));
                        }
                    });
                    ui.end_row();
                }
            });
            if !inv.tweaks.is_empty() {
                ui.separator();
                ui.label(RichText::new(t!("skills.upgrades")).strong());
                for tw in &inv.tweaks {
                    ui.label(
                        RichText::new(tw.describe(&lines::skill_name_of(d, &tw.skill))).color(Color32::from_rgb(140, 200, 255)),
                    );
                }
            }
        });
}

/// Names over items on the ground (click to pick up) and over usable spots.
fn labels(ctx: &egui::Context, g: &GameFrame, proj: &Projector, out: &mut Vec<GameCmd>, touch: bool) {
    let mut placed: Vec<Rect> = Vec::new();
    let mut loot: Vec<_> = g.loot.iter().filter(|l| l.rest || l.age > 0.4).collect();
    loot.sort_by_key(|l| std::cmp::Reverse(l.rarity));
    for l in loot {
        let Some(s) = proj.to_screen(l.pos + Vec3::Y * 0.35) else { continue };
        let font = FontId::proportional(if l.rarity >= Rarity::Rare { 14.0 } else { 12.5 });
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(l.name.clone(), font.clone(), rarity_color(l.rarity)));
        let size = galley.size() + EVec2::new(10.0, 4.0);
        let mut rect = Rect::from_center_size(s, size);
        // Stack labels that would overlap.
        for _ in 0..12 {
            match placed.iter().find(|r| r.intersects(rect)) {
                Some(r) => rect = rect.translate(EVec2::new(0.0, r.top() - rect.bottom() - 2.0)),
                None => break,
            }
        }
        placed.push(rect);
        let id = egui::Id::new(("loot", l.id));
        egui::Area::new(id).fixed_pos(rect.min).order(egui::Order::Background).interactable(true).show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter();
            let bg = if resp.hovered() { Color32::from_black_alpha(235) } else { Color32::from_black_alpha(175) };
            p.rect_filled(r, 3.0, bg);
            if l.rarity >= Rarity::Rare {
                p.rect_stroke(r, 3.0, Stroke::new(1.0, rarity_color(l.rarity)), StrokeKind::Inside);
            }
            p.galley(r.min + EVec2::new(5.0, 2.0), galley, Color32::WHITE);
            if resp.clicked() {
                out.push(GameCmd::Pickup(l.id));
            }
        });
    }
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("spot_labels")));
    for (i, s) in g.spots.iter().enumerate() {
        let Some(at) = proj.to_screen(s.pos + Vec3::Y * 2.5) else { continue };
        let name = spot_name(&s.name);
        let near = g.near == Some(i);
        let c = if near { Color32::from_rgb(255, 225, 150) } else { Color32::from_rgb(200, 190, 170) };
        p.text(
            at + EVec2::new(1.0, 1.0),
            Align2::CENTER_CENTER,
            &name,
            FontId::proportional(15.0),
            Color32::from_black_alpha(200),
        );
        p.text(at, Align2::CENTER_CENTER, &name, FontId::proportional(15.0), c);
        if near && !touch {
            let what = match s.kind {
                SpotKind::Vendor => t!("spot.verb.trade"),
                SpotKind::Stash => t!("spot.verb.stash"),
                SpotKind::Portal => t!("spot.verb.travel"),
                SpotKind::Exhibit => t!("spot.verb.examine"),
                SpotKind::Exit => t!("spot.verb.descend"),
                SpotKind::Gamble => t!("spot.verb.gamble"),
                SpotKind::Alchemist => t!("spot.verb.brew"),
                SpotKind::Chest => t!("spot.verb.chest"),
                SpotKind::Hero => t!("spot.verb.hero"),
                SpotKind::Gate => t!("tools.gate.use"),
            };
            p.text(
                at + EVec2::new(0.0, 18.0),
                Align2::CENTER_CENTER,
                t!("spot.hint", action = what),
                FontId::proportional(12.0),
                Color32::from_white_alpha(220),
            );
        }
    }
}

/// A spot's name: a line id ("spot.smith") shows the line; others carry their name as made.
fn spot_name(name: &str) -> String {
    pav_core::lines::display(lines::spot_name(name))
}

/// A worn slot's name, over an item's tooltip ("Left Ring").
fn slot_title(s: EquipSlot) -> String {
    lines::equip_slot_name(s)
}
