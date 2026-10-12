# Agent-first asset system: plan

Status: **resumed in slim form on 2026-10-12.** Gate G2 was a NO-GO (gate log in §10.2); the
user then closed it under the prototype testing budget and kept only the Phase 3-6 features that
are needed and showed a benefit, with lean gates (§10.3). Started 2026-10-12
(§11 checklist; `docs/HISTORY.md` has each row). The executing session starts at §15 (prompt)
and resumes from the first unticked row of the progress checklist in §11. Line numbers cite commit `a9ca003` and drift as work lands; search by symbol.
Reference examples and prototypes from the planning work are in `docs/asset_plan/` (§14). A five-lens
adversarial review was started and stopped early for usage: fix any inconsistency you find in
this file in the PR that hits it.

## 0. Verdict

**Go, with conditions.** The goal is reachable for the look this engine actually has: stylised,
low-poly, cel-shaded props with procedural surfaces. Every claim below was measured in this
repo's headless engine or on downloaded library files during planning.

| claim | evidence |
|---|---|
| Today's format is valid but expensive | An agent wrote a 49-part market stall and a 56-part well from the docs alone, both valid on the first try. But 62-63% of each file was repetition or texture faked with geometry (30 hand-placed stones, 18 awning parts), about 65-75% of Kenney quality. |
| Format 2 cuts authoring tokens | Same assets with the same structure (o200k tokens): stall 2,581 → 746 (−71%), well 2,849 → 900 (−68%), bench 393 → 280 (−29%), greatsword 497 → 436 (−12%). It costs more on tiny assets: wall torch 383 → 405 (+6%). Two independent implementations agree on all 121 resolved parts within 0.005 units, and headless renders match intent. |
| Replies are bloated today | `inspect BUILTIN/bench` 1,463 → 416 tokens (−72%), patch reply 317 → 51, asset listing 106 → ~35 tokens per row; saved files −60%. No format change needed (Phase 0). |
| Procedural "textures" work on this renderer | Prototype: 14 patterns on 10 WGSL kernels. A Rust CPU mirror matches the GPU on 100% of pixels within 1/255. Cost at the game camera (llvmpipe, 960×540): +19% typical, +50% worst case (every surface 3 layers). The 8 example materials average 107 tokens per file, 70 inline. |
| Public libraries convert | Kenney, KayKit and older Quaternius packs (CC0, ~10k models) colour almost losslessly: 97.8-100% of triangles are reproducible as flat or vertex colour. Fit prototype on Kenney Mini Dungeon: median min-view silhouette IoU 0.959 at 260 tokens per asset, with 26% of area in read-only mesh parts (exact meshes would cost 2,082 tokens). |
| End to end | Replaying the dogfood stall session costs `tok_x` 10.8k tokens (50k with context re-reads). The same stall in format 2 projects to ~5.5k (~17.7k). |

**Conditions.**
1. The format-2 gate G2 (§10) must pass before Phases 3-6. *Superseded on 2026-10-12: the user
   closed G2 after its NO-GO and resumed Phases 3-6 in slim form (§10.3).*
2. Bulk import waits for the pilot GO (§8.8).
3. Materials stay within the frame budget and pass the browser shader check.

**Will not be reached:**
- faithful import of photoreal or scanned libraries (Poly Haven, ambientCG, Smithsonian): they become restyled references only;
- bitmap textures of any kind (doctrine P4);
- skinned character import.

KayKit-style bevelled packs stay mesh-heavy (with primitives only, median IoU 0.822).

**Stop rule.** Each gate has kill criteria (§10.2). If one holds after one remediation round,
the executing agent stops, records the numbers, and advises against continuing. Phases 0-1
are useful even if everything after them stops.

## 1. Goals and how they are measured

*Superseded on 2026-10-12 for Phases 3-6 by the slim targets in §10.3: the kept rows' objective
checks and budgets (the "still runs well" row below stands), a reviewed core library of ~30
exemplars, the pilot's objective GO, and the user's review pages instead of judged win rates.
The table is the original target, kept for the record.*

| goal | measure (final gate G4 unless noted) |
|---|---|
| fewer tokens | median per-task `tok_x` ≤ 0.70× baseline and `tok_cum` ≤ 0.50× (G2: ≤ 0.75 and ≤ 0.65); edit tasks ≤ 0.60× |
| higher quality | blind pairwise wins ≥ 22/32 vs baseline with rubric Δ ≥ +0.3 (G2: no worse than parity) |
| easier for agents | acceptance ≥ baseline; median failed calls ≤ 1; first create/copy valid |
| a large library of quality examples | ~520 curated exemplars in ~75 parametric families, plus thousands of CC0 translations on disk with stored fidelity; listing ≤ 40 tokens per row |
| imports close to source | pilot ≥ 80% in the `fit` tier, median IoU ≥ 0.90, median asset ≤ 2,500 chars |
| still runs well | materials typical ≤ +25% frame time, kit + dressing ≤ +15%, wasm ≤ +3 MB raw, embedded library ≤ 3 MB |

`tok_x` and `tok_cum` are defined in §10.1; baseline = arm A, the engine at `a9ca003`.

## 2. Foundation: what is kept and extended

The user's rule: keep what is established, extend it, and replace only where keeping it would
block a goal.

| existing piece | treatment |
|---|---|
| Object Studio props format 1 (`crates/pav_core/src/props.rs`): named parts, strict validation, atomic `{op,part,fields}` patches, FNV revisions, 32-step history (`asset_tools.rs edit_at`), live preview, `asset_spawn`, room `asset=` | **extended** into format 2, a strict superset. Every v1 file stays valid, keeps its `prop1-` revision, and is never rewritten on load |
| `assets` / `asset_edit` / `asset_preview` / `asset_spawn` tools, `kind=` routing, live feedback tickets | **extended**: new args and actions, `kind=material|mesh`. The only new top-level tool is `asset_import` |
| motion import pipeline (`pav_tools/src/mocap/*`, `clip_import`, `anim/catalogs`, licence tiers, `.cache/`) | **reused one for one** for models and textures: shared module, same catalog grammar, same tiers |
| renderer hooks (`MeshKey::Cone/Custom`, `SdfInstance` round cones, per-vertex colour, unused instance `params[3]`) | **used**: no new mesh pipeline, one material storage buffer |
| `pav_core/build.rs` embedding tables; anim tiers (embedded / on disk / local) | **extended** with `ASSET_DATA` and `MATERIALS` tables, same tiers |
| Creature Studio (SpawnForge sidecar) | **untouched**. Its Issue/did-you-mean format is ported to Rust (vendor code unchanged) |
| Shardfall themes, palette roles, statics region cache, puppet weapon span trick | **extended** as consumers (§9.6) |

Replaced pieces, each justified in its section:
- the typed serde reader as the only prop reader (no part ids in errors, no migrations, §5.1);
- the pretty writer that saves every default (3× file bloat, §7.1);
- per-instance definitions in snapshots (size growth, §5.8);
- the creature-ops merge in the `asset_edit` schema (advertises ops props reject, §7.1);
- the hard-coded six-kind `Builder::prop` match with its shared RNG stream (§9.6);
- the 2-colour checker floor, only where a theme sets a floor material (§6.5).

## 3. Decisions approved with this plan

Approving or executing this plan approves these defaults. The user can override any of them
before or during execution. Record each in `docs/HISTORY.md` when its phase lands; none of
them needs a doctrine escalation.

