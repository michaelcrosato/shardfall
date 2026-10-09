# Shardfall — Engine & Showcase Design Plan

"Shardfall" is the game and repository name. "Pavilion" is the engine's playable tech demo.
This document is the source of truth for what we are building and why; the principles are in `docs/DOCTRINE.md`.
If a pasted version of this plan in chat differs from this file, the pasted version wins; update this file to match.

## 1. What we're building
An AI-first game engine, plus a living tech-demo showcase built on it, written in Rust. Everything you see and hear is generated from code.

The showcase is a continuous pavilion of themed wings and test rooms set inside an infinite streaming world. It serves as a personal evaluation lab and as living documentation.

The engine is meant to be reused for many different games. We don't know yet what game we're making, so flexibility wins over polish.

## 2. Principles
`docs/DOCTRINE.md` sets the principles and the escalation process, and outranks this document.
`AGENTS.md` says how the doctrine applies to this repo and lists the working rules under it:
gameplay first, use tools, run well on a modern PC, approximations that feel right, check
selectively, token efficiency.

## 3. Platform & tech
- **Where it runs:**
  - Native desktop app.
  - The human plays on Windows 11 (RTX 3060 Ti at work, RTX 4070 Super at home).
  - Agents build, run and test on Linux (cloud containers, WSL2 Ubuntu).
- **Language:** Rust. The stable toolchain is pinned with `rust-toolchain.toml`, and `Cargo.lock` is committed.
- **Graphics:** wgpu, which implements the WebGPU API.
  - Vulkan is the default backend on Windows and Linux.
  - DX12 can be selected with a startup setting.
  - No OpenGL or WebGL.
- **Headless rendering for agents:** Mesa lavapipe (software Vulkan) on machines without a GPU. Same engine code, different driver.
- **Physics:** rapier3d. Coordinates are f32, with a one-line switch to f64 (`rapier3d-f64`) if we ever see jitter far from the origin.
- **Other libraries:** winit for window and input, gilrs for gamepads, cpal for audio output (with our own synthesizer), egui for the debug UI.
- **Browser build:** a late milestone. The same code compiles to WebAssembly and WebGPU. Library choices keep this possible, but we don't maintain it until then.
- **Assets:** no image, model, texture or audio files. Geometry, surfaces, effects and sound are all generated from code. Fonts are allowed, for UI and in-world text.

## 4. World & camera
- **The world is fully 3D.** "2D top-down" is a camera preset, not an engine limit.
- **Camera parameters are live:**
  - tilt, from 90° (straight down) to 0° (side view);
  - rotation;
  - distance/zoom;
  - orthographic or perspective projection;
  - field of view.

  They can be changed mid-game from the tuning panel, each room saves its own defaults, and game code can animate the camera.
- **Standard camera helpers:**
  - fade or cut away geometry between the camera and the player;
  - hide upper floors while the player is below them;
  - mouse aim targets the point on the ground plane at the player's height;
  - movement input is relative to the camera's rotation.
- **Default look:** tilted to about 60–75°, stylized with flat or cel shading and outlines. A 90° orthographic view gives the clean 2D look.

## 5. Rendering
- **Pipeline:** a real 3D renderer with depth, MSAA, HDR and a post-processing chain.
- **Procedural geometry:** boxes, rounded boxes, cylinders, spheres, capsules, extruded shapes, tile terrain, tubes and ropes.
- **Analytic SDF primitives** (spheres, capsules, etc.) are pixel-perfect at any zoom, and can blend smoothly into each other for organic shapes.
- **Swappable styles** per room or per object: flat 2D look, cel/toon, or fully lit.
- **Lighting:** dynamic lights with real shadows, emissive surfaces and bloom.
- **GPU compute is cosmetic only** (particles and similar). Anything that affects gameplay runs on the CPU.
- **Text:** in-world text is rendered by the engine from a font; UI text goes through egui.

## 6. Characters
- **Puppet model:** an invisible skeleton, driven by procedural animation, with parts attached to the bones.
  - **Procedural animation:** walk cycles driven by speed, foot IK to the ground, leaning into turns, recoil, squash-and-stretch, and secondary motion.
  - **Parts:** mostly spheres and capsules, so silhouettes read as clean 2D shapes from any camera angle.
- **One motion, many looks:**
  - a cutout (flat 2D art on a camera-facing card, for fixed-camera designs);
  - a puppet with a flat 2D look;
  - a lit puppet.
