# AGENTS.md — rulebook for coding agents

Pavilion is an AI-first game engine plus a living showcase, written in Rust. The design is in
`docs/DESIGN.md` (source of truth); progress and decisions are in `docs/PROGRESS.md`. Read both
before starting; update `docs/PROGRESS.md` when you finish something.

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
              static blocks + ladders in 32 m chunks, tile levels (level.rs), characters
              (character.rs: movement models, jump, crouch/crawl, ladders, bombs), puppet
              (puppet.rs: skeleton + procedural animation), history (rewind, replays), params
  pav_render  wgpu renderer (Vulkan/DX12): Scene description -> shadow pass -> MSAA scene
              pass -> composite (outlines, tonemap). Procedural meshes + analytic SDF
              spheres/capsules/rounded cones. Offscreen capture -> PNG.
  pav_view    sim frame -> render Scene: camera rig (tilt/yaw/distance/fov/ortho, all live),
              interpolation between ticks, visual settings (ViewSettings)
  pav_tools   agent layer: tool registry + `pav` CLI (one-shot, REPL) + MCP stdio server
  pav_app     the game (`pavilion` binary): window, boot diagnostics, input (keyboard/mouse,
              gamepad via gilrs), system keys, tuning panel, pause menu, sim thread
rooms/        room data files (TOML, ASCII tile layers + legend); built into the exe
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
- Physics precision switch: in `crates/pav_core/Cargo.toml` change `package = "rapier3d"` to
  `"rapier3d-f64"`.

## Build, run, test
```sh
scripts/setup-linux.sh             # once per machine (apt packages, lavapipe, mingw, wine)
cargo build                        # everything (Linux)
cargo test                         # unit tests
cargo run -p pav_app               # the game (needs a display; Xvfb works with lavapipe)
scripts/build-windows.sh           # -> target/x86_64-pc-windows-gnu/dist/pavilion.exe
scripts/smoke-windows.sh 25        # run the .exe under Wine+lavapipe, screenshot + log in out/smoke/
scripts/smoke-linux.sh 10          # same for the native Linux build
```
From WSL2 you can launch the Windows build directly: `./target/x86_64-pc-windows-gnu/dist/pavilion.exe`.

## Agent CLI (`pav`) and MCP
Every tool works headless (captures use lavapipe when there is no GPU). The same tools are an
MCP server: `.mcp.json` registers `pav mcp` (stdio), so Claude Code agents in this repo get
them as `mcp__pavilion__*` tools (captures come back as images).
```sh
cargo run -q -p pav_tools --bin pav -- help
pav bench ticks=1200                          # ticks/sec
pav capture scene=test ticks=300 out=out/a.png
pav set path=camera.tilt value=90             # (one-shot: pointless alone; use the REPL)
printf 'step ticks=200\ncamera preset=top\ncapture out=out/b.png\n' | pav repl
printf 'input move=[1,0] ticks=30\ninput press=jump move=[0,1] ticks=40\nplayer\n' | pav repl
```
Tools: `scenes load step status entities params set camera capture bench gpu player input spawn
despawn teleport rewind snapshot_save snapshot_load record_save replay` (`pav help` for args).
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

### Adding a scene
Add a builder to `crates/pav_core/src/scenes.rs` and list it in `SCENES`. (Rooms as data
files with hot reload arrive in M3; then prefer data files for rooms built from existing parts.)

## Game controls (current)
Keyboard+mouse: WASD move, Space jump (hold = higher), C/Ctrl crouch, Z crawl toggle, F or left
click throw bomb at the cursor, Shift walk slowly, walk into ladders to climb; right-drag rotate
camera, wheel zoom, 1–8 camera presets. Gamepad: left stick move, right stick aim, A jump,
B crouch, Y crawl, X/RT bomb, LT slow, LB/RB rotate camera, D-pad zoom, Start menu, Back rewind.

Fixed system layer (never rebinds): Esc pause menu · F1 tuning panel · F2 go-to menu · F3 boot
diagnostics · F4 leave room (M3) · F5 reset room · F6 pause · F7 step · F8/F9 slower/faster ·
hold Backspace rewind · F11 fullscreen · F12 screenshot (saved next to the exe).

## Tests
`cargo test` includes `crates/pav_core/tests/gameplay.rs`: walk, jump onto a crate, climb the
ladder, bomb the floor and drop through, crawl the tunnel, rewind repeatability, snapshot files.
Extend it when you add movement features; it is the cheapest way to catch feel regressions.

## Conventions
- Rust stable pinned in `rust-toolchain.toml`; `Cargo.lock` committed; edition 2024.
- Keep `docs/PROGRESS.md` current enough to resume from after a context reset.
- Decide anything that isn't genuinely the user's call; record the decision in `docs/PROGRESS.md`.
- The user plays the Windows build; the log file `pavilion.log` and `pavilion.toml` (startup
  settings, e.g. `backend = "dx12"`) sit next to the `.exe`.
