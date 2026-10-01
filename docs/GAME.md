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
  from gear, vendor and stash, weapon visuals.
- **G3 Builds:** passive tree (data + layout + UI), all 16 skills, masteries, keystones,
  respec.
- **G4 Monsters:** genome, parts rendering, palettes, archetype brains, monster affixes, boss
  phases, creature lab.
- **G5 World:** town hub with animated NPCs, layout generator and themes, twelve mechanics,
  designed levels 1-12, endless Depths, waypoints, saving.
- **G6 Showcase:** juice pass, the agent tools, bot balance pass, browser build, docs and the
  final build.
