# Object Studio

Objects (props) are JSON documents of named parts (the live studio: ASSET_STUDIO_LIVE.md). A
command is one line, `tool key=value ...` or `{"tool":"…","args":{…}}`; keep JSON on its one line
(a document over several lines arrives as broken commands).

## The loop

1. `assets [find=words]` lists templates (BUILTIN/box, crate, bench, lantern) and saved objects
   (WORKSHOP/name).
2. `asset_edit action=create name=x asset={…}` makes an object from a whole document.
3. `asset_edit action=patch name=x if_revision=R ops=[…]` changes it (R: the last reply's
   revision). Each reply's check lists what that edit got wrong.
4. `asset_preview name=x views=true figure=true` draws a sheet. Send it with the edit it checks;
   a create and its sheet, then one fix batch and its sheet, is often the whole job.
5. `asset_spawn action=add name=x pos=[x,y,z]` places a copy in the world.

## A document

```json
{"format":2,"name":"bench","unit":"cm",
"description":"A wooden bench: broad seat, four legs, backrest leaning 12 degrees back on the rear legs.",
"materials":{"wood":{"color":"#bc8150"},"wood_dark":{"color":"#594635"}},
"parts":{
"seat":{"type":"rounded_box","size":[220,18,76],"radius":2.5,"pos":[0,80,0],"mat":"wood"},
"backrest":{"type":"rounded_box","size":[220,8],"radius":2.5,"from":[0,92,-38],"to":[0,141,-48],"mat":"wood"},
"leg_front":{"type":"box","size":[10,8],"from":[85,0,25],"to":[85,71,25],"mat":"wood_dark","mirror":"x"},
"leg_back":{"type":"box","size":[10,8],"from":[85,1,-26],"to":[85,141,-56],"mat":"wood_dark","mirror":"x"},
"crossbar":{"type":"box","size":[6,6],"from":[-80,31,-33],"to":[80,31,-33],"mat":"wood_dark"}},
"sockets":{"sit":{"pos":[0,89,4]}}}
```

Centimetres; x right, y up, z forward (+z is the front); the origin is on the floor under the
centre. Part names: lowercase letters, digits, _ and -.

- **Place** a part by pos (its centre); by from/to, the visible ends of a box, rounded_box,
  cylinder, cone, tube or capsule (a box then takes size [across, thick]); or by attach:"seat"
  (on its top; {"to":"seat","at":"front"}: top, bottom, front, back, left, right) plus
  offset:[x,y,z]. yaw, pitch, roll are degrees about y, x, z: +yaw turns +z toward +x, +pitch
  tips +z down. Round parts stand along y; axis:"x" lays one along x.
- **Shapes** (type; full sizes): box size; rounded_box size, radius; sphere radius; ellipsoid
  size; cylinder radius, height; cone radius, radius_end, height; capsule radius, height; tube
  radius, thickness, height; torus radius, minor_radius; wedge size, peak (-1 back … 1 front, 0 a
  gable); lathe profile [[r,y],…]; extrude points [[x,y],…], depth. Round shapes take arc
  (degrees) and facing for part of a circle.
- **Colour.** Each part needs color (#rrggbb or a role: $floor $floor2 $wall $pillar $accent
  $light) or mat, a key in materials ({color, look, emissive}). look: cel (default), flat, lit,
  unlit, cutout. emissive 0-32 glows; above 0.5 it lights the room.
- **Groups.** parts:{…} in a node makes a group: its children sit in its frame and inherit mat,
  color, look, emissive and solid (the nearest level wins).
- **Copies.** mirror:"x" (or ["x","z"]) adds the reflection; array:{count, step:[x,y,z]} a row;
  grid:{count:[a,b], step:[sa,sb]} a floor grid (plane:"xy": a wall); radial:{count, radius} a
  ring round the part's position, each copy facing out (axis:"z", arc:180, start:-90: an arch).
  All are centred; cycle:[…] gives copy i entry i mod n (materials or colours). Copies are named
  leg[0], mirror copies leg~x.
- **More.** solid:false: not in the collider; hidden:true: collision only. light:{color, radius,
  intensity} and fx:{preset} (fire, smoke, sparks, embers, steam, magic …) sit at the part.
  chance, jitter and a group's pick:n vary copies by seed. sockets: sit, grip, grip2, tip, top,
  hang, wall_mount. {"ref":"BUILTIN/crate","pos":[…],"scale":0.5} reuses an asset.

Limits: 256 nodes, 1,024 parts after copies, 4 lights. `asset_edit action=schema [topic=node]` is
the field guide (also tags, category, spin, sway, segments, half, half_height, remap, source, fit,
mesh, recolor; material base, roughness, metallic, map, turn, layers).

## Edits

```json
{"tool":"asset_edit","args":{"action":"patch","name":"x","if_revision":"prop2-…","ops":[
 {"op":"set","part":"seat","fields":{"mat":"wood_dark","size":[220,20,76]}},
 {"op":"add","part":"arm","value":{"type":"box","size":[6,20,60],"pos":[-104,99,0],"mat":"wood","mirror":"x"}},
 {"op":"remove","part":"crossbar"}]}}
```

Ops target part (roof.slab in a group; parts:[…] for several), material or socket. set merges
fields ("array.count":8 reaches in, null deletes, a new type replaces the shape); add takes a
whole part; also remove, rename, copy (to), mirror, array, expand (copies become parts), move
(by), scale (by), unit (to). One bad op rejects the batch. action=replace sends a whole document;
undo, redo; dry_run:true saves nothing; a stale if_revision is refused. Replies carry revision,
size_cm ("220x142x98": width x height x depth), changed_parts and new check findings. Errors
name the place and a fix: `ops[0] seat.mat: no material "wod" (expected wood, wood_dark) — did
you mean "wood"?`. action=inspect returns the document (resolved=true: every part after copies).

## Check

`asset_edit action=check name=x [target=WxHxD]` (cm) lists problems and a cost line: floating,
below_floor, not_grounded, poke (through another part), hidden (inside another), z_fight
(coinciding faces), mirror (an unmirrored pair), size (against target), tiny, unit_suspect and
the budgets heavy, meshes, lights. A hang socket hangs the object from it; wall_mount puts its
back (z 0) on the wall.

## Sheets and copies

views=true shows front, right, back and 3/4; views=[front,top,30] picks sides (left, top,
degrees); elevation= the camera angle (20, or game); tile= pixels per view (240).
`asset_spawn action=add name=x pos=[3,0,2] [yaw=90] [scale=1] [collide=false] [seed=3]` returns an
id for update and remove; edits refresh placed copies.

## Files

Saves go to assets/props/workshop/name.json. A metre document that format 1 holds is saved as
format 1 (shape:{type,…} per part), like the templates; a copy of one is in metres, so send
{"op":"unit","to":"cm"} before centimetres. PAV_ASSETS moves the root; hand edits reload.
