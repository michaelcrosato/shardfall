# Shardfall: Linux agent handoff

This is a standalone source snapshot. It includes the engine, game, rooms, game data,
agent CLI, MCP configuration, and documentation. It does not include Git history,
build caches, saved heroes, or prebuilt executables. No GitHub access is needed.

Give the receiving agent this instruction:

> Extract the archive, read START_HERE.md and AGENTS.md, then build the headless `pav`
> CLI. Use its simulation, capture, and content tools to inspect and work on the game.

Read `docs/DOCTRINE.md`, `AGENTS.md`, `docs/DESIGN.md`, and `docs/PROGRESS.md` before changing
the project.
`docs/GAME.md` describes Shardfall. The archive has no remote or checked-out branch;
initialize a local Git repository if you want to track your changes.

## Linux setup

Use a Linux machine with Rust installed through rustup. `rust-toolchain.toml` selects
Rust 1.98.1 automatically. The first build needs network access to download the
toolchain and dependencies; `Cargo.lock` pins the dependency versions.

On Debian / Ubuntu, install the native build and runtime dependencies below. If
already running as root, omit `sudo`. On a managed machine, ask its administrator
to install missing packages.

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libasound2-dev libudev-dev \
  mesa-vulkan-drivers vulkan-tools xvfb libxkbcommon-x11-0 libx11-xcb1 \
  libxcursor1 libxrandr2 libxi6
```

Simulation tools need no display or GPU. Screenshot tools use Vulkan and can use
Mesa lavapipe (software rendering) on a machine without a GPU. Keep builds on disk
and limit parallel jobs on machines with little RAM.

## Build and inspect the game headlessly

Run these commands from the extracted `shardfall` directory:

```sh
CARGO_BUILD_JOBS=4 cargo build --locked -p pav_tools --bin pav
./target/debug/pav help
./target/debug/pav bench scene=town ticks=600
./target/debug/pav capture scene=world ticks=60 width=640 height=360 out=out/pavilion.png
```

`world` is the tech demo pavilion; `town` is Shardfall's town; `arena` is the wave
arena; `lab` is the Menagerie; `level/1` starts the descent. A room key such as
`drift` loads that demo by itself. `pav repl` keeps a session across commands:

```sh
printf 'load scene=world\ngoto room=drift\ncapture width=640 height=360 out=out/drift.png\n' | ./target/debug/pav repl
./target/debug/pav levelmap depth=7 nav=true out=out/level7.png
./target/debug/pav campaign scene=town from=1 to=3 wall=60
cargo test --locked -p pav_core
```

Inspect the generated images with the agent's image viewer. `AGENTS.md` describes
the rest of the tools, including driving the player, authoring creatures, replaying
input, checking rooms, and reloading game data. `.mcp.json` exposes the same CLI as
the `shardfall` MCP server for clients that support this configuration.

## Run the game window

```sh
CARGO_BUILD_JOBS=4 cargo build --locked -p pav_app
./target/debug/shardfall --scene world   # tech demo
./target/debug/shardfall --scene town    # Shardfall
```

Without a graphical desktop, use `xvfb-run -a` around the game command, or use the
headless capture tools above. The demo's F2 menu jumps between rooms. Startup
settings and hero saves are written next to the game executable.

For Windows cross-compilation and Wine checks, the original `scripts/setup-linux.sh`
installs the additional tools and `scripts/build-windows.sh` builds the executables.

## Work on animations with a live preview

The same studio can build reusable objects from named parts. Start with
`scripts/asset-studio.sh`, connect `.mcp.studio.json`, and use `assets`, `asset_edit`,
`asset_preview`, and `asset_spawn`. The Objects and Animations tabs share the live bridge.
`docs/ASSET_STUDIO.md` describes batches, revisions, placement, and frame feedback.

Run `scripts/animation-studio.sh` to build the game and CLI, then open the animation workspace.
Use `.mcp.animation-studio.json` in your LLM client, or run `./target/debug/pav live` in a second terminal.
`anim_edit` creates and edits clips. `anim_preview` controls the visible playback.
Edits save automatically and update the running preview.

The original `.mcp.json` runs a separate headless session. Use the live configuration to control the visible window.
Read `docs/ANIMATION_STUDIO.md` for the complete workflow and a sample animation made from scratch.

## Make another handoff archive

From a Git checkout, run `scripts/package-agent.sh`. It packages the current
contents of tracked files, including local edits, plus this guide and the packaging
script. Add new source files to Git before packaging them. The archive and its
SHA-256 checksum are written to `out/agent/` by default.

## Creature Studio

The native studio also builds procedural creatures from the pinned SpawnForge compiler.
Install Node 22.18 or newer, then run `scripts/creature-studio.sh` from the repository root.
The launcher installs the exact npm lock, builds the self-contained compiler, builds both
Rust programs, and opens the Creatures view with the live bridge.

Use `.mcp.studio.json` for an LLM that controls the open window. `creature_edit` returns
an asynchronous job; use `creature_status` before the next dependent edit. Published builds
have a source revision and a frame ticket. Sources are readable JSON under
`assets/creatures/workshop`, and the last valid native mesh stays visible during a build.
Read `docs/CREATURE_STUDIO.md` for the complete workflow and the native material limits.
