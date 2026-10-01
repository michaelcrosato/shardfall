# Progress log

All engine milestones M1–M10 and all Shardfall milestones G1–G6 are complete (Shardfall is
the showcase hack-and-slash built on the engine; design: `docs/GAME.md`; progress: the
*Shardfall* sections at the end of this file). Next work, if any, is new content or polish:
add data (themes, levels, families, affixes, uniques, tree clusters) and check it with the
tools (`levelmap`, `see`, `campaign`, `turntable def=`).

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
- Gamepad: D-pad left toggles the map. Browser saves go to local storage.
- Tests: tests/feel.rs (gibs, streak bonus, boss entrance), nav unit test (paths round walls,
  flow points the way).

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
- Navigation is derived data (never saved): rebuilt on demand from the level's blocks, so
  snapshots stay small and identical whether or not it was built.
- Balance is judged by a bot campaign, not by feel alone: it's repeatable, and agents can run
  it after every data change.
