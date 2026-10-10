# Live Creature Studio

Create procedural creatures from readable blueprints, then view and direct them in
Shardfall's native renderer. An LLM can choose a template or body plan, change named parts
and surfaces, build a rig, inspect its bones, and play its generated motion. The human can
use the same controls in the open studio.

The LLM runs in your client. It calls the engine through MCP. A local, pinned SpawnForge
compiler builds the geometry and motion in a separate process. Each accepted build updates
the native stage. The engine does not need a model API key or a hosted render service.

## Start and connect

### Windows download

Extract the whole ZIP to a writable folder. Double-click `Start-Creature-Studio.cmd` and
keep its window open. The package includes the compiler, a portable Node runtime, and a
saved ridgeback demo with its compiled cache. You do not need to install npm for this
package. Keep the `runtime` and `tools/creature-compiler` folders beside the programs.

Copy the server from `mcp-live.example.json` into your LLM client's MCP settings. Set
`command` to the full path of the extracted `pav.exe`, then reconnect the server:

```json
{
  "mcpServers": {
    "shardfall": {
      "command": "C:/path/to/Shardfall-Studio/pav.exe",
      "args": ["mcp", "--live"]
    }
  }
}
```

The client must support a local MCP server. The default bridge is `127.0.0.1:7878`.
A server started with `pav mcp` has its own headless session. Use `pav mcp --live` to
control the open window. To select another port, start the app with
`--bridge 127.0.0.1:7879` and the MCP server with `mcp --live 127.0.0.1:7879`.

`Start-Object-Studio.cmd` and `Start-Animation-Studio.cmd` open the other workspaces.
You can add `--backend dx12` to a Windows launcher command. The default native renderer
uses Vulkan.

### Source checkout

Use Node 22.18 or newer and the pinned Rust toolchain. See `START_HERE.md` for native
system dependencies. From the repository root on Linux, run:

```sh
scripts/creature-studio.sh
```

From a Windows source checkout with a native Rust toolchain, run:

```powershell
./scripts/creature-studio.ps1
```

These scripts install the pinned npm dependencies, build the compiler bundle, build
`shardfall` and `pav`, and open Creature Studio with its live bridge. To build each step
separately:

```sh
npm ci --ignore-scripts --no-audit --no-fund --prefix tools/creature-compiler
npm run build --prefix tools/creature-compiler
cargo build --locked -p pav_app -p pav_tools
./target/debug/shardfall --creature-studio
```

Use `.mcp.studio.json` as the checkout's MCP configuration. Set the server's working
directory to the repository root. With built programs, these commands also work:

```sh
shardfall --creature-studio
shardfall --creature-studio CREATURE/my_creature
shardfall --creature-studio --scene level/1
```

A new source checkout can open an empty stage before its first creature build. The
explicit scene stays behind the stage; the default studio scene is empty. The top bar
switches between Animations, Objects, Creatures, and Game. Each workspace keeps its camera
and controls. Creature Studio pauses the world while its own playback clock runs.

## First prompt

> Create a creature named moss_stalker from the ridgeback_stalker template at low quality.
> Give it shorter horns, dark teal skin, and clear stripes. Keep its walk and bite motions.
> Use named part and surface paths. Wait for each build to publish before the next edit.
> Use the current source revision for each edit. Keep my camera when the new shape appears.
> Play its walk, check the frame ticket, and take a capture. Then wait for my next direction.

Useful follow-up directions include “make the tail longer,” “increase the stripe contrast,”
“give it larger forefeet,” “pause halfway through the bite,” and “undo the last change.”
The agent should inspect the current blueprint and catalog before it chooses field names.

## Agent loop

| Tool | Purpose |
| --- | --- |
| `creature_catalog` | Discover templates, body plans, part modules, surfaces, motion parameters, and the blueprint schema. |
| `assets kind=creature` | List compiled creatures and saved sources. |
| `creature_edit` | Create, copy, inspect, patch, replace a surface or blueprint, undo, redo, or rebuild. |
| `creature_status` | Read an asynchronous build job and its result. |
| `creature_preview` | Open a creature, control playback, fit the camera, or inspect bone transforms. |
| `studio_status` | Read the active workspace and confirm native frame submission. |
| `capture` | Return an image from the same native stage and camera. |
| `filmstrip` | Capture several stage frames in one image. |

The generic tools also accept `asset_edit kind=creature` and
`asset_preview kind=creature`. They use the same creature handlers.

Use this sequence for each change:

1. Inspect the source or discover a template. Keep its source revision.
2. Send one edit with related changes in one `ops` batch.
3. Keep the returned job ID. Poll `creature_status` until it is `published`, `failed`, or
   `superseded`. A queued reply does not mean the new geometry is visible yet.
