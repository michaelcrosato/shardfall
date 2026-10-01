# Pavilion

A showcase game engine in Rust. You walk around a pavilion whose corridors lead to small rooms,
and each room demonstrates one thing: movement feel, physics, procedural animation, visual
effects, rendering styles and filters, and small genre games. Everything is generated from code
(no image, model or sound files), every parameter can be tuned live, and agents can drive it all
through the same tools a person uses.

- Design: [`docs/DESIGN.md`](docs/DESIGN.md). Current state and decisions:
  [`docs/PROGRESS.md`](docs/PROGRESS.md). Rules for agents working here: [`AGENTS.md`](AGENTS.md).

## Playing
Windows: run `pavilion.exe`. It writes `pavilion.toml` next to itself on first start (Vulkan by
default; `backend = "dx12"` switches). `pavilion.exe --room drift` starts in a room.
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
| Esc | menu |
| F1 | tuning panel (every parameter, presets) |
| F2 | jump to any room |
| F4 / F5 | leave the room / reset it |
| F3 · F12 | boot diagnostics · screenshot |

Gamepads work too. Each room shows an info card on entry (what it demonstrates, what to try) and
its own controls.

## Rooms
| Wing (corridor) | Rooms |
|---|---|
| Movement & Feel Lab (east) | Playground, Feel Lab, Tightrope, Slalom, Timing Gates, Dodge Gauntlet, Camera Bench, Verticality Tower |
| Physics Lab (north) | Stacking & Toppling, Springs & Soft Bodies, Chains & Rope Bridges, Conveyors, Bounce & Friction Gallery, Destructible Floors, Stress Test |
| Animation Lab (west) | Walk Cycles, Jointed Limbs, Impact & Recoil, Squash & Stretch, Secondary Motion, Character Style Bench |
| Visual Effects (south) | Lights & Shadows, Particle Garden, Bloom & Glow, Heat & Shockwaves, Global Illumination |
| Styles & Filters (east) | Pure Styles, Style vs Filters, Filter Stack Bench |
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

## Agent tools
`pav` runs the engine headless with one tool registry (load rooms, step, drive the player, tune
parameters, rewind, record and replay, capture screenshots, filmstrips and audio, benchmark):
```sh
cargo run -q -p pav_tools --bin pav -- help
printf 'load scene=world\ngoto room=drift\ncapture out=out/drift.png\n' | pav repl
```
The same tools are an MCP server (`pav mcp`, registered in `.mcp.json`), and a live bridge into
the running game: start it with `--bridge`, then use `pav live` (REPL) or `pav mcp --live`.
