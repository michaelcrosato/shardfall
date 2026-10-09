# AGENTS.md — rulebook for coding agents

Shardfall is a procedural hack-and-slash game built on the Pavilion engine, written in Rust. The design is in
`docs/DESIGN.md` (source of truth); progress and decisions are in `docs/PROGRESS.md`. Read both
before starting; update `docs/PROGRESS.md` when you finish something.

For a standalone Linux source handoff, `START_HERE.md` covers setup and headless CLI examples.

## Principles (priority order)
1. **Gameplay first, then robustness.** Never paint the engine into a corner; keep options open.
2. **Use tools.** If a tool does a job faster, more accurately or cheaper over time, use or build one.
3. **Optimization and compatibility come later.** It must still run well on a modern PC.
4. **Approximations are fine if they feel right.** Feel beats physical accuracy.
5. **Check selectively.** Verify what often breaks or is expensive to debug; skip routine checks
   of things that almost never break. "If it's broken, the user will say so" is valid.
6. **Token efficiency over speed.** Prefer the CLI/capture tools over reading lots of code.

## Architecture map
```
crates/
  pav_core    simulation, no window/GPU/audio deps: Sim, entities, physics (rapier3d),
              static regions (statics.rs: terrain chunks, rooms, hub; active or dormant),
              world.rs (pavilion layout, streaming, room tracking), room.rs (room files),
              terrain.rs (procedural wilderness), tile levels (level.rs), characters
              (character.rs: movement models instant/momentum/grid/committed, jump,
              crouch/crawl, ladders, ledge grab/mantle, swimming, axis lock, moving platforms,
              knockback, pushing shares momentum, bombs, ice slide), puppet (puppet.rs: body
              plans biped/spider/lizard/beetle/blob/quadruped, procedural animation, hit recoil,
              creature lunges, foot IK, legs that stride the way the body travels, idle breathing
              and blinks, cutout look and camera cheats; the biped is a `Skel` of joints that the
              procedural animation, moves and clips each produce and `dress` turns into parts),
              moves.rs (attacks and gestures as data, anim/moves.toml: arcs around the shoulders,
              wind-up from wherever the hands were, strikes with lunge/lean/twist/hop/spin,
              follow-through; skills name a move), clips.rs (motion clips: readable key poses
              translated from open animation libraries, retargeted onto the biped with IK,
              crossfaded over the procedural animation, upper body only or mirrored; performers,
              idle clips, walk clips played as fast as the character moves), anim.rs (the /anim files: embedded, live reload), rig.rs (creature feet that plant and step,
              follow-the-leader spines, verlet tails/antennae), ai.rs (NPC brains: idle,
              wander, patrol, circle, follow; they drive characters through InputFrames),
              nav.rs (walkable grid from blocks and props, A* paths, flow fields),
              fxdef.rs (object lights / particle emitters / distortion, view-only),
              vehicle.rs (drift car on rapier's raycast vehicle, arcade helicopter),
              health.rs (shootable objects: hp, score, boss phases/bar, finish),
              stealth.rs (guard sight cones with line of sight and an alert meter),
              zones.rs (trigger zones + labels), course.rs (timers, gates, checkpoints, pits,
              pads, camera cues, hits/respawn), behaviors.rs (movers, rotators, emitters,
              spawners), projectile.rs (lightweight bullets), feel.rs (feel metrics),
              softbody.rs (rapier soft bodies: jelly, balls, cloth, ropes), joints.rs (joints
              owned by entities, recreated on wake), destruct.rs (crumbling/breakable tiles,
              conveyors and bounce pads for props), history, params
  pav_render  wgpu renderer (Vulkan/DX12): Scene description -> particles (compute) -> sun +
              point-light shadow passes (shadows.rs) -> MSAA scene pass (+ GPU particles) ->
              bloom chain and distortion (fx.rs) -> composite (outlines, screen-space GI,
              tonemap, filter stack: pixelate/CRT/scanlines/dither/palettes/grading, split;
              pixel art on part of the scene: objects flagged `flags::PIXEL`, chosen in the view
              by `view.filter.pixel_target` = all/characters/hero/others/world/entity/objects/
              environment; every filter can aim at a part: `flags::OBJECT` marks characters &
              objects, the normal buffer carries it, `*_on` = all/objects/environment; painterly and
              print styles: Kuwahara oil paint, CMYK halftone, ASCII, pencil sketch (`filter.stylize`);
              hazy air: volumetric sun shafts ray-marched through the sun's shadow map and lamp
              halos in closed form (`view.haze`, `shafts`, `halos`); screen transitions (iris,
              diamonds, dissolve, mosaic, blinds, fade); wind in the vertex shader for instances
              flagged `flags::SWAY` (leaves) / `flags::GRASS` (tufts that part around the player)).
              Procedural meshes + analytic SDF spheres/capsules/rounded cones, SDF-font text in
              the world (text.rs). Offscreen capture -> PNG.
  pav_view    sim frame -> render Scene: camera rig (tilt/yaw/distance/fov/ortho, all live),
              look.rs + looks.toml (the Look & Filters layer: filter sections, presets, whole looks),
              water.rs (water zones ripple: a CPU wave equation on a height grid, stirred by
              whatever crosses the waterline, splashes, blasts and rain; drawn as a dynamic mesh),
              interpolation between ticks, visual settings (ViewSettings)
  pav_audio   synthesized sound: oscillators/noise/envelopes/filters, event -> sound bank,
              cpal output (optional), offline .wav rendering
  pav_tools   agent layer: tool registry + `pav` CLI (one-shot, REPL) + MCP stdio server +
              live bridge client (bridge.rs), and the motion importer (mocap/: glTF libraries,
              CMU's ASF/AMC, BVH; loops cut at their best cycle; fitted into readable key poses,
              number for number what my-3D2dge's own importer writes)
  pav_app     the game (`shardfall` binary): window, boot diagnostics, input (keyboard/mouse,
              gamepad via gilrs), system keys, tuning panel, pause menu, sim thread
              (simhost.rs; stepped from the frame loop in the browser), live bridge server
              (bridge.rs), egui input (uiinput.rs). Builds natively and for wasm32 (WebGPU).
anim/         animation data, embedded and hot-reloadable (`anim_reload`): moves.toml (the moves table)
              and clip sets (*.json: QUATERNIUS, MESH2MOTION, CMU, STYLE100: 423 clips with credits).
              anim/cmu/ holds every take of the CMU database (4,770 clips) on disk only: `clips
              load=cmu`. anim/catalogs/ says how each set is made (tags, descriptions, sources,
              picks): `clip_import catalog=...` rebuilds it from the libraries.
rooms/        room data files (TOML: info card, station guide `[learn]`, wing, primary device, movement model, camera,
              params, keys, entrance, ASCII tile layers + legend, [[object]]s). Every file
              here is embedded in the exe at build time AND hot-reloaded at runtime.
              `_template.toml` documents every field.
```
Data flow: `Sim::step(InputFrame)` (fixed tick, own thread) -> `Sim::frame()` -> `RenderFrame`
-> `pav_view::ViewBuilder::build` (interpolates prev/curr) -> `pav_render::Scene` -> `Renderer`.

