//! Game commands: everything the player does through menus (equip, sell, stash, travel...).
//! They ride in the input frame (`InputFrame::cmd`), one per tick, so replays, rewind and the
//! agent tools see them exactly like button presses.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::Rarity;
use super::data::data;
use super::hero::STASH_SIZE;
use super::items::{EquipSlot, RollSpec, Slot, roll_item};
use super::{Game, feet_of, flat, refresh_hero};
use crate::frame::SimEvent;
use crate::sim::Sim;

/// A menu action. Item ids are `Item::id`; slots are `EquipSlot` indices.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameCmd {
    /// Pick up a ground item (clicked label).
    Pickup(u32),
    /// Wear a bag item (in its usual slot, or a given one).
    Equip(u32),
    EquipTo(u32, u8),
    Unequip(u8),
    /// Throw a bag item on the ground.
    Drop(u32),
    /// Vendor: sell a bag item, buy a vendor item, sell every bag item up to a rarity.
    Sell(u32),
    Buy(u32),
    SellAll(u8),
    /// Stash: put in, take out.
    Stash(u32),
    Take(u32),
    /// Skill bar slot <- skill id.
    Bar(u8, u16),
    /// Go somewhere (`Place` code).
    Travel(u32),
    /// Auto-loot items at least this rare (0 normal .. 3 unique, 4 = off).
    AutoLoot(u8),
    /// Sort the bag (slot, then rarity).
    Sort,
    /// Passive tree: take a node, give one back (costs gold), reset everything (costs more),
    /// pick a mastery's option.
    Allocate(u32),
    Refund(u32),
    Respec,
    Mastery(u32, u8),
    /// The Menagerie: let an exhibit out to fight (spot index); new creatures for every pedestal.
    Release(u32),
    Reroll,
    /// Use a spot in the world (spot index): take the way down, open a cursed chest.
    Use(u32),
    /// The gambler: a mystery item of a slot (`Slot` index) for gold.
    Gamble(u8),
    /// The alchemist: 0 = one more potion, 1 = stronger potions.
    Brew(u8),
    /// The Hall of Heroes (town): play as a character (index in game/heroes.toml order).
    Character(u8),
}

/// Where the hero can be. Codes: 0 town, 1 arena, 2 the Menagerie, 100 + n depth n.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    #[default]
    Town,
    Arena,
    /// The Menagerie: the creature lab.
    Lab,
    /// A level of the descent: 1..=12 designed, then the endless Depths.
    Level(u32),
}

impl Place {
    pub fn code(self) -> u32 {
        match self {
            Place::Town => 0,
            Place::Arena => 1,
            Place::Lab => 2,
            Place::Level(n) => 100 + n,
        }
    }
    pub fn from_code(c: u32) -> Option<Place> {
        match c {
            0 => Some(Place::Town),
            1 => Some(Place::Arena),
            2 => Some(Place::Lab),
            n if n > 100 => Some(Place::Level(n - 100)),
            _ => None,
        }
    }
    pub fn name(self) -> String {
        match self {
            Place::Town => "Emberwatch".into(),
            Place::Arena => "The Proving Grounds".into(),
            Place::Lab => "The Menagerie".into(),
            Place::Level(n) => {
                let p = super::world::plan(&data(), n);
                format!("{} · {}", p.label(), p.name)
            }
        }
    }
    /// The scene name for this place.
    pub fn scene(self) -> String {
        match self {
            Place::Town => "town".into(),
            Place::Arena => "arena".into(),
            Place::Lab => "lab".into(),
            Place::Level(n) => format!("level/{n}"),
        }
    }
}

/// Something in the world the hero can use (walk up and press interact).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotKind {
    Vendor,
    Stash,
    Portal,
    /// A creature on a pedestal (the Menagerie).
    Exhibit,
    /// The way down to the next depth.
    Exit,
    /// A cursed chest (levels).
    Chest,
    /// Odo the gambler and Mother Wren the alchemist (town).
    Gamble,
    Alchemist,
    /// A playable character on their pedestal in the Hall of Heroes (town).
    Hero,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spot {
    pub kind: SpotKind,
    pub name: String,
    pub pos: Vec3,
    /// How close the hero must be to use it.
    pub reach: f32,
    /// Details shown when used (exhibits: the genome card).
    #[serde(default)]
    pub info: Vec<String>,
}

/// Vendors pay an item's value and charge four times it.
pub fn buy_price(it: &super::items::Item) -> u64 {
    it.value() * 4
}

