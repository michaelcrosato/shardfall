# Progress

The current state, kept to one page because every session reads it. The full journal (what was
built, how it works, every decision) is `docs/HISTORY.md`, newest first: search it when you need
the background on something.

## Where things stand (2026-10-10)
- All engine milestones (M1–M10) and Shardfall milestones (G1–G6) are done. Shardfall is the
  hack-and-slash showcase built on the engine (`docs/GAME.md`); the pavilion is the engine's
  tech demo (`docs/DESIGN.md`).
- Since then:
  - **Doctrine** (`docs/DOCTRINE.md`) adopted; `AGENTS.md` says how it applies here.
  - **Mobile:** touch controls, a pared-down HUD, swipe-to-dismiss and performance work.
  - **Animation:** moves as data, motion clips translated from open libraries by the engine's
    own tools (CMU, 100STYLE, Quaternius, M2M; Mixamo, Bandai Namco and LaFAN1 locally), and
    monsters in captured motion. **Animation Studio** adds live LLM and manual editing,
    shared playback, saved workshop clips, revisions, undo/redo, and file reloads
    (`docs/ANIMATION_STUDIO.md`; start with `shardfall --animation-studio`).
  - **Object Studio:** reusable props made from named primitive parts, with the same live
    LLM and manual edit loop (`docs/ASSET_STUDIO.md`; `shardfall --asset-studio`). Atomic JSON
    edits save with revisions and undo/redo, then update the preview and all placed copies.
    Each studio keeps separate controls and cameras. `studio_status` confirms
    native frame submission without a screenshot; native file edits reload after 250 ms.
  - **Creature Studio:** the pinned SpawnForge compiler creates procedural rigs from themes,
    templates or blueprints. Named part and surface edits use shared revisions, disk history,
    asynchronous jobs, and native frame feedback (`docs/CREATURE_STUDIO.md`;
    `shardfall --creature-studio`). The native stage plays baked motion and retains the last
    good result after failed or superseded builds. The Windows package includes portable Node.
  - **Looks:** every filter on a part of the scene, painterly and print styles, volumetric light,
    water, wind, and a station guide in every room.
  - **Playable characters** (`game/heroes.toml`): Kestrel the Brawler (captured boxing and
    karate, fist and kick moves) and Ysolde the Stormcaller (captured spellwork, staff strikes)
    beside the Wanderer, each a hero of their own with a shared stash, chosen in Emberwatch's Hall
    of Heroes, where all three perform. Looks can now name a move or clip per combo swing and a
    captured dodge.
