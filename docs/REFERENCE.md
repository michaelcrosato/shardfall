# Reference

The detail behind `AGENTS.md`, read on demand: open the section you need, not the whole page.
`pav help` lists every tool with its arguments.
- [Code map](#code-map): what each crate and data folder holds.
- [Studios](#studios): animation, objects and creatures, edited live.
- [Tools](#tools): each tool family with examples.
- [Browser and phones](#browser-and-phones): running the web build headless, emulating a phone.
- [Controls](#controls): keyboard, gamepad, touch and the fixed system keys.
- [Tests](#tests): what each suite covers.

## Code map
```
crates/
  pav_core    simulation, no window/GPU/audio deps: Sim, entities, physics (rapier3d),
              static regions (statics.rs: terrain chunks, rooms, hub; active or dormant;
              blocks, decor and baked library props: drawn by the view's region cache, one
              collider box each, no entity),
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
              follow-through; skills name a move, a look may name its own per skill and combo
              swing: `attack_moves`), clips.rs (motion clips: readable key poses
              translated from open animation libraries, retargeted onto the biped with IK,
              crossfaded over the procedural animation, upper body only or mirrored; performers,
              idle clips, walk clips played as fast as the character moves), anim.rs (the /anim files: embedded, live reload), rig.rs (creature feet that plant and step,
              follow-the-leader spines, verlet tails/antennae), ai.rs (NPC brains: idle,
              wander, patrol, circle, follow; they drive characters through InputFrames),
              nav.rs (walkable grid from blocks and props, A* paths, flow fields),
              fxdef.rs (object lights / particle emitters / distortion, view-only),
              vehicle.rs (drift car on rapier's raycast vehicle, arcade helicopter: flight only,
              no weapons, damage or fuel), health.rs (shootable rapier objects: hp, score, boss
              phases/bar, finish),
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
              the world (text.rs). Procedural materials: rows of a storage buffer read by one
              kernel library (shaders/materials.wgsl, matched pixel for pixel by
              `pav_core::material::eval`); `pav_view::materials` interns them into slots for
              prop parts, static blocks (world-mapped) and decor. `render_scale` draws below the target's resolution and
              upscale.rs stretches it over (phones). Offscreen capture -> PNG.
  pav_view    sim frame -> render Scene: camera rig (tilt/yaw/distance/fov/ortho, all live),
              look.rs + looks.toml (the Look & Filters layer: filter sections, presets, whole looks),
              water.rs (water zones ripple: a CPU wave equation on a height grid, stirred by
              whatever crosses the waterline, splashes, blasts and rain; drawn as a dynamic mesh),
              assets.rs (`emit_asset`: a library asset drawn from game state each frame),
              interpolation between ticks, visual settings (ViewSettings)
  pav_audio   synthesized sound: oscillators/noise/envelopes/filters, event -> sound bank,
              cpal output (optional), offline .wav rendering
  pav_tools   agent layer: tool registry + `pav` CLI (one-shot, REPL) + MCP stdio server +
              live bridge client (bridge.rs), and the motion importer (mocap/: glTF and FBX
              libraries, CMU's ASF/AMC, BVH; Rigify, Unreal, Mixamo/MotionBuilder, 100STYLE and
              Bandai Namco skeletons; loops cut at their best cycle; fitted into readable key
              poses, number for number what my-3D2dge's own importer writes; the whole CMU
              database surveyed and translated), and the asset importer (import/: what both
              importers share, one `Scene` read from glTF/OBJ, normalisation, translation into
              props with fidelity metrics, the licence gate that routes packs to lib/ or
              local/; `asset_import`, `docs/ASSET_IMPORT.md`)
  pav_app     the game (`shardfall` binary): window, boot diagnostics, input (keyboard/mouse,
              gamepad via gilrs), system keys, tuning panel, pause menu, sim thread
              (simhost.rs; stepped from the frame loop in the browser), live bridge server
              (bridge.rs), egui input (uiinput.rs), touch screens (touch.rs: finger routing,
              invisible stick, button fan, auto-attack, swipe-to-dismiss), graphics quality
              (quality.rs: render scale, dynamic resolution). Builds natively and for wasm32
              (WebGPU).
anim/         animation data, embedded and hot-reloadable (`anim_reload`): moves.toml (the moves table)
              and clip sets (*.json: QUATERNIUS, MESH2MOTION, CMU, STYLE100: 437 clips with credits).
              anim/cmu/ holds every take of the CMU database (4,770 clips) and anim/100style/ the
              100STYLE runs, backward walks and runs, sidesteps and idles (780 loops), on disk
              only: `clips load=cmu`, `clips load=100style`. anim/local/ (git-ignored) holds what
              is translated from libraries whose licences forbid sharing it (Bandai Namco, LaFAN1,
              Mixamo): `clips load=local`. anim/catalogs/ says how each set is made (tags,
              descriptions, sources, picks): `clip_import catalog=...` rebuilds it from the
              libraries.
rooms/        room data files (TOML: info card, station guide `[learn]`, wing, primary device, movement model, camera,
              params, keys, entrance, ASCII tile layers + legend, [[object]]s). Every file
              here is embedded in the exe at build time AND hot-reloaded at runtime.
              `_template.toml` documents every field.
```

## Studios
Animation Studio: `scripts/animation-studio.sh` or `shardfall --animation-studio` opens an
isolated preview and the live bridge. Use `.mcp.animation-studio.json` for visible MCP edits.
`anim_edit` creates/copies/inspects/edits clips, with revisions and 32-step undo/redo;
`anim_preview` plays, pauses, scrubs, and inspects their poses. Every accepted edit saves and
updates the live library. The preview preserves the game state. Native animation files also
reload automatically. See `docs/ANIMATION_STUDIO.md`. REPLs accept structured JSON lines:
`{"tool":"anim_edit","args":{"action":"key","name":"WORKSHOP/Wave","time":0.5,"pose":{"armR":[80,20,50,25,0]}}}`.
Use the returned `revision` as `if_revision` on later edits. Copy source clips before editing;
keep local source copies and their history in `anim/local`.

Object Studio: `scripts/asset-studio.sh` or `shardfall --asset-studio` opens reusable prop
authoring on the same live bridge (`.mcp.studio.json`). `assets find=words` searches templates
(`BUILTIN/`), library packs (`LIB/pack/name`; `LIB/core` is embedded in every build, other packs
load from disk natively, `assets/props/local/` is git-ignored) and saved definitions, ranked,
with `category= tags= tier= similar=` filters. `asset_edit` creates/copies/inspects/patches
readable JSON with stable part names, atomic batches, revisions, and 32-step history.
`asset_preview` opens the isolated stage; `views=true` returns a framed 4-view contact sheet
(`figure=true` adds a 1.8 m figure) and leaves the stage and camera as they were. `capture stats=only` returns render cost without
an image. `asset_spawn` places one-root instances in the preserved scene. Edits refresh visuals
and compound collision together. Native file changes reload after 250 ms of quiet. Saved
definitions are `assets/props/workshop/*.json` (`PAV_ASSETS` overrides the props root).
`studio_status ticket=N` confirms frame submission for live edits without a capture. The
Animations / Objects / Game tabs retain each workspace's controls and camera. See
`docs/ASSET_STUDIO.md` (tools and format) and `docs/ASSET_STUDIO_LIVE.md` (the live studio).
Keep procedural parts as text; use `if_revision` and batch related part changes in one `ops`
array. Materials (planks, brick, cobbles … as per-pixel recipes, `docs/MATERIALS.md`): parts say
`"mat":"MAT/oak_planks"` or a key in the asset's `materials`; `kind=material` on `assets`,
`asset_edit` (workshop materials `MAT/workshop/x` in `assets/materials/workshop/`, `PAV_MATERIALS`
overrides the root; revisions `mat1-`) and `asset_preview` (a swatch with each layer's legibility
at the game camera). Game code draws units and scenery from its own state with library assets,
one call per thing per frame: `pav_view::assets::emit_asset(scene, "BUILTIN/crate",
Placement::at(pos).yaw(deg).id(id), seed, &palette)` (no physics needed; `docs/ASSET_STUDIO.md`,
"Library assets in game code"). Game data and rooms may name only embedded assets.

Creature Studio: `scripts/creature-studio.sh` or `shardfall --creature-studio` opens the
procedural creature stage. `tools/creature-compiler` vendors the exact SpawnForge core/modules
pin and bundles it with `npm ci --ignore-scripts && npm run build` (run in that directory).
Keep the pin and source hashes intact; do not resolve against a moving upstream checkout.
`creature_catalog`, `creature_edit`, `creature_preview`, and `creature_status` expose theme,
template, blueprint, named-part and surface authoring through the same MCP bridge. Edits
queue asynchronous builds. Wait for `published`, `failed`, or `superseded`; only publication
gets a frame ticket. Use the new source `revision` for the next edit, and `studio_status`
for actual frame submission. Failed/obsolete builds keep the last valid native mesh.
Sources, history and disposable caches are under `assets/creatures` (`PAV_CREATURES`
overrides it). Node runs outside the simulation. The Windows package includes the bundle
and portable Node. See `docs/CREATURE_STUDIO.md` for native material limits and the tool
contract. This is an authoring/preview stage; gameplay spawning of these rigs is separate.

## Tools
- Import: `asset_import` alone lists the open model libraries; `get=kenney/mini_dungeon` downloads
  a pack its catalog pins (`assets/catalogs/<lib>.json`) into .cache/assets, checked by SHA-256;
  `from=<model> list=true` summarises a .glb/.gltf/.obj (parts, triangles, size in cm, materials,
  main colours, warnings); `from=<model|folder|lib/pack>` translates into format-2 props
  (primitives, plus one read-only vertex-coloured mesh part for what they cannot fit; `fit=prims|mesh`)
  with the catalog's category and tags, routed by the licence gate: CC0/CC-BY packs to
  `assets/props/lib/<pack>/` (committed, with `PACK.json`; then the index and `CREDITS.md`),
  other licences, packs whose catalog says `"local"` and loose files to the git-ignored
  `assets/props/local/<pack>/` (or `out=`), with their fidelity (six-view IoU, ΔE, Chamfer mm,
  tier; `docs/ASSET_IMPORT.md`); `survey=<lib/pack|folder>` fits without writing
  and fills the ledger (`assets/catalogs/<lib>/<pack>.tsv`, keeping each row's `note`; `fit=false`:
  census only); `check=<asset.json> compare=true` measures a translation again and returns
  source-over-translation views. `up= yaw= scale= place=` bring a model into the engine's frame
  (a translation records them in `source.norm` for `check`).
- Shardfall (scenes `town`, `arena`): `game` is the status, `autoplay seconds=30` lets a bot
  fight, `loot_roll level=40 count=5000` summarises loot tables without playing, `give
  unique=skyfall equip=true` hands over gear, `inventory` describes everything worn/carried,
  `game_cmd do=sell id=12` does any menu action exactly as the player would (it rides in the
  input frame, so replays include it). `tree find=fire`, `tree take=Unbowed` (allocates the
  path), `tree_map` (PNG of the generated passive tree), `genome seed=7 parts=wings,horns`
  (grow a creature), `turntable seed=7` / `family=` / `boss=` (render it from all sides),
  `animsheet` (its attack frame by frame), `bestiary count=500` (generator variety), `boss`
  (spawn a designed or generated boss). Playable characters (`game/heroes.toml`: the Wanderer,
  Kestrel, Ysolde; the Hall of Heroes in town): `hero character=kestrel` plays as one (each keeps
  their own hero; the stash is shared), `game_cmd do=character name=ysolde` as the player would
  (in town), `turntable character=` / `animsheet character= skill=slash swing=3` show one. Levels: `level depth=17` (what a depth is: name,
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
  `themes.toml` and `levels.toml`, dressed with LIB/core props from the sets in `props.toml`).
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
  catalog=anim/catalogs/mesh2motion.json set=MESH2MOTION` a glTF library (Rigify, Unreal-style or
  Mixamo rigs; `rm=` each file's root-motion twin, as Quaternius ships them, gives loops their
  speed), `from=lib.fbx` an FBX file (every animation stack a clip), `from=take.bvh at=2-9
  loop=true set=X` a BVH take (`way=back|left|right` for a loop that goes that way), and
  `catalog=anim/catalogs/cmu.json set=CMU` (a catalog with "$pick": cmu, 100style, bandai or
  lafan1) cuts its moments out of a capture database, downloading the takes; it checks the result
  reads back and reports the fit (`log=N` notes). `mocap` describes the open databases (CMU,
  100STYLE, Bandai Namco, LaFAN1, Mesh2Motion, Quaternius, Mixamo), `mocap find=limp` searches
  their takes, `mocap get=02_01,Zombie_FW` downloads (a 100STYLE or LaFAN1 take alone out of its
  archive; `get=cmu`: all 2,548 CMU takes), `mocap survey=ledger` / `survey=library` measures
  every CMU take into anim/cmu/takes.tsv / translates them into anim/cmu, and `mocap cut=13_17
  at=1.5-2.6 name=Jab set=MINE` (or `loop=true`: the best cycle; `way=`) fits one moment into
  anim/<set>.json. Room NPCs perform with `clips = [...]` / `moves = [...]`; any
  look's `idle_clip` plays while standing still and `walk_clip` (a walk loop that records its
  `speed`, e.g. STYLE100/Old_Walk) while walking, at the rate its stride matches the ground,
  `run_clip` above the pace between the two; `death_clip` is a captured fall and `attack_clips
  = { claw = "QUATERNIUS/Zombie_Scratch" }` captured attacks by skill, timed so the clip's
  strike (shown by `clips name=`; `@seconds` after the name overrides it) lands on the hit, or
  a list of them, one per swing of the skill's combo; `attack_moves = { cleave = "sweep" }`
  swings a move of anim/moves.toml in place of the skill's own (a list per swing too);
  `dodge_clip` is a captured dodge in place of the tumble (bipeds: game/monsters.toml,
  bosses.toml and heroes.toml looks, and the genome's biped `gaits`, `idle_clips`,
  `death_clips`, `attack_clips`; `animsheet family=ghoul skill=claw` shows one);
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

## Browser and phones
Headless Chromium can run the browser build (WebGPU on SwiftShader): launch with
`--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader --use-angle=swiftshader`
(other flag sets show a blank canvas). `?room=NAME&seed=N` picks the start room.
Phones: emulate one in headless Chromium with Playwright (`viewport 844x390, isMobile, hasTouch,
deviceScaleFactor: 1`; Chrome's DPR emulation leaves the canvas at CSS size) and drive it with
CDP `Input.dispatchTouchEvent`; SwiftShader frames take about a second, so send a tap's start and
end back to back.
From WSL2 you can launch the Windows build directly: `./target/x86_64-pc-windows-gnu/dist/shardfall.exe`.

## Controls
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

Touch (phones, tablets; the browser build): put a thumb down anywhere and drag to move (an
invisible stick); attacks are automatic (the nearest foe in reach; a menu setting); the fan in
the bottom right: dodge (big button), skills (tap: at the nearest foe · drag: aim, let go to
cast, back onto the button to cancel · hold: keep casting), potion, and a "use" button that
appears when something is in reach; top right: menu and bag. Swipe messages, banners and the
minimap away; tap the minimap for the big map; tap the game outside a window to close it.
Graphics quality (Auto/Low/Medium/High) and auto-attack are in the touch pause menu.

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
captured death, walkers and villagers keeping their styles' pace); `tests/monsters.rs` also has
a ghoul shambling, running, clawing and falling in captured motion and generated bipeds drawing
theirs from the genome; `crates/pav_tools/tests/mocap.rs`
(a glTF rig and a CMU take translate number for number like my-3D2dge's importer, whose output is
in tests/fixtures/mocap; the CMU survey measures a take as the committed ledger says; a BVH walk
cuts into a loop that closes; a sidestep take gives a loop each way, facing forward; an FBX file
written by the test reads as a take); `tests/import.rs` (glTF and OBJ models built in code read
back exactly, mirrored nodes stay outward, texels give exact face colours, malformed files are
errors, normalisation, the `asset_import` tool), `tests/import_fit.rs` (a table folds into a top
and mirrored legs, a vase is a lathe, a blade an extrusion, a noisy blob a mesh part within budget;
the engine reads every translation back; byte-identical twice; translate, check, compare and
survey through the tool), `tests/import_library.rs` (packs go to lib/ or local/ by licence
with their `PACK.json`, credits, index and catalog tags; unknown licences refused),
`tests/planted_defects.rs` (the store's five planted defects, each caught: size_band,
below_floor, socket_missing, a missing texture failing the import), `tests/store_rooms.rs` (the
Library Hall is current, the store rooms pass room_check, every id a sign names resolves, each
Limits sign is true, the Catalogue pages through every row with rewind and snapshots, Kenney
Mini Dungeon through every step) and
`tests/import_khronos.rs` (opt-in,
`-- --ignored`: 12 Khronos sample models at a pinned commit). `pav_core/tests/prop_materials.rs`
(props wear library and inline materials, format 1's colours and revisions hold, unknown names,
one edit that adds and uses a material, snapshots) and `pav_tools/tests/material_preview.rs`
(a swatch renders headless with its layer lines).
Shardfall: `tests/arpg.rs` (combat) and `tests/loot.rs` (drops, equipping, town trade, travel,
rewind across travel, unique powers), `tests/skills.rs` (all 16 skills, tweaks, channels, the
passive tree through commands, keystones, the bot spending points), `tests/monsters.rs`
(archetypes, broods, bombers, affixes, bosses through their phases, the Menagerie),
`tests/levels.rs` (every designed level has its mechanics, each mechanic works, endless
depths, the sealed exit, waypoints, rewind in a level; dressing never moves a mechanic or a
monster, is reproducible, keeps its snapshot budget and the paths; baked props collide, sleep
and restore), `tests/town.rs` (townsfolk, gambling,
brewing, saves), `tests/feel.rs` (gibs, kill streaks, boss entrances), `tests/heroes.rs`
(playable characters: the hall, switching with a shared stash, strikes per combo swing,
captured dodges).
View: `pav_view/tests/pixel_art.rs`, `parts.rs` (characters & objects vs environment, filters per
part, styles per part, styles/haze reach the renderer, transition loop), `look.rs` (look
sections, presets, whole looks), `water.rs` (ripples spread and fade, rain, pool strips merge)
and `learn.rs` (station guides and the field guide stay consistent with the engine);
`material_parity.rs` renders every material kernel on the GPU (lavapipe will do) and compares
it with the CPU reference.
Extend it when you add movement features; it is the cheapest way to catch feel regressions.
Also, named for what they cover: `pav_core/tests/` `animation.rs`, `animation_preview.rs`,
`asset_library.rs`, `bench_holdout.rs`, `fx.rs`, `genre.rs`, `prop_families.rs`,
`prop_format2.rs`, `prop_instances.rs`, `prop_patch.rs`, `vehicles.rs`; `pav_tools/tests/`
`animation_preview.rs`, `asset_errors.rs`, `asset_format2.rs`, `asset_library.rs`,
`asset_preview.rs`, `foundations.rs`, `reply_budget.rs`, `store.rs`.
