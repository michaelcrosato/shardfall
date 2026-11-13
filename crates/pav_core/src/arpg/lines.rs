//! Shardfall's line book (AI Hub Languages v1): `game/lines.toml` (menus, HUD, help, messages)
//! plus lines made from the game data, so a name or description written in `game/*.toml` is
//! translatable under a stable id without being written twice.
//!
//! Data ids: `skill.<key>.name|about`, `hero.<key>.name|title|about`, `monster.<key>.name`,
//! `item.<key>.name`, `item_kind.<kind>`, `unique.<key>.name|lore`,
//! `boss.<key>.name|title|lore|say.<i>`, `theme.<key>.name|place`, `level.<n>.name|about`,
//! `tree.<key>.name|lore` (notables, keystones, skill upgrades and the start),
//! `tree.sector.<key>.name`, `tree.mastery.<key>.name|<i>`, `stat.<key>` and `stat.<key>.less`,
//! `affix.<key>.<i>`, `monster_affix.<key>.name`, `genome.<part>.<i>`, `rare_name.first|<slot>.<word>`
//! and `rare_monster.first|second|epithet.<i>`.
//!
//! The helpers here give data names in the language in use, in reading order: they draw as
//! they are (a name has no numbers to lay out) and fill other lines' blanks.

use crate::lines::{Arg, Book, logical, or_original_logical};

use super::boss::BossDef;
use super::combat::Rarity;
use super::data::{CharacterDef, Data, Element, FamilyDef, SkillDef, data, source};
use super::items::{BaseDef, EquipSlot, Item, Slot, UniqueDef};
use super::stats::Stat;
use super::tree::{Node, NodeKind, Tree};
use super::world::{LevelPlan, Mechanic};

/// The id piece for a data key: lowercase letters, digits and `_`.
pub fn key_id(key: &str) -> String {
    let mut out: String = key.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "_".into() } else { out }
}

/// The negative wording of a stat template (`10% reduced`), or None when it has no word to
/// swap and only the number's sign changes.
pub(crate) fn less(template: &str) -> Option<String> {
    let swaps = [("increased", "reduced"), ("reduced", "increased"), ("more", "less"), ("faster", "slower")];
    for (from, to) in swaps {
        if template.contains(from) {
            return Some(template.replacen(from, to, 1));
        }
    }
    template.starts_with("+{}").then(|| template.replacen("+{}", "-{}", 1))
}

