# Animation Studio

Animation Studio lets an LLM inspect, create, and change Shardfall animations while you watch the result.
The studio uses the same puppet, clip sampler, and WebGPU renderer as the game.
By default, each accepted edit updates the running preview. There is no rebuild between pose edits.

The desktop studio connects to an LLM through the existing MCP server and local live bridge.
The LLM client must be able to run a local MCP server on the computer that runs the studio.
The game does not need an API key. You supply prompts through your LLM client.

## Start the studio

Run these commands from the repository root. Use the normal Linux setup in `START_HERE.md` first.

```sh
scripts/animation-studio.sh
```

The script builds both `shardfall` and `pav`, then opens the studio.
To select the first animation:

```sh
scripts/animation-studio.sh QUATERNIUS/Idle_Loop
```

If the programs are already built:

```sh
./target/debug/shardfall --animation-studio
```

On Windows, run the provided binaries from a command prompt:

```bat
shardfall.exe --animation-studio
```

If image tools cannot start Vulkan on Windows, they try DirectX 12.

For a native Windows source build, use `scripts/animation-studio.ps1` from PowerShell.
For a Windows cross build on Linux, use `scripts/build-windows.sh`.
After that build, `python3 scripts/package-animation-studio.py` makes a ZIP with both
programs, a double-click launcher, an MCP configuration example, and this guide.

`--animation-studio` opens the live bridge at `127.0.0.1:7878`.
Use `--bridge 127.0.0.1:7879` if another instance uses the default port.
The window shows the actual bridge address. **Esc → Animation Studio** also opens a preview during a game.

## Connect your LLM

Keep the studio open. Configure your MCP client with the server in `.mcp.animation-studio.json`.
Use the repository root as the server's working directory.
This configuration starts:

```sh
cargo run -q -p pav_tools --bin pav -- mcp --live
```

The equivalent configuration for prebuilt programs is:

```json
{
  "mcpServers": {
    "shardfall": {
      "command": "/absolute/path/to/pav",
      "args": ["mcp", "--live"]
    }
  }
}
```

On Windows, set `command` to the absolute path of `pav.exe`.
Use the path format required by your MCP client. For a different bridge port, append its address after `--live`.
Restart or reconnect the MCP server after you change its configuration.

The repository's original `.mcp.json` keeps a separate headless session.
Use the live configuration when you want tool calls to change the window you are watching.
The local bridge must run on the same computer as the MCP process, unless you supply another reachable bridge address.

Try this prompt:

> Find an idle animation. Copy it into the workshop as Greeting. Open its live preview.
> Add a right-handed wave over two seconds. Keep the feet still. Show me the result at half speed.
> Inspect the clip before each edit and use its revision. Capture a filmstrip so you can check the motion.

The LLM can also start from a neutral pose:

> Create a new animation named Bow from the rest pose. Make it last two seconds.
> Bend the chest forward, lower the head, hold briefly, and return to the start pose.
> Show each accepted edit in the live preview. Keep the final pose equal to the first pose.

## View and control an animation

Use **Animation library** to search the loaded clips. Select a clip, then select **View selected clip**.
The **Procedural moves** section previews attacks and gestures from `anim/moves.toml`.

| Control | Function |
|---|---|
| Play / Pause | Start or stop the preview clock |
| Time slider | Pause and select an exact time |
| −1 frame / +1 frame | Step at the animation's reported frame rate |
| Previous key / Next key | Select the adjacent stored key pose |
| Speed | Change preview speed without changing saved animation data |
| Repeat | Repeat the preview, including clips that normally play once |
| Mirror | Preview the opposite side without changing saved data |
| Upper body | Apply a clip to the chest, head, and arms |
| Root motion | Show recorded body travel |
| Fit camera | Frame the full motion after a large pose change |
| Right-drag / mouse wheel | Rotate / zoom |
| F6 / F7 | Pause or play / step one frame |
| F8 / F9 | Decrease / increase preview speed |
| F12 | Save a window screenshot |

The stage has its own clock. It leaves the game, physics, recording, and rewind history unchanged.
**Close preview and resume game** returns to the saved game state and camera.

The preview reads the current animation on each frame, including while it is paused.
Editing the selected clip keeps the current camera, time, and playback controls.
Selecting a different clip starts that clip at its beginning and fits the camera.