impl Game {
    fn notify(&mut self, sim: &Sim, text: impl Into<String>) {
        let at = self.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0).unwrap_or_default();
        self.float_text(at + Vec3::Y * 2.3, text);
    }

    /// The nearest usable spot within its reach of the hero.
    pub fn near_spot(&self, sim: &Sim) -> Option<usize> {
        let h = self.hero_id.and_then(|h| feet_of(sim, h))?.0;
        self.spots
            .iter()
            .enumerate()
            .map(|(i, s)| (i, flat(s.pos - h).length(), s.reach))
            .filter(|x| x.1 <= x.2)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|x| x.0)
    }

    fn near_kind(&self, sim: &Sim, k: SpotKind) -> bool {
        self.near_spot(sim).is_some_and(|i| self.spots[i].kind == k)
    }

    /// Carries out a command; complaints float over the hero.
    pub(crate) fn command(&mut self, sim: &mut Sim, cmd: GameCmd, events: &mut Vec<SimEvent>) {
        if let Err(e) = self.try_command(sim, cmd, events) {
            self.notify(sim, e);
        }
    }

    fn try_command(&mut self, sim: &mut Sim, cmd: GameCmd, events: &mut Vec<SimEvent>) -> Result<(), String> {
        let d = data();
        let mut gear = false;
        match cmd {
            GameCmd::Pickup(id) => self.pickup(sim, id, events)?,
            GameCmd::Equip(id) => {
                self.hero.equip(&d, id, None)?;
                gear = true;
            }
            GameCmd::EquipTo(id, slot) => {
                let s = EquipSlot::from_index(slot as usize).ok_or("no such slot")?;
                self.hero.equip(&d, id, Some(s))?;
                gear = true;
            }
            GameCmd::Unequip(slot) => {
                let s = EquipSlot::from_index(slot as usize).ok_or("no such slot")?;
                self.hero.unequip(s)?;
                gear = true;
            }
            GameCmd::Drop(id) => {
                let i = self.hero.inventory.iter().position(|it| it.id == id).ok_or("not in the bag")?;
                let it = self.hero.inventory.remove(i);
                let at = self.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0).unwrap_or(self.spawn);
                self.drop_item(sim, it, at, events);
            }
            GameCmd::Sell(id) => {
                if !self.near_kind(sim, SpotKind::Vendor) {
                    return Err("Find a vendor to sell".into());
                }
                let i = self.hero.inventory.iter().position(|it| it.id == id).ok_or("not in the bag")?;
                let it = self.hero.inventory.remove(i);
                self.hero.gold += it.value();
                events.push(SimEvent::Coin { pos: self.spots.first().map(|s| s.pos).unwrap_or_default() });
                // The vendor keeps the last few sold items for buy-back.
                self.buyback.push(it);
                if self.buyback.len() > 6 {
                    self.buyback.remove(0);
                }
            }
            GameCmd::SellAll(max) => {
                if !self.near_kind(sim, SpotKind::Vendor) {
                    return Err("Find a vendor to sell".into());
                }
                let max = [Rarity::Normal, Rarity::Magic, Rarity::Rare, Rarity::Unique][(max as usize).min(3)];
                let (sell, keep): (Vec<_>, Vec<_>) =
                    std::mem::take(&mut self.hero.inventory).into_iter().partition(|it| it.rarity <= max);
                self.hero.inventory = keep;
                let gold: u64 = sell.iter().map(|it| it.value()).sum();
                if gold > 0 {
                    self.hero.gold += gold;
                    let at = self.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0).unwrap_or_default();
                    self.float(at + Vec3::Y * 2.0, gold as f32, super::FloatKind::Gold);
                    events.push(SimEvent::Coin { pos: at });
                }
            }
            GameCmd::Buy(id) => {
                if !self.near_kind(sim, SpotKind::Vendor) {
                    return Err("Find a vendor to buy".into());
                }
                if self.hero.bag_full() {
                    return Err("Inventory full".into());
                }
                let (list, i) = match self.vendor.iter().position(|it| it.id == id) {
                    Some(i) => (0, i),
                    None => (1, self.buyback.iter().position(|it| it.id == id).ok_or("sold out")?),
                };
                let price = if list == 0 { buy_price(&self.vendor[i]) } else { self.buyback[i].value() };
                if self.hero.gold < price {
                    return Err(format!("Not enough gold ({price})"));
                }
                self.hero.gold -= price;
                let it = if list == 0 { self.vendor.remove(i) } else { self.buyback.remove(i) };
                self.hero.inventory.push(it);
            }
            GameCmd::Stash(id) => {
                if !self.near_kind(sim, SpotKind::Stash) {
                    return Err("Find the stash".into());
                }
                if self.hero.stash.len() >= STASH_SIZE {
                    return Err("Stash full".into());
                }
                let i = self.hero.inventory.iter().position(|it| it.id == id).ok_or("not in the bag")?;
                let it = self.hero.inventory.remove(i);
                self.hero.stash.push(it);
            }
            GameCmd::Take(id) => {
                if !self.near_kind(sim, SpotKind::Stash) {
                    return Err("Find the stash".into());
                }
                if self.hero.bag_full() {
                    return Err("Inventory full".into());
                }
                let i = self.hero.stash.iter().position(|it| it.id == id).ok_or("not in the stash")?;
                let it = self.hero.stash.remove(i);
                self.hero.inventory.push(it);
            }
            GameCmd::Bar(slot, skill) => {
                let slot = slot as usize;
                if slot >= self.hero.bar.len() || skill as usize >= d.skills.len() {
                    return Err("no such slot or skill".into());
                }
                let def = d.skill(skill);
                if def.monster || def.power {
                    return Err(format!("{} is not a hero skill", def.name));
                }
                if def.unlock > self.hero.level {
                    return Err(format!("{} unlocks at level {}", def.name, def.unlock));
                }
                // Swapping: if the skill is already on the bar, the slots trade places.
                if let Some(other) = self.hero.bar.iter().position(|k| *k == def.key) {
                    self.hero.bar.swap(slot, other);
                } else {
                    self.hero.bar[slot] = def.key.clone();
                }
            }
            GameCmd::Travel(code) => {
                let place = Place::from_code(code).ok_or("unknown destination")?;
                if let Place::Level(n) = place {
                    if n > self.hero.max_depth.max(1) {
                        return Err(format!("{} is not reached yet", place.name()));
                    }
                }
                self.travel = Some(place.code());
            }
            GameCmd::Gamble(slot) => {
                if !self.near_kind(sim, SpotKind::Gamble) {
                    return Err("Odo is not here".into());
                }
                let slot = *Slot::ALL.get(slot as usize).ok_or("no such slot")?;
                let price = super::scene::gamble_price(self.hero.level, slot);
                if self.hero.gold < price {
                    return Err(format!("{price} gold needed"));
                }
                if self.hero.inventory.len() >= super::hero::INVENTORY_SIZE {
                    return Err("Your bag is full".into());
                }
                let d = data();
                let rng = &mut sim.state.rng;
                let roll = rng.f32();
                let rarity = if roll < 0.03 {
                    Rarity::Unique
                } else if roll < 0.25 {
                    Rarity::Rare
                } else if roll < 0.85 {
                    Rarity::Magic
                } else {
                    Rarity::Normal
                };
                let level = self.hero.level + rng.below(3);
                let id = self.hero.new_id();
                let spec = RollSpec { level, rarity: Some(rarity), slot: Some(slot), rarity_bonus: 0.0 };
                let item = roll_item(&d, &mut sim.state.rng, spec, id)
                    .or_else(|| roll_item(&d, &mut sim.state.rng, RollSpec { rarity: Some(Rarity::Rare), ..spec }, id))
                    .ok_or("Odo shrugs: nothing in the box")?;
                self.hero.gold -= price;
                let name = item.name.clone();
                let r = item.rarity;
                self.hero.inventory.push(item);
                self.inv_changed();
                self.notify(sim, format!("Odo: \"{}{}\"", name, if r >= Rarity::Rare { "! Lucky you." } else { "." }));
                if let Some((f, _)) = self.hero_id.and_then(|h| feet_of(sim, h)) {
                    events.push(SimEvent::Loot { pos: f, rarity: r as u8 });
                }
            }
            GameCmd::Brew(kind) => {
                if !self.near_kind(sim, SpotKind::Alchemist) {
                    return Err("Mother Wren is not here".into());
                }
                let price = super::scene::brew_price(&self.hero, kind).ok_or("That brew is as strong as it gets")?;
                if self.hero.gold < price {
                    return Err(format!("{price} gold needed"));
                }
                self.hero.gold -= price;
                match kind {
                    0 => {
                        self.hero.potion_max += 1;
                        self.hero.potions = self.hero.potion_max;
                        self.notify(sim, format!("Mother Wren: \"{} potions now. Use them.\"", self.hero.potion_max));
                    }
                    _ => {
                        self.hero.mods.add(super::stats::Stat::PotionInc, 20.0);
                        self.notify(sim, "Mother Wren: \"Stronger stuff. Don't drink it all at once.\"");
                    }
                }
                refresh_hero(sim, self, false);
                self.inv_changed();
            }
            GameCmd::Use(i) => {
                let s = self.spots.get(i as usize).ok_or("nothing there")?.clone();
                let hid = self.hero_id.ok_or("no hero")?;
                let (feet, _) = feet_of(sim, hid).ok_or("no hero")?;
                if flat(feet - s.pos).length() > s.reach + 1.0 {
                    return Err(format!("{} is too far away", s.name));
                }
                match s.kind {
                    SpotKind::Exit => super::mechanics::use_exit(self)?,
                    SpotKind::Chest => super::mechanics::open_chest(self, i as usize)?,
                    _ => return Err("nothing to do there".into()),
                }
            }
            GameCmd::AutoLoot(r) => {
                self.auto_loot = match r {
                    0 => Rarity::Normal,
                    1 => Rarity::Magic,
                    2 => Rarity::Rare,
                    3 => Rarity::Unique,
                    // Nothing is rarer than unique: an unreachable filter turns auto-loot off.
                    _ => Rarity::Unique,
                };
                self.auto_loot_off = r >= 4;
            }
            GameCmd::Allocate(id) => {
                let t = &d.tree;
                if self.hero.points() == 0 {
                    return Err("No passive points left".into());
                }
                if !t.can_allocate(&self.hero.tree, id) {
                    return Err("Not connected to your tree".into());
                }
                self.hero.tree.insert(id);
                gear = true;
            }
            GameCmd::Refund(id) => {
                if !d.tree.can_refund(&self.hero.tree, id) {
                    return Err("That would cut off other nodes".into());
                }
                let cost = self.hero.refund_cost();
                if self.hero.gold < cost {
                    return Err(format!("Refunding costs {cost} gold"));
                }
                self.hero.gold -= cost;
                self.hero.tree.remove(&id);
                self.hero.masteries.remove(&id);
                gear = true;
            }
            GameCmd::Respec => {
                let cost = self.hero.respec_cost();
                if self.hero.gold < cost {
                    return Err(format!("A full reset costs {cost} gold"));
                }
                self.hero.gold -= cost;
                self.hero.tree.clear();
                self.hero.masteries.clear();
                gear = true;
            }
            GameCmd::Mastery(id, option) => {
                let t = &d.tree;
                let n = t.node(id).ok_or("no such node")?;
                if !self.hero.tree.contains(&id) || n.mastery.is_empty() {
                    return Err("Allocate the mastery first".into());
                }
                let m = t.masteries.get(&n.mastery).ok_or("unknown mastery")?;
                if option as usize >= m.options.len() {
                    return Err("no such option".into());
                }
                // Each option once per mastery family.
                let taken = self
                    .hero
                    .masteries
                    .iter()
                    .any(|(nid, o)| *nid != id && *o == option && t.node(*nid).is_some_and(|x| x.mastery == n.mastery));
                if taken {
                    return Err(format!("{} is already chosen elsewhere", m.options[option as usize].0));
                }
                self.hero.masteries.insert(id, option);
                gear = true;
            }
            GameCmd::Character(i) => {
                if self.place != Place::Town {
                    return Err("Heroes wait in the Hall of Heroes, in Emberwatch".into());
                }
                let key = d.characters.get(i as usize).map(|c| c.key.clone()).ok_or("No such hero")?;
                super::hall::switch(self, sim, &key)?;
            }
            GameCmd::Release(i) => {
                if self.place != Place::Lab {
                    return Err("Only in the Menagerie".into());
                }
                super::scene::release_exhibit(self, sim, i as usize)?;
            }
            GameCmd::Reroll => {
                if self.place != Place::Lab {
                    return Err("Only in the Menagerie".into());
                }
                super::scene::stock_lab(self, sim, true);
            }
            GameCmd::Sort => {
                let key =
                    |it: &super::items::Item| (it.slot(&d), std::cmp::Reverse(it.rarity), std::cmp::Reverse(it.level), it.id);
                self.hero.inventory.sort_by_key(key);
            }
        }
        self.inv_changed();
        if gear {
            refresh_hero(sim, self, false);
        }
        Ok(())
    }

    /// New vendor wares at the hero's level.
    pub(crate) fn restock(&mut self, sim: &mut Sim) {
        let d = data();
        self.vendor.clear();
        let level = self.hero.level;
        let plan: [(Option<Slot>, Rarity); 12] = [
            (Some(Slot::Weapon), Rarity::Magic),
            (Some(Slot::Weapon), Rarity::Rare),
            (Some(Slot::Body), Rarity::Magic),
            (Some(Slot::Helmet), Rarity::Magic),
            (Some(Slot::Gloves), Rarity::Magic),
            (Some(Slot::Boots), Rarity::Magic),
            (Some(Slot::Offhand), Rarity::Magic),
            (Some(Slot::Belt), Rarity::Magic),
            (Some(Slot::Ring), Rarity::Magic),
            (Some(Slot::Amulet), Rarity::Magic),
            (None, Rarity::Rare),
            (None, Rarity::Rare),
        ];
        for (slot, rarity) in plan {
            let id = self.hero.new_id();
            if let Some(it) =
                roll_item(&d, &mut sim.state.rng, RollSpec { level, rarity: Some(rarity), slot, rarity_bonus: 0.0 }, id)
            {
                self.vendor.push(it);
            }
        }
        self.inv_changed();
    }
}