- **Camera-aware rules** recover 2D "cheats", such as eyes that stay visible or a body that leans toward the viewer. Stepped animation ("on twos") is optional.
- **Everything is a parameter:** proportions, colors and limb count are sliders.

## 7. Physics, height & movement
- **Real verticality:**
  - jumping gaps, climbing onto boxes, ladders, ledges and falls;
  - crawling under things and ducking projectiles;
  - multi-floor buildings with destructible floor tiles (blow a hole, drop to the floor below).
- **Flat rooms** lock motion to a plane (bullet hell). **Side-view rooms** are a camera setting plus an axis lock, so Celeste-style rooms are possible.
- **High-count projectiles and bullets** use a lightweight custom system, not physics bodies.
- **Movement models** plug in per character, can be switched from a dropdown per room, and every number in them is tunable:
  - **Instant:** immediate response, with a hold-to-slow focus button.
  - **Momentum:** acceleration, skidding, air control.
  - **Grid/tile:** step-by-step movement.
  - **Committed animations:** moves play out fully once started.
  - **Vehicle:** grip and drift.
  - **Flight:** banking and inertia.
  - **Special states:** climbing and swimming.

  Each room marks the model it was designed for. Other models can be tried wherever it makes sense.
- **Tick rate** defaults to 60 per second, with 120 and 240 selectable. Input delay is measurable, and vsync and smoothing can be toggled.

## 8. Simulation architecture
- **Headless-first:** the simulation core is a library with no window, graphics or audio dependencies. It runs in two ways:
  - inside the game, on its own thread; the renderer observes and interpolates, and neither blocks the other;
  - as a headless command-line program at maximum speed.
- **Time control:** fixed tick rate, speed control from slow motion to maximum, pause and single-step.
- **Repeatability (the cheap version):** the same machine, build, seed and recorded inputs give the same result.
  - It exists to support rewind and bug replays.
  - It is not guaranteed across machines.
  - If it ever gets in the way of gameplay or performance, gameplay wins.
- **Time travel:**
  - periodic snapshots plus an input log;
  - rewind restores the nearest snapshot and re-simulates forward;
  - history is limited by memory (adjustable);
  - acting after a rewind starts a new timeline;
  - snapshots can be saved to files.
- **Streaming:**
  - the world is split into chunks, loaded around interest points (the player, agents), never around the camera;
  - terrain regenerates from the seed;
  - changed or moved objects are saved and restored when you return;
  - the pavilion sits at the center with wings branching outward, and procedural wilderness beyond it is used for streaming and encounter tests.

## 9. Input
- **Devices:** keyboard+mouse and gamepad are always supported.
  - Each room declares a primary device, and its controls and design are built around it.
  - The secondary device gets a best-effort mapping that never changes the design.
  - On-screen prompts show the device touched last.
- **Fixed system layer that never rebinds:** pause, rewind, step, tuning panel, teleport menu, leave room and reset room.
- **Guidance and layout:** an on-screen control guide appears on entering a room. Keys are bound by physical position.

## 10. Sound
- Everything is synthesized in code: oscillators, noise, envelopes and filters.
- The game runs without a sound device, and headless runs can write `.wav` files.

## 11. Tools for humans
- **Boot diagnostics:**
  - numbered stages with timings, written to the terminal, a log file and the screen;
  - if graphics fail to start, a Windows pop-up shows the error and the log file's path;
  - crash reports appear on screen and in the log.
- **Tuning panel (egui):**
  - every registered parameter, grouped: physics, movement, camera, shaders, audio and simulation speed;
  - presets can be saved, exported and imported;
  - FPS, tick and frame-timing graphs;
  - possibly live shader editing.
- **Sandbox editing:** spawn, drag and delete objects, and save the room to a data file.
- **Navigation:**
  - a teleport menu and a reset button per room;
  - an info card per room saying what it demonstrates and what to try;
  - a command-line option to launch straight into a room with a given seed.

## 12. Agent layer
- **One tool registry, three interfaces:** CLI commands, an MCP server (stdio), and later a live bridge into the running game.
- **Tools:**
  - list and describe rooms;
  - start, run, and step N ticks;
  - spawn, query and modify entities;
  - get and set parameters;
  - save, load and rewind snapshots;
  - record and replay inputs;
  - capture: screenshots, filmstrips (many frames tiled into one image), timing reports and `.wav` audio;
  - benchmark ticks per second;
  - validate and hot-load room files.
