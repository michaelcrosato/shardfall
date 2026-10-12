# Object Studio

Objects (props) are readable JSON made of named primitive parts. Four tools find, edit, view and
place them, the same from the CLI, `pav repl`, MCP and the live studio, where people watch each
accepted edit (`ASSET_STUDIO_LIVE.md`: starting it, connecting a client, frame feedback). A
command is one line: `tool key=value ...`, or `{"tool":"…","args":{…}}` when a value has spaces
or is JSON. The examples below wrap; send each command on one line.

## The loop

1. `assets [find=words]`: rows of name, size, parts and description. Templates are `BUILTIN/box`,
   `BUILTIN/crate`, `BUILTIN/bench` and `BUILTIN/lantern`; saved objects are `WORKSHOP/name`.
2. `asset_edit action=inspect name=BUILTIN/bench`: a definition, defaults left out.
3. Make the object in one call: `asset_edit action=create name=x asset={…}` with a whole
   definition, or `action=copy from=BUILTIN/bench name=x`.
4. Change it with `asset_edit action=patch name=x if_revision=R ops=[…]`, where R is the last
   reply's `revision`.
5. Every edit reply lists under `check` what that edit got wrong; fix it in the next batch.
6. Look at it: `asset_preview name=x views=true figure=true`. Send an edit and its sheet together.
7. `asset_spawn action=add name=x pos=[x,y,z]` places a copy in the world.

## Definition (format 1)

```json
{"format":1,"name":"marker","description":"A glowing marker on a post.","parts":{
 "post":{"shape":{"type":"cylinder","half_height":0.6,"radius":0.05},"pos":[0,0.6,0],"color":"#26313e"},
 "light":{"shape":{"type":"sphere","radius":0.16},"pos":[0,1.3,0],"color":"#79debd","emissive":2,"solid":false}}}
```

Metres: x right, y up, z forward (+z is the front). The origin is the placement point, normally on
the floor under the object's centre. `pos` is a part's centre.

| field | meaning (default) |
| --- | --- |
| `shape` | `box` `half:[x,y,z]` (half extents: a 1 m cube has 0.5); `rounded_box` adds `radius`; `sphere` `radius`; `cylinder` and `capsule` `radius` and `half_height` along the part's y. A cylinder is 2·half_height tall, a capsule 2·(half_height+radius). |
| `pos` | `[x,y,z]`, the part's centre (`[0,0,0]`) |
| `yaw`, `pitch`, `roll` | degrees about y, then x, then z (0): +yaw turns +z toward +x, +pitch tips +z down |
| `color` | `#rrggbb` (`#e8704a` orange: set it) |
| `look` | `cel`, `flat`, `lit`, `unlit` or `cutout` (`cel`) |
| `emissive` | glow 0-32 (0); above 0.5 the part also lights its surroundings |
| `solid` | part of a placed copy's collider (`true`) |

Part names are lowercase letters, digits, `_` and `-`. 1-256 parts; sizes 0.001-1000 m.
`asset_edit action=schema` is the full field guide (`topic=shapes` returns one section).

## Edits

```json
{"tool":"asset_edit","args":{"action":"patch","name":"x","if_revision":"prop1-…","ops":[
 {"op":"set","part":"seat","fields":{"color":"#3e9a9e","pos":[0,0.5,0]}},
 {"op":"add","part":"arm_left","value":{"shape":{"type":"box","half":[0.05,0.1,0.4]},"pos":[-1,0.6,0],"color":"#26313e"}},
 {"op":"remove","part":"crossbar"}]}}
```

`set` merges the listed fields (a `shape` replaces the whole shape); `add` takes a new part name
and a whole part; `remove` deletes one. A batch is checked whole: one bad op rejects it and
nothing changes. `replace` swaps in a whole definition; `undo` and `redo` step through the last 32
batches; `dry_run:true` returns the reply, with `revision_next`, without saving. Format-2
objects also take `rename`, `copy`, `mirror`, `array`, `expand`, `move`, `scale` and `unit`, and
address groups, materials and sockets (`asset_edit action=schema format=2 topic=ops`).
`action=inspect resolved=true` lists every drawn part after copies and mirrors.

Replies carry `name`, `revision`, `saved`, `size_cm` (`"220x143x86"`: width x height x depth),
`undo`/`redo` counts, `changed_parts`, and `check` when the edit introduced problems. A stale
`if_revision` is refused: inspect again. Errors name the place, what is valid and a fix:

```
ops[2] seat.color: "grey" is not a #rrggbb colour — use a hex colour such as "#808080"
```

## Check

`asset_edit action=check name=x [target=WxHxD]` (cm) lists every problem, one line each, plus a
`cost` line (triangles, meshes, lights). Codes: `floating` (not joined to the rest or the floor),
`below_floor`, `not_grounded` (nothing reaches the floor), `poke` (through another part's
surface), `hidden` (inside another part), `z_fight` (faces that coincide and flicker), `mirror` (a
left/right pair that is not mirrored), `size` (against `target`), `tiny`, `unit_suspect`
(centimetres typed as metres), `default_color`, and the budgets `heavy`, `meshes` and `lights`.
An object whose top is at its origin hangs from it; one on a wall (its back at z 0, off the
floor) is joined to the wall, and `not_grounded` only notes that it is off the floor.

## Sheets

`asset_preview name=x views=true` returns one 960×240 image of the front, right, back and a 3/4
view, each framed to fit. `views=[front,top,30]` picks the sides (`left`, `top`, or degrees round
from the front); `elevation=` is the camera's angle above the horizon (20; `game` is the game's
camera); `tile=` the pixels per view (240); `figure=true` adds a 1.8 m figure for objects 0.5-20 m
in size. `capture stats=only` reports what the current view costs to draw.

## Placing copies

`asset_spawn action=add name=x pos=[3,0,2] [yaw=90] [scale=1] [collide=false]` returns the copy's
`id`; `action=list`, `update id=… pos=… yaw=…` and `remove id=…` manage copies. `seed=3` picks a
variant (chance, jitter, pick) and `roles={"$wall":"#5a6b7c"}` colours its theme roles. A copy
moves, collides and deletes as one object. Accepted edits refresh every placed copy; snapshots keep
each definition they were saved with, once.

## Files

Templates are `assets/props/templates/*.json`, built in. Saves go to
`assets/props/workshop/name.json` in one canonical layout (a header line, one line per part);
`PAV_ASSETS` moves the props root. Hand edits load too: the live studio reloads a changed file
after 250 ms of quiet, and a bad file keeps its last valid definition.
