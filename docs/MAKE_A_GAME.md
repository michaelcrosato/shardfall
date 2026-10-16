# Make a game on Pavilion

For an agent asked to build a game on this engine. Read this page whole (about 3k tokens); it
is the only one you need to start. Search the others when a step points to them. The engine
was made for agents: everything runs, renders and checks headless, and every tool is a CLI
command, a REPL line and an MCP tool.

## What you get
- **A simulation** on the CPU at 60 ticks a second (`pav_core`): characters that walk, jump,
  climb and swim, rapier physics, pathfinding, projectiles, zones, procedural animation,
  props from a library. The same inputs and seed always play the same: snapshots, rewind
  and replays are exact.
- **A WebGPU renderer** (`pav_render`, `pav_view`): lighting, shadows, bloom, styles and
  filters (cel, pixel, painterly...), set per level as data.
- **Synthesized sound** (`pav_audio`): it plays the simulation's events, no audio files.
- **Tools** (`pav`): capture a frame, step time, drive the player, run your game's bot over
  many seeds, record and replay. Images come back as PNGs you can look at.
- **Three builds** from one source: a window on Linux and Windows, and the browser (also as
  one self-contained HTML file).

Shardfall, the hack-and-slash in `crates/pav_core/src/arpg/`, and the pavilion of demo rooms
are built on the same engine. They are examples, not starting points: start from the
**starter game**.

## Read this, skip that
- **Use:** this page; `crates/pav_core/src/games/` (the hook and the starter, about 600
  lines); `rooms/_template.toml` (the level format: search it for a field); `docs/REFERENCE.md`
  "Tools" (search for a tool); `pav help`.
- **`AGENTS.md`** is the rulebook for agents working on the engine and Shardfall. Its
  "Key rules" apply to your game too (below). Its workflow does not: `docs/HISTORY.md`,
  `docs/PROGRESS.md`, pull requests, escalations and the asset-store plans are the
  maintainers'. Follow your own task's instructions for commits and delivery.
- **Skip:** `docs/HISTORY.md`, `docs/ASSET_SYSTEM_PLAN.md`, `docs/AGENT_ASSET_STORE.md`,
  Shardfall's code and data (`arpg/`, `game/`), `pavilion-lite/` (an older, frozen engine).
  If you need to read engine source to use the engine, say so in your feedback (below).

## Quick start
```sh
cargo build -p pav_tools --bin pav                     # first build: a few minutes
alias pav=./target/debug/pav
pav play scene=starter bot=true seeds=10               # the starter's bot wins 10/10
pav capture scene=starter ticks=120 width=640 height=360 out=out/starter.png
printf 'load scene=starter\ninput move=[1,0] ticks=60\nplay\ncapture out=out/a.png\n' | pav repl
cargo test -p pav_core --test games                    # the starter's tests
cargo run -p pav_app -- --scene starter                # the window (WASD; needs a display)
```
`START_HERE.md` has the Linux packages to install. Look at captures with your image tool;
small sizes are cheaper. With MCP (`.mcp.json`), the same tools are `mcp__shardfall__*`.

**The edit loop.** Running from the repository's root, `pav` reads your level file at every
`load` (and `play seeds=`), so changes to the map and to its `[game]` numbers need no rebuild.
A change to Rust code rebuilds `pav` (about 2 minutes on 4 cores) and a test binary (as long
again): batch code edits, and put balance numbers in `[game]`.

## Your game in six steps
1. **Copy the starter.** `crates/pav_core/src/games/starter.rs` and `starter.toml` to
   `<key>.rs` and `<key>.toml` in the same folder. Rename the struct; set `const KEY` to your
   key (lowercase, no `/`); point `include_str!` at your level.
2. **Register it.** In `games/mod.rs`: `pub mod <key>;` and one line in `GAMES`:
   `entry::<mykey::MyGame>("One sentence about it.", mykey::build)`. Now `load scene=<key>`
   works in the tools, the window (`--scene <key>`) and the browser (`?scene=<key>`).
