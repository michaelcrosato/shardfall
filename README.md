# Shardfall

A procedural hack-and-slash adventure built in Rust and WebGPU, with a playable Pavilion
tech demo. Explore Emberwatch and the endless Depths, or visit the pavilion's rooms to try
movement, physics, procedural animation, visual effects, rendering styles, and genre demos.
Everything is generated from code, every parameter can be tuned live, and agents can drive
the game through the same tools a person uses.

Play [Shardfall](https://shardfall-eight.vercel.app), or open the
[Pavilion tech demo](https://shardfall-eight.vercel.app/?scene=world). Use recent Chrome
or Edge with WebGPU and hardware acceleration enabled. Phones and tablets play by touch (Chrome
on Android, Safari on iOS 26 or later): drag anywhere to move, attacks are automatic, and the
buttons in the corner dodge, cast and drink.

- Compact engine for AI agents: [`pavilion-lite/`](pavilion-lite/) (one Cargo package, CPU
  renderer, agent tools, sample games; `pavilion-lite/package.sh` makes the handoff archive).
  Frozen: a finished experiment, outside the doctrine because its renderer isn't WebGPU.
- Linux agent handoff: [`START_HERE.md`](START_HERE.md). Run `scripts/package-agent.sh`
  to create a standalone source archive and checksum in `out/agent/`.
- Design: [`docs/DESIGN.md`](docs/DESIGN.md). Current state: [`docs/PROGRESS.md`](docs/PROGRESS.md);
  history and decisions: [`docs/HISTORY.md`](docs/HISTORY.md). Principles:
  [`docs/DOCTRINE.md`](docs/DOCTRINE.md). Rules for agents working here: [`AGENTS.md`](AGENTS.md).

## The adventure
A fast hack-and-slash built on the engine ([`docs/GAME.md`](docs/GAME.md)). `shardfall.exe`
starts in **Emberwatch**, the town: trade with Hilda the smith, gamble with Odo, buy stronger
potions from Mother Wren, keep loot in the stash, and take the portal down. Twelve designed
levels each introduce one mechanic (shrines, powder kegs, spike plates, rift gates, wind,
totems, lava, ice, crumbling floors, darkness, cursed chests, time bubbles) and mix in the
ones before; after level 12 the Depths generate new combinations forever. The portal also
leads to the Proving Grounds (wave arena) and the Menagerie (generated creatures). Your hero
is saved next to the executable on every trip and on quit. The pavilion of engine rooms is
still there: Esc menu → Load scene → `world` (or `shardfall.exe --scene world`).

| Key | Action |
|---|---|
| W A S D · mouse | move · aim |
| Left / right mouse, Q E R F | skills (hold to repeat) · Shift: attack in place |
| Space | dodge roll |
| 1 | potion |
| G | use: trade, stash, portal, gamble, brew, the way down, cursed chests |
| I (Tab) · P · C · K · M | inventory · passive tree · character · skill bar · map |
| T | town portal |
| Esc | menu (difficulty sliders for play-testing, new hero, Look & filters) |

Items drop as you fight (walk over magic+ items or click their names); hover an item to compare
it with what you wear. Gamepad: X Y B RB LB RT skills, A dodge, D-pad up potion, D-pad right use,
D-pad left map, D-pad down the hero's panels (LB / RB switch between inventory, character,
skills and passive tree). With a window open, the stick moves a cursor: A clicks, X
right-clicks, hold Y for shift (take a whole path in the tree), the right stick scrolls and
zooms, B closes. Start opens the menu, which works the same way.

## Playing
Windows: run `shardfall.exe`. It writes `shardfall.toml` next to itself on first start (Vulkan by
default; `backend = "dx12"` switches). `shardfall.exe --room drift` starts in a room of the
pavilion (`--scene world` for the pavilion itself).
Browser: serve the web build (below) and open it in a recent Chrome or Edge; add `?room=drift`
to the address to start in a room.

| Key | Action |
|---|---|
| W A S D | move (Shift: walk slowly) |
| Space | jump (hold for higher) |
| C / Ctrl | crouch (hold) · dodge roll while running |
| Z | crawl (toggle) |
| F / left click | throw a bomb, or fire the blaster in shooting rooms |
| E | get in / out of a vehicle |
| Backspace (hold) | rewind time |
| Right-drag · wheel · 1-8 | rotate · zoom · camera presets |
| Esc | menu (Look & filters: pixel art, cel shading, outlines, oil paint, halftone, ASCII, pencil, palettes, grading, haze... on the whole scene, the characters & objects or the environment) |
| H | how the room works (the station guide); outside rooms the field guide |
| F1 | tuning panel (every parameter, presets) |
| F2 | jump to any room |
| F4 / F5 | leave the room / reset it |
| F3 · F12 | boot diagnostics · screenshot |

Gamepads work too. Each room shows an info card on entry (what it demonstrates, what to try) and
its own controls. Every room is also a lesson: H (or the card's **How it works**) opens its
station guide with what you are seeing, how the engine does it step by step, live sliders for
the settings involved, where games use it, phrases to ask for it, what it costs, the engine's
own code and the words to know; stepping on a pad shows what that pad changed and why. The
**Field guide** (Esc menu) collects every word and every room's "ask for it" phrases.

## Rooms
| Wing (corridor) | Rooms |
|---|---|
| Movement & Feel Lab (east) | Playground, Feel Lab, Tightrope, Slalom, Timing Gates, Dodge Gauntlet, Camera Bench, Verticality Tower |
| Physics Lab (north) | Stacking & Toppling, Springs & Soft Bodies, Chains & Rope Bridges, Conveyors, Bounce & Friction Gallery, Destructible Floors, Stress Test |
| Animation Lab (west) | Walk Cycles, Jointed Limbs, Impact & Recoil, Squash & Stretch, Secondary Motion, Character Style Bench |
| Visual Effects (south) | Lights & Shadows, Particle Garden, Bloom & Glow, Heat & Shockwaves, Global Illumination, Light Shafts, Wind & Water |
| Styles & Filters (east) | Pure Styles, Style vs Filters, Filter Stack Bench, Mix & Match, Paint & Print, Screen Transitions |
| Genre Wing (north) | Bullet Hell, Grid Stealth, Drift Circuit, Helicopter Run |
| Workshop (south) | Sandbox (spawn, drag, delete and save objects) |

Rooms are TOML files in [`rooms/`](rooms/) ([`_template.toml`](rooms/_template.toml) documents
every field). They are built into the executable and hot-reload from disk while the game runs.

## Building
```sh
scripts/setup-linux.sh        # once: packages, lavapipe (software Vulkan), mingw, Wine
cargo run -p pav_app          # the game on Linux
cargo test                    # tests
scripts/build-windows.sh      # Windows .exe (cross-compiled)
scripts/build-web.sh          # browser build -> target/web (WebAssembly + WebGPU)
python3 -m http.server -d target/web 8000   # then open http://localhost:8000
```

## Deploying to Vercel

Import [`michaelcrosato/shardfall`](https://github.com/michaelcrosato/shardfall) into Vercel.
The root `vercel.json` selects the **Other** framework, installs the pinned Rust and
wasm-bindgen tools through `scripts/build-vercel.sh`, and serves `target/web/` as a
static WebAssembly game. No environment variables or backend services are needed.

With an authenticated Vercel CLI, deploy from the repository root:

```sh
vercel link --project shardfall --yes
vercel deploy --prod --yes
```

To build and try the same output locally:

```sh
scripts/build-vercel.sh
python3 -m http.server -d target/web 8000
```

Use recent Chrome or Edge with WebGPU and hardware acceleration enabled. Open `/?scene=world`
for the tech demo or `/?room=drift` for a particular room. Saves stay in each browser's local
storage; visiting a different site address uses a separate save.

## Agent tools
`pav` runs the engine headless with one tool registry (load rooms, step, drive the player, tune
parameters, rewind, record and replay, capture screenshots, filmstrips and audio, benchmark):
```sh
cargo run -q -p pav_tools --bin pav -- help
printf 'load scene=world\ngoto room=drift\ncapture out=out/drift.png\n' | pav repl
```
The same tools are an MCP server (`pav mcp`, registered in `.mcp.json`), and a live bridge into
the running game: start it with `--bridge`, then use `pav live` (REPL) or `pav mcp --live`.

For building game content there are tools that make and judge it: grow a creature from a seed
(`genome`), author one as JSON and see it from every side with its measurements (`turntable
def={...}`), roll loot tables (`loot_roll`), map any level with its mechanics and what the AI
thinks is walkable (`levelmap nav=true`), describe any depth (`level depth=40`), take a
screenshot with numbered marks and a legend of what each mark is (`see`), and let a bot play
the whole descent and report every level (`campaign`):
```sh
pav levelmap depth=7 nav=true out=out/level7.png
printf 'load scene=town\ncampaign from=1 to=12 wall=300\n' | pav repl
```