Key rules:
- `pav_core` must never depend on graphics, windowing or audio crates.
- Everything that changes during play lives in `SimState` (cloning it = snapshot). Keep it `Clone`.
- Gameplay code reads plain struct fields; expose tunables via `impl Tunable` (see `params.rs`).
  Tunables are automatically in the tuning panel, the `params`/`set` tools and preset files.
- Colors in data are sRGB hex (`Color::hex("#e8704a")`); internally linear.
- No asset files (images, models, audio). Geometry, effects and sound come from code. Fonts OK.
  Animation clips are allowed as readable key poses (anim/*.json, the format `clips.rs`
  documents): plain text a model can read and edit, translated from openly licensed libraries
  with `clip_import`. Keep each set's credit and licence; never add binary animation files.
  Downloaded captures (.glb, .amc, .bvh) go to .cache/mocap (git-ignored), never into the repo.
- Physics precision switch: in `crates/pav_core/Cargo.toml` change `package = "rapier3d"` to
  `"rapier3d-f64"`.

## Build, run, test
```sh
scripts/setup-linux.sh             # once per machine (apt packages, lavapipe, mingw, wine)
cargo build                        # everything (Linux)
cargo test                         # unit tests
cargo run -p pav_app               # the game (needs a display; Xvfb works with lavapipe)
scripts/build-windows.sh           # -> target/x86_64-pc-windows-gnu/dist/shardfall.exe
scripts/smoke-windows.sh 25        # run the .exe under Wine+lavapipe, screenshot + log in out/smoke/
scripts/smoke-linux.sh 10          # same for the native Linux build
scripts/build-web.sh               # browser build -> target/web/ (serve over HTTP; Chrome/Edge)
```
Browser build: `cfg(target_arch = "wasm32")` branches live in `pav_app` only (no threads, async
GPU setup, no files). Check it with `cargo clippy -p pav_app --target wasm32-unknown-unknown`.
Headless Chromium can run it (WebGPU on SwiftShader): launch with `--enable-unsafe-webgpu
--enable-features=Vulkan --use-vulkan=swiftshader --use-angle=swiftshader` (other flag sets
show a blank canvas). `?room=NAME&seed=N` picks the start room.
From WSL2 you can launch the Windows build directly: `./target/x86_64-pc-windows-gnu/dist/shardfall.exe`.

## Agent CLI (`pav`) and MCP
Every tool works headless (captures use lavapipe when there is no GPU). The same tools are an
MCP server: `.mcp.json` registers `pav mcp` (stdio), so Claude Code agents in this repo get
them as `mcp__shardfall__*` tools (captures come back as images).
```sh
cargo run -q -p pav_tools --bin pav -- help
pav bench ticks=1200                          # ticks/sec
pav capture scene=test ticks=300 out=out/a.png
pav set path=camera.tilt value=90             # (one-shot: pointless alone; use the REPL)
printf 'step ticks=200\ncamera preset=top\ncapture out=out/b.png\n' | pav repl
printf 'input move=[1,0] ticks=30\ninput press=jump move=[0,1] ticks=40\nplayer\n' | pav repl
```
Live bridge: start the game with `--bridge` (or `bridge = "127.0.0.1:7878"` in shardfall.toml),
then `pav live` is a REPL into the running game and `pav mcp --live` an MCP server for it. Same
tools; they act on what is on screen (captures render the game's camera on a second device).

Tools: `scenes load step status entities params set camera capture bench gpu player input spawn
despawn teleport rewind snapshot_save snapshot_load record_save replay rooms room goto
room_reset room_check room_reload stream filmstrip camera_bench course feel audio_capture`;
Shardfall: `game hero monster autoplay skills game_reload loot_roll give inventory game_cmd tree
tree_map genome bestiary boss turntable animsheet level levelmap go goto_feature see campaign
theme_swatch`; looks: `look`; teaching: `guide`; animation: `clips clip_import mocap anim_reload`
(`pav help` for args).
- Shardfall (scenes `town`, `arena`): `game` is the status, `autoplay seconds=30` lets a bot
  fight, `loot_roll level=40 count=5000` summarises loot tables without playing, `give
  unique=skyfall equip=true` hands over gear, `inventory` describes everything worn/carried,
  `game_cmd do=sell id=12` does any menu action exactly as the player would (it rides in the
  input frame, so replays include it). `tree find=fire`, `tree take=Unbowed` (allocates the
  path), `tree_map` (PNG of the generated passive tree), `genome seed=7 parts=wings,horns`
  (grow a creature), `turntable seed=7` / `family=` / `boss=` (render it from all sides),
  `animsheet` (its attack frame by frame), `bestiary count=500` (generator variety), `boss`
  (spawn a designed or generated boss). Levels: `level depth=17` (what a depth is: name,
  palette, mechanics, boss; `to=` for a range), `level` (the live one: every feature with its
  position and state), `levelmap depth=7 seed=3` (top-down PNG of a freshly built level, no
  GPU), `go place=level depth=9` (travel there, waypoint unlocked), `goto_feature kind=keg`
  (stand next to one), `game_cmd do=use spot=N` (the way down, a cursed chest);
  `autoplay` walks levels to their exits by itself.
- Building and judging content: `see` is a screenshot with numbered marks on monsters, the
  hero, townsfolk, loot, spots and level pieces plus a legend (talk about "mark 7" and know
  it's a rare Frost Ghoul at 40%); `campaign from=1 to=12 wall=300` lets the bot play down
  through the levels and reports each (time, deaths, kills, levels gained): run it after data
  changes; `turntable family=ghoul def={"parts":[{"kind":"wings"}],"shirt":"#3050a0"}` authors
  a creature as JSON over any family/boss/genome (or `{"body":"quadruped",...}` for a new one)
  and reports its anatomy; `theme_swatch depths=13-20` shows palettes; `levelmap nav=true`
  shows what the AI thinks is walkable and its planned way out. Game data lives in `game/*.toml`
  (`game_reload` re-reads it live; the tree is generated from `game/tree.toml`; levels from
  `themes.toml` and `levels.toml`).
- Looks: `look` lists the Look & Filters sections, presets and whole looks; `look name=HD-2D`,
  `look section=pixel preset=Chunky on=objects` set the session's look layer (applied in every
  capture); `look bench=all` renders this moment under every look in one numbered PNG. Filters
  aim at `all`, `objects` (characters & objects) or `environment` through `view.*_on` /
  `view.filter.*_on` / `pixel_target`, and `view.style_objects` / `view.style_environment`.
- Teaching: `room key=bloom` includes the room's station guide (`learn`) and its pads' notes;
  `guide term=kuwahara` / `guide search=shadow` read the field guide
  (crates/pav_core/src/field_guide.toml), `guide asks=true` lists every room's "ask for it"
  phrases.
- `course` shows the running course timer, gates, hits, falls, last result and best times;
  `feel` shows feel metrics (response ticks, time to top speed, stopping, turnaround, jump).
- Animation: `animsheet move=roundhouse` draws any move frame by frame (`hit=`, `side=-1` for the
  alternate swing), `animsheet clip=CMU/Cartwheel` any motion clip (`mirror=`, `upper=`,
  `travel=`; on the hero by default, `def=` for another look). `clips` lists the sets, `clips
  find=dance` searches names, tags and descriptions, `clips name=SET/Clip` shows one as readable
  key poses with its source and licence, `clips load=cmu` adds the on-disk CMU library.
  `clip_import from=<set.js|set.json|folder>` translates a set, `from=a.glb,b.glb sources=A,B
  catalog=anim/catalogs/mesh2motion.json set=MESH2MOTION` a glTF library (Rigify or Unreal-style
  rigs), `from=take.bvh at=2-9 loop=true set=X` a BVH take, and `catalog=anim/catalogs/cmu.json
  set=CMU` (a catalog with "$pick") cuts its moments out of a capture database, downloading the
  takes; it checks the result reads back and reports the fit. `mocap` describes the open
  databases (CMU, 100STYLE, Mesh2Motion, Quaternius), `mocap find=limp` searches their takes,
  `mocap get=02_01,Zombie_FW` downloads (a 100STYLE take alone out of its 1.5 GB archive) and
  `mocap cut=13_17 at=1.5-2.6 name=Jab set=MINE` (or `loop=true`: the best cycle) fits one
  moment into anim/<set>.json. Room NPCs perform with `clips = [...]` / `moves = [...]`; any
  look's `idle_clip` plays while standing still and `walk_clip` (a walk loop that records its
  `speed`, e.g. STYLE100/Old_Walk) while walking, at the rate its stride matches the ground;
  `anim.tempo` / `anim.mirror` steer performers.
- `camera_bench` renders the current moment from several camera presets/tilts in one PNG.
- `signal name=drop` fires spawners listening for a pad signal (no need to walk onto the pad).
- `filmstrip` tiles N frames (optionally while driving the player) into one PNG: the cheapest
  way to check motion and animation. `audio_capture` renders a session's sounds to .wav.
- `stream point=[x,y,z]` adds a streaming interest point (agents exploring the world).
- `input` drives the player: `move=[x,z]` world direction (x = east, z = south), `hold=`/`press=`
  buttons (jump, crouch, crawl, use, focus, interact, primary), `aim=[x,y,z]`, `ticks=N`.
- `record_save` + `replay` reproduce a session exactly (same machine/build) and check the hash:
  use them for bug reports. The game saves `replays/last.replay.json` from the tuning panel.
- One-shot calls accept `scene=`, `seed=` and `ticks=` to set up the session first.
- `pav repl` keeps one session across lines (lines are `tool key=value ...`; `#` = comment).
- Look at captures with your image-reading tool; prefer small sizes (e.g. `width=640 height=360`).

### Adding a tool
Write `fn t_name(s: &mut Session, a: &Args) -> Result<Output>` in `crates/pav_tools/src/tools.rs`
and add a `Tool { .. }` entry to `TOOLS`. It is automatically in the CLI, REPL and MCP.

### Adding a room (preferred: data only, no rebuild)
Every room is a station of the world demo and teaches: give it a `[learn]` block (what you see,
how it works as the engine does it, where games use it, phrases to ask for it, cost, live
`knobs`, field-guide `terms`, 1-3 `[[learn.code]]` excerpts copied from the engine) and a `note`
on every pad that changes something (`pav_view/tests/learn.rs` checks words, settings, files and
notes; `rooms/bloom.toml` is the model).
Copy `rooms/_template.toml` to `rooms/<key>.toml` and edit (the template documents every
field: layers, legend blocks, ladders, props, zones, labels, objects with move/rotate/emitter/
spawner behaviours, hazards, soft bodies, materials, joints and chains; `rooms/feel_lab.toml`
and `crates/pav_core/tests/physics_room.toml` are full examples). It appears in the pavilion on
its wing's corridor (auto-placed and rotated so its entrance faces the corridor). Check it with
`pav room_check path=rooms/<key>.toml`, look at it with `pav capture scene=<key>` (the room
alone, fast) or `printf 'load scene=world\ngoto room=<key>\ncapture\n' | pav repl`. The running
game hot-reloads room files. New *mechanics* are Rust (character.rs / sim.rs behaviours /
entity fields); keep rooms as data wherever possible.

### Scenes
`world` (pavilion + rooms + streaming wilderness), `world/<room>` (start in a room),
`<room key>` (that room alone, no terrain), `test`, `empty`, and Shardfall's `town` (the game
starts here by default; the saved hero is loaded into any game scene), `arena`, `lab` (the
Menagerie of creatures) and `level/<n>` (depth n: 1-12 designed, then the endless Depths).
Code scenes live in `scenes.rs` (Shardfall places in `arpg/scene.rs`).

## Game controls (current)
Keyboard+mouse: WASD move, Space jump (hold = higher), C/Ctrl crouch, Z crawl toggle, F or left
click throw bomb at the cursor, Shift walk slowly, walk into ladders to climb, jump at a ledge
and keep pushing toward it to grab it (push again/Space = pull up, C = drop), Space/C swim up/down,
committed model: C while running = dodge roll; right-drag rotate camera, wheel zoom, 1–8 camera
presets. Gamepad: left stick move, right stick aim, A jump,
B crouch, Y crawl, X/RT bomb, LT slow, LB/RB rotate camera, D-pad zoom, Start menu, Back rewind.

Shardfall: WASD move, LMB/RMB/Q/E/R/F skills, Space dodge, 1 potion, Shift attack in place,
G use (vendor, stash, portal, gambler, alchemist, the way down, cursed chests), I/Tab
inventory, P passive tree, C character, K skills, M map, T town portal; gamepad
X/Y/B/RB/LB/RT skills, A dodge, D-pad up potion, D-pad right use, D-pad left map, D-pad down
panels (LB/RB switch); in any window or the menu the stick drives a cursor (A click, X
right-click, hold Y shift, right stick scroll, B close). Controller flows can be scripted and
screenshotted: `shardfall --pad-script FILE` (lines `<frames> [left=x,y] [right=x,y]
[hold=A,Y] [tap=DPadDown]`, see `PadScript` in input.rs).

H (world demo): how the room you are in works (the station guide: steps, pads, live settings,
uses, ask-for-it phrases, cost, the engine's code, words); outside rooms the field guide
(guide_ui.rs). Stepping on a pad with a note shows the note at the bottom for a while.
Fixed system layer (never rebinds): Esc pause menu · F1 tuning panel · F2 rooms (teleport) ·
F3 boot diagnostics · F4 leave room · F5 reset room · F6 pause · F7 step · F8/F9 slower/faster ·
F10 edit mode (sandbox) · hold Backspace rewind · F11 fullscreen · F12 screenshot.
The pause menu's *Look & filters* window (also atop the F1 panel) puts every filter on the whole
scene, the characters & objects or the environment, with sliders, presets and whole looks; it
is a layer over the scene's own settings, saved to `shardfall_looks.json` (look_ui.rs).
Rooms may remap game keys while you are inside (`[keys]` in the room file); system keys never change.

## Tests
`cargo test` includes `crates/pav_core/tests/gameplay.rs` (walk, jump onto a crate, climb the
ladder, bomb the floor and drop through, crawl the tunnel, rewind repeatability, snapshot files),
`tests/world.rs` (pavilion layout, streaming, props persisting while dormant, room
enter/exit/reset, overrides, saved-object round trip), `tests/movement.rs` (courses, pads,
ledge grab, swimming, pits, grid/committed models, projectiles, moving platforms, hazards),
`tests/physics.rs` (rope bridge, chain, soft bodies, conveyor, bounce, crumble/regrow, glass,
spawner pads, rewind with soft bodies; its room is `tests/physics_room.toml`, a compact example
of every physics feature) and `tests/rooms.rs` (every room file builds; courses complete).
`tests/motion.rs` (clips decode like the format's reference player, every embedded clip plays
cleanly, a set file round-trips, fades, chained moves, kicks, performers, idle clips, the hero's
captured death, walkers and villagers keeping their styles' pace); `crates/pav_tools/tests/mocap.rs`
(a glTF rig and a CMU take translate number for number like my-3D2dge's importer, whose output is
in tests/fixtures/mocap; a BVH walk cuts into a loop that closes).
Shardfall: `tests/arpg.rs` (combat) and `tests/loot.rs` (drops, equipping, town trade, travel,
rewind across travel, unique powers), `tests/skills.rs` (all 16 skills, tweaks, channels, the
passive tree through commands, keystones, the bot spending points), `tests/monsters.rs`
(archetypes, broods, bombers, affixes, bosses through their phases, the Menagerie),
`tests/levels.rs` (every designed level has its mechanics, each mechanic works, endless
depths, the sealed exit, waypoints, rewind in a level), `tests/town.rs` (townsfolk, gambling,
brewing, saves), `tests/feel.rs` (gibs, kill streaks, boss entrances).
View: `pav_view/tests/pixel_art.rs`, `parts.rs` (characters & objects vs environment, filters per
part, styles per part, styles/haze reach the renderer, transition loop), `look.rs` (look
sections, presets, whole looks), `water.rs` (ripples spread and fade, rain, pool strips merge)
and `learn.rs` (station guides and the field guide stay consistent with the engine).
Extend it when you add movement features; it is the cheapest way to catch feel regressions.

## Disk space
Cloud containers have a fixed disk allowance. Dev builds use line-table debug info and no
incremental cache; if the disk fills up anyway, delete stale binaries in `target/debug/deps`
(or run `cargo clean`).

## Conventions
- Rust stable pinned in `rust-toolchain.toml`; `Cargo.lock` committed; edition 2024.
- Keep `docs/PROGRESS.md` current enough to resume from after a context reset.
- Decide anything that isn't genuinely the user's call; record the decision in `docs/PROGRESS.md`.
- Finishing a task or milestone: commit, push, open a PR into `main`, merge it and delete the
  branch. This is standard procedure (the user's standing instruction); don't ask first.
- The user plays the Windows build; the log file `shardfall.log` and `shardfall.toml` (startup
  settings, e.g. `backend = "dx12"`) sit next to the `.exe`.
