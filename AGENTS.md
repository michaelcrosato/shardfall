# AGENTS.md — rulebook for coding agents

Making a new game on the engine? Read `docs/MAKE_A_GAME.md`: the "Key rules" below apply to
your game, the maintainers' workflow (HISTORY, PROGRESS, PRs, escalations) doesn't.

Shardfall is a procedural hack-and-slash game built on the Pavilion engine, written in Rust. The
principles are in `docs/DOCTRINE.md`, which outranks everything else here. The current state is
`docs/PROGRESS.md` (one page). Read both before starting; Claude Code loads them, with this file,
through `CLAUDE.md`. The design is in `docs/DESIGN.md` (engine) and `docs/GAME.md` (Shardfall):
read them when your work touches the design. `docs/REFERENCE.md` holds the full code map, every
tool family with examples, the controls and what each test suite covers: open the section you
need. The journal and past decisions are in `docs/HISTORY.md`, newest first: search it, don't
read it whole. For a standalone Linux source handoff, `START_HERE.md` covers setup and headless
CLI examples.

## How the doctrine applies here
Adopted 2026-10-09, partway through development. Code from before then isn't rewritten to fit;
fix a mismatch when it gets in the way.
- **Platform (5, 10):** the development platform is Linux, where agents build and test. The
  user's Windows `.exe` and the browser build count as part of the target too: a problem the user
  can see gets fixed now. Replays are promised to match only on the machine that recorded them; to
  chase something the user saw, start from a snapshot taken just before it.
- **Renderer (6):** wgpu is the WebGPU implementation: Vulkan on Linux and Windows (DX12 as a
  Windows option), the browser's own WebGPU on the web, lavapipe where there's no GPU. GPU compute
  stays cosmetic. The game UI is egui.
- **API and internals (7, 8):** the game (`pav_core/src/arpg/`) still lives inside the engine
  crate, so the compiler doesn't enforce the line. The engine modules the game uses are its API:
  `sim`, `entity`, `frame`, `rng`, `input`, `params`, `statics`, `nav`, `zones`, `character`,
  `projectile`, `puppet`, `parts`, `moves`, `clips`, `shape`, `color`, `fxdef`, and `games` (the
  hook a new game implements: `docs/MAKE_A_GAME.md`). Everything else
  (renderer, physics wiring, synth, importers) is internals. If game work means reading
  internals, fix the API or the error message.
- **Versions (9):** current pins stay (Rust 1.98.1, wgpu 30, rapier 0.36, egui 0.36, glam 0.33,
  cpal 0.18). The rule applies to new dependencies and to upgrades.
- **Assets (4):** no image, model, texture or audio files; fonts are allowed. Animation and
  objects are readable text: key poses (`anim/*.json`, the format `clips.rs` documents, made with
  `clip_import`) and props made of named primitive parts (`assets/props`, `docs/ASSET_STUDIO.md`).
  Raw downloads (.glb, .gltf, .obj, .fbx, .bvh, .amc) go to `.cache/` (git-ignored); never commit
  them, or images. Keep each source's credit and licence. Libraries whose licences forbid sharing
  what is made from them (Bandai Namco, LaFAN1, Mixamo) are translated only into the git-ignored
  local tiers (`anim/local`, `assets/props/local`).
- **Escalation:** ask in chat and log it in `docs/ESCALATIONS.md`. If you can schedule a check-in
  (e.g. `send_later`), set one for 15 minutes and carry on with other work; if not, make the call,
  log it and continue.
- **Pavilion Lite** (`pavilion-lite/`) is frozen: its renderer runs on the CPU, not WebGPU (6),
  so it isn't developed further (`docs/ESCALATIONS.md`).