/// The whole book: the hand-written file, then the lines made from data.
pub fn book() -> Book {
    let text = source("lines").unwrap_or_default();
    let mut b: Book = toml::from_str(&text).unwrap_or_else(|e| {
        log::error!("game/lines.toml: {e}");
        Book { language: "en".into(), ..Default::default() }
    });
    b.version = 1;
    let d = data();
    for s in &d.skills {
        let k = key_id(&s.key);
        b.add(format!("skill.{k}.name"), &s.name, Some("A skill's name, on the skill bar and in tooltips."));
        b.add(format!("skill.{k}.about"), &s.about, Some("What the skill does, in its tooltip."));
    }
    for c in &d.characters {
        let k = key_id(&c.key);
        b.add(format!("hero.{k}.name"), &c.name, Some("A playable character's name."));
        b.add(format!("hero.{k}.title"), &c.title, Some("The character's epithet, after their name (hero.full_name)."));
        b.add(format!("hero.{k}.about"), &c.about, Some("Who the character is, in the Hall of Heroes."));
    }
    for f in &d.families {
        b.add(format!("monster.{}.name", key_id(&f.key)), &f.name, Some("A kind of monster."));
    }
    for base in &d.bases {
        b.add(format!("item.{}.name", key_id(&base.key)), &base.name, Some("An item's base type."));
        if matches!(base.slot, Slot::Weapon | Slot::Offhand) {
            b.add(
                format!("item_kind.{}", key_id(&base.kind)),
                cap(&base.kind),
                Some("A kind of weapon or off-hand, in an item's tooltip after its rarity (item.kind)."),
            );
        }
    }
    for a in &d.affixes {
        for (i, n) in a.names.iter().enumerate() {
            b.add(
                format!("affix.{}.{i}", key_id(&a.key)),
                n,
                Some(
                    "A word in a magic item's name: a prefix before the base, or a suffix after it (item.name.magic joins them).",
                ),
            );
        }
    }
    for w in super::items::RARE_FIRST {
        b.add(
            format!("rare_name.first.{}", key_id(w)),
            *w,
            Some("The first word of a rare item's own name (item.name.rare): grim and evocative, like a war-name."),
        );
    }
    for slot in Slot::ALL {
        for w in super::items::rare_second(slot) {
            b.add(
                format!("rare_name.{}.{}", slot.key(), key_id(w)),
                *w,
                Some("The second word of a rare item's own name (item.name.rare), fitting the kind of item."),
            );
        }
    }
    for u in &d.uniques {
        let k = key_id(&u.key);
        b.add(format!("unique.{k}.name"), &u.name, Some("A unique item's name."));
        b.add(format!("unique.{k}.lore"), &u.lore, Some("A unique item's flavour text, in italics in its tooltip."));
    }
    for boss in &d.bosses {
        let k = key_id(&boss.key);
        b.add(format!("boss.{k}.name"), &boss.name, Some("A boss's name."));
        b.add(format!("boss.{k}.title"), &boss.title, Some("A boss's epithet."));
        b.add(format!("boss.{k}.lore"), &boss.lore, Some("A boss's story, on its banner."));
        for (i, p) in boss.phases.iter().enumerate() {
            b.add(format!("boss.{k}.say.{i}"), &p.say, Some("What the boss shouts as a fight's phase begins (boss.phase)."));
        }
    }
    for (key, t) in &d.themes {
        b.add(format!("theme.{}.name", key_id(key)), &t.name, Some("A level's look and place, such as a frozen cavern."));
        b.add(
            format!("theme.{}.place", key_id(key)),
            t.name.rsplit(' ').next().unwrap_or(""),
            Some("The place word of the theme's name (Halls, Caverns), in generated depths' names (level.endless.name)."),
        );
    }
    for (i, l) in d.levels.iter().enumerate() {
        b.add(format!("level.{}.name", i + 1), &l.name, Some("A designed level's name, named for the mechanic it introduces."));
        b.add(format!("level.{}.about", i + 1), &l.about, Some("The level's mechanic, on its banner."));
    }
    // The tree: nodes with names of their own; the rest are named for their sector.
    for n in d
        .tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Start | NodeKind::Notable | NodeKind::Keystone | NodeKind::Skill))
    {
        let k = key_id(&n.key);
        let note = match n.kind {
            NodeKind::Skill => "A passive tree node that upgrades a skill: the skill's name, then the upgrade's.",
            NodeKind::Keystone => "A keystone of the passive tree: a big node that bends the rules.",
            _ => "A passive skill tree node.",
        };
        b.add(format!("tree.{k}.name"), &n.name, Some(note));
        b.add(format!("tree.{k}.lore"), &n.lore, Some("The node's flavour text."));
    }
    for s in &d.tree.sectors {
        b.add(
            format!("tree.sector.{}.name", key_id(&s.key)),
            &s.name,
            Some("A sector of the passive tree (a theme such as Might or Arcana), and the name of its small nodes."),
        );
    }
    for (key, m) in &d.tree.masteries {
        let k = key_id(key);
        b.add(
            format!("tree.mastery.{k}.name"),
            &m.name,
            Some("A mastery of the passive tree: a node with options to choose from."),
        );
        for (i, (name, _, _)) in m.options.iter().enumerate() {
            b.add(format!("tree.mastery.{k}.{i}"), name, Some("A mastery option's name; its effects follow it."));
        }
    }
    for m in &d.monster_affixes {
        b.add(format!("monster_affix.{}.name", key_id(&m.key)), &m.name, Some("A word before a magic or rare monster's name."));
    }
    for (part, names) in &d.genome.names {
        for (i, n) in names.iter().enumerate() {
            b.add(
                format!("genome.{}.{i}", key_id(part)),
                n,
                Some("A piece of a generated monster's name; monster.name.generated joins the pieces."),
            );
        }
    }
    for (group, words, note) in [
        (
            "first",
            super::genome::RARE_FIRST,
            "The start of a rare monster's own name, joined to the second part (monster.name.rare_given).",
        ),
        (
            "second",
            super::genome::RARE_SECOND,
            "The end of a rare monster's own name, joined to the first part (monster.name.rare_given).",
        ),
        ("epithet", super::genome::RARE_EPITHET, "A rare monster's epithet, after its name (monster.name.rare)."),
    ] {
        for (i, w) in words.iter().enumerate() {
            b.add(format!("rare_monster.{group}.{i}"), *w, Some(note));
        }
    }
    for s in Stat::ALL {
        let t = s.template();
        let k = key_id(s.key());
        b.add(
            format!("stat.{k}"),
            t.replace("{}", "{value}"),
            Some("A modifier on an item, node or monster. {value} is a number."),
        );
        if let Some(neg) = less(t) {
            b.add(format!("stat.{k}.less"), neg.replace("{}", "{value}"), Some("The same modifier when it is negative."));
        }
    }
    b
}

