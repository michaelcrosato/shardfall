# Progress log

All engine milestones M1–M10 and all Shardfall milestones G1–G6 are complete (Shardfall is
the showcase hack-and-slash built on the engine; design: `docs/GAME.md`; progress: the
*Shardfall* sections at the end of this file). Next work, if any, is new content or polish:
add data (themes, levels, families, affixes, uniques, tree clusters) and check it with the
tools (`levelmap`, `see`, `campaign`, `turntable def=`).

## Mobile: touch controls, a pared-down HUD, swipes and performance — 2026-10-09
The browser build had no touch input at all (winit turns a finger into `WindowEvent::Touch`,
which nothing read, so on a phone neither the game nor its windows responded) and drew the desktop
HUD at full device resolution. Now phones and tablets are a first-class device (`Device::Touch`,
detected from `(pointer: coarse)` at start or from the first finger):
- **Controls** (`pav_app/src/touch.rs`): every finger is routed when it lands: the corner
  buttons first, then windows (egui gets it as its pointer, the first finger only, plus every
  finger as a touch for pinches), then the on-screen buttons, then swipeable things, then labels;
  anything else is the move stick. The stick is invisible until a thumb lands anywhere on the
  open game; a faint ring then shows where, and it follows a thumb that runs past its edge.
  The right thumb has a fan round the corner: dodge (the big button), skills on two arcs (tap:
  at the nearest foe it can reach, or the nearest at all for missiles; drag: aim by hand with a
  ring and dots on the ground, let go to cast, drag back onto the button to cancel; hold: keep
  casting; channels last while held), the potion with its charges, and a "use" pill named for
  what is in reach (Trade, Travel, Descend, Open...). Top right: the menu, the bag (a badge
  counts unspent passive points) and, when the minimap was swiped away, a map button.
- **Auto-attack** (`touch::pick_target`, a setting, on by default): the first skill strikes the
  nearest foe in its reach (`skills::reach`, now shared with the bot; `SlotHud` carries reach,
  range, channel and ground-target) unless the player is running away from it; with the stick
  still, the hero steps in to a foe that is fighting it a little out of reach. Its button is
  hidden; with auto-attack off it takes the corner and dodge moves to the bottom row. All of it
  rides in the input frame, so replays and the simulation are untouched.
- **Less on screen** (`arpg_ui::hud` on touch): slim life, mana and experience bars with a level
  badge in the top left (no orbs, no skill bar, no key letters), the level's name under them, a
  smaller minimap under the corner buttons, banners placed for a short screen. No boot panel, FPS
  overlay, key hints or keyboard guide; a short "Playing by touch" card instead, until swiped.
- **Swipe away** (`touch::Swipes`): toasts, the arrival banner, a level's intro (now with its
  mechanics), the station guide's pad notes, the tips card and the minimap follow the finger and
  fly off when swiped; they stay away until their content changes (the minimap until the map
  button). A tap on the minimap opens the big map; a tap or swipe closes it. A tap on the open
  game closes what is open over it (menu, windows, map, guides), one at a time.
- **Windows for fingers**: bigger hit targets and text (`ui::touch_style`, only while on touch),
  windows that fit the screen below the corner buttons and scroll by dragging; in the bag, shop
  and stash a tap shows the item's card (what it is, what it changes, and buttons: Wear, Wear on
  the right hand, Sell, Stash, Drop, Take off, Buy, Buy back, Take) instead of acting blind; the
  passive tree shows a tapped node with Take / Take the path / Refund / mastery buttons (a second
  tap takes it) and zooms with two fingers. A pause menu made for phones: Resume, Town portal,
  Auto-attack, Graphics (Auto/Low/Medium/High), Show FPS, Full screen, the rest under "More".
- **Performance**: `Renderer::render_scale` draws the scene below the window's resolution and a
  bilinear pass (`pav_render/src/upscale.rs`) stretches it over the window, so the UI stays sharp;
  `max_shadow_lights` caps point-light shadow casters (six depth passes each). `quality.rs`:
  Auto is High on a desktop (unchanged: every pixel, every effect) and Medium on a touch screen
  (about a megapixel, one shadowed lamp, no screen-space GI or sun shafts) with dynamic
  resolution (slower than 50 fps drops a step, a few smooth seconds raise one); Low halves the
  pixels and drops haze, halos, bloom and distortion. Touch screens draw at most 60 frames a
  second. Saved with auto-attack and "tips seen" in `shardfall_prefs.json` / local storage.
- **The page**: no browser zoom, scrolling, callouts or tap flashes over the game; `100dvh` so
  the canvas follows a phone's browser bars; the first tap goes full screen and turns sideways
  where the browser allows it (Android); a web manifest (full screen, landscape) for "Add to
  Home Screen"; a hint to turn the phone sideways that fades. Held upright, the camera's field
  of view becomes the width's, so the sides are not a sliver.
- Checked in headless Chromium with phone emulation (844x390, touch, `pointer: coarse`) driven
  by CDP touch events: the stick walks, the bag opens, a tapped item shows its card, a tap
  outside closes it, the menu, tips, intro banner and minimap swipe away, the map button brings
  the minimap back, auto-attack fights an arena wave, a hand-aimed skill fires where aimed.
  Unit tests: routing, auto-attack targets, swipes, a quick tap clicking in egui, quality scale.
- Not done: no real phone was available (SwiftShader only); Chrome's DPR emulation sizes the
  canvas at CSS pixels (a real phone gives CSS x DPR), so layouts were checked at DPR 1. No
  camera rotation or pinch-zoom of the world on touch (the camera is fixed in the game anyway).

