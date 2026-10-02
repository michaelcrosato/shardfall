# AGENTS.md: Pavilion Lite

Pavilion Lite is a small 3D game engine in Rust, built so AI agents can make and test games
without a GPU, a display or asset files. Everything is code: shapes, procedurally animated
characters, levels as ASCII text. The simulation is deterministic (60 ticks per second), the
renderer runs on the CPU (screenshots work anywhere), and one tool registry drives everything
from the command line, a REPL or MCP. People play the result in a window.

Read this whole file before writing code. It is the only document you need.

## Quick start

Needs Rust 1.89+ and network access to crates.io for the first build. No Rust? Install it
with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y` (then
`. "$HOME/.cargo/env"`). No system packages are needed for building, tests or screenshots.

```sh
cargo build                                  # first build: 1-3 minutes
cargo run -q -- games                        # registered games
cargo run -q -- capture game=platformer ticks=60 out=out/p.png    # then look at out/p.png
cargo run -q -- autoplay game=arena seconds=20                    # the game's bot plays
cargo test                                   # engine + every game
cargo run --release -- play arena            # a window, for people (keyboard + mouse)
```

`cargo run -q -- <tool>` rebuilds when code changed, then runs one tool. `./target/debug/pav`
is the same binary without the rebuild check.

## How to make a game

1. Copy `src/games/template.rs` to `src/games/<name>.rs` and rename the struct.
2. Register it in `src/games/mod.rs`: `mod <name>;` and a `GameDef` line in `GAMES`.
3. `setup` builds the world (level, player, enemies). `update` runs the rules every tick.
   `draw` adds HUD and extra shapes. `status` reports state as JSON. `bot` plays the game.
4. Look and test with the tools: `capture marks=true`, `input`, `ascii`, `status`, `autoplay`.
5. Write a test that the bot can win (copy the template's test). `cargo test` must pass.
6. Tell the human: `cargo run --release -- play <name>`.

The sample games are complete, commented examples. Read the one closest to your idea:

| game | shows |
|---|---|
| `template` (~90 lines) | top-down level, player, pickups by trigger, HUD, bot, test |
| `platformer` | side-view level (`plane = "xy"`), momentum movement, one-way planks, moving lift, spikes, checkpoints, enemies you stomp, goal, a bot that finishes it |
| `arena` | twin-stick shooter: aim, projectiles, health, waves, enemy brains, pathfinding, pickups, dash, restart, HUD bars |

## Rules

- **All game state lives in your `Game` struct** (`#[derive(Clone, Default)]`) or in the
  `World`. No statics, globals, `thread_local`, files or clocks: rewinds and replays clone and
  re-run the game, so state outside them breaks.
- **Randomness only from `w.rng`** (deterministic). Same seed + same inputs = same game.
- **Time is ticks**: `w.dt` is 1/60 s, `w.time()` is seconds since start. Count timers down
  by `w.dt` in `update`.
- **Events are from the previous tick**: read `w.events` in `update` (enter/exit triggers,
  projectile hits, kills, jumps, landings, falls). Clone the list before changing the world.
- **Entities are ids** (`Id = u32`, never reused). `w.get(id)` returns `Option`: an entity
  may be gone. Use `w.set_pos` / `w.set_vel` / `w.push` to move things, not `e.pos = ..`.
- **Units and axes**: metres, seconds, degrees in data. `+x` east, `+y` up, `+z` south
  (`-z` north). Yaw 0 faces south (+z). A person is 1.7 m tall.
- **A character's `pos` is its feet**; other entities' `pos` is their centre.
- **Colours are `"#rrggbb"`** strings everywhere.
- **No asset files.** Build looks from shapes, puppets and `Draw` calls.
- Keep the engine (`src/*.rs`) and your game (`src/games/<name>.rs`) separate. Change the
  engine only for a real missing feature, and keep `cargo test` green.

## Files

