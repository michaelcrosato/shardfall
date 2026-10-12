# Live Object Studio

The live studio shows each accepted object edit through Shardfall's normal renderer while a
person directs an LLM. The LLM client calls the engine through MCP; the native controls call the
same tools, with the same validation, saves and undo history. No model API key or render service
is needed. The tools and the object format are in `ASSET_STUDIO.md`.

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
> revision for each edit. Show me a contact sheet when the first version is ready, then wait
> for my next direction.

Useful follow-up prompts include “make the backrest taller,” “move both armrests out by
10 centimetres,” “undo the last change,” and “place two copies beside the entrance.”
The agent should ask for a world position or inspect the scene when a placement direction
does not identify a clear location.

## Frame feedback

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
4. Request a contact sheet at a visual checkpoint, or when the agent needs to inspect form.

Capture runs on a second GPU device and encodes an image. It costs more than an ordinary edit.
The normal edit path does not make a capture. File edits are also supported: native file
watching waits for 250 ms of quiet before it reads a changed file. A bad file reports an error
and leaves its last valid definition active. Tool saves publish directly and skip this delay.

## Placed copies in the game

Accepted definition changes refresh the visuals and collision for active and sleeping
instances of that asset. Their positions, scales, root IDs, and room assignments stay in
place. Fixed-object changes invalidate the game's cached navigation map. Existing native
F10 move and delete controls work on the whole object, including while the world is paused.

Instances live in the current scene and full snapshots. Ordinary hero saves are not a
level layout file. In a pavilion room, F10 Save writes `asset` and `scale` references in its
`[[object]]` entries. Keep the corresponding workshop JSON with the room. Snapshots embed
exact definitions, so loading an old snapshot does not silently use a newer library shape.
Later accepted edits can refresh its instances again.

## Example and scope

Run the supplied example with the studio open:

```sh
pav live --stop-on-error < scripts/examples/object-lantern.jsonl
```

It creates `WORKSHOP/garden_lantern`. It stops if that name already exists. The Windows
package includes the same example at its top level.

Objects are procedural parts with surface controls, placed in scenes. Imported mesh models,
texture painting and a general level layout editor are future work (`ASSET_SYSTEM_PLAN.md`).
Animation authoring is in the same studio; see `ANIMATION_STUDIO.md`.

## Verification

`scripts/smoke-asset-studio.py` checks the real native window through MCP. It covers saved
editing, rendered changes, bad batches, revision guards, undo/redo, placed instance refresh,
file reloads, workspace switching, cold startup, contact sheets, and frame feedback. It uses temporary asset
folders and writes captures plus a JSON result to `out/asset-studio-smoke`.

The focused Rust tests cover schema validation, atomic persistence, snapshot definitions,
compound collision and streaming wake, room references, CPU scene building, paused picking,
and passive UI frames. The build checks also cover the native, Windows, and browser targets.
