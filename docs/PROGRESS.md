# Progress

The current state, kept to one page because every session reads it (Claude Code loads it through
`CLAUDE.md`). The full journal (what was built, how it works, every decision) is
`docs/HISTORY.md`, newest first: search it when you need the background on something.

## Where things stand (2026-10-16)
- All engine milestones (M1–M10) and Shardfall milestones (G1–G6) are done. Shardfall is the
  hack-and-slash showcase built on the engine (`docs/GAME.md`); the pavilion is the engine's
  tech demo (`docs/DESIGN.md`).
- Since then: the doctrine (`docs/DOCTRINE.md`; `AGENTS.md` says how it applies); mobile touch
  controls; moves and clips as data from open libraries, with the **Animation**, **Object** and
  **Creature Studios** (`docs/*_STUDIO.md`); looks (filters per part, painterly, volumetric
  light, water, wind); three playable characters (`game/heroes.toml`); the asset system
  (format-2 props, procedural materials, glTF/OBJ import behind a licence gate, `LIB/core`).
- **Agent asset store** (`docs/AGENT_ASSET_STORE.md`): stages S0–S5 built (2026-10-15). A fresh
  agent searches and places store entries (`assets find=`, `shelf=`); 59 core props, 18
  materials, 437 clips, five KayKit characters as rigid figures. Measured: a library pays only
  where it holds the asset (S1 no; S3 props met 0.29 of needs against 0.8).
- **Game makers' handoff** (2026-10-16): an agent making a new game reads one page,
  `docs/MAKE_A_GAME.md`, and copies the starter game (`load scene=starter`): the game hook
  `pav_core::games` (rules, HUD text, status, a bot), its level a room file, `play bot=true
  seeds=10`. `START_HERE.md` routes and prompts; `scripts/check-handoff.sh` checks the archive.
- **On show in the world demo:** the Workshop wing's Library Hall, Limits, Catalogue and Store
  Showcase, and the animation wing's Clip Browser (every clip on the puppet and the five figures).
  The figures ship in every build (`props::store::SHIPPED`).
- Builds: the Windows `.exe` (cross-compiled here, played by the user) and the browser build on
  Vercel (https://shardfall-eight.vercel.app). Pavilion Lite (`pavilion-lite/`) is frozen.

## Next
- **Gameplay and content:** add data (themes, levels, families, affixes, uniques, tree clusters,
  characters) and check it with the tools (`levelmap`, `see`, `campaign`, `turntable`,
  `animsheet`).
- **Co-op** (`docs/COOP_PLAN.md`): up to four players roaming one world. Planned in stages
  C0-C6, not started; rulings E1-E3 asked, each with a default.
- **Test games** (`docs/FEEDBACK.md`): fresh agents build a classic's vertical slice on the
  engine from `START_HERE.md`'s "Make a game" prompt; each goes to `michaelcrosato/ai-hub` as
  one HTML file. One so far (Capital Strike, before the hook).
- **Waiting on the user:** S3b's rigid-or-skinned verdict
  (https://claude.ai/artifact/VjP2RUbwaSJ3ggXkrJJnQs); S4's and S5's other halves run as
  sessions in my-3dge and riftlight from `docs/handoffs/` when the user starts them.
- **Armed, not run** (`docs/AGENT_ASSET_STORE.md` §13): S6 phase-out at each change of the
  benchmark agent, and the triggers listed there, each with a small plan when it fires.

## Open issues
- The HUD, inventory, passive tree and touch controls (egui in `pav_app`) are invisible to the
  tools and headless captures (doctrine 2, 3); game actions are reachable through `game_cmd`.
- The `campaign` bot stalls on level 4, the Rift Gates (seed 1: the boss's seal stays shut).
- Copies of one asset patched to different materials under one key draw the same material.
- Older known issues and limits: "Known issues and limits" in `docs/HISTORY.md`.

## Keeping this page current
When you finish something, write it up as a dated entry at the top of `docs/HISTORY.md` (what
changed, how it works, the decisions), then update the sections above. Keep this page under 60
lines: `pav_tools/tests/docs_limits.rs` fails past that, so move detail to HISTORY.