```
src/world.rs      World: entities, physics (rapier3d), triggers, projectiles, sparks, camera, look
src/entity.rs     Entity, Spawn builder, Shape, Body, Look, Mover (moving platforms)
src/character.rs  walking characters: movement models, jumps, slopes, platforms, knockback, dash
src/puppet.rs     procedural bodies (biped, blob, beast) and their animation; held items; act poses
src/level.rs      ASCII levels in TOML (top-down or side view)
src/sim.rs        Game trait, Sim (fixed tick, rewind, replays)
src/nav.rs        grid pathfinding (A*)
src/view.rs       world -> picture; Draw (HUD text, bars, labels, extra shapes)
src/render.rs     CPU renderer (shadows, toon shading, outlines, fog, text); PNG
src/tools.rs      the agent tools: CLI, REPL, MCP
src/window.rs     the play window (winit + softbuffer)
src/params.rs     tunable parameters by path (`movement.speed`...)
src/input.rs      Input (move, aim, buttons)       src/util.rs  Color, Rng, angles
src/games/        the games (mod.rs is the registry)
```

## API

`use pavlite::prelude::*;` brings in everything below plus `Vec2`, `Vec3`, `Quat`, `json!`,
`Value`.

### Game

```rust
pub trait Game: Clone + Send {          // (Clone via #[derive(Clone)])
    fn setup(&mut self, w: &mut World);                       // build the world
    fn update(&mut self, w: &mut World, input: &Input);       // rules, every tick
    fn draw(&self, w: &World, d: &mut Draw) {}                // HUD, extra shapes (read-only)
    fn status(&self, w: &World) -> Value { Value::Null }      // JSON for agents
    fn params(&mut self, v: &mut dyn ParamVisitor) {}         // tunables as game.<name>
    fn bot(&mut self, w: &World) -> Option<Input> { None }    // scripted player
}
```

Each tick: the player's character gets `input`, then `update` runs, then the engine moves
movers and characters, steps physics, moves projectiles, checks triggers, and records events.
To restart or change level inside the game: `*w = World::new(w.seed); *self = Default::default(); self.setup(w);`
(this also resets the tick count and any `game.*` values tuned with `set`; carry over what you
want to keep).

`fn params` lists your tunables with `v.float(name, &mut self.x, min, max, help)`,
`v.bool(name, &mut self.flag, help)` and `v.choice(name, &mut index, &["a", "b"], help)`.

### World

| | |
|---|---|
| `w.spawn(Spawn) -> Id`, `w.despawn(id) -> bool` | create / remove |
| `w.get(id)`, `w.get_mut(id)` | read / change plain fields (hp, color, kind, visible, puppet...) |
| `w.player()`, `w.player: Option<Id>` | the controlled entity (the camera follows it) |
| `w.each(kind)`, `w.ids(kind)`, `w.count(kind)`, `w.nearest(kind, pos, max)` | find by kind |
| `w.set_pos(id, pos)`, `w.set_rot(id, quat)`, `w.set_vel(id, v)` | teleport / velocity |
| `w.push(id, dv, stun)` | kick a prop, knock back a character (no control for `stun` s) |
| `w.drive(id, Input)` | what an NPC character does this tick: call it every tick in `update`; undriven NPCs stand still |
| `w.player_entered(kind) -> Vec<Id>` | triggers of `kind` the player touched last tick (pickups, goals, hazards) |
| `w.dash(id, dir, speed, secs)`, `w.act(id, Act::Swing, secs)` | dash; play a pose |
| `w.damage(id, amount) -> bool` | hp down, white flash; true if it just died; ignored while `invuln > 0` |
| `w.shoot(Shot)`, `w.burst(pos, "#hex", count, speed)`, `w.shake(strength)` | projectiles, sparks, screen shake |
| `w.raycast(from, dir, max, ignore: Option<Id>) -> Option<RayHit{id, point, normal, dist}>` | first solid hit (characters and props count) |
| `w.can_see(a, b, &[ids to ignore])`, `w.overlap(center, r) -> Vec<Id>` | line of sight; everything touching a sphere |
| `w.ground_at(x, z, from_y) -> Option<f32>` | level floor height below a point (blocks and platforms only) |
| `w.solid_at(p, half)`, `w.solid_box(p, half_vec)` | the wall/block filling a box, if any |
| `w.inside(trigger) -> &[Id]` | what overlaps a trigger now |
| `w.marker("name")`, `w.markers_named("name")` | points from the level |
| `w.load_level(toml) -> Result<LevelInfo>` | build a level into the world |
| `w.rng` `.f32() .range(a,b) .below(n) .chance(p) .dir2() .in_sphere()` | randomness |
| `w.camera`, `w.env`, `w.config` | camera, light, physics settings (also params, below) |
| `w.tick`, `w.dt`, `w.time()`, `w.events`, `w.entities`, `w.shots` | state |

