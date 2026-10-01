//! Shardfall menus: the inventory with a paper doll, item tooltips that compare with what is
//! worn, the smith's shop, the stash, the portal's destinations, the character sheet and the
//! skill bar picker, plus clickable loot labels on the ground. Everything the player does here
//! becomes a `GameCmd` sent to the simulation (so it records and replays like any input).

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2 as EVec2};
use glam::Vec3;
use pav_core::arpg::combat::Rarity;
use pav_core::arpg::data::{Data, data};
use pav_core::arpg::items::{EquipSlot, Item, Slot};
use pav_core::arpg::stats::{Mods, Stat, describe};
use pav_core::arpg::{GameCmd, GameFrame, InvView, Place, SpotKind};

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
}

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
    ui.label(RichText::new(format!("{}   ·   item level {}", t.kind, t.level)).small().color(Color32::from_white_alpha(150)));
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
                    ui.label(
                        RichText::new(format!("T{}{}", tier + 1, if *local { " local" } else { "" }))
                            .small()
                            .color(Color32::from_white_alpha(90)),
                    );
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
    ui.label(RichText::new(format!("Sells for {} gold", it.value())).small().color(Color32::from_rgb(220, 180, 80)));
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
        RichText::new(if old.is_some() { "If you wear this instead:" } else { "If you wear this:" })
            .small()
            .color(Color32::from_white_alpha(150)),
    );
    if new.slot(d) == Slot::Weapon {
        let nd = (ns.phys[0] + ns.phys[1]) * 0.5 * ns.aps;
        let od = (os.phys[0] + os.phys[1]) * 0.5 * os.aps;
        line(ui, nd - od, &format!("{:+.1} weapon damage per second", nd - od));
    }
    let mut any = false;
    for (s, v) in &diff.0 {
        if v.abs() < 0.05 {
            continue;
        }
        any = true;
        let text = describe(*s, v.abs());
        let text = if *v < 0.0 { format!("lose: {text}") } else { text };
        line(ui, *v, &text);
    }
    if !any && new.slot(d) != Slot::Weapon {
        ui.label(RichText::new("no stat changes").small());
    }
    let np = ns.power.map(|p| p.describe());
    let op = os.power.map(|p| p.describe());
    if np != op {
        if let Some(p) = op {
            line(ui, -1.0, &format!("lose: {p}"));
        }
        if let Some(p) = np {
            line(ui, 1.0, &p);
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
            let same = self.panel == Some(s.kind) && self.panel_spot == g.near;
            self.panel = if same { None } else { Some(s.kind) };
            self.panel_spot = g.near;
            if matches!(s.kind, SpotKind::Vendor | SpotKind::Stash) && self.panel.is_some() {
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
                || (k == SpotKind::Exhibit && g.near != self.panel_spot)
            {
                self.panel = None;
            }
        }
        labels(ctx, g, proj, &mut out);
        let Some(inv) = g.inv.clone() else { return out };
        if self.inventory {
            self.inventory_window(ctx, &d, &inv, &mut out);
        }
        if self.character {
            character_window(ctx, &mut self.character, &inv, g);
        }
        if self.skills {
            skills_window(ctx, &mut self.skills, &d, &inv, &mut out);
        }
        self.tree.ui(ctx, &inv, &mut out);
        if inv.points > 0 && !self.tree.open {
            // A nudge above the experience bar.
            let screen = ctx.content_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("points_hint")));
            let at = Pos2::new(screen.center().x, screen.bottom() - 28.0);
            p.text(
                at,
                Align2::CENTER_BOTTOM,
                format!("+{} passive point{}  (P)", inv.points, if inv.points == 1 { "" } else { "s" }),
                FontId::proportional(14.0),
                Color32::from_rgb(255, 215, 120),
            );
        }
        match self.panel {
            Some(SpotKind::Vendor) => self.vendor_window(ctx, &d, &inv, &mut out),
            Some(SpotKind::Stash) => self.stash_window(ctx, &d, &inv, &mut out),
            Some(SpotKind::Portal) => self.portal_window(ctx, g, &mut out),
            Some(SpotKind::Exhibit) => self.exhibit_window(ctx, g, &mut out),
            Some(SpotKind::Exit | SpotKind::Chest) | None => {}
        }
        out
    }

    fn inventory_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let vendor = self.panel == Some(SpotKind::Vendor);
        let stash = self.panel == Some(SpotKind::Stash);
        egui::Window::new("Inventory")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::RIGHT_CENTER, EVec2::new(-16.0, -40.0))
            .show(ctx, |ui| {
                // Paper doll.
                let size = 54.0;
                let (area, _) = ui.allocate_exact_size(EVec2::new(4.7 * (size + 8.0), 4.0 * (size + 6.0)), Sense::hover());
                for (slot, cx, cy) in DOLL {
                    let r = Rect::from_min_size(area.min + EVec2::new(cx * (size + 8.0), cy * (size + 6.0)), EVec2::splat(size));
                    let item = inv.equipment.get(slot.index()).and_then(|i| i.as_ref());
                    let label = match slot {
                        EquipSlot::Ring1 | EquipSlot::Ring2 => "ring",
                        s => s.key(),
                    };
                    let resp = ui.put(r, |ui: &mut egui::Ui| cell(ui, d, item, size, label, false));
                    if let Some(it) = item {
                        if resp.clicked() || resp.secondary_clicked() {
                            out.push(GameCmd::Unequip(slot.index() as u8));
                        }
                        resp.on_hover_ui(|ui| {
                            item_tooltip(ui, d, it, Some(slot.name()));
                            ui.label(RichText::new("Click: take off").small().color(Color32::from_white_alpha(120)));
                        });
                    }
                }
                ui.label(RichText::new(format!("{} gold", inv.gold)).color(Color32::from_rgb(255, 205, 70)));
                ui.separator();
                // The bag: 8 x 5.
                let hint = if vendor {
                    "Click: sell · Right-click: more"
                } else if stash {
                    "Click: stash · Right-click: more"
                } else {
                    "Click: wear · Right-click: more"
                };
                egui::Grid::new("bag").spacing(EVec2::splat(4.0)).show(ui, |ui| {
                    for i in 0..pav_core::arpg::hero::INVENTORY_SIZE {
                        let it = inv.inventory.get(i);
                        let better = it.is_some_and(|it| is_upgrade(d, inv, it));
                        let resp = cell(ui, d, it, 42.0, "", better);
                        if let Some(it) = it {
                            if resp.clicked() {
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
                                if ui.button("Wear").clicked() {
                                    out.push(GameCmd::Equip(id));
                                    ui.close();
                                }
                                if ring && ui.button("Wear on the right hand").clicked() {
                                    out.push(GameCmd::EquipTo(id, EquipSlot::Ring2.index() as u8));
                                    ui.close();
                                }
                                if vendor && ui.button("Sell").clicked() {
                                    out.push(GameCmd::Sell(id));
                                    ui.close();
                                }
                                if stash && ui.button("Put in the stash").clicked() {
                                    out.push(GameCmd::Stash(id));
                                    ui.close();
                                }
                                if ui.button("Drop").clicked() {
                                    out.push(GameCmd::Drop(id));
                                    ui.close();
                                }
                            });
                            resp.on_hover_ui(|ui| {
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        item_tooltip(ui, d, it, None);
                                        compare(ui, d, it, worn_for(inv, d, it));
                                    });
                                    if let Some(w) = worn_for(inv, d, it) {
                                        ui.separator();
                                        ui.vertical(|ui| item_tooltip(ui, d, w, Some("Wearing")));
                                    }
                                });
                            });
                        }
                        if (i + 1) % 8 == 0 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Sort").clicked() {
                        out.push(GameCmd::Sort);
                    }
                    ui.label("Auto-pickup:");
                    let names = ["everything", "magic +", "rare +", "unique", "off"];
                    let mut sel = inv.auto_loot.min(4) as usize;
                    egui::ComboBox::from_id_salt("autoloot").selected_text(names[sel]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            if ui.selectable_value(&mut sel, i, *n).clicked() {
                                out.push(GameCmd::AutoLoot(i as u8));
                            }
                        }
                    });
                });
                ui.label(
                    RichText::new(format!("{}   ·   green frame: an upgrade   ·   I to close", hint))
                        .small()
                        .color(Color32::from_white_alpha(120)),
                );
            });
        if !open {
            self.inventory = false;
        }
    }

    fn vendor_window(&mut self, ctx: &egui::Context, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
        let mut open = true;
        egui::Window::new("Hilda the Smith")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_CENTER, EVec2::new(16.0, -40.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("\"Steel for coin. Bring me what you don't need.\"")
                        .italics()
                        .color(Color32::from_rgb(200, 170, 130)),
                );
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
                            if resp.clicked() {
                                out.push(GameCmd::Buy(it.id));
                            }
                            resp.on_hover_ui(|ui| {
                                item_tooltip(ui, d, it, None);
                                compare(ui, d, it, worn_for(inv, d, it));
                                ui.label(
                                    RichText::new(format!("Buy for {price} gold (click)")).color(Color32::from_rgb(255, 205, 70)),
                                );
                            });
                        });
                        if (i + 1) % 6 == 0 {
                            ui.end_row();
                        }
                    }
                });
                if !inv.buyback.is_empty() {
                    ui.separator();
                    ui.label("Buy back");
                    ui.horizontal(|ui| {
                        for it in &inv.buyback {
                            let resp = cell(ui, d, Some(it), 40.0, "", false);
                            if resp.clicked() {
                                out.push(GameCmd::Buy(it.id));
                            }
                            resp.on_hover_ui(|ui| {
                                item_tooltip(ui, d, it, None);
                                ui.label(
                                    RichText::new(format!("Buy back for {} gold", it.value()))
                                        .color(Color32::from_rgb(255, 205, 70)),
                                );
                            });
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Sell all normal").clicked() {
                        out.push(GameCmd::SellAll(0));
                    }
                    if ui.button("Sell all normal and magic").clicked() {
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
        egui::Window::new(format!("Stash ({}/{})", inv.stash.len(), pav_core::arpg::hero::STASH_SIZE))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_CENTER, EVec2::new(16.0, -40.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("stash").spacing(EVec2::splat(4.0)).show(ui, |ui| {
                        for i in 0..pav_core::arpg::hero::STASH_SIZE {
                            let it = inv.stash.get(i);
                            let resp = cell(ui, d, it, 40.0, "", false);
                            if let Some(it) = it {
                                if resp.clicked() {
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
                ui.label(RichText::new("Click: take into the bag").small().color(Color32::from_white_alpha(120)));
            });
        if !open {
            self.panel = None;
        }
    }

    fn exhibit_window(&mut self, ctx: &egui::Context, g: &GameFrame, out: &mut Vec<GameCmd>) {
        let Some(i) = self.panel_spot else { return };
        let Some(s) = g.spots.get(i) else { return };
        let mut open = true;
        egui::Window::new(&s.name)
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
                    if ui.button("Release it (fight)").clicked() {
                        out.push(GameCmd::Release(i as u32));
                        self.panel = None;
                    }
                    if ui.button("New creatures").clicked() {
                        out.push(GameCmd::Reroll);
                        self.panel = None;
                    }
                });
            });
        if !open {
            self.panel = None;
        }
    }

    fn portal_window(&mut self, ctx: &egui::Context, g: &GameFrame, out: &mut Vec<GameCmd>) {
        let mut open = true;
        let deepest = g.inv.as_ref().map(|i| i.max_depth).unwrap_or(0).max(1);
        let d = data();
        egui::Window::new("Portal")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, EVec2::ZERO)
            .show(ctx, |ui| {
                ui.label(RichText::new("Where to?").strong());
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
                ui.label(RichText::new("Waypoints: the descent").strong());
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for n in (1..=deepest).rev() {
                        let plan = pav_core::arpg::world::plan(&d, n);
                        let p = Place::Level(n);
                        let here = p == g.place;
                        let mut text = RichText::new(format!("{} · {}", plan.label(), plan.name)).size(15.0);
                        if plan.endless {
                            text = text.color(Color32::from_rgb(200, 170, 255));
                        }
                        let b = ui.add_enabled(!here, egui::Button::new(text).min_size(EVec2::new(300.0, 26.0)));
                        let b = b.on_hover_ui(|ui| {
                            ui.label(RichText::new(&plan.about).italics());
                            let mech: Vec<&str> = plan.mechanics.iter().map(|m| m.name()).collect();
                            ui.label(format!("Mechanics: {}", mech.join(", ")));
                            ui.label(format!("Monster level {}", plan.monster_level));
                            if let Some(b) = &plan.boss {
                                ui.label(RichText::new(format!("Boss: {b}")).color(Color32::from_rgb(255, 150, 80)));
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

fn character_window(ctx: &egui::Context, open: &mut bool, inv: &InvView, g: &GameFrame) {
    let s = &inv.sheet;
    egui::Window::new("Character")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::LEFT_TOP, EVec2::new(16.0, 60.0))
        .show(ctx, |ui| {
            if let Some(h) = &g.hero {
                ui.label(RichText::new(format!("{}  ·  Level {}", h.name, h.level)).size(17.0).strong());
            }
            let w = &inv.weapon;
            egui::Grid::new("sheet").num_columns(2).striped(true).show(ui, |ui| {
                let mut row = |k: &str, v: String| {
                    ui.label(k);
                    ui.label(RichText::new(v).color(Color32::from_rgb(235, 225, 200)));
                    ui.end_row();
                };
                row("Life", format!("{:.0}  (+{:.1}/s)", s.life_max, s.life_regen));
                row("Mana", format!("{:.0}  (+{:.1}/s)", s.mana_max, s.mana_regen));
                row("Weapon", format!("{}: {:.0}-{:.0} at {:.2}/s", w.name, w.phys[0], w.phys[1], w.aps));
                row("Attack speed", format!("{:+.0}%", (s.attack_speed - 1.0) * 100.0));
                row("Cast speed", format!("{:+.0}%", (s.cast_speed - 1.0) * 100.0));
                row("Critical strike", format!("{:.1}% × {:.0}%", w.crit * s.crit_inc + s.crit_flat, s.crit_multi * 100.0));
                row("Armour", format!("{:.0}", s.armor));
                row("Evade / Block", format!("{:.0}% / {:.0}%", s.evasion, s.block));
                row(
                    "Resist fire/cold/light/poison",
                    format!("{:.0} / {:.0} / {:.0} / {:.0}", s.res[1], s.res[2], s.res[3], s.res[4]),
                );
                row("Movement", format!("{:+.0}%", (s.move_speed - 1.0) * 100.0));
                row("Cooldowns", format!("{:+.0}% faster", (s.cooldown - 1.0) * 100.0));
                row("Area", format!("{:+.0}%", (s.area - 1.0) * 100.0));
                row("Item rarity / gold", format!("{:+.0}% / {:+.0}%", s.item_rarity, s.gold_find));
                row("Experience", format!("{:+.0}%", (s.xp_gain - 1.0) * 100.0));
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
                (Stat::DamageMore, "MORE"),
            ]
            .iter()
            .filter(|(st, _)| m.get(*st).abs() > 0.05)
            .map(|(st, n)| format!("{:+.0}% {n}", m.get(*st)))
            .collect();
            if !incs.is_empty() {
                ui.separator();
                ui.label(RichText::new("Damage").strong());
                ui.label(incs.join("   "));
            }
            if !inv.powers.is_empty() {
                ui.separator();
                ui.label(RichText::new("Powers").strong());
                for p in &inv.powers {
                    ui.label(RichText::new(p.describe()).color(Color32::from_rgb(255, 150, 60)));
                }
            }
        });
}

fn skills_window(ctx: &egui::Context, open: &mut bool, d: &Data, inv: &InvView, out: &mut Vec<GameCmd>) {
    egui::Window::new("Skills")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_BOTTOM, EVec2::new(0.0, -140.0))
        .show(ctx, |ui| {
            ui.label("Choose what each button does.");
            let keys = ["Left mouse", "Right mouse", "Q", "E", "R", "F"];
            let skills = d.hero_skills();
            egui::Grid::new("bar").num_columns(2).show(ui, |ui| {
                for (slot, key) in keys.iter().enumerate() {
                    ui.label(*key);
                    let cur = inv.bar[slot].clone();
                    let cur_name = d.skill_id(&cur).map(|i| d.skill(i).name.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt(("bar", slot)).selected_text(cur_name).width(200.0).show_ui(ui, |ui| {
                        for id in &skills {
                            let s = d.skill(*id);
                            let locked = s.unlock > inv.level;
                            let label = if locked { format!("{} (level {})", s.name, s.unlock) } else { s.name.clone() };
                            let r = ui.add_enabled(!locked, egui::Button::selectable(s.key == cur, label));
                            if r.clicked() {
                                out.push(GameCmd::Bar(slot as u8, *id));
                            }
                            r.on_hover_text(&s.about);
                        }
                    });
                    ui.end_row();
                }
            });
            if !inv.tweaks.is_empty() {
                ui.separator();
                ui.label(RichText::new("Upgrades from the passive tree").strong());
                for t in &inv.tweaks {
                    let name = d.skill_id(&t.skill).map(|i| d.skill(i).name.clone()).unwrap_or_default();
                    ui.label(RichText::new(t.describe(&name)).color(Color32::from_rgb(140, 200, 255)));
                }
            }
        });
}

/// Names over items on the ground (click to pick up) and over usable spots.
fn labels(ctx: &egui::Context, g: &GameFrame, proj: &Projector, out: &mut Vec<GameCmd>) {
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
        let near = g.near == Some(i);
        let c = if near { Color32::from_rgb(255, 225, 150) } else { Color32::from_rgb(200, 190, 170) };
        p.text(
            at + EVec2::new(1.0, 1.0),
            Align2::CENTER_CENTER,
            &s.name,
            FontId::proportional(15.0),
            Color32::from_black_alpha(200),
        );
        p.text(at, Align2::CENTER_CENTER, &s.name, FontId::proportional(15.0), c);
        if near {
            let what = match s.kind {
                SpotKind::Vendor => "trade",
                SpotKind::Stash => "open the stash",
                SpotKind::Portal => "travel",
                SpotKind::Exhibit => "examine",
                SpotKind::Exit => "descend",
                SpotKind::Chest => "break the seal (keepers will come)",
            };
            p.text(
                at + EVec2::new(0.0, 18.0),
                Align2::CENTER_CENTER,
                format!("G: {what}"),
                FontId::proportional(12.0),
                Color32::from_white_alpha(220),
            );
        }
    }
}