## Create and edit clips

Enter a new name and select **Create from rest pose** or **Copy selected clip**.
The original library clip stays available. The new clip becomes the active preview.

Each accepted edit saves automatically. Use **Undo** or **Redo** to move through the last 32 edits of that clip.
This history remains available after the programs restart.
Undo of a new clip removes that clip. Redo restores it.
**Mirror clip** changes the saved data. **Duration factor** changes the saved duration and key times.
A factor of `0.5` makes the animation twice as fast.

Pause the preview to edit a particular moment. **Insert key at cursor** stores the sampled pose at that time.
**Pose channels** accepts a JSON object containing only the channels to change.
For example:

```json
{"armR": [80, 20, 50, 25, 0], "head": [15, 0, 0]}
```

Unspecified channels keep their sampled values. L and R refer to the subject's own sides.
The inspector tools return the complete channel legend. Times use seconds; angles use degrees.
Positions use percentages of standing hip height. Limb directions use `[forward, out, up, bend, twist]`.
Use `null` to remove an optional channel, such as `"blade": null` or `"root": null`.
Required channels, such as `hips` and `armR`, must contain number arrays.

Motion clips use the engine's biped format. The studio can also preview procedural moves.
Edit procedural move definitions in `anim/moves.toml`; their existing data format stays unchanged.

## LLM tool reference

All tools work through MCP, the live bridge, and the headless CLI.

| Tool | Purpose |
|---|---|
| `clips` | Search clips and inspect source information |
| `anim_edit` | Create, copy, inspect, change, undo, and redo clips |
| `anim_preview` | Open, control, and inspect the current stage |
| `capture` | Return the current stage as a PNG image |
| `filmstrip` | Return several stage frames in one PNG image |
| `animsheet` | Sample a named animation at regular times without changing the preview |
| `clip_import` / `mocap` | Translate supported source animation formats into readable clips |

### `anim_edit`

| Action | Main arguments | Result |
|---|---|---|
| `create` | `name`, `duration`, `loop` | New clip with neutral start and end poses |
| `copy` | `name`, `from` | Copy of an existing `SET/Clip`, with source credits |
| `inspect` | `name` | Full clip data, legend, source, revision, and history counts |
| `key` | `name`, `time`, `pose` | Insert or replace a key; `{}` inserts the sampled pose |
| `delete_key` | `name`, `time` | Remove the key at that stored time |
| `replace` | `name`, `clip` | Replace complete clip data and retain its source information |
| `retime` | `name`, `factor` | Scale duration and key times; keep locomotion distance consistent |
| `mirror` | `name` | Swap the saved left and right motion |
| `undo` / `redo` | `name` | Restore the preceding or following edit |

Use the returned canonical `name` in later calls. New clips normally use `WORKSHOP/Name`.
Copies of local source clips use `WORKSHOP_LOCAL/Name`.
Imported sets cannot be overwritten with `anim_edit`; copy the source first.

Every result includes `revision`. Pass this string as `if_revision` on the next edit.
If the file changed in the meantime, the tool returns a conflict and keeps the current file.
Inspect again, then apply the requested change to the new version.
`preview=false` saves an edit without selecting it in the stage.

The response contains the complete `clip` object. Its duration field is `dur`, and its repeat field is `loop`.
Use that object with `action=replace` for a complete update. This action retains `src`, `orig`, and `take`.
Use `action=key` for a partial pose update.

| Response field | Meaning |
|---|---|
| `name` | Full name to use in later calls |
| `file` | Saved set path; `null` for an imported source |
| `revision` | Clip content identifier for `if_revision` |
| `saved` / `changed` | Whether this call saved a file and changed the clip |
| `undo` / `redo` | Available history entries for this clip |
| `source` / `credit` | Source information and set credit |
| `clip` / `text` | Structured clip data and the readable clip text |
| `legend` | Channel names, axes, and units |

Undo of clip creation returns `clip: null` and `revision: "absent"`.
Keep the returned `name` to restore the clip with `action=redo` and `if_revision="absent"`.
If preview selection fails after a successful save, the response includes `preview_error`.
The saved animation remains available.

