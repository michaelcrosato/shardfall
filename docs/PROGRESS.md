# Progress

The current state, kept to one page because every session reads it (Claude Code loads it through
`CLAUDE.md`). The full journal (what was built, how it works, every decision) is
`docs/HISTORY.md`, newest first: search it when you need the background on something.

## Where things stand (2026-10-15)
- All engine milestones (M1–M10) and Shardfall milestones (G1–G6) are done. Shardfall is the
  hack-and-slash showcase built on the engine (`docs/GAME.md`); the pavilion is the engine's
  tech demo (`docs/DESIGN.md`).
- Since then:
  - **Doctrine** (`docs/DOCTRINE.md`) adopted; `AGENTS.md` says how it applies here.
  - **Mobile:** touch controls, a pared-down HUD, swipe-to-dismiss and performance work.
  - **Animation:** moves as data, motion clips translated from open libraries by the engine's
    own tools (CMU, 100STYLE, Quaternius, M2M; Mixamo, Bandai Namco and LaFAN1 locally), and
    monsters in captured motion. **Animation Studio** edits clips live, by an LLM or by hand
    (`docs/ANIMATION_STUDIO.md`; `shardfall --animation-studio`).
  - **Object Studio:** reusable props made from named primitive parts, with the same live edit
    loop (`docs/ASSET_STUDIO.md`; `shardfall --asset-studio`).
  - **Creature Studio:** procedural rigs from the pinned SpawnForge compiler, with the same loop
    (`docs/CREATURE_STUDIO.md`; `shardfall --creature-studio`).
  - **Looks:** every filter on a part of the scene, painterly and print styles, volumetric light,
    water, wind, and a station guide in every room.
  - **Playable characters** (`game/heroes.toml`): Kestrel the Brawler and Ysolde the Stormcaller
    beside the Wanderer, each a hero of their own with a shared stash, chosen in Emberwatch's Hall
    of Heroes. Looks can name a move or clip per combo swing and a captured dodge.
  - **Asset system** (`docs/ASSET_SYSTEM_PLAN.md`, closed in slim form on 2026-10-13; its §11
    lists what it deferred): format-2 props (params, presets, seeded variants; format-1 props draw
    exactly as before), 12 procedural materials the GPU draws exactly as the CPU evaluates them,
    dungeon themes that wear them, glTF/OBJ import measured on the CPU behind a licence gate
    (CC0/CC-BY packs to `assets/props/lib/`, the rest to the git-ignored local tier), `LIB/core`
    embedded in every build, and Shardfall's levels dressed with its props from `game/props.toml`.
    The benchmark harness is `scripts/asset_bench/`; gate G2 was a NO-GO that the user closed
    (format 2 stays).
- Builds: the Windows `.exe` (cross-compiled here, played by the user) and the browser build on
  Vercel (https://shardfall-eight.vercel.app).
- Pavilion Lite (`pavilion-lite/`) is frozen: a finished experiment and handoff package.

## Next
**Agent asset store** (`docs/AGENT_ASSET_STORE.md`; every stage's journal in HISTORY):
foundational assets plus transforms for agents. **S0:** find with plurals, synonyms, `none` and a
route; the shelf; admitted licences and credits. **S1:** no, final (the store paid only where it
held the asset). **S2 (the user's call):** checks, lineage, `STORE.tsv`, format-2 knobs and
presets, foundations, the Hall, Limits and Catalogue rooms. **S3 is done: below critical mass** (2026-10-15): 20 fresh blind briefs over two rounds;
after a fill of 12 foundations (LIB/core has 59 assets, 40 foundations in 15 archetypes), 3 core
materials and search fixes, met/needs reached props 0.29, clips 0.25, materials 0.43 (rule: 0.8);
`bench.py cost` prices stages in US$. **S3b is done:** five CC0 KayKit characters imported as
rigid figures (16 segments each) that play every embedded clip with no check findings, a
`character` store kind, in the Catalogue; skinning waits for the user's verdict on its review
page (pending user). **S4 is done** on Shardfall's side: ten CC0 clips readable from their
store rows, their body points in mm as a fixture; the cross-engine match is pending a my-3dge
session (`docs/handoffs/S4-my-3dge.md`). **S5 is done** on Shardfall's side: its arm (store
props under the `16-bit` pixel look) renders the 3 briefs; §11's criteria wait for a riftlight
session (`docs/handoffs/S5-riftlight.md`). The plan's S0-S5 are built; what remains is armed.
Shardfall only (the user, 2026-10-15): assets may come in from sibling repositories when they fit
Shardfall's formats, but nothing is written there.
**Armed, not run** (`docs/AGENT_ASSET_STORE.md` §13), each with a small plan when it fires:
- **S6 phase-out, per kind** (test limit $0 until then): fires at each change of the benchmark
  agent; the sprite trial reruns on the same event.
- **Harvester** (lift reusable files from test games): fires when 3 games list reusable files in
  store formats (FEEDBACK's new prompt lines collect them).
- **New kinds** beyond props, materials, clips and characters: fire when a game needs one.
- **Several voters:** a second voter. **Lineage reports:** 3 forks change one field. **Lock
  file:** a build needs one. **Semantic lane:** held-out recall below 8/10.
- **Props and feel across engines:** my-3dge gains a model importer and headless capture.
- **Clip federation and a separate repository** (§12): a second engine needs props or materials.
- **JSON Schema:** a TypeScript engine reads entries.
- **Pending elsewhere:** S3b's rigid-or-skinned verdict (the user); S4's cross-engine match (a
  my-3dge session); S5's criteria (a riftlight session).

**Test games** (`docs/FEEDBACK.md`): fresh agents build a classic's vertical slice on the engine
and evaluate it; each game goes to `michaelcrosato/ai-hub` as one HTML file
(`scripts/build-web.sh --single-file`). One so far, Capital Strike (from Jungle Strike); larger
engine work waits for patterns.

New content or polish: add data (themes, levels, families, affixes, uniques, tree clusters,
characters) and check it with the tools (`levelmap`, `see`, `campaign`, `turntable def=`,
`turntable character=`, `animsheet character= skill= swing=`).

## Open issues
- The HUD, inventory, passive tree and touch controls (egui in `pav_app`) are invisible to the
  tools and headless captures (doctrine 2, 3); game actions are reachable through `game_cmd`.
  Fix it when it gets in the way.
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
