# Progress log

Current milestone: **M3 World & rooms** (M1, M2 complete).

## Status by milestone
| Milestone | State |
|---|---|
| M1 Foundation | ✅ complete (2026-09-30) |
| M2 Core gameplay | ✅ complete (2026-09-30) |
| M3 World & rooms | ⏳ next |
| M4–M10 | pending (separate goal) |

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

## Next (M3 World & rooms)
1. Room framework: room files in `rooms/` (+ built-in copies), metadata (name, about/info card,
   primary device, movement model, camera defaults, controls), enter/exit, reset, teleport
   menu, `--room` launch, control guide on entry.
2. Hot reload of room files (notify) with validation errors shown in-game and via a tool.
3. Chunk streaming around interest points; pavilion hub at the centre with doors to rooms;
   procedural wilderness terrain regenerated from the seed; changed objects persist.
4. Sandbox editing: spawn, drag, delete, save room to a file.
5. Synth sound v1 (cpal output + offline .wav rendering), events -> sounds.
6. Filmstrip capture tool.
