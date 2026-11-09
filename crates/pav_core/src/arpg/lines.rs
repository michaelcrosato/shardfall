//! Shardfall's line book (AI Hub Languages v1): `game/lines.toml` (menus, HUD, help, messages)
//! plus lines made from the game data, so a name or description written in `game/*.toml` is
//! translatable under a stable id without being written twice.
//!
//! Data ids: `skill.<key>.name|about`, `hero.<key>.name|title|about`, `monster.<key>.name`,
//! `item.<key>.name`, `unique.<key>.name|lore`, `boss.<key>.name|title|lore`, `theme.<key>.name`,
//! `level.<n>.name|about`, `tree.<key>.name|lore`, `stat.<key>` and `stat.<key>.less`,
//! `affix.<key>.<i>`, `monster_affix.<key>.name`, `genome.<part>.<i>`.

use crate::lines::Book;

use super::data::{data, source};
use super::stats::Stat;

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
fn less(template: &str) -> Option<String> {
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
        b.add(format!("hero.{k}.title"), &c.title, Some("The character's epithet, under their name."));
        b.add(format!("hero.{k}.about"), &c.about, Some("Who the character is, in the Hall of Heroes."));
    }
    for f in &d.families {
        b.add(format!("monster.{}.name", key_id(&f.key)), &f.name, Some("A kind of monster."));
    }
    for base in &d.bases {
        b.add(format!("item.{}.name", key_id(&base.key)), &base.name, Some("An item's base type."));
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
    }
    for (key, t) in &d.themes {
        b.add(format!("theme.{}.name", key_id(key)), &t.name, Some("A level's look and place, such as a frozen cavern."));
    }
    for (i, l) in d.levels.iter().enumerate() {
        b.add(format!("level.{}.name", i + 1), &l.name, Some("A designed level's name, named for the mechanic it introduces."));
        b.add(format!("level.{}.about", i + 1), &l.about, Some("The level's mechanic, on its banner."));
    }
    for n in &d.tree.nodes {
        let k = key_id(&n.key);
        b.add(format!("tree.{k}.name"), &n.name, Some("A passive skill tree node."));
        b.add(format!("tree.{k}.lore"), &n.lore, Some("The node's flavour text."));
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
}