- Builds: the Windows `.exe` (cross-compiled here, played by the user) and the browser build on
  Vercel (https://shardfall-eight.vercel.app).
- Pavilion Lite (`pavilion-lite/`) is frozen: a finished experiment and handoff package.

## Next
**Test games** (`docs/FEEDBACK.md`): fresh agents build a classic's vertical slice on the engine and
evaluate it; each game goes to `michaelcrosato/ai-hub` as one HTML file (`scripts/build-web.sh
--single-file`). First: Capital Strike (Jungle Strike). Larger engine work waits for patterns.

**Asset system plan** (resumed in slim form on 2026-10-12; next rows 5.3, 4.3, then 6.1):
`docs/ASSET_SYSTEM_PLAN.md` (format 2, procedural materials, library import, curated library,
benchmark gates). It was executed with the prompt in its §15, row by row through its §11 checklist.
Phase 0 is done (gates GA and G0 pass): compact asset replies and canonical prop files (0.1),
errors that name `parts.<id>`/`ops[i]` with a fix and `asset_edit action=schema` (0.2), and the
benchmark harness `scripts/asset_bench/` with the arm-A baseline (0.3: median `tok_x` 6,480). Phase
1 is done (gate G1 passes: median `tok_x` 0.74 and `tok_cum` 0.88 of arm A's, 14/14 accepted
against 11/14, no lint findings left): `asset_edit action=check` and findings in every edit reply
(1.1), contact sheets `asset_preview views=true` and `capture stats=only` (1.2), agent-first
`docs/ASSET_STUDIO.md` with the live studio in `docs/ASSET_STUDIO_LIVE.md`, and calibrated judges
(1.3). Phase 2's rows are done: format 2 is read, checked (errors with fixes for common guesses)
and written canonically with `prop2-` revisions (2.1), and compiles to resolved parts with
generated meshes, seeded variants and compiled definitions in the library; every §5.9 golden holds
and format-1 parts compile bit-exactly (2.2). Drawing, collision, nav, bounds, previews, `check`
and the native studio read compiled parts, so format-2 assets are created, drawn and placed;
format-1 assets draw and collide exactly as before (2.3). Format-2 assets take patch ops (rename,
copy, mirror, array, expand, move, scale, unit, dotted targets), snapshots carry each definition
once, and placed copies take a seed and a palette (2.4). `docs/ASSET_STUDIO.md` teaches format 2
and `BUILTIN/bench` is fixed (2.5). **Gate G2 is a NO-GO** after its one remediation round, though
no kill criterion held. Round 1 met quality (15 of 24 pairs won) but not tokens (`tok_x` 0.86,
`tok_cum` 1.22 of arm A's). Round 2, after the docs and error fixes, met tokens (`tok_x` 0.51,
`tok_cum` 0.56, edits 0.39, 29/30 accepted) but not quality (10 of 24 pairs, rubric Δ −0.18).
Those token ratios count wrapper traffic only: end to end its runs cost about 9% less than arm
A's, since each model call re-reads a ~55k-token fixed prompt and they make more calls (HISTORY).
The user then closed G2 (format 2 stays) and resumed Phases 3-6 in slim form: only features that
are needed and showed a benefit, with lean gates (plan §10.3; deferred items listed in §11).
Procedural materials exist as data with a CPU reference evaluator (`pav_core::material`, 12 core
materials; 3.1), and the renderer draws them exactly as it evaluates (3.2: a storage-buffer
uber-shader, `pav_view::materials::MaterialTable`, parity 100% within 1/255, +16%/+48% frame
time, `scripts/check-wgsl.sh`). Props and statics wear them (3.3): `"mat":"MAT/oak_planks"` or
a materials-table entry on any part, `Block`/`Decor` `material`, workshop materials
`MAT/workshop/x` through `asset_edit kind=material` (`mat1-` revisions), swatches with per-layer
legibility from `asset_preview kind=material`, `docs/MATERIALS.md`. Every dungeon theme wears
them (3.4): `themes.toml` `floor_mat/wall_mat/pillar_mat/cap_mat` give all 8 themes flagstone
floors, dressed-stone walls, drum pillars and coping stones in their own colours (the level's
palette rides in `Mood` to the view), identical levels and mechanics, and `view.material_detail`
(`full`, `first_layer` on phones, `off`); its G3 review is 8 before/after captures for the user.
Rows 5.1 and 5.2 are done early: the importers share `pav_tools/src/import/`; `asset_import` reads
glTF and OBJ into one `Scene`, downloads pinned packs, translates models into format-2 props
(primitives, mirrors and arrays, plus one read-only vertex-coloured mesh part the engine now
draws) and measures them on the CPU (six-view IoU, ΔE, Chamfer, tier; `check=`, `compare=`).
Kenney Mini Dungeon: median worst-view IoU 0.983 at 293 tokens, byte-identical rebuilds. Next for
import: row 5.3 (catalogs, licence gate, the pilot).
Row 4.1 is done: library names and tiers (`LIB/<pack>/x`, `LIB/core` embedded, other packs on
disk, `assets/props/local` kept off git) with budget tests, `assets find=` with ranking, filters,
`similar=` and ~35-token rows read from each folder's `index.tsv`, and
`pav_view::assets::emit_asset`, which draws a library asset from a game's own state each frame
(wearing its materials).
Row 4.2's tooling is in (part 1): `PACK.json` licences with `assets/props/CREDITS.md`,
`asset_edit action=publish` into `LIB/core` behind a library gate and a review ledger, and
`scripts/review_page.py` (review pages whose verdicts the agent reads back); part 2 put 28
reviewed exemplars into `LIB/core` (the six hard-coded Shardfall kinds as assets, dungeon dressing,
town and furniture), all accepted by the user, who then ruled that such assets need no routine
review (AGENTS.md, "Testing budget").

New content or polish: add data (themes, levels, families, affixes, uniques, tree clusters,
characters) and check it with the tools (`levelmap`, `see`, `campaign`, `turntable def=`,
`turntable character=`, `animsheet character= skill= swing=`).

## Open issues
- The HUD, inventory, passive tree and touch controls are invisible to the tools and headless
  captures (doctrine 2, 3); game actions are reachable through `game_cmd`.
- Older known issues and limits (engine rooms, browser build) are listed under "Known issues and
  limits" in `docs/HISTORY.md`.
- The `campaign` bot stalls on level 4, the Rift Gates (seed 1: 113 kills at 15 game minutes and
  still at 60, the boss's seal shut), in the 3.3 build and after it alike (HISTORY, row 3.4).
- Creature authoring uses the native Node sidecar. Full SpawnForge shader materials and
  spawning generated rigs as gameplay enemies are separate work.

## Keeping this page current
When you finish something, write it up as a dated entry at the top of `docs/HISTORY.md` (what
changed, how it works, the decisions), then update the three sections above. Keep this page to
about one screen.