### Spawn and Entity

```rust
Spawn::new("crate", pos).cube(Vec3::splat(0.8)).body(Body::Dynamic).color("#a0703c")
Spawn::new("coin", pos).ball(0.3).body(Body::Trigger).look(Look::Glow).spin(Vec3::Y * 3.0)
Spawn::new("lift", pos).cube(Vec3::new(3.0, 0.3, 2.0)).mover(Vec3::X * 6.0, 4.0, 0.5)  // kinematic
Spawn::new("plank", pos).cube(Vec3::new(2.0, 0.2, 2.0)).oneway()        // jump up through it
Spawn::character("goblin", feet).size(1.2, 0.35).agility(0.8, 0.0).hp(5.0).team(2)
    .puppet(Puppet::biped("#40a040").held(Held::Sword).hat("#603010"))
```

Builder methods: `name shape cube ball body color look rot vel spin team hp life hidden mover
oneway material(density, friction, bounce) ccd puppet size(height, radius) agility(speed, jump)`.

- `Body`: `Static` (walls), `Dynamic` (props), `Kinematic` (moved by code: `vel`, `spin`,
  `mover`; carries characters), `Trigger` (no collision; Enter/Exit events), `None` (visual).
- `Shape`: `Box{half}`, `Sphere{radius}`, `Capsule{half_height, radius}`,
  `Cylinder{half_height, radius}`; `Shape::cube(size)`, `Shape::ball(r)`.
- `Look`: `Cel` (toon, default), `Lit`, `Flat`, `Glow` (self-lit; pickups, bullets, lava).
- `Entity` fields: `id name kind pos rot vel spin shape body color look visible team hp
  max_hp flash invuln oneway life mover character puppet`; `e.center()`, `e.flat_dist(p)`,
  `e.alive()`.
- Teams: projectiles pass through their own team. `hp > 0` entities take projectile damage.

### Characters, movement, puppets

