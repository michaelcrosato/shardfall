# Shardfall — the showcase game

A fast hack-and-slash built on the Pavilion engine. Run-based: from the town you take a
portal into a level, clear it, find the exit, and go deeper. Twelve designed levels each
introduce one mechanic (and are named after it); after that the Depths generate levels forever
by combining mechanics, monster archetypes, bosses and colour palettes.

It is also the reference for agents building games on this engine: every system is data plus
small generators, and every system has an agent tool to build and inspect it.

## Pillars
1. **Fast and fluid.** Move with WASD, aim with the mouse, attack on press, cancel
   recovery into a dodge, and get hit-stop, shake, flash and knockback on every hit.
2. **Deep builds.** A large passive tree (hundreds of nodes, keystones, skill masteries and an
   endless repeatable outer ring), 16 active skills, and items with rolled affixes and uniques
   whose powers change how skills behave.
3. **Infinite modularity.** Monsters are genomes (body plan + parts + palette + archetype
   brain + element + affixes). Bosses are genomes plus phase scripts. Items are base + affixes
   scaled by item level. Levels are a layout grammar (rooms, corridors, arenas) plus a theme
   plus mechanics. Each part is authored data first, then rolled procedurally from the same
   vocabulary.
4. **Engine showcase.** It uses procedural rigs and animation, dynamic lights (spells, braziers,
   darkness), particles, bloom, distortion, physics (loot bounces, barrels roll, debris),
   rewind, the room/layout builder, synth audio, the agent tools and the live bridge.
5. **Bypassable mechanics.** A casual player can ignore any level mechanic and still clear the
   level. Speedrunners and farmers can exploit it: kegs chain through packs, currents carry you
   past rooms, portals skip routes, and darkness doubles drops.

## Architecture
- `pav_core::arpg` holds the game rules, inside the simulation (`SimState::game`), so
  snapshots, rewind, replays, the agent tools and the live bridge all work on the game.
  - `stats` (stat ids, stat blocks, aggregation)
  - `combat` (actors, damage, ailments, hit-stop)
  - `skills` (active skills as data and behaviours)
  - `hero` (profile: level, XP, gold, items, tree, skill bar; saved as JSON)
  - `items` and `loot` (bases, affixes, uniques, drops)
  - `tree` (passive tree data and layout)
  - `genome` (monster generator, parts, palettes, names), `brain` (archetypes), `boss`
  - `levelgen` (layout grammar, themes, endless levels), `mechanics` (the twelve), `town`
- `game/` (repo root) holds the data: skills, item bases and affixes, uniques, tree clusters,
  monster families, palettes, themes and the designed levels. It is embedded in the
  executable.
- `pav_view::arpg` draws the game: weapons in hand, skill effects, telegraphs, ground loot
  with rarity beams, damage numbers and monster auras.
- `pav_app::arpg_ui` is the game UI: life and mana orbs, skill bar, XP, inventory, tooltips,
  passive tree, vendor, stash, map, death screen, waypoints and the difficulty sliders.
- `pav_tools` adds the game tools: `hero`, `give`, `monster`, `turntable`, `animsheet`,
  `loot_roll`, `tree_info`, `levelgen`, `levelmap`, `duel` and `autoplay`.

## Milestones
- **G1 Combat core:** arena, hero combo, dodge and first skills, three monster archetypes with
  telegraphs, damage, death, XP and gold, HUD, hit feel, difficulty sliders.
- **G2 Loot:** items, affixes, rarity, drops, pickup, inventory and equipment, tooltips, stats
  from gear, vendor and stash, weapon visuals. *(done: see "Items" below)*
- **G3 Builds:** passive tree (data + layout + UI), all 16 skills, masteries, keystones,
  respec. *(done: see "Builds" below)*
- **G4 Monsters:** genome, parts rendering, palettes, archetype brains, monster affixes, boss
  phases, creature lab. *(done: see "Monsters" below)*
- **G5 World:** town hub with animated NPCs, layout generator and themes, twelve mechanics,
  designed levels 1-12, endless Depths, waypoints, saving. *(done: see "World" below)*
- **G6 Showcase:** juice pass, the agent tools, bot balance pass, browser build, docs and the
  final build. *(done)*

## Items (G2)
- **Bases** (`game/items.toml`, 114): ten weapon kinds and nine slots, six tiers each by item
  level (1, 10, 22, 38, 55, 75). Numbers grow with item level by `base_scale` (x45 at 75, and on
  forever), so a tier is a look and a bigger implicit, never a dead end.
- **Affixes** (`game/affixes.toml`, 74): prefix/suffix, slot lists (slot names, weapon kinds or
  `armor`), tiers `[ilvl, min, max]`; past the last tier values grow by `grow` per level
  (percent stats cap with `grow = 0`). `local` affixes change the item itself (weapon damage,
  attack speed, crit; armour). Magic: 1-2 affixes, rare: 3-6 (max 3 per side).
