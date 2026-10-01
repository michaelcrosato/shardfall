# Progress log

Current milestone: **M5 Physics Lab** (M1–M4 complete; M4–M10 run as one goal).

## Status by milestone
| Milestone | State |
|---|---|
| M1 Foundation | ✅ complete (2026-09-30) |
| M2 Core gameplay | ✅ complete (2026-09-30) |
| M3 World & rooms | ✅ complete (2026-10-01) |
| M4 Movement & Feel Lab | ✅ complete (2026-10-01) |
| M5–M10 | ⏳ next |

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

## Next: M5 Physics Lab
Rooms (wing = "physics", north corridor): stacking & toppling, springs & soft bodies (rapier 0.36
soft bodies: cuboid/sphere/cloth/rope), chains & rope bridges (joints in room data + chain/bridge
generators), conveyors, bounce & friction gallery (spawners, tilted ramps), destructible floors
(crumbling and impact-breakable tiles), stress test with a physics stats overlay and spawn pads
(signals from pads to spawners). Renderer: per-frame dynamic meshes for soft bodies.
Helper-agent tips: give each helper its own scratch subdirectory; keep briefs explicit about
coordinates, tools and what to verify.
