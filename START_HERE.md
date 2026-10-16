# Shardfall and the Pavilion engine: start here

A Rust game engine built for AI agents (Pavilion), the hack-and-slash built on it (Shardfall)
and a pavilion of demo rooms. This page is for an agent receiving the source, from a clone of
https://github.com/michaelcrosato/shardfall or from a handoff archive (no Git history, build
caches, saved heroes or executables; `git init` if you want to track your changes).

## Which route
- **Make a new game on the engine:** read `docs/MAKE_A_GAME.md` whole. It is the only page
  you need to start; it begins from the starter game (`crates/pav_core/src/games/`).
- **Work on the engine or on Shardfall:** read `docs/DOCTRINE.md`, `AGENTS.md` and
  `docs/PROGRESS.md` (Claude Code loads them through `CLAUDE.md`); `docs/DESIGN.md` and
  `docs/GAME.md` when the work touches the design.

Prompts to give the receiving agent:

> Make a game: read START_HERE.md and docs/MAKE_A_GAME.md, install the Linux packages and
> build the `pav` CLI. Then make *(your idea, e.g. "a top-down maze where you collect five
> keys and escape while ghosts patrol")* on the engine, starting from the starter game. Give
> it a bot that wins and the tests from the guide, check it with the capture tools, tell me
> how to play it, and end with the engine feedback the guide asks for.

> Work on the engine: read START_HERE.md, then follow AGENTS.md. *(the task)*

Agents in auto modes may refuse to unpack an archive or run its scripts unless the prompt
says to; say so in the prompt when you hand over an archive.

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

## Build and inspect headlessly

```sh
CARGO_BUILD_JOBS=4 cargo build --locked -p pav_tools --bin pav
./target/debug/pav help
./target/debug/pav play scene=starter bot=true seeds=5
./target/debug/pav capture scene=world ticks=60 width=640 height=360 out=out/pavilion.png
```

`starter` is the starter game; `world` is the pavilion of demo rooms; `town` is Shardfall's
town; `arena` is the wave arena; `lab` is the Menagerie; `level/1` starts the descent. A room
key such as `drift` loads that demo by itself. `pav repl` keeps a session across commands:

```sh
printf 'load scene=world\ngoto room=drift\ncapture width=640 height=360 out=out/drift.png\n' | ./target/debug/pav repl
./target/debug/pav levelmap depth=7 nav=true out=out/level7.png
./target/debug/pav campaign scene=town from=1 to=3 wall=60
cargo test --locked -p pav_core
```

Inspect the generated images with the agent's image viewer. `docs/REFERENCE.md` describes
every tool. `.mcp.json` exposes the same CLI as the `shardfall` MCP server; run the `cargo
build` above before starting an MCP client, because a first build takes longer than clients
wait for a server.

## Run the game window

```sh
CARGO_BUILD_JOBS=4 cargo build --locked -p pav_app
./target/debug/shardfall --scene starter # the starter game
./target/debug/shardfall --scene world   # tech demo
./target/debug/shardfall --scene town    # Shardfall
```

Without a graphical desktop, use `xvfb-run -a` around the game command, or use the
headless capture tools above. The demo's F2 menu jumps between rooms. Startup
settings and hero saves are written next to the game executable.

For Windows cross-compilation and Wine checks, `scripts/setup-linux.sh` installs the
additional tools and `scripts/build-windows.sh` builds the executables.

## The studios (live editing in the window)
- **Objects:** `scripts/asset-studio.sh`, with `.mcp.studio.json` for the LLM: `assets`,
  `asset_edit`, `asset_preview`, `asset_spawn` (`docs/ASSET_STUDIO.md`,
  `docs/ASSET_STUDIO_LIVE.md`).
- **Animation:** `scripts/animation-studio.sh`, with `.mcp.animation-studio.json` (or `pav
  live` in a second terminal): `anim_edit`, `anim_preview` (`docs/ANIMATION_STUDIO.md`).
- **Creatures:** Node 22.18 or newer, then `scripts/creature-studio.sh`; `creature_edit`
  returns a job to follow with `creature_status` (`docs/CREATURE_STUDIO.md`).

Edits save to readable files and show in the window without a rebuild. `.mcp.json` runs a
separate headless session; the studio configurations control the visible window.

## Make another handoff archive
From a Git checkout, `scripts/package-agent.sh` packages the tracked files (with local edits)
into `out/agent/shardfall-linux-agent.tar.gz` and a SHA-256 checksum; add new files to Git
first. It leaves out `pavilion-lite/` (an older engine, frozen). `scripts/check-handoff.sh`
unpacks a fresh archive elsewhere, builds it and plays the starter, as a receiving agent
would: run it after changing what the archive carries or this page.
