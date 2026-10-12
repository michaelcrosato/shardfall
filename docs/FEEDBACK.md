# Test-game feedback

Engine feedback from test games: a fresh agent builds a vertical slice of a 1985-2015 classic on
this engine, then evaluates the engine. Each game is stored as one self-contained HTML file in
`michaelcrosato/ai-hub` (`games/`, source zip in `sources/`). The maintainer checks each claim
against the code, fixes what blocks the next game, and waits for patterns before larger work:
features everyone uses, features no one uses, problems that keep coming back. Newest game first
under "Games"; the tally is updated with every game.

**Prompt.** The user's template, with the game's name, year and platforms as SOURCE: build an
original spiritual successor as one complete, polished level that doubles as a tech demo; use the
engine, extend it as needed; deliver one self-contained HTML file; then a one-page candid
evaluation of the engine (used/not used, engine work you wrote ranked by output saved, engine
changes, bugs and wrong docs, source you had to read, fidelity limits, testing, what helped and
hurt, missing features by impact, the one change). The questions get adjusted as patterns show.

**Cost** is counted end to end from the agent's transcript (model calls, tokens read, weighted
cost at cache reads 0.1, cache writes 1.25, visible output 5; hidden reasoning is not recorded).

## Tally (1 game)
| Asked for | Games | Impact | Status |
|---|---|---|---|
| A supported second-game path: a `Game` hook for step, frame, view and HUD instead of editing `sim.rs`, `frame.rs`, `build.rs` and `app.rs` | 1 | high | waiting for a pattern |
| Entities, shots, health and AI that don't require rapier (`health.rs`, `projectile.rs`) | 1 | high | waiting |
| A single-file web build (gzip + inline) | 1 | high | **done**: `scripts/build-web.sh --single-file` |
| Primitive-part models drawn per frame from game state (props can't be) | 1 | medium | in the asset plan, row 4.1 |
| Stock HUD widgets (bars, radar, off-screen markers, boss bar, banner) | 1 | medium | waiting |
| Data-driven sound bank (`pav_audio::sounds_for` is a closed match) | 1 | medium | waiting |
| Per-scene default camera on `load` | 1 | medium | waiting |
| HUD visible to headless capture | 1 | medium | known gap |
| Open top-down map data (roads, water, spawns) | 1 | low | waiting |
| Continuous-play and top-down map captures | 1 | low | waiting |

| Engine part | Used | Skipped |
|---|---|---|
| `Sim` (rewind, `state_hash`, snapshot), `pav_render::Scene`, particle presets, `pav_audio` synth, `pav_tools` capture/repl/input, the wasm shell and egui, `Rng`/`InputFrame`/bindings | 1 | 0 |
| rapier entities, `character.rs`, `nav.rs`, `health.rs`, `projectile.rs`, `vehicle.rs` helicopter, `course.rs`, `zones.rs`, `stealth.rs`, `ai.rs` | 0 | 1 |
| Rooms/TOML, puppet, moves, clips, the asset and creature studios (props, format 2) | 0 | 1 |

## Games

### Capital Strike, from Jungle Strike (1993; SNES, Genesis) — 2026-10-12
- **Result:** one mission over Washington. Gun, rockets, homing missiles, flares and a dash;
  armour, fuel and ammo; winch rescues; a motorcade escort; SAMs, AA guns, tanks, gunships and a
  three-phase boss; loadouts, difficulties, checkpoints, radar and a procedural soundtrack.
  `ai-hub/games/Jungle-Strike-Capital-Strike-2026-10-12-model-unknown.html` (9.8 MB); source as a
  patch series against `6618522` in `ai-hub/sources/`.
- **Agent:** Sonnet at medium effort, a subagent in its own worktree of this repo.
- **Cost:** 1.65 h, 170 model calls, 54M tokens read (context grew from 56k to 485k), about 6.5M
  weighted; 106k tokens of visible output. About 4,270 lines written, 3,965 of them game code
  inside the engine crates.
- **Verdict (agent's):** the renderer, wasm shell, audio synth and headless tools carried the
  visuals and the first wasm build ran; the gameplay half didn't help a non-RPG, non-physics game,
  so 4,000 lines went into a game layer with no engine facility, and the packaging was all its own.
- **Checked:** the claims about `vehicle.rs` and `health.rs` hold (rapier-bound; no weapons,
  damage or fuel), and AGENTS.md now says so. The game didn't touch props, format 2 or the studios:
  its units are SDF and cube parts drawn in code, because a prop can't be drawn from game state.
- **Acted on:** the single-file web build (every test game needs one), the AGENTS.md wording, and
  the asset plan's row 4.1 now includes drawing a library asset from game code. Everything else
  waits for a second game.