4. On publication, read the new source revision, diff, and warnings. Check the live frame
   ticket when you need confirmation from the native window.
5. Use a capture at a visual checkpoint, then continue from the published revision.

### Create and inspect

Tool requests use the same name and `args` object in MCP and the JSON-line CLI:

```json
{"tool":"creature_catalog","args":{"module":"horn.curved"}}
{"tool":"creature_edit","args":{"action":"create","name":"moss_stalker","template":"ridgeback_stalker","quality":"low","if_revision":"absent"}}
```

The create reply returns a numeric `job`. Use that returned number here; `12` is an example:

```json
{"tool":"creature_status","args":{"job":12}}
```

When that job is published:

```json
{"tool":"creature_edit","args":{"action":"inspect","name":"CREATURE/moss_stalker"}}
```

Inspect returns the full blueprint, source file path, source revision, history counts,
compiled revision, clip names, and bounds. Create and edit replies stay small; the compiled
mesh arrays are not sent through ordinary tool replies.

Names use `CREATURE/name`. A bare lowercase name selects the same namespace. Use a letter
first, then lowercase letters, numbers, underscores, or hyphens. Reserved Windows file
names are rejected. The name in the blueprint is a display title; the tool's `name` is the
stable saved-asset ID.

Bundled template IDs are `ridgeback_stalker`, `ash_dragon`, `cave_bat`, `bog_troll`,
`ember_beetle`, `reef_shark`, `stone_tortoise`, and `wild_horse`. The catalog gives their
names and the parameters of their modules. `creature_catalog schema=true` returns the full
blueprint JSON schema without starting the compiler.

Creation can also use a theme and seed:

```json
{"tool":"creature_edit","args":{"action":"create","name":"seeded_beast","theme":"beast","seed":51,"quality":"low","if_revision":"absent"}}
```

Or start from a small blueprint with a body plan's defaults:

```json
{"tool":"creature_edit","args":{"action":"create","name":"new_quadruped","quality":"low","if_revision":"absent","blueprint":{"format":"spawnforge/0.2","name":"New Quadruped","extends":"quadruped","seed":51}}}
```

Choose one of `template`, `theme`, or `blueprint`. A theme can also take `constraints` from
the generator's schema. After creation, inspect the normalized blueprint before editing it.

### Edit named parts and surfaces

Copy `revision` from inspect or the last published job into `if_revision`. The placeholder
below must be replaced with that value:

```json
{
  "tool": "creature_edit",
  "args": {
    "action": "patch",
    "name": "CREATURE/moss_stalker",
    "if_revision": "COPY_THE_CURRENT_SOURCE_REVISION",
    "ops": [
      {"op":"set","path":"parts[id=horns].params.length","value":0.16},
      {"op":"set","path":"skin.palette.base","value":"#285a78"},
      {"op":"set","path":"skin.layers[type=stripes].strength","value":0.8}
    ]
  }
}
```

One batch can contain 1 to 100 operations. The compiler checks the complete result before
it saves and publishes it. An invalid operation rejects the whole batch. A source change
is one undo step.

| Operation | Meaning |
| --- | --- |
| `set` | Set a field at `path` to `value`. An object value replaces that object. |
| `add` | Append `value` to a list such as `parts`, `limbs`, or `skin.layers`. |
| `remove` | Remove a field or list item. Removing an inherited field returns it to the preset or default. |
| `mirror` | Change a limb or part's attachment side. Optional `side` is `both`, `left`, `right`, or `center`; default is `both`. |
| `scale` | Multiply a number or numeric profile by positive `by`. An empty path scales the whole creature. |

Use stable IDs for limbs and parts: `limbs[id=foreleg].length` or
`parts[id=horns].params.curve`. A `set` path must name a field below an ID; it cannot end at
`parts[id=horns]`. Use `add` to introduce a complete new part, and `remove` to remove an ID.

Surface layers, gaits, and actions can use their type, such as
`skin.layers[type=stripes].count` or `motion.gaits[type=walk].stride`. A type selector finds
the first item of that type. Inspect the list and use an index when a type occurs more than
once. The catalog describes module parameters and valid ranges.

For a full surface change, `action=surface` takes the complete `skin` object. Include all
palette entries and layers you want to retain. For a full blueprint change, use
`action=replace` with `blueprint`. Copy a saved source with `action=copy`, `from`, and a new
`name`. Copy the source before making a separate design branch.

The compiler can reuse geometry and baked motion for compatible surface-only edits. Changes
to anatomy, motion, palette colors used by generated parts, or the base skin material require
a full build. The published job reports `build_kind`; use its warnings to judge which visual
features the native preview can show.