- **Uniques** (`game/uniques.toml`, 19): fixed stats scaled to the drop level and a **power**.
  Powers (`arpg/powers.rs`, 16 kinds: corpse burst, frost crits, blood magic, execute, fire
  trail, orbiting blades, echo strike, dodge reset, storm call, block nova, ignite aura, stand
  firm, meteor slam, frenzy, mana shield, quickening) belong to actors, not items, so passive
  keystones (G3) and monster affixes (G4) reuse them. Power hits use internal `power_*` skills
  in skills.toml.
- **Drops**: item chance per kill by monster rarity (bosses shower), rarity weights 70/24/5.5/0.5
  shifted by item rarity and monster rank, slot shares fixed regardless of base counts. Items
  pop out and bounce (simple ballistic flight, ground found by ray), gold piles fly to the hero.
  Auto-pickup by rarity filter; labels on the ground are clickable.
- **Commands**: every menu action is a `GameCmd` carried in `InputFrame::cmd` (queued one per
  tick by the app), so trading and equipping record, replay and rewind exactly.
- **Places**: `Place::Town` (Emberwatch: Hilda the smith sells and buys, the stash, a portal)
  and `Place::Arena` (the Proving Grounds). Travel rebuilds the scene inside the same tick and
  random stream, carrying the `Game` (hero, bags, settings).
- **Looks**: the hero's puppet wears the gear: weapon kind/colour/glow, shield or focus, helmet
  shapes (cap, helm, great helm, crown, horned, halo), shoulder plates, cape, gloves, buckle.

## Builds (G3)
- **Skills**: 16 for the hero, from quick attacks to channelled spins, war cries, meteors and
  blizzards; every behaviour is data (`behavior` in skills.toml), and monsters can use any skill.
- **Tweaks** change one skill (more projectiles, bigger area, an element...). They come from the
  tree's skill branches today and can come from items and monster affixes next.
- **Passive tree**: six sectors - Might, Fury, Precision, Arcana, Elements, Bulwark - each a
  road with notables, three wheels around masteries (pick one of four options), up to three
  skill branches and a keystone; bridges between neighbours carry hybrid stats and six more
  keystones (Echoing Blades, Wind Dancer, Glass Edge, Avatar of Storm, Pyre, Avatar of Flame).
  Beyond: the Astral rings, endless, stronger ring by ring. One point per level; refund and
  respec cost gold.

## Monsters (G4)
- **Genome**: body plan × proportions × parts × palette (element) × archetype (brain, skill
  pools, stat shape) → a named creature, the same from the same seed. Five body plans, ten
  part kinds that fit any plan, five elements, nine archetypes (brute, stalker, spitter,
  charger, caster, swarm, bomber, summoner, tank).
- **Affixes** make magic and rare monsters (Fast, Molten, Frost-Touched, Multishot,
  Teleporting, Warlord, Brood, Enraging...); rares get names of their own.
- **Bosses** grow from genome seeds with forced looks and scripted phases; beyond the five
  designed ones they are generated (summon a brood, change tactics, enrage).
- **The Menagerie** shows twenty creatures at a time with their genome cards; let any of them
  out to fight it, or grow a new set.

## World (G5)
- **Emberwatch** (the town): Hilda the smith (vendor), Odo the gambler (a sealed box per slot:
  magic 60%, rare 22%, unique 3%), Mother Wren the alchemist (up to 6 potions, stronger brews),
  Captain Brannoc by the portal, villagers on their rounds and Biscuit the dog. The portal
  lists every depth you've reached.
- **The descent**: each level is rooms and corridors generated from a seed, painted by a theme,
  with one signature mechanic and earlier ones mixed in. Find the way down; on boss levels it
  is sealed until the boss falls (and the first time pays a passive point).

| # | Level | Mechanic | Also | Boss |
|---|---|---|---|---|
| 1 | The Shrines | shrines: walk through for a boon | | |
| 2 | The Powder Keg | kegs: hit one near a pack | | |
| 3 | The Gauntlet | spike plates on a beat | shrines | |
| 4 | The Rift Gates | paired gates, shortcuts | kegs | The Hollow King |
| 5 | The Windways | wind carries everyone | spikes | |
| 6 | The Totem Fields | totems ward monsters | shrines, kegs | |
| 7 | The Molten Floor | lava burns whoever stands in it | wind | Cinder Wyrm |
| 8 | The Frozen Lake | ice: slide; frozen foes shatter | totems, spikes | |
| 9 | The Crumbling Halls | the floor falls behind you | gates, kegs | |
| 10 | The Lightless Deep | darkness; light the wells | shrines, lava | Mother of Swarms |
| 11 | The Cursed Vaults | chests call keepers, then pay | wind, totems | |
| 12 | The Time Rift | bubbles of slow time | crumbling, ice | Frostbound Colossus |

- **The Depths** (13 onward, forever): a theme turned around the colour wheel and lit by
  another, two to four mechanics at once, monsters grown with the depth's favoured
  archetypes, a boss every third depth. "The Howling Molten Caverns" is always Depth N; its
  rooms are new every visit.
- **Mechanics are optional**: every level can be cleared by fighting. Exploits: chain kegs
  through packs, ride the wind, kite monsters into lava or over spikes, freeze them on ice and
  shatter them, let them fall through the floor you broke, light wells mid-fight, fight inside
  a time bubble, take the gate shortcut straight to the exit.