/// Makes Shardfall's book the engine's line source.
pub fn install() {
    crate::lines::set_source(book);
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// ------------------------------------------------------------------------- data names

/// A line made from data: its translation (or a test language's), else the data's own text,
/// in reading order. Empty text stays empty.
pub fn data_line(id: &str, original: &str) -> String {
    if original.trim().is_empty() || super::data::loading() {
        return original.to_string();
    }
    or_original_logical(id, original)
}

/// A line in reading order with its blanks filled: a piece of another line. (While the data
/// loads, the book can't be made yet: the blanks' values, for the loader's checks.)
pub fn piece(id: &str, args: &[(&str, Arg)]) -> String {
    if super::data::loading() {
        let values: Vec<String> = args
            .iter()
            .map(|(_, a)| match a {
                Arg::Int(i) => i.to_string(),
                Arg::Num(f) => f.to_string(),
                Arg::Text(t) => t.clone(),
            })
            .collect();
        return values.join(" ");
    }
    logical(id, args)
}

pub fn skill_name(s: &SkillDef) -> String {
    data_line(&format!("skill.{}.name", key_id(&s.key)), &s.name)
}

pub fn skill_about(s: &SkillDef) -> String {
    data_line(&format!("skill.{}.about", key_id(&s.key)), &s.about)
}

/// A skill's name by key (the key itself when there is no such skill).
pub fn skill_name_of(d: &Data, key: &str) -> String {
    d.skill_id(key).map(|i| skill_name(d.skill(i))).unwrap_or_else(|| key.to_string())
}

pub fn hero_name(c: &CharacterDef) -> String {
    data_line(&format!("hero.{}.name", key_id(&c.key)), &c.name)
}

pub fn hero_title(c: &CharacterDef) -> String {
    data_line(&format!("hero.{}.title", key_id(&c.key)), &c.title)
}

pub fn hero_about(c: &CharacterDef) -> String {
    data_line(&format!("hero.{}.about", key_id(&c.key)), &c.about)
}

/// "Kestrel the Brawler".
pub fn hero_full_name(c: &CharacterDef) -> String {
    if c.title.is_empty() {
        return hero_name(c);
    }
    piece("hero.full_name", &[("name", hero_name(c).into()), ("title", hero_title(c).into())])
}

pub fn family_name(f: &FamilyDef) -> String {
    data_line(&format!("monster.{}.name", key_id(&f.key)), &f.name)
}

pub fn monster_affix_name(key: &str, name: &str) -> String {
    data_line(&format!("monster_affix.{}.name", key_id(key)), name)
}

/// A word of a generated monster's name: `part`'s `i`-th (game/genome.toml `[names]`).
pub fn genome_word(part: &str, i: usize, word: &str) -> String {
    data_line(&format!("genome.{}.{i}", key_id(part)), word)
}

pub fn base_name(b: &BaseDef) -> String {
    data_line(&format!("item.{}.name", key_id(&b.key)), &b.name)
}

pub fn unique_name(u: &UniqueDef) -> String {
    data_line(&format!("unique.{}.name", key_id(&u.key)), &u.name)
}

pub fn unique_lore(u: &UniqueDef) -> String {
    data_line(&format!("unique.{}.lore", key_id(&u.key)), &u.lore)
}

/// A magic item's name word for an affix's tier.
pub fn affix_word(d: &Data, key: &str, tier: u8) -> Option<String> {
    let a = d.affixes.iter().find(|a| a.key == key)?;
    let i = (tier as usize).min(a.names.len().checked_sub(1)?);
    Some(data_line(&format!("affix.{}.{i}", key_id(key)), &a.names[i]))
}

/// An item's name in the language in use, made from its parts: a unique's own name, a magic
/// item's affix words round its base, a rare item's two words, else the base's name.
pub fn item_name(d: &Data, it: &Item) -> String {
    if let Some(u) = it.unique_def(d) {
        return unique_name(u);
    }
    let Some(b) = it.base_def(d) else { return data_line("", &it.name) };
    let base = base_name(b);
    match it.rarity {
        Rarity::Magic => {
            let word = |prefix: bool| {
                it.mods.iter().find(|m| m.prefix == prefix && !m.affix.is_empty()).and_then(|m| affix_word(d, &m.affix, m.tier))
            };
            match (word(true), word(false)) {
                (Some(p), Some(s)) => {
                    piece("item.name.magic", &[("prefix", p.into()), ("base", base.into()), ("suffix", s.into())])
                }
                (Some(p), None) => piece("item.name.magic_prefix", &[("prefix", p.into()), ("base", base.into())]),
                (None, Some(s)) => piece("item.name.magic_suffix", &[("base", base.into()), ("suffix", s.into())]),
                (None, None) => base,
            }
        }
        Rarity::Rare => rare_item_name(b.slot, &it.name),
        _ => base,
    }
}

/// A rare item's own name ("Doom Bite"): its two words, each translated.
pub fn rare_item_name(slot: Slot, name: &str) -> String {
    let parts =
        name.split_once(' ').filter(|(a, b)| super::items::RARE_FIRST.contains(a) && super::items::rare_second(slot).contains(b));
    match parts {
        Some((a, b)) => piece(
            "item.name.rare",
            &[
                ("first", data_line(&format!("rare_name.first.{}", key_id(a)), a).into()),
                ("second", data_line(&format!("rare_name.{}.{}", slot.key(), key_id(b)), b).into()),
            ],
        ),
        None => data_line("", name),
    }
}

pub fn slot_name(s: Slot) -> String {
    piece(&format!("slot.{}", s.key()), &[])
}

pub fn equip_slot_name(s: EquipSlot) -> String {
    match s {
        EquipSlot::Ring1 | EquipSlot::Ring2 => piece(&format!("slot.{}", s.key()), &[]),
        s => slot_name(s.slot()),
    }
}

pub fn rarity_name(r: Rarity) -> String {
    piece(&format!("rarity.{}", ["normal", "magic", "rare", "unique"][r as usize]), &[])
}

pub fn element_name(e: Element) -> String {
    piece(&format!("element.{}", e.name().to_lowercase()), &[])
}

/// A boss's name: a designed boss's line; a grown boss's name is made in its language.
pub fn boss_name(b: &BossDef) -> String {
    if b.key.starts_with("gen:") { b.name.clone() } else { data_line(&format!("boss.{}.name", key_id(&b.key)), &b.name) }
}

pub fn boss_title(b: &BossDef) -> String {
    if b.key.starts_with("gen:") { b.title.clone() } else { data_line(&format!("boss.{}.title", key_id(&b.key)), &b.title) }
}

pub fn boss_lore(b: &BossDef) -> String {
    if b.key.starts_with("gen:") { b.lore.clone() } else { data_line(&format!("boss.{}.lore", key_id(&b.key)), &b.lore) }
}

/// What a boss shouts as its `i`-th phase begins.
pub fn boss_say(b: &BossDef, i: usize, say: &str) -> String {
    if b.key.starts_with("gen:") { say.to_string() } else { data_line(&format!("boss.{}.say.{i}", key_id(&b.key)), say) }
}

/// A boss's name by key (a designed key or "gen:<seed>").
pub fn boss_name_of(d: &Data, key: &str, level: u32) -> String {
    super::boss::boss_def(d, key, level).map(|b| boss_name(&b)).unwrap_or_else(|| key.to_string())
}

pub fn theme_name(key: &str, name: &str) -> String {
    data_line(&format!("theme.{}.name", key_id(key)), name)
}

/// A designed level's name and its line about the mechanic.
pub fn level_name(depth: u32, name: &str) -> String {
    data_line(&format!("level.{depth}.name"), name)
}

pub fn level_about(depth: u32, about: &str) -> String {
    data_line(&format!("level.{depth}.about"), about)
}

/// What the player sees of a level mechanic: its name, a one-line hint, the adjective in
/// generated depths' names.
pub fn mechanic_name(m: Mechanic) -> String {
    piece(&format!("mechanic.{}.name", m.key()), &[])
}

pub fn mechanic_hint(m: Mechanic) -> String {
    piece(&format!("mechanic.{}.hint", m.key()), &[])
}

pub fn mechanic_adjective(m: Mechanic) -> String {
    piece(&format!("mechanic.{}.adjective", m.key()), &[])
}

/// A level's name: a designed level's line, or a generated depth's made from its mechanics
/// and its theme's place ("The Howling Molten Caverns").
pub fn plan_name(p: &LevelPlan) -> String {
    if !p.endless {
        return level_name(p.depth, &p.name);
    }
    let adj: Vec<String> = p.mechanics.iter().take(2).map(|m| mechanic_adjective(*m)).collect();
    // A blended theme's key is "<a>+<b>@<hue>": its name and place are a's.
    let key = p.theme.key.split('+').next().unwrap_or_default();
    let place = data_line(&format!("theme.{}.place", key_id(key)), p.theme.name.rsplit(' ').next().unwrap_or("Depths"));
    match adj.as_slice() {
        [a, b] => piece("level.endless.name", &[("first", a.into()), ("second", b.into()), ("place", place.into())]),
        _ => data_line("", &p.name),
    }
}

/// A level's line about its mechanics (on its banner and the portal's list).
pub fn plan_about(p: &LevelPlan) -> String {
    if !p.endless {
        return level_about(p.depth, &p.about);
    }
    let names: Vec<String> = p.mechanics.iter().map(|m| mechanic_name(*m)).collect();
    piece("level.endless.about", &[("mechanics", list_and(&names).into())])
}

/// "a, b and c" in the language in use.
pub fn list_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => {
            let mut list = rest[0].clone();
            for next in &rest[1..] {
                list = piece("list.comma", &[("list", list.into()), ("next", next.into())]);
            }
            piece("list.and", &[("list", list.into()), ("last", last.into())])
        }
    }
}