## Jobs, revisions, and visible feedback

| Value | Meaning |
| --- | --- |
| `creature_edit` inspect or published job `revision` | Source revision. Use it as the next edit's `if_revision`. |
| Inspect `asset_revision`, or preview `revision` | Revision of the compiled native definition. Do not use this as the source guard. |
| Preview `source_revision` | Source revision used to build that compiled definition. |
| `job` | A build request ID in the current process/session. |
| `feedback.ticket` | A live frame-submission ID. It is separate from revisions and jobs. |

A stale source guard fails before it queues a new edit. The source is checked again before
save and publication. A newer request for the same asset supersedes unfinished work for
that asset. Jobs have these states:

| State | Meaning |
| --- | --- |
| `queued` | Waiting for the compiler worker. |
| `building` | Validating, building, baking colors and motion, or saving. |
| `ready` | Saved and waiting for its owning session to publish the definition. |
| `published` | The native session accepted the definition. Read its source revision and warnings. |
| `failed` | The job could not complete. Read `error` and any structured `issues`. |
| `superseded` | A newer request replaced this unfinished job. Follow the newer request. |

Keep the last valid preview while a build runs. The stage adopts a completed edit to its
selected creature even when `preview=false`. That flag prevents the edit from choosing a
new preview. With `preview=true`, a completed build selects its result only if the user has
kept the same workspace and selection since the request. This prevents a late build from
pulling the user away from another workspace.

Compatible revisions of the same creature keep its clip, playhead, playback settings, and
camera. A changed bone hierarchy or removed selected clip resets the playhead. Use Fit after
a large size change. A restored snapshot keeps its exact immutable compiled definition;
reading the registry does not silently replace it with a later revision.

On a live publication, use its returned frame ticket:

```json
{"tool":"studio_status","args":{"ticket":25}}
```

Use the actual returned ticket; `25` is an example. `feedback.state=submitted` means the
native window submitted a frame with that edit and its camera changes. It does not measure
GPU completion or the monitor's physical display. A later edit can supersede an earlier
unsubmitted ticket. `creature_status job=...` also includes the current state of that job's
feedback ticket. Headless sessions have no native-window submission tracker.

## Playback and visual inspection

After the build is published, select a clip from `creature_preview` status:

```json
{"tool":"creature_preview","args":{"name":"CREATURE/moss_stalker","clip":"walk","playing":true}}
{"tool":"creature_preview","args":{"playing":false,"time":0.15}}
{"tool":"creature_preview","args":{"step":1}}
{"tool":"creature_preview","args":{"action":"pose"}}
{"tool":"capture","args":{"width":960,"height":640}}
```

Times are seconds. Scrubbing and source-frame steps pause playback unless `playing=true`
is also supplied. Time clamps to the selected clip's endpoints, including its last frame.
Use `looping` to override repeat behavior, `speed` for a multiplier from 0.05 to 8, and
`clip=rest` for the unanimated rest pose. The preview samples local bone tracks and skins
all mesh streams on the CPU before the native renderer draws them.

`action=pose` returns bone names, parent indices, stage-space positions in metres, and
quaternions in XYZW order. It uses the same playhead, scale, and stage rotation as the image.

`turntable=true` rotates while playback runs. `yaw` sets the base rotation in degrees;
`scale` is a uniform preview scale from 0.01 to 100. `action=fit` frames the rest bounds and,
when the turntable is enabled, its range of rotations. Wide action or flight poses can
extend beyond rest bounds; zoom out when needed. `action=close` returns to the preserved
game. A status call alone does not switch workspaces.

The panel provides clip selection, Play/Pause, Restart, Step, Fit, Loop, Turntable, and
speed. F6 plays or pauses, F7 steps, and F8/F9 change speed. Right-drag orbits the camera.
Dirty drafts stay in the panel while an LLM changes the source. Apply uses the draft's
original revision; a conflicting edit requires a new inspection or an explicit discard.

For a motion sheet, set `playing=true` and call `filmstrip` with `frames` and `every`.
Filmstrip advances the preview clock; the world behind the stage stays paused. Captures
render and encode an image, so use metadata and frame tickets for routine confirmation.

## Saves, undo, and file edits

The default source root is `assets/creatures`, relative to the process's working directory.
The launchers set that directory to the checkout or extracted package. Set `PAV_CREATURES`
to choose another root. In live mode, the running app's root controls saves, regardless of
the MCP client's working directory.