- **Rooms vs mechanics:** rooms built from existing parts are data files that hot-reload with no rebuild. New mechanics are Rust code, written from templates.
- **Rulebook:** `AGENTS.md`, plus a `CLAUDE.md` pointer. It covers the principles, an architecture map, how to add a room or mechanic, and how to test and capture.

## 13. Showcase roster
- **Movement & Feel Lab:**
  - courses: tightrope, slalom, timing gates, dodge gauntlet;
  - a movement-model switcher, with live input-delay and feel metrics;
  - a camera bench (sweep angle and projection on the same course);
  - a multi-floor verticality building (ladders, ledges, crawl spaces, ducking, blow a hole and drop).
- **Physics Lab:** stacking, springs and soft bodies, chains and rope bridges, conveyors, a bounce and friction gallery, destructible floors, and a stress test with a ticks/sec meter.
- **Procedural Animation Lab:** walk cycles, jointed limbs (spider or lizard), impact recoil, squash-and-stretch, secondary motion (tails, antennae), and the Character Style Bench (one character, a swingable camera, and a switch between cutout, flat-look puppet and lit puppet).
- **Visual Effects Wing:** lights and shadows, GPU particles, bloom, screen distortion, and global illumination (stretch goal).
- **Aesthetic & Filter Wing:** side-by-side comparisons of the pure style against filters (scanlines, CRT curvature, dithering, color grading, pixelation), plus a build-your-own filter stack bench.
- **Genre Wing:** bullet hell, grid stealth, a drift car and a helicopter.

## 14. Delivery & workflow
- **Repo:** `michaelcrosato/shardfall`, stays private. Vercel hosts the browser build;
  the root `vercel.json` defines the build and static output.
- **Branch:** work on `main` (the GitHub default). Commit and push often, because cloud containers are temporary.
- **Windows build:** the `.exe` is cross-compiled in the cloud and sent in chat at the end of each milestone. Local WSL2 agents can build the `.exe` and launch it directly on Windows.
- **Progress log:** `docs/PROGRESS.md` records the current milestone, what's done, what's next, and decisions made, so any agent can resume.
- **Build order:** sequential. Once the foundation and rulebook exist, self-contained rooms can be handed to cheaper helper agents and reviewed. This also tests the goal that smaller models can extend the engine.
- **Decisions:** ask the user only about decisions that are genuinely theirs. Otherwise decide, record the decision in this document, and continue. Escalations follow the doctrine and are logged in `docs/ESCALATIONS.md`.

## 15. Milestones (each ends with a working `.exe`)
1. **M1 Foundation:**
   - workspace and crates;
   - window, wgpu on Vulkan, and boot diagnostics;
   - a headless simulation loop;
   - lavapipe capture (screenshots) working in the cloud;
   - Windows cross-compile verified;
   - `AGENTS.md` and `PROGRESS.md`.

   *Done when:* the `.exe` opens and shows the boot log and a lit test scene, the headless CLI runs ticks, and capture produces a PNG.
2. **M2 Core gameplay:**
   - rapier3d integration and tile-based levels with heights;
   - character puppet v1;
   - movement models v1 (instant and momentum);
   - the camera system, with all parameters live;
   - keyboard+mouse and gamepad input, with the system layer;
   - the egui tuning panel;
   - time controls (pause, step, speed, rewind);
   - agent CLI and MCP server v1.

   *Done when:* the player can walk, jump onto boxes, climb a ladder, blow a hole in a floor and drop through, tweak camera and movement live, and rewind.
3. **M3 World & rooms:**
   - chunk streaming, the pavilion and the wilderness;
   - the room framework: enter/exit, input switching, control guide, info card, reset, teleport, launch-into-room;
   - room data files with hot reload;
   - sandbox editing;
   - synth sound v1;
   - filmstrip capture.
4. **M4:** Movement & Feel Lab.
5. **M5:** Physics Lab.
6. **M6:** Procedural Animation Lab, including the Character Style Bench.
7. **M7:** Visual Effects Wing.
8. **M8:** Aesthetic & Filter Wing.
9. **M9:** Genre Wing.
10. **M10:** Browser build, live agent bridge, and a polish pass.

## 16. Out of scope for now
- Determinism across machines.
- Mac support and other browsers.
- Compatibility work.
- Code signing.
- Performance optimization beyond "runs well on a modern PC".
- GitHub automation.