`Entity::character` (`Character`): `height radius vel grounded air_time facing input speed jump
stun dash_time anim`, `forward()`. The player's character is driven by the tick's input;
others by `w.drive`. Characters block each other, climb steps and slopes, ride moving
platforms and push dynamic props. Only `jump` is built in; your game gives other buttons
meaning. Shared tunables (`movement.*`): `model` (`instant` | `momentum`), `speed`, `accel`,
`decel`, `skid`, `air_control`, `gravity`, `jump_height`, `allow_jump`, `jump_cut`,
`coyote_time`, `jump_buffer`, `max_fall`, `step_height`, `push_mass`, `lock_axis`
(`z` for side views), `face_aim` (face the mouse). Per character: `.agility(speed, jump)`
multiplies `movement.speed` and the jump height (0 = can't jump). A `move_dir` shorter than 1
walks slower (analog). NPCs: `w.drive` them every tick (an undriven NPC stands still).

**Jump math for level design**: a held jump rises `jump_height` m and stays in the air
`2 * sqrt(2 * jump_height / gravity)` s, covering `speed * air_time` m on flat ground (the
`status` tool reports this as `reach`). Defaults: 1.35 m up, 0.58 s, 3.5 m far. Keep ledges
at most ~80% of the jump height above where the player takes off, and gaps at most ~80% of
the jump length; a platform right at the apex is a coin toss. Releasing jump early cuts the
rise (`jump_cut`).

`Puppet`: `Puppet::biped(hex)`, `::blob(hex)`, `::beast(hex)` then `.scale(k)`,
`.held(Held::Sword | Gun | Staff)`, `.hat(hex)`, `.colors(skin, legs)`; fields `skin body legs
feet eyes accent look stride`. They walk, bob, lean, squash on landing; `w.act(id, Act::Swing |
Shoot | Cheer, secs)` plays an action pose. Hit flash is automatic.

### Projectiles, events, input

```rust
w.shoot(Shot::new(from, dir * 20.0).owner(me).team(1).damage(2.0).radius(0.15)
    .color("#9ef0ff").knockback(4.0).life(1.5).gravity(0.0));
for ev in w.events.clone() { match ev {
    Event::Enter { trigger, other } | Event::Exit { trigger, other } => ..,
    Event::Hit { target, owner, pos, damage } => ..,   // damage already applied
    Event::Killed { id, by } => ..,                     // hp hit 0 from a shot; despawn it yourself
    Event::Jump { id } | Event::Land { id, speed } | Event::Fell { id } => ..,  // Fell: below sim.kill_y
} }
```

Enter/Exit fire for ANY character or moving body touching a trigger (an enemy walking over a
key too), so check `other == w.player`, or use `w.player_entered("key")`.

`Input { move_dir: Vec2 (x east, y south), aim: Option<Vec3>, held, pressed }`;
`input.down(buttons::FIRE)`, `input.just(buttons::JUMP)`, `Input::toward(from, to)`.
Buttons: `JUMP FIRE ALT USE CROUCH DASH` (keys: Space, J/left mouse, K/right mouse, E,
C/Ctrl, Shift). Move keys are WASD/arrows, relative to the camera.

### Drawing (in `Game::draw`)

The HUD canvas is 360 units tall and `d.width` wide (640 at 16:9). Text `size` is letter
height in those units (8 small, 16 normal, 32 title).

```rust
d.text(x, y, size, "#ffffff", "Score 10");        // top-left corner at (x, y)
d.title(y, size, "#ffd34d", "YOU WIN");            // centred horizontally
d.rect(x, y, w, h, "#101216", alpha);              d.bar(x, y, w, h, frac, "#ff4a6a");
d.label(world_pos, "Boss", "#ffffff");             d.health(world_pos, frac, "#ff5a4a");
d.sphere(center, radius, "#hex", Look::Glow);      d.cube(center, size, "#hex", Look::Cel);
d.line(a, b, radius, "#hex", Look::Lit);           d.ring(center, radius, "#hex");  // flat, on the ground
// Anything else, rotated boxes included:
use pavlite::render::Prim;
d.shape(Prim::Box { center, rot: Quat::from_rotation_y(0.5), half }, "#hex", Look::Cel);
d.shape(Prim::Cone { a, b, ra, rb }, "#hex", Look::Lit);   // tapered capsule: spikes, horns, beams
```

Drawn shapes are free decoration (no physics, no marks in screenshots): prefer them over
entities for torches, spikes, flags and effects.

### Camera and look

`w.camera`: `tilt` (90 = top-down, 0 = side), `yaw` (0 looks north), `distance`, `fov`,
`ortho`, `height`, `follow` (default the player), `target`, `lag`, `shake`. Presets: top-down
`tilt 90 + ortho`; classic `tilt 60, distance 18`; side view `tilt 8, distance 17` with
`movement.lock_axis = "z"`; close third person `tilt 25, distance 7, fov 60`.
`w.env`: `sky horizon sun_elevation sun_azimuth sun sun_color ambient fog shadows outlines`.

### Pathfinding

```rust
// NavGrid::build(world, min_corner, max_corner, cell, radius): min.y is the floor height.
let nav = NavGrid::build(w, Vec3::new(0.0, 0.0, 0.0), Vec3::new(32.0, 0.0, 22.0), 0.5, 0.32);
if let Some(path) = nav.path(from, to) { /* walk toward path[0]; drop it within ~0.5 m */ }
nav.walkable(p); nav.clear(a, b);                  // a cell / a straight line
```

- It reads only the level (blocks and moving platforms); characters and props never block
  it, so build it whenever you like. After the level changes (a door opens, a wall breaks),
  build it again.
- `radius` = the walker's capsule radius (0.32 for a default character). Bigger values close
  doorways for smaller walkers.
- Keep the path in your game state, re-plan every 0.3-0.5 s (not every tick), and steer
  straight at the target when `w.can_see` it (see `arena.rs`).

## Levels

A level is TOML: ASCII map layers plus a legend. Embed it with a raw string (see the games)
and call `w.load_level(TEXT)` in `setup`. Check one without a game:
`cargo run -q -- level_check path=my_level.toml`. Unknown fields are errors (typos show).

Top-down (`plane = "xz"`, the default): columns go east, rows go south, heights come from
the legend.

```toml
name = "Yard"
cell = 1.0                     # metres per character (default 1)
origin = [0, 0, 0]             # world position of column 0, row 0
sky = "#7fb2e5"                # optional colours
horizon = "#dfe9f2"

[params]                        # any engine tunable, applied on load
"movement.speed" = 8
"camera.tilt" = 70

[[layer]]
y = 0                           # base height of this layer
map = """
########
#P..c..#
#..^^..#
########
"""

[legend]                        # one entry per character; '.' and ' ' without one are empty
"#" = { block = { y0 = 0, y1 = 2, color = "#8a8f99" } }            # wall, 2 m tall
"." = { block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }         # floor slab, top at y = 0
"P" = { marker = "player", block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }
"c" = { spawn = { kind = "coin", shape = "sphere", size = 0.3, body = "trigger", look = "glow", color = "#ffd34d", y = 0.8 }, block = { y0 = -0.5, y1 = 0 } }
"^" = { trigger = { kind = "spikes", y1 = 0.5, color = "#e8402a" }, block = { y0 = -0.5, y1 = 0 } }
```

Side view (`plane = "xy"`): the map is a picture of the level as you see it from the side.
Columns go east (+x); the TOP line of the map is the highest row and the LAST line sits on the
layer's `y`. Every block is `z0..z1` deep (default -1..1). Use it with
`"movement.lock_axis" = "z"` and a low camera (`"camera.tilt" = 8`). Mark thin platforms
`oneway = true` so characters can jump up through them from below (otherwise they bump
their heads); a tower of platforms stacked above each other needs that.

```toml
[[layer]]
plane = "xy"
map = """
       c
   ===        F
P       ^
##########  ####
"""

[legend]
"#" = { block = { color = "#5fa83a" } }                                     # fills its cell
"=" = { block = { y0 = 0.8, y1 = 1.0, color = "#b07a43", oneway = true, move = [3, 0, 0], period = 4 } }  # thin moving one-way platform
"^" = { trigger = { kind = "spikes", y1 = 0.5 } }
"c" = { spawn = { kind = "coin", shape = "sphere", size = 0.3, body = "trigger", look = "glow", color = "#ffd34d", y = 0.5 } }
"F" = { spawn = { kind = "goal", size = [0.3, 3.0, 0.3], body = "trigger", look = "glow", color = "#7cf08a" } }
"P" = { marker = "player" }
```

- Every character is one cell, spaces included (a space is empty air), so keep the lines
  aligned: a stray leading space shifts that row one cell east. Compare your map with
  `ascii plane=xy` (side) or `ascii` (top-down) after loading the game.
- `block` / `blocks = [..]`: boxes. `y0`, `y1` (m): above the layer's `y` (xz) or inside the
  cell (xy). `z0`, `z1`: depth of xy blocks (default -1..1). `color look kind inset hp`
  (`hp` > 0: shootable, never merged), `oneway` (jump up through it, stand on top),
  `move = [dx, dy, dz]` + `period hold phase`: a moving platform. Identical neighbours merge
  into one box.
- `spawn = {..}`: one entity per cell. `kind` (required), `shape` (box | sphere | capsule |
  cylinder), `size` (box: edge or [x,y,z]; sphere: radius; capsule/cylinder: [height, radius]),
  `body` (default dynamic), `color look`, `y` (height of its centre above the cell floor;
  default: resting on it), `rot = [x,y,z]` degrees, `spin` (degrees/s around up), `hidden`
  (not drawn), `hp team move period`.
- `trigger = {..}`: invisible trigger box over the cell (neighbours merge): `kind`, `y0`
  (0), `y1` (2), `z0`, `z1`, `color` (makes it visible).
- `marker = "name"`: a point on the cell floor (`w.marker("name")`, `markers_named`).
- **The cell floor** (for `y`, markers and spawns) is the top of that same character's
  blocks; with no blocks it is the layer's `y` (xz) or the bottom of the cell (xy). So a
  spawn with `y = 0.5` on a 1.2 m pillar tile sits at 1.7 m.
- Several things can share a cell: `{ marker = "player", block = {..} }`.
- Big areas: one legend character per surface lets neighbours merge into a few large boxes.
  Patterns like checkerboards stop merging (hundreds of entities); draw such detail with
  `Draw` instead.

## Tools

One registry, three ways in. Every tool returns one line of JSON (only `pav help` on the
command line prints plain text). Misspelled argument names are errors that list the right
ones. A one-shot call starts the game fresh each time (`ticks=N` first advances it N idle
ticks); use the REPL or MCP to keep one game going across many calls.

```sh
cargo run -q -- <tool> game=NAME [seed=N] [ticks=N] [key=value ...]   # one shot (ticks= steps first)
printf 'load game=arena\ninput move=[1,0] hold=fire ticks=60\ncapture marks=true\n' | cargo run -q -- repl
cargo run -q -- mcp                       # MCP server on stdio (.mcp.json registers it)
cargo run -q -- help                      # every tool and argument
```

| tool | what it does |
|---|---|
| `games`, `load game= seed=` | list games; start one fresh |
| `status` | tick, time, hash, entity counts by kind, player, your `status()`, and `reach` (how high and far the player can jump) |
| `step ticks=` | advance with no input; returns the player and the events (as short lines) |
| `input move=[x,z] toward=[x,y,z] hold=a,b press=a aim=[x,y,z] ticks=` | drive the player (`toward` walks to a point and stops; a jump needs `press=jump`, `hold=jump` keeps it rising) |
| `capture out= width= height= marks=true at= tilt= yaw= distance= ortho= ssaa=` | screenshot PNG; `marks=true` numbers entities (not level blocks or `body: none` decorations) and returns a legend; `marks=key,slime` only those kinds |
| `filmstrip frames= every= columns= width= height=` (+ input args) | frames over time in one PNG: motion and animation |
| `ascii radius= cell= at= plane=xy` | text map: walls, floor, pits, entities as letters (the centre snaps to the cell grid) |
| `entities kind= near=[..] radius= limit= blocks=`, `entity id=` | list / inspect |
| `params prefix=`, `set path= value=` | read / tune any parameter live |
| `spawn kind= ...`, `despawn id=`, `teleport id= pos=` | edit the world |
| `rewind ticks=`, `snapshot name=`, `restore name=` | time travel (up to 60 s; history restarts at load and restore), try alternatives |
| `record path=`, `replay path=` | save the inputs since start; re-run them and compare the hash |
| `autoplay seconds= until=won trace=30` | your `bot` plays (stops early when `status().won` is true); `trace=N` logs position, velocity and input every N ticks |
| `autoplay seeds=1-20 seconds=60 until=won` | a fresh game per seed; one status row each: the quickest way to find rare bugs |
| `bench ticks= frames=` | ticks per second and render time |
| `level_check path=` / `text=` | validate a level file |

Tips: look at captures with your image viewer (`width=640 height=360` is plenty). Use the REPL
for multi-step experiments (one process, state kept). `capture marks=true` lets you say
"mark 3" and know it's the goblin with 2 hp. If the MCP server is running while you change Rust
code, restart it (the CLI rebuilds by itself). `record` warns if tools edited the world
(replays hold only inputs).

## Testing

- `cargo test` runs engine tests and every game's tests (`src/games/mod.rs` checks that every
  registered game runs, draws, rewinds and replays exactly).
