# Materials

A material is a surface recipe drawn per pixel: a base colour and look plus up to 3 pattern
layers (planks, brick, cobbles, noise …). No images, no UVs. Props wear them by name.

## Names

- Core, read-only: MAT/oak_planks, barrel_staves (oak planks upright), brick_wall, cobblestone,
  flagstones ($floor), mossy_stone, roof_shingles, awning_stripes, wrought_iron, gold_trim,
  pb/gold, pb/iron. `assets kind=material [find=wood]` lists them with layers and cost.
- Yours: MAT/workshop/x, saved as assets/materials/workshop/x.json.

## On a prop

```json
"materials":{"wood":"MAT/oak_planks","iron":"MAT/wrought_iron","glow":{"color":"#ffcc66","emissive":3},
 "rough":{"base":"MAT/cobblestone","scale":0.5,"map":"world"}},
"parts":{"lid":{"type":"cylinder","radius":30,"height":4,"pos":[0,92,0],"mat":"wood"},
 "band":{"type":"tube","radius":31,"thickness":1,"height":5,"pos":[0,80,0],"mat":"MAT/wrought_iron"},
 "stain":{"type":"box","size":[20,1,20],"pos":[0,0.5,0],"mat":"wood","color":"#6a4a2a"}}
```

- materials: key → a library name, or a material written here (lengths in the asset's unit).
- mat on a part, a group or the document: a key, or a library name directly. Groups pass it on;
  the nearest mat decides, so a part wearing its own never takes an outer material's look or glow.
- A part's own color keeps the patterns and replaces the base colour (stain above).
- How patterns are laid (map): object (the part's own frame; default), axis (round parts:
  cylinder, cone, capsule, tube, lathe: around and along the axis, so staves run up a barrel),
  world (world space: neighbouring parts join seamlessly). A material's map overrides.
- `check` reports unknown_material: an unknown name draws magenta until fixed.

## A material

| field | meaning |
|---|---|
| base | MAT/name to start from; fields here override, layers replace its layers |
| color | #rrggbb or a role ($floor $wall $accent …); required unless base |
| look | cel (default), flat, lit, unlit |
| emissive | 0-32 glow; above 0.5 lights the room |
| roughness, metallic | 0-1; metallic adds a highlight band |
| map | object, world or axis (above) |
| scale | 0.1-10, multiplies every layer size |
| turn | true: 2D patterns turn 90 degrees (planks upright) |
| layers | up to 3, bottom first: [{type, …}] |

Files add format 1, name (workshop/x), unit (cm), description, tags.

## Layers

Every layer: type; color (paints) or shade (darkens, 0.6 default; 1 = none); strength 0-1;
glow 0-8; facing up, down or side; masked_by prev or not_prev (only where the layer below
painted, or did not); seed. Sizes are feature sizes in the unit.

| type | params (defaults, cm) | cost |
|---|---|---|
| planks | size [120,20], gap 1.5, axis | 1 |
| brick | size [40,20], offset 0.5, gap 3 | 1 |
| tiles | size 50 or [w,h], gap 2 | 1 |
| cobbles | size 25 (flagstones 60-80), jitter 0.8, gap 3, bevel | 2.5 |
| shingles | size [30,20], shape round/square/point, gap 1.5 | 1.5 |
| voronoi | size 30, jitter 1, feature spots/edges/cells, cover 0.3, gap 2 | 2.5 |
| dots | size 15, radius 4, offset 0.5, jitter | 1 |
| checker | size 50 | 1 |
| stripes | size 30, cover 0.5, axis x (or [1,1,0]), soft | 1 |
| wave | size 20, axis, rings, distort, cover 0.5, soft 0.5 | 1.5 |
| wood | size 8, axis y, distort 1.5, cover 0.35, soft 0.4 | 1.5 |
| noise | size 50, detail 3 (1-4), distort, cover 0.5, soft 0.3 | 0.5 per detail |
| magic | size 50, depth 2, distort 1, cover 0.5, soft 0.3 | 1.5 |
| gradient | axis y, from 0, to 100 (cm above the part's bottom) | 1 |

Cellular patterns (planks, brick, tiles, cobbles, shingles, voronoi, dots) also take vary (each
cell's brightness), cells (2-4 colours, one per cell), cavity (dark joints) and bevel.

## Legibility

At the game camera one pixel is 1 cm at 1080p and 2 cm at 540p (phones). A layer fades to its
average colour below 3 pixels per feature and vanishes below 1. Keep features 5 cm or larger,
gaps 1.5 cm or wider, at most 3 layers and 6 cost units; edits warn otherwise. Put the shape
in parts and the surface in materials: a 2 cm rivet is a part, not a dot.

## Tools

1. `asset_edit kind=material action=create name=moss asset={"color":"#5a6a3a","layers":[…]}`
   (or `action=copy from=MAT/oak_planks name=dark_oak`). Replies: revision (mat1-…), layers,
   cost, check.
2. `asset_preview kind=material name=MAT/workshop/moss [compare=MAT/mossy_stone] [view=game]`
   writes a swatch (each row: sphere, cube, cylinder, 2×2 m world slab; 6 rows at most) and, per
   layer, its feature size, pixels at the game camera, how much of the surface it changes and a
   verdict (clear, soft at 540p, 1080p only, fades to flat, invisible, barely shows). The
   image is 960×540, so view=game shows what a 540p screen does.
3. Patch: `ops:[{"op":"set","fields":{"color":"#4a5a30"}},{"op":"set","layer":0,"fields":{"size":40}},
   {"op":"add","layer":{"type":"noise","size":20,"shade":0.8}},{"op":"remove","layer":1}]`
   with if_revision; null deletes a field; undo, redo, inspect, replace, schema as for props.

Edits save at once and every view redraws the material's colour and patterns; a change of its
look or emissive reaches a prop that wears it when the prop is next edited or loaded.

## Examples

```json
{"color":"#7a5a3a","layers":[{"type":"planks","size":[90,15],"gap":1.5,"vary":0.25,"cavity":0.3}]}
{"base":"MAT/brick_wall","color":"#6a6a70","scale":1.5}
{"color":"#3a4a5a","roughness":0.4,"metallic":1,"layers":[{"type":"noise","size":20,"cover":0.3,"color":"#8a4a2a","facing":"up"}]}
```
