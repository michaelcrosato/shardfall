# Progress

The current state, kept to one page because every session reads it. The full journal (what was
built, how it works, every decision) is `docs/HISTORY.md`, newest first: search it when you need
the background on something.

## Where things stand (2026-10-09)
- All engine milestones (M1–M10) and Shardfall milestones (G1–G6) are done. Shardfall is the
  hack-and-slash showcase built on the engine (`docs/GAME.md`); the pavilion is the engine's
  tech demo (`docs/DESIGN.md`).
- Since then:
  - **Doctrine** (`docs/DOCTRINE.md`) adopted; `AGENTS.md` says how it applies here.
  - **Mobile:** touch controls, a pared-down HUD, swipe-to-dismiss and performance work.
  - **Animation:** moves as data, motion clips translated from open libraries by the engine's
    own tools (CMU, 100STYLE, Quaternius, M2M; Mixamo, Bandai Namco and LaFAN1 locally), and
    monsters in captured motion.
  - **Looks:** every filter on a part of the scene, painterly and print styles, volumetric light,
    water, wind, and a station guide in every room.
- Builds: the Windows `.exe` (cross-compiled here, played by the user) and the browser build on
  Vercel (https://shardfall-eight.vercel.app).
- Pavilion Lite (`pavilion-lite/`) is frozen: a finished experiment and handoff package.

## Next
New content or polish: add data (themes, levels, families, affixes, uniques, tree clusters) and
check it with the tools (`levelmap`, `see`, `campaign`, `turntable def=`).

## Open issues
- Vercel: PR #17's preview deployment failed within seconds, before building anything (a Markdown-only
  change). The reason is only in the Vercel dashboard; until a deployment succeeds, the live
  site stays on the last good build.
- The HUD, inventory, passive tree and touch controls are invisible to the tools and headless
  captures (doctrine 2, 3); game actions are reachable through `game_cmd`.
- Older known issues and limits (engine rooms, browser build) are listed under "Known issues and
  limits" in `docs/HISTORY.md`.

## Keeping this page current
When you finish something, write it up as a dated entry at the top of `docs/HISTORY.md` (what
changed, how it works, the decisions), then update the three sections above. Keep this page to
about one screen.
