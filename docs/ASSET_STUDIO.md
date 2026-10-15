# Object Studio

Objects (props) are JSON documents of named parts. A command is one line, `tool key=value ...`
or `{"tool":"…","args":{…}}`; keep it on that line, document and all: a line break splits it
into broken commands.

## The loop

1. `assets find=words` finds a starting point: templates (BUILTIN/box, crate, bench, lantern),
   library packs (LIB/pack/name, read-only: copy one; imported packs: docs/ASSET_IMPORT.md) and
   saved objects (WORKSHOP/name). Rows are name cat size_cm tier mesh tok tags desc, best first
   (tier: standard, checked or disk; mesh: % read-only mesh); a miss says `none` with a route;
   filters `category=`, `tags=` (`@sit`: a socket), `tier=standard` (what every build has),
   `similar=NAME`; `next` pages.
2. Make the object and look at it in one go: a create command like the one below and, on the
   next line, `asset_preview name=x views=true figure=true`. The create reply has size_cm and
   check (what is wrong); the preview writes a sheet of four views beside a 1.8 m figure, the
   image to look at (the reply's path; no capture needed).
3. If the check or the sheet shows a fault, fix it in one more go: a patch (below) and a new
   sheet. Otherwise stop; the first go is often the whole job.

## A document

```json
{"tool":"asset_edit","args":{"action":"create","name":"bench","asset":{"format":2,"name":"bench","unit":"cm","description":"A wooden bench, its backrest leaning on the rear legs.","materials":{"wood":{"color":"#bc8150"},"wood_dark":{"color":"#594635"}},"parts":{"seat":{"type":"rounded_box","size":[220,18,76],"radius":2.5,"pos":[0,80,0],"mat":"wood"},"backrest":{"type":"rounded_box","size":[220,8],"radius":2.5,"from":[0,92,-38],"to":[0,141,-48],"mat":"wood"},"leg_front":{"type":"box","size":[10,8],"from":[85,0,25],"to":[85,71,25],"mat":"wood_dark","mirror":"x"},"leg_back":{"type":"box","size":[10,8],"from":[85,1,-26],"to":[85,141,-56],"mat":"wood_dark","mirror":"x"},"crossbar":{"type":"box","size":[6,6],"from":[-80,31,-33],"to":[80,31,-33],"mat":"wood_dark"}},"sockets":{"sit":{"pos":[0,89,4]}}}}}
```

Centimetres; x right, y up, z forward (+z is the front); the origin is on the floor under the
centre. Part names: lowercase letters, digits, _ and -.

- **Place** a part by pos (its centre); by from/to, the visible ends of a box, rounded_box,
  cylinder, cone, tube or capsule (they set its length: a box takes size [across, thick]); or by
  attach:"seat" (on its top; {"to":"seat","at":"front"}: top, bottom, front, back, left, right)
  plus offset. yaw, pitch, roll: degrees about y, x, z; +yaw turns +z toward +x, +pitch tips +z
  down. Round parts stand along y; axis:"x" lays one along x.
- **Shapes** (type; full sizes): box size; rounded_box size, radius (at most half the thinnest
  side); sphere radius; ellipsoid size; cylinder radius, height; cone radius, radius_end, height;
  capsule radius, height; tube radius, thickness, height; torus radius, minor_radius; wedge size,
  peak (-1 back … 1 front, 0 a gable); lathe profile [[r,y],…]; extrude points [[x,y],…],
  depth. arc (degrees) keeps part of a round shape, its middle toward facing (x, y, z, -x, -y,
  -z).
- **Colour.** Each part needs color (#rrggbb or a role: $floor $floor2 $wall $pillar $accent
  $light) or mat: a key in materials ({color, look, emissive}, or a patterned library material
  such as "MAT/oak_planks"), or a library name itself. Patterns, the library and your own
  materials: docs/MATERIALS.md. look: cel (default), flat, lit, unlit, cutout. emissive 0-32
  glows; above 0.5 it lights the room.
- **Groups.** parts:{…} in a node makes a group: its children sit in its frame and inherit mat,
  color, look, emissive and solid.
- **Copies.** mirror:"x" (or ["x","z"]) adds the reflection; array:{count, step:[x,y,z]} a row;
  grid:{count:[a,b], step:[sa,sb]} a floor grid (plane:"xy": a wall); radial:{count, radius} a
  ring round the part's position, each copy facing out (axis:"z", arc:180, start:-90: an arch).
  All are centred. cycle goes inside one (array:{count, step, cycle:["wood","wood_dark"]}) and
  gives copy i entry i mod n. Copies are named leg[0], mirror copies leg~x.
- **More.** solid:false: not in the collider; hidden:true: collision only. light:{color, radius,
  intensity} and fx:{preset} (fire, smoke, sparks, embers, steam, magic …) sit at a visible part.
  chance, jitter and a group's pick:n vary copies by seed. sockets: sit, grip, grip2, tip, top,
  hang, wall_mount. {"ref":"BUILTIN/crate","pos":[…],"scale":0.5} reuses an asset.
  {"mesh":"kenney_mini_dungeon/chair"} is a mesh part `asset_import` wrote: place, scale, mirror,
  recolor:{"0":"#rrggbb"} or solid:false it; never edit its file.

Limits: 256 nodes, 1,024 parts after copies, 4 lights, 16 mesh parts. `asset_edit action=schema [topic=node]` is
the field guide (also tags, category, spin, sway, segments, half, half_height, remap, source, fit,
mesh, recolor; material base, roughness, metallic, map, turn, layers).

## Edits

```json
{"tool":"asset_edit","args":{"action":"patch","name":"x","if_revision":"prop2-…","ops":[{"op":"set","part":"seat","fields":{"mat":"wood_dark","size":[220,20,76]}},{"op":"add","part":"arm","value":{"type":"box","size":[6,20,60],"pos":[-104,99,0],"mat":"wood","mirror":"x"}},{"op":"remove","part":"crossbar"}]}}
```

if_revision is the last reply's revision. Ops target part (roof.slab in a group; parts:[…] for
several), material or socket: set merges fields ("array.count":8 reaches in, null deletes); add
takes a whole part; also remove, rename, copy (to), mirror, array, expand (copies become parts),
move (by), scale (by), unit (to). One bad op rejects the batch; dry_run:true saves nothing; undo,
redo. create refuses an existing name: action=replace resends a whole document, a patch just
the change.
Replies carry revision, size_cm (width x height x depth), changed_parts and new findings.
action=inspect returns the document (resolved=true: every part after copies).