3. **Draw the level** in `<key>.toml` (next section), with your rules' numbers in its `[game]`
   table (the starter's `Rules`). `pav room_check path=<file>` checks it.
4. **Write the rules** in your `Game` impl (the hook, below).
5. **Teach the bot** every rule you add (`Game::bot`): it is how you, the tests and the next
   agent find out whether the game can be won.
6. **Copy the tests** in `crates/pav_core/tests/games.rs` to `tests/<key>.rs`: the bot wins on
   ten seeds, a careless player loses, and snapshots, rewind and replays are exact.

## The level: a room file
A level is a room file (`rooms/_template.toml` documents every field; the pavilion's
`rooms/*.toml` are working examples). Rooms built this way stand alone, with no wing or
corridor. In a room file:
- `[layout]`: an ASCII map per floor (`[[layout.layer]]`, 1 cell = 1 m; column = x east,
  row = z south) and a `legend` per character: floor and wall `blocks` (heights, colours,
  `destructible`, `crumble`, `ghost`), ladders, `prop`s, `label`s, `grass`, trigger `zone`s
  (`kill`, `water`, `checkpoint`, `conveyor`, `bounce`, `pad`, `camera`, course
  `start`/`gate`/`finish`) and `marker = "<name>"`.
- **Markers** are your own pieces: `crate::scenes::markers(sim, "shard")` lists every
  marked cell (world positions, map order). `marker = "player"` is the start.
- `[[npc]]`: characters (`kind = biped | spider | lizard | beetle | blob | quadruped`, `look` colours and
  proportions, `ai = idle | wander | patrol | circle | follow | guard`). Your game can drive
  them instead (the starter's slimes).
- `[[object]]`: library props (`asset = "LIB/core/<name>"`; `pav assets find=barrel` searches
  the store, `assets shelf=prop` lists the best), physics objects, `[[joint]]`s and `[[chain]]`s.
- `[camera]` (`tilt`, `distance`, `yaw`, `ortho`), `[view]` (light, sky, fog, bloom, styles,
  filters: `pav params prefix=view` lists them), `[params]` (movement and physics while in
  the level: `pav params`).

- `[game]`: your rules' numbers, free-form; your `build` reads them (`def.game`).
- `entrance` is required by the format; with a `player` marker a level doesn't use it.

Read the level with `games::level_text(KEY, include_str!(...))`: the file on disk when running
from the repository, else the copy built in (tests, the browser, an executable). The camera
follows the player; to see a whole level from above, `camera preset=top` then
`set path=camera.distance value=30` (raise it for bigger levels) and `capture`.

## The hook: `pav_core::games::Game`
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct MyGame { /* the rules' state: score, timers, entity ids, outcome */ }

impl Game for MyGame {
    const KEY: &'static str = "mygame";
    fn pre(&mut self, sim: &mut Sim, input: &InputFrame, dt: f32, events: &mut Vec<SimEvent>)
        -> BTreeMap<EntityId, InputFrame>;          // before movement: inputs for characters you drive
    fn post(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>); // after physics: rules
    fn hud(&self, sim: &Sim) -> Vec<String>;        // text lines, top left; `play` returns them
    fn status(&self, sim: &Sim) -> serde_json::Value; // your state for tools and tests
    fn outcome(&self) -> Outcome;                   // Playing | Won | Lost
    fn bot(&self, sim: &Sim) -> InputFrame;         // what a good player would press now
}
pub fn build(sim: &mut Sim, rest: Option<&str>) -> Result<()>; // the scene; sets sim.state.play
```
Every method has a default. `build` gets the rest of a scene name `mygame/<rest>` (a level
number, say). Each tick runs: behaviours, your `pre`, characters move (your inputs replace
their own), physics, shots move and hit, your `post`. While `pre`/`post` run, your game is out
of `sim.state.play`: use `self` for your state and `sim` for the world. In `post`, `events`
already holds this tick's events: read them to react (`Hit { target, .. }` names the
character a shot struck: it is stunned, knocked back and safe from shots for 0.6 s), and push
your own. From outside, `sim.play()?.get::<MyGame>()` reads your game; in a test,
`sim.state.play.as_mut().unwrap().get_mut::<MyGame>().unwrap()` changes it.

## Building blocks
| You need | Use |
|---|---|
| The player | `sim.state.player`; it walks by the keys' `InputFrame` (`move_dir`, `buttons::JUMP`, `PRIMARY`...); `sim.feet(id)` is where a character stands (`pos` is its centre) |
| Characters | `[[npc]]` in the level, or `sim.spawn_npc(name, feet, facing, PuppetDef::preset(BodyPlan::Blob), None, None)`; drive them from `pre` |
| Pickups, hazards, scenery | `sim.spawn(Spawn::new("name", pos).visual(Visual::new(shape, Color::hex("#7fe3ff"))).behavior(Behavior::Spin { speed }))`; `sim.despawn(id)`; `sim.set_position(id, feet)` |
| Moving platforms, emitters, spinners | `Behavior::Move`, `Rotate`, `Emitter`, `Spawner` (`entity.rs`), or `[[object]]` in the level |
| Shots | `sim.state.projectiles.spawn(Projectile { pos, vel, radius, life, color, knockback, gravity, owner, team, damage })`: plain structs, stopped by walls; `Team::Enemy` shots hit characters (a `Hit` event), `Team::Player` shots hit things with `health` |
| Pathfinding | `nav::NavGrid::for_room(sim)` (the level's walkable grid); `grid.path(from, to)`; `grid.flow(goal, reach)` steers many chasers at once; `grid.walkable(p)` |
| Randomness | `sim.state.rng` only (`range`, `below`, `f32`): seeded, so a seed always plays the same |
| Messages | `sim.say("text")`: centre top for a few seconds |
| Sound and effects | push `SimEvent`s from `pre`/`post`: `Coin`, `Pickup`, `Hit`, `Explosion`, `Shot`, `Jump`, `Land`... The renderer and the synth play them |
| Tuning | your numbers: the level's `[game]`; the engine's: `pav params`, `pav set path=movement.speed value=4.5` (movement, physics, camera, view), and a level's `[params]` set them while in it |
| Looks | the level's `[view]`; `pav look` lists looks and filters; `rooms/mix_match.toml` mixes styles per part |
| Animation | characters animate procedurally; motion clips: `pav assets shelf=clip`, then `clips = ["SET/Clip"]` on an `[[npc]]` |

## Rules that keep it working
- **Same inputs and seed, same game.** Random numbers come from `sim.state.rng`; never read the
  clock; never let `HashMap`/`HashSet` iteration order reach game state (`BTreeMap`, or sort).
- **All game state lives in your struct or in entities**, and your struct is `Clone` + serde
  (snapshots are clones; files and the replay hash are its CBOR). A cache that follows from
  the level (a nav grid) is `#[serde(skip)]` and rebuilt when missing.
- **`pav_core` has no window, GPU or audio code**, and game code must run in the browser: no
  threads, blocking calls or file access (embed data with `include_str!`).
- **Check with the tools, not by reading engine code.** If a tool or error message didn't tell
  you what you needed, that is engine feedback.

## Check your work (no screen needed)
| Question | Command (`pav <tool>` or a `pav repl` line) |
|---|---|
| Can it be won? Lost? | `play bot=true seeds=10` (wins, seconds, the state of every run not won) |
| What's on screen? | `capture width=640 height=360 out=out/a.png`; `filmstrip` for motion in one image; to watch the bot, repeat `play bot=true ticks=30` and `capture` in a `pav repl` |
| Does a rule fire? | `input move=[1,0] ticks=30`, then `play` (HUD, outcome, `status`) |
| Where is everything? | `entities`, `player`, `status` (with the state hash) |
| Same every time? | `record_save path=out/r.json`, then `replay path=out/r.json` (checks the hash); `rewind`, `snapshot_save`/`snapshot_load` |
| Is the level valid? | `room_check path=crates/pav_core/src/games/<key>.toml` |
| Does it run fast? | `bench scene=<key> ticks=1200` |
| Does it still pass? | `cargo test -p pav_core --test <key>`, `cargo clippy -p pav_core --all-targets -- -D warnings`, `cargo fmt --all` |

## Ship it
- **Window:** `cargo run -p pav_app -- --scene <key>` (Linux; `xvfb-run -a` without a display).
  F5 restarts the game (Esc opens the menu).
- **Windows `.exe`:** `scripts/build-windows.sh` (after `scripts/setup-linux.sh`).
- **Browser, one file:** `scripts/build-web.sh --single-file --scene <key> --title "My Game"
  --out out/mygame.html`: about 10 MB, runs from `file://` in Chrome or Edge (WebGPU).

## Limits today
- HUD: text lines only. Panels, bars or menus mean egui code in `pav_app` (`hud.rs`).
- Drawing from game state without an entity (`pav_view::assets::emit_asset`) and new sounds
  (`pav_audio`, a closed list) mean code outside `pav_core`.
- Your game compiles into the engine crate (`pav_core/src/games/`), beside Shardfall.
- The camera follows the player with the level's `[camera]` settings; there is no
  per-game camera code. Keep levels about 20 m across, or raise `[camera] distance`.
- The window's controls card lists the engine demo's keys (bombs, vehicles), not your game's.

## Report back
When you hand the game back, add a short **engine feedback** section, written for an engine
agent who has never seen your work: what you used, the engine source you had to read and
why, what you wrote that the engine should have had (with the file), wrong or missing docs,
and the one change that would have helped most. The maintainers log it in
`docs/FEEDBACK.md`, and what keeps coming back becomes engine work.