/// "a, b, c" in the language in use.
pub fn list_comma(items: &[String]) -> String {
    let mut it = items.iter();
    let mut list = it.next().cloned().unwrap_or_default();
    for next in it {
        list = piece("list.comma", &[("list", list.into()), ("next", next.into())]);
    }
    list
}

/// The hero's name: their playable character's, in the language in use.
pub fn hero_display_name(d: &Data, h: &super::hero::Hero) -> String {
    d.character(h.character_key(d)).map(hero_name).unwrap_or_else(|| h.name.clone())
}

/// A passive tree sector's name (by its index in the tree).
pub fn sector_name(t: &Tree, i: usize) -> String {
    t.sectors.get(i).map(|s| data_line(&format!("tree.sector.{}.name", key_id(&s.key)), &s.name)).unwrap_or_default()
}

pub fn mastery_name(key: &str, name: &str) -> String {
    data_line(&format!("tree.mastery.{}.name", key_id(key)), name)
}

pub fn mastery_option(key: &str, i: usize, name: &str) -> String {
    data_line(&format!("tree.mastery.{}.{i}", key_id(key)), name)
}

/// A passive tree node's name: its own line, or made from its sector's name.
pub fn tree_node_name(t: &Tree, n: &Node) -> String {
    let sector = |key: &str| t.sectors.iter().position(|s| s.key == key).map(|i| sector_name(t, i));
    match n.kind {
        NodeKind::Mastery => mastery_name(&n.mastery, &n.name),
        NodeKind::Astral => {
            let id = if n.lore == "star" { "tree.name.astral_star" } else { "tree.name.astral" };
            piece(id, &[("sector", sector_name(t, n.sector as usize).into())])
        }
        NodeKind::Minor => {
            let parts: Vec<&str> = n.key.split('.').collect();
            match parts.as_slice() {
                // The middle of a bridge: both sectors.
                ["bridge", a, b, _, "1"] => match (sector(a), sector(b)) {
                    (Some(a), Some(b)) => piece("tree.name.bridge", &[("first", a.into()), ("second", b.into())]),
                    _ => data_line("", &n.name),
                },
                _ if t.sectors.get(n.sector as usize).is_some_and(|s| s.name == n.name) => sector_name(t, n.sector as usize),
                _ => data_line("", &n.name),
            }
        }
        _ => data_line(&format!("tree.{}.name", key_id(&n.key)), &n.name),
    }
}