| # | decision | default |
|---|---|---|
| D1 | Format 2 and material files declare `"unit":"cm"` or `"m"`. **cm is the house unit** for new assets, library content and imports; v1 stays implicit metres; rooms, placement and physics stay metres. Measured: 11.5% fewer tokens, and prop-scale numbers become 1-token integers | yes |
| D2 | Format 2 parts are flat (`type` and shape fields on the part). The v1 nested `shape` object is still read everywhere. Measured: 5% fewer tokens, simpler `set` | yes |
| D3 | Under P4, tool-written readable text counts as data: fitted primitives, procedural material recipes, and small quantized vertex-coloured mesh files (agents may place, scale, mirror and recolour them, never hand-edit vertices). Never commit .glb/.obj/.fbx/.png; raw downloads go to `.cache/`. Same reasoning as the animation decision (HISTORY.md: "readable text counts as data") | yes |
| D4 | Commit bulk CC0 translations that pass the import gates as on-disk packs (budget ≤ 40 MB, not embedded, `.vercelignore`d), like `anim/cmu` | yes, after pilot GO |
| D5 | The embedded, public build ships CC0 and own work only. CC-BY stays in on-disk packs with credits. QAL, NC, ND, SA and unknown licences go to git-ignored local tiers | yes |
| D6 | Poly Pizza needs an API key and blocks cloud containers | skip until a key is given |
| D7 | Theme materials on by default for all 8 themes. View setting `material_detail = full | first-layer | off`; phones default to first-layer until measured | yes |
| D8 | Material frame budget: ≤ +25% typical, ≤ +60% worst case (llvmpipe, 960×540) | yes |
| D9 | Commit accepted CC0 fitted texture recipes (`assets/materials/fit/<lib>/`, ~100 tokens each, on disk) | yes |
| D10 | Benchmark at the lean scale in §10: ~135 author runs on a pinned Sonnet at medium effort, ~320 judge calls on a pinned Opus. Human votes optional | superseded on 2026-10-12 by the lean gates (§10.3) |
| D11 | Trim asset tool replies; in-repo consumers are updated in the same PR. Private scripts that parse current reply shapes would break | proceed |
| D12 | Native Object Studio UI gets upkeep only; every new feature is agent-facing | yes |
| D13 | Optional help-string diet for non-asset tools (TOOLS-11, ~1k tokens off every session's tool list) | yes, after Phase 0 |
| D14 | Level dressing moves after monsters on its own RNG stream, which re-rolls fixed-seed level layouts once at the switch | yes |

## 4. System overview

```
agent ──assets find=──▶ index.tsv rows ──asset_edit copy/patch──▶ prop source (format 2 JSON, cm)
                                                               │ strict read → migrate → check (Issues)
                                                               ▼
material library (MAT/…) ──resolve──▶ compile (deterministic) ──▶ CompiledProp: flat resolved parts
                                                               │
              ┌──────────────────────┬─────────────────────────┼─────────────────────┐
         render (emit_prim,     colliders (compound,        nav / bounds        check (lint) +
         material slot+seed)    hulls for new shapes)                           contact sheets
asset_import: library zip/GLB/OBJ ─▶ Scene ─▶ segment ─▶ fit primitives / mesh parts ─▶ props + fidelity
```

## 5. Object format 2

### 5.1 Versioning, reader, writer, revisions
- `format` = minimum reader version. Readers accept 1..=2; `format > 2` → "needs a newer engine".
  The writer emits the lowest format the content needs (`needed_format`: format 1 iff unit is m
  and only v1 shapes, fields and keys are used). A v1 file upgrades only when an accepted edit
  needs a v2 feature: one history step, and the reply says `"upgraded":2`.
- `props.rs` splits into `props/{mod,doc,read,write,compile,geom,patch,lint}.rs`; the module path
  `crate::props` stays. Reader pipeline:
  1. `read::strict_value` rejects duplicate keys at any depth.
  2. `read::migrate(&mut Value)`: format range check; 1→2 is additive (row 2.1: `migrate(&Value) -> Result<u32, Issue>`, since 1→2 needs no rewrite of the Value).
  3. `read::check(&Value) -> Vec<Issue>`: field tables per node type, ranges, exclusivity,
     references; up to 20 issues with part paths and did-you-mean.
  4. Typed parse into the flat model; v1 nested `shape` is mapped.
  5. `compile`.
- Row 2.1 landed steps 1-4 (`props/doc.rs`, `props/read/v2.rs`). Until their phases land, `MAT/…` materials, mesh parts and `params`/`presets` are refused as not yet available, and inline material layers are checked by pattern and parameter name only.
- Revisions:
  - v1 keeps `prop1-` + the existing FNV-1a over a `V1Mirror` of today's serde layout (props.rs:274-280), so untouched assets, `if_revision` values and history heads stay valid;
  - v2 uses `prop2-` + FNV over the canonical compact text;
  - assets with `ref` or mesh dependencies also get a closure key `propk-` = FNV(revision + sorted dependency keys).
- Canonical writer (`write::text`, replaces `to_string_pretty` at props.rs:290):
  - defaults omitted (row 2.1: format 2 writes each field as written and never adds one; an explicit default stays, because surface fields cascade and an explicit `look:"cel"` overrides a group's look); numbers as shortest f32 round-trip, integers without `.0`, never exponents;
  - engine-computed numbers rounded to 0.1 mm absolute;
  - layout: a header line (`format, name, unit`, root defaults), then `description`, `tags`, `category`, `source`, `fit`, `materials`, `parts`, `sockets`, `params`, `presets`, each starting a line;
  - every map entry on its own line, in key order, nested groups indented 2 spaces per depth (row 2.1: tool inputs are JSON objects with no order, and a revision must not depend on how a document arrived; the examples' authored order reads to the same text);
  - part key order: kind (`type|ref|mesh`), shape fields, `from,to,pos,attach,offset`, `axis,yaw,pitch,roll,spin`, `mat,color,look,emissive`, `solid,hidden,light,fx,sway`, `mirror,array,grid,radial`, `chance,jitter,pick`, `scale,remap,recolor`, `parts` last;
  - idempotent.

### 5.2 Units, sizes, placement
- **Units (D1).** Format 2 requires `unit`. A v2 file without it is an error. Every reply echoes
  `size_cm:"WxHxD"`. A `unit_suspect` warning fires when the asset extent is < 5 cm or > 30 m.
  `op:"unit"` converts a document; `set unit` is refused with that fix. Inline materials use their
  asset's unit. Only `light.radius` and `fx.area` convert; `fx.size/speed/life` are multipliers.
- **Full sizes.** v2 canonical: `size:[x,y,z]` (box, rounded_box, ellipsoid, wedge) and `height`
  (cylinder, cone, tube; capsule height = tip to tip). v1 `half`/`half_height` stay valid.
- **Placement**: exactly one of `pos` (centre, default origin), `from`+`to`, or `attach`(+`offset`).
  - `from/to` (box, rounded_box, cylinder, cone, tube, capsule) are the **visible ends** for every shape. Capsule cap centres are derived (`from + r·d̂`, `to − r_end·d̂`); an error fires if the ends are shorter than `r + r_end`. Local +y runs from→to; local +z is asset +y made perpendicular to the axis (+z when the axis is exactly vertical); x = y×z. A from/to box takes exactly 2 size values `[across, thick]`. Excludes `axis`/Euler; `spin` twists about the axis.
  - `axis` (`x|y|z|-x|-y|-z`): a round shape's own axis (default y; extrude default z), via fixed base rotations, applied before yaw/pitch/roll. Y family: x rz(−90), z rx(90), −x rz(90), −z rx(−90), −y rx(180). Z family: x ry(90), y rx(−90), −x ry(−90), −y rx(90), −z ry(180).
  - `yaw/pitch/roll` stay (YXZ, degrees; +yaw turns +z toward +x, +pitch tips +z down), for group tilts and fine adjustment.
- Legend lines: "x is horizontal: a board leaning sideways turns its width front-to-back." Warning
  `frame_turn` fires for a non-square from/to box within 10° of vertical (but not vertical) that
  leans more in x than in z; its fix is "use spin:90 or a vertical axis".

### 5.3 Primitives (prop-only `PartShape`)
The shared `Shape` enum is untouched, so world.rs:1309-1320, edit.rs:169 and asset_ui.rs:555
keep their exhaustive matches.

| type | fields | render | collider |
|---|---|---|---|
| box | size\|half | `MeshKey::Cube` | cuboid |
| rounded_box | +radius | `MeshKey::rounded_box` | round_cuboid |
| sphere | radius | `SdfInstance` a=b | ball |
| ellipsoid | size | `MeshKey::Sphere` scaled | hull (42 points) |
| capsule | radius, radius_end?, height\|half_height\|from/to | `SdfInstance` (ra≠rb = round cone) | capsule / hull of 2 spheres |
| cylinder | radius, radius_end?, height\|from/to, arc?, facing?, segments? | `Cylinder`; frustum/arc/segments → Custom | cylinder / hull |
| cone | as cylinder, radius_end default 0 | `MeshKey::Cone`; frustum → Custom | cone / hull |
| tube | radius (outer), thickness (inward), height\|from/to, arc?, facing? | Custom | 12 cuboids |
| torus | radius, minor_radius, arc?, facing? | Custom | 12·arc/360 capsules |
| wedge | size, peak (−1 back … 0 gable … 1 front) | Custom | hull (6) |
| lathe | profile [[r,y]…] (2-32 points), segments? (6-64, default 24) | Custom | hull |
| extrude | points [[x,y]…] (front view, 3-64, simple polygon), depth (centred on local z) | Custom | hull |

Rules for the generated shapes:
- **Arcs.** `arc` is in degrees (default 360). The part stays centred on `facing`, which is in the parent frame and perpendicular to the axis. `pos` is the full circle's centre, and the anchor box is the visible sector.
- **Coordinates.** Extrude and lathe coordinates are relative to `pos`.
- **Generators.** Two pav_core generators in `props/geom.rs`, `revolve(profile, closed, segments, arc)` and `extrude(polygon, depth)`, produce both `MeshKey::Custom(fnv(kind, quantized params))` meshes and the hull points.
- **Mesh sharing.** Frustum, tube, torus, wedge and arc meshes are unit-normalised, so one mesh serves every size. Lathe, extrude and mesh parts are absolute.
- **Caching.** `ViewBuilder` caches `HashMap<u64, Arc<MeshData>>` and pushes each key it uses into `scene.custom_meshes` once per frame. The renderer evicts keys after 300 unused frames (renderer.rs:924-932). No new `MeshKey` variants.

### 5.4 Structure
| feature | syntax | semantics |
|---|---|---|
| group | node with `parts:{…}`, optional pos/attach/yaw/pitch/roll and defaults `mat,color,look,solid,emissive` | children use the group frame; the document root takes the same defaults |
| mirror | `"mirror":"x"` or `["x","z"]` | the node's whole subtree maps through the improper frame M·F (M in the parent frame). Rotations become proper only at each leaf (R·diag(−1,1,1)), and only there are local-x parameters reflected (extrude points, mesh vertices, flipped winding) |
| array | `{count, step}` or `{count, span}` | copies centred on pos; `span` spreads copies evenly (step = span/(count−1)) |
| grid | `{count:[a,b], step:[sa,sb], plane:"xz"}` | centred; runs along plane[0] then plane[1]; index = ia + a·ib |
| radial | `{count, radius, start?, arc?, axis?}` | right-hand rotation about +axis (−axis reverses). Angle 0 = +z for axis y, +y for axes x and z. Each copy is turned with its angle; arc < 360 divides by count−1 |
| cycle | `cycle:[…]` inside array/grid/radial | copy i takes entry i%n. A name sets the copy's `mat`; `#hex`/`$role` sets `color` and clears `mat`. It reaches only children without their own `mat`/`color` |
| attach | `"attach":"seat"` or `{to, at?, self?}` + `offset` | position only. `top bottom front back left right` (and `_` combos) are directions in the parent frame, resolving to the face/edge/corner centre of the target's bounding box in that frame. `from`/`to` are endpoints; `[u,v,w]` ∈ [−1,1] is in the target's local box. `at` default top; `self` default = the opposite word on self's own box; `offset` in the parent frame. The target is one resolved part (`id[i]` for copies) or a group (union of children), among the attaching node's siblings (`group.part` reaches inside one; row 2.2). Cycles are errors. The target's authored placement counts: its chance, jitter and pick apply after (row 2.3) |
| ref | `{"ref":"LIB/core/crate", pos…, scale?, remap?:{mat:mat}}` | inlines that asset's resolved parts as `id.part`; read-only inside; depth ≤ 3; `props::install` returns dependents, which are recompiled and refreshed (row 2.3: resolved in place with the caller's seed; `remap` acts inside the referenced cascade; a referenced asset cannot be removed) |

Order and ids:
- Replication runs first, then each copy is mirrored. `step` and the radial centre are in the parent frame; a node's own rotation does not turn its `step`.
- One replication (array, grid or radial) per node, plus mirror. Compose more through groups: the well's staggered stone rows are 2 nodes.
- Resolved ids: copies are `id[i]`, mirror copies append `~x`, `~z` or `~xz` (axes in xyz order), and group children are written `group[i]~x.child`.

Surface cascade (nearest level wins):
1. Walk from the part through each enclosing group up to the document root.
2. At each level, take an explicit `color`/`look`/`emissive` if one is written, otherwise that level's `mat`.
3. A v2 part with no surface source anywhere is an error (v1 keeps its default `#e8704a`).

### 5.5 Sockets, roles, variation, extras, provenance, mesh parts
- **sockets**: `"sockets":{"grip":{pos|attach, axis|yaw…}}`, at most 16. +y is the socket's working direction. Vocabulary: `grip, grip2, tip` (+y along the blade), `top` (+y up), `wall_mount` (+y into the wall), `hang`, `sit` (row 2.1: a closed set; other names get did-you-mean).
- **roles**: a `color` may be `$floor $floor2 $wall $pillar $accent $light`, the same names as the `ThemeDef` colour fields. They resolve at render time from the instance's palette, else `props::DEFAULT_ROLES`. `PropInstance` gains `seed: u32`, `roles` and `params` (all serde default). (Seed and palette land with row 2.4's seeds and rooms, params with LIB-4; row 2.3 draws seed 0 with `DEFAULT_ROLES`.)
- **variation**: `chance` (0-1), `jitter:{pos:[x,y,z], rot:[y,p,r], scale:[lo,hi], color}` and group `pick:n`.
  - The random value is u = FNV-1a64(`"{seed}:{resolved id}:{field}"`) mapped to [−1,1], so adding parts never reshuffles others and `SimState.rng` is never touched.
  - `jitter.color: j` means linear RGB × (1 + j·u).
- **extras**:
  - `hidden`: collision only, must be solid;
  - `light`: `LightDef` minus offset (fxdef.rs:10-23); setting it disables the emissive auto-light;
  - `fx`: `EmitterDef` minus offset and ground;
  - `sway`.
  Each is placed at the part's position. Per asset: lights ≤ 4, fx ≤ 4.
- **metadata** (written by tools):
  - `category`, one of `furniture container architecture lighting vegetation rock weapon armour tool food town dungeon signage`;
  - `tags` (≤ 16, `[a-z0-9_]`, ≤ 24 chars);
  - `source` {library, url, license (SPDX), author, sha256, title}, kept on copy;
  - `fit` {mean_mm, worst_mm, iou, de, mesh_pct, parts_hash}, with `fit_stale` reported after edits.
- **mesh part** (D3): `{"mesh":"<pack>/<name>", pos…, scale?, recolor?:{"0":"wood"}}`.
  - It references `assets/props/lib/<pack>/meshes/<name>.json` = `{format:1,name,unit:"mm",tris,budget,palette:[hex…],v:[x,y,z…] (integers),f:[a,b,c,p…],source}`, limited to ≤ 4,096 triangles and ≤ 64 palette colours, with at most 16 mesh nodes per asset.
  - It is read-only: patches are refused with the fix "re-import or recolor", and inspect shows only `{tris, palette, size}`.
  - It renders as unwelded per-face colours under `Custom(fnv(sha, recolor, mirrored))`, with a hull collider.
- **params/presets** (parametric families): see §9.4.

### 5.6 Compile and consumers
`compile::compile(&PropAsset, &Deps, seed) -> Result<CompiledProp, Vec<Issue>>` (row 2.2: `compile(&Doc, deps, seed)`; the library holds `PropDef`s and keeps each one's format-1 view for the consumers until F5).
- It is pure f32 and deterministic: ids in BTreeMap order, depth-first, copies in index order, metres internally.
- It runs once per key, at install, parse or snapshot load, and caches up to 64 seed variants.
- `PropDef {asset, revision, key, deps, meshes, compiled, variants}` replaces `Arc<PropAsset>` in the library, `PropInstance` and `PropFrame` (prop_instance.rs:17-30). (Row 2.3: done; `props::get` returns the definition, which keeps a format-1 view for format-1 patches and the native drafts. Parts carry exact boxes, format 1's bounds bit for bit; the view builder caches generated meshes.)
- `CompiledProp {parts: Vec<ResolvedPart>, bounds, sockets, lights, fx, warnings}`.
- `ResolvedPart {id, prim, pos, rot, scale, color: Hex|Role, tint, look, emissive, mat_index, solid, hidden, sway, aabb}`.

| consumer | change |
|---|---|
| render `pav_view/src/build.rs` `emit_object` (1505-1523) | loop over compiled parts via a new `emit_prim`; `emit_shape` stays for Visuals and vehicles; part lights/fx via `fx::light`/`emitter_burst`, `emit_carry` keyed by (entity, part) |
| physics `prop_instance.rs` `collider` (77-92) | compound from solid (incl. `hidden`) parts, table §5.3; > 256 entries is an error ("mark decorative parts solid:false") |
| nav `nav.rs` (92-103) | solid resolved AABBs |
| bounds, selection, preview | `def.bounds(scale)` (prop_instance.rs:66-73, edit.rs:281, prop_preview.rs:155-158) |
| inspect | `parts` as authored; `inspect resolved=true` lists resolved id/type/size, with from/to for axial parts and Euler only for parts placed with `pos` (moved to row 2.4 with F6; row 2.3 lists resolved parts in `bounds=true`) |

v1 assets compile 1:1: the four templates produce today's exact `MeshInstance`/`SdfInstance` lists
(regression test).

Limits:

| what | limit |
|---|---|
| authored nodes, incl. groups and refs (`MAX_PARTS`) | 256 |
| group depth / ref depth | 4 / 3 |
| resolved parts | 1,024 |
| array/radial count | 64 |
| grid | 32×32 and ≤ 256 copies |
| distinct generated meshes | 64, ≤ 8,192 triangles each |
| lengths | 0.001-1000 m after unit conversion |

Compile warnings (≤ 10 lines in the reply):
- `unit_suspect`;
- `floating`: the resolved AABB-overlap graph at 0.5 cm tolerance has more than one component;
- `sdf_heavy`: more than 64 sphere/capsule parts;
- `frame_turn`;
- `mirror_overlap`: a mirror copy within 0.5 mm of its source.

### 5.7 Patch ops, dry run, errors
Keep `{op, part, fields}`:
- `part` takes dotted ids and `parts:[…]` targets several;
- `material` and `socket` targets address those maps; no target means document fields;
- dotted keys inside `fields` address nested fields (`"array.count":8`), and `null` deletes a field.

| op | effect |
|---|---|
| set / add / remove | as today; add with a dotted id adds into a group |
| rename `{part,to}` | also rewrites `attach.to` and socket refs |
| copy `{part,to,fields?}` | deep copy plus overrides |
| mirror `{part,axis}` / array `{part,count,step}` | sets the structure field |
| expand `{part}` | replaces a replicated node by explicit nodes `<id>_<i>`, keeping the placement form (from/to copies with transformed endpoints, never opaque Euler) |
| move `{part\|parts, by}` / scale `{by, part\|parts?, about?}` | moves or rescales lengths |
| unit `{to}` | converts the whole document |

- Ops apply to the Value, then the whole document is re-checked and compiled: atomic, ≤ 1,024 ops.
- `dry_run=true` on create/copy/patch/replace returns the full reply with `saved:false` and `revision_next`. It writes nothing, publishes nothing and issues no ticket.
- `Issue {severity, path, code, message, expected?, fix?}` lives in `crates/pav_core/src/issue.rs` (Phase 0). It is a Rust port of SpawnForge `vendor/core/src/blueprint/issues.ts` and `suggest.ts`. It renders as `path: message (expected …) — fix`, e.g. `ops[2] crate.fruit.grid.count: 0 is outside 1-32 — use a value in range`.
- Codes: `unknown_key unknown_value unknown_part unknown_material unknown_asset missing invalid_type out_of_range conflict duplicate cycle limit format`.
- Hints for common guesses: colour→color; rotation→axis, from/to or yaw/pitch/roll; position→pos; dimensions/half_extents→size; diameter→radius; material→mat; length→height or from/to; children→parts; count→array.count; collide→solid; cube→box; frustum→cone+radius_end; ring→tube/torus; ramp/gable→wedge; mesh→asset_import.

### 5.8 Snapshots, history, rooms
- **Dedupe.** `SimState` gains `#[serde(default)] prop_defs: PropDefTable`, declared before `physics/entities/world` (sim.rs:101-140), with a field-order test.
  - The table stores each reachable definition once, with dependencies and mesh files by sha. (Row 2.4: done, keyed by a u64 hash of `PropDef::key`; row 2.3 had embedded each instance's document with the definitions its references compiled against.)
  - `PropInstance.definition` serializes as its key and resolves through a key → `Weak<PropDef>` registry. Legacy inline v1 definitions still load.
  - `state_hash` hashes keys, not definitions.
  - Target: < 64 B per extra instance of one asset (today ~1.5 KB).
- **History.** Undo/redo becomes `Vec<Option<Value>>` (asset_tools.rs:23-28): old typed files read as Values, and restoring runs migrate + parse. (Landed in row 2.3: format-2 creates need it.)
- **Rooms.** `asset=`/`scale=` stay. `ObjectDef` gains optional `seed` and `params`, written by `object_toml` when set. `room_check` reports unknown assets with did-you-mean. (Row 2.4: `seed` and the palette `roles` landed; `params` waits for LIB-4 with `PropInstance.params`.)

### 5.9 Conformance corpus
The five examples and three probes (once `docs/asset_plan/examples/format2/`) became
`crates/pav_core/tests/prop_format2.rs` and its fixtures in `tests/fixtures/props/format2/`.
Golden resolved values are in cm, with tolerance 0.01 cm and 0.05°.
- bench `leg_back~x` (−85, 70.5, −41), pitch −12.01.
- well:
  - `rim` y 85;
  - `stones[1].ring[3]` (65.62, 66, −21.32), yaw 108;
  - `stones_mid[0]` (21.32, 40, 65.62);
  - `roof~z.slab` (0, 207.3, −41), yaw 180, pitch 35.
- market_stall `awning[0].stripe` x −108.25; `crate[1].fruit[5]` (78, 129.06, 53.8).
- greatsword `pommel` y −40.5.
- probe_mirror `arm~x.hand` (−50, 80, 0); `outer.inner.cube` resolves #30c040.
- probe_attach:
  - `cube` (0, 128, 0) and `knob` (0, 106, 0);
  - `spokes_z[0]` at +y and `spokes_z[1]` at −x;
  - `cap_height` and `cap_fromto` have equal extents.
- Resolved part counts: bench 7, market_stall 46, well 53, wall_torch 6, greatsword 9.

### 5.10 Example (the bench, format 2, 280 tokens; v1 template 508)
```json
{"format":2,"name":"bench","unit":"cm",
"description":"A wooden bench: broad seat, four legs, backrest leaning 12 degrees back on the rear legs.",
"tags":["furniture","seat","wood","town"],
"materials":{
"wood":{"color":"#bc8150"},
"wood_dark":{"color":"#594635"}},
"parts":{
"seat":{"type":"rounded_box","size":[220,18,76],"radius":2.5,"pos":[0,80,0],"mat":"wood"},
"backrest":{"type":"rounded_box","size":[220,8],"radius":2.5,"from":[0,92,-38],"to":[0,141,-48],"mat":"wood"},
"leg_front":{"type":"box","size":[10,8],"from":[85,0,25],"to":[85,71,25],"mat":"wood_dark","mirror":"x"},
"leg_back":{"type":"box","size":[10,8],"from":[85,0,-26],"to":[85,141,-56],"mat":"wood_dark","mirror":"x"},
"crossbar":{"type":"box","size":[6,6],"from":[-80,31,-33],"to":[80,31,-33],"mat":"wood_dark"}},
"sockets":{
"sit":{"pos":[0,89,4]}}}
```
This version also fixes the template's rear legs, which pierce the backrest.

Authoring guidance for docs and exemplars:
- inline a single-use colour as `color` (a colour-only material used once costs about 8 tokens more);
- use materials for shared, patterned or emissive surfaces;
- keep silhouette detail in geometry and surface detail in materials.

## 6. Materials and textures
"Textures" here means procedural material recipes evaluated per pixel. No image files (P4), no UVs.

### 6.1 Material object and tiers
| tier | path → name | notes |
|---|---|---|
| core | `assets/materials/*.json`, `pb/*.json`, `pal/*.json` → `MAT/oak_planks`, `MAT/pb/gold`, `MAT/pal/wood_bark` | embedded (`pav_core/build.rs` table `MATERIALS`), read-only, ≤ 200 files |
| workshop | `assets/materials/workshop/` → `MAT/workshop/x` | editable through `edit_at`; revision `mat1-<fnv>` |
| fitted | `assets/materials/fit/<lib>/` → `MAT/fit/<lib>/x` | on disk, on demand, `.vercelignore` (D9) |
| local | `assets/materials/local/` | git-ignored |
| per prop | `"materials":{key: "MAT/…" or inline object}` | parts say `"mat":"key"` or `"mat":"MAT/…"` |

| field | default | meaning |
|---|---|---|
| `format, name, unit, description, tags, source, fit` | files only | `unit` per D1 (house cm) |
| `base` | — | inherit `MAT/…`; shallow merge, `layers` replaced wholesale; depth ≤ 4, no cycles |
| `color` | required unless `base` | `#rrggbb` or a role |
| `look` | cel | flat\|cel\|lit\|unlit |
| `emissive` | 0 (0-32) | > 0.5 still spawns a point light |
| `roughness`, `metallic` | unset, 0 | glTF/MaterialX names; metallic adds a cel highlight |
| `map` | auto | object\|world\|axis; auto = world for static blocks, axis for cylinder/capsule/cone parts, else object |
| `scale` | 1 (0.1-10) | multiplies every layer size |
| `turn` | false | rotates 90° about local z (axis map: swaps u/v), e.g. barrel staves |
| `layers` | [] (≤ 3) | bottom first |

Resolution order:
1. the part's own `color`/`look`/`emissive`;
2. then the material chain;
3. then defaults.

Base colour and emissive travel in the instance, so one GPU slot serves every tint. Only layers that
use roles need one slot per palette.

```json
{"format":1,"name":"oak_planks","unit":"cm","description":"Warm oak boards: floors, decks, crates, tables.",
 "tags":["wood","floor"],"color":"#a0703f","roughness":0.7,
 "layers":[{"type":"planks","size":[120,20],"gap":2,"vary":0.2,"cavity":0.25},
           {"type":"wood","size":10,"axis":"x","cover":0.3,"shade":0.8}]}
```

### 6.2 Closed v1 pattern vocabulary (14 names, 10 GPU kernels)
Every layer takes:
- `type`;
- either `color` (hex/role) or `shade` (multiplies perceived brightness underneath; default 0.6);
- `strength` 1, `glow` 0-8 (emissive where painted, no light);
- `facing` `up|down|side` (world normal), `masked_by` `prev|not_prev`, and `seed`.

Cellular patterns (C) add `vary`, `cells` (2-4 colours picked per cell), `cavity` and `bevel`.
2D patterns add `axis` (the long direction) and `angle`.

Sizes are feature sizes in the document unit. Blender's inverted `scale` is never used.

| type | Blender | space | params (defaults in metres) |
|---|---|---|---|
| noise | Noise | 3D | size 0.5, detail 3 (1-4), distort 0, cover 0.5, soft 0.3 |
| voronoi (C) | Voronoi | 2D | size 0.3, jitter 1, feature spots\|edges\|cells, cover 0.3, gap 0.02 |
| wave | Wave | 1D | size 0.2, axis, rings false, distort 0, cover 0.5, soft 0.5 |
| brick (C) | Brick | 2D | size [0.4,0.2], offset 0.5, gap 0.03, vary 0.15, cavity 0.3 |
| checker | Checker | 2D | size 0.5 |
| gradient | Gradient | 1D | axis y, from 0, to 1 (above the part bottom; world y in world map) |
| magic | Magic | 3D | size 0.5, depth 2 (1-4), distort 1, cover 0.5, soft 0.3 |
| planks (C) | brick-based | 2D | size [1.2,0.2], gap 0.015, vary 0.2, cavity 0.2, random end joints |
| tiles (C) | brick-based | 2D | size 0.5 or [w,h], gap 0.02, vary 0.1, cavity 0.2 |
| cobbles (C) | voronoi-based | 2D | size 0.25, jitter 0.8, gap 0.03, vary 0.2, cavity 0.5, bevel 0.3·size (flagstones: size 0.6-0.8) |
| stripes | wave-based | 1D | size 0.3, cover 0.5, axis x, soft 0 (diagonal: axis [1,1,0]) |
| dots (C) | — | 2D | size 0.15, radius 0.04, offset 0.5, jitter 0 |
| shingles (C) | — | 2D | size [0.3,0.2], shape round\|square\|point, gap 0.015, vary 0.15, cavity 0.4 (scales: size [0.08,0.06]) |
| wood | Wave rings | 3D | size 0.08, axis y, distort 1.5, cover 0.35, soft 0.4 |

`cover` for noise and magic becomes a threshold through 21-entry quantile tables, computed by the
CPU reference and regenerated by a test.

Combination, in linear RGB, with `c` starting at the instance colour:
```
per layer: m = AA mask × facing × prev-mask
  cellular: c = cells[pick]?; c *= (1+vary(2r−1))²; c *= (1−cavity·profile(d,gap,bevel))²
  c = mix(c, color ?? c·shade^2.2, m·strength); glow += glow·m·strength; prev = m
```

### 6.3 Legibility and budgets
- At the Shardfall camera (tilt 56°, 15.5 m, fov 40°) one pixel is 1.04 cm at 1080p, 2.1 cm at
  540p and 4.2 cm at 270p.
- Validation warns on a feature below 5 cm or a gap below 1.5 cm. A layer fades to its mean between
  3 px and 1 px, and is skipped below 1 px.
- Budgets: ≤ 3 layers, ≤ 4 cell colours, ≤ 4 inheritance levels. A warning fires above 6 cost units:
  - 1 unit: the brick family, checker, stripes, gradient, dots;
  - 1.5: shingles, wood, wave, magic;
  - 2.5: voronoi, cobbles;
  - 0.5 per octave: noise.
- Per scene: ≤ 256 resolved slots soft, 4,095 hard.

### 6.4 Rendering strategy (decided)
- **Data-driven uber-shader.** A fixed WGSL library in `shaders/materials.wgsl` switches per layer
  inside a loop over a storage-buffer row. A material edit is a 368-byte `write_buffer`, and no
  pipeline is rebuilt. Generated WGSL was rejected: every edit recompiles (≥ 0.4 s), and it needs
  generators on both CPU and GPU.
- **Instance hook.** `params[3] = slot | seed<<12`. Slot 0 is today's exact path. The seed is
  hash(entity, part) for object/axis maps and 0 for world maps, which keeps 4 m floor tiles seamless.
- **Mapping.**
  - object: the part-local metric frame from the pre-sway position;
  - world: world position;
  - axis: arc length around local y × height.
  - 2D patterns use dominant-axis projection, exact on boxes; round parts should use 3D patterns or the axis map. 3D lookups are inset 1 mm.
- **Anti-aliasing without derivatives.** `Globals.params.w = 2/(proj[1][1]·height_px)` gives an
  analytic pixel footprint. Lines use box-filtered pulses, each layer fades to its mean, and fbm
  drops sub-pixel octaves. No `dpdx`/`fwidth` anywhere, so the code stays Tint-safe inside loops
  and after `fs_sdf`'s discard.
- **Not in v1** (MAT-10, each gated on a measured need):
  - bump: barely visible under cel shading, +9%, and outline speckle if it reaches the normal buffer;
  - texture arrays: +29-38%;
  - alpha cutout;
  - a pixel-grid "stamp" kernel.
- **CPU reference** `pav_core::material::eval`, using the same PCG hash: drives swatches, coverage
  stats, vertex-colour bakes, import fitting and parity tests.

### 6.5 Engine work by file
- `pav_render/src/scene.rs`:
  - `MeshInstance`, `SdfInstance` and `DynamicMesh` gain `material: u32`;
  - `MeshInstance` gains `ext: Vec3` (half extents; ZERO = derive from the columns; set it for RoundedBox);
  - new Pod `GpuLayer` (7×vec4) and `GpuMaterial` (368 B);
  - `Scene.materials: Arc<Vec<GpuMaterial>>` plus `materials_version`.
- `renderer.rs`:
  - the shader is `concat!(materials.wgsl, scene.wgsl)` (line 259);
  - group-1 layout with storage **binding 2** (bindings 0-1 are the font's), at lines 389-393;
  - the buffer grows to the next power of two;
  - `params.w` footprint at line 973; `params[3]` from `.material` at lines 1060, 1080 and 1118;
  - instance attribute `@location(10) ext`;
  - `set_bind_group(1, …)` after line 1263.
- `scene.wgsl`:
  - `MeshOut` gains `local`, `lnormal` and flat `ext` (7 of 16 varyings);
  - `fs_mesh` evaluates front faces and `TWO_SIDED` backs;
  - `fs_sdf` builds its frame from a→b;
  - `shade(…, pbr)`: `pbr.x < 0` = legacy. Cel metal: `step(mix(0.995,0.93,r), n·h)·metallic`. Lit exponent `mix(80,6,r)`.
  - The geometric normal always goes to the outline buffer.
- `pav_view/src/materials.rs` (new) `MaterialTable` on `ViewBuilder`:
  - interns (material revision, palette hash) into stable slots;
  - re-resolves in place on library or palette changes;
  - `emit_object`, `emit_shape`, the static block loop (1016-1036) and decor pass slot and seed.
- `pav_core`:
  - new `material/{mod,schema,resolve,eval,library}.rs`;
  - `statics.rs` `Block`/`Decor` gain `material: Option<String>`;
  - props gains the `materials` table and `mat`; `color` becomes explicit-only, with a JSON migration that writes `#e8704a` into v1 parts so `prop1-` revisions hold;
  - `arpg/world.rs` `ThemeDef` gains `floor_mat, wall_mat, pillar_mat, cap_mat`. When set, `floor()` (644) uses `floor[0]` with no checker or jitter, and `room_walls()`/`furniture()` tag their blocks. No RNG change. `Mood.palette`; `blend_theme` keeps A's materials.
- Checks:
  - golden captures of 3 scenes with all slots 0 unchanged (≤ 1/255);
  - `pav_view/tests/material_parity.rs`: ≥ 99.9% of pixels within 1/255 per kernel;
  - `pav_render/examples/material_bench.rs`: typical ≤ +25%, all-3-layer ≤ +60%;
  - `scripts/check-wgsl.sh`: dump the WGSL and run `createShaderModule` + `getCompilationInfo` in headless Chromium with the AGENTS.md SwiftShader flags, on every WGSL change, because naga is laxer than Tint.

### 6.6 Material tools
- `kind=material` on `assets`/`asset_edit`/`asset_preview`.
- Ops: `{op:set,fields}`, `{op:set,layer:i,fields}`, `{op:add,layer:{…}}`, `{op:remove,layer:i}`.
- `asset_preview kind=material name=… [compare=a,b ≤6] [theme=key] [view=close|game]` stages a
  swatch prop (rows of sphere, cube, cylinder and a 2×2 m world slab). It replies per layer with
  the feature size, pixels at the game camera, coverage and a verdict.
- `docs/MATERIALS.md`: schema, vocabulary, legibility, examples.

### 6.7 Translating texture libraries
All of it runs through `asset_import kind=material catalog=assets/catalogs/materials/<lib>.json`.
Raw files stay in `.cache/assets/textures/`.

- **Kenney Pattern Pack Pixel** (CC0, 64 tiles of 2-3 colours):
  - about 60% map exactly to checker, stripes, dots or tiles/brick (diamonds via `angle:45`);
  - about 20 tiles (meanders, hearts, symbols) go into `$skip` with a reason;
  - no pixel-grid kernel in v1;
  - Pattern Pack (median 17 colours) goes through the same path after Oklab quantisation to ≤ 4 cell colours.
- **Photoreal CC0** (ambientCG, Poly Haven): restyled references, never faithful imports.
  - Fetch the 1K PNG colour map and its physical size (ambientCG `full_json` in cm; Poly Haven `/info` in mm).
  - Resample to 128 px; palette by Oklab k-means (k = 3); size seeds from the top-3 spectral peaks.
  - Candidate types come from the category (bricks → brick/tiles, paving → cobbles/tiles/brick, floors → planks, ground → noise/voronoi, roofing → shingles).
  - Search: about 150 CPU renders per seed and type, multi-start. Loss = `0.03·ΔE_mean + 0.04·ΔE_dist + spec`, where spec is the oriented-spectrum L1.
  - Gates: ΔE_mean ≤ 3, ΔE_dist ≤ 3, spec ≤ 0.7× the flat-colour baseline, then a **mandatory** agent review of the target-vs-fit sheet. SSIM is diagnostic only: it preferred a flat colour in the probe.
  - Probe: Bricks105 passed (ΔE 0.7, spec 0.24 vs 0.50 flat). WoodFloor051 was rejected on a wrong seed. PavingStones138 passed the numbers with the wrong layout, which is why the review is mandatory.
- **Physically Based** (CC0 JSON, 116 materials) → about 24 embedded `MAT/pb/<slug>`.
  - Non-metals copy the colour.
  - Metals use Oklab L×0.8 and chroma ×1.6, because there are no reflections (gold #ffe496 → #d1a400, iron → #908c87).
  - Roughness = max(pb, 0.25).

### 6.8 Creature Studio
SpawnForge skins stay vertex-baked and unchanged. They share only the keys `type/color/strength`
and the PCG idea. A name map (creature stripes/spots/mottle ≈ prop stripes/dots/noise) is a
possible later step.

## 7. Tools and agent experience

### 7.1 Phase 0: reply and schema hygiene (no format change)
| reply | now (tokens) | after |
|---|---|---|
| `inspect BUILTIN/bench` | 1,463 | 416 (−72%) |
| `inspect BUILTIN/lantern` | 1,214 | 328 |
| `assets`, 4 rows | 425 | 143 |
| copy / patch reply | 346 / 317 | 48 / 51 |
| preview status / `studio_status` | 138 / 174 | 39 / 63 |
| saved stall / well file | 6,516 / 7,612 | 2,619 / 2,982 |
| legend | 169 in every inspect | ~350 once (`action=schema`) |
| MCP tools/list | 11,468 | ≤ 11,042, including 12 new args |

- **Numbers.** `reply::tidy(&mut Value)` runs once in `tools::call` (tools.rs:883, after `run`)
  over JSON and image meta. An f64 that holds an exact f32 prints as that f32's shortest decimal,
  and integral values print as integers. This covers all 76 tools, the CLI, MCP and both bridge
  paths. It is lossless.
- **Reply contracts.** Edit replies keep:
  - name, revision, saved, undo, redo, `size_cm`;
  - `changed_parts` (create/copy: `parts:N`), and history_reset or refreshed_instances only when set;
  - `preview_error`, `check` findings, and `file` on create/copy only.
  They drop changed, editable, part_count, bounds and the nested preview block.
- **Other replies.**
  - Inspect omits defaults; the legend (`legend=true`) and bounds (`bounds=true`) are opt-in.
  - The `assets` listing becomes `{cols, rows, total, matched, next}`.
  - Creature replies keep their shape (creature_ui.rs reads `blueprint`); they only get the tidy pass.
- **Errors.** `crates/pav_core/src/issue.rs` (§5.7) is used for format 1 now:
  - parse to a Value and deserialize each part on its own, so every error carries `parts.<id>`;
  - op errors start with `ops[i]` (0-based);
  - a shape error lists the valid types with hints (height → half_height, size → half, cone needs format 2);
  - an unknown action, kind or arg lists the valid values and the nearest match.
- **Schemas.**
  - `asset_edit` ops items become the prop ops only, plus one generic `kind=creature` item. Delete the `.extend(creature_ops)` merge at tools.rs:953-971.
  - Merged creature arg names stay, with help text "kind=creature only".
  - New `asset_edit action=schema [kind] [topic]` returns the field guide, generated from the validation tables (test `schema_covers_every_field`).
  - Help text: ≤ 50 tokens per tool, ≤ 12 words per arg.
- **Writer.** `PropAsset::text()` becomes the canonical writer's v1 subset. Revisions are unchanged.
- **Consumers updated in the same PR:**
  - `asset_ui.rs report()` classifies replies by `saved`/`asset`;
  - `scripts/smoke-asset-studio.py` reads `rows`;
  - new `tests/reply_budget.rs` caps sizes: inspect bench ≤ 1,100 chars, patch ≤ 220, assets row ≤ 120, preview status ≤ 160.

### 7.2 Agent loop (stated in `mcp.rs` instructions and the `asset_edit` help)
| step | call | budget (tokens) |
|---|---|---|
| discover | `assets find="barrel" limit=8` | ≤ 300 |
| read exemplar | `asset_edit inspect name=LIB/core/barrel` | 280-900 |
| copy | `asset_edit copy from=… name=keg` | ≤ 60 |
| patch | one batch of 3-8 ops | 100-250 out, ≤ 60 back plus ~16 per new finding |
| checkpoint | `asset_preview views=true` (or `views=` on the patch) | 315 image + ≤ 60 |
| fix | ≤ 2 more patch + sheet rounds, then stop | ≤ 700 |
| place | `asset_spawn add` | ≤ 60 |

Target: ≤ 2.5k tokens per copy-modify asset. From scratch: call `action=schema` once and write the
definition.

### 7.3 `asset_edit action=check` (lint) and the cost line
`action=check name=X [target=WxHxD] [gate=core] [all=true]` is read-only and also works on BUILTIN
and LIB assets.
- Output lines are `code: ids fact`, at most 10, then `+N more`. A clean asset has no `check` key.
- Every accepted edit and every `dry_run` returns only the findings new since the previous revision.
- `gate=core` adds the library lints (§9.3).
- `crates/pav_core/src/props/lint.rs` runs on resolved parts: OBBs, an AABB sweep, then a 15-axis
  SAT test with exact containment for round shapes. It also emits the compile warnings.

| code | rule |
|---|---|
| floating | parts within 0.5 cm count as joined; lists each group that does not reach a part touching the floor (min y ≤ 0.5 cm), with up to 3 ids and the gap |
| below_floor / not_grounded | a part's min y < −0.5 cm / the asset's min y > 0.5 cm; skipped with `wall_mount` or `hang` sockets (row 2.3: and `grip`, for held items; the socket anchors `floating` instead) |
| poke | A crosses box-like B along B's thinnest axis with A's centre inside B's face, sticking out 0.5-3 cm on one side and ≥ 3 cm on the other |
| hidden | every corner of A is ≥ 1 mm inside a box-like B |
| z_fight | parallel same-facing faces ≤ 1 mm apart, overlapping ≥ 1×1 cm, differing in colour/material/look, at ≥ 3 of 9 uncovered samples; floor-facing bottoms skipped |
| mirror | ids differing only by left/right or l/r that are not mirror images (\|x+x′\| > 1 cm, different y/z/size, yaw/roll not negated) |
| size | an axis more than 10% off `target` |
| tiny | largest dimension < 2 cm (~1 px at the game camera) |
| heavy / meshes / lights | > 20k triangles / > 8 distinct mesh keys / > 4 lights |
| unit_suspect, sdf_heavy, frame_turn, mirror_overlap | from compile |
| default_color, fit_stale | a v1 part with no colour; an import edited since its `fit` |

Validated in the prototype:
- 0 false positives on the 4 templates and the dogfood well;
- 4 real findings on the stall;
- 7/7 planted defects found;
- ~16 tokens per line.

It misses the bench's leg-through-backrest bug, which only the sheet shows, so sheets stay
mandatory at checkpoints. Time budget: ≤ 10 ms for 256 parts and ≤ 50 ms for 1,024 (release build).

The CPU `cost` line reports distinct MeshKeys, triangles from `MeshKey::generate()`, SDF count,
lights and generated-mesh KB. Example: the bench has 2,004 triangles, 1,944 of them from 2 rounded
boxes.

### 7.4 Contact sheet
`asset_preview [name=] views=true|[front,right,back,left,top,iso,<yaw°>] [elevation=20|game]
[tile=240] [figure=true]` returns one auto-framed PNG.
- `views=true` = front, right, back, iso. The default is 960×240 (~315 image tokens).
- `figure=true` adds a 1.8 m figure (stage objects `STAGE_ID_BASE+42/43`) when the asset's largest side is 0.5-20 m.
- `StudioCheckpoint` (preview_tools.rs:102-127) restores the human's camera afterwards.
- Implemented as a new `fn sheet()` in `asset_preview_tools.rs`, using `capture::tile_frames`.

### 7.5 Render cost
`RenderStats` (renderer.rs:153-162) gains batches, triangles, mesh_bytes, custom_meshes, lights and
materials. `capture stats=true` adds them to the image meta; `capture stats=only` returns JSON with
no image (headless and over `--live`). There is no new tool.

### 7.6 Tool surface
| kind | assets | asset_edit | asset_preview | asset_spawn |
|---|---|---|---|---|
| prop | rows | all actions | stage, sheet | yes |
| material | rows | workshop edits, check, schema | swatch, sheet | — |
| mesh | rows (tris, colours, size, src) | inspect summary; patches refused | stage | — |
| creature | as now | forwards to creature_edit | forwards to creature_preview | — |

The only new top-level tool is `asset_import` (§8.2). Read-only actions (`check`, `schema`, dry
runs) stay out of `live_feedback::changes_studio`.

### 7.7 Docs and rules
- **AGENTS.md asset rule** (lines 30-32 and 147-153) becomes: "Animation, objects and materials are
  readable text: key poses (`clip_import`); props and materials fitted into readable primitives,
  procedural materials and small tool-written vertex-coloured mesh files (`asset_import`). Raw
  downloads go to `.cache/` (git-ignored); never commit .glb/.obj/.fbx/.png. Keep each source's
  credit and SPDX licence; non-redistributable sources go only in git-ignored local tiers
  (`anim/local`, `assets/**/local`)."
- **AGENTS.md "Adding a tool"** (319-321) is stale and gets these steps:
  1. Write `t_name` in a family module; it rejects unknown args.
  2. Add a `TOOLS` entry (help ≤ 50 tokens).
  3. Put enums and ops in `schema()`; a new asset kind uses `call()` routing, not a new tool.
  4. Add studio-changing actions to `changes_studio`.
  5. Add an `mcp.rs` instructions clause if agents should find the tool first.
  6. Test it on `Session::new("empty", 1)`.
- **`mcp.rs` instructions Objects clause** (≤ 25 tokens more): "Objects: `assets find=` → inspect an
  exemplar → `asset_edit copy` → one `patch` batch (reply lists `check` findings) →
  `asset_preview views=true` at checkpoints → `asset_spawn`. `asset_edit action=schema` is the field
  guide; `asset_import` translates open libraries."
- Row 1.3 wrote the parts of the rule and the clause that exist; the materials and `asset_import`
  wording is added in the rows that land them (Phases 3 and 5).
- **Other docs.**
  - `docs/ASSET_STUDIO.md` (≤ 14 KB) gets a format-2 Definition section, the agent loop and links to `MATERIALS.md` and `ASSET_IMPORT.md`.
  - `field_guide.toml` adds procedural material, triplanar mapping, vertex color, z-fighting and gltf.

### 7.8 Native UI
Upkeep only (D12):
- Phase 0: the `report()` key fix.
- Format 2:
  - a `props::V1Part` adapter (`PropPart::to_v1()`) for drafts;
  - "edit as JSON" for non-v1 parts;
  - `Deref<Target=PropAsset>` on `PropDef`.
  - (Row 2.3: the definition's format-1 view is the adapter, so format-1 parts keep their drafts; a format-2 asset is edited as a whole document in the JSON editor. No `V1Part` type or `Deref` was needed.)
- Every phase: `cargo build -p pav_app`, `cargo test -p pav_app`, and `cargo clippy -p pav_app --target wasm32-unknown-unknown`.

## 8. Import pipeline (`asset_import`)

### 8.1 Evidence (prototype on downloaded packs)
- **Structure.** Kenney Mini Dungeon (28 static GLBs) and KayKit Dungeon (26) are median 1 node, 1 primitive and 1 material (one gradient colormap). So segmentation must use connected components and colour, not nodes or materials. Kenney Nature Kit uses named flat materials.
- **Colormaps.** 99.9% (Kenney MD) and 97.8% (KayKit) of triangles are reproducible as vertex colour.
- **Repeats and symmetry.**
  - 30/132 (Kenney MD) and 40/248 (KayKit) components sit in translation-repeat groups.
  - 17/28 Kenney MD models are ≥ 95% x-mirror-symmetric.
- **Facing and origin vary.** The Kenney chair faces +z and the KayKit chair faces −x (yaw 90). The KayKit torch's origin is 40 cm above its base.
- **Fit, Kenney MD.**
  - primitives only: median min-view IoU 0.922, ΔE 3.0, 278 tokens;
  - with mesh fallback below 0.85: 0.959, 28/28 ≥ 0.85, ΔE 2.3, 26% of area in mesh, 260 tokens + mesh files;
  - exact mesh: 2,082 tokens.
- **Fit, KayKit.**
  - primitives only: 0.822 (11/26 ≥ 0.85) at 450 tokens;
  - with fallback: 24/26 ≥ 0.85, but 56% of area in mesh parts.

### 8.2 Tool surface
One tool, implemented in `crates/pav_tools/src/asset_import.rs` and registered after `clip_import`
(tools.rs:506). Its MCP schema is ≤ 2 KB, and it is not in `changes_studio`.

| call | does |
|---|---|
| no args | library directory: name, what, licence, packs, models, access, catalog, cached, ledger rows, use (the `mocap` no-args pattern) |
| `find="wood chair" lib= limit=30` | searches ledgers, including the agent `note` column |
| `get=kenney/mini_dungeon,…` | downloads into `.cache/assets/<lib>/<pack>/`: a whole zip up to 64 MB, else members by HTTP range (`RemoteZip`); GitHub repos per file at a pinned commit. Checks sha256 and writes `SOURCE.json` {url, version, sha256, licence_sha256, files, fetched} |
| `survey=<lib>/<pack>` | reads every model, dry-run translates, and writes the ledger, keeping `note`; `fit=false` takes a census only |
| `from=<file\|folder\|zip#glob> set=<pack>` | ad-hoc translation; without catalog licence data, output goes to the local tier only |
| `catalog=assets/catalogs/<lib>.json only=a,b` | reproducible rebuild |
| `list=true` | nodes, meshes, materials, images, extensions, tris, size, warnings; writes nothing |
| `check=<asset>` / `compare=true` | fidelity of any asset carrying `source` against its cached source / one image, source row over translation row, 6 views at 160 px |
| `mode=auto\|prims\|mesh`, `log=`, `out=`, `kind=material` | auto = primitives + mesh fallback; kind=material = §6.7 |

- Ledger columns: `id file tris mats tex comps size_cm flags parts mesh_pct iou iou_mean iou_gpu de chars tier used note`.
- The reply follows the clip_import shape and ends with `next:"asset_preview name=…; asset_import check=… compare=true"`.
- The HTTP User-Agent is `pavilion-asset-import/<version>`. **Never put personal data in request headers.** During planning, the user's email went out once as a crates.io User-Agent.

### 8.3 Refactor first (one PR, no behaviour change)
Move these into `crates/pav_tools/src/import/{mod,gltf,fetch,catalog,ledger,report}.rs`, with
re-exports so existing paths stay:
- glb.rs: `Gltf/open/accessor` (189-260), the matrix helpers (121-160) and the parent/order/world code (337-397);
- fetch.rs, with `cache_dir_for("assets")` = `$PAV_ASSET_CACHE` else `.cache/assets`;
- mod.rs: `Json/Catalog/Entry` (39-174), `Ledger` (388-419) and `csv_line`;
- anim_tools.rs: `write_set_to`, the report helper and `set_out`.

Harden inputs during the move:
- check the BIN chunk type;
- honour `bufferView.buffer`;
- zip entry names basename-only, inflated entries ≤ 256 MiB, CRC32 checked.

Gate: `cargo test -p pav_tools` unchanged, plus a byte-identical `clip_import` rebuild of
`anim/mesh2motion.json`.

### 8.4 One intermediate, readers, normalisation
```rust
// import::scene — colours linear; world space; metres; CCW front faces
Scene { nodes: Vec<Node>, meshes: Vec<Mesh>, materials: Vec<Material>, images: Vec<Image>, info: Info }
Node { name: String, parent: Option<usize>, world: DMat4 }   // parents first
Mesh { node: usize, material: Option<usize>, pos, nrm: Vec<[f32;3]>, uv: Vec<[f32;2]>, col: Vec<[f32;4]>, tris: Vec<[u32;3]> }
Material { name, base: [f32;4], metallic, roughness, emissive: [f32;3], alpha: Opaque|Mask(f32)|Blend,
           double_sided, unlit, base_tex: Option<TexRef { image, uv_set, xform: [f32;6] }> }
Image { name, w, h, rgba: Vec<u8> }   // decoded, sRGB as stored
```
Limits: ≤ 2M triangles, ≤ 64 images, each ≤ 4096².

**glTF: extend the in-house reader.** Add:
- indices and `mode`; NORMAL, TEXCOORD_0/1, COLOR_0;
- sparse accessors and multiple buffers; `.gltf` with .bin, data URIs or zip siblings;
- materials, textures and images;
- KHR_texture_transform, KHR_materials_emissive_strength, KHR_materials_unlit.

`extensionsRequired` Draco/meshopt/basisu/webp → an error naming the extension. PNG decoding uses
`png.workspace = true`.

The `gltf` 1.4.1 crate (passes P9) is the fallback, adopted only if more than 3 Khronos core models
still fail after one fix round. Pin it as `=1.4.1`, without default features, behind the same `Scene`.

**OBJ/MTL.** Use `tobj = { version = "4.0", default-features = false }` (MIT, no dependencies; 4.0.3
from 2025-01-20 passes P9). Read `Kd` as linear, and check it against a glTF twin in the pilot.
Quaternius OpenGameArt mirrors ship OBJ.

**Deferred:** an FBX geometry reader on `fbx::parse`, and JPEG decoding (I9). Each is needed only
for an FBX-only or JPEG-only pack.

Normalisation (`import::normalize`, f64):
- **Axes.** glTF and the engine are both right-handed, +y up and front +z, so no change. A catalog `up:"z"` rotates OBJ −90° about x.
- **Per pack/asset.** `yaw` in 90° steps so the front faces +z, `scale` (m per unit), and `origin` keep|floor|center.
- **Winding.** det < 0 reverses triangles; primitives whose normals disagree on > 50% of area are flipped.
- **Colour.** sRGB texels → linear before averaging; hex is written only at the end.
- **Weld.** At 0.1 mm, for adjacency only.
- **Skinned and animated.** Skinned meshes are read in bind pose and `$skip`ped by default; animated nodes are read at rest.
- **Verification.**
  - Khronos OrientationTest: every arrow tip within 0.2 units of its target;
  - NegativeScaleTest: ≥ 99% of faces agree with their normals;
  - an in-code asymmetric fixture.

### 8.5 Translation (`import::fit`, deterministic, single-threaded per asset)
1. **Segment.**
   - Groups = (node, material); components = connected within a group.
   - Face colour = UV-area mean of the base texture (7 barycentric samples, linear) × factors, in Oklab.
   - Per component, greedy clusters at ΔE_OK 0.04; flag `multicolour` if a second cluster has ≥ 15% of the area and differs by ≥ 0.06.
2. **Repeats.** Equal shape keys are fitted once and share parameters.
3. **Fit** each component in its frame: a yaw-only OBB in 3° steps, or a full PCA OBB if ≥ 15% smaller.
   - Candidates:
     - box;
     - rounded_box (radius = mean gap, if ≥ 0.5 cm);
     - cylinder/cone (+ `radius_end`, `segments` when faceted);
     - sphere/ellipsoid, capsule;
     - lathe (10 slices, RDP 2%, ≤ 12 points);
     - extrude (64 px silhouette outline, RDP 1 px, ≤ 24 points);
     - wedge.
   - Score = min of 3 local-axis silhouette IoUs at 64 px, minus 0.0002 × part tokens. Keep a fit ≥ 0.85; else try one VHACD split (≤ 4 hulls, refit each); else use a mesh part.
   - Asset loop: while IoU < 0.90 or ΔE > 6, upgrade the component with the largest error share (≤ 2 upgrades each).
4. **Canonicalise.** Angles to 1°, snapped to 0/90/180 within 2°; cm to 0.1.
5. **Structure** (planes through the asset origin):
   - a mirror pair in x then z (|x+x′| ≤ 2 mm) → `mirror`;
   - ≥ 3 equal parts at equal step (± 2 mm) → `array`;
   - lattices → `grid`;
   - ≥ 4 at equal radius and angle step → `radial`.
6. **Names.**
   - With more than one mesh node: the node name.
   - Else the material name (`woodBark` → `wood_bark`).
   - Else `<palette>_<kind>` by area.
7. **Colour.** Area-weighted mean. Snap to a shared palette material `MAT/pal/<name>` (built from pilot clusters) when ΔE_OK ≤ 0.03 → `mat`, else `color`. Emissive capped at 4.
8. **Mesh part** (§5.5): welded integer mm, a ≤ 64-colour palette (Oklab median cut), budget 4,096 triangles (vertex clustering until within), `source` copied.
9. **Colliders.** Primitives are solid. Parts below 0.5% of bbox volume and under 3 cm thick are `solid:false`. At most 64 solid parts; concave mesh parts get ≤ 8 `hidden` VHACD boxes (`pav_core::rapier::parry`).
10. **Write.** `LIB/<pack>/<leaf>` with source, tags, description and fit, through the canonical writer, then a parse + compile read-back.

Target throughput: ≤ 0.5 s per model in Rust. The Python prototype took 7.5 s.

### 8.6 Fidelity metrics and tiers
- **Stored, CPU, deterministic.**
  - 6 orthographic views (front, back, left, right, top, iso at yaw 45° and pitch 30°) at 128 px.
  - `fit.iou` = minimum view IoU (the ledger also stores the mean).
  - `mean_mm` / `worst_mm` = symmetric Chamfer mean / 95th percentile over 4,096 samples.
  - `de` = Oklab ΔE×100 over shared pixels; `mesh_pct` = % of source area in mesh parts.
- **GPU cross-check.** The source is drawn as `MeshKey::Custom` with per-vertex colour and the translation through `emit_object`, all unlit, without outlines or fog, under an ortho camera. Test: |iou_gpu − iou| ≤ 0.03.
- **Tiers** (tag `import_<tier>`):

  | tier | criteria | use |
  |---|---|---|
  | `exact` | mode=mesh, IoU ≥ 0.98, ΔE ≤ 1.5 | reference |
  | `fit` | IoU ≥ 0.85, mean ≥ 0.90, ΔE ≤ 6, ≤ 6,000 chars; `editable` when mesh_pct ≤ 30 | bulk |
  | `draft` | IoU ≥ 0.75, ΔE ≤ 10 | disk only |
  | `fail` | — | not written; the ledger says why |
  | `exemplar` | — | §8.9 |

### 8.7 Catalogs, licences, determinism, paths
```json
{"$library":"kenney",
 "$sources":{"mini_dungeon":{"label":"Kenney Mini Dungeon","origin":"Mini Dungeon 2.0 by Kenney, GLB","license":"CC0-1.0",
   "url":"https://kenney.nl/media/pages/assets/mini-dungeon/<hash>/kenney_mini-dungeon.zip","version":"2.0",
   "sha256":"…","licence_file":"License.txt","licence_sha256":"…","models":"Models/GLB format/*.glb",
   "pack":"kenney_mini_dungeon","yaw":0,"scale":1,"origin":"keep"}},
 "$skip":{"character-human":"rigged character: creature/puppet domain"},
 "$pick":{"gate_door":["gate.glb","door"]},
 "barrel":["container wood","wooden barrel with two iron hoops"],
 "chest":["container treasure","chest with a hinged lid",{"mode":"mesh"}]}
```
- **Catalog grammar** is the anim catalogs' (`//` comments; order = output order). Entries are
  written after a `compare=` look. For git sources, `version` is the commit SHA and `sha256` hashes
  the sorted file manifest.
- **Licence gate** `import/licence.rs` (SPDX allowlist, default-deny):

  | licence | routing |
  |---|---|
  | CC0-1.0 | shared |
  | CC-BY-3.0/4.0 | on disk with credit (D5) |
  | CC-BY-SA, OGA-BY, GPL, QAL, NC, ND, Mixamo, Fab, ShareTextures | `local` |
  | anything else | refused ("unknown licence X: review it, add it to import/licence.rs") |

  `get` stops if the bundled licence text lacks its marker or no longer matches `licence_sha256`.
  Copies keep `source`, and copying a local-tier asset writes into a local tier.
- **Determinism.** Pinned hashes, f64 internals, BTreeMap order, and no RNG, time or thread-dependent
  output (dates only in `SOURCE.json`). Test: two rebuilds give an identical tree hash.
- **Paths:**
  - catalogs: `assets/catalogs/<lib>.json` (materials: `assets/catalogs/materials/<lib>.json`);
  - ledgers: `assets/catalogs/<lib>/<pack>.tsv`;
  - output: per §9.1. Catalogs and ledgers are committed even for local-only libraries, because they hold only metadata (the anim precedent).

### 8.8 Sources, access, pilot
| # | source | access (tested 2026-10-11) | first packs |
|---|---|---|---|
| 1 | Kenney, CC0, ~4,600 models | pack page → version-hashed zip; range requests work | Mini Dungeon, Castle Kit, Nature Kit, Food Kit |
| 2 | KayKit, CC0, 10 GitHub repos | one GitHub tree call + raw files at a pinned commit | Dungeon Remastered, Furniture Bits |
| 3 | Quaternius CC0 (Bestiary is QAL → local) | Google Drive quota-blocked → OpenGameArt zips, 10 s crawl delay | LowPoly House Interior, Modular Dungeon |
| 4 | Poly Pizza, CC0/CC-BY | API key in env `POLY_PIZZA_KEY`, never stored; static host blocks cloud containers | skipped (D6) |
| — | Khronos glTF-Sample-Assets | raw files at a pinned commit | conformance only |
| — | ambientCG, Poly Haven (CC0 photoreal) | APIs | material references only (§6.7) |

Avoid: ShareTextures (restricted "custom CC0"), Quaternius QAL packs, Mixamo, BlenderKit Royalty
Free, Fab/Megascans, NC/ND sets, and the Thingiverse/Printables NC sets.

**Pilot ≈ 250 models:** Mini Dungeon 28, Castle 76, Nature 50, Food 40, KayKit Dungeon 40,
Quaternius 20.
- **GO** to all packs if every one of these holds:
  - ≥ 80% in the `fit` tier, median IoU ≥ 0.90, median asset ≤ 2,500 chars, ≥ 40% of fit assets `editable`;
  - 0 panics and byte-identical rebuilds;
  - GPU/CPU IoU within 0.03 on ≥ 95% of views;
  - ≥ 70% of ≥ 30 re-authorings accepted.
- **NO-GO** (< 60% `fit`): keep exact-mesh imports as local references only, stop bulk translation,
  and record it in HISTORY.

### 8.9 Re-authoring into exemplars
Helper agents get briefs of 10 assets each (the M4-M9 precedent). Each gets the draft, its
`check= compare=true` sheet and the catalog tags.
- **They write** a WORKSHOP copy with:
  - semantic part names;
  - groups and mirror/array/radial;
  - library `mat`, and attach where it is natural.
  They keep mesh parts and never edit them.
- **Accept iff all hold:**
  - IoU ≥ max(0.85, draft − 0.03) and ΔE ≤ max(6, draft + 1);
  - mesh_pct ≤ the draft's;
  - chars ≤ 0.8× the draft and ≤ 3,000;
  - ≤ 2 critique rounds.
- **Publish** accepted assets to `lib/core` (§9.3), keeping `source`.

### 8.10 Tests
- **`tests/common/mod.rs`:**
  - `glb(json, bin)`, moved from tests/mocap.rs;
  - `MeshBuilder` (box, cylinder, revolve, extrusion, materials, COLOR_0, embedded textures, negative-scale nodes, sparse accessors);
  - `png_rgba(w,h,px)` via `png::Encoder`;
  - OBJ as inline strings. No binaries are committed.
- **`tests/import.rs`:**
  - exact read-back; negative-scale winding; palette texture → exact face colours;
  - a table (top + 4 legs) → 2 nodes with `mirror:["x","z"]` at IoU ≥ 0.99;
  - a vase → lathe and a blade → extrude, at IoU ≥ 0.95;
  - a noisy blob → a mesh part within budget;
  - malformed input → errors, not panics;
  - the licence gate; byte-identical rebuild; GPU/CPU agreement.
- **`tests/import_khronos.rs`** (`#[ignore]`) fetches Box, BoxTextured, BoxVertexColors, Duck,
  OrientationTest, NegativeScaleTest, TextureCoordinateTest, TextureTransformTest, VertexColorTest,
  MultiUVTest, SimpleSparseAccessor and AlphaBlendModeTest into `.cache/`.

## 9. Library and Shardfall

### 9.1 Names, layout, tiers
| name | file | editable | loads |
|---|---|---|---|
| `BUILTIN/<leaf>` | `assets/props/templates/` (4, unchanged) | no | embedded |
| `WORKSHOP/<leaf>` | `assets/props/workshop/` (unchanged) | yes | native |
| `WORKSHOP_LOCAL/<leaf>` | `assets/props/local/workshop/` | yes | native; copies of local-licence sources |
| `LIB/<pack>/<leaf>` | `assets/props/lib/<pack>/` (or `assets/props/local/<pack>/`) | no (copy first) | `core` embedded; other packs on demand (native) |
| `MAT/…` | §6.1 | | |

- **References.** Every reference site uses the grammar `NAME[:preset]`. `pack` = `[a-z][a-z0-9_]{0,31}`; `core` is reserved.
- **Pack folders** hold `<leaf>.json`, `meshes/`, `index.tsv` (machine-written, with an agent `note` column) and `PACK.json` {pack, title, license, credit, source, catalog}. `lib/core/review.tsv` is the curation ledger.
- **Embedding.** A new build.rs table `ASSET_DATA` embeds `templates/*.json` and `lib/core/**` (json, meshes, index). It replaces the hand-listed `TEMPLATES` (props.rs:384-389). LIB/core entries parse lazily on first `props::get`.
- **Native disk fallback.** `props::disk_root()` = `$PAV_ASSETS`, else `./assets/props`.

| tier | content | budget (test) | git | exe / wasm | Vercel |
|---|---|---|---|---|---|
| embedded | templates + `lib/core/**` | ≤ 3,000,000 B hard (target 2 MB); ≤ 16 KB per prop, ≤ 64 KB per mesh | yes | yes | uploaded |
| disk | other `lib/<pack>/**`, `assets/materials/fit/**` | ≤ 40,000,000 B; ≤ 1 MB per file | yes | no | `.vercelignore` |
| local | `assets/props/local/**`, `assets/materials/local/**` | none | `.gitignore` | no | no |

- **Tests.**
  - pav_core: `embedded_assets_fit_budget`, `embedded_library_is_valid`, and `game_asset_refs_are_embedded` (every ref in `game/**/*.toml` and `rooms/*.toml` resolves to BUILTIN or LIB/core);
  - pav_tools: `committed_packs_fit_budget`, `committed_packs_are_redistributable`, `index_files_are_current`.
- **Browser.** wasm has only the embedded tier. Unknown refs are load errors on every platform, replacing today's silent fallbacks.
- **`scripts/package-studio.py`** adds `CREDITS.md` and `--packs a,b`, and fails above 30 MB unless `--allow-large`.

### 9.2 Retrieval for agents
- `index.tsv` columns: `name cat size_cm parts tok fid src tags params sockets note`.
  - `tok` = ceil(canonical bytes / 2.8).
  - `fid` is `authored`, `fit93` (IoU %) or `mesh40` (% of area in mesh).
  - `src` is `own` or `CC0 kenney`.
- Queries read the index files, never the assets. WORKSHOP rows are computed live.
- New `assets` args:
  - `find` (all words must match; score: leaf 5, tag/category 3, name 2, description 1; ties go to fewer `tok`);
  - filters `cat`, `tag`, `pack`, `max_tokens`, `has_socket`, `family=true`;
  - `similar=NAME`: a deterministic tag/category/size/part-count score, no embeddings;
  - `name=NAME`: one ~120-token card (params, presets, sockets, materials, source, `use:` line);
  - `page`/`limit` (20, ≤ 100); `credits=true|write`; `reindex=<pack>`.
- Reply: `{"cols":"name cat size_cm parts tok fid src tags params/@sockets","rows":[["LIB/core/barrel","container","56x90x56",5,180,"authored","own","wood dungeon town","height girth hoops lid"],…]}`.
  It costs ~35 tokens per row, against 105 today.
- **Copy, then modify** is the default loop: `asset_edit copy from=LIB/… name=x` accepts `:preset`,
  `params=` and `bake=true`. Evidence: SceneCraft without its learned library fell from CLIP 69.8
  to 48.3. The dogfood from-scratch stall took ~2.6k tokens over 15 calls to reach ~65% of Kenney.

### 9.3 Exemplars and curation
| category | target | families | first wave (W0, bold) / later |
|---|---|---|---|
| furniture | 50 | 6 | bench (re-authored), table, chair, shelf |
| container | 40 | 5 | **barrel, crate_stack**, sack, urn, coffin |
| architecture | 80 | 10 | **kit_wall, kit_trim, kit_corner, kit_door_frame, kit_pilaster, column**, stairs |
| lighting | 25 | 4 | **wall_torch**, brazier, lantern, candle_cluster |
| vegetation | 50 | 8 | **mushroom_cluster**, trees ×6, bush, vines |
| rock | 30 | 5 | **rock_cluster, crystal_cluster**, rubble_pile, stalagmite |
| weapon | 60 | 10 | one family per weapon kind with presets t1-t6; shields, foci |
| armour | 36 | 6 | helmets (7 looks), pauldrons, monster horns and plates |
| tool | 25 | 3 | pickaxe, anvil, ladder, bucket |
| food | 25 | 3 | bread, cheese_wheel, bottle, mug |
| town | 40 | 6 | **fence**, well, market_stall, cart, signpost, house |
| dungeon | 40 | 6 | **bone_pile**, chains, cage, altar, banner |
| signage | 15 | 3 | sign, hanging_sign, plaque |

The total is ~520 exemplars in ~75 families; W0 is ~30 assets. The six hard-coded Shardfall kinds
(rocks, crates, columns, crystals, bones, mushrooms) become `rock_cluster`, `crate_stack`, `column`,
`crystal_cluster`, `bone_pile` and `mushroom_cluster`, using the same roles.

**Gate for `lib/core`** (`asset_edit action=check gate=core`):
1. Zero compile warnings and zero library lint:
   - `size_band` (category bands, e.g. weapon 15-250 cm);
   - `tokens` (food/tool/signage 400; container/lighting/rock/vegetation 600; furniture/weapon/armour/architecture 800; town/dungeon set pieces 2,000; hard cap 2,000);
   - `raw_colors` (> 6 distinct hex);
   - `missing` (description ≤ 120 chars, category, 2-6 tags);
   - `sockets` (weapons: grip + tip; wall props: wall_mount);
   - `sdf` (> 8 sphere/capsule parts in dressing categories).
2. Sheet review:
   - a 4-view sheet at game elevation plus one close view;
   - checklist: reads at game distance, no floating or poking parts, origin on floor/wall, front +z, roles where themes should tint.
3. Provenance complete.

`asset_edit action=publish name=WORKSHOP/x pack=core review="ok: …"`:
1. runs the gate;
2. writes `lib/core/<leaf>.json`;
3. rewrites `index.tsv`;
4. appends a `review.tsv` row at the current revision.

Test `core_assets_are_reviewed` forces a re-review after any edit.

Growth loop:
1. import CC0 packs to disk;
2. re-author the best drafts (§8.9);
3. author from briefs to fill gaps;
4. get variety from families, presets, `jitter` and `chance` rather than more files;
5. cap critique at 2 rounds and revert regressions.

Each wave reports counts per category, p90 tokens, review coverage, and tokens per accepted asset
from one fresh-agent run.

### 9.4 Parametric families (no expressions)
- Param types:
  - `range` param: `{"range":[lo,hi],"int"?:true,"set":{path:factor}}`, each target = value × factor;
  - `choices` param: `{"choices":[a,b,…],"set":{path:[value per choice]}}`;
  - `presets`: `{name:{param:value}}`.
- **Defaults.** The file's literals are the defaults, so a family file is always a valid concrete
  asset. A check requires targets to agree with the defaults within max(0.5 unit, 0.5%).
- **Dependent placement** comes from `attach`, `mirror`, `array.span` and `chance`, never arithmetic.
  Targets cannot be `type|ref|mesh|attach.to`.
- **Limits.** ≤ 8 params, ≤ 8 targets each, ≤ 8 choices, ≤ 16 presets.
- **Pipeline.** `props::apply_params` runs after check and before compile. The cache key becomes
  (key, params hash, seed).
- **Accepted by** `asset_spawn`, `asset_preview` (`params=`, `seed=`), room objects, `ref` nodes and
  dressing items, plus `NAME:preset` anywhere. `asset_edit action=param` adds a param from target
  paths and computes the factors.
- **Example:** `docs/asset_plan/examples/families/fence.f2.json`. It is 406 tokens; its 4 variants
  (default 13 parts, low 11, long 17, ruined 8) rendered correctly through the prototype compiler.

### 9.5 Credits and licences
- **Provenance.** Per-asset `source` is kept through copy, re-authoring and publish. `PACK.json`
  repeats the licence.
- **Licence rules** (tests):
  - `lib/` holds only CC0-1.0, CC-BY-3.0 and CC-BY-4.0;
  - `lib/core` holds only CC0 or own work (D5);
  - anything else goes only to `local/`.
- **Local dependencies.** A WORKSHOP save that depends on a local-only source, ref or mesh is
  refused with "save as WORKSHOP_LOCAL/x".
- **Credits.** `assets credits=true` groups by licence, with full attribution lines for CC-BY.
  `credits=write` regenerates `assets/CREDITS.md` (props, MAT sources, anim credits), checked by
  `credits_file_is_current`; README links it.

### 9.6 Shardfall integration (ranked by payoff)
| # | item | change | determinism / snapshot |
|---|---|---|---|
| 1 | theme materials (Phase 3) | `themes.toml` `floor_mat/wall_mat/pillar_mat/cap_mat`, all role-coloured so blended depths follow; e.g. overgrown_ruins mossy_flagstones/stone_blocks/banded_stone | no RNG; Block gains an optional string |
| 2 | theme kits | `ThemeDef.kit {wall,trim,corner,door,pilaster,column,sconce}`. `room_walls` places wall segments over new `block_flags::HIDDEN = 64` blocks (collide, not drawn) and trim replacing the cap course (world.rs:698-702); corners at rect corners, door frames at doors, columns over Pillars blocks, `wall_torch` entities replacing `sconce()` | layout only, no RNG or `taken`; kit pieces `solid:false` |
| 3 | static bake | `statics.rs StaticProp {asset,pos,rot,scale,seed,params,collider}` in `StaticChunk.props`; compound colliders; nav marking; emitted by the view region cache (build.rs:1011-1075) through `emit_prim`; assets with `fx` become entity props | ~80 B per prop; `PropDefTable` walks statics; `state_hash` adds the prop count |
| 4 | data dressing | new `game/props.toml` sets (the old six names are sets, so `themes.toml` is unchanged); `decorate` splits into `ambience()` (in place) and `dress()`, run **after** `monsters()` with `Rng::new(seed ^ 0xd2e5_5a11)`; it reads `taken` but adds nothing after; `Builder::prop` (944-1043) becomes a set lookup; place modes wall/floor/corner/wall_mount; clearance = footprint + 0.5 m | one-time re-roll at the switch (D14); afterwards dressing never moves mechanics |
| 5 | item/weapon models | `BaseDef.model` (e.g. `"LIB/core/sword:t3"`), `WeaponLook.model`; `pose_ex` (puppet.rs:875) returns `PoseProp{asset, at}` through the span trick (891-900); swing trail from the `tip` socket; `LootView.model` drawn on the ground; capsule fallback | view-only |
| 6 | monster gear | `Attach.asset` on parts.rs anchors via PoseProp; `game/genome.toml [gear.<element>]` pools from `Rng::new(seed ^ 0x6765_6172)` | genomes unchanged except gear |
| 7 | town as data | `game/town.toml` `[[place]]{asset,pos,yaw,params}` → static props; `[anchors]` feed Spot/NPC wiring in `build_town` | no RNG |

```toml
# game/props.toml: level dressing sets; themes.toml `props` lists set names
[crates]
place = "wall"
items = [ { asset = "LIB/core/crate_stack", w = 3 },
          { asset = "LIB/core/barrel", w = 2, params = { height = [70, 110] } },
          { asset = "LIB/core/barrel:keg", w = 1, scale = [0.9, 1.1] } ]
```
New tests in `pav_core/tests/levels.rs`:
- `dressing_never_moves_mechanics_or_monsters`: depths 1-12, 13, 17 and 25 built with dressing and kits on and off give identical features, monsters, exit, portal and `state.rng`;
- `dressing_is_reproducible`;
- `dressing_snapshot_cost`: ≤ 16 KB per level; baseline level/6 is 1,224,889 B;
- `dressing_keeps_paths`.

Also: `levelmap nav=true` for depths 1-12, and `campaign from=1 to=12` within ±20% minutes of the
pre-change run. level/6 should drop from 406 entities to ~240, and render time with kit and dressing
should rise by at most 15%.

### 9.7 First visible win (end of Phase 4)
In the desktop and browser game, Level 1 (overgrown ruins) changes:
- mossy flagstone floors instead of the 4 m two-tone checker;
- coursed stone walls under a trim course, buttressed corners and framed doorways;
- banded columns, and bracketed wall torches instead of glowing balls;
- 3-6 varied, theme-tinted props per room.

Other themes and blended depths restyle through roles. Mechanics, packs and routes stay identical
(test). For agents, re-dressing a theme is a 3-line `game/props.toml` edit.

## 10. Evaluation harness and gates

### 10.1 Harness (`scripts/asset_bench/`)
It asks one question: given only the docs an agent would normally get, does a fresh agent make
**better assets for fewer tokens**? Arms follow the phases; each includes the earlier ones.

| arm | build | gate |
|---|---|---|
| A | frozen baseline `a9ca003` (format 1, today's replies) | GA |
| T | format 1 + Phase 0 + Phase 1 tools | G1 |
| F | format 2 | G2 |
| FM | F + materials | G3 |
| L | full system with the library | G4 |
| I | `asset_import` | G5 |

**Authoring tasks** (sizes W×H×D cm, ±15%; `~` = ±25%; † = checked only where the arm can express it):

| id | name | brief | size | needs |
|---|---|---|---|---|
| P01 | trestle_table | tavern table for six, plank top, two X-braced trestles, stretcher | 200×78×80 | |
| P02 | ladderback_chair | turned legs, three back slats, woven seat | 46×100×46 | |
| P03 | strongbox | iron-banded chest, domed lid, corner caps, lock plate | 80×62×52 | |
| P04 | grain_sacks | three tied burlap sacks, one slumped | ~100×75×70 | |
| P05 | castle_wall | kit wall: stone courses, 4 crenellations, arrow slit, flush ends | 400×360×80 | |
| P06 | arch_doorway | kit arch for a 200×300 opening, voussoirs, keystone | 320×380×60 | |
| P07 | tripod_brazier | iron bowl on three curved legs, glowing coals | 60×110×60 | light |
| P08 | ring_chandelier | wooden ring, 6 candles, chains; origin at the hook | 110×90×110 | light, hang |
| P09 | dead_tree | twisted trunk, 4 forked bare branches, roots | ~300×450×300 | |
| P10 | potted_shrub | round flowering shrub, terracotta pot | ~60×80×60 | |
| P11 | boulder_cluster | three mossy boulders, largest 120 | ~220×120×170 | |
| P12 | crystal_outcrop | five glowing shards on a rock base | ~120×150×100 | light |
| P13 | battle_axe | double-bit axe, wrapped haft, pommel cap | 36×110×6 | †grip, grip2, tip |
| P14 | round_shield | plank shield, iron rim and boss | 70×70×12 | †grip |
| P15 | horned_helm | great helm, eye slit, breathing holes, two horns | 50×45×35 | |
| P16 | spiked_pauldron | layered steel shoulder plate, three spikes | 32×26×34 | |
| P17 | anvil_stump | anvil on a stump, hammer resting on it | 70×85×45 | |
| P18 | cheese_board | cheese wheel with a wedge cut, board, knife | 50×22×50 | |
| P19 | hand_cart | two spoked wheels, shafts, canvas-covered load | 220×150×130 | |
| P20 | notice_board | two posts, small roof, pinned papers | 160×220×40 | |
| P21 | gibbet_cage | iron cage hanging from a gallows post, bones | 120×260×90 | |
| P22 | sarcophagus | carved stone lid, cracked corner | 230×110×100 | |
| P23 | tavern_sign | sign on an iron wall bracket, tankard emblem; origin on the wall, extends +z | 90×80×12 | wall, †wall_mount |
| P24 | alchemist set | bookshelf, table, cauldron sharing one palette/materials | own sizes | caps 45 cmds / 9 images |

**Edit tasks** start from the arm's own asset for the source task:

| id | source | change |
|---|---|---|
| E1 | P01 | 260 long, a third trestle, same height |
| E2 | P19 | dark stained wood, brass fittings, wheels +20%, still grounded |
| E3 | P05 | 600 wide, crenellation spacing kept, two arrow slits |
| E4 | P13 | single-bit axe with a back spike; sockets stay valid |
| E5 | P07 | frost variant: blue light, legs 30% taller |
| E6 | P24 | whole set re-themed "frost", icicles under the shelf |

**Import tasks** (G5):

| id | task |
|---|---|
| IT1 | find a CC0 wooden barrel, import it, place 3 copies |
| IT2 | import the KayKit Dungeon chair facing +z and report its fidelity |
| IT3 | survey Kenney Food Kit and import its 10 best `fit` models as one pack |
| IT4 | re-author `LIB/kenney_mini_dungeon/chest` to the §8.9 rule |
| IT5 | `from=` a harness-made OBJ zip whose License.txt says CC-BY-SA-4.0; it must land in the local tier with credit |
| IT6 | reproduce the Kenney Mini Dungeon `table` from a reference sheet; arm A by hand vs arm I with `asset_import`, both scored by `asset_import check=` |

Subsets:
- S8 = P01 P03 P05 P07 P13 P19 P21 P24, which covers every edit source;
- S8m (surface-heavy) = P03 P05 P06 P11 P17 P19 P22 P24.

`tasks.toml` holds one `[[task]]` each with `id, kind, name(s), brief, size, tol, needs, from,
check, cap`. Tasks are frozen in Phase 0. Test `bench_holdout` fails if a `lib/` leaf equals a task
name, and runs record `copied_from`.

**Runs.**
- **Agents.** One fresh subagent per run, defined in `.claude/agents/asset-bench-author.md`:
  - frontmatter: tools Bash, Read, Write; **model `sonnet`, effort `medium`, pinned**, so gate comparability does not depend on the orchestrator's model or effort;
  - prompt (~290 tokens): read only `RUN/brief.md` and `RUN/docs/`; never read repo code, tests, assets or history; drive the engine only through `scripts/asset_bench/pav RUN`; look at every requested image; ≤ 30 commands and ≤ 6 images; finish with `RUN/done.json`.
  If the pinned model is unavailable, use the closest one, record it, and re-run GA's S8 subset.
  Authors run one at a time: they keep scratch files in `/tmp`, and concurrent runs in G1's first
  batch overwrote each other's (that batch was discarded).
- **Docs.** `RUN/docs/` holds only that arm's `ASSET_STUDIO.md` (via `git show <rev>:…`; import
  tasks add `ASSET_IMPORT.md`) and a generated README. Field guides and the library are reached
  through logged calls.
- **Wrapper.** `scripts/asset_bench/pav RUN` (stdlib Python):
  - starts one `pav repl` from `.cache/asset_bench/bin/pav-<arm>` per invocation, with cwd RUN and `PAV_ASSETS`/`PAV_CREATURES` under RUN;
  - prints replies unchanged and appends `RUN/log.jsonl`;
  - refuses commands past the cap.

**Acceptance.**
- **Authoring run**, checked by `bench.py score`, which re-scores all stored runs with the newest build:
  - the asset compiles;
  - `check` reports no floating, unit_suspect, below_floor or not_grounded (the last two are skipped for hang/wall assets);
  - every axis of `size_cm` is within tolerance;
  - hang assets have max y within ±2 cm of 0; wall assets have min z in [−1, +2] cm;
  - light tasks have ≥ 1 light (format 1: an emissive part > 0.5);
  - † sockets are present.
- **Edit run:** the task's `check` passes and ≥ 2 of 3 judges answer `done` on a before/after sheet.

**Metrics** (`runs.tsv`, one row per run):
- `tok_x` = o200k(commands + replies) + image tokens (ceil(w/28)·ceil(h/28)) + o200k(all of `RUN/docs`);
- `tok_cum` = the context each invocation starts from, summed, plus the final context (this captures re-reading over turns);
- `tok_run` = runner-reported tokens (informational only);
- `turns calls failed refused first_ok lint_block lint_other accepted wall_s engine_ms parts tris size_cm copied_from model effort`;
- for import rows: `iou iou_mean de mesh_pct chars`.

Gates use `tok_x` and `tok_cum` because they are deterministic. Arm A baseline from the dogfood
stall replay: 18 calls, 3 failed, `tok_x` 10,761, `tok_cum` 50,345.

**Quality (blind).**
- **Render.** `bench.py pairs` renders each final asset with the newest build
  (`asset_preview views=true tile=320 elevation=20 figure=true`), pairs run i of an arm with run i
  of A, and stacks each pair into one 1280×648 image with a hashed name and a randomised order.
- **Judges.** Three fresh `asset-bench-judge` subagents per pair:
  - tool Read only, **model `opus`, effort `medium`, pinned**, ~270-token prompt;
  - they see the image and the brief, never the arm;
  - each returns `{"pref":1|2|0,"s1":[5],"s2":[5],"why"}` on silhouette, proportion, detail, colour/material coherence and style fit, each 1-5.
- **Aggregation.** The majority of 3 decides each pair; criterion scores take the median.
- **Calibration** (G1): 12 planted-defect pairs from the BUILTINs and 3 Gate-A assets, shown in both orders (as run: crate, bench, lantern, P01, P07 and P19, two defects each; `bench.py calib`).
  - Defects: one part floated 10 cm, y×1.8, random saturated colours, the largest part deleted, a part pushed 30% through another, every part replaced by its bounding sphere.
  - Pass: the majority picks the intact asset in ≥ 10/12 pairs in both orders, with ≤ 2 order flips.
  - One rubric rewrite is allowed, then `judge.md` is frozen and its hash is recorded.
- Optional: the user votes on 6 random pairs at G2 and G4.

**Storage and rerun.**
- **Committed:**
  - `scripts/asset_bench/{bench.py, pav, tasks.toml, README.md}`;
  - `.claude/agents/asset-bench-{author,judge}.md`;
  - `results/{runs.tsv, judgments.tsv, summary.json}`;
  - every final asset as text in `results/assets/<gate>/<arm>/`.
- **Git-ignored** (P4): run dirs, logs and PNGs in `.cache/asset_bench/`.
- **Commands:** `bench.py setup` (venv with a pinned tiktoken), `build --arm A|HEAD` (release `pav`;
  arm A from a `git worktree` at `a9ca003`, deleting its target afterwards), `prepare --gate --arm
  --tasks --runs`, then launch the author subagents, then `score`, `pairs --vs A`, launch the judges,
  `collect` and `report`.
- `bench.py selftest` replays a scripted session twice; both replays must give identical `tok_x`,
  and the 31st command must be refused.
- The harness is `scripts/asset_bench/` (README there). A session that started before `.claude/agents/` existed cannot launch the pinned agents by name; it runs a general-purpose agent with the same model, effort and prompt body, and records that.

### 10.2 Gates and stop rule
- A gate passes only if every check passes.
- A failing gate gets **one remediation round**: docs, errors, defaults, schema topics or examples,
  never format semantics.
- Token ratios are computed per task (arm ÷ mean of A on that task); gates use the median over tasks.
- If a kill criterion still holds after remediation: stop, write the NO-GO with its numbers in HISTORY
  and in this plan, keep what landed, tell the user, and advise against continuing.
- Also stop and re-plan with the user if a phase runs past twice its size estimate without passing.

| gate | runs | pass | kill |
|---|---|---|---|
| GA | A: 24 tasks + S8 second run + E1-E6 (38) | ≥ 85% of runs write `done.json`; record the median \|ln(tok_x₁/tok_x₂)\| over S8; if it exceeds ln 1.5, G2 uses 2 runs per task | fix the harness |
| G0 | no agent runs | Phase-0 checks (§7.1 reply budget test, all error probes name their part/op, template revisions unchanged, tools/list ≤ 47,000 chars, smoke script passes, `cargo test` green) | — |
| G1 | T: S8 + E1-E6 (14); calibration | Phase-1 checks (7/7 planted lint defects, sheet framing 55-90% of tile height and camera restored, triangle stats exact); calibration passes; tok_x ≤ 0.85; acceptance ≥ A's (1 task slack); `lint_other` per accepted asset ≤ 50% of A's; tok_cum ≤ A's | if calibration fails twice, structural checks plus a user spot check replace the judges |
| G2 | F: 24 + E1-E6 (30) | format checks (revision goldens, the §5.9 corpus, v1 template scenes identical, 1,024-part compile < 1 ms, < 64 B per extra instance); authoring tok_x ≤ 0.75 and tok_cum ≤ 0.65; edits tok_x ≤ 0.60; acceptance ≥ A's (1 slack); median failed ≤ 1; wins + ties/2 ≥ 12/24 and rubric Δ ≥ −0.1 | **K1** tok_x > 0.90 and wins < 15/24; **K2** wins + ties/2 ≤ 8/24; **K3** acceptance > 15 points below A's, or median failed ≥ 3 |
| G3 | FM: S8m vs F's G2 runs (8) | material checks (parity ≥ 99.9% per kernel; typical ≤ +25% and 3-layer ≤ +60% frame time; Chromium WGSL check; slot-0 goldens ≤ 1/255); 3 new materials valid first try at ≤ 150 tokens each; FM wins ≥ 5/8 | typical cost > +40% after remediation: materials for levels only |
| G4 | L: 24 + S8 second run + E1-E6 (38; 32 pairs) | library and game checks (embedded ≤ 3 MB, core reviewed, ≤ 40 tokens per listing row, dressing tests, campaign within ±20%, level/6 render ≤ +15%, wasm ≤ +3 MB raw); authoring tok_x ≤ 0.70 and tok_cum ≤ 0.50; edits ≤ 0.60; **full success** = wins ≥ 22/32 and rubric Δ ≥ +0.3; parity = wins + ties/2 ≥ 16/32 | **K4** tok_x > 0.85 and wins < 22/32, or wins + ties/2 ≤ 11/32: stop LIB-6 and I8 |
| G5 | I: IT1-IT6 + IT6 in arm A (7) | §8.8 pilot GO; ≥ 5/6 tasks accepted; IT6 tok_x ≤ 0.5× the hand-made run with IoU ≥ hand + 0.10 | **K5** pilot < 60% `fit`: stop I8; exact-mesh imports stay local |

- **Sign-test context:** 24 pairs need ≥ 17 wins for p ≈ 0.03; 32 pairs need ≥ 22 wins for p ≈ 0.025.

**Gate log** (details in `docs/HISTORY.md`; numbers in `scripts/asset_bench/results/`):
- GA, 2026-10-12: **pass.** 38/38 runs wrote `done.json`, 37 accepted (floating parts not yet
  checked); median `tok_x` 6,480, `tok_cum` 14,242, failed 0. S8 median |ln(tok_x₁/tok_x₂)| =
  0.257 < ln 1.5, so G2 runs each task once.
- G0, 2026-10-12: **pass.** Reply budget test, 19 located error probes, golden template
  revisions, tools/list 46,926 chars, smoke 12/12, `cargo test` green.
- G1, 2026-10-12: **pass** (first round). Arm T (`132447c`), S8 and E1-E6 (14 runs, authors run
  one at a time): median `tok_x` ratio 0.739, `tok_cum` 0.877; 14/14 accepted (arm A 11/14 after
  the re-score with `check`); `lint_other` per accepted asset 0 (arm A 0.273); every edit 3/3
  `done`. Calibration: intact picked 10/12 and 11/12, 1 order flip; judge prompt frozen (sha256
  `4db5021c`). Phase-1 checks: 7/7 planted lint defects, framing 55-90% with the camera
  restored, exact triangle counts. Gate A re-scored with `check`: 31/38 accepted.
- G2, 2026-10-12: **NO-GO** after the remediation round (no kill criterion held in either round).
  Format checks pass (revision goldens, the §5.9 corpus, format-1 scenes identical, 1,024-part
  compile 0.42-0.44 ms, a placed copy < 64 B).
  - Round 1, arm F (`8a0764d`): fails on tokens. Authoring `tok_x` 0.857 and `tok_cum` 1.217;
    edits 0.534; 30/30 accepted (A 25); median failed 1; 15 wins, 9 losses; rubric Δ +0.10.
  - Remediation (`c0ae045`): one-line doc examples, a one-go loop, the sheet as the image,
    short errors for split JSON, hints for `cycle` and direction words, `create` points at
    `replace`, `mirror_overlap` on boxes.
  - Round 2, arm F2 (`c0ae045`): fails on quality. Authoring `tok_x` 0.511 and `tok_cum` 0.561;
    edits 0.392; 29/30 accepted (A 25); median failed 0; 10 wins, 14 losses (parity needs 12);
    rubric Δ −0.183 (limit −0.1). 11 of 24 tasks changed outcome between rounds against the
    same arm-A assets; pooled, 25 of 48 pairs went to format 2.
  - Correction, same day: the token checks count wrapper traffic only. Counted end to end from
    the transcripts (`results/agent_tokens.tsv`: per-task ratios against arm A's mean, the
    fixed prompt held equal, visible output at 5 times input), F2's runs read 1.01 times arm
    A's tokens and cost 0.91 at cache prices (lower on 15 of 24 tasks); F's 1.20 and 1.10.
    Every model call re-reads the author's ~55k-token fixed prompt, and the format-2 runs made
    more calls (median 6 and 7 against 5). Round 2's token pass does not hold end to end.
- **Total load:** ~135 author runs and ~320 judge calls (calibration 72, edit votes 54, G2 72, G3 24, G4 96).

### 10.3 Lean gates (from 2026-10-12)
After the G2 NO-GO the user resumed Phases 3-6 in slim form under the prototype testing budget
(`AGENTS.md`, "Testing budget"). This replaces G3-G5 and the benchmark batches above:
- **G2 is closed: format 2 stays.** It works, its assets are accepted more often (29/30 against
  25/30) with half the lint findings, and it was not shown to look worse. Its cost and looks
  claims count as no proven benefit (end to end 0.91 of arm A's; pooled judged pairs 25 of 48).
- **Every row:** its own objective checks (tests, goldens, budgets, parity) pass.
- **Visual changes:** a review page the user flips through (before/after captures, one-key
  verdicts). No agent judge panels.
- **Cost claims:** at most 6 tasks, counted end to end from the session transcripts (the fields
  of `scripts/asset_bench/results/agent_tokens.tsv`). Keep the change only if it is clearly
  cheaper (lower on at least 5 of 6); otherwise it counts as no proven benefit.
- **G3** = the MAT checks (parity ≥ 99.9% per kernel, the D8 frame budget, the Chromium WGSL
  check, slot-0 goldens) + the 8-theme review page. **G4** = the library and game checks of the
  G4 row above + the Level 1 review page. **G5** = the §8.8 pilot criteria that the kept rows
  measure: ≥ 80% in the `fit` tier, median IoU ≥ 0.90, median asset ≤ 2,500 chars, ≥ 40% of fit
  assets `editable`, 0 panics and byte-identical rebuilds, plus a review page of a sample. The GPU
  cross-check and the 30 re-authorings are deferred with I4's GPU part and I7.
- A check that fails gets one fix round. If it still fails, the row ships without that feature and
  HISTORY says so.

## 11. Roadmap and progress checklist
Phases run in dependency order. Each row is one PR. Tick the row in this file in the PR that
lands it.

| phase (size) | goal | exit | what the user sees |
|---|---|---|---|
| 0 (L) | hygiene, errors, baseline | GA, G0 | ~70% smaller replies, ~60% smaller saved props, errors that name the part/op, a baseline table |
| 1 (L) | agent loop on format 1 | G1 | `check` findings in edit replies, one-call 4-view sheets with a 1.8 m figure, `capture stats=only` |
| 2 (XL) | format 2 core | **G2, the main go/no-go** | new primitives, mirror/array/radial, attach, sockets, seeded variants; bench template fixed |
| 3 (XL) | materials | G3 (lean) | planks, brick and shingles on props; flagstones and stone walls in every theme |
| 4 (XL) | library + Shardfall use | G4 (lean) | `assets find=`, ~30 curated exemplars, dressed rooms on theme materials (no theme kits) |
| 5 (XL) | import pilot | G5 (lean) | `asset_import`, a ~250-model Kenney/KayKit/Quaternius pilot with fidelity numbers, credits |
| 6 (L) | scale-out after a GO | pilot checks per pack | the GO packs on disk, searchable from `assets find=` |

Why this order:
- Phases 0-1 pay off even if format 2 stops, and they build the harness's scoring tools.
- Import comes after the library because it writes the names and tiers LIB-1 defines, and the first visible game win needs no import.
- I1-I2 may move earlier if other work blocks.

**Progress** (sizes: S < 1 day of agent work, M 1-3, L 3-7):
- [x] 0.1 TOOLS-1 reply hygiene + TOOLS-4 canonical v1 writer (M)
- [x] 0.2 TOOLS-2 `issue.rs` + format-1 error paths; TOOLS-3 schema trims and `action=schema` (M)
- [x] 0.3 EVAL-1 harness, EVAL-2 Gate A on `a9ca003`, G0 checks (M + runs)
- [x] 1.1 TOOLS-5 `check`/`dry_run`/cost line on v1 + TOOLS-8 kind routing (M)
- [x] 1.2 TOOLS-6 contact sheet + TOOLS-7 render stats (M)
- [x] 1.3 TOOLS-9 docs/rules, EVAL-3 calibration, re-score GA, EVAL-4 G1; TOOLS-11 optional (S + runs)
- [x] 2.1 F1 strict reader + v2 hints, F2 source model, writer, revisions, `needed_format` (v2 `create` refused until 2.3, when the consumers draw compiled parts) (L)
- [x] 2.2 F3 compile (all §5.4 rules) + F4 generators + the §5.9 conformance test (L)
- [x] 2.3 F5 consumers + TOOLS-10 UI adapter + lint on compiled parts (M)
- [x] 2.4 F6 patch ops v2 (expand keeps placement form) + F7 snapshot dedupe, seeds, room_check (M)
- [x] 2.5 F8 fixtures, template fix, docs, legend + EVAL-5 G2 (M + runs): **G2 NO-GO**, stopped
- [ ] 3.1 MAT-1 material core and CPU eval, seeded from `docs/asset_plan/examples/materials/` (M)
- [ ] 3.2 MAT-2 renderer + MAT-3 view integration, parity, frame-time bench, WGSL check (L)
- [ ] 3.3 MAT-4 props/statics integration + MAT-5 material tools, swatch, MATERIALS.md (M)
- [ ] 3.4 MAT-6 theme materials in levels; lean G3 (M)
- [ ] 4.1 LIB-1 names, tiers, budgets, packaging + LIB-2 index and retrieval (M)
- [ ] 4.2 LIB-3 provenance, licences, credits + LIB-5 publish/review + W0 core exemplars (~30) (L)
- [ ] 4.3 SF-1 static prop bake + SF-2 data-driven dressing; lean G4 (M)
- [ ] 5.1 I1 import refactor + I2 readers, normalisation, directory/get/list/survey (M)
- [ ] 5.2 I3 fit + I4 fidelity metrics on the CPU, compare image (L)
- [ ] 5.3 I5 catalogs, licence gate, tiers, ASSET_IMPORT.md + I6 pilot GO/NO-GO (lean G5) (M)
- [ ] 6.1 I8 scale-out of the GO packs into on-disk packs, indexed for retrieval (L)

**Deferred on 2026-10-12** (no shown need yet; feedback from the test games decides whether any
returns): LIB-4 parametric families, LIB-6 exemplar waves to ~520, SF-3 theme kits, SF-4 item and
weapon models, SF-5 monster gear, SF-6 town as data, MAT-7 texture-library fits, MAT-8 Physically
Based importer, MAT-9 material growth beyond what the rows use, MAT-10, I4's GPU cross-check, I7
re-authorings beyond the core exemplars, I9, and every judge panel.

### Work items
| id | what | key files | verify |
|---|---|---|---|
| TOOLS-1 | reply tidy + trimmed contracts + consumers | new `pav_tools/src/reply.rs`; `tools.rs` call; `asset_tools.rs` report/inspect/t_assets/edit_at; `asset_preview_tools.rs`; `live_feedback.rs`; `asset_ui.rs` report; `smoke-asset-studio.py`; new `tests/reply_budget.rs` | caps in §7.1; no f32-noise numbers; smoke passes |
| TOOLS-2 | Issue module + v1 error paths | new `pav_core/src/issue.rs`; props.rs parse/read_shape/patch; `asset_tools.rs` definition/check_args; fixtures `tests/fixtures/asset_errors.jsonl` from the 23 dogfood probes | each probe that fails on a part or op names `parts.<id>` or `ops[i]` (19 of 23; 2 are valid input, 2 are a stale revision and an existing name); did-you-mean cases |
| TOOLS-3 | schema trims, `action=schema`, opt-in legend/bounds | `tools.rs` TOOLS + schema(); `asset_tools.rs` | tools/list ≤ 47,000 chars; `schema_covers_every_field`; no `path` op in prop ops |
| TOOLS-4 | canonical v1 writer | props.rs text() | round trip; revisions unchanged; copied bench file ≤ 1,000 chars |
| TOOLS-5 | `check`, `dry_run`, cost line | new `props/lint.rs` (or `props_lint.rs` before the split); `asset_tools.rs` | bench with 7 planted defects gives exactly those codes; templates clean; ≤ 10 ms / 256 parts |
| TOOLS-6 | contact sheet + figure | `asset_preview_tools.rs` sheet(); `prop_preview.rs` figure; `tests/asset_preview.rs` | framing 55-90%; camera restored; warm 4 tiles < 1 s |
| TOOLS-7 | RenderStats + `capture stats` | `renderer.rs`; `tools.rs` t_capture | triangle sum matches an independent count |
| TOOLS-8 | kind routing for material/mesh | `tools.rs` call/schema; `live_feedback.rs` | unknown kind lists valid kinds; creature forwarding unchanged |
| TOOLS-9 | docs and rules | ASSET_STUDIO.md, AGENTS.md, `mcp.rs`, `field_guide.toml` | `cargo test -p pav_view --test learn`; instructions +≤ 25 tokens |
| TOOLS-10 | UI adapter for format 2 | props `V1Part`; `asset_ui.rs` | `cargo build/test -p pav_app`; wasm clippy |
| TOOLS-11 | optional help diet | `tools.rs` help strings | tools/list −~1k tokens, no behaviour change |
| EVAL-1 | harness | `scripts/asset_bench/*`, `.claude/agents/asset-bench-*.md`, `pav_core/tests/bench_holdout.rs` | `bench.py selftest`; holdout test |
| EVAL-2 | Gate A | `.cache/asset_bench/bin/pav-A`; results | 38 rows, ≥ 85% done; variance recorded |
| EVAL-3 | judge calibration | `bench.py calib`; judgments | §10.1 pass rule; judge prompt frozen |
| EVAL-4..8 | G1, G2, G3, G4, G5 runs | results + HISTORY | §10.2 |
| F1 | strict reader, Issues, hints | `props/{mod,read}.rs` | duplicate keys rejected with path; did-you-mean |
| F2 | v2 model, migrate/check, revisions, writer, `needed_format`, LEGEND | `props/{doc,read,write,mod}.rs`; `asset_tools.rs` definition | golden `prop1-` revisions of the templates and the dogfood v1 files unchanged; v2 fixtures rewrite byte-identically; missing unit is an error |
| F3 | compile, PropDef, registry, dependents, seeds, roles | `props/compile.rs`, `props/mod.rs`, `prop_instance.rs` | §5.9 goldens; resolved counts; identical bytes twice; 1,024 parts < 1 ms (release) |
| F4 | generators, hulls, mesh-file loader (the loader moved to I3, which writes mesh files) | `props/geom.rs` | outward winding; closed meshes; unit-normalised keys shared; mirrored mesh flips winding |
| F5 | consumers | `build.rs` emit_object/emit_prim + mesh cache; `prop_instance.rs` collider; `nav.rs`; `prop_preview.rs`; `asset_preview_tools.rs`; `asset_ui.rs`; `edit.rs` | v1 template scenes identical; each fixture renders; collider counts |
| F6 | patch ops v2, Value history, schema | `props/patch.rs`; `asset_tools.rs`; `tools.rs` | per-op tests; dry_run touches nothing; old typed history still undoes |
| F7 | snapshot dedupe, seeds, rooms, room_check | `sim.rs`, `prop_instance.rs`, `props/mod.rs`, `room.rs`, `world.rs`, `tools.rs` | < 64 B per extra instance; legacy snapshot loads; equal state_hash after save/load; field-order test |
| F8 | fixtures, bench template, docs | `pav_core/tests/prop_format2.rs`; `assets/props/templates/bench.json`; ASSET_STUDIO.md; LEGEND | docs test covers every checked key; token counts re-measured |
| MAT-1 | material core + CPU eval | `pav_core/src/material/*`; build.rs `MATERIALS`; `assets/materials/*.json` (seed from `docs/asset_plan/examples/materials/`) | schema errors with did-you-mean; inheritance limits; eval golden; every seed ≤ 6 cost units |
| MAT-2 | renderer path | `pav_render/src/{scene,renderer}.rs`; `shaders/{materials,scene}.wgsl`; examples `dump_wgsl.rs`, `material_bench.rs`; `scripts/check-wgsl.sh`; every instance construction site (`material: 0, ext: ZERO`) | slot-0 goldens; bench budget; Chromium check; wasm clippy |
| MAT-3 | view integration + parity | `pav_view/src/materials.rs`; `build.rs`; `tests/material_parity.rs` | stable slots; ≥ 99.9% parity per kernel |
| MAT-4 | props + statics integration | props materials table/`mat`/explicit colour; `statics.rs`; `asset_tools.rs` | `prop1-` revisions hold; atomic add-and-reference; snapshots round-trip |
| MAT-5 | material tools + MATERIALS.md | `tools.rs`, `asset_tools.rs`, `asset_preview_tools.rs`, `live_feedback.rs`, `mcp.rs`, `renderer.rs` stats | swatch headless; fresh agent writes 3 valid materials ≤ 150 tokens each |
| MAT-6 | theme materials | `arpg/world.rs`; `game/themes.toml`; `assets/materials/{flagstones,dungeon_wall,pillar_stone,wall_cap}.json` | identical mechanics, packs and levelmap for 5 seeds; campaign passes; 8-theme captures reviewed |
| MAT-7 | texture-library fits | `pav_tools/src/import/{material_fit,pattern_classify}.rs`; catalogs; `assets/materials/fit/` | synthetic recipes fit back (type, size ±10%, ΔE ≤ 1); pilot of 20+20 with review |
| MAT-8 | Physically Based presets | `import/physically_based.rs`; `assets/materials/pb/*.json` | gold → #d1a400, iron → #908c87, non-metals exact |
| MAT-9 | material library growth | `assets/materials/*.json` (≥ 24 more) | all load, ≤ 6 cost units, ≤ 160 tokens; family sheets reviewed |
| MAT-10 | gated options | shaders, renderer | only with a measured need |
| LIB-1 | names, tiers, budgets, packaging | build.rs `ASSET_DATA`; props canonical/resolve_ref/disk_root; `asset_tools.rs`; `arpg/data.rs`; `.gitignore`; `.vercelignore`; `package-studio.py` | budget/validity/refs tests; `scripts/build-web.sh` |
| LIB-2 | index + retrieval + `category` | `asset_tools.rs` t_assets; `tools.rs`; `mcp.rs` | ranking, filters, paging, similar; ≤ 40 tokens per row |
| LIB-3 | provenance, licence gates, credits | props source/PACK.json; `asset_tools.rs`; `assets/CREDITS.md`; README | redistributable + credits-current tests; local refusal |
| LIB-4 | parametric families | `props/{read,compile,patch}.rs`; `prop_instance.rs`; `room.rs`; `world.rs`; preview/spawn | fence family: default/low/long = 13/11/17 parts; snapshot round trip |
| LIB-5 | publish/review + W0 | `asset_tools.rs` publish; `live_feedback.rs`; `assets/props/lib/core/*` | `core_assets_are_reviewed`; zero lint; per-asset sheets; old six kinds compared side by side |
| LIB-6 | growth waves | `assets/props/lib/**` | per-wave report; budgets green |
| SF-1 | static prop bake | `statics.rs`, `nav.rs`, `sim.rs`, `build.rs` | colliders across activate/deactivate; nav; snapshot hash |
| SF-2 | data dressing | `game/props.toml`; `arpg/world.rs`; `arpg/data.rs`; `tests/levels.rs` | §9.6 tests; levelmap; campaign; level/6 ≈ 240 entities |
| SF-3 | theme kits + HIDDEN flag | `themes.toml`; `arpg/world.rs`; `statics.rs`; `build.rs`; `agent_tools.rs` theme_swatch | levelmap identical with kit on/off; rewind test; render ≤ +15% |
| SF-4 | item/weapon models | `items.toml`; `items.rs`; `arpg/mod.rs`; `puppet.rs`; `build.rs`; `pav_view/src/arpg.rs` | looks parse unchanged; turntable per tier; loot on the ground; capsule fallback |
| SF-5 | monster gear | `parts.rs`; `puppet.rs`; `genome.rs`; `genome.toml` | genomes 1..200 equal except gear |
| SF-6 | town as data | `game/town.toml`; `arpg/scene.rs` | `tests/town.rs` unchanged; NPC routes walkable |
| I1 | import refactor | `pav_tools/src/import/*`; mocap; `anim_tools.rs` | tests unchanged; byte-identical `anim/mesh2motion.json` |
| I2 | Scene, readers, normalisation, directory/get/list/survey | `import/{scene,gltf,obj,image,normalize,libraries,licence}.rs`; `asset_import.rs`; Cargo (png, tobj); tests | reader tests; Khronos corpus (else the gltf switch rule) |
| I3 | translation | `import/fit/*` | table/vase/blade/blob tests; Kenney MD median IoU ≥ 0.95 (auto), ≤ 300 tokens |
| I4 | metrics, GPU check, compare, `check=` | `import/{metrics,gpu_check}.rs` | \|iou_gpu − iou\| ≤ 0.03; compare image |
| I5 | catalogs, licence gate, tiers, docs | `import/{licence,catalog}.rs`; `assets/catalogs/kenney.json`; `.gitignore`; `.vercelignore`; `docs/ASSET_IMPORT.md` | licence tests; identical tree hash on rebuild |
| I6 | pilot | catalogs, ledgers, packs | §8.8 GO/NO-GO recorded |
| I7 | ≥ 30 re-authorings | `assets/props/lib/core/*` | ≥ 70% accepted; median chars −20% |
| I8 | scale-out | catalogs, ledgers, packs | per-pack ledgers; gates hold; within D4 budget |
| I9 | FBX geometry / JPEG | `import/fbx_geom.rs`; Cargo | only when needed |

## 12. Risks
| risk | mitigation |
|---|---|
| Agents misread the format-2 legend (both prototype implementations came from one model family) | fresh-agent runs at G2; conformance goldens pin the five rules that diverged |
| `prop1-` revision churn stales `if_revision` values and resets histories | capture golden revisions before F2; `V1Mirror` copies today's field order |
| cm/m confusion (rooms in metres, assets in cm) gives 100× errors | required `unit`, `size_cm` echoed, `unit_suspect`, `op:"unit"` |
| Material cost on phones and in the browser | caps, cost units, sub-pixel skipping, `material_detail` setting (D7), bench gate, measure on SwiftShader and one real phone before enabling on mobile |
| naga accepts WGSL that Chrome's Tint rejects | derivative-free shaders, binding 2, `check-wgsl.sh` on every WGSL change |
| KayKit-style packs stay mesh-heavy | mesh parts keep fidelity; `editable` flag and ≥ 40% gate; re-authoring; scope decided at the pilot |
| Licence leakage (QAL, custom CC0, CC-BY in the public build) | SPDX allowlist with default-deny, pinned licence-text hashes, local tiers, `lib/core` CC0-only test |
| Untrusted downloads (zip bombs, traversal, malformed glTF) | basename-only names, 256 MiB entry cap, CRC32, scene limits, no-panic tests |
| Dressing blocks paths or moves mechanics | dressing runs last on its own stream; non-interference, path and campaign tests |
| Snapshot dedupe depends on field order | declaration order + test; fallback relink pass |
| VLM judges are noisy (~0.66 agreement with humans) | calibration, majority of 3, randomised order, structural floor, optional human votes |
| Benchmark spend | lean scale (D10), subsets at G1/G3, A measured once, final assets kept for re-judging |
| Repo and wasm growth | embedded ≤ 3 MB and disk ≤ 40 MB tests; wasm growth recorded per phase |
| Overfitting the format to a few examples | 24-task benchmark across 13 categories plus the import pilot |

## 13. Out of scope
- Photoreal fidelity: photoreal sources only inform fitted materials.
- Skinned or animated character and creature import.
- Bitmap/UV textures, texture painting, texture arrays, bump maps, transparency (MAT-10 options only).
- CSG booleans, and meshes edited by agents.
- Node-graph, GLSL or MaterialX authoring.
- LOD and culling.
- New egui panels.
- FBX geometry and JPEG, until a needed pack requires them.
- CC-BY or restricted licences in the public build, and an in-game credits screen.
- Creature Studio changes.

## 14. Reference material (`docs/asset_plan/`)
These are planning artefacts. Use them as starting points, not product code. Delete each one when
the work item that uses it lands. Each is listed with the work item that uses it.
- `examples/format2/`: removed when F8 landed; the bench, market_stall, well, wall_torch, greatsword and the probes probe_mirror, probe_frames, probe_attach are `crates/pav_core/tests/fixtures/props/format2/` (the §5.9 corpus).
- `examples/materials/`: 10 materials plus `pb/gold`, `pb/iron`, converted to `unit:"cm"` (MAT-1).
- `examples/families/fence.f2.json` (LIB-4).
- `prototypes/f2expand.py`: independent stdlib reference expander, format 2 → format 1. Run it with `--anchors fix`. It treats capsule from/to as cap centres; §5.2 supersedes that.
- `prototypes/materials/`: `materials.wgsl` (494-line kernel library), diffs against `scene.wgsl`/`renderer.rs`/`scene.rs`, and the parity/bench harness `main.rs`, which includes the CPU mirror (MAT-1..3).
- `prototypes/material_fit/fitprobe.py` (MAT-7, numpy).
- `prototypes/lint4.py`: removed when TOOLS-5 landed (`crates/pav_core/src/props/lint.rs`).
- `prototypes/tools/`: removed when TOOLS-1 (`trim.py`) and TOOLS-3 (`schema_v1.json`) landed.
- `prototypes/import/`: `gltfload.py`, `structure.py`, `fitproto3.py`, `summarize3.py` (I3, I4, numpy).
- `prototypes/families/apply_params.py` (LIB-4).
- `prototypes/bench/`: removed when EVAL-1 landed (`scripts/asset_bench/`); the dogfood stall replay is now `scripts/asset_bench/selftest.cmds`.
- The 23 error probes became `crates/pav_tools/tests/fixtures/asset_errors.jsonl` with TOOLS-2. The format-1 baselines `dogfood_stall_v1.json` and `dogfood_well_v1.json` moved to `crates/pav_core/tests/fixtures/props/` with TOOLS-4 (golden revisions; F2 reuses them).

## 15. Executing this plan

**Effort level: `high`, raised to `xhigh` for Phases 2 and 3.** The priority is quality per token,
not speed.
- **Why high is enough for most of it.** The design work is done: this file fixes semantics,
  thresholds and tests, so most rows are careful execution of a spec, where high gives the best
  quality per token.
- **Where xhigh pays.** Phases 2 and 3 carry the subtle invariants: compile semantics against
  0.01 cm goldens, revision stability, snapshot field order, Tint-safe WGSL and CPU/GPU parity.
  A miss there means a failed G2 or G3 and re-running benchmark batches. Use xhigh for
  rows 2.1-2.5 and 3.1-3.4.
- **Not max.** It has diminishing returns on mechanical rows.
- **Not ultracode.** Unconstrained multi-agent spend buys wall-clock speed, which ranks last. The
  benchmark and exemplar batches already run as parallel subagents with pinned models, so
  orchestration effort does not change gate results.

**Execution prompt** (paste after `/clear`):

```
Execute the asset-system plan in docs/ASSET_SYSTEM_PLAN.md. Read docs/DOCTRINE.md, AGENTS.md,
docs/PROGRESS.md and the plan, then resume at the first unticked row of the plan's §11 Progress
checklist; rows run in order and the plan's sections specify each work item. The §3 decisions
are approved with their defaults unless I say otherwise. Land each row as its own PR into main,
merge it and delete its branch (AGENTS.md standing instruction; don't ask); after a merge,
restart your working branch from the updated main. Each PR adds a dated docs/HISTORY.md entry,
updates docs/PROGRESS.md and ticks its row. Close a phase with its lean gate (§10.3): objective
checks, a review page for visual changes, and at most 6 end-to-end runs for a cost claim; no judge
panels. A failing check gets one fix round, then the row ships without that feature and HISTORY
says so. Follow AGENTS.md "Testing budget". Escalate only what the doctrine requires. Never
commit images, downloads or scratch files; raw downloads go to .cache/. Never put personal data
in request headers. Prefer the CLI and capture tools over reading code.
```