## Working rules
Under the doctrine, in priority order:
1. **Gameplay first, then robustness.**
2. **Use tools.** If a tool does a job faster, more accurately or cheaper over time, use or build one.
3. **It must still run well on a modern PC** (the user's RTX 3060 Ti and 4070 Super). Other
   optimization and compatibility wait (10).
4. **Approximations are fine if they feel right.** Feel beats physical accuracy.
5. **Check selectively.** Verify what often breaks or is expensive to debug; skip routine checks
   of things that almost never break. "If it's broken, the user will say so" is valid.
6. **Cost over speed.** Prefer the CLI/capture tools over reading lots of code.

## Testing budget (prototype stage)
Every engine here is a prototype with one user: spend on finding what clearly helps, not on
measuring exactly how much. Completion comes first: a test decides how something is built and is
recorded; it never stops, skips or reverts planned work.
- **Test only to change a decision.** Before a test, name the choice it decides and its cost in
  US$ as Claude Code bills it, subagents included. If no choice rides on it, or a wrong choice
  would cost less than the test, skip it. Afterwards, report what it cost.
- **Size the test to the gap.** Agent runs (benchmarks, judges, eval agents) start small: 3-5 per
  arm. A clear gap decides it: 2:1 or better in wins, or 25% or more in cost, model calls or
  errors.
- **Near 50/50, make the call.** If the first batch is within 60/40 (or 10%), stop: the options
  are about equally good, so the choice matters little (Fredkin's paradox). Take the simpler,
  cheaper or more reversible option, log the call and why, and move on. Between clear and close:
  at most one more small batch, then decide.
- **Close calls belong to tool tests.** Checks with no model calls (cargo tests, `pav` checks and
  captures, scripted counts) cost almost nothing per run and repeat exactly, so they may scale up
  to settle narrow margins. Even then, don't chase gains under 5%; timing differences under 10%
  are noise.
- Looks and feel are the expensive, noisy kind: prefer objective proxies; use agent judges only for
  large, obvious differences, few of them, several items per call; leave fine visual judgement to
  the user on a quick comparison page.
- Explore broadly: test distinct hypotheses, not variations of one. Don't add or keep code without
  a verified benefit.
- Count cost end to end, in US$, not one tool's traffic: every model call re-reads the agent's
  whole context.
- Assets made through the documented loop (a brief, `check` to zero findings, a 4-view sheet at
  the game camera, at most two fix rounds, `publish` with a review) need no human review: the user
  accepted 28 of 28 (2026-10-12). Show the user only edge cases, or complex ones you are unsure of.
- Per PR (from 2026-10-16):
  - Run the full checks (tests of the touched crates, clippy, fmt, the wasm check when the app
    builds the change) once, on the head you merge.
  - After a rebase or a small fix, run only the tests of what changed. A docs-only PR needs none.
  - Merge as soon as the checks pass; don't wait for a review (Codex has posted none since #45 on
    2026-10-12). A review that arrives later: fix its real findings in a follow-up PR.
  - A push never deploys to Vercel (`vercel.json`), so a PR has no Vercel status to wait on.
    Deploy only when the user asks, the one way `CLAUDE.md` ("Deploying to Vercel") gives.

## Architecture
Crates (the full map, module by module: `docs/REFERENCE.md`, "Code map"):
- `pav_core`: the simulation, with no window, GPU or audio dependencies: `Sim`, entities, physics
  (rapier3d), the world, rooms and terrain, characters and puppets, moves and clips, AI and nav,
  props and materials; the game lives in `src/arpg/`.
- `pav_render`: the wgpu renderer: shadows, bloom and post effects, the filter stack, procedural
  meshes and materials, offscreen capture to PNG.
- `pav_view`: sim frame -> render scene: the camera rig, looks and filters, water, `emit_asset`.
- `pav_audio`: synthesized sound: cpal output, offline .wav rendering.
- `pav_tools`: the agent layer: the tool registry, the `pav` CLI and REPL, the MCP server, the
  live bridge client, and the motion and asset importers.
- `pav_ui`: Shardfall's egui UI (HUD, inventory, passive tree, touch controls), drawn by the
  window and, headless, by the tools (`ui`, captures).
- `pav_app`: the game (`shardfall` binary): window, input, sim thread, live bridge server, the
  pavilion's egui UI; native and wasm32.

Data: `anim/` (moves and motion clips), `rooms/` (room files, embedded in the exe and hot-reloaded;
`_template.toml` documents every field), `game/` (Shardfall's data), `assets/` (props, materials,
model catalogs), `store/` (the asset store's tables).
Data flow: `Sim::step(InputFrame)` (fixed tick, own thread) -> `Sim::frame()` -> `RenderFrame`
-> `pav_view::ViewBuilder::build` (interpolates prev/curr) -> `pav_render::Scene` -> `Renderer`.

Key rules:
- `pav_core` must never depend on graphics, windowing or audio crates.
- Everything that changes during play lives in `SimState` (cloning it = snapshot). Keep it `Clone`.
- Same inputs and seed, same state (doctrine 5): randomness comes from `rng`, seeded (the sim's
  `Rng`, or its hash for seed-derived content), never from the clock; don't let
  `HashMap`/`HashSet` iteration order reach game state (use a `BTreeMap` or sort).
- Gameplay code reads plain struct fields; expose tunables via `impl Tunable` (see `params.rs`).
  Tunables are automatically in the tuning panel, the `params`/`set` tools and preset files.
- Colors in data are sRGB hex (`Color::hex("#e8704a")`); internally linear.
- Physics precision switch: in `crates/pav_core/Cargo.toml` change `package = "rapier3d"` to
  `"rapier3d-f64"`.

## Build, run, test
```sh
scripts/setup-linux.sh             # once per machine (apt packages, lavapipe, mingw, wine)
cargo build                        # everything (Linux)
cargo test                         # every crate's tests (`-p pav_core` for one crate)
cargo fmt --all --check            # formatting
cargo clippy -p pav_core --all-targets -- -D warnings  # lints, for each crate you touched
cargo run -p pav_app               # the game (needs a display; Xvfb works with lavapipe)
scripts/build-windows.sh           # -> target/x86_64-pc-windows-gnu/dist/shardfall.exe
scripts/smoke-windows.sh 25        # run the .exe under Wine+lavapipe, screenshot + log in out/smoke/
scripts/smoke-linux.sh 10          # same for the native Linux build
scripts/build-web.sh               # browser build -> target/web/ (serve over HTTP; Chrome/Edge)
scripts/build-web.sh --single-file # -> target/web/shardfall.html: one ~10 MB page, runs from file://
scripts/check-wgsl.sh              # after any WGSL change: Chromium's Tint compiles every module
```
`--single-file` packs web/index.html, the JS glue and the gzipped wasm into one page with no
other loads (`scripts/pack_single_html.py`; `--title --description --theme-color --scene NAME --out PATH`).
Browser build: `cfg(target_arch = "wasm32")` code lives in `pav_app` (no threads, async GPU
setup); other crates only skip disk access there (the browser has no files) or pick its GPU
backend. Check it with `cargo clippy -p pav_app --target wasm32-unknown-unknown`. Headless
Chromium runs it only with the right flags (WebGPU on SwiftShader); those and phone emulation are
in `docs/REFERENCE.md`, "Browser and phones".
Disk: cloud containers have a fixed allowance. Dev builds use line-table debug info and no
incremental cache; if the disk fills up anyway, delete stale binaries in `target/debug/deps`
(or run `cargo clean`).

## Agent CLI (`pav`) and MCP
Every tool works headless (captures use lavapipe when there is no GPU). The same tools are an
MCP server: `.mcp.json` registers `pav mcp` (stdio), so Claude Code agents in this repo get
them as `mcp__shardfall__*` tools (captures come back as images). `pav help` lists every tool
and its arguments; `docs/REFERENCE.md` ("Tools") shows each family with examples.
```sh
cargo run -q -p pav_tools --bin pav -- help
pav bench ticks=1200                          # ticks/sec
pav capture scene=test ticks=300 out=out/a.png
printf 'step ticks=200\ncamera preset=top\ncapture out=out/b.png\n' | pav repl
printf 'input move=[1,0] ticks=30\ninput press=jump move=[0,1] ticks=40\nplayer\n' | pav repl
```
- One-shot calls accept `scene=`, `seed=` and `ticks=` to set up the session first. `pav repl`
  keeps one session across lines (`tool key=value ...` or JSON lines
  `{"tool":"name","args":{...}}`; `#` = comment).
- `input` drives the player: `move=[x,z]` world direction (x = east, z = south), `hold=`/`press=`
  buttons (jump, crouch, crawl, use, focus, interact, primary), `aim=[x,y,z]`, `ticks=N`.
- Look at captures with your image-reading tool; prefer small sizes (e.g. `width=640 height=360`).
  `filmstrip` tiles N frames into one PNG: the cheapest way to check motion and animation.
- `record_save` + `replay` reproduce a session exactly (same machine/build) and check the hash:
  use them for bug reports.
- Live bridge: start the game with `--bridge` (or `bridge = "127.0.0.1:7878"` in shardfall.toml),
  then `pav live` is a REPL into the running game and `pav mcp --live` an MCP server for it. Same
  tools; they act on what is on screen (captures render the game's camera on a second device).
- Studios edit on the live bridge, each with its own doc: Animation (`shardfall
  --animation-studio`, `.mcp.animation-studio.json`, `docs/ANIMATION_STUDIO.md`), Object
  (`--asset-studio`, `.mcp.studio.json`, `docs/ASSET_STUDIO.md` and `docs/ASSET_STUDIO_LIVE.md`;
  materials `docs/MATERIALS.md`) and Creature (`--creature-studio`, `.mcp.studio.json`,
  `docs/CREATURE_STUDIO.md`); `scripts/animation-studio.sh`, `asset-studio.sh` and
  `creature-studio.sh` build and open each. Pass the last reply's `revision` as `if_revision`,
  and copy a source clip or asset before changing it.
- Game data and rooms may name only assets every build carries: embedded (BUILTIN, LIB/core) or a
  shipped pack's (`props::store::SHIPPED`: the KayKit figures). Game code draws library assets
  from its own state with `pav_view::assets::emit_asset` (`docs/ASSET_STUDIO.md`, "Library
  assets in game code").
- `tools/creature-compiler` vendors the exact SpawnForge pin: keep the pin and source hashes
  intact; don't resolve against a moving upstream checkout.
- Shardfall content: game data lives in `game/*.toml` (`game_reload` re-reads it live). Check a
  change with the tools: `see` (a screenshot with numbered marks and a legend), `levelmap`,
  `turntable`, `animsheet`, and `campaign from=1 to=12 wall=300` (the bot plays down through the
  levels and reports each; run it after data changes).

Asset store (`docs/AGENT_ASSET_STORE.md`): before making a prop or clip, `assets shelf=prop` (or `shelf=clip`) lists the best entries and `assets find=words` (`kind=clip` for motion) searches; a miss gives `none` and a route.
In the Workshop wing, `library_hall` shows every standard object (generated: after publishing to core, rebuild and run `pav assets reindex=store` and `pav assets reindex=hall`), `limits` what the checks catch and miss, `catalogue` pages through every store row, `store_showcase` shows the materials; the animation wing's `clip_browser` plays every clip.

Tools: `scenes load step status entities npcs params set camera capture bench gpu player input
spawn despawn teleport rewind snapshot_save snapshot_load record_save replay rooms room goto
room_reset room_check room_reload stream signal filmstrip camera_bench course feel audio_capture`;
looks `look`; teaching `guide`; games on the hook `play`; Shardfall `game hero monster autoplay skills game_reload
loot_roll give inventory game_cmd ui tree tree_map genome bestiary boss turntable animsheet level
levelmap go goto_feature see campaign theme_swatch`; animation `clips clip_import mocap
anim_reload anim_edit anim_preview`; objects `assets asset_edit asset_preview asset_spawn
asset_import studio_status`; creatures `creature_catalog creature_edit creature_preview
creature_status`.

### Adding a tool
1. Write `fn t_name(s: &mut Session, a: &Args) -> Result<Output>` in the tool family's module
   (`asset_tools.rs`, `game_tools.rs`, ...); it rejects unknown arguments, naming the nearest.
2. Add a `Tool { .. }` entry to `TOOLS` in `tools.rs` (help ≤ 50 tokens, each argument ≤ 12
   words). It is then in the CLI, REPL and MCP.
3. Put enums and op shapes in `schema()`; a new asset kind is a `KIND_ROUTES` row, not a new tool.
4. Add actions that change what the live studio shows to `live_feedback::changes_studio`.
5. Add a clause to the `mcp.rs` instructions if agents should find the tool first.
6. Test it on `Session::new("empty", 1)`.

### Adding a room (preferred: data only, no rebuild)
Copy `rooms/_template.toml` to `rooms/<key>.toml` and edit; the template documents every field
(`rooms/feel_lab.toml` and `crates/pav_core/tests/physics_room.toml` are full examples). The room
is placed on its wing's corridor automatically. Every room is a station of the world demo and
teaches: give it a `[learn]` block and a `note` on every pad that changes something
(`pav_view/tests/learn.rs` checks them; `rooms/bloom.toml` is the model). Check it with
`pav room_check path=rooms/<key>.toml`; look at it with `pav capture scene=<key>` (the room alone,
fast) or `printf 'load scene=world\ngoto room=<key>\ncapture\n' | pav repl`. The running game
hot-reloads room files. New *mechanics* are Rust (character.rs / sim.rs behaviours / entity
fields); keep rooms as data wherever possible.

### Scenes
`world` (pavilion + rooms + streaming wilderness), `world/<room>` (start in a room),
`<room key>` (that room alone, no terrain), `test`, `empty`, and Shardfall's `town` (the game
starts here by default; the saved hero is loaded into any game scene), `arena`, `lab` (the
Menagerie of creatures) and `level/<n>` (depth n: 1-12 designed, then the endless Depths).
Games built on the hook (`pav_core::games::GAMES`) are scenes by their key: `starter` today.
Code scenes live in `scenes.rs` (Shardfall places in `arpg/scene.rs`).

## Tests
`cargo test` runs every suite; `cargo test -p <crate> --test <suite>` runs one. Suites live in
`crates/*/tests/`, named for what they cover (`docs/REFERENCE.md`, "Tests", describes each).
`pav_core/tests/gameplay.rs` and `movement.rs` are the cheapest way to catch feel regressions:
extend them when you add movement features. `pav_view/tests/material_parity.rs` needs a GPU (or
lavapipe); `pav_tools/tests/import_khronos.rs` is opt-in (`-- --ignored`).

## Conventions
- Rust stable pinned in `rust-toolchain.toml`; `Cargo.lock` committed; edition 2024.
- Finished work gets a dated entry at the top of `docs/HISTORY.md` (what changed, how it works,
  the decisions); then update `docs/PROGRESS.md`, which stays one page of current state.
- Decide anything that isn't genuinely the user's call; record the decision with its entry in
  `docs/HISTORY.md`. Escalate what the doctrine says to escalate, and log it in
  `docs/ESCALATIONS.md`.
- Finishing a task or milestone: commit, push, open a PR into `main`, merge it and delete the
  branch. This is standard procedure (the user's standing instruction, which counts as asking for
  the PR); don't ask first.
- The user plays the Windows build; the log file `shardfall.log` and `shardfall.toml` (startup
  settings, e.g. `backend = "dx12"`) sit next to the `.exe`.
- Keep this file short: under 32 KiB (Codex reads no further; `docs_limits.rs` checks it) and
  about 250 lines. Detail goes to `docs/REFERENCE.md` or a topic doc, with a pointer here.

## Code Review Rules
For reviewers (Codex when it runs, or `/code-review`). Flag a change that:
- breaks one of the "Key rules" under Architecture: a graphics, windowing or audio dependency in
  `pav_core`, play state outside a `Clone` `SimState`, results that can differ between runs on one
  machine (unseeded randomness, the clock, `HashMap`/`HashSet` iteration order);
- commits a binary asset other than a font, or content a licence keeps in the local tiers;
- makes threads, blocking calls or file access reachable on `wasm32`.
Leave style, wording and test coverage of routine code alone: this is a one-user prototype
("Working rules" 5, "Testing budget").