/// A passive tree node's flavour text (empty when it has none).
pub fn tree_node_lore(n: &Node) -> String {
    if n.lore == "star" {
        return String::new();
    }
    data_line(&format!("tree.{}.lore", key_id(&n.key)), &n.lore)
}

/// A spot's name: a line id ("spot.smith") shows the line; others carry their name as made.
pub fn spot_name(name: &str) -> String {
    if crate::lines::valid_id(name) { piece(name, &[]) } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn book_is_valid() {
        let b = super::book();
        assert_eq!(b.language, "en");
        assert!(b.lines.len() > 300, "{} lines", b.lines.len());
        for (id, line) in &b.lines {
            assert!(crate::lines::valid_id(id), "{id}");
            let texts: Vec<&String> = match line {
                crate::lines::Line::Text(t) => vec![t],
                crate::lines::Line::Plural(m) => {
                    assert!(m.contains_key("one") && m.contains_key("other"), "{id}: one and other");
                    m.values().collect()
                }
            };
            for t in texts {
                assert_eq!(t.matches('{').count(), t.matches('}').count(), "{id}: unbalanced braces");
                assert!(!t.contains('<'), "{id}: no HTML");
            }
        }
        for id in b.notes.keys().chain(b.max.keys()) {
            assert!(b.lines.contains_key(id), "note or max for a missing line: {id}");
        }
        assert!(!b.brief.trim().is_empty());
    }

    /// Every line id the game's code names (a string literal like "hud.gold" in the game's
    /// sources, and the ids it makes from keys) is in the book: a missing one would show its id.
    #[test]
    fn every_line_the_code_names_is_in_the_book() {
        use super::super::items::{EquipSlot, Slot};
        use super::super::mechanics::Boon;
        use super::super::world::Mechanic;
        let b = super::book();
        let mut ids: Vec<String> = Vec::new();
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dirs = ["pav_core/src/arpg", "pav_ui/src", "pav_app/src"];
        let groups = [
            "hud",
            "float",
            "msg",
            "error",
            "npc",
            "say",
            "item",
            "rarity",
            "element",
            "slot",
            "doll",
            "compare",
            "hint",
            "ui",
            "inventory",
            "character",
            "skills",
            "tree",
            "vendor",
            "stash",
            "shop",
            "gamble",
            "alchemist",
            "portal",
            "hero",
            "hall",
            "exhibit",
            "lab",
            "monster",
            "boss",
            "power",
            "tweak",
            "difficulty",
            "touch",
            "controls",
            "place",
            "spot",
            "feature",
            "level",
            "mechanic",
            "shrine",
            "list",
            "menu",
            "title",
            "tools",
            "plaza",
            "dev",
        ];
        for dir in dirs {
            for entry in std::fs::read_dir(crates.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_none_or(|e| e != "rs")
                    || (dir == "pav_app/src" && !["input.rs", "ui.rs", "app.rs"].iter().any(|f| path.ends_with(f)))
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for lit in text.split('"').skip(1).step_by(2) {
                    let group = lit.split('.').next().unwrap_or("");
                    if lit.contains('.')
                        && !lit.ends_with(".toml")
                        && !lit.ends_with(".rs")
                        && crate::lines::valid_id(lit)
                        && groups.contains(&group)
                    {
                        ids.push(lit.to_string());
                    }
                }
            }
        }
        for m in Mechanic::ALL {
            for part in ["name", "hint", "adjective"] {
                ids.push(format!("mechanic.{}.{part}", m.key()));
            }
        }
        ids.extend(Boon::ALL.iter().map(|b| format!("shrine.{}", b.name().to_lowercase())));
        ids.extend(Slot::ALL.iter().map(|s| format!("slot.{}", s.key())));
        ids.extend(EquipSlot::ALL.iter().filter(|s| s.slot() != Slot::Ring).map(|s| format!("doll.{}", s.key())));
        ids.extend(["slot.ring1", "slot.ring2", "doll.ring"].map(String::from));
        ids.extend(["normal", "magic", "rare", "unique"].map(|r| format!("rarity.{r}")));
        ids.extend(super::super::data::Element::ALL.iter().map(|e| format!("element.{}", e.name().to_lowercase())));
        ids.extend(["everything", "magic", "rare", "unique", "off"].map(|n| format!("inventory.auto_pickup.{n}")));
        for k in ["all", "physical", "fire", "cold", "lightning", "poison", "elemental", "attack", "spell", "melee", "more"] {
            ids.push(format!("character.damage.{k}"));
        }
        for brew in ["flask", "brew"] {
            ids.extend(["buy", "maxed"].map(|w| format!("alchemist.{brew}.{w}")));
        }
        let missing: Vec<&String> = ids.iter().filter(|id| !b.lines.contains_key(*id)).collect();
        assert!(missing.is_empty(), "lines named in code but not in game/lines.toml: {missing:?}");
        assert!(ids.len() > 400, "{} ids found", ids.len());
    }

    #[test]
    fn names_are_made_from_their_parts() {
        use super::*;
        let d = data();
        // Every tree node has a name in the book's language, and in English it is the one the
        // tree was built with.
        for n in &d.tree.nodes {
            assert_eq!(tree_node_name(&d.tree, n), n.name, "{}", n.key);
        }
        let mut rng = crate::rng::Rng::new(3);
        for i in 0..300 {
            let spec = super::super::items::RollSpec { level: 1 + i % 80, rarity_bonus: 300.0, ..Default::default() };
            let it = super::super::items::roll_item(&d, &mut rng, spec, i).unwrap();
            assert_eq!(item_name(&d, &it), it.name, "{it:?}");
        }
    }
}
