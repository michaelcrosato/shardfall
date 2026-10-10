# Live Object Studio

Build reusable objects from named primitive parts. An LLM can create a new object, copy a
template, inspect its parts, change several parts at once, and place copies in the game.
The open studio shows each accepted edit through Shardfall's normal renderer.

The human gives direction in their LLM client. The client calls the engine through MCP.
The native controls call the same tools, with the same validation, saves, and undo history.
The engine does not need a model API key or a separate render service.

## Start and connect

From source, on Linux:

```sh
scripts/asset-studio.sh
```

On Windows with a native Rust toolchain:

```powershell
./scripts/asset-studio.ps1
```

The scripts build `shardfall` and `pav`, then open the object workspace and the live bridge.
With built programs, use:

```sh
shardfall --asset-studio
shardfall --asset-studio WORKSHOP/garden_bench
shardfall --asset-studio --scene level/1
```

`--studio` is an alias for `--asset-studio`. An explicit scene is preserved behind the stage.
The default studio scene is empty. You can also open Object Studio from the game's pause
menu. The top bar switches between Animations, Objects, and Game. The two workspaces retain
their selections, controls, and cameras when you switch.

For the Windows download, extract the whole ZIP to a writable folder. Double-click
`Start-Object-Studio.cmd`. Use `Start-Animation-Studio.cmd` to start with animations instead.
Both launchers keep file paths relative to the extracted folder.

Use `.mcp.studio.json` as the source checkout's MCP configuration. Set the server's working
directory to the repository root. In the Windows package,
copy `mcp-live.example.json` into your client's MCP settings. Set `command` to the full path
of the extracted `pav.exe`. Keep the arguments `mcp`, `--live`. Reconnect the MCP server.
The default live address is `127.0.0.1:7878`.

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

The client must support a local MCP server. A server started without `--live` runs its own
headless session. To use another address, start the game with `--bridge 127.0.0.1:7879` and
the server with `pav mcp --live 127.0.0.1:7879`.

## First prompt

> Copy the bench template to garden_bench. Make the seat and backrest teal. Use dark metal
> legs. Add an armrest on each side. Keep the object about 2.2 metres wide. Apply each design
> change as one patch and show it in the live studio. Keep my camera in place. Use the last
> revision for each edit. Take a capture when the first version is ready, then wait for my
> next direction.

Useful follow-up prompts include “make the backrest taller,” “move both armrests out by
10 centimetres,” “undo the last change,” and “place two copies beside the entrance.”
The agent should ask for a world position or inspect the scene when a placement direction
does not identify a clear location.

## Agent loop

| Tool | Use |
| --- | --- |
| `assets` | Find templates and saved objects. Returns names, revisions, and bounds. |
| `asset_edit` | Create, copy, inspect, patch, replace, undo, or redo an object. |
| `asset_preview` | Open, fit, rotate, or close the isolated object stage. |
| `asset_spawn` | Add, list, update, or remove placed instances in the preserved scene. |
| `studio_status` | Read the current workspace and confirm a live frame submission. |
| `capture` | Return a PNG through MCP from the same renderer and camera. |
| `snapshot_save` | Save exact scene state, including placed object definitions. |

Use `assets` to discover names, then inspect or copy one. Built-in objects are `BUILTIN/box`,
`BUILTIN/crate`, `BUILTIN/bench`, and `BUILTIN/lantern`. Saved objects use `WORKSHOP/name`.
Names are lowercase. Start a new object with `create`, using a template or a complete
definition in `asset`.

All REPLs accept JSON lines. The same tool name and `args` object work in MCP:

```json
{"tool":"asset_edit","args":{"action":"copy","from":"BUILTIN/bench","name":"garden_bench"}}
{"tool":"asset_edit","args":{"action":"inspect","name":"WORKSHOP/garden_bench"}}
```

Inspect returns the definition, field guide, part names, bounds, revision, and history state.
Edit replies return compact metadata. Pass the returned `revision` as `if_revision` on the
next edit. If it is stale, inspect again before making another change.

```json
{
  "tool": "asset_edit",
  "args": {
    "action": "patch",
    "name": "WORKSHOP/garden_bench",
    "if_revision": "COPY_THE_LAST_REVISION_HERE",
    "ops": [
      {"op":"set","part":"seat","fields":{"color":"#3e9a9e"}},
      {"op":"set","part":"backrest","fields":{"color":"#3e9a9e"}},
      {
        "op":"add",
        "part":"armrest_left",
        "value": {
          "shape":{"type":"rounded_box","half":[0.07,0.06,0.42],"radius":0.025},
          "pos":[-1.0,1.12,0],
          "color":"#26313e"
        }
      }
    ]
  }
}
```

`set` changes only the listed fields. A supplied `shape` replaces the whole shape object.
`add` needs a new part ID and a complete part in `value`. `remove` needs an existing part ID.
The final object must contain at least one part. There is a limit of 256 parts.

The whole batch is checked before it is saved or published. One failed operation rejects
the entire batch. One accepted batch is one undo step. Each object keeps up to 32 undo/redo
steps on disk. `undo` after initial creation removes the authored file; `redo` restores it.
Existing placed copies retain their embedded definition if their source file is removed.
Use `asset_spawn action=remove` to remove a placed copy.

By default, an accepted edit opens its object in the preview. Use `preview:false` to update
placed copies while keeping the current workspace. Editing the selected object preserves
the preview clock and camera. Use `asset_preview action=fit` after a large size change.

## Fast feedback

The live bridge publishes the edited scene after the tool returns. The window draws it on
its next available frame. Edits use the existing renderer; there is no code build or page
reload for changes to a definition.

Live edit replies contain `feedback.ticket`. Use this optional check:

```json
{"tool":"studio_status","args":{"ticket":12}}
```

`feedback.state` is `pending`, `submitted`, `superseded`, or `unknown`. A submitted ticket
means a frame with that edit and its camera changes was submitted by the native window.
It does not measure GPU completion or the monitor's physical display. If a later edit
replaces an earlier one before it is drawn, the earlier ticket is reported as superseded.
The last 64 tickets are retained. Tickets are separate from content revisions and world
ticks, so paused edits and undo can be confirmed correctly.

The report includes tool application time and time to frame submission. These times exclude
transport from the LLM client. Headless sessions have no window feedback tracker.

For a short human feedback loop:

1. Combine related part changes in one `ops` batch.
2. Keep the studio open and keep its camera in place.
3. Use metadata or `studio_status` for routine confirmation.
4. Request a small capture at a visual checkpoint, or when the agent needs to inspect form.

Capture runs on a second GPU device and encodes an image. It costs more than an ordinary edit.
The normal edit path does not make a capture. File edits are also supported: native file
watching waits for 250 ms of quiet before it reads a changed file. A bad file reports an error
and leaves its last valid definition active. Tool saves publish directly and skip this delay.

## Definition format

An asset is a readable JSON document. Map keys under `parts` are stable part IDs:

```json
{
  "format": 1,
  "name": "marker",
  "description": "A small glowing marker on a post.",
  "parts": {
    "post": {
      "shape": {"type":"cylinder","half_height":0.6,"radius":0.05},
      "pos": [0,0.6,0],
      "color": "#26313e"
    },
    "light": {
      "shape": {"type":"sphere","radius":0.16},
      "pos": [0,1.3,0],
      "color": "#79debd",
      "emissive": 2,
      "solid": false
    }
  }
}
```

| Field | Meaning |
| --- | --- |
| `shape` | `box`, `rounded_box`, `sphere`, `capsule`, or `cylinder`. |
| `pos` | Local centre in metres: X right, Y up, Z forward. Defaults to `[0,0,0]`. |
| `yaw`, `pitch`, `roll` | Degrees about Y, X, Z in that order. Default 0. |
| `color` | sRGB `#rrggbb`. |
| `look` | `flat`, `cel`, `lit`, `unlit`, or `cutout`. Ordinary prop cutout uses a flat look. |
| `emissive` | Glow strength from 0 to 32. Default 0. |
| `solid` | Include this part in a colliding instance's compound shape. Default true. |

Boxes use `half:[x,y,z]`; a rounded box also needs `radius`. A sphere needs `radius`.
Cylinders and capsules need `half_height` and `radius`. A cylinder's total height is twice
`half_height`. A capsule's total height is twice `half_height + radius`.

Place the asset's origin at a useful reference point, usually the floor. The preview lifts
its lowest point to the stage. A placed instance uses its explicit origin position.
Unknown fields, bad colors, invalid sizes, duplicate part IDs, and non-finite values fail
validation with an error. There is no executable script in the definition.

## Place and preserve objects

```json
{"tool":"asset_spawn","args":{"action":"add","name":"WORKSHOP/garden_bench","pos":[3,0,2],"yaw":90,"scale":1,"collide":true}}
{"tool":"asset_spawn","args":{"action":"list"}}
{"tool":"asset_spawn","args":{"action":"update","id":123,"pos":[4,0,2],"yaw":45}}
{"tool":"asset_spawn","args":{"action":"remove","id":123}}
```

Use the ID returned by `add`; `123` is an example. Each instance has one root ID. It moves
and deletes as one object. Colliding instances use one fixed compound collider made from
solid parts. `collide:false` makes a visual-only instance. Dynamic multipart bodies are
outside this increment.

Accepted definition changes refresh the visuals and collision for active and sleeping
instances of that asset. Their positions, scales, root IDs, and room assignments stay in
place. Fixed-object changes invalidate the game's cached navigation map. Existing native
F10 move and delete controls work on the whole object, including while the world is paused.

Instances live in the current scene and full snapshots. Ordinary hero saves are not a
level layout file. In a pavilion room, F10 Save writes `asset` and `scale` references in its
`[[object]]` entries. Keep the corresponding workshop JSON with the room. Snapshots embed
exact definitions, so loading an old snapshot does not silently use a newer library shape.
Later accepted edits can refresh its instances again.

## Files and scope

Templates are readable files in `assets/props/templates`, embedded in the programs.
Accepted work saves in `assets/props/workshop/name.json`; history is in `assets/props/.editor`.
Set `PAV_ASSETS` on the game process to use another props root. The first save creates missing
folders. A cold CLI or MCP process loads saved definitions before constructing a scene.

This increment adds procedural objects, surface controls, and scene placement. Imported
mesh models, texture painting, and a general level layout editor are future asset types.
Animation authoring remains available in the same studio; see `ANIMATION_STUDIO.md`.

Run the supplied example with the studio open:

```sh
pav live --stop-on-error < scripts/examples/object-lantern.jsonl
```

It creates `WORKSHOP/garden_lantern`. It stops if that name already exists. The Windows
package includes the same example at its top level.

## Verification

`scripts/smoke-asset-studio.py` checks the real native window through MCP. It covers saved
editing, rendered changes, bad batches, revision guards, undo/redo, placed instance refresh,
file reloads, workspace switching, cold startup, and frame feedback. It uses temporary asset
folders and writes captures plus a JSON result to `out/asset-studio-smoke`.

The focused Rust tests cover schema validation, atomic persistence, snapshot definitions,
compound collision and streaming wake, room references, CPU scene building, paused picking,
and passive UI frames. The build checks also cover the native, Windows, and browser targets.