The authoring validator checks times, channel names, array lengths, finite values, and limb directions.
It rejects invalid edits before changing the animation file.
The saved format uses milliseconds, so distinct keys must remain at least one millisecond apart.
Authoring supports up to 4,096 keys and 600 seconds per clip.
Some imported clips have a final interpolation key after their stated duration.
Copying such a clip replaces that tail with the pose at its exact end time.
The source remains unchanged. The copy keeps the original duration and motion within the saved format's precision.

### `anim_preview`

Set `clip="SET/Clip"` or `move="roundhouse"` to select an animation.
Use `playing`, `time`, `step`, `speed`, `repeat`, `mirror`, `upper`, and `travel` to control it.
`step` is a signed number of frames at the reported `fps`. Seeking or stepping pauses unless `playing` is supplied.

`action="pose"` returns the sampled readable key, joint positions, and body bounds.
`action="fit"` fits the camera to the full animation. `close=true` returns to the game.
A call with no arguments returns the current preview status.
Opening a saved `WORKSHOP/Name` or `WORKSHOP_LOCAL/Name` also works after the tool process restarts.

### Example calls

From a second terminal, connect an interactive REPL:

```sh
./target/debug/pav live
```

Enter one command per line:

```text
clips find=idle
anim_edit action=copy from=QUATERNIUS/Idle_Loop name=Greeting
anim_preview playing=false time=0.5
anim_edit action=inspect name=WORKSHOP/Greeting
```

REPLs also accept complete JSON lines. This keeps spaces and nested pose arrays intact:

```json
{"tool":"anim_edit","args":{"action":"key","name":"WORKSHOP/Greeting","time":0.5,"pose":{"armR":[80,20,50,25,0]}}}
{"tool":"anim_preview","args":{"playing":true,"speed":0.5}}
{"tool":"filmstrip","args":{"frames":8,"every":6,"width":320,"height":320,"out":"out/greeting.png"}}
```

For a complete sample that creates a wave from scratch:

```sh
./target/debug/pav live --stop-on-error < scripts/examples/animation-wave.jsonl
```

The sample creates `WORKSHOP/HelloWave`. It stops before later edits if that clip already exists.
Use another name to run it again. Keep `--stop-on-error` when running command files.
For a headless run, replace `live` with `repl`. `capture` and `filmstrip` use the same renderer in both modes.

## Files and hot reload

| Path | Content |
|---|---|
| `anim/workshop.json` | Authored clips and copies of distributable sources |
| `anim/local/workshop_local.json` | Copies of local sources that must stay local |
| `anim/.editor/` | Per-clip undo and redo history; ignored by Git |
| `anim/local/.editor/` | History for local source clips; ignored by Git |

Set `PAV_ANIM` to use another animation directory. Run the game and tools against the same directory.
The native file watcher reads changed animation files after a short delay.
It replaces only the changed set, so other loaded libraries remain available.
Invalid files keep the last valid live animation and produce an error message.
You can correct the file and save it again.

An external file edit can make the saved editor history stale. Undo and redo refuse that history.
The next accepted tool edit starts a new history from the changed file.
Source credit and licence fields stay with copied animations.
Keep `anim/local` out of commits, as required by the repository's existing asset rules.

The browser build can display the animation workspace with `?animation_studio=true`.
Local file edits and the TCP/MCP live bridge require the desktop program.

## Code map

- `pav_core::animation_edit`: pure validation and pose operations.
- `pav_core::animation_preview`: preview state, clock, sampling, and stage frames.
- `pav_core::clips`: atomic in-memory set replacement and endpoint sampling.
- `pav_tools::animation_tools`: file updates, source retention, revisions, and undo/redo.
- `pav_tools::preview_tools`: shared preview controls and pose inspection.
- `pav_app::animation_ui`: native controls that call the registered tools.
- `pav_app::animation_watch`: file changes installed into the running library.

There is no separate animation renderer and no new external dependency.

## Check the live workflow

Build the native app and tools, then run this check with a display available:

```sh
cargo build --locked -p pav_app -p pav_tools
python3 scripts/smoke-animation-studio.py
```

The script starts the actual desktop studio and a live MCP process. It changes a paused
pose, captures the result, checks revision conflicts and undo/redo, reloads an external
file edit, and opens the saved animation in a new tool process. It uses a temporary
animation directory. Images and the result list go to `out/animation-studio-smoke`.
