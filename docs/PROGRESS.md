# Progress log

Current milestone: **M10 Browser build, live bridge, polish** (M1–M9 complete; M4–M10 run as one goal).

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
| M10 Browser build, live bridge, polish | 🔨 in progress (bridge + browser build done) |

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

## M10 Browser build, live bridge, polish — in progress (started early, alongside M9)
- **Live agent bridge** ✅ (a4fdd13): the game listens with `--bridge [ADDR]` (default
  127.0.0.1:7878) or `bridge = "ADDR"` in pavilion.toml. JSON lines over TCP; each request runs a
  registry tool on the simulation thread against the running game (`Session::from_live`), with the
  game's camera and view; camera/view changes come back to the game. Captures use a separate
  headless device. Clients: `pav live [ADDR]` (REPL) and `pav mcp --live [ADDR]` (MCP).
- **Browser build** ✅ (f8975fb): `scripts/build-web.sh` -> `target/web/` (index.html,
  pavilion.js, pavilion_bg.wasm ~13 MB). Same `pav_app` crate with `cfg(target_arch = "wasm32")`:
  the sim is stepped from the frame loop (no threads), GPU setup is async (finishes in
  `about_to_wait`), egui input via a small adapter (`uiinput.rs`; egui-winit is native-only), Web
  Audio (resumed on first input), `web-time` Instant, room from `?room=NAME&seed=N`, no file
  watcher / bridge / screenshots. Verified in headless Chromium (WebGPU on SwiftShader, flags
  `--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader
  --use-angle=swiftshader`): boots, renders the world, plays sound, takes keyboard input.
- Polish pass: ⏳.

## Decisions (M10)
- Bridge requests swap the live `Sim` into a `Session` and back on the sim thread (no copies);
  live sessions skip the session's own room camera/view sync (the game does that).
- Browser build reuses the app crate rather than a separate web crate; wasm-bindgen CLI must
  match Cargo.lock's wasm-bindgen version (0.2.129).
