# History

The full journal, newest first: what was built, how it works, and the decisions made along the
way. Search it when you need the background on something; don't read it whole. The current
state is `docs/PROGRESS.md`. The engine milestones (M1–M10) are in the middle of this file and
Shardfall's (G1–G6) at the end.

## Asset plan 6.1: the scale-out closes with the pilot — 2026-10-13

Row 6.1 of `docs/ASSET_SYSTEM_PLAN.md` (I8), in slim form. Its exit, "the GO packs on disk,
searchable from `assets find=`", already holds after 5.3, so the row adds no code.
- **Where the GO packs stand.** 5.3's lean G5 passed two packs, and both were imported whole:
  - Kenney Mini Dungeon: 28 of 30 models; the 2 rigged characters are skipped.
  - Quaternius Medieval Village: 33 of 34 props.

  They sit under `assets/props/lib/` (288 KB of the 40 MB budget) with `PACK.json`, an index and
  their credits in `assets/props/CREDITS.md`. They are kept out of the web upload
  (`.vercelignore`) and shipped in the studio zip by `scripts/package-studio.py --packs`.
  `assets tier=disk` lists their 61 assets, and `assets find=barrel tier=disk` returns both
  packs' barrels.
- **Not scaled further.** The other four pilot packs miss only G5's editability bar, so they stay
  local. No test game and no Shardfall row has asked for more models (`docs/FEEDBACK.md`).
  Any pinned pack rebuilds on this machine with `asset_import get=` and `from=`
  (`docs/ASSET_IMPORT.md`). A pack joins `lib/` when it passes the G5 checks.
- **Seen while checking:** `assets find=` needs every word to match, with no stemming.
  - "wooden table" finds nothing: no tag says "wooden", and `LIB/core/table` says "wood".
  - "table" finds them: 7 on this machine, local packs included.
  - Imported assets carry only their name, category and two tags. If a game's searches come up
    short, the fix is descriptions in the catalog lines, or stemming in `find=`.

## Asset plan 4.3: levels dressed with library props — 2026-10-13

Row 4.3 of `docs/ASSET_SYSTEM_PLAN.md` (SF-1 static bake, SF-2 data dressing; lean G4). Shardfall's
rooms now get 3-6 LIB/core props each, from data and in the theme's colours, instead of the six
hard-coded kinds (rocks, crates, columns, crystals, bones and mushrooms as raw blocks and spheres).
- **Baked props** (`statics.rs`): `StaticProp {asset, pos, rot, scale, seed, solid, collider}` in
  `StaticChunk.props`, with the colours for the theme roles in `StaticChunk.palette`. A baked prop
  has no entity. Its collider is one box round its solid parts, made on `add_prop` and on waking
  and removed on sleep, and `NavGrid::build` marks the same box. The view's region cache draws
  the parts as a placed copy's (`emit_prim`, `Surfaces`) into a scratch scene it keeps, sends
  their generated meshes with every scene and makes their part lights each frame (they flicker).
  Each prop has its own outline group and material seeds, hashed from its region and index.
  `state_hash` adds the prop count. A snapshot writes each prop as one array (~72 B) naming its
  asset. Props name embedded assets only, which change only with a rebuild, so the snapshot
  carries no definitions for them and `PropDefTable` does not walk statics.
- **Dressing** (`arpg/world/dress.rs`, `game/props.toml`): the six old names are sets, so
  themes.toml is unchanged. Each set has a `place` (wall, corner, floor or wall_mount) and items
  `{asset, w, place, scale, solid}`; `data.rs` checks that every item names an embedded asset that
  compiles and that every theme's sets exist. `decorate` became `ambience()` (particles and grass,
  in place) and `dress(seed)`, which runs at the very end of `build_level_with(sim, depth, game,
  dress)`, after the exit seal, from `Rng::new(seed ^ 0xd2e5_5a11)`. It keeps clear of `taken`,
  every actor (the boss, kegs, totems), the wall lights and the wind's strips, gives a standing
  prop its footprint plus 0.5 m, and adds nothing to `taken`. Each room gets 3-6 placed props
  (up to three picks each). Hung props (`wall_mount`) skip doorways, lights and the south wall,
  which the game camera looks over. An asset with `fx` would stay a placed prop (no set uses
  one). `levelmap` draws the props (grey boxes) and counts them.
- **The sets:** rocks (rock_cluster, rubble_pile, stalagmite; corners), crates (crate_stack,
  barrel, sack, urn), columns (column, column_broken, banner on the wall), crystals and mushrooms
  (walked through, as before), bones (bone_pile, walked through; chains on the wall). Rocks,
  crates and columns are solid, as the old blocks and columns were.
