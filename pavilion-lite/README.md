# Pavilion Lite

A compact 3D game engine in Rust for AI agents to build games with. It is the essential core
of the Pavilion engine (behind Shardfall) in one Cargo package of about 7,000 lines:

- a deterministic simulation at 60 ticks per second, with rapier3d physics, walking
  characters, moving platforms, triggers, projectiles, health and pathfinding;
- procedurally animated characters and ASCII-map levels: no asset files;
- a CPU renderer (toon shading, sun shadows, outlines, text), so screenshots work in any
  container without a GPU;
- one set of agent tools (screenshots with numbered marks, text maps, input driving, live
  parameters, rewind, replays, bots, benchmarks) as a CLI, a REPL and an MCP server;
- a window for people to play the result (keyboard and mouse);
- three sample games: `template`, `platformer`, `arena`.

**Agents: read [AGENTS.md](AGENTS.md).** It is the only document you need.

```sh
cargo build
cargo run -q -- capture game=platformer ticks=60 out=out/p.png
cargo test
cargo run --release -- play arena
```

Requirements: Rust 1.89 or newer and crates.io access for the first build. No system
packages for headless use; the play window needs a desktop (on Linux: X11 with
libxkbcommon-x11, or Wayland). Windows and macOS work as they are.