- Give your game a `bot` and a test that it WINS (reaches the goal, clears the level), not
  just "makes progress" (see `template.rs`). If the bot can't win, the level is probably too
  hard (check `reach` in `status`) or the bot too simple (see `platformer.rs`: it waits for
  the lift and brakes in mid-air when a jump would land in a pit). Fix that; don't weaken
  the test. Bots are also how you balance: `autoplay` after every change.
- Test more than one seed: `autoplay seeds=1-30 until=won` (and a loop over seeds in your
  test). Bugs that show up one run in twenty are common in bots and AI.
- When a bot stalls, `autoplay trace=30` shows where and with what input; `entity id=N` shows
  an NPC's current input.
- `cargo clippy --all-targets` and `cargo fmt` keep the code tidy.

## Playing

`cargo run --release -- play <game> [seed=N] [size=1280x720] [scale=2]`. WASD/arrows move,
Space jump, J or left mouse fire, K or right mouse alt, E use, Shift dash, C crouch, mouse aims.
Esc quit, F1 help, F5 restart, P pause, F7 step, hold Backspace rewind, F12 screenshot,
Tab stats. `scale=2` renders at half resolution (faster). On Linux the window needs a desktop
session (X11 with libxkbcommon-x11, or Wayland); everything else runs headless.

## Limits (by design)

No sound, gamepads, textures, model files or networking. The renderer is a CPU rasteriser:
opaque shapes only (HUD rectangles can be translucent), shadows near the camera target, a few
thousand boxes and a few hundred puppets stay fast. Physics is rapier3d (dynamic props are
boxes, spheres, capsules, cylinders; characters are kinematic capsules). Repeatability holds on
the same machine and build.