- **Checks (lean G4):**
  - `tests/levels.rs`: `dressing_never_moves_mechanics_or_monsters` (depths 1-12, 13, 17 and 25,
    dressed and bare: identical features, actors with their ids and places, spots, seal and
    `state.rng`); `dressing_is_reproducible`; `dressing_snapshot_cost`; `dressing_keeps_paths`
    (the way out, and every room the bare level reaches); `baked_props_collide_sleep_and_restore`.
  - `levelmap nav=true` at depths 1, 4, 7, 10 and 17 (seed 7): the routes to the exit are the
    same length as before (145, 215, 246, 280 and 267 m, within 0.2 m).
  - `campaign scene=town seed=1 from=1 to=12` (60 game minutes): with a level-30 hero, levels
    1-10 took 17.5 game minutes before and 17.0 after (−3%). Before, it also finished level 11
    (2.9 min) and stalled at level 12's boss; after, it stalled in level 11, a re-rolled vault
    that the bot cannot clear with the dressing switched off either (seed 2 of a fresh level 11:
    stuck dressed and bare, 3.9 min before the change). At the hero's own level the bot finished
    3 levels before (4.7 min, then stuck at level 4's boss) and 11 after (levels 1-3: 3.4 min).
    The one-time re-roll moves single levels a lot either way.
  - level/6 (seed 1): 406 entities → 270, 591 blocks → 522. Its render at 960×540 on lavapipe:
    median 203 ms before, 194 ms after (noise).
  - The raw dist wasm grows by 88 KB (26,490,394 → 26,578,824 B).
  - `cargo test` for pav_core, pav_view and pav_tools (`reply_budget` fails on this base, as on
    main before 3.4's fix), clippy, fmt and `cargo check -p pav_app --target
    wasm32-unknown-unknown` pass.
  - The Level 1 review page: Level 1 and Depth 14 (ruins blended), 5 before/after pairs at the
    game camera, seed 7 (`.cache/g4/manifest.json`).
- **The snapshot gate, not met as written.** The bake itself (records and palettes) is 3.8-9.1 KB
  a level, inside the plan's 16 KB, but each solid prop's collider adds ~580 B (rapier writes
  every field of a collider), so the dressing costs 13-37 KB of snapshot at the 15 tested depths.
  A compound per region would cost more (~700 B plus ~280 B a box; a 20-part crate stack's own
  compound ~6 KB). Making props walk-through would meet it, at the cost of rocks, crates and
  columns that were solid before. The old hard-coded dressing cost 75-80 KB a level, so a level's
  snapshot still shrinks (level/1 669 → 608 KB, level/6 1,311 → 1,249 KB at seed 3). The test
  checks what holds: records ≤ 80 B a prop, the bake ≤ 16 KB, ≤ 600 B per collider, nothing else.
- **Decisions:** one box per prop rather than the compound of its parts (above; the paths match
  it). No theme kits (SF-3 deferred) and no `params` on items (LIB-4 deferred). A one-time re-roll
  of mechanics and monsters at the switch (D14): the old dressing drew from the level's stream.
- **Review and landing.**
  - Under the user's rule (AGENTS.md, "Testing budget"), the orchestrator judged the 5
    before/after pairs. The banners hang on the walls, the glowing mushrooms, broken columns,
    rocks and bones read in theme colours, and no prop blocks a door or a path.
  - The snapshot gate is accepted as built. Its purpose, a small snapshot, holds: a level's
    snapshot shrinks.
  - Landed on top of 3.4: `dress` takes the theme's colours from `ThemeDef::palette` (3.4's), and
    the region cache keys on both the level palette and the materials generation.

## Asset plan 5.3: catalogs, the licence gate, the import pilot (lean G5: two packs GO) — 2026-10-12

Row 5.3 (I5 + I6) of `docs/ASSET_SYSTEM_PLAN.md`. Imported CC0 packs are now library packs that
`assets find=` ranks; the pilot decided which are committed.
- **Licence gate and tiers** (`import/licence.rs`, `asset_import::out_dir`). `from=lib/pack`
  routes by the pack's SPDX licence: CC0-1.0 and CC-BY-3.0/4.0 (`asset_library::license_allowed`)
  to `assets/props/lib/<pack>/`, with a `PACK.json` (pack, title, licence, credit, source,
  catalog; checked by `read_pack`) and every asset's `source.license` the pack's
  (`source_allowed`); share-alike, NC, ND, OGA-BY, GPL, QAL and store licences, packs whose
  catalog source says `"local": "why"`, and files outside a pinned pack to the git-ignored
  `local/<pack>/`; anything else is refused ("unknown licence X: review it, then add it to
  …licence.rs"); `core` is never an import target. A whole pack's run removes files of models it
  no longer writes, moves the pack out of the other tier (keeping its index notes), then
  reindexes it and rewrites `CREDITS.md` (`assets reindex=<pack>` does the same by hand). `get`
  also refuses a licence file that does not say the catalog's licence (`CC0`, `publicdomain/zero`
  …), and a git source is pinned by the SHA-256 of its files' sha256sum lines (it was the file
  names only).
- **Catalogs** (`assets/catalogs/{kenney,kaykit,quaternius}.json`): pins (URL or repo and commit,
  archive and licence SHA-256), placement, `label`/`credit`, then one line per model,
  `"<pack>/<id>": ["<category> <2-6 tags>", "<description>"]`, which translations write as
  `category` and `tags`; `$skip` with the reason (Mini Dungeon's two rigged characters,
  Quaternius's smoke puff); `"listed": true` translates only lined models; a line naming no model,
  or one whose first word is not a category, is an error. The 267 lines were made by a script from
  the model names (a keyword table for the category, the name's words and the pack's theme as
  tags) and read over. KayKit's ids drop the `.gltf` of `chair.gltf.glb`.
- **Sources reached from this container:** Kenney Castle, Nature and Food Kits (zips, 2-11 MB);
  KayKit Dungeon Remastered at commit `b0ca9bd` (raw files; the GitHub API and github.com are
  refused here, so the file list came from a blob-less git clone's tree); Quaternius's LowPoly
  Medieval Village Pack (Dec 2020) from OpenGameArt, 8.3 MB of OBJ (the plan's House Interior and
  Modular Dungeon are not among Quaternius's OpenGameArt uploads; the Fantasy Props MegaKit is,
  but 74 MB with trim-sheet textures). Pilot subsets are systematic, not picked by fit: Nature 50
  of 329 (colour variants folded, every 5th by name), Food 40 of 200 (every 5th), KayKit 40 of 203
  (colour variants folded, every 4th), all 34 Quaternius props. Food Kit is scaled 0.3 (its apple
  was 20 cm, its carrot 72 cm) and Quaternius 3 (its barrel 20 cm, its door 72 cm); the modular
  kits keep their own scale.
- **Pilot (lean G5, plan §10.3)**, from the ledgers `assets/catalogs/<lib>/<pack>.tsv`:

  | pack | translated | fit (≥ 80%) | median IoU (≥ 0.90) | median chars (≤ 2,500) | editable (≥ 40% of fit) | G5 | goes to |
  |---|---|---|---|---|---|---|---|
  | Kenney Mini Dungeon | 28 (+2 skipped) | 28 (100%) | 0.983 | 721 | 15 (54%) | pass | lib |
  | Kenney Castle Kit | 76 | 76 (100%) | 0.998 | 398 | 15 (20%) | editable short | local |
  | Kenney Nature Kit (50 of 329) | 50 | 50 (100%) | 0.985 | 521 | 15 (30%) | editable short | local |
  | Kenney Food Kit (40 of 200) | 40 | 39 (98%) | 0.971 | 318 | 7 (18%) | editable short | local |
  | KayKit Dungeon Remastered (40 of 203) | 40 | 40 (100%) | 0.993 | 897 | 14 (35%) | editable short | local |
  | Quaternius Medieval Village props | 33 (+1 skipped) | 31 (94%) | 0.961 | 1,186 | 15 (48%) | pass | lib |
  | **overall** | 267 (+3 skipped) | 264 (99%) | 0.983 | 521 | 81 (31%) | editable short | |

  0 panics (each model now runs under `catch_unwind`; the summary counts `panics`); every pack
  rebuilds byte for byte (two translations of each into fresh folders, identical trees). Not a
  full GO (editable 31% overall) and far from a NO-GO (99% fit, against < 60%): **the two packs
  that pass every criterion are committed** (`lib/kenney_mini_dungeon` 57 KB, 46 files;
  `lib/quaternius_medieval_village` 231 KB, 55 files: 288 KB of the 40 MB disk budget, largest
  file 18 KB); Castle, Nature, Food and KayKit stay in the local tier, each catalog source saying
  why (the whole pilot would have been 1.1 MB). Per pack 2-7 s. The GPU cross-check and the 30
  re-authorings stay deferred. Review sample: 12 compare images in `.cache/g5/` (manifest for
  `scripts/review_page.py`).
- **Why editability fails:** Kenney paints a model in bands of one texture on one connected mesh
  (castle towers and walls, most food), and organic shapes (canopies, bread, bananas) fit no
  primitive, so the asset loop keeps them as one mesh part: exact to look at (IoU ~1), cheap in
  tokens, but not editable. **The fix round** cut multicoloured pieces along their colours (each
  colour patch's connected parts fitted, flat patches as 3-8 mm slabs on their face): +17
  editable assets of 627 (199 → 216 on the four whole Kenney packs), but worst-view IoU fell to
  0.91-0.97 on those, their size rose 2-3× (cocktail 360 → 1,006 chars) and castle towers lost
  their crenellations; limited to splits costing ≤ 0.02 IoU it gave +6. No proven benefit:
  dropped, and the four packs ship local, as §10.3 says.
- **Smaller fixes and decisions.** A view in which the source and the translation both draw
  under 1% of their pixels (a zigzag sheet edge-on: Food Kit's bacon read IoU 0 and `fail`) is
  skipped like a sliver. Food's
  whisk fails at scale 0.3 (wires under the mesh part's millimetre grid). `check=` calls a
  whole-mesh `auto` translation `exact` where the translation said `fit` (5.2's rule, left). Facing
  stays per pack: KayKit's chair faces −x. The plan's `find=` over ledgers, `catalog=` rebuilds
  and per-asset placement are left out (no need shown); the tool's help is unchanged (tools/list
  stays under its cap). The library tests install each pack's mesh files, since they run outside
  the repository root the engine reads them from. New doc `docs/ASSET_IMPORT.md` (importing,
  tiers, checking one asset), linked from `ASSET_STUDIO.md`; test `tests/import_library.rs`
  (routing, `PACK.json`, credits, index, catalog tags, `local`, refusals) and the licence table's
  unit test.
- **Review fixes (Codex, PR #48).**
  - A view is skipped only when both renders are thin. A translation 20 times too big no longer
    reads as a perfect fit.
  - Loose imports into `local/import/` keep each other: the stale-file cleanup is for whole
    catalog packs only.
  - `from=` into `lib/` needs the cache that `get` fetched under the catalog's current pin.
    Pinning the hash a first `get` recorded counts as the same pin.
  - A catalog model line that is not an array is refused, not read as missing.
  - Second round: the licence counts as part of the pin (`SOURCE.json`'s checked licence and
    licence hash must match the catalog's). A CC-BY pack's licence text must name its version
    and none of ShareAlike, NonCommercial or NoDerivatives. Two catalog sources may not write one
    pack. A model that errors on a whole-pack rebuild keeps its last files. A pack that changed
    tiers moves only whole.
  - Third round:
    - A `lib/` source must pin its archive and licence-text hashes.
    - `out=` may not point into `lib/` or `templates/`.
    - The pack name `import` is reserved for loose files.
    - A rebuild that moves a pack to the other tier, or changes its licence, stops on any model
      error instead of keeping stale files.
    - A whole pack of one model takes the folder path, so its stale files are pruned.
  - Fourth round, as one gate (`libraries::verified_for_lib` plus two pack checks) run before
    anything is written into `lib/`:
    - Pins must be non-empty strings.
    - The archive's SHA-256 must have been checked by `get`; ranged reads of packs over 64 MB
      stay local.
    - A part of a pack may not change the pack's details.
    - A renamed pack is refused while its old folder holds the source.
    - `out=` is normalised before the library check, so `local/../lib` is caught.
  - Not done: loose files of one name replacing each other in `local/import/`. That is a
    re-import on this machine, and `out=` keeps them apart.
  - Not done: a translation displaced ten object widths from its source in every view could skip
    all six views. Only a hand edit makes one, and `check=`'s Chamfer numbers show it.
  - Not done: requiring descriptions on imported assets. Their names, categories and tags drive
    `assets find=`, and 61 hand-written lines are not worth it until search misses something.
  - Both committed packs rebuild byte-identically under these rules.

## Asset plan 3.4: theme materials in levels, lean G3 — 2026-10-12

Row 3.4 of `docs/ASSET_SYSTEM_PLAN.md` (MAT-6), in slim form: every dungeon theme now has
flagstone floors, dressed-stone walls, drum pillars and coping stones, drawn by the 3.2-3.3
material path in the theme's own colours.
- **Themes.** `ThemeDef` gains optional `floor_mat`, `wall_mat`, `pillar_mat`, `cap_mat`
  (`game/themes.toml`; data load refuses a name that is not a core material). The level
  builder tags its blocks: `floor()` with a material lays `floor[0]` with no checker or jitter
  (ice keeps its tiles and no material); room and corridor walls wear the wall material, the
  walls' top course and the slabs on ring and cross furniture the cap material, pillars the
  pillar material, low split walls, ring blocks and cross corner blocks the wall material.
  Nothing else changed: no RNG call, block order or geometry, so levels, packs and mechanics are
  identical. `blend_theme` keeps A's materials (its roles take the turned colours).
- **Materials** (core, cm, all role-coloured): `flagstones` (now 80 cm cobbles whose cells take
  `$floor`/`$floor2`, gap 3, cavity 0.25: the theme's two floor colours stone by stone, as the
  old checker alternated them; 2.5 units), `dungeon_wall` (70×35 cm brick courses, `$floor`
  dust up to 70 cm; 2), `pillar_stone` (45 cm drum joints on the sides, light noise; 2),
  `wall_cap` (1 m coping tiles on top faces, which match the 1 m walls on the world grid, light
  noise; 2). All within the 6-unit cap; every layer "clear" at 540p in `asset_preview
  kind=material view=game`. The first flagstones (70 cm, gap 4.5, bevel 6, cavity 0.35) read as
  busy crazy paving with thick black joints in the game: calmer joints won.
- **Palette for statics.** `Mood` gains `palette` (role → hex, from `ThemeDef::palette`), so it
  reaches the view with the level (`GameFrame.level.mood`). The static block and decor loop
  hands it to `MaterialTable::intern` as props hand theirs, so a layer's roles take the theme's
  colours (one row per material and palette: 4 rows in a level). The region cache keys on the
  palette's hash beside the materials generation. Outside levels the roles stay the defaults.
- **`view.material_detail`** (plan D7): `full | first_layer | off`. The table packs rows with one
  layer for `first_layer` and hands out slot 0 for `off` (the row stays interned); a change
  re-packs every row and bumps the instance generation, so static regions rebuild.
  `MatSlot::param` of slot 0 is 0. The app's graphics quality caps it at `first_layer` on
  Medium and Low, and Auto is Medium on touch screens: phones draw the first layer, desktops all.
- **Tools.** `level rooms=true` lists the live level's rooms (role, shape, centre, size), to
  find a pillar room; `level depth=` shows the theme's materials.
- **Checks (lean G3).** Materials are view-only: for 5 seeds (depth/seed 2/1, 5/2, 8/3, 9/4,
  14/5: powder kegs, windways, ice, crumbling floors, an endless blend) the `levelmap` PNGs and a
  REPL transcript (`level` and `entities` at load, `autoplay seconds=20`, then `level`,
  `entities`, `game`) are byte-identical to the 3.3 build's, 10 of 10 files; only the state hash
  in `load`'s reply moves (blocks carry their colour and material, `Mood` its palette).
  `campaign` (seed 1, from level 1) gives identical reports before and after, at 15 and at 60
  game minutes: levels 1-3 cleared (1 death, hero level 1 to 6), then the bot stays on level 4,
  the Rift Gates (113 kills, 7 rooms seen, the boss's seal shut), in both builds: a stall that
  predates this row. Tests of
  pav_core, pav_view, pav_tools and pav_app: 392 pass, 2 ignored (`rooms=` took `tools/list`
  to 48,042 chars, over its 48,000 cap: paid for by trimming `level`'s help, now 47,982);
  `eval_matches_its_golden` re-blessed for flagstones and the 3 new materials; new unit test
  `detail_trims_layers_or_draws_plain`, the blocks test checks a palette gets its own row, the
  quality test the phone cap. Clippy (`-D warnings`, all targets) and rustfmt clean on what this
  touches; `cargo check -p pav_app --target wasm32-unknown-unknown` passes. No shader change, so
  no bench. The 8-theme review (one capture per theme with `material_detail=off` and one with
  `full`, same seed, room and frame, 960×540 at the game camera, `.cache/g3/manifest.json` for
  `scripts/review_page.py`) were captured for review.
- **Decisions.** Caps are a role, not a colour: the walls' top course and the furniture slabs
  (pillar-coloured on rings and crosses) wear `wall_cap`, whose layers only shade, so any base
  colour works. The palette rides in `Mood` (view data the level already carries) rather than in
  each block or region. Ice floors keep their checker. Low quality also gets `first_layer`, not
  `off`: D7 names only first-layer. The Obsidian Rift's one designed depth (10) is the Lightless
  Deep, nearly black at the game camera, so its review capture is depth 17 (obsidian_rift
  architecture turned 25°, Storm Spire lights).
- **Left out.** Per-theme material variety (all 8 use the same set, tinted by their colours),
  materials on scattered rocks, crates and mechanic pieces, `theme=` on the swatch, measuring
  `first_layer` on a phone (D7's "until measured"), and the material bench (no shader change).
- **Review:** under the user's rule (AGENTS.md, "Testing budget"), the orchestrator judged the
  8 before/after captures (plain colours vs materials, same seed and camera) instead of a human
  review page; floors, walls and pillars read clearly in every theme and monsters stay distinct.
- **Also fixed:** `reply_budget` assumed the whole asset listing fits on one page. With the 28
  core assets from 4.2 it is now 33 rows, past the default page of 20, so the test failed on main
  as well. It now checks pages against the listing's `matched` count.

## Asset plan 4.2, part 2: 28 core exemplars — 2026-10-12

The exemplar half of row 4.2 (LIB-5 W0). `LIB/core` now holds 28 reviewed assets, embedded in
every build and found by `assets find=`.
- **What:**
  - The six hard-coded Shardfall kinds as library assets: `rock_cluster`, `crate_stack`,
    `column` (plus `column_broken`), `crystal_cluster`, `bone_pile` and `mushroom_cluster`. Their
    sizes match the code's, and they use the same theme roles (`$wall`, `$pillar`, `$accent`).
  - Dungeon dressing: `rubble_pile`, `stalagmite`, `altar`, `cage`, `chains`, `banner`,
    `wall_torch`.
  - Town and furniture: `barrel`, `sack`, `urn`, `brazier`, `candle_cluster`, `table`, `chair`,
    `bench`, `shelf`, `fence`, `well`, `market_stall`, `signpost`, `anvil`.
  
  All are format 2 in cm, with core or inline materials only. They have 4-68 parts and
  266-718 tokens each (median about 470).
- **How they were made:** two fresh agents, each with a 14-asset brief, used only the CLI
  (`pav repl`) against private copies of `assets/props`. They followed the documented loop:
  create, `check` to zero findings, a 4-view sheet at the game elevation with the 1.8 m figure,
  at most two fix rounds, then `asset_edit action=publish` with `if_revision` and a review. The
  two sets took 62 and about 111 tool calls (about 160k and 230k agent tokens).
- **Review:** the user looked at all 28 sheets on a review page
  (`scripts/review_page.py`, verdicts stored in the page) and accepted all 28. `review.tsv` lines
  read `ok (user review 2026-10-12); author: …`. The user then ruled that assets made this way
  need no routine human review: only edge cases or complex ones go to them (AGENTS.md, "Testing
  budget").
- **Found while authoring** (fixed or noted):
  - An outer material's glow leaked into parts that wear their own material. This is fixed in
    row 3.3: only the nearest material counts.
  - A glowing part (emissive > 0.5) adds a light that counts toward the 4-light budget.
  - `jitter`/`chance` vary seed 0 too.
  - Sheets show wall props standing on the floor; in game they hang from their `wall_mount`.
  - The `BUILTIN/crate` template has `poke` findings of its own.
  - A new core asset can be referenced only after a rebuild (it is embedded).
- **Left out:** the plan's W0 theme-kit pieces (`kit_wall`, `kit_trim`, `kit_corner`,
  `kit_door_frame`, `kit_pilaster`), since SF-3 theme kits are deferred, and side-by-side
  sheets against the old hard-coded kinds (the Level 1 before/after of row 4.3 shows them in
  game).

## Asset plan 3.3: props and statics wear materials, material tools — 2026-10-12

Row 3.3 of `docs/ASSET_SYSTEM_PLAN.md` (MAT-4, MAT-5): props name procedural materials, statics
can, and agents make and judge their own through the shared asset tools.
- **Props.** A format-2 `materials` entry is a library name (`"MAT/oak_planks"`) or a material
  written inline, read by `material::read_inline` in the asset's unit (one checker for files and
  props: inline layers lose `angle`, which no kernel draws, as files already did; the "every
  field" sink fixture changed accordingly). `mat` on a part, group or document, cycle entries and
  `remap` values may name `MAT/…` directly. The canonical writer keeps inline materials as
  written, in the schema's order. Compile resolves each key once (`Mats`): a resolved part's
  `color` is now its own only (`Option`), and `mat` is a `PartMat` (`MAT/…`, `#key` for the
  asset's inline materials, `ASSET#key` inside a reference, with the material as compiled). The
  cascade's look and emissive come from what a material's base chain writes. A name the library
  lacks still compiles (snapshots keep loading), draws magenta and is `check`'s
  `unknown_material` with a did-you-mean; a malformed `MAT/` name is a reader error.
- **Format 1 unchanged.** Its default colour was already written into the source model as the
  part's own (`Doc::from_v1`): the plan's "JSON migration". Tested: an implicit and an explicit
  `#e8704a` read to one `prop1-` revision, the templates keep their goldens, their parts carry
  colours and no material, and captures of `test`, `town` and `level/3` (640×360, 120 ticks)
  are byte-identical to the 3.2 build's.
- **View.** `ViewBuilder` syncs its `MaterialTable` with `Library::current()` each build, so a
  workshop edit or reload re-packs the rows it changed in every view (the app's and every
  session's). Parts intern library materials by name and inline ones as `ASSET#key`
  (`PropFrame` gains `asset`); round parts (cylinder, cone, capsule, tube, lathe) default to the
  axis map, others to object, and a material's own map wins (asked as `None`, so the row follows
  the library, per the 3.2 review fix cherry-picked here as 6a6f23b). Seeds are
  `seed(entity, part index)`; the instance colour is the part's own, else the material's, through
  the instance's palette; generated meshes carry `ext` from `prim.local_box()`. A plain material
  (no layers, legacy shading) uses no row. `Block`/`Decor` gain `material` (left out of
  snapshots when unset): blocks are world-mapped with seed 0, decor seeded by its position. The
  table no longer bumps `instance_generation` when it adds a row (only a re-pack that changes
  instance values does), so a static region is not rebuilt for a new material, and a re-intern
  with an equal material re-packs nothing.
- **Tools.** `kind=material` routes on `assets` (rows `name layers cost desc`), `asset_edit`
  (create, copy, inspect, patch, replace, undo, redo, schema; workshop materials
  `MAT/workshop/x` in `assets/materials/workshop/`, `PAV_MATERIALS` overrides; the props'
  lock, `if_revision`, 32-step history and atomic save; ops `{op:set,fields}`,
  `{op:set,layer:i,fields}`, `{op:add,layer}`, `{op:remove,layer:i}`, errors re-addressed to
  `ops[i]`; each edit resolves in the library it would join, so cycles and broken bases are
  refused; canonical files and `mat1-<fnv>` revisions from `material::write`) and
  `asset_preview` (a swatch: per material a sphere, cube, cylinder and a 2×2 m world-mapped
  slab, `compare=` up to 6 rows, `view=close|game`, 960×540; per layer the feature size, pixels
  at 1080p and 540p, the share of a floor and wall it visibly changes over the layers below, and
  a verdict). Sessions and the app load workshop materials before props. tools/list: 47,966
  chars, under the 48,000 cap, by trimming help text (mocap, clip_import, the asset tools'
  kind and action lines) instead of raising it.
- **Docs.** `docs/MATERIALS.md` (names, use on props, fields, the 14 patterns with defaults and
  costs, legibility, the tool loop, examples), linked from `ASSET_STUDIO.md`; AGENTS.md and the
  MCP instructions name `kind=material`.
- **Checks.** `pav_core/tests/prop_materials.rs` (library and inline materials, explicit
  colours and revisions, unknown names, one patch that adds a material and uses it, snapshot
  round trip, statics serde); view tests (words, maps, seeds, colours and `ext` on the barrel;
  blocks world-mapped with seed 0; colour-only materials draw plain); `material_tools` unit
  tests (create, a five-op patch, op errors, conflicts, dry run, undo/redo, copy, cycles) and
  `pav_tools/tests/material_preview.rs` (a headless swatch and its lines). Demo: the fixture
  `banded_barrel` (barrel_staves, wrought_iron, oak_planks) lints clean. `cargo test
  --workspace` and clippy pass; three `chunks_exact` loops in `creatures.rs` became `as_chunks`
  for the toolchain's clippy.
- **Not done** (noted in the plan): `theme=` on the swatch, a native watcher for hand-edited
  material files, and recompiling props when a workshop material's look or emissive changes
  (colour and patterns update at once). Two placed revisions of one asset share its inline rows;
  the last packed wins until the stale copies refresh.
- **Landed after rows 4.1, 4.2 (part 1) and 5.2.** Assets drawn from game code
  (`pav_view::assets::emit_asset`) wear their materials too: the view shares its material table
  (now behind a mutex) with game code for the frame, beside the whole-scene style
  (`assets::FrameScope`); outside a frame their parts draw in their colours alone. Mesh parts
  (5.2) keep an explicit white base colour under 3.3's optional part colour, and `recolor` takes a
  material key's colour from the library. The library gate (row 4.2) refuses `MAT/workshop/…`
  materials in a published asset: core ships without the workshop. Only the nearest material feeds
  a part's look and emissive (an exemplar author found a document-wide glowing material lighting a
  part that wore its own stone); explicit fields on outer levels still cascade.

## Asset plan 5.2: translation into props and the CPU fidelity metrics — 2026-10-12

Row 5.2 (I3 + I4's CPU half) of `docs/ASSET_SYSTEM_PLAN.md`, ahead of Phases 3-4 like 5.1.
- **Mesh parts in the engine** (F4's loader, moved to I3). `pav_core::props::mesh`: the mesh file
  of §5.5 (`{format:1,name,unit:"mm",tris,budget,palette,v,f,source}`, whole millimetres, ≤ 4,096
  triangles, ≤ 64 colours), parsed with every index checked and written canonically. A part
  `{"mesh":"<pack>/<name>"}` (placement, mirror, copies, `scale`, `recolor:{"0":"#hex"|material}`)
  is read by the format-2 reader (an unknown mesh is an error with the fix), compiles to
  `Primitive::Mesh`, draws as a generated mesh with per-vertex colours (`GeomMesh.colors` →
  `Vertex.color`; the part's own colour is white), collides as its hull, and keys its definition
  (`propk-` over the mesh file's FNV). Meshes are found installed (the importer installs what it
  writes) or on disk under `$PAV_ASSETS` (`lib/<pack>/meshes/`, then `local/<pack>/meshes/`).
- **I3, `import::fit`** (single-threaded per model, f64, BTreeMap order, no randomness):
  - `segment`: connected parts per (node, material) at 0.1 mm; face colours are now the 7-sample
    area mean of the texture (Radon's rule) in linear light × factors, with the sampler's wrap
    modes (5.1's review fix); each piece's colour spread and multicolour flag (§8.5 step 1).
  - `frame`: the yaw-only box in 3° then 1° steps, or the principal axes when 15% smaller, angles
    snapped (whole degrees, quarter turns within 2°).
  - `prims`: box, ellipsoid or sphere, and down each local axis a cylinder, a least-squares
    frustum or cone, a capsule, a lathe (side silhouettes' radius per row, Douglas–Peucker 2%,
    ≤ 12 points) and an extrusion (traced outline, 1 px, ≤ 24 points); when the outline is a
    regular polygon (5-16 corners), faceted cylinders, frustums and lathes (`segments`, turned to
    the corners). Score: worst of the three local silhouette IoUs at 64 px (holes filled, since
    pieces are often open shells, and views in which a flat piece is edge-on skipped) less 0.0002
    a token. Equal shapes are fitted once. Below 0.85 the piece goes into the mesh part.
  - The asset loop: while the six-view IoU is under 0.90 the primitive piece with the largest
    shape error ((1 − IoU) × area) moves into the mesh part; while ΔE is over 6, the one with the
    largest colour error (spread × area).
  - `mesh`: one mesh part per asset for every such piece, welded on whole millimetres (over budget:
    coarser cells, a fifth larger each time, each cell's corners at their mean), the distinct hex
    colours or a 64-colour Oklab median cut.
  - `write`: each primitive in the equivalent form with the fewest angles (the cube's 24 turns, a
    round shape's axis field or one-angle turns either way along its axis, an outline's quarter
    turns and turn-over, a faceted shape's corner turns); centimetres whole when within 3% of the
    extent a number sizes or places, else to 0.1; outlines from their lowest corner
    counter-clockwise; mirror pairs in x then z into `mirror`, three or more equal parts at an
    equal step into `array`; names `<colour>_<type>` (node names when a model has several), the
    colour most parts share at the root; then the engine's reader and canonical writer.
- **I4, `import::metrics` + `import::raster`** (CPU): six orthographic views at 128 px (the source
  ∪ translation window; a view where the source is under 2% as deep as wide is skipped): `iou`
  (worst) and `iou_mean`; `de` = Oklab ΔE × 100 over shared pixels; `mean_mm`/`worst_mm` = the
  symmetric Chamfer distance over 4,096 surface samples each way to the other surface (mean,
  95th percentile); tiers per §8.6 (`exact` only for `fit=mesh`; `editable` = fit with mesh ≤ 30%);
  a token estimate (o200k-like pre-tokens, ~4% over tiktoken here); the compare image (source row
  over translation row, six views at 160 px, lit) in `.cache/asset_import/compare/`. The GPU
  cross-check (`iou_gpu`) is deferred (§10.3).
- **The tool.** `from=FILE|DIR|lib/pack` translates (`fit=auto|prims|mesh`) and writes each asset
  with its mesh file in `meshes/` beside it, to `out_dir()`: `out=`, else the local tier
  `assets/props/local/<pack>/` (now git-ignored; the licence gate that routes CC0 packs to
  `lib/<pack>` is row 5.3, and row 4.1's names slot into that one function). `survey=` fits
  without writing and fills the ledger's `parts mesh_pct iou iou_mean de chars tier` (`iou_gpu`
  empty; `fit=false`: census only), models on up to 8 threads, each on one. `check=FILE` measures
  a translation against the source its `source` names; `compare=true` returns the image. Rigged
  models are skipped (§8.4). Translations carry `source` {library, title, license} and `fit` {iou,
  de, mesh_pct, mean_mm, worst_mm}.
- **Kenney Mini Dungeon** (30 models, 2 rigged skipped; `assets/catalogs/kenney/mini_dungeon.tsv`):
  28/28 in the `fit` tier, 15 of them editable; worst-view IoU median 0.983 (min 0.902), mean-view
  median 0.991, ΔE median 1.2, mesh share median 13%; o200k tokens (tiktoken 0.12.0) median 293
  (230 without `source`/`fit`), max 508; 2.75 s on one core for all 30 (0.77 s on 8 threads);
  two translations of the pack are byte-identical. Targets met: median IoU ≥ 0.95 at ≤ 300 tokens.
- **Decisions.**
  - Multicoloured pieces are not split by colour: the patches are open surfaces no primitive
    fits, and slabs on a face z-fight. They are fitted whole in their mean colour, and the asset
    loop moves them into the mesh part when the colours are off (the walls, whose texture paints
    stone and mortar on one mesh, end up mostly mesh).
  - §8.5's split before the mesh part (cut a piece across an axis, fit both sides, up to four)
    was built and measured: one asset more editable for 4% more tokens and 5× the time, after
    filling silhouette holes had already taken the mesh share from 38% to 13%. No proven benefit:
    dropped.
  - One mesh part per asset (all its fallback pieces) costs one node; its collider is one hull.
  - Pack models' `source` names the library pack and the model; the pack's pin (catalog,
    `SOURCE.json`) holds the URL and SHA-256, which per asset would cost ~60 tokens. No
    `parts_hash` or tags yet (tags come with the catalog entries of row 5.3).
  - A snapshot's definitions find their mesh files through the registry or disk when restored;
    the table does not carry mesh files yet.
  - tools/list stays under G0's 48,000 chars (47,981): the `mocap` help lost its list of
    databases (its no-args reply has them).
- **Review fixes** (9 Codex findings). Ids that make the same name (`Foo`/`foo`, `a/b`/`a_b`)
  get distinct ones (an id that already is its name keeps it, the others `_2`, `_3`...). A model
  more than 1% see-through (alpha mask or blend) is refused (`alpha: ...`): props are opaque. A
  single file in the `fail` tier is not written either. Placement other than the pack's pin or
  the default is recorded in `source.norm` (`"scale=0.5 place=center"`), which `check` replays. A
  file or folder inside a downloaded pack translates only its models (surveying part of a pack
  needs `out=`). Thin small parts (under 3 cm and 0.5% of the asset's box, plan §8.5 step 9) are
  `solid:false`, a mesh part too (mesh parts now take `solid`). Open surfaces of double-sided
  materials make the mesh file `two_sided`: the view draws its backs lit (`TWO_SIDED`), not as the
  cut-open cap (both windings would z-fight: the mesh pipeline does not cull). More than 16 mesh
  parts after copies is an error. `core/<name>` meshes load from the embedded files (the
  browser). Mini Dungeon re-surveyed: only floor-detail changed (5 stones `solid:false`, 1,013 →
  1,083 chars); 10 of its mesh files are two-sided.

## Asset plan 4.2, part 1: pack licences, credits, publish to LIB/core, review pages — 2026-10-12

The tooling half of row 4.2 of `docs/ASSET_SYSTEM_PLAN.md` (LIB-3, LIB-5). The W0 exemplars
follow in part 2, once row 3.3 lets props wear materials.
- **Licences** (`pav_tools/src/asset_library.rs`): every committed pack has a `PACK.json`
  (`pack title license credit source [catalog]`). `lib/` may hold CC0-1.0, CC-BY-3.0 and
  CC-BY-4.0, `lib/core` CC0-1.0 only (own work); test `committed_packs_are_redistributable`
  also checks each asset's `source.license`. `lib/core/PACK.json` and `review.tsv` are not
  embedded, and the index skips `PACK.json`.
- **Credits:** `assets reindex=<pack>` of a committed pack also rewrites
  `assets/props/CREDITS.md` (each pack's licence and credit line; local packs are not shared,
  so not listed); test `credits_file_is_current`. The studio package ships it, and the README
  links it. The plan's `assets credits=true|write` args are left out: reindexing writes the
  file, and the tools/list cap had no room.
- **Publish:** `asset_edit action=publish name=x if_revision=R review="ok: …"` puts a workshop
  asset into `LIB/core` after a sheet review of revision R (a newer revision is refused). The gate: no `check` findings, a description of 1-120
  characters, a category, 2-6 tags, a token cap by category (400 food/tool/signage, 600
  container/lighting/rock/vegetation, 800 most, 2,000 town/dungeon), references to BUILTIN or
  LIB/core only, and own work or a source naming the pack's licence (every asset with a `source`
  names one). It reads everything first, then writes `lib/core/x.json`, its index row, a
  `review.tsv` line (`name revision review`, a review never empty) and the credits, putting the
  files back if any write fails. `PACK.json` needs a title, credit and source. Test
  `core_assets_are_reviewed`: every core asset passes the gate and has a review of its current
  revision, so an edited core asset needs a new review. Other packs come from import (row 5.3),
  so publish has no `pack=` arg; the `review` arg was paid for by trimming the clip library's
  `load=` help (tools/list stays under 48,000 characters). The plan's refusal of WORKSHOP saves
  that depend on local-only sources is left out until an import brings such sources.
- **Review pages** (`scripts/review_page.py`): the lean gates' visual checks go to the user's
  eye. A manifest of items (captures, facts, a note) becomes one page, published as a private
  Artifact with the `db` capability: the user clicks ok / fix / drop (or keys 1-3, j/k to move)
  and adds notes, which save to the artifact's store for the agent to read back; without the
  store they stay in the browser and "Copy verdicts" gives them as text.

## Asset plan 4.1: library names and tiers, `assets find=`, assets drawn from game code — 2026-10-12

Row 4.1 of `docs/ASSET_SYSTEM_PLAN.md` (LIB-1, LIB-2) plus what Capital Strike asked for.
- **Names and tiers** (`pav_core/src/props/store.rs`): `BUILTIN/x`, `WORKSHOP/x`,
  `WORKSHOP_LOCAL/x` and `LIB/<pack>/x` (pack `[a-z][a-z0-9_]{0,31}`; `core` is the embedded
  one). The build script's new `ASSET_DATA` table embeds `assets/props/templates/` and
  `lib/core/**` (json, index) and replaces the hand-listed templates. `props::get` reads a
  pack's asset on first use: `LIB/core` from the embedded data, other packs from
  `disk_root()/lib/<pack>/` or `local/<pack>/` (native only); `props::resolve_ref` says why a
  name fails (unknown, with the nearest known name, or a file that does not compile), and rooms,
  `spawn_prop`, `room_check`, previews and `emit_asset` use it. Library assets are read-only:
  `asset_edit copy from=LIB/...` first. `WORKSHOP_LOCAL` saves to `local/workshop/` (git-ignored
  with all of `assets/props/local`). Existing names and `prop1-` revisions are unchanged.
- **Budgets and packaging:** tests `embedded_assets_fit_budget` (3 MB, 16 KB a prop, 64 KB a
  mesh), `embedded_library_is_valid`, `game_asset_refs_are_embedded` (every `asset`/`model` in
  rooms and game data is BUILTIN or LIB/core and resolves), `committed_packs_fit_budget` (40 MB,
  1 MB a file) and `index_files_are_current`. `.vercelignore` drops packs other than core, local
  tiers and fitted materials; `scripts/package-studio.py --packs a,b` ships on-disk packs and
  refuses more than 30 MB of them without `--allow-large`.
- **Retrieval** (`pav_tools/src/asset_search.rs`): each folder's `index.tsv` (`name cat size_cm
  parts tok fid src tags sockets note desc`; `assets reindex=templates|<pack>` writes it and
  keeps `note`, and `cat`/`tags` that a format-1 document cannot hold) is what queries read;
  workshop rows are made live. `assets find=` needs every word and ranks leaf 5, tag/socket/
  category 3, name 2, description 1, ties to fewer tokens; `category= tags= (@sit: a socket)
  tier= similar=NAME limit= page=`. Rows are `name cat size_cm parts tok tags desc`, cut to
  ≤ 116 bytes: 33-36 o200k tokens measured with tiktoken (the reply-budget test counts 3 bytes a
  token). The four templates got categories and tags.
- **Assets drawn from game code** (`pav_view/src/assets.rs`): `emit_asset(scene, name,
  Placement::at(pos).yaw(deg).id(id), seed, &palette)` draws a unit or a piece of scenery from a
  game's own state each frame, exactly what a placed copy draws (tested against `emit_prop`),
  with generated meshes cached across calls and sent once per scene. No `fx` particles (they
  need per-emitter state). `PropDef::varies` makes every seed of an asset without
  chance/jitter/pick reuse its compiled parts, so per-unit seeds cost nothing there.
- **Decisions:** `desc` joins the index so descriptions are searchable without reading assets;
  `params` waits for LIB-4; reply rows leave out `fid`/`src` while all rows are authored and own;
  `committed_packs_are_redistributable` and `CREDITS.md` move to 4.2 with the licences;
  `lib/core` stays empty (a header-only index) until 4.2 publishes reviewed exemplars; the
  tools/list cap goes from 47,000 to 47,500 characters for the new `assets` args.

## Asset plan 3.2: the renderer draws materials — 2026-10-12

Row 3.2 of `docs/ASSET_SYSTEM_PLAN.md` (MAT-2, MAT-3): procedural materials draw on the GPU,
pixel for pixel as the CPU reference evaluates them. Nothing in the game wears one yet: props
and statics name materials in 3.3, themes in 3.4.
- **Renderer.** `pav_render/src/shaders/materials.wgsl` (the kernel library, ported from the
  prototype with 3.1's fixes) is compiled in front of `scene.wgsl`. Materials are rows of a
  storage buffer (group 1, binding 2; bindings 0-1 are the font's), 368 bytes each
  (`GpuMaterial`, Pod, cast from `pav_core`'s packed words), grown to the next power of two and
  uploaded only when `Scene::materials_version` changes. `MeshInstance`, `SdfInstance` and
  `DynamicMesh` gain `material` (`slot | seed << 12`; 0 is the old path, untouched) and
  `MeshInstance` gains `ext` (half extents; set for rounded boxes, which are built at size).
  `Globals.params.w` is the pixel footprint (no derivatives, so the kernels are Tint-safe in
  loops and after `fs_sdf`'s discard); meshes pass a part-local frame from the vertex shader,
  SDFs build one along a→b. `shade` takes roughness/metallic (a cel highlight band for metals,
  a roughness-driven lit exponent); the outline buffer keeps the geometric normal. A slot past
  the table draws plain.
- **View.** `pav_view::materials::MaterialTable` (on `ViewBuilder`, `materials()`) interns
  (name, map, the role colours the layers use) into stable slots and attaches the table to every
  built scene; `refresh(lib)` re-packs rows in place after a library change, and
  `instance_generation` invalidates the static region caches, which bake the base colour.
  `MatSlot::param(seed)` gives the instance word (world maps take seed 0; `seed(entity, part)`
  for the others). `emit_shape` and `emit_object` take a material word; blocks, decor and props
  pass 0 until 3.3.
- **Checks** (the lean gate's objective part). Slot-0 captures of `test`, `town` and `level/3`
  at 640×360 are byte-identical to captures from before the change. `pav_view/tests/
  material_parity.rs` renders every kernel (variants as extra layers) world-mapped on a floor
  and object-mapped on a turned, scaled, seeded slab, and compares with `eval_packed`: 100% of
  pixels within 1/255 in all 20 cases. `pav_render/examples/material_bench.rs` (a Shardfall-like
  room at the game camera, 960×540, llvmpipe): 68.9 ms with materials off, +16% typical, +48%
  with three-layer mossy stone on every surface (D8 budgets +25% and +60%).
  `scripts/check-wgsl.sh` dumps every renderer module (`examples/dump_wgsl.rs`) and compiles it
  with `createShaderModule` + `getCompilationInfo` in headless Chromium: all clean.
- **Decisions.** A palette with new role colours gets a row of its own instead of re-packing in
  place, so a cached instance never shows another palette's colours; slots aren't freed in a
  session (4,095 at most, then a material draws in its plain colour; a warning past 256).
  Table versions are unique across tables, so a renderer that outlives a view never keeps a
  stale table. The prototype folder `docs/asset_plan/prototypes/materials/` is removed.
- **Shared target dirs.** Cargo hashes workspace crates by their path relative to the
  workspace, so two worktrees building into one `CARGO_TARGET_DIR` overwrite each other's
  crates and can silently test another checkout's code. A per-package profile tweak (e.g.
  `--config 'profile.dev.package.pav_core.codegen-units=255'`, likewise `pav_render`) gives a
  worktree its own crate hashes while sharing the compiled dependencies.

## Asset plan 5.1: the import refactor and the reading half of `asset_import` — 2026-10-12

Row 5.1 (I1 + I2) of `docs/ASSET_SYSTEM_PLAN.md`, moved ahead of Phases 3-4 as §11 allows.
- **I1, no behaviour change.** What the motion importer shares with the asset importer moved to
  `crates/pav_tools/src/import/`: `fetch` (downloads; `cache_dir_for("assets")` =
  `$PAV_ASSET_CACHE` else `.cache/assets`), `catalog` (`Json`, `Catalog`, `Entry`), `ledger`
  (`Ledger`, `csv_line`), `gltf` (the container, accessors, matrices, node hierarchy) and `report`
  (`write_set_to`, the clip report, `set_out`). `mocap` re-exports them, so old paths hold.
  Hardened on the way: GLB chunk types, `bufferView.buffer`, view bounds with checked arithmetic,
  accessors capped at 16M elements, zip entries CRC-32-checked and capped at 256 MiB inflated,
  names written to disk as their base name. `cargo test -p pav_tools` passes unchanged and
  `clip_import` rebuilds MESH2MOTION byte for byte as before the move. The committed
  `anim/mesh2motion.json` differs from today's rebuild only by one legend line added to
  `clips.rs` after it was made (and the title and credit it was given by hand).
- **I2, one intermediate.** `import::scene::Scene` (nodes, world-space meshes in metres with CCW
  front faces, linear colours, materials, decoded images). Readers: glTF 2.0 in-house (.glb;
  .gltf with .bin files or data URIs; indices and strip/fan modes; normals, two UV sets, vertex
  colours; sparse accessors; materials with KHR_texture_transform, emissive strength and unlit;
  PNG images; skinned meshes in their bind pose; required Draco/meshopt/Basis/WebP named in the
  error) and OBJ/MTL through `tobj` 4.0.3 (`Kd` linear, `d`, `Ke`, `map_Kd`; v turned down).
  JPEG is listed but not decoded (deferred, I9). `import::normalize`: up axis, yaw in exact
  quarter turns, metres per unit, `place=keep|floor|center`, inside-out primitives flipped,
  0.1 mm welds, connected parts per (node, material), face colours (base × texel × vertex colour,
  in linear light) and a main-colour palette merged in Oklab.
- **The tool, read-only for now.** `asset_import`: no args, the library directory; `get=lib/pack`
  downloads a pack pinned in `assets/catalogs/<lib>.json` (whole zip up to 64 MB with its SHA-256
  checked, else members by ranges; git sources as listed files at a commit), unpacks only the
  model folder and the licence, checks the licence's SHA-256 and writes `SOURCE.json`;
  `from=<model> list=true` summarises one model, `from=<folder> list=true` a row each;
  `survey=lib/pack|folder` writes a census ledger (`id file tris mats tex comps size_cm flags`,
  the fit columns empty, notes kept). Kenney Mini Dungeon 2.0 is pinned: 30 models read in
  0.17 s, 5,493 triangles, median 4 parts, two skinned characters; two surveys give identical
  bytes.
- **Khronos corpus.** `tests/import_khronos.rs` (ignored; downloads 12 core models at commit
  `edc7c9e`): all 12 read. OrientationTest's arrow tips land 0.003 units from their targets,
  NegativeScaleTest's faces all agree with their normals, sparse values apply, alpha modes,
  vertex colours, second UV sets and texture transforms read. The `gltf` crate fallback is not
  needed.
- **Decisions.** Sizes are reported in cm (D1) while the `Scene` stays in metres (§8.4).
  Placement is `place=` in catalogs and the tool: the plan's catalog example used `origin` for
  both the credit text and the placement. `list=true` is the per-model summary; `find=`,
  `from=` translation, `catalog=`, `check=`/`compare=` and the licence gate arrive with rows 5.2
  and 5.3. KayKit's GitHub listing needs the API, which this container's proxy refuses; git
  sources take an explicit file list until then. `tests/mocap.rs` keeps its own glb helper so the
  existing tests stay untouched; `tests/common` has the importer's builders. G0's tools/list cap
  in `tests/asset_errors.rs` rises from 47,000 to 48,000 chars: the list stood at 46,994 and
  `asset_import` adds 956 (§8.2 budgets up to 2 KB).

## Asset plan 3.1: materials, the core and the CPU reference — 2026-10-12

Row 3.1 of `docs/ASSET_SYSTEM_PLAN.md` (MAT-1): procedural materials exist as data and can be
evaluated on the CPU. Nothing draws them yet (3.2), and props, statics and themes don't reference
them yet (3.3, 3.4).
- **Files.** `assets/materials/*.json` and `pb/*.json` (12 seeds, moved from
  `docs/asset_plan/examples/`) are embedded by `pav_core/build.rs` as `MAT/oak_planks`,
  `MAT/pb/gold` and so on. A strict reader (`material/schema.rs`) gives errors with the path and a
  fix (did-you-mean, ranges, "lengths are in cm"); lengths become metres on read, so the same
  material in cm or m packs identically. `read_inline` is the seam for props' inline materials.
- **Rules.** `base` inherits with a shallow merge, `layers` replaced wholesale, at most 4 hops, no
  cycles. Budgets: ≤ 3 layers, ≤ 4 cell colours, a warning above 6 cost units; legibility
  warnings for features under 5 cm or gaps under 1.5 cm. Every seed loads clean (the costliest,
  mossy_stone, is 5.5 units).
- **CPU reference** (`material/eval.rs`): a line-for-line port of the prototype's WGSL kernels and
  its CPU mirror (same PCG hash, value noise, fbm octave fade, pulses, cell code), reading the
  same packed 368-byte row the GPU will read, so 3.2's parity test compares like with like. The
  `cover` quantile tables are regenerated by a test. An eval golden hashes 64 samples per
  material in four setups, quantised to 8-bit sRGB so it survives libm differences.
- **Decisions.** `angle` is refused for now (no kernel rotates; only the deferred texture fits
  wanted it). Each layer's salt comes from its index and `seed`, not the material name's length as
  in the prototype, so renaming a material keeps its look and `seed` re-rolls a layer. The
  prototype's undocumented shingles flip and two debug kernels are dropped.

## Test games: a feedback log and a single-file web build — 2026-10-12

The user tests the engine with test games: a fresh agent builds a vertical slice of a 1985-2015
classic on it, then writes a candid one-page evaluation of the engine. Results go to
`michaelcrosato/ai-hub` as one self-contained HTML file each.
- **`docs/FEEDBACK.md`** keeps one entry per game (result, agent, cost end to end from the
  transcript, the evaluation's verdict, what was checked and acted on) and a tally of asks, so
  repeated problems and unused features show up. Larger engine work waits for a pattern.
- **First game: Capital Strike**, from Jungle Strike (1993; SNES, Genesis), by Sonnet at medium
  effort in its own worktree: 1.65 h, 170 model calls, about 6.5M tokens weighted by price. The
  renderer, wasm shell, audio synth and headless tools carried it; about 4,000 lines of gameplay
  went into a game layer the engine has no facility for, and it used no props.
- **Acted on now:** `scripts/build-web.sh --single-file` writes one page with the wasm-bindgen
  glue inline and the wasm gzipped in base64 (unpacked with `DecompressionStream`): 9.7 MB for
  Shardfall, from file:// or any host with no other requests. `--title`, `--description`,
  `--theme-color` fill what ai-hub reads; `--scene` bakes a start scene that a query still
  overrides. The page is generated from `web/index.html`, which now loads either the inline game
  or `./shardfall.js`. Also: AGENTS.md no longer implies the helicopter and `health.rs` are a
  ready weapon and damage layer, and the asset plan's row 4.1 gains drawing a library asset from
  game code.

## Asset plan re-plan: Phases 3-6 slimmed, lean gates — 2026-10-12

The user answered the G2 escalation: finish the asset system with only the features that are
needed and showed a benefit, and test it the cheap way.
- **G2 closed.** Format 2 stays: it works, agents' assets are accepted more often (29/30 against
  25/30) with half the lint findings, and it was not shown to look worse. Its cost and looks
  claims count as no proven benefit.
- **Testing budget.** The user's prototype testing policy is now a section of `AGENTS.md`: the
  fewest runs that give confidence, objective checks first, the user's eye on a comparison page
  for looks, cost counted end to end, and a cost estimate before any bigger test.
- **Lean gates** (plan §10.3) replace G3-G5's benchmark batches and judge panels: each row's own
  objective checks, a review page for visual changes, and at most 6 end-to-end runs where cost is
  the claim.
- **Kept, 11 rows:** materials (core, renderer with parity and budget, props and tools, theme
  materials), the library core (names, tiers, retrieval, credits, ~30 reviewed exemplars, static
  props and data-driven dressing), the import pilot (readers, fit, CPU metrics, catalogs and the
  licence gate) and its scale-out after a GO.
- **Deferred:** parametric families, exemplar waves to ~520, theme kits, item and weapon models,
  monster gear, town as data, texture-library fits, the Physically Based importer, extra material
  growth, the GPU fit cross-check, re-authorings beyond the core set, and every judge panel. Test
  games built on the engine decide whether any of them returns.

## Asset plan: G2's token pass doesn't hold end to end — 2026-10-12

A correction to the G2 record in the next entry, from the session's own transcripts. The NO-GO
stands and the plan stays stopped at row 2.5.
- **What the token checks leave out.** `tok_x` and `tok_cum` count what crosses the wrapper: the
  docs, commands, replies and images. Every model call of an author agent also re-reads the
  agent's fixed prompt and tools, about 55k tokens before its first command, and a run's own
  content is only 11-14% of what it reads. So a run's cost follows its number of model calls.
- **End to end.** `scripts/asset_bench/results/agent_tokens.tsv` has each author run's model
  calls, context sizes and visible output. Compared as the gate compares (each of the 24
  authoring tasks against the mean of arm A's runs; medians of the per-task ratios), with the
  fixed prompt held at arm A's 54,827 tokens (F2's batch got 59,280 for reasons outside the
  format):
  - Tokens read, cached or not: arm F 1.20 times arm A's (lower on 9 of 24 tasks), arm F2 1.01
    (12 of 24). Model calls per run (median): A 5, F 7, F2 6. The harness's `turns` count
    wrapper calls only.
  - Priced with every read as a cache read (a tenth), each run's own context written once
    (1.25) and its visible output (tool calls and text, in o200k tokens) at 5: F 1.10, F2 0.91
    (lower on 15 of 24). The transcripts keep no hidden reasoning, so that is left out.
  - Caveat: arm A ran about ten hours earlier with a prompt one line shorter.
- **So** round 2's wrapper saving (`tok_x` 0.51, `tok_cum` 0.56) mostly doesn't carry through:
  end to end, F2's runs cost about 9% less than arm A's, lower on 15 of 24 tasks, which these
  runs can't tell from noise. Its other objective results stand (29/30 accepted against 25, lint
  findings per accepted asset 0.48 against 1.08, median failed calls 0), and the quality vote
  stays a fail.
- **What the testing cost.** The session that ran rows 0.1-2.5 took about 13 hours. Weighting
  tokens by price (cache reads 0.1, cache writes 1.25, output 5), it used about 127M, about 40%
  of it on benchmark runs, judging and their analysis (G2 alone about 35M). Each of the 288 judge
  agents read about 120k tokens, nearly all of it fixed prompt, to look at one image and a short
  prompt (about 1.6k tokens). The largest single cost was the main session re-reading its own
  context, a median of 442k tokens on each of 1,942 calls: about two thirds of the total. How
  testing and the gates change is the user's call, asked in chat.

## Asset plan 2.5: format-2 docs, the fixed bench, and a NO-GO at gate G2 — 2026-10-12

Row 2.5 of `docs/ASSET_SYSTEM_PLAN.md` (F8, EVAL-5). The row's work landed, but gate G2 failed
after its one remediation round, so the plan stops here: Phases 3-6 wait for the user. No kill
criterion (K1-K3) held in either round.
- **Docs and the bench (F8).** `docs/ASSET_STUDIO.md` teaches format 2 with the format-2 bench as
  its example. A test checks that the doc names every key, shape, socket, look and op the reader
  checks, and that its bench reads, compiles clean and takes the doc's patch. `BUILTIN/bench`'s
  rear legs now lean with the backrest instead of piercing it (still format 1; 220x142x98 cm,
  clean check). `asset_edit action=schema` returns format 2's guide unless `format=1` is passed,
  and `inspect legend=true` gives the legend of the asset's own format.
- **Harness.** `bench.py pairs` stacks each authoring run over or under the baseline's run of the
  same task (hashed names, random order). `report --vs` applies G2's pass checks and kill
  criteria. `score` re-scores every stored run with the newest build.
- **Format checks: pass.** Revision goldens, the §5.9 corpus, and format-1 template scenes and
  compounds are unchanged. A 1,024-part asset compiles in 0.42-0.44 ms, and a placed copy
  serialises in under 64 bytes.
- **Round 1, arm F (`8a0764d`): fails on tokens.**
  - Results: 30/30 runs accepted against arm A's 25 of the same 30 tasks. Median ratios to arm A
    were `tok_x` 0.857 and `tok_cum` 1.217 for authoring (limits 0.75 and 0.65) and 0.534 for
    edits. Median failed calls were 1 over all 30 runs (3 over the authoring runs alone). Arm F
    won 15 pairs and lost 9; rubric Δ +0.10.
  - Where the tokens went: 8 runs sent the doc's multi-line example a line at a time, failing up
    to 20 lines each, with every piece echoed back. 29 runs followed each sheet with a
    520-910-token `capture`, as in the run README's example. In 9 runs a create failed on a
    misuse (`cycle` on the node, `facing:"up"`, `height` beside from/to), and 4 runs re-sent
    `create` on an existing name. Runs took a median of 3 turns; arm A's took 1.
- **Remediation (docs, errors, defaults; `c0ae045`).**
  - Docs: both examples are whole one-line commands. The loop makes and shows the object in one
    go, says the sheet is the image (no capture needed) and stops when check and sheet are
    clean. `create` versus `replace`, `cycle` inside the copies, `facing`, the rounded_box radius
    and which findings are faults are spelled out. The doc went from 1,858 to 1,719 tokens.
  - Errors: the REPL answers each piece of a split JSON command with one short line, and two
    commands on one line with a hint. `cycle` on a node and direction words (`up`, `left` ...)
    get fixes. `create` on an existing name points at `replace`.
  - Check: `mirror_overlap` compares the parts' boxes, not their origins, so an off-centre blade
    no longer warns.
- **Round 2, arm F2 (`c0ae045`): fails on quality.**
  - Tokens pass: authoring `tok_x` 0.511 and `tok_cum` 0.561, edits 0.392. 29/30 runs accepted
    (arm A 25), with median failed calls 0, median 2 turns and 4 calls, and 3 captures in 30
    runs (58 in round 1).
  - Quality fails: arm A's asset won 14 of 24 pairs. Arm F2 won 10 with no ties, where parity
    needs 12, and the rubric Δ was −0.183 (limit −0.1). Judges marked down a brazier lost in
    its own glow, sacks that read as vases, merlons sitting on top of a wall and a cart load
    shaped like an egg.
  - The one rejected run (P09) left a tree 0.9 cm off the floor: its agent read a patch reply's
    empty `check`, which lists only new findings, as a clean asset.
- **Verdict: NO-GO.** Round 1 met quality but not tokens; round 2 met tokens but not quality. The
  gate needs both in one round, and its one remediation round is spent. The judging is noisy
  against identical arm-A assets: 11 of 24 tasks changed outcome between the rounds, and turn
  counts do not separate wins from losses. Pooled, the format-2 arms won 25 of 48 pairs. Whether
  to stop, re-judge or change the gate is the user's call; nothing past row 2.5 has started.
  That question is logged in `docs/ESCALATIONS.md`; until the user answers, the plan stays
  stopped.
- Everything that landed stays: format 2 reads, compiles, draws, patches and saves, and the
  remediation's docs and errors are what arm F2 measured. Every final asset is text in
  `scripts/asset_bench/results/assets/G2/`; the run logs and images are in `.cache/asset_bench/`.

## Asset plan 2.4: format-2 patches, one definition per snapshot, seeds — 2026-10-12

Row 2.4 of `docs/ASSET_SYSTEM_PLAN.md` (F6, F7). Format-2 assets are edited by patch, a snapshot
carries each definition once, and placed copies take a seed and a palette.
- **Patch ops on format 2** (`props/patch/v2.rs`). A batch applies to the document's format-2
  view, then the result is read, checked and compiled whole, at most 1,024 ops, atomically.
  - Targets: `part` (dotted through groups), `parts:[…]`, `material`, `socket`, or none for
    the document's own fields. `fields` take dotted keys (`"radial.count":12`) and `null`
    deletes; a `type` replaces the shape fields, and `size`/`half` (or `height`/`half_height`)
    replace each other.
  - Ops: set, add (into a group by a dotted id), remove, rename (attach and socket targets
    follow; for a material, its `mat`, cycle and remap references), copy, mirror, array, expand,
    move, scale and unit. Each problem names its op (`ops[0] stones.ring.radial.count: 0 is
    outside 1 to 64`), compile errors too.
  - `expand` replaces a replicated node by parts `<id>_<i>` placed where its copies were: ends
    stay ends (a spin keeps the cross-section turned), positions take yaw/pitch/roll, attached
    copies shift their offset, cycle entries become each copy's `mat` or `color`, and an arc
    keeps its facing on quarter turns (other turns are refused). An attachment to one copy
    follows it; one to the whole node is refused. So is a node a seed varies (`chance`,
    `jitter` or `pick` in it or inside it, or a `pick` on its group), since new names would
    draw its copies anew. Tests expand the fixtures' 6 replicated nodes without such variation
    (the other 5 are refused), and radial copies of ends, of turned wedges, of attached tabs
    and of arcs, and compare every compiled part's tint and hull (within 0.01 cm).
  - `unit` converts every length (parts, steps, light radius, fx area, sockets, material
    pattern sizes); `scale` multiplies lengths about each part's centre (an attached part
    keeps its offset), or the whole asset about the origin (a reference's `scale` grows with
    it). Every fixture converted to metres
    and back compiles to the same parts.
  - Format-1 assets keep format 1's own batches (set, add and remove of format-1 fields, with
    the same messages); other ops go through the format-2 view, and the result saves in format
    1 while it fits.
- **Tools.** `asset_edit action=patch` edits format-2 assets. `action=inspect resolved=true`
  lists every drawn part in the asset's unit: type and size, then from/to for a part placed by
  its ends, else pos and yaw/pitch/roll, and `mirrored` for a mirror copy. `action=schema
  format=2` is format 2's guide with an `ops` topic (the default stays format 1 until row 2.5).
- **Snapshot dedupe** (`props/table.rs`). `SimState::prop_defs`, declared first (a test checks
  the field order), holds each definition the placed copies use once, with what their
  references compiled against; a copy names its definition by key. A `PropInstance` is 59-61 B
  for the templates, and an extra bench costs its 411 B entity instead of also carrying its
  1,079 B definition. `state_hash` hashes the keys. `snapshot()`, `save_state` and `restore`
  rebuild the table from the placed copies, awake or asleep. Snapshots from before the table,
  with definitions inline, still load to the same `state_hash`, and save with a table again.
- **Seeds and palettes.** `PropInstance` gains `seed` (the variant it draws and collides with)
  and `roles` (colours for `$wall`…, else `DEFAULT_ROLES`). `asset_spawn` takes `seed=` and
  `roles=`, `asset_preview` and its contact sheets `seed=`. Room objects take `seed` and `roles`
  (written back by the room editor, documented in `rooms/_template.toml`), and `room_check`
  names an asset the library lacks, with did-you-mean.
- Fix: the end-to-end test stool from row 2.3 had its three legs in one place (a `radial` of
  radius 0 turns copies about their own centre); they now stand 12 cm out.
- Review fixes: `expand` refused nothing a seed varies (above) and panicked on a cycle without
  entries, which it now refuses; scaling an attached part about itself multiplied its offset;
  `inspect resolved=true` put an arc's ends beside its axis; and `asset_preview seed=` framed
  seed 0's bounds instead of the variant it shows.
- Decisions:
  - `ObjectDef.params` waits for LIB-4, with `PropInstance.params` (§5.5): before parametric
    families there is nothing for a room's params to set.
  - A snapshot key is the FNV-1a hash of `PropDef::key` (a u64), so a copy fits in 64 B with
    its asset name.
  - The placement tools name the variant `seed=`, as the plan does. Like other tools' seeds, a
    one-shot `pav <tool> seed=N` also seeds that call's session.
  - `tools/list` stays under 47,000 chars (46,993) with the seven new arguments, by shortening
    the asset tools' argument descriptions. Dogfood probe 21 (a `mirror` op with `path` on the
    format-1 well) now gets format 2's reply: `path` is unknown, address a part with `part`.

## Asset plan 2.3: compiled parts drive every consumer — 2026-10-12

Row 2.3 of `docs/ASSET_SYSTEM_PLAN.md` (F5, TOOLS-10 and lint on compiled parts). Everything
that draws, collides with, measures or checks a prop now reads its compiled parts, so format-2
assets can be created, drawn and placed.
- **Definitions everywhere.** `props::get` returns the `PropDef`; `install(name, def)` returns
  the assets that reference it, recompiled; the library keeps definitions only. `PropInstance`
  and `PropFrame` hold the definition. Snapshots still embed each instance's document: a
  format-1 definition writes exactly the bytes it always wrote (state hashes of existing worlds
  are unchanged), a format-2 one its canonical document and the definitions its references
  compiled against, compiled again from those alone on load.
- **Exactly format 1.** A part's box is now exact per shape (`centre`/`extent`, format 1's own
  formulas for its shapes) and `bounds(scale)` matches format 1's bounds bit for bit at any
  scale. Frames skip identity turns instead of multiplying by them, so a format-1 part keeps its
  rotation's signed zeros. Tests compare the old and new paths: the renderer's mesh, SDF and
  light lists (`pav_view` build.rs) and the physics compounds (`prop_instances.rs`) are
  identical for the templates and both dogfood files at three scales and poses.
- **Drawing** (`pav_view/src/build.rs`): `ViewBuilder::emit_prop` draws each part through
  `emit_prim`. Format 1's shapes go through `emit_shape`; ellipsoids use the sphere mesh, plain
  cones the cone mesh, round cones the distance field; the rest are generated meshes, built
  once per key, kept by the view builder (up to 256) and sent once per frame. Part lights go
  through `fx::light` (radius times the instance scale); part emitters through the same carry as
  objects', keyed by (entity, emitter). Only parts without a light of their own add the emissive
  glow light. `sway: leaves` sets the sway flag and `grass` draws a tuft. Theme roles take
  `DEFAULT_ROLES` (part, light and emitter colours alike); jitter tints multiply. Hidden parts
  are not drawn. A new previewed prop (or a contact sheet) clears the particles left by the
  last one.
- **Collision** (`prop_instance.rs`): a compound of the solid parts, hidden ones included.
  Format 1's shapes are built as before; capsules with equal radii are capsules, plain cones
  cones, tubes 12 boxes round the wall, tori a ring of up to 12 capsules, and everything else a
  convex hull. More than 256 shapes is a compile error ("mark decorative parts solid:false").
  The fixtures give 7, 42, 16, 0, 64, 15, 3 and 10 shapes. Nav marks solid resolved boxes.
- **Lint** (`props/lint.rs`) runs on the drawn parts of the compiled asset. Generated shapes,
  ellipsoids and round cones count by their box and never hide or cover another part. The
  compile warnings join the findings (its box-contact `floating` gives way to the exact one, and
  `unit_suspect` now names the unit). `hang` and `grip` sockets anchor the asset at that point
  and `wall_mount` at its wall, instead of the floor. The examples: the bench's leaning rear
  legs dip 0.8 cm under the floor, the stall keeps format 1's four crate faults, the well's
  water pokes through its middle stones, the sword and torch are clean.
- **Tools.** `asset_edit` creates, copies, replaces, undoes and redoes documents of either
  format, saved in the lowest format that holds them; `patch` stays format 1 until row 2.4 (a
  format-2 asset answers "send the whole document with action=replace"). Undo steps are JSON
  documents (older typed files read the same way). `check` and the cost line work on compiled
  parts (generated meshes, ellipsoids, cones and part lights count), `inspect bounds=true` lists
  resolved parts, `assets` lists every definition, saved files whose references load later in
  the folder wait for them, and placed copies of dependents refresh too. An end-to-end test
  creates a format-2 stool, checks it, renders it (the generated ring is one uploaded mesh and
  the triangles match the cost line), places it (16 shapes), replaces, undoes and redoes it.
- **Native Object Studio** (TOOLS-10, upkeep only): it lists every definition; format-1 parts
  keep their drafts (the definition's format-1 view is the adapter, so no `V1Part` type was
  needed), and format-2 assets are edited as whole documents in the JSON editor.
- The conformance fixtures moved to `pav_core/tests/fixtures/props/format2/`, where the view and
  tool tests read them too. The asset studio smoke test passes 13/13.
- **Review follow-ups from row 2.2** (the Codex review of its PR, each finding verified first):
  - A reference resolves its document in place, with the caller's seed and its parts' full
    ids, so each copy varies on its own and seeds reach inside. Its `remap` swaps materials
    inside the referenced surface cascade, so explicit colours still win.
  - `remap` keys are checked against the referenced asset's materials, with did-you-mean.
  - A reference's lights and emitters count toward the limits of 4.
  - A definition keeps the definitions its references compiled against; every seed variant
    uses them.
  - Removing an asset that others reference is refused, and `asset_edit` refuses such an undo
    before it touches disk.
  - Not changed: `attach` resolves against the target's authored placement, before its
    chance, jitter and pick. Moving attachments with a jittered target would mean resolving
    anchors per seed and per copy; no asset needs that yet.
- **Review follow-ups from this row** (the Codex review of its PR, each finding verified first):
  - A snapshot carried a format-2 instance's own document only, and a restore compiled its
    references against the live library: editing a referenced asset changed a saved world and
    removing it broke the load. It now embeds `{"document", "refs"}` with the definitions the
    references compiled against, and a restore reads only those.
  - `asset_edit` wrote the file and its history before the registry could refuse the edit (a
    stool replaced to hold the pair of stools that holds it). It now asks first
    (`props::check_install`/`check_remove`) and writes nothing on a refusal; if publishing still
    fails, the asset file and its history are put back.
  - The file watcher retries a file that failed while others in the same batch load, so a file
    that references one later in the batch loads in the same pass.
  - `asset_preview views=true` on a prop that draws nothing (every part hidden or left out at
    seed 0) says so instead of framing an empty sheet.
- Decisions:
  - Undo history as JSON documents moved here from F6: format-2 creates need it.
  - `grip` sockets anchor the floor rules like `hang` (the plan named `hang` and
    `wall_mount`): a held item is not meant to stand on the floor.
  - `inspect resolved=true` moves to row 2.4 with the other `asset_edit` changes (F6).
  - `PropInstance` gains its seed and palette with row 2.4's seeds and rooms (F7), and params
    with parametric families (LIB-4); until then copies draw seed 0 with the default palette.

## Asset plan 2.2: format 2 compiles — 2026-10-12

Row 2.2 of `docs/ASSET_SYSTEM_PLAN.md` (F3, F4 and the §5.9 conformance test). A prop document
of either format compiles to resolved parts, the form the consumers will draw, collide and
measure from row 2.3.
- **Compile** (`props/compile.rs`): `compile(&Doc, deps, seed) -> CompiledProp` (parts, bounds,
  sockets, lights, fx, warnings), pure and deterministic (f32, BTreeMap order, depth first,
  copies in index order, metres). It covers all of §5.4. From/to frames: local y from → to,
  local z the parent's up made perpendicular, `spin` about the axis. `axis` uses the base
  rotations of §5.2 before yaw, pitch and roll; an arc turns about its axis to stay centred on
  `facing`. Groups carry their frame and surface. Copies: arrays by `step` or `span`, grids with
  `index = ia + a·ib`, and radials turning each copy, with angle 0 at +z for a y axis and +y
  otherwise; `cycle` entries set a copy's material or colour. Mirrors map a whole subtree through
  the reflection, and only leaves turn proper (a `mirrored` flag reflects local x). `attach`
  finds the face, edge or corner centre of the target's box in the parent frame, the target's
  ends, or a `[u,v,w]` point; the attaching part's own anchor defaults to the opposite side. Refs
  inline the referenced asset's parts as `id.part`, through scale and mirrors, with `remap`. The
  surface cascade takes the nearest explicit field, then that level's material. Variation is
  seeded per part: FNV-1a of `"{seed}:{id}:{field}"` drives `chance`, `jitter` (position,
  rotation, scale, colour) and `pick`, so adding a part reshuffles nothing else. Lights, fx,
  `sway`, `hidden` and sockets are resolved too. Warnings: `unit_suspect`, `floating` (box
  contact at 0.5 cm, up to 10 groups), `sdf_heavy`, `frame_turn`, `mirror_overlap`. Limits:
  1,024 parts and 64 generated meshes.
- **Exact format 1.** Frames keep their rotation as a quaternion with mirror signs, so a format-1
  part compiles to exactly its own position, rotation and shape (a capsule's cap centres at ±
  `half_height`), bit for bit, for the templates and both dogfood files. The renderer's instance
  lists can then stay identical when the consumers switch in row 2.3.
- **Generators** (`props/geom.rs`): the resolved `Primitive` with its local box (an arc's box is
  its visible sector) and hull points. The meshes the renderer lacks are generated: frustums
  (tapered or cut cylinders and cones), tubes, tori, wedges, lathes and extrusions. The first
  four are unit-normalised like the renderer's meshes, so one mesh serves every size. Lathes and
  extrusions are absolute; a mirrored extrusion is reflected and rewound. Keys are FNV over
  quantized parameters (thousandths, tenths of a degree, tenths of a millimetre). Every mesh is
  closed and faces out (welded edges pair up, the signed volume is positive).
- **Definitions** (`PropDef`): the library holds each document with its revision, its compiled
  parts (seed 0), up to 64 seed variants, its references and their depth (at most 3), and a key:
  the revision, or with references `propk-` over the revision and the referenced keys.
  `install_def` returns the definitions that reference the installed asset, which it
  recompiles; an asset that would reference itself is refused. The consumers keep reading each
  definition's format-1 view (`props::get`) until row 2.3; format-2 definitions have none.
- **Conformance** (`pav_core/tests/prop_format2.rs`): every §5.9 golden holds on the first run.
  The counts are bench 7, greatsword 9, stall 46, torch 6 and well 53. Among the others:
  `leg_back~x` at (−85, 70.5, −41) with pitch −12.01; the well's `stones[1].ring[3]` and
  `roof~z.slab` (yaw 180, pitch 35); the stall's `crate[1].fruit[5]` at (78, 129.06, 53.8); the
  pommel at y −40.5. The probes resolve `arm~x.hand`, the cascade's #30c040, and the attached
  cube and knob. Compiling twice gives identical bytes; the five examples and the templates warn
  nothing. In release a 1,024-part asset compiles in about 0.4 ms on this container (budget 1
  ms): placements are computed once per sibling map, the cascade borrows from the document,
  boxes come from absolute rotations, and mesh keys hash without a buffer.
- Decisions:
  - Format-2 `create` stays refused until row 2.3, when the consumers draw compiled parts, so
    no asset is ever saved that the engine cannot draw. The plan's row list said "until 2.2".
  - `attach` targets are the attaching node's siblings (`group.part` reaches inside one): a
    copy of a group then attaches inside itself. The checker no longer falls back to the top
    level.
  - The mesh-file loader of F4 moves to row 5.3 (I3), which writes mesh files; nothing reads
    or writes them before. Mesh nodes stay "not available yet".
  - Theme roles default to neutral stone and a warm light (`DEFAULT_ROLES`).
  - `PropInstance` gains its seed, roles and params with the consumers (2.3) and snapshots (2.4).

## Asset plan 2.1: format 2 reads, checks and writes — 2026-10-12

Row 2.1 of `docs/ASSET_SYSTEM_PLAN.md` (F1, F2), the first of Phase 2. Format 2 is read, checked
and written, but not built: compile is row 2.2, so an asset that needs format 2 is refused with
the reason, after its errors.
- **Source model** (`props/doc.rs`): `Doc` is the document as written, in its own unit: a tree of
  nodes (primitive, group, `ref`, mesh), materials, sockets, tags, category, source and fit, each
  optional field kept as written (None: not written). Both formats read into it; a format-1 file
  is in metres and its nested `shape` maps onto the flat part. `PropAsset` stays the built model
  the engine uses until the consumers move (F5).
- **Reader.** `read::migrate` routes by format and refuses a newer one ("format 3 needs a newer
  engine (expected 1 or 2)"); 1→2 needs no rewrite of the Value. The format-2 check
  (`props/read/v2.rs`) builds the typed model as it reads: field tables per node kind; types and
  ranges in the document's unit (a length error names the unit, and a metre value in a cm
  document gets the conversion); one of `pos`, `from`/`to` or `attach`(+`offset`); `from`/`to`
  only on the six end-placed types and never with `axis` or Euler angles; `spin` only with them;
  box sizes `[x,y,z]` or `[across,thick]`; capsule ends at least `r + r_end` apart; the tube,
  torus and rounded-box radius limits; `facing` across the axis in the parent frame; one
  replication per node; cycles of material keys, hex colours and roles; `hidden` parts solid;
  material keys with did-you-mean; attach targets resolved siblings first (a copy needs its
  index, a mirror copy its `~axis`; attaching to itself, its own group, its own part or round in
  a circle is refused); `ref` targets that exist; the limits (256 nodes, groups 4 deep, 64 copies,
  grids 32×32 and 256, 1,024 parts, 4 lights and 4 fx after copies); and every part gets a colour
  from itself, a group, the root or a cycle. The guesses in §5.7 get their fix (colour,
  rotation, position, dimensions, half_extents with the doubled size, diameter with the halved
  radius, material, length, children, count, collide, cube, frustum, ring, ramp, mesh, a nested
  `shape`), and a fix that names a field replaces that field's "missing" line. An unknown key
  lists the valid keys only when it has no fix, since a node takes about thirty.
- **Not yet available**, and the errors say so: library materials (`MAT/…`, Phase 3), mesh parts
  (Phase 5), `params`/`presets` (Phase 4). Inline materials are checked against the 14 patterns
  and their parameter names; layer values stay checked JSON until materials are evaluated (MAT-1).
- **Writer and revisions** (`props/write.rs`): §5.1's layout. A header line (format, name, unit,
  the root's surface defaults); each section on a line; each map entry on its own line, group
  children indented two spaces per depth; §5.1's key order; numbers as the shortest f32 round
  trip, zero as `0`. Fields are written as written: an explicit default stays, since surface
  fields cascade and an explicit `look:"cel"` overrides a group's look. Map entries are written in key order: tool inputs arrive as JSON objects
  with no order, and a revision must not depend on how a document arrived, so the plan's
  examples (the bench lists its seat first) read to the same text. `prop2-` revisions hash the
  compact text (the same without line breaks), so a layout change keeps them. `needed_format` is
  1 for metres with only format 1's shapes, fields and keys; such a document is written as format 1
  and keeps a `prop1-` revision.
- **Tools.** `PropAsset::from_value` reads format 2 whole. A document format 1 holds is built and
  saved as format 1; anything else is refused at its first format-2 feature: `unit: unit "cm"
  needs format 2, which this engine checks but cannot build yet — write format 1 (asset_edit
  action=schema)`. The tools' field guide stays format 1; `props::format2::schema` holds format
  2's (legend, document, node, shapes, materials, limits) for row 2.5.
- **Tests.** `pav_core/tests/prop_format2.rs` holds the five examples and three probes inline, in
  canonical form (the §5.9 corpus; compile goldens join in 2.2). Each rewrites byte-identically
  with a pinned `prop2-` revision, the authored bench and the probes' layouts read to the
  canonical text, a document using every field round-trips, and the four templates and both
  dogfood files keep their golden `prop1-` revisions through the source model. Unit tests pin the
  checker's messages; `pav_tools/tests/asset_format2.rs` covers create, dry run and saving.
- Decisions: socket names are the closed vocabulary (grip, grip2, tip, top, wall_mount, hang,
  sit), with did-you-mean for others. A wedge's `peak` defaults to 0, a gable. `half` and
  `half_height` stay readable on format 1's shapes only.

## Asset plan 1.3: agent-first object docs, judge calibration, gate G1 passes — 2026-10-12

Row 1.3 of `docs/ASSET_SYSTEM_PLAN.md` (TOOLS-9, EVAL-3, the GA re-score, EVAL-4). Phase 1 is
closed: G1 passes on its first round.
- **Docs and rules (TOOLS-9).** Docs were half of arm A's `tok_x`, so `docs/ASSET_STUDIO.md` is
  now the agent reference: the loop, format 1, edits, check, sheets, placement and files in 1,684
  tokens (3,665 before; arm A's was 3,002). Starting the studio, connecting a client, the first
  prompt, frame feedback and verification moved to `docs/ASSET_STUDIO_LIVE.md`, which the Windows
  studio package also ships. `AGENTS.md`: the asset rule covers objects and raw downloads
  (`.cache/`, never .glb/.obj/.fbx/.png), and "Adding a tool" lists the six steps. The MCP
  instructions give the object loop (+13 tokens). The field guide adds z-fighting, vertex colour,
  procedural material, triplanar mapping and glTF. The plan's wording for the rule and the MCP
  clause also names `asset_import` and materials; those words are added when the features land
  (Phases 3 and 5).
- **Harness.** `bench.py calib`, `votes`, `judgment` and `report --vs`; `score` counts
  `lint_block`, `lint_other` and triangles, and an edit run needs 2 of 3 judges to answer `done`
  on a before/after image. Images are stacked with the standard library and judged under hashed
  names in one folder, so no path names the arm.
- **Calibration (EVAL-3): pass, first round.** 12 planted-defect pairs (the crate, bench and
  lantern templates and Gate-A P01, P07, P19; each defect twice), shown in both orders to three
  Opus judges at medium effort: 72 judgments. The intact asset won 10/12 pairs with it on top and
  11/12 with it below, with 1 order flip. The misses: a trestle table's top raised 10 cm (judged
  identical, both orders) and a brazier leg pushed up through its bowl (read as "coal specks").
  The judge prompt, which gained the edit-vote question before calibration, is frozen at sha256
  `4db5021c`.
- **GA re-scored with `check`:** 31 of 38 arm-A runs are accepted (37 before): floating parts in
  P17, P20, P21 (both runs), P24 and E6. All six arm-A edits got 3 of 3 `done` votes.
- **G1 (arm T = `132447c`, this row's first commit): pass.** 14 runs (S8 and E1-E6), all done and
  accepted (arm A: 11 of the same 14). Median `tok_x` ratio 0.739 (≤ 0.85), median `tok_cum` ratio
  0.877 (≤ 1), `lint_other` per accepted asset 0 (arm A 0.273); calibration passed; the Phase-1
  checks hold (7/7 planted lint defects, sheet framing 55-90% with the camera restored, exact
  triangle counts). Every edit got 3 of 3 `done` votes. Per task, `tok_x` ranged from 0.38 (E2)
  to 1.51 (P24) of arm A's.
- **What the runs show.** Arm T agents fixed every finding that edit replies reported, and looked
  through contact sheets. Most remaining waste is pretty-printed JSON sent to the line-based REPL:
  P03, P13 and E1 lost 12-18 commands each, the failure arm A also had. A sheet followed by
  `capture` shows the world, because a sheet leaves the stage as it was. Both are candidates for
  a later remediation or docs pass.
- **Incident.** The first G1 batch ran eight authors at once. Arm T agents piped commands
  through scratch files in `/tmp`, and three runs executed another task's asset. The whole batch
  was set aside (`.cache/asset_bench/discarded/`) and every run repeated one at a time; Gate A's
  logs show no crossing. The benchmark README now says to run authors one at a time.
- TOOLS-11 (the optional help diet) was not done; tools/list stays at 46,840 chars.

## Asset plan 1.2: contact sheets and render cost — 2026-10-12

Row 1.2 of `docs/ASSET_SYSTEM_PLAN.md` (TOOLS-6, TOOLS-7).
- **`asset_preview [name=] views=true|[front,right,back,left,top,iso,<degrees>] [elevation=20|game]
  [tile=240] [figure=true]`** returns one PNG. `views=true` is front, right, back and a 3/4 view
  (`iso`, at least 30° up): a 960×240 strip, ~315 image tokens; more than four views wrap to a
  second row (at most 8). Every tile has its own camera, fitted to points on the parts' surfaces
  (`PropPart::hull`: box corners, 16-point rings for cylinders, latitude rings for spheres and
  capsules), so round objects fill a tile as boxes do (about 80% of the larger side). Small
  objects narrow the lens instead of crossing the 0.1 m near plane. `elevation=game` is the game
  camera (56°, fov 40°). `figure=true` adds a grey 1.8 m figure (stage objects 42 and 43) past
  the object's back-right corner when its largest side is 0.5-20 m; otherwise the reply says it
  was left off. The PNG goes to `out/sheet-<name>.png`; the reply is `path`, `width`, `height`,
  `views` and `figure`. A `StudioCheckpoint` puts back the stage (selection, clock, turntable,
  mode) and the human's camera, pending world events are kept, and a sheet gets no live ticket.
- **`capture stats=true|only`**: `RenderStats` gains batches, triangles, mesh_bytes,
  custom_meshes, lights and materials (0 until procedural materials exist). `stats=true` adds them
  to the image meta; `stats=only` returns them with `render_ms` and encodes no image, headless or
  over `--live`.
- Validation (`crates/pav_tools/tests/asset_preview.rs`): six views each of five unlit test
  objects (wide, tall, round, flat, a 3.6 cm bead) span 55-90% of their tiles; camera, stage and
  world hash are unchanged afterwards; a warm 4-tile sheet takes under 1 s (about 80 ms in a debug
  build here, 0.56 s cold). The bench stage draws exactly the cost line's 2,004 triangles plus 37
  stage boxes counted from `MeshKey::generate`. The live smoke makes a sheet over the bridge
  (stage and camera unchanged) and reads `stats=only`: 13/13 checks pass.
- Decisions: `tools/list` stays under 47,000 chars (46,840) with the four new arguments by
  shortening `creature_edit`'s help (D13). The figure stands back-right, where the front, right,
  back and 3/4 views never put it in front of the object. Sheets take no `out=` argument (one file
  per asset) to keep the surface small. `pav_tools` gains `png` as a dev-dependency (already a
  workspace pin) to decode sheets in tests.

## Asset plan 1.1: a geometry lint, dry runs and a cost line — 2026-10-12

Row 1.1 of `docs/ASSET_SYSTEM_PLAN.md` (TOOLS-5, TOOLS-8). Format 1 is unchanged.
- **`asset_edit action=check name=X [target=WxHxD] [all=true]`** (read-only, BUILTIN too):
  `pav_core::props::lint` treats parts as oriented boxes (round shapes contained exactly), finds
  candidate pairs with an axis-aligned sweep and measures them with a 15-axis separating-axis
  test. Codes: `floating` (groups not joined within 0.5 cm to the floor; an asset whose top is at
  its origin hangs from it, and one on a wall anchors there), `below_floor`, `not_grounded`,
  `poke`, `hidden`, `z_fight` (coplanar overlapping faces in different surfaces, sampled where
  both faces really are and nothing covers them), `mirror` (left/right pairs compared as geometry,
  so a symmetric cylinder at roll 90° on both sides is a mirror image), `size`, `tiny`,
  `unit_suspect`, `default_color`, and from the renderer's meshes `heavy`, `meshes`, `lights`.
  Lines read `code: ids fact`, at most 10 then `+N more`; a clean asset has no `check` key. The
  `cost` line counts triangles from `MeshKey::generate` (the bench: 2,004, as the plan
  measured), distinct meshes, SDF parts, lights and generated KB.
- **Every accepted edit** reports the findings it introduced (compared by code and parts, so a
  moved part does not repeat its finding). **`dry_run:true`** on create, copy, patch or replace
  returns the same reply with `saved:false` and `revision_next`, writes and publishes nothing,
  and gets no live ticket.
- **Kind routing** is a table (`tools::KIND_ROUTES`): an unknown kind lists the kinds that tool
  serves; creature forwarding is unchanged; materials and meshes add rows later.
- Validation: the four templates and the dogfood well are clean and the stall shows its four real
  faults (as the prototype found); a bench with 7 planted defects gives exactly those 7 codes;
  256 parts lint within 10 ms (release). On the 44 arm-A assets it finds real faults (floating
  chain links and an anvil horn, buried cheese holes, voussoir fronts overlapping 2 cm² and more
  in different stone colours, light budgets); two false-positive kinds found there (rotated
  z-fight samples, symmetric mirror pairs) were fixed before landing.
- Decisions: `tools/list` stays under 47,000 chars with the three new arguments by shortening
  the `clip_import` and `mocap` help (part of D13's help diet, approved for after Phase 0).
  `lint4.py` was removed. The bench scorer now reads `check` and skips the floor codes for
  hanging and wall tasks; GA is re-scored with it in 1.3.

## Asset plan 0.3: the benchmark harness and the arm-A baseline (Gate A, G0) — 2026-10-12

Row 0.3 of `docs/ASSET_SYSTEM_PLAN.md` (EVAL-1, EVAL-2, G0). Phase 0 is closed: both of its gates
pass.
- **Harness** (`scripts/asset_bench/`, README there). `tasks.toml` freezes the 24 authoring
  tasks, 6 edit tasks and 6 import tasks of §10.1. `pav RUN` is a run's only way to the engine:
  one `pav repl` of the run's arm per call (cwd RUN, relative `props/`), replies unchanged,
  every command logged, refusals past the cap (30 commands and 6 images; 45/9 for the
  three-asset tasks). `bench.py` builds arms (release, arm A from a worktree at `a9ca003`,
  removed after), prepares runs (brief, the arm's own `ASSET_STUDIO.md`, a generated README),
  records runner reports, scores, collects and reports. `tok_x` reads `*_ms` fields as 0 and
  the run path as `RUN`, so `bench.py selftest` replays the planning stall session twice to
  equal numbers and the 31st command is refused. `crates/pav_core/tests/bench_holdout.rs`
  fails if a library leaf ever takes a task's name.
- **Agents.** `.claude/agents/asset-bench-author.md` (Sonnet, medium effort; Bash, Read,
  Write) and `asset-bench-judge.md` (Opus, medium effort; Read). This session started before
  they existed, and agent definitions load at session start, so every run used a
  general-purpose agent with model `sonnet`, effort `medium` and the author file's prompt
  body verbatim (plus "use only Bash, Read and Write"), recorded in each run as
  `agent=general-purpose+asset-bench-author-prompt`. A fresh session can launch them by name.
- **Gate A (arm A = `a9ca003`): pass.** 38 runs (24 tasks, the S8 tasks again, E1-E6): 38/38
  wrote `done.json` (≥ 85% needed), 37 accepted (P19's second cart sits 1 cm below the floor;
  floating parts are checked once `check` lands in 1.1, and the runs are re-scored then).
  Median `tok_x` 6,480, `tok_cum` 14,242, failed 0; first create valid in 37/38. Run-to-run
  spread over S8: median |ln(tok_x₁/tok_x₂)| = 0.257, below ln 1.5, so G2 runs each task once.
  The runner reported about 68-88k tokens per run (mostly its own context). Results:
  `results/runs.tsv`, `summary.json` and every final asset as text in `results/assets/GA/A/`.
- **What the baseline shows.** Arm A agents mostly write the whole asset in one `create`, look
  at one capture and stop (median 3 commands). Docs are about half of `tok_x` (arm A's
  `ASSET_STUDIO.md` is 3,002 tokens; HEAD's is 3,347), and arm A's verbose replies cost most in
  edit tasks (E2 22.6k, E6 29.1k `tok_x`, where inspecting a large asset dominates). Five runs
  lost 9-15 commands each sending pretty-printed JSON to the line-based REPL; that is the
  baseline's interface and is measured the same way in every arm. Risk for G1 (`tok_x` ≤ 0.85×
  A): with so few calls, the docs must not grow; the remediation round may trim them.
- **G0: pass.** The reply budget test, the 23-probe error fixture (19 located), golden template
  revisions, tools/list 46,926 chars (≤ 47,000), `scripts/smoke-asset-studio.py` 12/12 and the
  whole `cargo test`.
- Decisions: tiktoken is pinned at 0.12.0 (2025-10-06, the newest release at least 12 months
  old; doctrine 9). Briefs state needs in the user's terms ("it must give off light"), the same
  for every arm. The prototype bench files were removed; the stall replay is
  `scripts/asset_bench/selftest.cmds`.

## Asset plan 0.2: errors that name their place, a field guide, a leaner schema — 2026-10-12

Row 0.2 of `docs/ASSET_SYSTEM_PLAN.md` (TOOLS-2, TOOLS-3). Format 1 is unchanged; every file
that loaded before still loads with the same revision.
- **`pav_core::issue`**: a Rust port of SpawnForge's blueprint issues (`Issue {severity, path,
  code, message, expected, fix}`, rendered `path: message (expected …) — fix`; the
  optimal-string-alignment distance and `did_you_mean`). The vendored TypeScript is
  untouched.
- **Reading a prop** (`props/read.rs`): strict JSON first (a repeated key is an error with its
  path; serde_json would keep the last one silently), then one check of the whole document
  against field tables, reporting up to 20 problems, each with its path, the valid values and
  a fix: `parts.p.shape.height: unknown key (expected type, radius, half_height) — use
  "half_height":0.5`. Hints cover the usual guesses (colour, rotation, size, height,
  diameter, scale, material, cube/cone/torus, toon, colour names, `#f00`, numbers in
  quotes). The typed model is built only from a checked document; its old validation stays
  as a backstop. Range checks compare in f32, as the engine stores values.
- **Patches** (`props/patch.rs`) apply to the document's JSON and then run the same check, so
  a batch's errors read `ops[1] pad.color: "grey" is not a #rrggbb colour — use a hex colour
  such as "#808080"`; unknown ops, keys and parts suggest the nearest valid one. Batches stay
  atomic. `props.rs` became `props/{mod,read,patch,write}.rs` (the split §5.1 plans).
- **Tools.** Unknown actions, arguments, kinds and tools name the nearest valid one.
  `asset_edit action=schema [topic]` returns the field guide, generated from the same tables
  (`schema_covers_every_field` ties the tables to the typed model and the guide). The
  `asset_edit` ops schema lists only prop ops plus one `kind=creature` item (the merged
  creature ops with `path` are gone); merged creature arguments read "kind=creature only";
  the five asset tools' help is at most 36 words and each argument at most 12. MCP
  `tools/list`: 49,400 → 46,926 chars (11,525 → 11,062 tokens).
- **Tests.** The 23 dogfood probes are `crates/pav_tools/tests/fixtures/asset_errors.jsonl`
  with their exact replies: the 19 that fail on a part or an op name `parts.<id>` or
  `ops[i]`; two are valid input (an upper-case name is normalised, a part without a colour
  takes the default) and two are document-level (stale revision, existing name). Did-you-mean
  cases for actions, arguments, kinds and tools; tools/list ≤ 47,000 chars; no `path` among
  prop ops.
- Decision: the plan's TOOLS-2 check said every probe names a part or an op; four of the 23 are
  not part errors, so the plan now states the 19 that are.

## Asset plan 0.1: compact replies and a canonical prop writer — 2026-10-12

First row of `docs/ASSET_SYSTEM_PLAN.md` (TOOLS-1, TOOLS-4). No format change: every file,
revision and history head stays valid.
- **Numbers.** `pav_tools::reply::tidy` runs once in `tools::call`, so the CLI, REPL, MCP, both
  bridges and the native UI get it. A number that holds an exact f32 prints as that f32's
  shortest decimal (`1.17`, not `1.1699999570846558`) and integral numbers print as integers.
  It is lossless for f32 readers; genuine f64 values (timings) keep their digits.
- **Asset replies** (o200k tokens, before → after): inspect bench 1,463 → 390, inspect lantern
  1,214 → 302, `assets` with 4 rows 425 → 143, copy 341 → ~60 with a relative path, patch
  321 → 50, preview status 143 → 44, `studio_status` 179 → 60. Edit replies keep `name`,
  `revision`, `saved`, `size_cm` (`"220x143x86"`) and undo/redo when there are steps;
  create/copy add `parts` and `file`, other edits `changed_parts`; `refreshed_instances`,
  `history_reset` and `preview_error` appear only when they apply. Inspect leaves default
  fields out. The `assets` listing is `{cols, rows, total, matched, next}` with a new `page`
  argument; descriptions in rows stop at 80 characters. Preview status lists only controls
  that differ from a fresh stage. `studio_status` reports the object stage as name and
  revision, omits empty workspaces, and its feedback omits empty fields and the constant
  `measurement` sentence (ASSET_STUDIO.md states what the times measure).
- **Canonical writer.** `PropAsset::text()` writes a header line, the description, then one
  line per part in a fixed key order (shape, pos, yaw/pitch/roll, color, look, emissive,
  solid), leaving out only fields bit-identical to their defaults (negative zero is kept so a
  saved file always reads back to the same revision). Saved bench 2,772 → 923 bytes; the
  dogfood stall and well files 2,619 and 2,984 tokens (plan targets 2,619 / 2,982).
- **Tests.** Golden `prop1-` revisions of the four templates and the two dogfood files (moved
  to `crates/pav_core/tests/fixtures/props/`; F2 reuses them), writer round trip and
  idempotence, the reply contract, and `crates/pav_tools/tests/reply_budget.rs`: inspect bench
  ≤ 1,100 chars, patch ≤ 220, listing rows ≤ 120, preview status ≤ 160, no f32 noise.
- **Consumers updated** (D11): `asset_ui.rs` classifies replies by `saved`/`asset`;
  `scripts/smoke-asset-studio.py` reads listing rows and the trimmed preview status.
- Decisions: the opt-in `legend=true`/`bounds=true` on inspect (listed under TOOLS-3) landed
  here, because the inspect budget needs the legend out; `action=schema` follows in 0.2.
  Undo/redo counts are omitted when zero, as the plan's prototype did. `size_cm` uses whole
  centimetres from 10 cm up and tenths below.

## Asset system plan (proposal) — 2026-10-11

Planning only; no engine code changed. `docs/ASSET_SYSTEM_PLAN.md` is a self-contained, phased
plan for an LLM-agent-first asset system, written after mapping the engine, dogfooding Object
Studio, prototyping, and measuring public libraries. `docs/asset_plan/` holds its examples,
prototypes and fixtures.
- Verdict: go, with gates. Measured: format 2 cuts authoring tokens 68-71% on assets with a lot of
  repetition and 12-29% on small ones; Phase 0 trims replies 72-86%; procedural materials
  match CPU and GPU on 100% of pixels at about +19% frame time; Kenney Mini Dungeon
  translates at median silhouette IoU 0.959 and about 260 tokens per asset. Photoreal
  libraries only restyle.
- It extends Object Studio format 1 (v1 files keep their revisions), the asset tools, the mocap
  import pipeline and the renderer hooks; Creature Studio is untouched.
- Decisions approved with it (its §3): cm house unit with an explicit `unit`, flat part syntax,
  tool-written readable mesh and material text counts as data under P4, licence tiers, a lean
  benchmark with pinned subagents. Effort to execute: high, xhigh for Phases 2-3.
- During research one subagent sent the owner's email once as a crates.io User-Agent; the plan
  forbids personal data in request headers.

## Creature Studio: pinned SpawnForge creation in the native agent loop — 2026-10-10

The native window now has Animations / Objects / Creatures / Game workspaces. Creature Studio
creates a procedural creature from a theme, one of eight templates, or an existing blueprint.
The human can direct an LLM through the shared MCP bridge and see each accepted build in the
same native renderer. Manual controls call the same tools and keep their source revisions.

How it works:
- `tools/creature-compiler` vendors the production SpawnForge core and modules from commit
  `851880256987ecdb2895c6afd01f84df64199bdb`. Every source hash is checked before bundling.
  The Node worker uses a JSON-line protocol and has no browser or WebGL dependency. The
  catalog lists 81 modules and eight readable templates. The original code remains pinned;
  the new adapter owns the Shardfall conversion.
- Native format 1 carries bind bones, indexed mesh streams, four skin influences per vertex,
  linear vertex colours, bounds, sockets, warnings, and all available motion clips baked at
  30 frames per second. `pav_core` validates it without graphics dependencies. It samples
  local transforms, handles rest rotations and shortest-arc quaternion interpolation, and
  produces skinned native geometry through `pav_view`. Snapshots keep the exact definition.
- `creature_catalog`, `creature_edit`, `creature_preview`, and `creature_status` work through
  the CLI, persistent JSON-line sessions, MCP, and the live bridge. The shared asset tools
  also accept `kind:"creature"`. Named body sections, limbs, parts, palettes, and layers let
  an agent make a bounded change instead of rewriting a mesh. Inspect returns the current
  blueprint and revision.
- Authoring returns a queued job promptly. A persistent Node sidecar performs validation,
  generation, compilation, and motion baking away from the game loop. Ready results are
  validated and published as one change. Latest-request tracking and revision checks at
  acceptance, save, and publication prevent an obsolete build from replacing newer work.
  Failed and superseded jobs leave the last accepted source and visible definition intact.
  Worker transport has bounded replies, a timeout, and restart after transport failure.
- Accepted source lives in `assets/creatures/workshop`. Content-addressed native caches live
  in `.compiled` and the 32-step undo/redo history in `.editor` under the same root.
  `PAV_CREATURES` can move this workspace. Saves are atomic and use an exclusive file lock.
  New sessions can load a validated cache without starting Node. The native watcher queues
  missing caches for rebuild, reloads external changes after 250 ms of quiet, and retries
  transient lock contention.
- The Creature panel provides source search, theme/template/blueprint creation, copy, named
  edits, whole-blueprint edits, undo/redo, motion scrubbing, source-frame stepping, playback,
  turntable, and camera fit. An asynchronous reply cannot discard text typed after submission.
  External edits keep dirty drafts tied to their original revision. Undoing creation leaves
  an empty Creature stage with Redo reachable. Passive slider drawing cannot round and
  overwrite an agent's playhead or playback speed. The workspace bar remains available in Game.
- Each workspace retains its clock and camera. Compatible creature rebuilds preserve paused
  time and the view. Accepted live publications receive the shared `studio_status` ticket;
  a queued, failed, or superseded job does not claim a submitted frame. Native capture uses
  the same creature geometry and pose as the window.
- Palette or material changes compile geometry again because SpawnForge can use those
  fields for part colours or skeleton construction. Layer-only edits can reuse the last
  compiled geometry and motion, then bake new vertex colours. In particular, chitin changes
  invalidate the cache because that material can alter the skeleton.
- Source launchers build the compiler bundle before starting the app. The Windows package
  includes the pinned bundle, portable Node, a saved native demo, MCP configuration, examples,
  and the guide. Runtime lookup works beside the executable and does not need npm installed.

Native material scope: broad palette and surface layers are baked to vertex colours.
Fine relief, roughness maps, shader-only breathing and animated surface patterns are not
reproduced. Baked bone motion, including jaw and lid tracks, is retained. Fur shells are
omitted. Wing membranes are double-sided but opaque, and emissive glow is omitted.
The compiler reports these approximations. This milestone is an authoring and preview
workspace; gameplay spawning of generated rigs is a separate integration.

Verification:
- 117 distinct focused Rust tests pass across the app, core, tools, and view. Six Node
  compiler tests pass. All eight bundled templates pass native asset validation and
  independent rest-pose and baked-motion checks.
- Fifteen workflows pass against a real Linux window using software Vulkan. These cover
  creation, named and surface edits, baked motion, revisions, save/reload, undo/redo,
  invalid input, actual in-flight supersession, workspace state, and frame feedback.
  The packaged demo also opens and submits a native frame with Node unavailable.
- Five warm surface edits take a median 133 ms to return a queued job, 355 ms to build
  and save, and 544 ms from the request to native frame submission. Client confirmation
  with polling takes 690 ms. These software-renderer measurements exclude LLM time,
  GPU completion, and physical display latency; they are not Windows GPU benchmarks.
- Native app/tools and the Windows distribution build pass without warnings. The wasm
  app check passes. Windows PE import checks require only system DLLs for both programs
  and the portable Node runtime. Windows execution and browser execution were not
  available for this release check; the native studio workflow was exercised on Linux.

## Object Studio: reusable assets in the live agent loop — 2026-10-10

The studio now creates and edits reusable objects as well as animations. People give design
direction in their LLM client while the game window shows accepted changes. The first new
asset type is a prop made from named primitive parts. This fits the engine's procedural
renderer and text-asset rule, and lets an agent make precise changes without mesh import or
texture generation. Box, crate, bench, and lantern templates are embedded as readable JSON.

How it works:
- `props.rs` defines a versioned JSON format, named parts, strict validation, scaled bounds,
  and stable content revisions. Parts have a shape, transform, colour, surface, glow, and a
  collision flag. One object can have up to 256 parts. No new dependencies were added.
- `assets`, `asset_edit`, `asset_preview`, and `asset_spawn` work through the CLI, JSON-line
  REPL, and MCP. Inspect gives the full definition and field guide; edit replies stay small.
  Related `set`, `add`, and `remove` operations validate and publish as one batch. Revision
  guards reject stale edits. Atomic saves and 32-step undo/redo use `assets/props/workshop`
  and `.editor` (`PAV_ASSETS` overrides the root).
- A placed prop is one root entity with an embedded definition. Its parts use one compound
  collider. Edits refresh active and sleeping copies, with each root's placement intact.
  Existing physics handles stay stable while collision remains enabled, so room joints stay
  attached. Selection, streaming wake, and navigation use the whole object's bounds.
- Room object records preserve asset references and scale. Full snapshots embed exact
  definitions, so later library edits cannot change an old snapshot. Normal hero saves do
  not store level layouts. Removing a source leaves placed copies intact.
- The native Objects panel calls the same tools as agents. It provides template search,
  create/copy, named-part controls, JSON batches, placement, and undo/redo. Unsaved drafts keep
  their original revision after an external edit. Undoing creation leaves Redo reachable.
- Animations / Objects / Game share one window and bridge. Each preview keeps its subject,
  clock, and camera; the game state stays paused behind the active stage. Existing animation
  startup and tools remain available. `--asset-studio` (or `--studio`) opens the new stage;
  an explicit `--scene` remains behind it.
- Direct tool saves publish immediately. Native file watching waits for 250 ms of quiet,
  retains the last valid definition on errors, and detects recovery. Startup loads authored
  objects once; catalog reads cannot consume watcher updates. A sim-thread revision check
  rejects a queued file update if a newer edit was already published.
- `studio_status ticket=N` reports when the native window submitted a frame containing an
  edit and its camera changes. Tickets are independent of world ticks and content revisions,
  so paused edits and undo work. Replaced pending tickets report `superseded`. The path skips
  interpolation across edits and does not capture an image. Timings end at frame submission,
  not GPU completion or physical display.
- Bridge requests adopt their camera changes in order, including requests from several
  clients. A manual studio command first adopts any earlier bridge result. This stops delayed
  camera changes from overwriting a later human edit. A new ticket cancels old camera blends
  and uses the current focus on its first submitted frame.
- Source launchers, `.mcp.studio.json`, a runnable lantern example, a Windows package script,
  and a real-window MCP smoke script are included. The browser still builds and can show the
  procedural stage; native file editing and the local bridge are desktop features.

Scope: this adds procedural object assets and scene placement. Imported mesh models, bitmap
textures, and a general level editor are separate work. The reusable definition, revision,
preview, and placement interfaces provide a base for that work.

Verification: 100 focused Rust tests passed (63 core, 18 tools, 19 app), including existing
animation and playable-character regressions. The real native window passed 12 object and
10 animation workflow checks through MCP. The 20-edit sample used Vulkan llvmpipe software
rendering at 1600 x 900, with Cargo idle: median apply/save 2.04 ms, RPC return 292 ms, tool
start to frame submission 580 ms, and client-observed confirmation 898 ms. The last measure
includes status polling. These are software-renderer results, not measurements on the
user's Windows GPU. Captures were outside the timing sample.

## Two new playable characters and the Hall of Heroes — 2026-10-10
Shardfall had one hero, the Wanderer, moving with the engine's procedural animation. Now there
are three playable characters (`game/heroes.toml`), each with a look and a way of moving of their
own, chosen in a new Hall of Heroes in Emberwatch:
- **Kestrel the Brawler**: compact and broad, bare-chested and barefoot, white gi trousers and
  hand wraps, a red sash, a black mohawk (the crest part), iron hooks (claws). She fights in
  captured boxing and karate: a boxer's guard for an idle (CMU), a swaggering strut (100STYLE),
  a sprint (Quaternius), a jab, cross and roundhouse kick for the three swings of Slash (two
  Quaternius clips and a CMU take), a two-handed chi blast for Fireball, a ground pound for
  Earthsplitter, an angry outburst for War Cry; the procedural fist and kick moves for the rest
  (a leg sweep for Cleave, an axe kick for Leap Slam, a flying kick for Blade Dash, an uppercut,
  a haymaker, a knee); a captured shoulder roll for a dodge; falls on her face; a fist pump.
- **Ysolde the Stormcaller**: tall and slender, an indigo robe, hood and cape, gold at the
  waist, pale with glowing eyes, three motes of light circling her (orbs), a staff that keeps its
  glow whatever staff she holds. Spellwork in captured motion: a spell held ready for an idle, a
  graceful walk (100STYLE), a light jog, a hand driven forward for Fireball, a two-handed blast
  for Chain Lightning, shards hurled overhand, rising to call a Meteor down; quick staff strikes
  (thrust, backhand, rising cut) for Slash; a forward glide, arms raised in a V, for a dodge; a
  long dramatic fall; a salute when a boss falls.
- **The Wanderer** is unchanged (procedural animation) and stays who a new game starts as.

How it works:
- **Engine (the puppet API)**: `PuppetDef.attack_clips` takes a list per skill (one clip per
  swing of its combo; a string still works); new `attack_moves` (skill -> a move of
  anim/moves.toml in place of the skill's own, or a list per swing; a single move keeps
  alternating sides, a list plays each as written) and `dodge_clip` (a captured dodge played once
  per dodge in place of the procedural tumble, sped up to be done 0.3 s after the roll, on any
  biped: Shardfall's dodge and the committed model's roll). `PerSwing` is the one-or-a-list type;
  `moves::attack_move` / `clips::attack_clip(def, skill, combo)` pick the swing; the game's cast
  animation passes the combo index. Data checks cover moves as well as clips.
- **Characters** (`arpg/data.rs` `CharacterDef`, game/heroes.toml): name, title, about, look
  (puppet fields, so the animation kit lives there), starting weapon, gear and bar, captured
  deaths, level-up and boss gestures, and a showreel. Checked on load (clips, moves, skills, item
  bases, slots). `Hero.character` names one (`""` = the first, so old saves are Wanderers);
  `Hero::new_character` builds a level 1 hero in their kit. `refresh_hero` dresses the
  character's look (not the scene's puppet) in the gear; a character's own cape and weapon glow
  stay under any armour.
- **Separate heroes, shared stash**: `Game.roster` holds the heroes of the characters not being
  played. `hall::switch` (the `Character` game command, in town) parks the current hero and
  brings the chosen one forward (a new one at level 1 the first time), healed, with the stash
  (its items renumbered for the new hero's bags); the vendor restocks. The save file carries the
  roster (`roster`, absent in older saves).
- **The Hall of Heroes** (`arpg/hall.rs`): a pedestal per character along the south side of the
  square, their name on the floor, braziers at the ends. Each character stands on theirs
  performing a showreel built from their kit: every swing of each skill as they strike it (their
  captured clip or move), clips and moves; their idle clip shows between. Using a pedestal opens
  a card (who they are, how they move, where their hero stands) with "Play as ...".
- **Tools**: `hero character=KEY` switches from anywhere; `game_cmd do=character name=KEY` as
  the player would; `turntable character=` / `animsheet character=` show a character in their
  starting kit; `animsheet skill= swing=N` shows a combo's later swings; `hero`/`game` report
  the character and who waits in the hall.
- Tests: `tests/heroes.rs` (the roster and the looks differ, the hall's performers play clips
  and moves, progress kept and stash shared across switches, the roster round-trips through a
  save, Kestrel's jab-cross-roundhouse and sweep, Ysolde's staff strikes, the Wanderer's own
  slash, captured dodges replace the tumble and play once, one-or-a-list parsing).

Decisions:
- **Characters are separate heroes** (own level, gear, passives, waypoints), as in the genre,
  rather than looks on one hero; the stash is shared so loot can go to whoever needs it. A new
  character starts at level 1; `hero character=KEY level=N` jumps ahead for testing.
- **Skills are shared**: any character can use any skill; a character changes how a skill looks
  (their clip or move for it), not what it does. Their starting bar suits them.
- **Worn items colour the look**, as before; the new characters start without gloves or boots so
  Kestrel's wraps and bare feet show until real gear replaces them.
- Rejected: a captured cartwheel or backflip as a dodge (they travel sideways or backwards while
  the dodge faces forward); a levitating idle (the clip reads as a crouch on this rig).

## Animation Studio: shared live editing for people and LLMs — 2026-10-10
The user asked for a tool that an LLM can use to view, alter, and create animations while
the user watches each change in the running engine.
- **One shared stage:** `SimState.animation_preview` holds selection, time, playback speed,
  and pose options. The game, physics, recording, and rewind history stay still while it is
  open. The stage uses the normal puppet sampler, view builder, and wgpu renderer. Closing it
  restores the game and camera. The preview clock works even if the game was already paused.
- **Same API for the UI and agents:** `anim_preview` opens clips and procedural moves, plays,
  pauses, scrubs, steps, and reports poses, named joints, and bounds. `anim_edit` creates clips
  from rest, copies source motion, updates partial or full poses, removes keys, retimes,
  mirrors, and performs undo/redo. Both tools work through the CLI, MCP, and the live bridge.
  Captures and filmstrips use the same rendered stage. REPLs also accept full JSON request
  lines, so nested pose arrays and spaces do not need shell quoting tricks. Batch files can
  use `--stop-on-error` to stop before later edits after a failed create; interactive REPLs
  keep their existing error recovery.
- **Files and history:** editable clips go in `anim/workshop.json`; copies of restricted
  sources stay in `anim/local/workshop_local.json`. Original clips and source credits remain.
  Each edit validates and saves with a file lock and sibling-file replacement. Revisions
  reject stale writes; per-clip history stores 32 undo/redo steps across process restarts.
  External file changes invalidate stale history. The next accepted edit starts from the
  changed file. Undo of creation removes the clip; redo can restore it.
- **Changes appear live:** the preview reads the active library on each frame, including
  while paused. Editing the selected clip keeps its time, controls, and camera. Native file
  watching replaces only the changed set after a 250 ms debounce. Invalid files leave the
  last valid live data in use and show an error. Replacement shares unrelated clip sets,
  which keeps large imported libraries cheap to retain during edits. Playback sliders only
  write when edited: painting a rounded value must not overwrite a newer agent command.
- **Desktop workspace:** `--animation-studio [SET/Clip]` opens the stage and local live bridge.
  Esc → Animation Studio also opens it during a game. The egui controls search, select,
  create, copy, play, scrub, step, edit key channels, retime, mirror, undo, and redo. F6–F9
  control the preview while it is open. Sandbox editing and game input are held during the
  preview. The browser can view the stage; file authoring and the local bridge stay native.
- **Handoff:** `.mcp.animation-studio.json`, Linux and PowerShell launch scripts, a readable
  wave example, a Windows ZIP packager, and `docs/ANIMATION_STUDIO.md` cover setup and use.
  The original `.mcp.json` still starts an independent headless session. Use `mcp --live`
  when an LLM must change the visible window.
- **Scope decision:** this is a biped motion-clip editor built into the existing engine.
  It also previews procedural moves; those remain editable in `anim/moves.toml`. No second
  renderer, hosted LLM service, API-key setting, binary animation format, or dependency was
  introduced. Existing imported interpolation tails are normalized only when copied for
  editing, by keeping the sampled pose at the source clip's exact end time. On Windows,
  image tools try DirectX 12 if Vulkan adapter or device creation fails, matching the
  window's supported graphics paths.
- **Verification:** 70 selected tests pass across core motion/editing, tools, the CLI, and
  the app. The native app/tools build and browser target check pass. The real desktop/MCP
  smoke test passes all 10 checks under Xvfb and Vulkan lavapipe: creation, visible edits
  while paused, revision rejection, undo/redo, batch error handling, file reloads, the wave
  example, filmstrips, reopening saved motion, and closing the preview. Rendered poses,
  the filmstrip, and the actual studio window were inspected. CPU regressions cover safe
  stage IDs through the normal view builder and playback controls that emit no edits when
  painted without input.

## Progress log split, a wider replay check, Pavilion Lite frozen — 2026-10-09
Follow-ups to adopting the doctrine.
- **Startup reading** (principle 1, token efficiency): every session read about 150 KB before
  starting (this journal alone was 100 KB). `docs/PROGRESS.md` is now one page of current state,
  and this journal became `docs/HISTORY.md`. Agents read the doctrine, AGENTS.md and
  PROGRESS.md (about 40 KB); DESIGN.md and GAME.md when the work touches the design.
- **Replay check** (principle 5): `Sim::state_hash` covered only the tick, entity positions and
  rotations and the block count, so a replay could report a match while life, items, gold or
  the random numbers had diverged. It now hashes the random numbers, every entity, rigid and soft
  bodies (pose and velocity), projectiles, courses, signals, crumbling tiles and the whole game
  (the same CBOR as snapshot files; the state keeps its maps ordered, so the bytes repeat).
  `tests/arpg.rs` checks that gold, potions and the random numbers each change it. Replays
  recorded in one process still match in another (arena, playground, world, soft bodies,
  destruction, stress, test).
- **Pavilion Lite frozen** (principle 6): its renderer runs on the CPU, not WebGPU. It stays as a
  finished experiment and handoff package and isn't developed further (`docs/ESCALATIONS.md`).

## Doctrine adopted — 2026-10-09
`docs/DOCTRINE.md` (the user's agent-first engine doctrine) now sits above DESIGN.md and
AGENTS.md. Nothing in the code changed; AGENTS.md says how each principle applies here.
- **Kept as working rules** under it: gameplay first, use tools, run well on a modern PC,
  approximations that feel right, check selectively, token efficiency.
- **Dropped** "never paint the engine into a corner; keep options open": the doctrine says to
  solve problems when they surface, not in anticipation.

## Decisions (doctrine)
- **Mid-development adoption:** existing code isn't rewritten to fit; mismatches are fixed when
  they get in the way. The two known ones (UI invisible to the tools, a replay hash that covers
  only positions) are listed in AGENTS.md.
- **Versions (9):** current pins are kept; the 12-month rule applies to new dependencies and
  upgrades. wgpu, rapier and cpal are internals anyway; glam and egui appear in game code.
- **Platform (5, 10):** the user confirmed their Windows `.exe` and the browser count as part of
  the target. Cross-machine replays aren't promised; the user judged the risk too small to plan
  for, and the change (software maths, rapier's `enhanced-determinism`) is mechanical if needed.
- **No escape hatch (8):** game code reaches internals only through the API; gaps are filled by
  extending the API.
- **API (7, 8):** the 18 engine modules `pav_core/src/arpg/` uses are the API; the rest is
  internals. The game isn't split into its own crate yet.
- **Escalations** go to `docs/ESCALATIONS.md`.

## Mobile: touch controls, a pared-down HUD, swipes and performance — 2026-10-09
The browser build had no touch input at all (winit turns a finger into `WindowEvent::Touch`,
which nothing read, so on a phone neither the game nor its windows responded) and drew the desktop
HUD at full device resolution. Now phones and tablets are a first-class device (`Device::Touch`,
detected from `(pointer: coarse)` at start or from the first finger):
- **Controls** (`pav_app/src/touch.rs`): every finger is routed when it lands: the corner
  buttons first, then windows (egui gets it as its pointer, the first finger only, plus every
  finger as a touch for pinches), then the on-screen buttons, then swipeable things, then labels;
  anything else is the move stick. The stick is invisible until a thumb lands anywhere on the
  open game; a faint ring then shows where, and it follows a thumb that runs past its edge.
  The right thumb has a fan round the corner: dodge (the big button), skills on two arcs (tap:
  at the nearest foe it can reach, or the nearest at all for missiles; drag: aim by hand with a
  ring and dots on the ground, let go to cast, drag back onto the button to cancel; hold: keep
  casting; channels last while held), the potion with its charges, and a "use" pill named for
  what is in reach (Trade, Travel, Descend, Open...). Top right: the menu, the bag (a badge
  counts unspent passive points) and, when the minimap was swiped away, a map button.
- **Auto-attack** (`touch::pick_target`, a setting, on by default): the first skill strikes the
  nearest foe in its reach (`skills::reach`, now shared with the bot; `SlotHud` carries reach,
  range, channel and ground-target) unless the player is running away from it; with the stick
  still, the hero steps in to a foe that is fighting it a little out of reach. Its button is
  hidden; with auto-attack off it takes the corner and dodge moves to the bottom row. All of it
  rides in the input frame, so replays and the simulation are untouched.
- **Less on screen** (`arpg_ui::hud` on touch): slim life, mana and experience bars with a level
  badge in the top left (no orbs, no skill bar, no key letters), the level's name under them, a
  smaller minimap under the corner buttons, banners placed for a short screen. No boot panel, FPS
  overlay, key hints or keyboard guide; a short "Playing by touch" card instead, until swiped.
- **Swipe away** (`touch::Swipes`): toasts, the arrival banner, a level's intro (now with its
  mechanics), the station guide's pad notes, the tips card and the minimap follow the finger and
  fly off when swiped; they stay away until their content changes (the minimap until the map
  button). A tap on the minimap opens the big map; a tap or swipe closes it. A tap on the open
  game closes what is open over it (menu, windows, map, guides), one at a time.
- **Windows for fingers**: bigger hit targets and text (`ui::touch_style`, only while on touch),
  windows that fit the screen below the corner buttons and scroll by dragging; in the bag, shop
  and stash a tap shows the item's card (what it is, what it changes, and buttons: Wear, Wear on
  the right hand, Sell, Stash, Drop, Take off, Buy, Buy back, Take) instead of acting blind; the
  passive tree shows a tapped node with Take / Take the path / Refund / mastery buttons (a second
  tap takes it) and zooms with two fingers. A pause menu made for phones: Resume, Town portal,
  Auto-attack, Graphics (Auto/Low/Medium/High), Show FPS, Full screen, the rest under "More".
- **Performance**: `Renderer::render_scale` draws the scene below the window's resolution and a
  bilinear pass (`pav_render/src/upscale.rs`) stretches it over the window, so the UI stays sharp;
  `max_shadow_lights` caps point-light shadow casters (six depth passes each). `quality.rs`:
  Auto is High on a desktop (unchanged: every pixel, every effect) and Medium on a touch screen
  (about a megapixel, one shadowed lamp, no screen-space GI or sun shafts) with dynamic
  resolution (slower than 50 fps drops a step, a few smooth seconds raise one); Low halves the
  pixels and drops haze, halos, bloom and distortion. Touch screens draw at most 60 frames a
  second. Saved with auto-attack and "tips seen" in `shardfall_prefs.json` / local storage.
- **The page**: no browser zoom, scrolling, callouts or tap flashes over the game; `100dvh` so
  the canvas follows a phone's browser bars; the first tap goes full screen and turns sideways
  where the browser allows it (Android); a web manifest (full screen, landscape) for "Add to
  Home Screen"; a hint to turn the phone sideways that fades. Held upright, the camera's field
  of view becomes the width's, so the sides are not a sliver.
- Checked in headless Chromium with phone emulation (844x390, touch, `pointer: coarse`) driven
  by CDP touch events: the stick walks, the bag opens, a tapped item shows its card, a tap
  outside closes it, the menu, tips, intro banner and minimap swipe away, the map button brings
  the minimap back, auto-attack fights an arena wave, a hand-aimed skill fires where aimed.
  Unit tests: routing, auto-attack targets, swipes, a quick tap clicking in egui, quality scale.
- Not done: no real phone was available (SwiftShader only); Chrome's DPR emulation sizes the
  canvas at CSS pixels (a real phone gives CSS x DPR), so layouts were checked at DPR 1. No
  camera rotation or pinch-zoom of the world on touch (the camera is fixed in the game anyway).

## Animation: every open library through the engine's own tools — 2026-10-09
The follow-up that brings the rest of the libraries in, and the formats they ship in:
- **The whole CMU database, natively** (`mocap/cmu.rs`, a port of cmu.mjs's survey): `mocap
  get=cmu` downloads the site's 1.08 GB archive and unpacks every take (2,514 from it, 32 more one
  by one: 2,548); `mocap survey=ledger` measures each into anim/cmu/takes.tsv (length, where it
  moves, fit, travel, hip heights, flags) and `mocap survey=library` translates them, a set a
  subject, into anim/cmu (10.7 hours of motion, 4,770 clips, about 80 s each). Run over the
  whole database, all 2,548 ledger rows and all 113 sets came out identical to the committed ones
  my-3D2dge's tool made; the files differ only in the legend's line for `speed`, so anim/cmu is
  now the native tool's output.
- **100STYLE's other gaits** (`anim/100style`, on disk, `clips load=100style`): runs, backward
  walks and runs, sidesteps and sideways runs each way, and idles for every style: 780 loops, 9,779
  key poses, 2.8 MB, catalogs `anim/catalogs/100style_*.json` written from the dataset's lists.
  The cutter learned what they need. A loop can go a way (`way=back|left|right|forward`): every
  straight pass of the take going that way of where the hips face, never doubling back, is
  searched, and the loop is cut from the pass that holds the cleanest cycle, the body facing
  forward. A turn inside the cycle costs too, so it is not taken just after the performer comes
  round. Sidesteps swing the legs out, backward walks fore and aft, every body faces forward
  (median turn 0); the four styles that drag the leg they would lead with are left out, with the
  reason in the catalog. The forward walks, CMU and Mesh2Motion sets come out byte for byte as
  before; CMU's Walk_Backwards_Loop now steps back facing forward (`way` in its catalog line).
- **Quaternius' Universal Animation Libraries 1 and 2** fetched from itch.io and translated
  natively from version 2 (June 2026): 102 clips. `rm=` reads each file's root-motion twin: loops
  take their speed from it, clips that travel are added as `<clip>_RM`.
- **FBX** (`mocap/fbx.rs`): binary FBX 7 (Mixamo, Blender, Maya, Unreal exports): the node tree
  (zlib arrays), the skeleton with the SDK's transform (pre- and post-rotation, pivots, offsets,
  six rotation orders, scale), each animation stack's curves sampled into a take, z-up files
  turned y-up. `clip_import from=x.fbx` makes every stack a clip. Quaternius' own FBX of Library 1
  agrees with the glTF translation of the same clips (median 1.4 degrees off; the rest is where
  each route roots and faces a clip). `tests/mocap.rs` writes an FBX file and reads it back.
- **Rigs:** Mixamo's (MotionBuilder names, `mixamorig:` or not) for glTF too; Bandai Namco's;
  Unreal and Rigify names for BVH and FBX. Exporters that key every bone's translation, and
  skeletons whose zero pose is not a body standing (MotionBuilder lays bones along x), are read:
  position channels place a joint outright, and the body is measured in the frame where it
  stands straightest, every take of a library sharing the take that stands best. A bone end the
  file gives no length gets a typical one. A glTF file with more than one skinned mesh measures
  its tips on all of them.
- **Non-commercial libraries, translated for this machine** (git-ignored `anim/local`, `clips
  load=local`; never committed): the Bandai Namco Research motion dataset 1 (`$library: bandai`,
  175 takes in 15 styles: walks, runs, dashes and backward walks looped, gestures, fights and
  dances whole; 13.6 mm), LaFAN1 (`$library: lafan1`, a loop out of each of its 18 walks, runs and
  sprints, read one take at a time out of its 144 MB archive), and Mixamo downloads (.fbx or
  .glb).
- **Tools:** `mocap` describes seven libraries; `mocap get=` fetches CMU, 100STYLE, Bandai Namco
  and LaFAN1 takes; `mocap cut ... way=`; `clip_import` reads .fbx, `log=N` for all its notes,
  `way=` and `whole` (a take that is one cycle already) in catalog picks; a catalog import leaves
  a pick it cannot cut out, with the reason, rather than failing.
- **Monsters move in captured motion** (humanoid ones: the clips are human). A look's `run_clip`
  takes over from its `walk_clip` above the pace between them, so a monster shambles while it
  wanders (at its walk's own pace) and runs when it gives chase, its stride matched to the
  ground either way. `attack_clips` names a captured attack per skill, played over the skill's
  move and timed so the clip's strike (the moment a hand, or the blade's tip, reaches furthest
  ahead; `clips name=` shows it) lands on the hit, at near its own speed. `death_clip` is a
  captured fall: down within about 1.2 s, the body sinking and clearing after it. The ghoul
  sways hunched, shambles, runs, rakes with both hands and is thrown off its feet; the
  bonecrusher struts, brings its maul down overhead or sweeps it low; the Hollow King marches,
  slashes and splits the earth with captured swordplay. Generated bipeds draw a gait (a walk
  and its run), an idle, a fall and their skills' attacks from the genome (`game/genome.toml`),
  out of a stream of their own so every other gene stays as it was. Game data names only clips
  that exist (checked on load); `animsheet skill=` shows the captured attack.

## Animation: the engine translates open motion libraries itself — 2026-10-09
my-3D2dge's raw-capture importer (tools/anim-import.mjs, asf-amc.mjs, cmu.mjs and the encoding
half of readable.js), ported into `pav_tools::mocap`, so the libraries come in through this
engine's own tools instead of through the other engine's conversions:
- **Readers.** glTF binary (`glb.rs`: accessors, node hierarchy, skinned mesh for the tip bones,
  linear/step/cubic-spline tracks; Rigify `DEF-` and Unreal-style rigs), CMU's Acclaim files
  (`acclaim.rs`: the skeleton's axes and degrees of freedom, M = M_parent · C · R · C⁻¹), BVH
  (`bvh.rs`: hierarchy, channels, forward kinematics; bone maps for 100STYLE and the
  MotionBuilder names Mixamo and LaFAN1 use (that one tested on a synthetic skeleton only), as
  tables with fallbacks; units found from the legs).
- **Cutting takes** (`takes.rs`): a stretch of seconds; a loop at its best cycle (pose and speed
  match, a span that keeps moving), played in place facing the way it walked and closed exactly;
  the rest pose stood on the floor the takes stand on. New: a loop with no stretch given finds the
  take's longest straight run (every hip position within 12 cm of a line) or, for an idle, its
  longest still stretch, inside the frames a dataset marks as the style.
- **Encoding** (`readable.rs`): the body's rest measurements and spine shares, key poses from
  captured points, decoding on the captured body, Ramer–Douglas–Peucker keys and the fit that
  adds the frame the in-betweening misses most. Ported step for step: f64 in the reference's order,
  captured points stored as f32 like its Float32Arrays, fdlibm's trigonometry (`libm`, as V8),
  V8's `Math.hypot`, JavaScript's rounding and `toFixed`.
- **Verified number for number.** The three Mesh2Motion GLBs → MESH2MOTION: 177 clips, all
  119,432 numbers identical to my-3D2dge's own run, the rest bodies too (2.3 s against its 5.4 s).
  The CMU catalog → CMU: 60 clips from 25 subjects, the takes downloaded by the tool, all 104,101
  numbers identical. `tests/mocap.rs` keeps it so: a small Unreal-style rig and a CMU take
  (fixtures, with my-3D2dge's output for them).
- **Downloads** (`fetch.rs`, through curl, so cloud proxies just work): CMU takes from
  mocap.cs.cmu.edu, Mesh2Motion's GLBs from GitHub, and single files out of a zip archive on the
  web by ranged requests (100STYLE's 1.5 GB archive: one take costs its own bytes). Into
  .cache/mocap, git-ignored.
- **A new library: 100STYLE** (one performer, 100 styles of walking, CC BY 4.0). Its forward walks
  cut and fitted into STYLE100: 98 styles (the two spins left out: they cannot loop in place),
  1,550 key poses, 12 mm off on average, 440 KB, the dataset's own descriptions. The catalog
  (`anim/catalogs/100style.json`) was written from its Dataset_List.csv; picks play inside its
  Frame_Cuts.csv. A walk's shortest cycle is 0.8 s (a whole stride), so a loop never leads with
  the same foot twice; hops and two-footed jumps repeat after one.
- **Loops know their speed.** A clip's `speed` (percent of standing hip height a second, how far
  the cycle carried the hips) is new in the format (the legend says so; other readers ignore it).
  `walk_clip` on a look plays such a loop while the character heads forward on the ground, at its
  ground speed over the clip's own, so the stride matches the ground (`clips::walk_rate`, `pace`).
  The CMU set was rebuilt by the native importer to add speeds to its loops: nothing else changed.
- **Tools:** `clip_import` reads .glb, .bvh, set files and catalogs (`$pick` cuts from CMU or
  100STYLE, downloading the takes; `list=true` shows a file's rig and clips); `mocap` describes the
  databases, finds takes (CMU's ledger, 100STYLE's styles and descriptions), downloads them and
  cuts one moment into a set (`mocap cut=13_17 at=1.5-2.6 name=Jab set=MINE`). Catalogs for every
  embedded set in anim/catalogs/.
- **Where it shows:** a new station, **Walk Styles** (sixteen walkers on lanes, each in its style
  at its own pace; station guide), and Shardfall's villagers: Tomas strolls with his hands
  clasped behind his back, Elsie bounces along. NPCs can now move as slowly as an old man's
  shuffle (2% of full speed). New field-guide words: gait cycle, foot sliding.
- Not done: FBX (RancidMilk's CMU retargets; convert to glTF first); a Mixamo map for glTF;
  100STYLE's other gaits (backward, sideways, running, idles): `mocap cut=Old_FR loop=true` cuts
  any of them; the whole-database CMU survey (the ledger, anim/cmu's 4,770 clips) is still the one
  my-3D2dge's cmu.mjs made, while single takes and catalogs go through the native tools.
  (All of it done in the follow-up above.)

## Animation: moves as data, and motion clips from open libraries — 2026-10-09
Compared the puppet with my-3D2dge's (the same approach: a skeleton posed by math, two-bone IK,
no animation files) and took what it does better, rebuilt on this engine's foundation (pure pose
functions, state saved with the simulation, tools):
- **One skeleton.** The biped is now a `Skel` (pelvis, chest, head and their frames, shoulders,
  elbows, hands, hips, knees, ankles, toes, weapon direction). `procedural` builds it from the
  walk, crouch, crawl, climb, swim, air, hits and the current move; `clips::over` lays clips on
  it; `dress` turns it into parts (gear, helmets, capes and attachments follow its frames).
- **Moves as data** (`anim/moves.toml`, `moves.rs`): 31 moves, each an arc the hand or foot
  sweeps around the shoulders (from/to degrees, height and reach in arm lengths), timing (wind,
  active, recover, where the hit lands, hold) and body motion (lunge, hop, crouch, lean, twist,
  spin). The nine original actions keep their names and order (slash, overhead, thrust, cast and
  throw now take my-3D2dge's numbers); its other moves were converted to these units (radians to
  degrees, its units to arm lengths, hip heights and metres; its spin is `spinslash` here, ours
  stays the whirlwind); plus stir, stir_back and flick for townsfolk. A skill's hit lands where the skill says; the phases stretch
  around it. `PuppetState::set_action` remembers where the hands were when a new action starts
  (`chain_*`), so chained attacks wind up from there. The whirlwind now turns the body; kicks lift
  the foot through its wind-up; the weapon and its trail follow the arc. `MoveId` is written by
  name in data (an unknown name lists the moves).
- **Legs follow travel** (`PuppetState::travel`), **idle breathing, weight shift and blinks**
  (each character on its own beat: `seed`, from its id on the first tick).
- **Motion clips** (`clips.rs`): the readable key-pose format, kept as is so clips move between the
  engines (hips in percent of standing hip height; body/chest/head turn, lean, tilt; shoulder
  reach and shrug; each limb a direction plus a bend and a twist; feet; blade; root travel).
  Decoding is a port of the reference decoder onto the puppet's own proportions: no source body
  is needed. Checked against it: every limb within a few degrees on sword, dance, breakdance and
  death clips (the spine is one segment here, so a curled spine differs by up to ~14°; lying
  down the thicker torso keeps the pelvis higher). Clips fade in and out over 0.25 s, the old
  one held under the new; `UPPER` keeps the walking legs, `MIRROR`, `TRAVEL`, `ONCE` (fade out at
  the end; any action ends it). State holds only ids and times, so rewind and replays are exact.
- **The libraries, translated with tools.** `clip_import` reads a my-3D2dge set script or a
  readable set (or a folder of them), writes `anim/<set>.json` with the legend and credits,
  checks it reads back and reports the fit. Embedded: QUATERNIUS (88 clips, Universal Animation
  Library 1 and 2, CC0), MESH2MOTION (177, CC0), CMU (60 curated moments, free for all uses):
  325 clips, 6,469 key poses, 1.9 MB. On disk only: anim/cmu/ with every take of the CMU database
  (113 subjects, 4,770 clips, 241,740 key poses, 68 MB, ledger `takes.tsv`), `clips load=cmu`.
  Decision: readable key-pose text counts as data, not an asset file (AGENTS.md updated).
- **Tools:** `clips` (list, find, show as readable text with source and licence, load a folder),
  `clip_import`, `anim_reload`, `animsheet move=` / `clip=` (`mirror`, `upper`, `travel`, `hit`,
  `side`, `def`).
- **Where it shows:** room NPCs perform (`clips = [...]`, `moves = [...]`, `hold`, `mirror`,
  `upper`, `tempo`); any look can have an `idle_clip`; `anim.tempo` and `anim.mirror` settings.
  New stations in the animation wing: **Motion Library** (seven performers: dance, fight, sword,
  everyday, acrobatics, falls, monsters; tempo and mirror pads) and **Action Moves** (blades,
  fists, kicks, magic and more). Shardfall: the hero dies with a captured fall (a different one
  each time) and lies still until rising; cheers on level-up and when a boss falls (upper body,
  never blocking); villagers idle like people when they stop; townsfolk bow when greeted.
  Monsters keep the quick procedural topple (corpses clear in 1.7 s).
- New field-guide words: motion capture, retargeting, key pose, anticipation.
- Tests: `tests/motion.rs` (9), unit tests in moves.rs and clips.rs.
- Not done: a native importer for raw captures (BVH, glTF, ASF/AMC: the sets were translated from
  my-3D2dge's fitted conversions); cloth capes (still three cones); monsters with clips. (The
  importer and monsters in captured motion came after: see above.)

## Fix: slides stopping dead on flat floors, treadmill walkers — 2026-10-08
- **Not floor seams.** Slalom's floor is a single block. Rapier's character controller sometimes
  drops all of a move's horizontal motion when the move presses down into level ground: the
  floor's normal comes back as (0, 0.99999994, 0), which leaves its slope handling no horizontal
  tangent, so it reads the tiny downward remainder as slipping on a non-slip slope and removes
  everything. On the Slalom ICE run that was 8 ticks in 120. The "blocked by a wall" step
  then copied the lost motion into the velocity: an ICE slide on the momentum model stopped dead
  (7.5 to 0 m/s), and the instant model hitched for a tick. Now, when only level ground was hit
  and motion was lost, `character.rs` redoes the move in two parts, across and then down.
  Kinematic platforms still carry the character once: the across part never touches the floor.
- **Treadmills.** The belt's push is subtracted before the blocked check, so walking against a
  belt keeps the walker's own velocity and stride (Walk Cycles' treadmill walker: -0.3 to
  -3 m/s, about 2 steps a second). A belt that shoves the character into a wall stops it rather
  than setting it walking backwards.
- Tests (tests/movement.rs, on an inline flat-floor room with a belt):
  `slides_keep_their_speed_across_a_flat_floor` (momentum with the ICE settings and instant: no
  tick under 95% of top speed, across two floor materials), and
  `walking_against_a_treadmill_keeps_the_stride` (the walker's speed, steps and creep, the
  player's full stride against the belt, and no walking backwards into the wall). Both fail
  without the fix.

## Verticality Tower stairs you can walk up — 2026-10-08
The tower's stairs rose 0.5 m a step, above the character controller's 0.32 m auto-step
(`movement.step_height`), so every step needed a small jump. They were an engine quirk the guide
writers noted, not a design choice. Decision: re-cut them in data rather than raise the step
height (0.32 m keeps crates and curbs as jumps everywhere else). They are now 14 steps of 0.25 m
(the playground's rise) with the same 3.5 m total: five steps west along the lobby's south wall to
a corner, then nine north to the landing, since 14 one-metre treads don't fit in the old 7-row
stairwell. Legend `1`-`9`, `A`-`D` = step n at n x 0.25 m. The station guide's stair line and the
STAIRS sign moved with them. Checked in the REPL: from the lobby floor to the first-floor
checkpoint (y 3.52) with move input only, grounded all the way.

## Effects from the 2D WebGPU showcase, and a station guide in every room — 2026-10-08
Reviewed [michaelcrosato/2d-webgpu-demo](https://github.com/michaelcrosato/2d-webgpu-demo) (about 80
explained 2D GPU scenes, WebGPU and WebGL2). Many of its effects were already in Pavilion (bloom,
outlines, pixel art, dithering and palettes, CRT, grading, chromatic aberration, distortion,
screen-space GI, GPU particles, SDF shapes and text, soft bodies, verlet ropes). Purely 2D tricks
(Mode 7, raycaster, pseudo-3D road, 2D tilemaps, parallax) do not fit a 3D engine and were skipped,
as were GPU-only simulations (fluids, reaction-diffusion, slime, falling sand), which need a
texture-based material path this renderer does not have. Taken, and done the 3D way:
- **Painterly and print styles** (`view.filter.stylize` = paint / halftone / ascii / sketch, with
  `stylize_on` part, `stylize_size`, `stylize_mix`, `stylize_color`), in the composite on the
  finished picture: a generalized Kuwahara filter (8 sectors, polynomial weights; samples from the
  other part of the scene are skipped so paint never bleeds) plus brush streaks and canvas; CMYK
  halftone (four dot screens at 15/75/0/45 degrees, each dot reading its own centre; black only
  when no colour is kept); ASCII (12 characters from 5x7 bitmaps packed in integers); pencil
  sketch (three hatching families by darkness, Sobel contours, lines that redraw 6 times a
  second, coloured-pencil paper). A cheap picture (`base_color`: no GI/outlines/haze) is what they
  sample around a pixel; paint adds back what outlines and GI changed at the pixel.
- **Volumetric light** (`view.haze`, `haze_height`, `shafts`, `shafts_forward`, `halos`): the
  composite marches each pixel's view ray through height haze in 32 jittered steps, asks the
  sun's shadow map whether each bit of air is lit (shadows cut dark shafts), weights it by a
  Henyey-Greenstein phase and dims what lies behind; lamp halos are the closed-form integral of a
  point light along the ray (an arctangent per light). The post pass now binds the shadow map, its
  comparison sampler and the point lights. Pixels with no surface (sky, the void round a level)
  get no haze, and the haze stops a metre under the player's feet.
- **Screen transitions** (`view.filter.transition` = none / fade / iris / diamonds / dissolve /
  mosaic / blinds, `transition_time`, `transition_demo` loops one): a per-pixel mask from one
  number. The app plays the chosen one (default iris, on the player) when you arrive somewhere
  new: another place in Shardfall, or an F2 teleport. Tool captures never trigger it.
- **Wind** (`view.wind`, `wind_angle`, `wind_gusts`, `grass_push`): vertex animation in the scene
  shader for instances flagged `flags::SWAY` (leaves: moves with height above the instance's
  bottom; SDF spheres move whole) and `flags::GRASS` (bends with the square of height, away from
  the player's feet within 0.9 m, and sinks as it bends); gusts roll along the wind as a slow
  wave. Shadows sway too. New `MeshKey::Tuft` (7 two-sided blades). Data: `Sway` (none / leaves /
  grass) on `Visual`, `Decor`, room `[[object]]`s and legend props; legend `grass = { density,
  height, color }` scatters tufts per cell. The wilderness grows grass tufts
  (`terrain.grass_density`, default 0.9 per cell) and its tree canopies sway.
- **Rippling water** (`view.water_ripples`, `water_speed`, `water_fade` = amplitude half-life,
  `water_rain`): `pav_view::water` runs the 2D wave equation on a height grid per water zone
  (cells >= 14 cm, at most 160 x 160, 60 steps/s, reflecting edges, a weak spring back to the
  waterline). Anything crossing the waterline dents it by its speed, splashes and explosions drop
  big dents, rain small ones; dents displace without adding velocity (an early version sank the
  whole pool). Strips of one pool with the same waterline merge into one surface. The mesh's
  vertex colours carry the slope shading and crest foam, so ripples show in flat style too. The
  water's own timestep catches up to 0.5 s (tools render now and then).
- **Teaching (the demo's best idea, extended)**: every room has a station guide: `[learn]` in the
  room file (what you see, how it works step by step, where games use it, phrases to ask for it,
  cost, `knobs` = live settings, field-guide `terms`, `[[learn.code]]` excerpts copied from the
  engine) and a `note` on every pad (shown at the bottom of the screen while you stand on it).
  In the game: H (or the room card's **How it works**) opens the guide window with live sliders
  for its knobs; Esc → **Field guide** has ~85 words (`crates/pav_core/src/field_guide.toml`,
  searchable, cross-referenced) and every room's ask-for-it phrases (click to copy, room name to
  go there). Tools: `room` prints the guide and pad notes, `guide term= / search= / asks=true`.
  `pav_view/tests/learn.rs` keeps it honest: words exist, knobs are real settings, code files
  exist, pads with params have notes. The 34 existing rooms' guides were written by five agents
  from the engine's code (every snippet checked line by line against its file); bloom.toml is the
  model. They also found stale text, now fixed (helicopter spin-up, platform speeds, soft-body
  blasts, NOIR's split, sphere emissives, rain height).
- **World demo rooms** (generated by scripts kept outside the repo, then hand-editable):
  *Light Shafts* (vfx 6: a hall lit through west slits and a slatted roof, a colonnade with
  lanterns and swaying trees; HAZE / BEAMS / SUN pads), *Wind & Water* (vfx 7: a meadow of grass
  legend cells, trees and banners that sway, reeds, a pond with a step, a wading ring and a deep
  middle, a ball dropper; WIND / TOWARD / WATER pads), *Paint & Print* (aesthetic 5: each style
  on OFF / ALL / OBJECTS / WORLD, SIZE and COLOUR rows, the new whole looks; the village diorama),
  *Screen Transitions* (aesthetic 6: a pad per kind that loops it, speeds, STOP, NONE). Open-air
  rooms set `cutaway.height_cut = false` so canopies are not cut near the player.
- **Look & Filters**: new sections *Paint, print & sketch* and *Haze & light shafts* (presets:
  Oil paint, Gouache, Thick impasto, Underpainting, Comic print, Fine print, Newsprint, Terminal,
  Colour ASCII, Pencil, Coloured pencil, Loose sketch; Clear air, Light haze, Dusty sunbeams,
  Morning mist, Smoky hall, Lamplight, Pea soup); the arrival transition is in *Screen*. New whole
  looks: Oil painting, Painted world crisp heroes, Comic book, Newspaper, Hacker terminal,
  Sketchbook, Sketched world painted heroes, Sunbeams, Lamplit fog.
- **Shardfall**: themes have `haze`, `halos` and `grass` (themes.toml): torches glow in the smoke
  of the Ashen Halls and the dust of the Bone Crypts, spores in the Fungal Hollows; the Overgrown
  Ruins grow grass and the Sunken Temple seaweed in patches that part around the hero (placed by
  a hash of level and room, so levels are otherwise built exactly as before); the dusk town has a
  light haze with lamp halos; a gentle draught everywhere; the iris opens on the hero at every
  arrival.
- Checked: captures of every style (town, Mix & Match, Paint & Print), haze/shafts/halos (town,
  Light Shafts hall and colonnade), wilderness grass in wind, the feel-lab pool and the Wind &
  Water pond (wakes, splash rings from dropped balls, rain), each transition mid-close, four
  levels with their themes' air and grass; in the running game under Xvfb: the station guide (H)
  with live sliders, pad notes (STORM), the field guide and the Look & Filters sections.
- Not done: GPU compute simulations from the demo (fluids, sand, slime, reaction-diffusion,
  boids) need a texture material path; halos ignore shadows (a lamp behind a pillar still glows
  in front of it); the haze is a full-resolution 32-step march without temporal filtering, so it
  shows fine grain; the water is drawn opaque.
- Engine quirks the guide writers found (not fixed here): the stealth room's painted cone is cast
  from 0.6 m while sight is checked from 1.3 m. Fixed since (see above): the Verticality Tower's
  0.5 m stairs needed small jumps (auto-step is 0.32 m); an ICE slide on the momentum model stopped
  dead in Slalom (not at floor seams, as first thought); a treadmill walker's legs barely moved
  (belt push counted as blocked).

## Look & Filters: every filter on a part of the scene — 2026-10-08
- **Parts of the scene.** Every drawn instance is either *characters & objects* or the
  *environment*. The view flags `flags::OBJECT` on everything emitted after the static regions
  (characters, props, loot, projectiles, effects, game decals) except fixed scenery:
  `RenderObject::scenery` (from `Sim::frame`) is an entity with no character, a fixed or no
  body, no behaviour, bomb, health, vehicle, soft body or lifetime, and not a game actor
  (town buildings' props, braziers, columns, lava pools, sconces). The scene shader adds 1024
  to the normal buffer's w on objects (groups now fold into 1..1023; still exact in 16-bit
  floats; negative = pixel art as before), so the composite knows each pixel's part.
- **Targets per filter** (`all` / `objects` / `environment`): `view.outlines_on`,
  `view.filter.color_on` (palette, levels, dither), `grade_on` (temperature, tint, contrast,
  brightness, saturation), `scanlines_on`, `grain_on`, `chroma_on`; `pixel_target` gained
  `objects` and `environment`. Shading per part: `view.style_objects` /
  `view.style_environment` (flat / cel / lit; unlit markers and glows keep theirs; the old
  `view.style` still forces one style everywhere). Curvature, vignette, `pixelate` and the glow
  settings stay whole-screen. A pixel's part is read at the pixel-art block centre, so blocks
  agree; the dither pattern now steps with pixel-art blocks.
- **The look layer** (`pav_view::look`): nine sections (pixel art, shading, outlines, palette,
  grading, scanlines, grain & fringe, screen, glow) own fixed view paths; a section that is on
  replaces the scene's own settings (rooms, pads, places, the F1 panel), off hands them back.
  `looks.toml` (embedded) holds 3-8 presets per section (they never change the part a filter
  is on) and 12 whole looks: Pixel heroes, HD-2D, Pixel world crisp heroes, Game Boy world,
  Game Boy, 16-bit, Cartoon, Comic noir, Flat paper, Arcade CRT, Hologram heroes, Posterized
  world. Per-channel posterize shifts the hue of dull colours (the town's grey floor went pink
  and olive at 5-6 levels), so presets use 8+ levels and 24 for pixel-art levels.
- **Menu**: Esc → *Look & filters* (also a button at the top of the F1 panel): whole looks,
  *Scene default*, a before/after *Compare* line, *My looks* (save / use / delete), then a
  collapsible section per filter with an on switch, the part picker (whole scene / characters
  & objects / environment; pixel art also characters only, the hero, other characters, all but
  characters, one entity), preset chips (the matching one is highlighted) and sliders. Moving
  a slider switches its section on, starting from what was on screen. Works with the gamepad
  cursor (B closes). The look is saved to `shardfall_looks.json` next to the exe (browser:
  local storage) a moment after changes and on quit; agent tools over the live bridge see and
  change it.
- **World demo**: new room *Mix & Match* (aesthetic wing, order 4, `rooms/mix_match.toml`,
  generated from looks.toml by a script kept outside the repo): a village diorama with walkers,
  crates and balls and a fixed statue; west, a row per filter (PIXEL ART, INK OUTLINES, GAME
  BOY, NOIR GRADE, SCANLINES) with OFF / ALL / OBJECTS / WORLD pads that stack, then OBJECT
  STYLE and WORLD STYLE rows; east, a pad per whole look (they set every look path) and CLEAN.
- **Tool**: `look` (`name=` a whole look, `section=` + `preset=` / `on=` / `enabled=`,
  `reset=true`, `compare=`, `bench=all` renders this moment under every look, numbered).
  Sessions keep a look layer (`Session::look`) applied in every capture and across `load`.
- Tests: `pav_view/tests/parts.rs` (objects vs environment flags in town incl. a spawned crate
  and fixed scenery, pixel art on either part, styles per part, parts reach the renderer) and
  `tests/look.rs` (each section copies exactly its paths and no path has two owners, presets
  stay in their section and apply cleanly, looks switch on what they name, presets keep the
  part, JSON round trip). Checked visually with `look bench=all` in town, pad stacking and
  whole-look pads in the room, and the window in the running game under Xvfb.
- Not done: bloom is whole-scene (it is computed from the HDR image before the composite);
  a pixel's part comes from MSAA sample 0, so part edges are not anti-aliased.
## Turntable/animsheet frame tall creatures — 2026-10-08
- `turntable` and `animsheet` (`creature_frames` in `game_tools.rs`) cut off tall creatures.
  The Hollow King (5.26 m) lost its head and horns in every frame because the camera aimed at
  the 1.7 m capsule's centre from a distance set by `scale` alone. Now the tool poses every
  frame as the view will (same state, rig and camera direction), takes the box around all of
  them (`agent_tools::pose_bounds`, shared with `anatomy`) and aims at its centre. The distance
  is the old `2.2 + scale * 2.6` unless that would spill a box corner out of the frame (with
  `FRAME_MARGIN` 1.15); then the camera backs off. The camera holds still across frames.
- Checked by eye: all five bosses and generated genomes 164/172/191 now show head to feet in
  both tools, swings and orbiting orbs included. Ghouls, skitterers, spitters, oozes and
  ashdrakes keep the same distance, and the aim moves by a few cm at most.

## Pixel art on part of the scene — 2026-10-05
- `view.filter.pixel_art` (block size, 1 = off) turns part of the scene into pixel art while the
  rest stays sharp; `pixel_target` picks what: `all`, `characters` (every puppet), `hero`,
  `others` (characters but the hero), `world` (everything that isn't a character) or `entity`
  (`pixel_entity` = an entity id). `pixel_levels` posterizes the pixel-art colours and
  `pixel_outline` (on) draws a dark one-block outline just outside pixel-art silhouettes.
  Works with the whole-screen `pixelate`, palettes and the rest of the filter stack.
- How: the view flags the chosen instances `flags::PIXEL` (by outline group: entity id + 2,
  1 for level geometry); the scene pass writes their group negative into the normal buffer's
  w; the composite gives a block its centre's colour when this pixel or the centre is
  flagged, so flagged silhouettes step in whole blocks and nothing else is touched. Cost: a
  few extra texel loads per pixel, only while `pixel_art` > 1.
- The Filter Stack Bench has four new pads (ART OFF / CHARACTERS / HERO / WORLD); RESET
  clears it. Test: `pav_view/tests/pixel_art.rs` (each target flags exactly its instances).
- Not done yet: the block grid is fixed to the screen, so with `world` the level shimmers a
  little as the camera slides (snapping the camera to the block grid would fix it);
  shadows belong to the ground they fall on (pixelated with `world`, sharp with
  `characters`); text labels are never flagged themselves.

## Audit: the bot picks up loot — 2026-10-05
- Audit of the whole repo: fmt, clippy (warnings denied) and every test pass (main
  workspace and `pavilion-lite/`); every room file passes `room_check`. The one real problem
  was in the `campaign` balance runs: the bot needed 52-61 deaths and ~15-20 game-minutes
  for level 4 (the first boss, the Hollow King). The commit before the primary-attack fix
  was worse (boss alive after 30 game-minutes), so the cause was older than that fix.
- Cause: items are only taken by walking over them (the auto-loot filter) or by clicking
  them, and the bot did neither on purpose, so it reached every boss in its starting gear.
  Rift gates were ruled out (blocking them on the nav grid changed nothing).
- Fix (`bot.rs` `fetch_loot`): with nothing to fight, the bot walks to visible items within
  12 m that pass the loot filter and picks them up with `GameCmd::Pickup` (as a click
  would). In levels it follows a nav-grid path and skips items the grid can't reach: a first
  version walked straight and got stuck inside the town portal's ring of pillars. It gives
  up on an item after 10 s. Test: `loot.rs` `the_bot_picks_up_loot_nearby` (fails before).
- Campaign now (levels 1-11, deterministic): 0.7 / 1.0 / 1.4 / 8.0 (19 deaths) / 2.6 / 3.4 /
  6.1 / 8.3 / 0.8 / 10.1 / 6.7 game-minutes, 40 deaths in total, hero level 18 at level 12.
- Open: level 12's Frostbound Colossus stops the bot (47-49 deaths in 50 game-minutes at
  hero level 19 with or without this change; the G6 note that the bot clears all twelve is
  out of date). Retuning the final boss is a design call, so it is left as is.

## Primary attack input fix — 2026-10-03
- Shardfall consumed PRIMARY for the equipped skill, then forwarded it to the character
  controller, which also threw a Pavilion bomb (or fired the demo blaster). Clear PRIMARY
  from both held and pressed movement input after the game handles skills. Left click and
  the controller's primary attack now use only the equipped skill; held attacks still repeat.
- Regression coverage checks repeated sword swings without bomb/blaster events or bomb
  entities, and confirms left click still throws bombs in Pavilion rooms.
- Verified the new combat regression fails before the fix and passes afterward; all 23
  combat, gameplay, genre and skill tests pass. Formatting and pav_core clippy (all targets,
  warnings denied) pass.

## Pavilion Lite: the compact engine for AI agents — 2026-10-02
- New folder `pavilion-lite/`: the essential engine as ONE self-contained Cargo package (its
  own workspace, lockfile and docs; about 7,000 lines) to hand to other AI agents. It builds
  with plain `cargo build` on Rust 1.89+ and needs no system packages for headless use.
  `pavilion-lite/package.sh` makes `dist/pavilion-lite.tar.gz` / `.zip` (~110 KB, source only).
- Kept (compact rewrites of pav_core/pav_tools ideas): fixed 60 Hz deterministic sim with
  snapshots, rewind and input replays + state hash; rapier3d physics; the kinematic character
  controller (instant/momentum models, coyote/buffered/variable jumps, steps, slopes, moving
  platforms, pushing props, knockback, dash, axis lock); procedural puppets (biped, blob,
  beast; held items; act poses); ASCII TOML levels (top-down `xz` and new side-view `xy`
  planes, merged blocks, spawns, triggers, markers, movers, `[params]`); triggers, lightweight
  projectiles with teams/damage/knockback, hp/invulnerability, sparks, screen shake; grid A*;
  `Tunable` params by path; one tool registry as CLI, REPL and MCP (24 tools, incl. `capture
  marks=true`, `filmstrip`, `ascii`, `autoplay` bots, `record`/`replay`, `level_check`).
- Dropped: wgpu renderer, egui panels, audio, gamepad, streaming world, rooms/pavilion, soft
  bodies/joints/vehicles, Shardfall. Games are Rust modules implementing a small `Game`
  trait (setup/update/draw/status/params/bot), registered in `src/games/mod.rs`.
- Sample games with bots and tests: `template`, `platformer` (side view, bot finishes in
  ~13 s), `arena` (twin-stick waves, bot reaches wave 3+ in 60 s).
- Verified: 19 tests (engine units, every game runs/draws/rewinds/replays exactly, every tool
  end to end, bots win); clippy clean with and without the window; MCP over stdio; the window
  under Xvfb; a fresh extraction outside the repo builds and passes on Rust 1.97 and checks on
  1.89. Speed: 15k-80k ticks/s; 640x360 capture ~7 ms, 1280x720 ~18 ms on 4 cores.

- Fresh-agent dry run (another model, only the package and AGENTS.md): it built "Key Hunt"
  (six-room dungeon, keys, gate, slimes, bot, 3 tests; ~580 lines, zero compile errors, bot
  wins 40/40 seeds) in ~25 minutes and listed 13 friction points. All fixed: nav grid no
  longer sees characters as holes (`ground_at` ignores characters/props), nearest walkable
  path start, NPC input resets each tick, unknown tool arguments are errors, REPL quoting,
  `ascii` snaps to cells, `rewind` reports its reach, `marks=` kind filter, `autoplay`
  `until=`/`trace=`/`seeds=` sweeps, hidden level spawns, `player_entered`, fuller API docs.
- Second dry run (a smaller model, a side-view "Tower Climb"): it built and registered a
  working game with passing tests, but its bot couldn't reach the top: platforms stacked
  straight up made the player bump its head, and the level put ledges exactly at the jump
  apex. Added one-way platforms (`oneway = true` / `Spawn::oneway()`), a `reach` readout in
  `status` (jump height, air time, jump length), jump-math and "don't weaken the bot test"
  guidance, and a clearer side-view description (the map reads like a picture). The sample
  platformer now uses one-way planks; its bot predicts landings and brakes mid-air instead
  of hopping into the pit (wins in 11.8 s, 1 death).
- Idle characters on static ground skip rapier's controller (one raycast): 30 idle walkers
  851 -> 5565 ticks/s; buried under 200 crates 80 -> 460 ticks/s.

## Decisions (Pavilion Lite)
- **CPU renderer instead of wgpu**: agent sandboxes rarely have a Vulkan driver; a software
  rasteriser (boxes/cylinders as triangles, spheres and tapered capsules ray-traced per pixel,
  sun shadow map, cel/lit/flat/glow, outlines, fog, 8x8 bitmap font) makes screenshots work
  everywhere and keeps the package dependency-light. The window shows the same images
  through winit + softbuffer (no GPU).
- **One Cargo package**: engine = library, games = modules of the `pav` binary (editing a game
  recompiles only the binary). The package is its own workspace so it builds the same inside
  this repo or alone.
- **Single entity struct + game-owned side tables** (like Shardfall's actors): enemies' brains
  live in the game struct keyed by entity id, so snapshots stay one clone.
- **Events are read one tick later** (`w.events` in `update`), giving game code one simple
  hook instead of pre/post callbacks.
- Characters' `pos` is their feet; colliders become queryable at spawn (`set_aabb`), so
  raycasts work in `setup` before the first physics step.
- MSRV 1.89 (nalgebra/wide via rapier 0.36); no pinned toolchain in the package.

## Shardfall naming and Vercel deployment — 2026-10-01
- Renamed the GitHub repository to `michaelcrosato/shardfall` and updated the local remote.
  `main` remains the default; repository visibility remains private. Shardfall is the app
  and repository title; Pavilion remains the name of the playable tech demo/engine.
- Renamed the game binary/window, browser loader and generated assets, startup settings,
  log and screenshot names, MCP server, documentation, and Linux handoff archive.
- Rebuilt the Windows game and preserved the existing settings and saved hero while
  renaming the game folder to `Desktop\New AI Stuff\Shardfall`. The `Shardfall` desktop
  shortcut points to `shardfall.exe` there.
- Added `vercel.json` and `scripts/build-vercel.sh` for a static WebAssembly deployment.
  The build selects pinned Rust 1.98.1 and a matching wasm-bindgen CLI from `Cargo.lock`;
  the downloaded CLI is checked against the official release checksum. Build output is
  `target/web/`. Vercel metadata and generated environment files are ignored by Git.
- Production: https://shardfall-eight.vercel.app (project `shardfall`, Vercel team
  `michaelcrosato-1122s-projects`). Initial deployment used the verified local static
  build; GitHub is connected for subsequent deployments.
- The GitHub-triggered cold build of `4ff62c3` also succeeded on Vercel and was
  automatically promoted to production, confirming the remote toolchain/bootstrap path.
- Verification: Windows `dist` build, browser/Vercel build, shell lint/format checks;
  browser town/HUD, inventory input, local-storage save, and tech demo rendering. The
  initial production HTML, JS and Wasm returned HTTP 200 and matched the local build
  byte for byte. The subsequent Vercel rebuild serves all three successfully, with Wasm
  as `application/wasm`. Live browser startup has no console errors.
- Linux handoff is now `shardfall-linux-agent.tar.gz`, with a `shardfall/` archive root.

## Linux agent handoff — 2026-10-01
- Added `START_HERE.md` with Debian/Ubuntu prerequisites, Rust setup expectations, CLI
  examples, simulation/rendering checks, MCP usage, and native game launch commands.
- Added `scripts/package-agent.sh`: packages current tracked source (including local edits)
  plus the handoff guide/script under `pavilion/`, with a SHA-256 checksum. Git history,
  build caches, and generated output are excluded. New source files must be added to Git
  before packaging. Updated the design's branch instruction to `main`.
- Verified shell formatting/lint, extracted archive contents and source hashes (199 files),
  executable script permissions, and Cargo workspace metadata from the extracted source.
  This is a source handoff; the receiving agent builds binaries with its Linux dependencies.
- Delivered `pavilion-linux-agent.tar.gz` and its `.sha256` to the Windows desktop.

## Windows desktop build — 2026-10-01
- GitHub's default branch and this local checkout now use `main`.
- Built `main` at `54fa519` with `scripts/build-windows.sh` (Rust 1.98.1,
  `x86_64-pc-windows-gnu`, optimized `dist` profile, six build jobs).
- Verified the Windows CLI natively: 600 town simulation ticks and a 640×360 offscreen
  Vulkan capture (`out/build/windows-town.png`). Inspected the rendered town. The game GUI
  was not launched during validation.
- Copied the self-contained `pavilion.exe` to
  `C:\Users\micha\OneDrive\Desktop\Pavilion\pavilion.exe` and created `Pavilion.lnk` on
  the desktop. Verified the copy's SHA-256 and the shortcut target. Content is embedded;
  the executable imports only Windows system DLLs.

## Status by milestone
| Milestone | State |
|---|---|
| M1 Foundation | ✅ complete (2026-09-30) |
| M2 Core gameplay | ✅ complete (2026-09-30) |
| M3 World & rooms | ✅ complete (2026-10-01) |
| M4 Movement & Feel Lab | ✅ complete (2026-10-01) |
| M5 Physics Lab | ✅ complete (2026-10-01) |
| M6 Procedural Animation Lab | ✅ complete (2026-10-01) |
| M7 Visual Effects Wing | ✅ complete (2026-10-01) |
| M8 Aesthetic & Filter Wing | ✅ complete (2026-10-01) |
| M9 Genre Wing | ✅ complete (2026-10-01) |
| M10 Browser build, live bridge, polish | ✅ complete (2026-10-01) |

## M1 Foundation — done
- Cargo workspace: `pav_core`, `pav_render`, `pav_view`, `pav_tools`, `pav_app` (see AGENTS.md).
- Game window (winit) + wgpu on Vulkan (DX12 via `backend = "dx12"` in `pavilion.toml`).
- Boot diagnostics: 13 numbered, timed stages to terminal, `pavilion.log` and an on-screen panel
  (auto-hides after 10 s, F3 toggles). Windows pop-up with the log path when startup fails.
  Panic hook writes crash reports with backtraces to the log; simulation-thread crashes show a
  red banner in-game.
- Renderer: MSAA 4x HDR scene pass, directional sun with PCF shadows, point lights, flat/cel/
  lit/unlit styles per object, analytic SDF spheres/capsules/rounded cones (per-sample, pixel
  perfect), procedural meshes (cube, rounded box, sphere, cylinder, cone, plane), screen-space
  anti-aliased outlines, tonemapping (soft-knee default), cutaway + occlusion fade (active once
  there is a player).
- Simulation on its own thread at a fixed tick (60/120/240), renderer interpolates; pause, step,
  speed control; rapier3d physics; entities; static blocks in 32 m chunks; test scene.
- `pav` CLI: `scenes load step status entities params set camera capture bench gpu`, one-shot
  or REPL. Captures use lavapipe headless.
- Windows cross-compile (`scripts/build-windows.sh`), self-contained `.exe` (system DLLs only),
  verified booting and rendering under Wine + lavapipe (`scripts/smoke-windows.sh`).

## Decisions
- **Rust 1.98.1** pinned (egui 0.36 needs ≥ 1.95).
- **Math:** glam 0.33 everywhere, same types as rapier 0.36 (which moved to glam).
- **wgpu features:** Vulkan + DX12 only (+ `webgpu` for the future browser build); no GL.
- **Window surface** uses a non-sRGB format; the composite pass encodes sRGB itself (egui wants
  a non-sRGB target).
- **Windows target:** `x86_64-pc-windows-gnu` (mingw) cross-compile; `dist` profile (stripped,
  ~35 MB). Release build is a GUI app that attaches to the parent console when run from a terminal.
- **Files next to the .exe:** `pavilion.log` (fresh each run) and `pavilion.toml` (startup
  settings, created on first run; CLI flags override: `--scene --seed --backend --no-vsync --fullscreen`).
- **Snapshots:** `SimState` is plain `Clone` data (rapier sets are cloneable); the physics
  pipeline is workspace only and lives outside the state.
- **Entities:** one "fat" struct with optional parts in a `BTreeMap` (deterministic order); ids
  are never reused.
- **Level geometry:** axis-aligned blocks ("tiles with heights") stored per 32 m chunk; each
  chunk has a version so the renderer caches instance lists.
- **Parameters:** `Tunable` visitor trait; paths like `camera.tilt`, `view.light.sun_elevation`.
- **Colors:** authored as sRGB hex, stored linear.
- **Temporary keys (M1):** F5 reset, F6 pause, F7 step, F8/F9 speed. M2 finalizes the fixed
  system layer.

## M2 Core gameplay — done
- **Tile levels with heights** (`pav_core/src/level.rs`): ASCII layers + legend (TOML), blocks
  with relative heights, ladders, props, markers; identical neighbours merge into one block.
  First room file: `rooms/playground.toml` (crates, stairs, plateaus with a gap, crawl tunnel,
  two-storey building with ladder and destructible upper floor).
- **Character controller** (`character.rs`): kinematic capsule on rapier's character controller;
  movement models **instant** (with hold-to-slow focus) and **momentum** (accel/decel/skid/air
  control); jump with coyote time, buffering and variable height; crouch and crawl (capsule
  resizes, never stands up into a ceiling); ladder climbing with pull-up at the top; bombs thrown
  at the aim point that remove destructible tiles, throw debris and push things.
- **Puppet v1** (`puppet.rs`): skeleton with two-bone IK, speed-driven walk cycle, arm swing,
  bob, lean into acceleration, squash & stretch spring, crouch/crawl/climb/air poses, eyes that
  tilt toward the camera, optional stepped animation; all proportions/colours are sliders.
- **Camera**: follows the player; "walls down" cutaway lowers geometry near the player and hides
  what is above them (upper floors); occlusion fade tunnel; all parameters live; 8 presets.
- **Input**: keyboard+mouse (physical keys) and gamepad (gilrs), camera-relative movement,
  mouse aim on the ground plane at the player's height, right-stick aim; prompts follow the last
  device used. Fixed system layer (Esc/F1–F12, hold Backspace rewind).
- **Tuning panel** (F1): every tunable (sim, movement, bombs, puppet, camera, view, app) grouped
  by path with filter; presets saved as JSON in `presets/` next to the exe; frame/tick graphs.
- **Time**: pause, step, speed 0.05–8×, hold-to-rewind, timeline scrub slider, snapshot save/load
  (`snapshots/quicksave.snap`), replay save (`replays/last.replay.json`).
- **Agent layer**: new tools `player input spawn despawn teleport rewind snapshot_save
  snapshot_load record_save replay`; MCP stdio server (`pav mcp`, registered in `.mcp.json`).
- **Tests**: `crates/pav_core/tests/gameplay.rs` proves the M2 "done when" list headlessly.

## Decisions (M2)
- Characters are kinematic capsules (radius 0.32 m; stand 1.7 m, crouch 1.15 m, crawl 0.66 m).
  Character gravity (32 m/s²) is separate from prop gravity (9.81) for game feel.
- Rewind = snapshot every 20 ticks + per-tick input log; re-simulation is exact on the same
  build (verified by test). History window default 30 s (`sim.history_seconds`).
- Snapshot files use CBOR (ciborium): rapier and our tagged enums need a self-describing format.
- Replays record inputs from scene start (run-length encoded JSON) plus the final state hash.
- Cutaway lowers whole instances whose footprint touches the cut circle (clean "walls down"
  look) and hides instances entirely above the cut; SDF parts use per-pixel cutting.
- Bombs stop rolling after their throw arc so they land where aimed.
- Startup room setting renamed to `start_room` (M1 files with `scene = "test"` are ignored);
  default room: playground.
- Deliverables are sent zipped (chat upload limit is 30 MB; the .exe is ~40 MB).
- Deferred: live shader editing (M7/M8); per-room key rebinding (M3 room framework).

## M3 World & rooms — done
- **World** (`world.rs`): pavilion hub (plaza with fountain, four corridors with lamps, door gaps)
  at the centre; rooms are auto-placed along their wing's corridor and rotated so the entrance
  faces it; wings: movement/aesthetic → east, physics/genre → north, animation → west,
  vfx/misc → south.
- **Wilderness** (`terrain.rs`): terraced tile terrain from fbm noise (0.3 m steps, water, sand,
  grass, rock, snow), one mesh + one triangle-mesh collider per 32 m chunk, trees, rocks, loose
  props; flattened around the pavilion.
- **Streaming**: regions (terrain chunks, rooms, hub) load around interest points (player +
  agent points), never the camera; far regions go dormant with their entities (positions and
  velocities kept); untouched terrain is dropped and regenerated from the seed; modified regions
  and moved props persist. Budget: 2 region loads per 15 ticks.
- **Room framework**: room files (`room.rs`) with info card (about + try list), wing, primary
  device, movement model, camera defaults, parameter overrides and key remapping applied while
  inside; enter/exit tracking with events; F2 teleport menu, F4 leave, F5 reset room,
  `--room NAME` launch; control guide follows the room's primary device.
- **Room files**: embedded into the exe by `pav_core/build.rs`, overridden by `rooms/` (repo,
  cwd, next to the exe, or `PAV_ROOMS`) and hot-reloaded (notify); errors stay on screen until
  fixed and the broken room keeps its last good version. `rooms/_template.toml` documents it.
- **Sandbox editing** (F10): place / move / delete with the mouse, palette (shape, size, colour,
  physics), ghost preview, "save objects into room" rewrites only the `[[object]]` part of the
  room file (maps and comments kept). New room: `rooms/sandbox.toml`.
- **Synth sound v1** (`pav_audio`): oscillators, noise, ADSR, pitch glide, filter sweep; sounds
  for jump, footsteps, landing (by speed), throw, explosion, room entry; stereo panning and
  distance falloff; runs silently without a device; `audio` tunables (master, sfx, footsteps).
- **Tools**: `rooms room goto room_reset room_check room_reload stream filmstrip audio_capture`;
  CLI output is compact JSON.
- **Renderer**: per-vertex colours, custom meshes uploaded from the scene and evicted when
  unused, distance fog, per-vertex cutaway for large meshes (terrain).

## Decisions (M3)
- Static content is grouped into regions (`RegionKey::Chunk/Room/Hub`) instead of only spatial
  chunks: the region is the unit of streaming and of room resets.
- Rooms are units of streaming (active within 40 m of an interest point); terrain chunks
  within 2 chunks (dormant beyond 3).
- Terrain is one trimesh collider per chunk (shared shape → cheap snapshots/rewind).
- Default start scene is `world`; standalone room scenes stay available for fast agent work.
- Input taps shorter than a frame are never lost (keys and mouse).
- Deferred to later milestones: gamepad remapping per room (keyboard only now), impact
  sounds from physics contacts, signage/in-world text.

## M4 Movement & Feel Lab — done
- **Zones** (`zones.rs`, legend `zone = {...}`): start/finish/gate/checkpoint/kill/water/pad/camera,
  merged into rectangles; floor markings (checkered finish, flags on checkpoints, water surface).
- **Courses** (`course.rs`): leaving START starts the timer, gates in order (missed = +2 s),
  FINISH stores the result and best time per `room/course`; checkpoints; pits respawn; falling
  below -30 m respawns; HUD timer + result card + messages (`pav_app/src/hud.rs`).
- **Pads** apply parameters (restored when leaving the room) and sticky camera cues; **camera
  zones** apply a cue while inside; cues can sweep parameters and flip ortho (camera bench).
  Camera changes blend smoothly (`CameraRig::blend_from_current`); room cameras turn with the room.
- **Behaviours** (`behaviors.rs`): `move` (platforms/doors/crushers, with hold), `rotate`
  (sweepers, pendulums with pivot), `emitter` (aimed/forward/ring/spiral/random, bursts).
  Motion is a function of the tick (exact after rewind).
- **Projectiles** (`projectile.rs`): plain structs, ray-tested against fixed geometry, capsule test
  against characters; cap 6000. Drawn as glowing SDF spheres.
- **Characters**: models instant / momentum / **grid** / **committed** (turn rate, fixed jump arcs,
  crouch-while-running dodge roll); **ledge grab** + shimmy + pull-up, mantle when close to the top;
  **swimming** (float, dive, climb out), wading; **moving platforms** carry, kinematic objects push
  (squeezed against a wall = respawn); **hazards** (knockback/respawn) and hit stun +
  invulnerability flash; **axis lock** (`movement.lock_axis`, for side-view rooms); flat rooms via
  `movement.allow_jump = false`.
- **Feel metrics** (`feel.rs` + app overlay): response ticks, time to top speed, stop time/
  distance, turnaround, jump height/air time/distance, speed graph; **input delay** measured from
  the OS key event to the simulation tick and to the submitted frame. Model / tick rate / vsync /
  smoothing switches in the overlay. Rooms open it with `overlays = ["feel"]`.
- **In-world text** (`pav_render/src/text.rs`): SDF font atlas (Ubuntu Light from egui's fonts),
  alpha-to-coverage glyph quads; room `[[label]]`s, legend labels, zone labels; floor labels turn
  to stay readable for the current camera.
- **Tools**: `course`, `feel`, `camera_bench`; `player` shows hang/swim/roll/stun/model; agent
  sessions apply room camera defaults and cues like the game.
- **Rooms** (movement wing, east corridor):
  - `feel_lab` (model pads, sprint lane with distance marks, jump poles, gaps, ledge wall, pool),
  - `tightrope` (beams 0.6 → 0.15 m over a pit, zig-zag, sliding beam, turning plank, checkpoints),
  - `slalom` (10-gate and 6-gate courses, momentum by default, ICE / NORMAL / GRIP surface pads),
  - `timing_gates` (doors, crushers, sweepers, moving platforms over a pit, pendulums),
  - `gauntlet` (jump / duck / crawl / aimed / bullet-garden turret lanes; ROLL pad),
  - `camera_bench` (one loop course + 9 camera pads incl. auto sweep; camera zone in the tunnel),
  - `tower` (3-floor verticality: stairs, ladders, lift, ledge, crawl vent, duck corridor,
    bombable floor).
  Five of the rooms were built by helper agents (Sonnet) from briefs and reviewed: each was
  verified by driving a full run headlessly; their reports led to engine fixes (walking on
  kinematic objects, zone edges, START handling, label fitting, turret bullets, side views).
- Room names are written on the corridor floor in front of each door.
- Tests: `crates/pav_core/tests/movement.rs` (11 checks) and `tests/rooms.rs` (every room file
  builds standalone; every course has a START and a FINISH).

## Decisions (M4)
- Simulation order: behaviours (set mover velocities) → characters → physics step → bombs →
  projectiles → zones/courses → feel metrics. Rapier's character controller carries and pushes
  characters with kinematic bodies (it expects characters to move before the step); our code
  only adds crush detection (squeezed into a kinematic object = respawn), hazards, and keeps the
  controller's small gap above kinematic floors (without it a character cannot slide on them).
- Ledge grab is off by default; rooms opt in (`movement.ledge_grab`), because a jump plus a grab
  reaches about 3.4 m and would let players climb out of rooms with 2.5–3 m walls.
- Zones are half-open boxes on the ground plane; respawning into a zone doesn't trigger it;
  clipping another course's START never cancels a running timer.
- Low cameras (tilt < 30°) cut away everything in front of the player ("front cut"), so side
  views work in rooms with walls.
- Walking off an edge only grabs ledges higher than where you left the ground; after a jump any
  ledge can be grabbed (so walking into a bombed hole drops you, a short jump is rescued).
- Vehicle (drift car) and flight (helicopter) movement models come with the Genre Wing (M9);
  M4 adds grid, committed, swimming, ledges. Climbing ladders existed since M2.
- Dev builds use line-table debug info and no incremental cache (disk allowance in the cloud).
- Map rows: only a truly empty first line is dropped (a leading row of spaces is a real row).

## M5 Physics Lab — done
Seven rooms in the north corridor (wing `physics`), built by helper agents from data and reviewed:
- **Stacking & Toppling** (`stacking`): 10-box tower, Jenga, pyramid, mixed shapes, curving
  domino line, light vs heavy walls, crate shower pad.
- **Springs & Soft Bodies** (`soft_bodies`): jelly cubes soft/medium/stiff, balloons, tearable
  banner, walk-through curtain, ropes (one tied to a box), spring platforms on sliders, wobbly
  posts on spring joints.
- **Chains & Rope Bridges** (`chains`): 11 m and 16 m bridges over kill pits with checkpoints,
  wrecking ball, hinge door, hanging chains, Newton's cradle, seesaw.
- **Conveyors** (`conveyors`): crate loop, 1/3/6 m/s lanes, upstream course, sorting line into a
  bin, opposing belts.
- **Bounce & Friction Gallery** (`materials`): restitution drop lanes, friction ramps, bounce
  pads, seesaw, ice vs rubber, density lanes.
- **Destructible Floors** (`destruction`): crumble-run course, slow vs fast crumble, glass floor
  smashed by dropped weights, bomb wall, crumbling stairs.
- **Stress Test** (`stress`): spawn pads up to 600 bodies, bullet storm, physics stats overlay
  (~600 ticks/s with 600 bodies).
Engine: soft bodies, joints/chains/bridges, spawners + pad signals, conveyors, bounce pads,
crumbling/breakable tiles, physics overlay, damping, hinge springs, mass-aware pushing, bombs push
soft bodies, `signal` agent tool. Tests: tests/physics.rs (9) + every room builds.

## Decisions (M5)
- Chains default to density 400 and damping 0.3; bridges fix both ends by default
  (`fix_to = Option`), so light links can't fold under a character.
- Characters press on dynamic floors with `movement.weight` (70 kg), and pushing shares momentum:
  speed × push_mass / (push_mass + pushed mass), so heavy props are slow to shove.
- Autostep includes dynamic bodies, so you can walk onto low planks and seesaws.
- Joints to the world measure the object against the world (slider +limit = along +axis).
  `at_b` gives a joint a second anchor (springs that start stretched).
- Glass `strength` is a contact force in newtons; characters never break glass (they are
  kinematic), dropped weights do.
- Conveyors move props whose centre is in the zone; loops need belts that hand over one cell past
  corners (see conveyors.toml).
- Empty meshes are skipped in the renderer (a world capture used to panic on one).
- Helper-agent tips: give each helper its own scratch subdirectory; keep briefs explicit about
  coordinates, tools and what to verify; they work from the prebuilt `target/debug/pav`.

## M6 Procedural Animation Lab — done
Six rooms in the west corridor (wing `animation`), built by helper agents from data and reviewed:
- **Walk Cycles** (`walk_cycles`): stroll/walk/run track, proportions (short/long legs, big head,
  tiny, giant), walk styles (bouncy, stiff, swagger, on twos vs smooth), stairs and slope (foot
  IK), treadmill, pads that change your own stride / legs / scale / on twos.
- **Jointed Limbs** (`creatures`): spider pen over rocks, lizard run, beetles with 4/6/10 legs,
  leg-count line-up, PLAY AS spider / lizard / beetle / blob / giant spider / human, followers.
- **Impact & Recoil** (`recoil`): shooting gallery (knockback 2-14), bomb ring of bipeds and
  creatures, gauntlet corridor, stun vs bounce back, machine gun vs cannon, sizes & bodies.
- **Squash & Stretch** (`squash`): blob pond, squash dial (none/normal/rubber), trampolines,
  high drop, pads for rubber / stiff / blob you.
- **Secondary Motion** (`secondary`): wobble dial, tail lengths, antennae, spinning platforms,
  pets that follow you, pads to grow a tail / antennae / floppy / stiff.
- **Character Style Bench** (`style_bench`): you on a stage with CUTOUT / FLAT / CEL / LIT pads,
  yaw and tilt swing zones, fixed camera pads, cheats (eyes / lean to camera, on twos), and a
  line-up of all four looks plus a cutout and a lit spider.
Engine: body plans (biped/spider/lizard/beetle/blob; creatures are characters with a low capsule),
`rig.rs` (planted feet stepping, follow-the-leader spine, verlet tails/antennae/abdomen with a
wobble-scaled wag), foot IK on steps, hit-recoil spring, cutout look + face-camera lean, NPCs
(`[[npc]]`, `ai.rs`: idle/wander/patrol/circle/follow, separation, hops), `npcs` agent tool.
Tests: tests/animation.rs (5).

## Decisions (M6)
- Creatures reuse the character controller (crawl-height capsule, full speed, can jump) instead
  of a separate mover: they get conveyors, hits, water, platforms and agent tools for free.
- Body plan, proportions and animation are all puppet params, so pads can turn the player into
  a spider (`puppet.body`); NPCs carry their own puppet def (preset + `look` overrides).
- Characters are never sliced by the camera cutaway.
- Text glyphs are drawn without screen-space outlines (they speckled light text).
- Idle NPCs walk back to their spot and face their start direction after a knock.


## M7 Visual Effects Wing — done
Five rooms in the south corridor (wing `vfx`), built by helper agents and reviewed:
- **Lights & Shadows** (`lights`): shadow-casting flickering torches, RGB colour mixing, flicker
  and pulse lamps, a lantern sweeping shadows, a sun dial (dawn/noon/dusk/night), shadow switch.
- **Particle Garden** (`particles`): every preset on a plinth, snow / rain / fireflies, campfire,
  tuning overrides, bomb range, particles on/off.
- **Bloom & Glow** (`bloom`): brightness ladder, neon, bloom dial and threshold, sparkles, day vs
  night, glow balls.
- **Heat & Shockwaves** (`distortion`): lava haze, forge, lenses over checkers, ripple pool,
  repeating shockwaves, bomb range, distortion switch.
- **Global Illumination** (`gi`): Cornell box, contact shadows, colour spill, GI dial (labelled
  as a screen-space approximation, the stretch goal).
Engine: GPU particles (CPU birth, compute integration, premultiplied sprites; presets and
pre-warm), HDR bloom mip chain, distortion buffer (ring/haze/lens/ripple), point-light shadows (4
lights x 6 faces in a depth array), screen-space GI (bounce + AO), object `light` / `particles` /
`distortion`, explosions add sparks/fire/smoke/shockwave, room `[view]` tables and pad `view.*`
params, nearest-64 point lights. Tests: tests/fx.rs.

## M8 Aesthetic & Filter Wing — done
Three rooms in the east corridor (wing `aesthetic`):
- **Pure Styles** (`styles`): the same diorama in flat / cel / lit / unlit, style override pads,
  outline / cel band / rim / tonemap pads.
- **Style vs Filters** (`filters`): split screen (left pure, right filtered) with a pad per filter
  (scanlines, CRT, dither, PICO-8, pixelate, warm, cool, noir, VHS), camera cues aim at the diorama.
- **Filter Stack Bench** (`filter_bench`): rows of stackable filter pads, presets (Game Boy,
  arcade, VHS, noir, PICO-8, 1-bit, amber terminal, sepia), split on/off, reset; F1 has sliders.
Engine: filter stack in the composite (`view.filter.*`: pixelate, curvature, scanlines, dither +
levels or palette gameboy/pico8/cga/1bit/amber, temperature, tint, contrast, brightness, vignette,
grain, chroma, saturation, split).

## Decisions (M7, M8)
- Effects are view-only: lights, emitters and distortion ride on `Visual`, so they survive
  dormancy and edit-mode saves, and never touch determinism.
- Particles: no GPU spawn shader; the CPU writes new particles into a ring buffer (simple, works
  on WebGPU), a compute pass integrates them. New emitters pre-warm so captures and freshly woken
  rooms show them.
- Point shadows sample their six faces with the same matrices used to render them (2D array),
  avoiding cube-map orientation conventions.
- Rooms on neighbouring corridors are placed greedily so they never overlap (they slide along
  their corridor).
- Room `[view]` sun azimuth turns with the room's placement (like camera yaw).
- `view.filter.saturation` (split-aware) for grading; `view.saturation` is the global one.
- Agent `load` re-applies the room's `[camera]` / `[view]`.

## M9 Genre Wing — done
Four rooms in the north corridor (wing `genre`), built by helper agents and reviewed:
- **Bullet Hell** (`bullet_hell`): top-down blaster arena, drone and turret stages, a phased
  boss with a health bar; score, hits, dodgeable patterns (bot-verified 0-hit run, 83 s).
- **Grid Stealth** (`stealth`): a night heist on the grid model; guards with vision cones (cut
  by pillars; tables hide a crouching player), an alert meter, checkpoints, sweeping cameras,
  a crouch-only duct (bot runs reach the vault in 39-49 s with no SPOTTED).
- **Drift Circuit** (`drift`): ~157 m lap with gates, checkpoints, gravel kill traps, a grippy
  and a drifty car, a drift pad and a skid pad (bot laps 12.3-12.4 s).
- **Helicopter Run** (`helicopter`): 9 rings through a small city to a rooftop landing,
  practice pad with a touchdown target (bot run 20.9 s).
Engine: `vehicle.rs` (rapier ray-cast car with handbrake drift, arcade helicopter with a
ceiling; E / pad D-pad right gets in and out), `health.rs` (shootable objects: hp, score,
signal, finish, boss bar for the current room, sway, phases), `stealth.rs` (guard AI with
line-of-sight cones and an alert meter), blaster weapon (`bombs.weapon = "blaster"`,
fire_interval, bullet_speed, shoot_angle, shot_range), small `movement.hitbox`, room `height`.
Fixes from the helpers' reports: car steering sign, characters spawn just outside the
controller skin (they stuck at floor seams for ~1 s), grid steps follow stick strength (NPC
`speed` works in grid rooms), sentries keep their yaw, `player` shows the vehicle, `npcs` shows
guard alert. Tests: tests/vehicles.rs (5), tests/genre.rs (4).

## Decisions (M9)
- Cars use rapier's ray-cast vehicle controller (it serializes, so rewind stays exact); the
  helicopter is velocity-controlled with gravity off while the rotor is up. Riders become
  sensors and are hidden; E gets in and out.
- `VehicleDef` fields default to the kind's preset (a helicopter with only `kind` set flies).
- Projectiles have teams (enemy / player); player shots damage `health` objects.
- A boss with `health.finish` ends the course and stands in for a FINISH tile.
- A fixed `shoot_angle` is in the room's map frame and turns with the room's placement.
- Rooms have a `height` (default 8 m): flying rooms raise it so courses are not cancelled.
- Characters placed by their feet start 0.025 m up (outside the 0.02 m controller skin).
- Grid steps take the stick's strength as their pace (keyboard = full pace; NPC `speed` scales).
- The boss bar only shows for a boss in the player's current room.

## M10 Browser build, live bridge, polish — done
- **Live agent bridge**: the game listens with `--bridge [ADDR]` (default 127.0.0.1:7878) or
  `bridge = "ADDR"` in pavilion.toml. JSON lines over TCP; each request runs a registry tool on
  the simulation thread against the running game (`Session::from_live`), with the game's camera
  and view; camera/view changes come back to the game. Captures use a separate headless device.
  Clients: `pav live [ADDR]` (REPL) and `pav mcp --live [ADDR]` (MCP). Verified on Linux and with
  the Windows .exe under Wine (status, input, set, capture).
- **Browser build**: `scripts/build-web.sh` -> `target/web/` (index.html, pavilion.js,
  pavilion_bg.wasm ~13 MB). Same `pav_app` crate with `cfg(target_arch = "wasm32")`: the sim is
  stepped from the frame loop (no threads), GPU setup is async (finishes in `about_to_wait`),
  egui input via a small adapter (`uiinput.rs`; egui-winit is native-only), Web Audio (resumed on
  first input), `web-time` Instant, room from `?room=NAME&seed=N`, no file watcher / bridge /
  screenshots. Verified in headless Chromium (WebGPU on SwiftShader, flags
  `--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader
  --use-angle=swiftshader`): boots, renders the world, plays sound, takes keyboard input.
- **Polish**: wing signs over each corridor mouth and a welcome line in the plaza; F2 rooms in the
  hint bar; control guide lists vehicles and room keys; gamepad D-pad right gets in vehicles;
  more glyphs in the world font (dashes, ellipsis, ±, ², ½); bullet-hell hitbox dot; boss bar
  only in the boss's room; `player` / `npcs` tools report vehicle and guard alert; README.

## Known issues and limits
- Browser: needs WebGPU (recent Chrome/Edge); no screenshots, hot reload or file saves
  (sandbox save, presets); the sim shares the main thread, so very slow frames slow the game.
  Not yet tried on a real GPU in a browser (only SwiftShader in the container).
- Distortion has no depth test (shimmer can show through a wall in front of the source).
- The player cannot break glass (bombs and props can); crumble tiles test one centre ray.
- `follow` NPCs have no pathfinding (they walk straight at you and slide along walls).
- The `teleport` tool does not trigger zones (pads, checkpoints) where it lands.
- Drift: no reset-car key (F5 resets the room); grippy and drifty cars differ mostly under the
  handbrake. Helicopter: hitting a tower just stops it (no crash).
- Stealth: a guard that glimpses you stops sweeping (no forgiveness); SPOTTED counts as a fall.
- Bullet hell: score only counts once you leave START; player shots hit in 3D (targets near 1.1 m).
- Determinism across machines, Mac, other browsers: out of scope (DESIGN §16).

## Decisions (M10)
- Bridge requests swap the live `Sim` into a `Session` and back on the sim thread (no copies);
  live sessions skip the session's own room camera/view sync (the game does that).
- Browser build reuses the app crate rather than a separate web crate; wasm-bindgen CLI must
  match Cargo.lock's wasm-bindgen version (0.2.129).


# Shardfall (the showcase game)
Goal: a complete hack-and-slash on the engine (docs/GAME.md has the design and milestones
G1-G6). Work happens on the same branch; each milestone ends with merge to main + a zipped
Windows build.

| Milestone | State |
|---|---|
| G1 Combat core | ✅ done (3423ab6) |
| G2 Loot, items, inventory | ✅ done |
| G3 Passive tree, all skills | ✅ done |
| G4 Monster genome, bosses | ✅ done |
| G5 Town, levels, mechanics, endless | ✅ done |
| G6 Polish, agent tools, final build | ✅ done |

## G1 Combat core — done
- `pav_core::arpg` lives in `SimState::game` (Option<Box<Game>>): `game_pre` (hero input ->
  casts/dodge/potion; monster brains -> movement + casts) runs before characters move,
  `game_post` (casts land, shots, effects, ailments, regen, deaths, rewards, arena waves) after
  physics. Hit-stop scales the whole tick's dt (`Game::time_scale`).
- Data in `/game` (embedded by pav_core/build.rs as GAME_DATA; `data::reload(true)` re-reads
  ./game or $PAV_GAME): `skills.toml` (hero skills and monster attacks share one format),
  `monsters.toml` (families: body plan, archetype, skills, multipliers, puppet look).
- Stats (`stats.rs`, ~75 stats with item-text templates) -> `Sheet`. Damage: per element
  (physical/fire/cold/lightning/poison), armour, resistances, crit, ailments (bleed, ignite,
  chill/freeze, shock, poison stacks), knockback, leech/on-hit.
- Hero skills: slash (3-hit combo), cleave, leap_slam, blade_dash, fireball, frost_nova; dodge
  roll (Space, i-frames), potions (1). Monster skills: claw, bite, smash, stomp, spit, gore
  (telegraphed). Families: ghoul, bonecrusher, skitterer, spitter, ashdrake, bile_ooze.
- Puppets: `ActKind` action poses (slash, overhead, thrust, spin, cast, throw, roar, leap,
  lunge), held weapons (`WeaponLook`), whole-body motion (spin, lunge, leap lift, death topple),
  part glow, frame tint (hit flash, frozen, burning...). `pose_ex` returns the weapon span.
- View (`pav_view::arpg`): telegraph decals, swing arcs, rings, flashes, projectiles with light
  and trails, swing-trail particles, elite auras, ailment particles, screen shake; game event
  particles in fx.rs; synth sounds for the game events.
- App: game mode switches bindings (WASD, LMB/RMB/Q/E/R/F skills, Space dodge, 1 potion, Shift
  stand; gamepad X/Y/B/RB/LB/RT, A dodge), camera preset and look; HUD (`arpg_ui.rs`: orbs,
  skill bar with cooldown sweeps, XP bar, damage numbers, monster bars, banners, death);
  difficulty sliders + presets in the pause menu (`difficulty.*` params).
- Scene `arena` (wave arena). Bot (`arpg::bot`) + tools: game, hero, monster, autoplay, skills,
  game_reload. Tests: tests/arpg.rs (bot clears waves, monsters hurt, mana/cooldowns, dodge,
  rewind exact).

## G2 Loot, items, inventory — done
- Data: `game/items.toml` (114 bases, generated table), `game/affixes.toml` (74 affixes with
  endless tiers), `game/uniques.toml` (19 uniques with powers); loaded and validated in
  `arpg/data.rs` (unknown stats, slots, bases are load errors).
- `arpg/items.rs` (Slot, EquipSlot, BaseDef/AffixDef/UniqueDef, Item, roll_item, unique_item,
  base_scale, local affixes, tooltips `ItemText`, value), `arpg/loot.rs` (drops, ground items,
  gold magnet, auto-loot), `arpg/powers.rs` (16 powers + hooks: on fire, on kill, per tick,
  in `hit`), `arpg/cmd.rs` (`GameCmd`, `Place`, `Spot`, vendor restock), hero equipment (10
  slots), bag (40), stash (120), `refresh_hero` sums gear (weapon stats, armour, powers, blood
  magic) and dresses the puppet (`hero_look`, `PuppetDef::gear` = `GearLook`).
- Town scene `town` (Emberwatch: square, houses, well, lamps, forge, Hilda hammering with
  sparks and turning to greet, stash chest, portal with ripple distortion); arena has a portal
  home. `Sim::game_travel` rebuilds the place at the end of the tick (same tick/rng, entity ids
  continue, region versions bumped so the renderer refreshes).
- `InputFrame::cmd` (one menu command per tick; simhost queues them in `Shared::cmds`).
- View: loot (item shapes, rarity rings, light pillars for rare/unique), coins, burning fields,
  orbiting blades, the usable spot ring; fx/sounds for drops by rarity, pickups, travel.
- App (`arpg_items.rs`): inventory with paper doll, tooltips with tier tags and a compare panel
  (stat-by-stat gains/losses, DPS), green frame on upgrades, context menu (wear, right hand,
  sell, stash, drop), vendor (wares, buy-back, sell all), stash, portal window, character sheet,
  skill bar picker, clickable loot labels (stacked), spot labels with "G: trade" prompts;
  keys I/Tab C K T G; each place has its own light (town at dusk).
- Tools: `loot_roll` (tooltips or distribution summary), `give`, `inventory`, `game_cmd`.
- Tests: tests/loot.rs (drops picked up, equip changes weapon/two-hander/armour/helm look, town
  trade + buy-back + stash + travel both ways, rewind across travel exact, orbiting blades hit
  and vanish when unequipped); items unit tests (every level/rarity rolls, scaling, local).
- Known gaps (later milestones): no saving yet (G5), gamepad cannot drive the menus yet (G6),
  vendor only has the smith (more NPCs in G5), item level requirements are not used.

## G3 Passive tree and all skills — done
- 16 hero skills (game/skills.toml): slash, cleave, leap_slam, blade_dash, fireball, frost_nova,
  rend, ice_shards, war_cry, chain_lightning, whirlwind, blizzard, earthsplitter, blink, meteor,
  toxic_rain (unlocks 1-24). New behaviours: channel (held, pulses, pays per second; cast
  `button`/`pulses`), wave (rolling fissure of quick blasts), buff (war cry: buff mods to user +
  allies, taunt, shove), meteor (`EffectKind::Meteor`, falling rock), field (blizzard, cold
  shards fall), blink (stops short of walls), rain (scattered drops). Skill fields: duration,
  interval, chain, delay, scatter, buff.
- Tweaks (`skills::Tweak`, `TweakField`: damage count radius cooldown cost pierce chain ailment
  duration speed range knockback element) live on actors; `skills::skill_of(actor, id)` gives
  the tuned skill used everywhere (cast, fire, telegraph, HUD cost/cooldown).
- Passive tree (`arpg/tree.rs`, `game/tree.toml`): generated from sectors (road of 8 with
  notables, two lanes with 3 wheels around masteries and 3 skill branches, keystone at the end),
  bridges across gutters between sectors (inner + outer with a keystone), then 30 endless Astral
  rings (stats grow per ring). 265 main nodes (30 notables, 12 keystones, 18 masteries x 4
  options, 48 skill upgrades) + ~4500 Astral nodes. Ids are FNV hashes of layout keys. A light
  relaxation keeps nodes apart. Rules: adjacency to allocated/start, refund keeps connectivity
  (gold), respec (gold), mastery options once per sector. One point per level.
- New powers: `Convert` (Avatar keystones). Stat lines read naturally when negative.
- Monsters keep `base`/`mods` and `Actor::recompute` (buffs work for them too).
- UI: tree window (P): pan/zoom canvas, hover details, click / shift-click path / right-click
  refund, mastery picker, search, reset; "+N passive points (P)" nudge; skills window shows
  tree upgrades; icons for the new behaviours. View: meteors, fields (fire embers / falling ice),
  quick blasts.
- Tools: `tree` (find/take/refund/respec/mastery, what the tree gives), `tree_map` (software
  rasterised PNG of the tree; `Canvas` helper for diagrams). Bot spends points (notables,
  upgrades for its bar skills, no keystones), picks masteries, holds channels.
- Tests: tests/skills.rs (all 16 skills hurt monsters, Twin Flames doubles fireballs, channel
  lasts while held, war cry buff, blink distance, tree rules through commands incl. masteries
  and refund/respec, Avatar of Flame converts, bot spends points and keeps winning); tree unit
  tests (size, connectivity, no overlaps, path/refund rules).

## G4 Monster genome and bosses — done
- Attachments (`crate::parts`): horns, antlers, spikes, crest, tusks, mandibles, plates, eyes,
  orbs, wings on any body plan; body builders report `Anchors` (head, back line, shoulders);
  `PuppetDef::parts`.
- Genome (`arpg/genome.rs`, `game/genome.toml`): body plans (proportion ranges, legs, tails,
  antennae, weapons), parts (which bodies, sizes, counts), palettes per element, archetypes
  (brain + skill pools + stat shape + powers), names. `Genome::generate(data, seed, level,
  opts)` is deterministic; `MonsterSpec` unifies designed families and genomes for spawning
  (`spawn_spec_into` / `spawn_actor`). Element re-colours skills through a `*` tweak.
- New brains: caster, skirmisher (strike and retreat), swarm, bomber (bursts on contact),
  summoner. New powers: death_burst (telegraphed), summon (brood from the summoner's genome,
  same element, capped), enrage.
- Monster affixes (`game/monster_affixes.toml`, 18): stats, powers, `*` tweaks, extra skills,
  size/life/speed; magic 1, rare 2-3 plus a rare name. Monsters keep base/mods and recompute.
- Bosses (`arpg/boss.rs`, `game/bosses.toml`): 5 designed (Hollow King, Mother of Swarms,
  Cinder Wyrm, Frostbound Colossus, Abyssal Bloom) built from genome seeds with overrides
  (body, element, scale, parts, look), phases at life shares (speech, skills, summons, mods,
  powers); generated bosses `gen:<seed>` with three phases. Boss bar in the HUD.
- Arena: every 10th wave a boss (designed in order, then generated); from wave 4 a third of
  packs are generated creatures. Monsters casting big hero skills telegraph; hero spells have
  `effect` values for monster use.
- The Menagerie (`lab` scene, Place::Lab via portals): 20 pedestals (designed families, then
  genomes), genome cards on G, release one to fight, reroll the set.
- Tools: `genome` (seed/body/archetype/element/parts, spawn), `bestiary` (spread of N
  genomes: 487 distinct names in 500), `boss` (list/spawn), `turntable` (multi-angle render
  of any creature), `animsheet` (action frames).
- Tests: tests/monsters.rs (every archetype fights, bombers burst, broods, rare affixes/names,
  every boss through all phases incl. a generated one, wave 10 boss, Menagerie release/reroll,
  rewind exact) + genome variety/determinism unit test.

## G5 World — done
- Layout generator (`arpg/levelgen.rs`): rooms on a 32 m grid (random walk for the main path,
  side branches), sizes snapped to 4 m, five room shapes (plain, pillars, ring, split, cross),
  straight corridors with door gaps; `route()` plans a walk through doors and corridors;
  `place_of()`. Unit test: 200 seeds connect, never overlap, routes reach the exit.
- Themes (`game/themes.toml`, 8) and designed levels (`game/levels.toml`, 12, each one
  mechanic + earlier ones mixed in; bosses on 4, 7, 10, 12). `world::plan(depth)` says what a
  depth is; past 12 the endless Depths are a fixed combination per depth: a theme hue-shifted
  and blended with another's lights, 2-4 mechanics, favoured genome archetypes, monster level
  +2 per depth, a boss every third depth (unused designed ones, then generated).
- Builder (`arpg/world.rs`): floor tiles (2 m and player-only crumbling in crumbling rooms, ice
  colours on ice), walls with door gaps and a darker top course, room furniture by shape, wall
  sconces (theme light), props (rocks, crates, columns, crystals, bones, mushrooms), drifting
  particles per room, the exit gate (sealed by red runes while the boss lives), packs by room
  size (theme families or genomes by the theme's element weights), the boss waiting at the
  exit (not aggro until you arrive).
- The twelve mechanics (`arpg/mechanics.rs`, `Feature`s + a rule per tick):
  shrines (six boons, 15 s, stacking time), powder kegs (Neutral actors; blast hurts monsters
  by a share of their life, the hero a little; chain reactions on a fuse), spike plates (on a
  beat with a telegraph), rift gates (paired, shortcut from near the start to near the exit),
  windways (engine conveyor zones + scrolling chevrons), ward totems (monster actors; -60%
  damage taken to monsters near them), lava (burns everyone; monsters walk into it), ice
  (characters slide via the new `Character::slide`; frozen monsters on ice shatter), crumbling
  floors (engine crumble tiles, new `PLAYER_CRUMBLE` flag; falling monsters die, the hero
  climbs back out hurt), darkness (dark mood, the hero's lantern, wells that flare, burn
  monsters and Kindle the hero), cursed chests (G: three waves of keepers, then rare/unique
  loot), time bubbles (monsters, their casts, cooldowns and projectiles slowed; the hero
  Quickened). Hazards credit the hero for kills.
- `Place::Level(n)` (code 100+n, scene `level/<n>`), the way down (`SpotKind::Exit`,
  `GameCmd::Use`), waypoints (`Hero::max_depth`; the portal lists every depth reached), +1
  passive point for each boss level conquered. HUD: level card, intro banner, minimap turned
  with the camera (fog of war, marks), big map on M; level mood (sky, sun, ambient, fog).
- Town: Odo the gambler (mystery items by slot), Mother Wren the alchemist (more and stronger
  potions), Captain Brannoc at the portal, two villagers walking rounds, Biscuit the dog
  following the hero; greetings, coin flips, stirring, hammering. Hood helm kind.
- New body plan **Quadruped** (hounds, wolves, boars; the dog): legs under a raised body with
  backward joints, chest, neck, snout, ears, tail carried high; in the genome with fitting
  archetypes and parts. All creature bodies now coil and lunge when they attack.
- Bot: fights what is near in levels, otherwise walks the route to the exit (fat-ray steering
  around furniture, unsticking), takes the way down. From level 1 it reached level 4 in five
  game-minutes (hero level 7, 111 kills, no deaths).
- Saving: the hero as JSON next to the executable (every travel, every minute, on quit, before
  scene loads/resets), loaded when a game scene starts; "New hero" in the pause menu.
- Tools: `level` (what a depth is, or the live level: features with positions/state),
  `levelmap` (software-rendered top-down map with every piece), `go` (travel anywhere,
  unlocking waypoints), `goto_feature` (stand next to a shrine, keg, gate...), `game_cmd`
  travel to levels and `use`.
- Tests: tests/levels.rs (all 12 designed levels have their mechanics; endless depths
  combine and stay stable; each mechanic works; boss seals the exit; waypoints; rewind exact),
  tests/town.rs (townsfolk alive, gambling, brewing, portal, save round-trip).

## G6 Showcase polish, agent tools, final build — done
- Navigation (`pav_core::nav`): a walkable grid built from the floor blocks, walls, furniture
  and fixed props (grown by a walker's radius), A* paths smoothed by line of sight, and flow
  fields. Levels build it once (`LevelState::nav_grid`); monsters chase the hero around walls
  along a flow field toward the hero's cell (recomputed when the hero changes cell); the bot
  plans its way to the exit with A*. Both are pure functions of the grid and goal (and the
  grid ignores things that come and go), so rewind stays exact.
- Bot: line-of-sight targeting in levels, fat-ray steering, totems first, backing off when
  nearly dead without potions, never fighting on crumbling floor.
- Balance (`campaign` runs): monster level is now 1 + 2 per depth (23 at level 12), bosses one
  above. The bot clears all twelve designed levels in about 90 game-minutes: levels 1-3 in a
  few minutes each without trouble, boss levels cost it 4-24 deaths (level 12's Frostbound
  Colossus is the wall), the rest 0-6. Humans dodge better than the bot; it's meant to be
  challenging, not punishing.
- Juice: crushing blows (crits, 60%+ of life, shatters) burst monsters into physics chunks of
  their colours (budgeted); kill streaks (Rampage / Massacre / Annihilation) pay bonus xp; a
  boss's entrance (banner, hit-stop, shake, shockwave) the first time it sees you; a pillar of
  light over the way down when it opens; dimmer portals in the dark levels; sconce brackets
  are ghosts (a sliding hero got wedged under one).
- Agent tools (`pav_tools/src/agent_tools.rs`): `see` (screenshot with numbered marks on
  everything that matters and a legend: kind, name, rarity, life, distance, position: visual
  grounding for agents), `campaign` (the bot plays down through levels, a row per level, with
  a wall-clock cap), `theme_swatch` (every theme's palette and the endless blends),
  `turntable`/`animsheet def={...}` (author a creature as JSON on top of any family, boss,
  genome or a new body plan; reports its anatomy), `levelmap nav=true` (walkable grid and the
  planned way to the exit). Tool captures now use each place's own light (`pav_view::arpg::
  place_look`, shared with the app).
- Gamepad: D-pad left toggles the map. Browser saves go to local storage. (Menus: see
  "Gamepad menus" below.)
- Tests: tests/feel.rs (gibs, streak bonus, boss entrance), nav unit test (paths round walls,
  flow points the way).

## Gamepad menus — done
- Every window works on a controller through a virtual cursor (pointer events fed to egui):
  left stick (speeds up when pushed fully) or D-pad steps move it, A / X are left / right
  click, holding Y is shift, the right stick scrolls and zooms the tree, B closes. D-pad down
  opens the hero's panels and LB / RB cycle inventory, character, skills, passive tree. While
  a window or the menu is open the pad doesn't reach the game. Hints in the windows switch
  to button names on a pad (`arpg_items::hint`); the menu lists the game's pad controls.
- `--pad-script FILE` plays a scripted controller (`input::PadScript`), so controller flows
  are testable without hardware: used to check opening the bag and equipping by A, cycling
  to the tree, and taking a path with Y+A, with screenshots.

## Decisions (Shardfall)
- The game is part of the simulation (not a separate crate) so every engine feature works on
  it, including rewind mid-fight and the live bridge.
- One skill format for hero and monsters: archetype brains choose among skills, so procedural
  monsters can use any skill.
- Actors live in a side table (`Game::actors`, keyed by entity) instead of new Entity fields.
- Damage numbers and health bars are egui drawn over the 3D view (crisp); trails are additive
  particles (meshes have no transparency).
- Items store their rolled stat and value (not just an affix id), so data edits never change
  existing items; affix keys are kept for tooltips and analysis.
- Base numbers scale with *item level*, not base tier: endless scaling with one table; tiers
  differ by look and implicit size.
- Menu actions are input (GameCmd in the input frame) rather than direct state edits, so the
  replay/rewind guarantees cover trading too.
- Travel rebuilds the whole SimState in place (not a second Sim): history snapshots hold the
  entire state, so rewinding across a portal just works.
- Powers are on actors, fed by items now and by keystones/monster affixes later.
- The passive tree is generated from a small data file (cluster grammar + lanes/gutters layout)
  rather than hand-placed: one design language, easy to extend, and infinite (Astral rings).
- Skill upgrades are tweaks on the actor applied to the skill definition at use, so monsters
  can carry them too (G4 affixes like "extra projectiles").
- Everything that fights is a genome: designed families and bosses are specs/overrides of the
  same pieces the generator uses, so new content is data and endless content is the same
  language. Parts attach to anchors, not to specific skeletons.
- Levels reuse engine features instead of new systems where they fit: conveyors are the wind,
  crumble tiles are the crumbling floor, Neutral actors are kegs. New engine knobs were small
  and general (`Character::slide`, `PLAYER_CRUMBLE`, prop actors with no character).
- An endless depth's *identity* (name, palette, mechanics, boss) is fixed by its number, its
  *layout* is new each visit: players learn what Depth 20 is, but never memorise it.
- Hazards take a share of a monster's life (paying back resistances) rather than flat damage,
  so exploiting mechanics stays worthwhile at any depth.
- The save holds only the hero; places are rebuilt. Travel is the save point.
- Gamepad menus use a cursor rather than focus-hopping between widgets: it works for every
  window as written (grids, popups, the tree canvas) and costs no per-window code.
- Navigation is derived data (never saved): rebuilt on demand from the level's blocks, so
  snapshots stay small and identical whether or not it was built.
- Balance is judged by a bot campaign, not by feel alone: it's repeatable, and agents can run
  it after every data change.
