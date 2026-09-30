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
              static blocks in 32 m chunks, params (Tunable), input frames, scenes
  pav_render  wgpu renderer (Vulkan/DX12): Scene description -> shadow pass -> MSAA scene
              pass -> composite (outlines, tonemap). Procedural meshes + analytic SDF
              spheres/capsules/rounded cones. Offscreen capture -> PNG.
  pav_view    sim frame -> render Scene: camera rig (tilt/yaw/distance/fov/ortho, all live),
              interpolation between ticks, visual settings (ViewSettings)
  pav_tools   agent layer: tool registry + `pav` CLI (one-shot and REPL) [+ MCP in M2]
  pav_app     the game (`pavilion` binary): window, boot diagnostics, egui, sim thread
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

## Agent CLI (`pav`)
Every tool works headless (captures use lavapipe when there is no GPU).
```sh
cargo run -q -p pav_tools --bin pav -- help
pav bench ticks=1200                          # ticks/sec
pav capture scene=test ticks=300 out=out/a.png
pav set path=camera.tilt value=90             # (one-shot: pointless alone; use the REPL)
printf 'step ticks=200\ncamera preset=top\ncapture out=out/b.png\n' | pav repl
```
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
Mouse wheel zoom · right-drag rotate camera · 1–8 camera presets · F3 boot diagnostics ·
F5 reset scene · F6 pause · F7 step · F8/F9 slower/faster · V vsync · I interpolation · F11 fullscreen.

## Conventions
- Rust stable pinned in `rust-toolchain.toml`; `Cargo.lock` committed; edition 2024.
- Keep `docs/PROGRESS.md` current enough to resume from after a context reset.
- Decide anything that isn't genuinely the user's call; record the decision in `docs/PROGRESS.md`.
- The user plays the Windows build; the log file `pavilion.log` and `pavilion.toml` (startup
  settings, e.g. `backend = "dx12"`) sit next to the `.exe`.