| Path under the creature root | Contents |
| --- | --- |
| `workshop/<name>.json` | Authoritative source: format, asset name, quality, and editable blueprint. |
| `.compiled/<name>--<revision>.json` | Validated, disposable native data cache. |
| `.editor/<name>.json` | Source undo/redo history. |

The source wrapper has this shape:

```json
{
  "format": 1,
  "name": "my_creature",
  "quality": "low",
  "blueprint": {
    "format": "spawnforge/0.2",
    "name": "My Creature",
    "extends": "quadruped",
    "seed": 51
  }
}
```

Each asset keeps up to 32 undo and redo entries on disk. `action=undo` after creation removes
its source and compiled registry entry; the selected workspace becomes an empty stage.
Redo restores it through the build queue. A rebuild with unchanged source does not add an
undo step. A quality change is part of the source and can be undone.

The native watcher waits for 250 ms of quiet, then queues an external source-file change
through the same compiler and publication checks. Tool saves publish directly. Watch events
for those same saved revisions are skipped. A transient save lock is retried. Invalid file
edits leave the last valid compiled definition visible; fix the source file to proceed.
An external source change invalidates history based on the old source revision.

Startup loads valid saved caches without starting Node. The native watcher can queue saved
sources that need a build. A headless session has no file watcher; use
`creature_edit action=rebuild name=CREATURE/my_creature` for a source without a matching
cache. Keep the source and history folders when you replace the application. Job IDs and
frame tickets are process state and do not survive a restart.

## CLI use

With the native studio open, run the example from the repository root:

```sh
./target/debug/pav live --stop-on-error < scripts/examples/creature-ridgeback.jsonl
```

In a Windows package, use Command Prompt from the extracted folder:

```bat
pav.exe live --stop-on-error < creature-ridgeback.jsonl
```

The example queues creation of `CREATURE/example_ridgeback` and reads status. Poll the
returned job through your LLM or `pav live` before a dependent edit. Run it once, or change
its saved name before another run. `--stop-on-error` stops on immediate command errors;
it does not wait for an asynchronous job or detect that job's later failure.

A one-shot CLI creation waits for its build before the process exits:

```sh
./target/debug/pav creature_edit action=create name=cli_ridgeback template=ridgeback_stalker quality=low
./target/debug/pav creature_edit action=inspect name=CREATURE/cli_ridgeback
```

Persistent MCP, `pav live`, and `pav repl` remain asynchronous. Keep a headless persistent
session open and poll it until publication before quitting. For an image from a saved
compiled creature, open `pav repl`, select it with `creature_preview`, then call `capture`.

## Native scope and limits

This release creates and previews rigs. It does not yet spawn these generated rigs as combat
actors or room instances, retarget them to the game's human skeleton, or export GLB/FBX.
`asset_spawn` still places primitive props. Creature source import uses SpawnForge
blueprint JSON. The browser build does not run the local Node compiler.

The native adapter preserves arbitrary parent-first bones, model-space bind transforms,
optional local rest rotations, four bone influences per vertex, sockets, and baked local
bone clips. It draws skin, parts, eyes, and membranes. Normals follow the skinned geometry.
The native format and compiler revision are validated before publication.

Surface colors are baked into vertices at build time. This shows broad color patterns, but
fine patterns can be limited by mesh resolution. The original shader's relief, roughness,
animated patterns, emissive pattern glow, shader breathing, and shell fur are not drawn by
this path. Membranes are opaque, double-sided surfaces; transparency and transmission are
approximated. Baked jaw and eyelid tracks are retained when the source motion contains them.

Motion is baked from the generator on flat ground. It does not run the original procedural
controller, terrain adaptation, or gameplay logic each frame. Clip speed, distance, events,
and root-motion metadata are retained for future integration. Preview playback does not
execute combat events or drive a game actor. Read each build's warnings: some blueprint
features are accepted by the generator but have no geometry or native visual effect.

Start with low quality for shape changes, then rebuild at medium or high quality when the
form is ready. Compilation is asynchronous, not instantaneous. The cache can reduce work
for repeated compatible surface edits; it does not turn arbitrary topology changes into
immediate parameter updates.

The bundled compiler uses SpawnForge revision
`851880256987ecdb2895c6afd01f84df64199bdb` and native adapter format 1. It contains the pinned
core and module sources plus Shardfall's adapter. See
`tools/creature-compiler/UPSTREAM.md`, `pin.json`, and the packaged third-party notices for
source provenance and license metadata. Do not replace the vendor sources with another
SpawnForge revision without updating the adapter and native validation together.

For custom installations, `PAV_CREATURE_WORKER` selects the compiler's `worker.mjs` entry
point and `PAV_NODE` selects a Node executable. Use absolute paths. The normal source and
Windows package layouts do not need these overrides.
