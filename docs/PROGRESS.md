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
**Agent asset store** (designed 2026-10-15): `docs/AGENT_ASSET_STORE.md`, a library of
foundational assets plus transforms for agents across the engine family. Its nine §14 rulings took
their defaults (`docs/ESCALATIONS.md`). **S0 is done:** `assets find=` matches plurals and
synonyms, says `none` with nearest rows and a route, and logs misses (`store/misses.tsv`);
`assets shelf=prop|clip`; admitted licences (`store/licences.tsv`) and `store/CREDITS.md`;
`find@5` 9/10, absent 5/5. **S1's answer is no, final** (2026-10-15): with the store, a fresh
agent was cheaper on only 2 of 6 room-scale briefs, the two whose assets the store held (about
half the tokens), and 10-30% dearer on the rest. The user then asked for **S2** anyway, and it is
done: `size_band` and `socket_missing` checks, fork lineage, `store/STORE.tsv` and
`store/README.md`, format-2 `params`/`presets`/`NAME:preset`, 28 foundations (four for each of
seven archetypes in `store/foundations.tsv`) and the `library_hall`, `limits` and `catalogue`
rooms. **S3 is done: below critical mass** (2026-10-15): 20 fresh blind briefs over two rounds;
after a fill of 12 foundations (LIB/core has 59 assets, 40 foundations in 15 archetypes), 3 core
materials and search fixes, met/needs reached props 0.29, clips 0.25, materials 0.43 (rule: 0.8);
`bench.py cost` prices stages in US$. **S3b is done:** five CC0 KayKit characters imported as
rigid figures (16 segments each) that play every embedded clip with no check findings, a
`character` store kind, in the Catalogue; skinning waits for the user's verdict on its review
page (pending user). **Next: S4 and S5**, run from the plan's §15 ("Executing S3–S5"): each
built and merged whatever its exit result, within hard test limits.
Shardfall only (the user, 2026-10-15): assets may come in from sibling repositories when they fit
Shardfall's formats, but nothing is written there; S4's my-3dge reader and S5's riftlight sprite
side become hand-off prompts in `docs/handoffs/`.
**Armed, not run:** S6 (fires when the benchmark agent changes) and the §13 triggers.

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