## Check and the rest

`asset_edit action=check name=x [target=WxHxD]` (cm) lists all findings and a cost line. Faults:
floating, below_floor, not_grounded, unit_suspect, size; judge on the sheet: poke, hidden,
z_fight, mirror, tiny. A hang socket hangs the object from it; wall_mount puts its back (z 0) on
the wall. Sheets also take views=[front,top,30], elevation= and tile=.
`asset_spawn action=add name=x pos=[x,y,z]` places a copy in the world. Saves go to
assets/props/workshop/name.json; a metre template's copy is in metres: send
{"op":"unit","to":"cm"} before centimetres. `copy from=X` writes the fork's lineage:
`"lineage":{"method":"edited","origin":"import","parents":["X@rev"],"license":"CC0-1.0","credit":"…"}`,
with X's origin (`import` for an import, `agent` for own work), licence and credit; edits and
publish keep it, a patch cannot set it, and a copy of a format-1 template stays format 1 without one.

## Names and tiers

BUILTIN/name (templates) and LIB/core/name are embedded in every build, the browser's too: game
data and rooms may name only these. Other packs (assets/props/lib/pack/) load on first use in
native builds; assets/props/local/ (packs, and WORKSHOP_LOCAL/name copies of their assets) is
git-ignored, for sources whose licences forbid sharing. Each pack folder has an index.tsv that
`assets` reads; `assets reindex=pack` (or `=templates`) rewrites it, keeping its note column.
A pack's PACK.json holds its licence (lib/ takes CC0 and CC-BY, lib/core CC0 only); reindexing a
pack rewrites assets/props/CREDITS.md.

`asset_edit action=publish name=x if_revision=R review="ok: …"` puts a workshop asset into LIB/core
after you look at its sheet (views=true elevation=game) at revision R. It needs no check findings, a description (≤ 120
chars), a category, 2-6 tags, references to BUILTIN or LIB/core only, core or inline materials
(not MAT/workshop/), and at most 400 tokens
(food, tool, signage), 600 (container, lighting, rock, vegetation), 800 (most) or 2,000 (town,
dungeon). It writes lib/core/x.json, its index row and a review.tsv line at this revision (an
edited core asset needs a new review); rebuild to embed it.

## Library assets in game code

A game that keeps its own state draws an asset for a unit or a piece of scenery each frame with
one call, in its view code while the frame's scene is built (as `pav_view/src/arpg.rs` draws
Shardfall):

```rust
use pav_view::assets::{emit_asset, Palette, Placement};
let at = Placement::at(unit.pos).yaw(unit.heading_deg).id(unit.id); // .scale(1.5), .rot(quat)
emit_asset(scene, "BUILTIN/crate", at, unit.id % 8, &Palette::new())?; // any name assets lists
```

It draws what a placed copy draws (parts, glow, lights; not `fx` particles). The seed picks the
variant (0 as authored; keep to a few seeds per asset); the palette colours `$roles`
({"$wall":"#5a6b7c"}); `id` keeps each unit's outline its own. Unknown names fail with the
nearest one. Units need no physics: the game owns their positions.

Scenery that never moves can instead be baked into the level's static regions
(`statics::StaticProp`: no entity, one collider box, ~75 B in a snapshot); Shardfall dresses
its rooms this way from `game/props.toml` (docs/GAME.md, "Dressing").