## Animation: every open library through the engine's own tools — 2026-10-09
The follow-up that brings the rest of the libraries in, and the formats they ship in:
- **The whole CMU database, natively** (`mocap/cmu.rs`, a port of cmu.mjs's survey): `mocap
  get=cmu` downloads the site's 1.08 GB archive and unpacks every take (2,514 from it, 32 more one
  by one: 2,548); `mocap survey=ledger` measures each into anim/cmu/takes.tsv (length, where it
  moves, fit, travel, hip heights, flags) and `mocap survey=library` translates them, a set a
  subject, into anim/cmu (10.7 hours of motion, 4,770 clips, about 80 s each). Run over the
  whole database, all 2,548 ledger rows and all 113 sets came out identical to the committed ones
  my-3D2dge's tool made; the files differ only in the legend's line for `speed`, so anim/cmu is
  now the native tool's output.
- **100STYLE's other gaits** (`anim/100style`, on disk, `clips load=100style`): runs, backward
  walks and runs, sidesteps and sideways runs each way, and idles for every style: 780 loops, 9,779
  key poses, 2.8 MB, catalogs `anim/catalogs/100style_*.json` written from the dataset's lists.
  The cutter learned what they need. A loop can go a way (`way=back|left|right|forward`): every
  straight pass of the take going that way of where the hips face, never doubling back, is
  searched, and the loop is cut from the pass that holds the cleanest cycle, the body facing
  forward. A turn inside the cycle costs too, so it is not taken just after the performer comes
  round. Sidesteps swing the legs out, backward walks fore and aft, every body faces forward
  (median turn 0); the four styles that drag the leg they would lead with are left out, with the
  reason in the catalog. The forward walks, CMU and Mesh2Motion sets come out byte for byte as
  before; CMU's Walk_Backwards_Loop now steps back facing forward (`way` in its catalog line).
- **Quaternius' Universal Animation Libraries 1 and 2** fetched from itch.io and translated
  natively from version 2 (June 2026): 102 clips. `rm=` reads each file's root-motion twin: loops
  take their speed from it, clips that travel are added as `<clip>_RM`.
- **FBX** (`mocap/fbx.rs`): binary FBX 7 (Mixamo, Blender, Maya, Unreal exports): the node tree
  (zlib arrays), the skeleton with the SDK's transform (pre- and post-rotation, pivots, offsets,
  six rotation orders, scale), each animation stack's curves sampled into a take, z-up files
  turned y-up. `clip_import from=x.fbx` makes every stack a clip. Quaternius' own FBX of Library 1
  agrees with the glTF translation of the same clips (median 1.4 degrees off; the rest is where
  each route roots and faces a clip). `tests/mocap.rs` writes an FBX file and reads it back.
- **Rigs:** Mixamo's (MotionBuilder names, `mixamorig:` or not) for glTF too; Bandai Namco's;
  Unreal and Rigify names for BVH and FBX. Exporters that key every bone's translation, and
  skeletons whose zero pose is not a body standing (MotionBuilder lays bones along x), are read:
  position channels place a joint outright, and the body is measured in the frame where it
  stands straightest, every take of a library sharing the take that stands best. A bone end the
  file gives no length gets a typical one. A glTF file with more than one skinned mesh measures
  its tips on all of them.
- **Non-commercial libraries, translated for this machine** (git-ignored `anim/local`, `clips
  load=local`; never committed): the Bandai Namco Research motion dataset 1 (`$library: bandai`,
  175 takes in 15 styles: walks, runs, dashes and backward walks looped, gestures, fights and
  dances whole; 13.6 mm), LaFAN1 (`$library: lafan1`, a loop out of each of its 18 walks, runs and
  sprints, read one take at a time out of its 144 MB archive), and Mixamo downloads (.fbx or
  .glb).
- **Tools:** `mocap` describes seven libraries; `mocap get=` fetches CMU, 100STYLE, Bandai Namco
  and LaFAN1 takes; `mocap cut ... way=`; `clip_import` reads .fbx, `log=N` for all its notes,
  `way=` and `whole` (a take that is one cycle already) in catalog picks; a catalog import leaves
  a pick it cannot cut out, with the reason, rather than failing.
- **Monsters move in captured motion** (humanoid ones: the clips are human). A look's `run_clip`
  takes over from its `walk_clip` above the pace between them, so a monster shambles while it
  wanders (at its walk's own pace) and runs when it gives chase, its stride matched to the
  ground either way. `attack_clips` names a captured attack per skill, played over the skill's
  move and timed so the clip's strike (the moment a hand, or the blade's tip, reaches furthest
  ahead; `clips name=` shows it) lands on the hit, at near its own speed. `death_clip` is a
  captured fall: down within about 1.2 s, the body sinking and clearing after it. The ghoul
  sways hunched, shambles, runs, rakes with both hands and is thrown off its feet; the
  bonecrusher struts, brings its maul down overhead or sweeps it low; the Hollow King marches,
  slashes and splits the earth with captured swordplay. Generated bipeds draw a gait (a walk
  and its run), an idle, a fall and their skills' attacks from the genome (`game/genome.toml`),
  out of a stream of their own so every other gene stays as it was. Game data names only clips
  that exist (checked on load); `animsheet skill=` shows the captured attack.

## Animation: the engine translates open motion libraries itself — 2026-10-09
my-3D2dge's raw-capture importer (tools/anim-import.mjs, asf-amc.mjs, cmu.mjs and the encoding
half of readable.js), ported into `pav_tools::mocap`, so the libraries come in through this
engine's own tools instead of through the other engine's conversions:
- **Readers.** glTF binary (`glb.rs`: accessors, node hierarchy, skinned mesh for the tip bones,
  linear/step/cubic-spline tracks; Rigify `DEF-` and Unreal-style rigs), CMU's Acclaim files
  (`acclaim.rs`: the skeleton's axes and degrees of freedom, M = M_parent · C · R · C⁻¹), BVH
  (`bvh.rs`: hierarchy, channels, forward kinematics; bone maps for 100STYLE and the
  MotionBuilder names Mixamo and LaFAN1 use (that one tested on a synthetic skeleton only), as
  tables with fallbacks; units found from the legs).
- **Cutting takes** (`takes.rs`): a stretch of seconds; a loop at its best cycle (pose and speed
  match, a span that keeps moving), played in place facing the way it walked and closed exactly;
  the rest pose stood on the floor the takes stand on. New: a loop with no stretch given finds the
  take's longest straight run (every hip position within 12 cm of a line) or, for an idle, its
  longest still stretch, inside the frames a dataset marks as the style.
- **Encoding** (`readable.rs`): the body's rest measurements and spine shares, key poses from
  captured points, decoding on the captured body, Ramer–Douglas–Peucker keys and the fit that
  adds the frame the in-betweening misses most. Ported step for step: f64 in the reference's order,
  captured points stored as f32 like its Float32Arrays, fdlibm's trigonometry (`libm`, as V8),
  V8's `Math.hypot`, JavaScript's rounding and `toFixed`.
- **Verified number for number.** The three Mesh2Motion GLBs → MESH2MOTION: 177 clips, all
  119,432 numbers identical to my-3D2dge's own run, the rest bodies too (2.3 s against its 5.4 s).
  The CMU catalog → CMU: 60 clips from 25 subjects, the takes downloaded by the tool, all 104,101
  numbers identical. `tests/mocap.rs` keeps it so: a small Unreal-style rig and a CMU take
  (fixtures, with my-3D2dge's output for them).
- **Downloads** (`fetch.rs`, through curl, so cloud proxies just work): CMU takes from
  mocap.cs.cmu.edu, Mesh2Motion's GLBs from GitHub, and single files out of a zip archive on the
  web by ranged requests (100STYLE's 1.5 GB archive: one take costs its own bytes). Into
  .cache/mocap, git-ignored.
- **A new library: 100STYLE** (one performer, 100 styles of walking, CC BY 4.0). Its forward walks
  cut and fitted into STYLE100: 98 styles (the two spins left out: they cannot loop in place),
  1,550 key poses, 12 mm off on average, 440 KB, the dataset's own descriptions. The catalog
  (`anim/catalogs/100style.json`) was written from its Dataset_List.csv; picks play inside its
  Frame_Cuts.csv. A walk's shortest cycle is 0.8 s (a whole stride), so a loop never leads with
  the same foot twice; hops and two-footed jumps repeat after one.
- **Loops know their speed.** A clip's `speed` (percent of standing hip height a second, how far
  the cycle carried the hips) is new in the format (the legend says so; other readers ignore it).
  `walk_clip` on a look plays such a loop while the character heads forward on the ground, at its
  ground speed over the clip's own, so the stride matches the ground (`clips::walk_rate`, `pace`).
  The CMU set was rebuilt by the native importer to add speeds to its loops: nothing else changed.
- **Tools:** `clip_import` reads .glb, .bvh, set files and catalogs (`$pick` cuts from CMU or
  100STYLE, downloading the takes; `list=true` shows a file's rig and clips); `mocap` describes the
  databases, finds takes (CMU's ledger, 100STYLE's styles and descriptions), downloads them and
  cuts one moment into a set (`mocap cut=13_17 at=1.5-2.6 name=Jab set=MINE`). Catalogs for every
  embedded set in anim/catalogs/.
- **Where it shows:** a new station, **Walk Styles** (sixteen walkers on lanes, each in its style
  at its own pace; station guide), and Shardfall's villagers: Tomas strolls with his hands
  clasped behind his back, Elsie bounces along. NPCs can now move as slowly as an old man's
  shuffle (2% of full speed). New field-guide words: gait cycle, foot sliding.
- Not done: FBX (RancidMilk's CMU retargets; convert to glTF first); a Mixamo map for glTF;
  100STYLE's other gaits (backward, sideways, running, idles): `mocap cut=Old_FR loop=true` cuts
  any of them; the whole-database CMU survey (the ledger, anim/cmu's 4,770 clips) is still the one
  my-3D2dge's cmu.mjs made, while single takes and catalogs go through the native tools.
  (All of it done in the follow-up above.)

## Animation: moves as data, and motion clips from open libraries — 2026-10-09
Compared the puppet with my-3D2dge's (the same approach: a skeleton posed by math, two-bone IK,
no animation files) and took what it does better, rebuilt on this engine's foundation (pure pose
functions, state saved with the simulation, tools):
- **One skeleton.** The biped is now a `Skel` (pelvis, chest, head and their frames, shoulders,
  elbows, hands, hips, knees, ankles, toes, weapon direction). `procedural` builds it from the
  walk, crouch, crawl, climb, swim, air, hits and the current move; `clips::over` lays clips on
  it; `dress` turns it into parts (gear, helmets, capes and attachments follow its frames).
- **Moves as data** (`anim/moves.toml`, `moves.rs`): 31 moves, each an arc the hand or foot
  sweeps around the shoulders (from/to degrees, height and reach in arm lengths), timing (wind,
  active, recover, where the hit lands, hold) and body motion (lunge, hop, crouch, lean, twist,
  spin). The nine original actions keep their names and order (slash, overhead, thrust, cast and
  throw now take my-3D2dge's numbers); its other moves were converted to these units (radians to
  degrees, its units to arm lengths, hip heights and metres; its spin is `spinslash` here, ours
  stays the whirlwind); plus stir, stir_back and flick for townsfolk. A skill's hit lands where the skill says; the phases stretch
  around it. `PuppetState::set_action` remembers where the hands were when a new action starts
  (`chain_*`), so chained attacks wind up from there. The whirlwind now turns the body; kicks lift
  the foot through its wind-up; the weapon and its trail follow the arc. `MoveId` is written by
  name in data (an unknown name lists the moves).
- **Legs follow travel** (`PuppetState::travel`), **idle breathing, weight shift and blinks**
  (each character on its own beat: `seed`, from its id on the first tick).
- **Motion clips** (`clips.rs`): the readable key-pose format, kept as is so clips move between the
  engines (hips in percent of standing hip height; body/chest/head turn, lean, tilt; shoulder
  reach and shrug; each limb a direction plus a bend and a twist; feet; blade; root travel).
  Decoding is a port of the reference decoder onto the puppet's own proportions: no source body
  is needed. Checked against it: every limb within a few degrees on sword, dance, breakdance and
  death clips (the spine is one segment here, so a curled spine differs by up to ~14°; lying
  down the thicker torso keeps the pelvis higher). Clips fade in and out over 0.25 s, the old
  one held under the new; `UPPER` keeps the walking legs, `MIRROR`, `TRAVEL`, `ONCE` (fade out at
  the end; any action ends it). State holds only ids and times, so rewind and replays are exact.
- **The libraries, translated with tools.** `clip_import` reads a my-3D2dge set script or a
  readable set (or a folder of them), writes `anim/<set>.json` with the legend and credits,
  checks it reads back and reports the fit. Embedded: QUATERNIUS (88 clips, Universal Animation
  Library 1 and 2, CC0), MESH2MOTION (177, CC0), CMU (60 curated moments, free for all uses):
  325 clips, 6,469 key poses, 1.9 MB. On disk only: anim/cmu/ with every take of the CMU database
  (113 subjects, 4,770 clips, 241,740 key poses, 68 MB, ledger `takes.tsv`), `clips load=cmu`.
  Decision: readable key-pose text counts as data, not an asset file (AGENTS.md updated).
- **Tools:** `clips` (list, find, show as readable text with source and licence, load a folder),
  `clip_import`, `anim_reload`, `animsheet move=` / `clip=` (`mirror`, `upper`, `travel`, `hit`,
  `side`, `def`).
- **Where it shows:** room NPCs perform (`clips = [...]`, `moves = [...]`, `hold`, `mirror`,
  `upper`, `tempo`); any look can have an `idle_clip`; `anim.tempo` and `anim.mirror` settings.
  New stations in the animation wing: **Motion Library** (seven performers: dance, fight, sword,
  everyday, acrobatics, falls, monsters; tempo and mirror pads) and **Action Moves** (blades,
  fists, kicks, magic and more). Shardfall: the hero dies with a captured fall (a different one
  each time) and lies still until rising; cheers on level-up and when a boss falls (upper body,
  never blocking); villagers idle like people when they stop; townsfolk bow when greeted.
  Monsters keep the quick procedural topple (corpses clear in 1.7 s).
- New field-guide words: motion capture, retargeting, key pose, anticipation.
- Tests: `tests/motion.rs` (9), unit tests in moves.rs and clips.rs.
- Not done: a native importer for raw captures (BVH, glTF, ASF/AMC: the sets were translated from
  my-3D2dge's fitted conversions); cloth capes (still three cones); monsters with clips. (The
  importer and monsters in captured motion came after: see above.)

## Fix: slides stopping dead on flat floors, treadmill walkers — 2026-10-08
- **Not floor seams.** Slalom's floor is a single block. Rapier's character controller sometimes
  drops all of a move's horizontal motion when the move presses down into level ground: the
  floor's normal comes back as (0, 0.99999994, 0), which leaves its slope handling no horizontal
  tangent, so it reads the tiny downward remainder as slipping on a non-slip slope and removes
  everything. On the Slalom ICE run that was 8 ticks in 120. The "blocked by a wall" step
  then copied the lost motion into the velocity: an ICE slide on the momentum model stopped dead
  (7.5 to 0 m/s), and the instant model hitched for a tick. Now, when only level ground was hit
  and motion was lost, `character.rs` redoes the move in two parts, across and then down.
  Kinematic platforms still carry the character once: the across part never touches the floor.
- **Treadmills.** The belt's push is subtracted before the blocked check, so walking against a
  belt keeps the walker's own velocity and stride (Walk Cycles' treadmill walker: -0.3 to
  -3 m/s, about 2 steps a second). A belt that shoves the character into a wall stops it rather
  than setting it walking backwards.
- Tests (tests/movement.rs, on an inline flat-floor room with a belt):
  `slides_keep_their_speed_across_a_flat_floor` (momentum with the ICE settings and instant: no
  tick under 95% of top speed, across two floor materials), and
  `walking_against_a_treadmill_keeps_the_stride` (the walker's speed, steps and creep, the
  player's full stride against the belt, and no walking backwards into the wall). Both fail
  without the fix.

## Verticality Tower stairs you can walk up — 2026-10-08
The tower's stairs rose 0.5 m a step, above the character controller's 0.32 m auto-step
(`movement.step_height`), so every step needed a small jump. They were an engine quirk the guide
writers noted, not a design choice. Decision: re-cut them in data rather than raise the step
height (0.32 m keeps crates and curbs as jumps everywhere else). They are now 14 steps of 0.25 m
(the playground's rise) with the same 3.5 m total: five steps west along the lobby's south wall to
a corner, then nine north to the landing, since 14 one-metre treads don't fit in the old 7-row
stairwell. Legend `1`-`9`, `A`-`D` = step n at n x 0.25 m. The station guide's stair line and the
STAIRS sign moved with them. Checked in the REPL: from the lobby floor to the first-floor
checkpoint (y 3.52) with move input only, grounded all the way.

## Effects from the 2D WebGPU showcase, and a station guide in every room — 2026-10-08
Reviewed [michaelcrosato/2d-webgpu-demo](https://github.com/michaelcrosato/2d-webgpu-demo) (about 80
explained 2D GPU scenes, WebGPU and WebGL2). Many of its effects were already in Pavilion (bloom,
outlines, pixel art, dithering and palettes, CRT, grading, chromatic aberration, distortion,
screen-space GI, GPU particles, SDF shapes and text, soft bodies, verlet ropes). Purely 2D tricks
(Mode 7, raycaster, pseudo-3D road, 2D tilemaps, parallax) do not fit a 3D engine and were skipped,
as were GPU-only simulations (fluids, reaction-diffusion, slime, falling sand), which need a
texture-based material path this renderer does not have. Taken, and done the 3D way:
- **Painterly and print styles** (`view.filter.stylize` = paint / halftone / ascii / sketch, with
  `stylize_on` part, `stylize_size`, `stylize_mix`, `stylize_color`), in the composite on the
  finished picture: a generalized Kuwahara filter (8 sectors, polynomial weights; samples from the
  other part of the scene are skipped so paint never bleeds) plus brush streaks and canvas; CMYK
  halftone (four dot screens at 15/75/0/45 degrees, each dot reading its own centre; black only
  when no colour is kept); ASCII (12 characters from 5x7 bitmaps packed in integers); pencil
  sketch (three hatching families by darkness, Sobel contours, lines that redraw 6 times a
  second, coloured-pencil paper). A cheap picture (`base_color`: no GI/outlines/haze) is what they
  sample around a pixel; paint adds back what outlines and GI changed at the pixel.
- **Volumetric light** (`view.haze`, `haze_height`, `shafts`, `shafts_forward`, `halos`): the
  composite marches each pixel's view ray through height haze in 32 jittered steps, asks the
  sun's shadow map whether each bit of air is lit (shadows cut dark shafts), weights it by a
  Henyey-Greenstein phase and dims what lies behind; lamp halos are the closed-form integral of a
  point light along the ray (an arctangent per light). The post pass now binds the shadow map, its
  comparison sampler and the point lights. Pixels with no surface (sky, the void round a level)
  get no haze, and the haze stops a metre under the player's feet.
- **Screen transitions** (`view.filter.transition` = none / fade / iris / diamonds / dissolve /
  mosaic / blinds, `transition_time`, `transition_demo` loops one): a per-pixel mask from one
  number. The app plays the chosen one (default iris, on the player) when you arrive somewhere
  new: another place in Shardfall, or an F2 teleport. Tool captures never trigger it.
- **Wind** (`view.wind`, `wind_angle`, `wind_gusts`, `grass_push`): vertex animation in the scene
  shader for instances flagged `flags::SWAY` (leaves: moves with height above the instance's
  bottom; SDF spheres move whole) and `flags::GRASS` (bends with the square of height, away from
  the player's feet within 0.9 m, and sinks as it bends); gusts roll along the wind as a slow
  wave. Shadows sway too. New `MeshKey::Tuft` (7 two-sided blades). Data: `Sway` (none / leaves /
  grass) on `Visual`, `Decor`, room `[[object]]`s and legend props; legend `grass = { density,
  height, color }` scatters tufts per cell. The wilderness grows grass tufts
  (`terrain.grass_density`, default 0.9 per cell) and its tree canopies sway.
- **Rippling water** (`view.water_ripples`, `water_speed`, `water_fade` = amplitude half-life,
  `water_rain`): `pav_view::water` runs the 2D wave equation on a height grid per water zone
  (cells >= 14 cm, at most 160 x 160, 60 steps/s, reflecting edges, a weak spring back to the
  waterline). Anything crossing the waterline dents it by its speed, splashes and explosions drop
  big dents, rain small ones; dents displace without adding velocity (an early version sank the
  whole pool). Strips of one pool with the same waterline merge into one surface. The mesh's
  vertex colours carry the slope shading and crest foam, so ripples show in flat style too. The
  water's own timestep catches up to 0.5 s (tools render now and then).
- **Teaching (the demo's best idea, extended)**: every room has a station guide: `[learn]` in the
  room file (what you see, how it works step by step, where games use it, phrases to ask for it,
  cost, `knobs` = live settings, field-guide `terms`, `[[learn.code]]` excerpts copied from the
  engine) and a `note` on every pad (shown at the bottom of the screen while you stand on it).
  In the game: H (or the room card's **How it works**) opens the guide window with live sliders
  for its knobs; Esc → **Field guide** has ~85 words (`crates/pav_core/src/field_guide.toml`,
  searchable, cross-referenced) and every room's ask-for-it phrases (click to copy, room name to
  go there). Tools: `room` prints the guide and pad notes, `guide term= / search= / asks=true`.
  `pav_view/tests/learn.rs` keeps it honest: words exist, knobs are real settings, code files
  exist, pads with params have notes. The 34 existing rooms' guides were written by five agents
  from the engine's code (every snippet checked line by line against its file); bloom.toml is the
  model. They also found stale text, now fixed (helicopter spin-up, platform speeds, soft-body
  blasts, NOIR's split, sphere emissives, rain height).
- **World demo rooms** (generated by scripts kept outside the repo, then hand-editable):
  *Light Shafts* (vfx 6: a hall lit through west slits and a slatted roof, a colonnade with
  lanterns and swaying trees; HAZE / BEAMS / SUN pads), *Wind & Water* (vfx 7: a meadow of grass
  legend cells, trees and banners that sway, reeds, a pond with a step, a wading ring and a deep
  middle, a ball dropper; WIND / TOWARD / WATER pads), *Paint & Print* (aesthetic 5: each style
  on OFF / ALL / OBJECTS / WORLD, SIZE and COLOUR rows, the new whole looks; the village diorama),
  *Screen Transitions* (aesthetic 6: a pad per kind that loops it, speeds, STOP, NONE). Open-air
  rooms set `cutaway.height_cut = false` so canopies are not cut near the player.
- **Look & Filters**: new sections *Paint, print & sketch* and *Haze & light shafts* (presets:
  Oil paint, Gouache, Thick impasto, Underpainting, Comic print, Fine print, Newsprint, Terminal,
  Colour ASCII, Pencil, Coloured pencil, Loose sketch; Clear air, Light haze, Dusty sunbeams,
  Morning mist, Smoky hall, Lamplight, Pea soup); the arrival transition is in *Screen*. New whole
  looks: Oil painting, Painted world crisp heroes, Comic book, Newspaper, Hacker terminal,
  Sketchbook, Sketched world painted heroes, Sunbeams, Lamplit fog.
- **Shardfall**: themes have `haze`, `halos` and `grass` (themes.toml): torches glow in the smoke
  of the Ashen Halls and the dust of the Bone Crypts, spores in the Fungal Hollows; the Overgrown
  Ruins grow grass and the Sunken Temple seaweed in patches that part around the hero (placed by
  a hash of level and room, so levels are otherwise built exactly as before); the dusk town has a
  light haze with lamp halos; a gentle draught everywhere; the iris opens on the hero at every
  arrival.
- Checked: captures of every style (town, Mix & Match, Paint & Print), haze/shafts/halos (town,
  Light Shafts hall and colonnade), wilderness grass in wind, the feel-lab pool and the Wind &
  Water pond (wakes, splash rings from dropped balls, rain), each transition mid-close, four
  levels with their themes' air and grass; in the running game under Xvfb: the station guide (H)
  with live sliders, pad notes (STORM), the field guide and the Look & Filters sections.
- Not done: GPU compute simulations from the demo (fluids, sand, slime, reaction-diffusion,
  boids) need a texture material path; halos ignore shadows (a lamp behind a pillar still glows
  in front of it); the haze is a full-resolution 32-step march without temporal filtering, so it
  shows fine grain; the water is drawn opaque.
- Engine quirks the guide writers found (not fixed here): the stealth room's painted cone is cast
  from 0.6 m while sight is checked from 1.3 m. Fixed since (see above): the Verticality Tower's
  0.5 m stairs needed small jumps (auto-step is 0.32 m); an ICE slide on the momentum model stopped
  dead in Slalom (not at floor seams, as first thought); a treadmill walker's legs barely moved
  (belt push counted as blocked).

## Look & Filters: every filter on a part of the scene — 2026-10-08
- **Parts of the scene.** Every drawn instance is either *characters & objects* or the
  *environment*. The view flags `flags::OBJECT` on everything emitted after the static regions
  (characters, props, loot, projectiles, effects, game decals) except fixed scenery:
  `RenderObject::scenery` (from `Sim::frame`) is an entity with no character, a fixed or no
  body, no behaviour, bomb, health, vehicle, soft body or lifetime, and not a game actor
  (town buildings' props, braziers, columns, lava pools, sconces). The scene shader adds 1024
  to the normal buffer's w on objects (groups now fold into 1..1023; still exact in 16-bit
  floats; negative = pixel art as before), so the composite knows each pixel's part.
- **Targets per filter** (`all` / `objects` / `environment`): `view.outlines_on`,
  `view.filter.color_on` (palette, levels, dither), `grade_on` (temperature, tint, contrast,
  brightness, saturation), `scanlines_on`, `grain_on`, `chroma_on`; `pixel_target` gained
  `objects` and `environment`. Shading per part: `view.style_objects` /
  `view.style_environment` (flat / cel / lit; unlit markers and glows keep theirs; the old
  `view.style` still forces one style everywhere). Curvature, vignette, `pixelate` and the glow
  settings stay whole-screen. A pixel's part is read at the pixel-art block centre, so blocks
  agree; the dither pattern now steps with pixel-art blocks.
- **The look layer** (`pav_view::look`): nine sections (pixel art, shading, outlines, palette,
  grading, scanlines, grain & fringe, screen, glow) own fixed view paths; a section that is on
  replaces the scene's own settings (rooms, pads, places, the F1 panel), off hands them back.
  `looks.toml` (embedded) holds 3-8 presets per section (they never change the part a filter
  is on) and 12 whole looks: Pixel heroes, HD-2D, Pixel world crisp heroes, Game Boy world,
  Game Boy, 16-bit, Cartoon, Comic noir, Flat paper, Arcade CRT, Hologram heroes, Posterized
  world. Per-channel posterize shifts the hue of dull colours (the town's grey floor went pink
  and olive at 5-6 levels), so presets use 8+ levels and 24 for pixel-art levels.
- **Menu**: Esc → *Look & filters* (also a button at the top of the F1 panel): whole looks,
  *Scene default*, a before/after *Compare* line, *My looks* (save / use / delete), then a
  collapsible section per filter with an on switch, the part picker (whole scene / characters
  & objects / environment; pixel art also characters only, the hero, other characters, all but
  characters, one entity), preset chips (the matching one is highlighted) and sliders. Moving
  a slider switches its section on, starting from what was on screen. Works with the gamepad
  cursor (B closes). The look is saved to `shardfall_looks.json` next to the exe (browser:
  local storage) a moment after changes and on quit; agent tools over the live bridge see and
  change it.
- **World demo**: new room *Mix & Match* (aesthetic wing, order 4, `rooms/mix_match.toml`,
  generated from looks.toml by a script kept outside the repo): a village diorama with walkers,
  crates and balls and a fixed statue; west, a row per filter (PIXEL ART, INK OUTLINES, GAME
  BOY, NOIR GRADE, SCANLINES) with OFF / ALL / OBJECTS / WORLD pads that stack, then OBJECT
  STYLE and WORLD STYLE rows; east, a pad per whole look (they set every look path) and CLEAN.
- **Tool**: `look` (`name=` a whole look, `section=` + `preset=` / `on=` / `enabled=`,
  `reset=true`, `compare=`, `bench=all` renders this moment under every look, numbered).
  Sessions keep a look layer (`Session::look`) applied in every capture and across `load`.
- Tests: `pav_view/tests/parts.rs` (objects vs environment flags in town incl. a spawned crate
  and fixed scenery, pixel art on either part, styles per part, parts reach the renderer) and
  `tests/look.rs` (each section copies exactly its paths and no path has two owners, presets
  stay in their section and apply cleanly, looks switch on what they name, presets keep the
  part, JSON round trip). Checked visually with `look bench=all` in town, pad stacking and
  whole-look pads in the room, and the window in the running game under Xvfb.
- Not done: bloom is whole-scene (it is computed from the HDR image before the composite);
  a pixel's part comes from MSAA sample 0, so part edges are not anti-aliased.
## Turntable/animsheet frame tall creatures — 2026-10-08
- `turntable` and `animsheet` (`creature_frames` in `game_tools.rs`) cut off tall creatures.
  The Hollow King (5.26 m) lost its head and horns in every frame because the camera aimed at
  the 1.7 m capsule's centre from a distance set by `scale` alone. Now the tool poses every
  frame as the view will (same state, rig and camera direction), takes the box around all of
  them (`agent_tools::pose_bounds`, shared with `anatomy`) and aims at its centre. The distance
  is the old `2.2 + scale * 2.6` unless that would spill a box corner out of the frame (with
  `FRAME_MARGIN` 1.15); then the camera backs off. The camera holds still across frames.
- Checked by eye: all five bosses and generated genomes 164/172/191 now show head to feet in
  both tools, swings and orbiting orbs included. Ghouls, skitterers, spitters, oozes and
  ashdrakes keep the same distance, and the aim moves by a few cm at most.

## Pixel art on part of the scene — 2026-10-05
- `view.filter.pixel_art` (block size, 1 = off) turns part of the scene into pixel art while the
  rest stays sharp; `pixel_target` picks what: `all`, `characters` (every puppet), `hero`,
  `others` (characters but the hero), `world` (everything that isn't a character) or `entity`
  (`pixel_entity` = an entity id). `pixel_levels` posterizes the pixel-art colours and
  `pixel_outline` (on) draws a dark one-block outline just outside pixel-art silhouettes.
  Works with the whole-screen `pixelate`, palettes and the rest of the filter stack.
- How: the view flags the chosen instances `flags::PIXEL` (by outline group: entity id + 2,
  1 for level geometry); the scene pass writes their group negative into the normal buffer's
  w; the composite gives a block its centre's colour when this pixel or the centre is
  flagged, so flagged silhouettes step in whole blocks and nothing else is touched. Cost: a
  few extra texel loads per pixel, only while `pixel_art` > 1.
- The Filter Stack Bench has four new pads (ART OFF / CHARACTERS / HERO / WORLD); RESET
  clears it. Test: `pav_view/tests/pixel_art.rs` (each target flags exactly its instances).
- Not done yet: the block grid is fixed to the screen, so with `world` the level shimmers a
  little as the camera slides (snapping the camera to the block grid would fix it);
  shadows belong to the ground they fall on (pixelated with `world`, sharp with
  `characters`); text labels are never flagged themselves.

## Audit: the bot picks up loot — 2026-10-05
- Audit of the whole repo: fmt, clippy (warnings denied) and every test pass (main
  workspace and `pavilion-lite/`); every room file passes `room_check`. The one real problem
  was in the `campaign` balance runs: the bot needed 52-61 deaths and ~15-20 game-minutes
  for level 4 (the first boss, the Hollow King). The commit before the primary-attack fix
  was worse (boss alive after 30 game-minutes), so the cause was older than that fix.
- Cause: items are only taken by walking over them (the auto-loot filter) or by clicking
  them, and the bot did neither on purpose, so it reached every boss in its starting gear.
  Rift gates were ruled out (blocking them on the nav grid changed nothing).
- Fix (`bot.rs` `fetch_loot`): with nothing to fight, the bot walks to visible items within
  12 m that pass the loot filter and picks them up with `GameCmd::Pickup` (as a click
  would). In levels it follows a nav-grid path and skips items the grid can't reach: a first
  version walked straight and got stuck inside the town portal's ring of pillars. It gives
  up on an item after 10 s. Test: `loot.rs` `the_bot_picks_up_loot_nearby` (fails before).
- Campaign now (levels 1-11, deterministic): 0.7 / 1.0 / 1.4 / 8.0 (19 deaths) / 2.6 / 3.4 /
  6.1 / 8.3 / 0.8 / 10.1 / 6.7 game-minutes, 40 deaths in total, hero level 18 at level 12.
- Open: level 12's Frostbound Colossus stops the bot (47-49 deaths in 50 game-minutes at
  hero level 19 with or without this change; the G6 note that the bot clears all twelve is
  out of date). Retuning the final boss is a design call, so it is left as is.

## Primary attack input fix — 2026-10-03
- Shardfall consumed PRIMARY for the equipped skill, then forwarded it to the character
  controller, which also threw a Pavilion bomb (or fired the demo blaster). Clear PRIMARY
  from both held and pressed movement input after the game handles skills. Left click and
  the controller's primary attack now use only the equipped skill; held attacks still repeat.
- Regression coverage checks repeated sword swings without bomb/blaster events or bomb
  entities, and confirms left click still throws bombs in Pavilion rooms.
- Verified the new combat regression fails before the fix and passes afterward; all 23
  combat, gameplay, genre and skill tests pass. Formatting and pav_core clippy (all targets,
  warnings denied) pass.

## Pavilion Lite: the compact engine for AI agents — 2026-10-02
- New folder `pavilion-lite/`: the essential engine as ONE self-contained Cargo package (its
  own workspace, lockfile and docs; about 7,000 lines) to hand to other AI agents. It builds
  with plain `cargo build` on Rust 1.89+ and needs no system packages for headless use.
  `pavilion-lite/package.sh` makes `dist/pavilion-lite.tar.gz` / `.zip` (~110 KB, source only).
- Kept (compact rewrites of pav_core/pav_tools ideas): fixed 60 Hz deterministic sim with
  snapshots, rewind and input replays + state hash; rapier3d physics; the kinematic character
  controller (instant/momentum models, coyote/buffered/variable jumps, steps, slopes, moving
  platforms, pushing props, knockback, dash, axis lock); procedural puppets (biped, blob,
  beast; held items; act poses); ASCII TOML levels (top-down `xz` and new side-view `xy`
  planes, merged blocks, spawns, triggers, markers, movers, `[params]`); triggers, lightweight
  projectiles with teams/damage/knockback, hp/invulnerability, sparks, screen shake; grid A*;
  `Tunable` params by path; one tool registry as CLI, REPL and MCP (24 tools, incl. `capture
  marks=true`, `filmstrip`, `ascii`, `autoplay` bots, `record`/`replay`, `level_check`).
- Dropped: wgpu renderer, egui panels, audio, gamepad, streaming world, rooms/pavilion, soft
  bodies/joints/vehicles, Shardfall. Games are Rust modules implementing a small `Game`
  trait (setup/update/draw/status/params/bot), registered in `src/games/mod.rs`.
- Sample games with bots and tests: `template`, `platformer` (side view, bot finishes in
  ~13 s), `arena` (twin-stick waves, bot reaches wave 3+ in 60 s).
- Verified: 19 tests (engine units, every game runs/draws/rewinds/replays exactly, every tool
  end to end, bots win); clippy clean with and without the window; MCP over stdio; the window
  under Xvfb; a fresh extraction outside the repo builds and passes on Rust 1.97 and checks on
  1.89. Speed: 15k-80k ticks/s; 640x360 capture ~7 ms, 1280x720 ~18 ms on 4 cores.

- Fresh-agent dry run (another model, only the package and AGENTS.md): it built "Key Hunt"
  (six-room dungeon, keys, gate, slimes, bot, 3 tests; ~580 lines, zero compile errors, bot
  wins 40/40 seeds) in ~25 minutes and listed 13 friction points. All fixed: nav grid no
  longer sees characters as holes (`ground_at` ignores characters/props), nearest walkable
  path start, NPC input resets each tick, unknown tool arguments are errors, REPL quoting,
  `ascii` snaps to cells, `rewind` reports its reach, `marks=` kind filter, `autoplay`
  `until=`/`trace=`/`seeds=` sweeps, hidden level spawns, `player_entered`, fuller API docs.
- Second dry run (a smaller model, a side-view "Tower Climb"): it built and registered a
  working game with passing tests, but its bot couldn't reach the top: platforms stacked
  straight up made the player bump its head, and the level put ledges exactly at the jump
  apex. Added one-way platforms (`oneway = true` / `Spawn::oneway()`), a `reach` readout in
  `status` (jump height, air time, jump length), jump-math and "don't weaken the bot test"
  guidance, and a clearer side-view description (the map reads like a picture). The sample
  platformer now uses one-way planks; its bot predicts landings and brakes mid-air instead
  of hopping into the pit (wins in 11.8 s, 1 death).
- Idle characters on static ground skip rapier's controller (one raycast): 30 idle walkers
  851 -> 5565 ticks/s; buried under 200 crates 80 -> 460 ticks/s.

## Decisions (Pavilion Lite)
- **CPU renderer instead of wgpu**: agent sandboxes rarely have a Vulkan driver; a software
  rasteriser (boxes/cylinders as triangles, spheres and tapered capsules ray-traced per pixel,
  sun shadow map, cel/lit/flat/glow, outlines, fog, 8x8 bitmap font) makes screenshots work
  everywhere and keeps the package dependency-light. The window shows the same images
  through winit + softbuffer (no GPU).
- **One Cargo package**: engine = library, games = modules of the `pav` binary (editing a game
  recompiles only the binary). The package is its own workspace so it builds the same inside
  this repo or alone.
- **Single entity struct + game-owned side tables** (like Shardfall's actors): enemies' brains
  live in the game struct keyed by entity id, so snapshots stay one clone.
- **Events are read one tick later** (`w.events` in `update`), giving game code one simple
  hook instead of pre/post callbacks.
- Characters' `pos` is their feet; colliders become queryable at spawn (`set_aabb`), so
  raycasts work in `setup` before the first physics step.
- MSRV 1.89 (nalgebra/wide via rapier 0.36); no pinned toolchain in the package.

## Shardfall naming and Vercel deployment — 2026-10-01
- Renamed the GitHub repository to `michaelcrosato/shardfall` and updated the local remote.
  `main` remains the default; repository visibility remains private. Shardfall is the app
  and repository title; Pavilion remains the name of the playable tech demo/engine.
- Renamed the game binary/window, browser loader and generated assets, startup settings,
  log and screenshot names, MCP server, documentation, and Linux handoff archive.
- Rebuilt the Windows game and preserved the existing settings and saved hero while
  renaming the game folder to `Desktop\New AI Stuff\Shardfall`. The `Shardfall` desktop
  shortcut points to `shardfall.exe` there.
- Added `vercel.json` and `scripts/build-vercel.sh` for a static WebAssembly deployment.
  The build selects pinned Rust 1.98.1 and a matching wasm-bindgen CLI from `Cargo.lock`;
  the downloaded CLI is checked against the official release checksum. Build output is
  `target/web/`. Vercel metadata and generated environment files are ignored by Git.
- Production: https://shardfall-eight.vercel.app (project `shardfall`, Vercel team
  `michaelcrosato-1122s-projects`). Initial deployment used the verified local static
  build; GitHub is connected for subsequent deployments.
- The GitHub-triggered cold build of `4ff62c3` also succeeded on Vercel and was
  automatically promoted to production, confirming the remote toolchain/bootstrap path.
- Verification: Windows `dist` build, browser/Vercel build, shell lint/format checks;
  browser town/HUD, inventory input, local-storage save, and tech demo rendering. The
  initial production HTML, JS and Wasm returned HTTP 200 and matched the local build
  byte for byte. The subsequent Vercel rebuild serves all three successfully, with Wasm
  as `application/wasm`. Live browser startup has no console errors.
- Linux handoff is now `shardfall-linux-agent.tar.gz`, with a `shardfall/` archive root.

## Linux agent handoff — 2026-10-01
- Added `START_HERE.md` with Debian/Ubuntu prerequisites, Rust setup expectations, CLI
  examples, simulation/rendering checks, MCP usage, and native game launch commands.
- Added `scripts/package-agent.sh`: packages current tracked source (including local edits)
  plus the handoff guide/script under `pavilion/`, with a SHA-256 checksum. Git history,
  build caches, and generated output are excluded. New source files must be added to Git
  before packaging. Updated the design's branch instruction to `main`.
- Verified shell formatting/lint, extracted archive contents and source hashes (199 files),
  executable script permissions, and Cargo workspace metadata from the extracted source.
  This is a source handoff; the receiving agent builds binaries with its Linux dependencies.
- Delivered `pavilion-linux-agent.tar.gz` and its `.sha256` to the Windows desktop.

## Windows desktop build — 2026-10-01
- GitHub's default branch and this local checkout now use `main`.
- Built `main` at `54fa519` with `scripts/build-windows.sh` (Rust 1.98.1,
  `x86_64-pc-windows-gnu`, optimized `dist` profile, six build jobs).
- Verified the Windows CLI natively: 600 town simulation ticks and a 640×360 offscreen
  Vulkan capture (`out/build/windows-town.png`). Inspected the rendered town. The game GUI
  was not launched during validation.
- Copied the self-contained `pavilion.exe` to
  `C:\Users\micha\OneDrive\Desktop\Pavilion\pavilion.exe` and created `Pavilion.lnk` on
  the desktop. Verified the copy's SHA-256 and the shortcut target. Content is embedded;
  the executable imports only Windows system DLLs.

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
- Gamepad: D-pad left toggles the map. Browser saves go to local storage. (Menus: see
  "Gamepad menus" below.)
- Tests: tests/feel.rs (gibs, streak bonus, boss entrance), nav unit test (paths round walls,
  flow points the way).

## Gamepad menus — done
- Every window works on a controller through a virtual cursor (pointer events fed to egui):
  left stick (speeds up when pushed fully) or D-pad steps move it, A / X are left / right
  click, holding Y is shift, the right stick scrolls and zooms the tree, B closes. D-pad down
  opens the hero's panels and LB / RB cycle inventory, character, skills, passive tree. While
  a window or the menu is open the pad doesn't reach the game. Hints in the windows switch
  to button names on a pad (`arpg_items::hint`); the menu lists the game's pad controls.
- `--pad-script FILE` plays a scripted controller (`input::PadScript`), so controller flows
  are testable without hardware: used to check opening the bag and equipping by A, cycling
  to the tree, and taking a path with Y+A, with screenshots.

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
- Gamepad menus use a cursor rather than focus-hopping between widgets: it works for every
  window as written (grids, popups, the tree canvas) and costs no per-window code.
- Navigation is derived data (never saved): rebuilt on demand from the level's blocks, so
  snapshots stay small and identical whether or not it was built.
- Balance is judged by a bot campaign, not by feel alone: it's repeatable, and agents can run
  it after every data change.
