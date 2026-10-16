# Asset Import

`asset_import` turns open model libraries (glTF, OBJ) into format-2 props: primitives where
they fit, plus at most one read-only vertex-coloured mesh part for the rest. A rigged humanoid
becomes a figure instead ("Characters" below). Most of the time you do not import: `assets
find=words` already lists the imported packs.

## Use what is imported

- `assets find=barrel` ranks every pack's assets with the templates and the workshop; each row
  says its store tier (`standard`, `checked`, `disk`) and mesh share. `tier=disk` keeps committed
  packs (`LIB/kenney_mini_dungeon/...`, `LIB/quaternius_medieval_village/...`), `tier=checked`
  their editable, lint-clean assets, `tier=local` those kept on this machine (left out otherwise).
- A library asset is read-only: `asset_edit action=copy from=LIB/<pack>/<name> name=x`, then edit
  the copy. Never edit a `{"mesh":...}` part's file; recolour it (`recolor`) or replace it with
  primitives.
- Game code draws one with `emit_asset(scene, "LIB/<pack>/<name>", ...)`; rooms and game data may
  name only assets every build carries (BUILTIN, LIB/core, and the shipped packs in
  `props::store::SHIPPED`).

## Import a pack

```
asset_import                                      # the libraries, their pinned packs, what is cached
asset_import get=kenney/castle_kit                # download into .cache/assets/, SHA-256 checked
asset_import from=kenney/castle_kit               # translate, route by licence, index, credits
asset_import survey=kenney/castle_kit             # fit without writing: the ledger
```

- A pack is pinned in `assets/catalogs/<lib>.json` under `$sources`: url and archive `sha256`
  (or `repo`, `version` = commit, `files` and the SHA-256 of their sha256sum lines), the
  `licence_file` and its `licence_sha256`, the `license` (SPDX), `models` (a glob), `pack` (the
  LIB name: `[a-z][a-z0-9_]{0,31}`), placement (`up`, `yaw`, `scale` metres per unit, `place`
  keep|floor|center), `label` and `credit` (its `PACK.json`). `get` refuses a changed archive or
  licence, and a licence file that does not say the `license`.
- After the source, one line per model, written by hand or by a script and read over:
  `"<pack>/<id>": ["<category> <2-6 tags>", "<description>"]`. The first word is the category
  (furniture, container, architecture, lighting, vegetation, rock, weapon, armour, tool, food,
  town, dungeon, signage, character); the tags are what `assets find=` matches besides the name.
  `$skip`: `"<pack>/<id>": "why"` (effects; a rigged model whose skeleton no table knows is
  skipped anyway, with the reason). A source
  with `"listed": true` translates only the models it has a line for. A line naming no model is
  an error.
- `from=` writes `<pack>/<name>.json` and `<pack>/meshes/<name>.json`, removes files of models it
  no longer writes, then reindexes the pack and rewrites `assets/props/CREDITS.md`. The reply
  has the tiers, medians and one row per model; `to` says where it went and `why`.
- A `lib/` pack is translated only from the cache `get` fetched under the catalog's current pin
  and licence (after changing either, `get=` again: it re-reads the licence file). Loose files and
  folders go to `local/import/`, where each import keeps the others.
- A `lib/` source pins `sha256`, `licence_file` and `licence_sha256`. Each catalog source writes
  its own `pack` (two sources naming one pack are refused; `import` is the loose files'). `out=`
  never points into `lib/` or `templates/`.
- A model that cannot be read on a rebuild keeps its last files; a pack that changes tiers or
  licence is rebuilt only without such errors, and moves only whole.
- `lib/` also needs the archive's SHA-256 checked by `get`. A pack over 64 MB is read by ranges
  and stays local. A part of a pack cannot change the pack's licence, credit or source. A
  renamed pack is refused while its old `lib/` folder still holds the source.
- Loose files of the same name replace each other in `local/import/`; `out=` keeps them apart.

## Where translations go (the licence gate)

| licence | folder | git |
|---|---|---|
| CC0-1.0, CC-BY-3.0/4.0 | `assets/props/lib/<pack>/` with `PACK.json` | committed (≤ 40 MB, ≤ 1 MB a file) |
| CC-BY-SA, NC, ND, OGA-BY, GPL, QAL, store licences | `assets/props/local/<pack>/` | ignored |
| a catalog source with `"local": "why"` | `local/` | ignored |
| a file or folder outside a pinned pack | `local/import/` (or `out=`) | ignored |
| anything else | refused: read the licence, add it to `import/licence.rs` | |

Every asset in a `lib/` pack names the pack's licence in `source.license`; the tests
`committed_packs_are_redistributable`, `committed_packs_fit_budget`, `index_files_are_current`
and `credits_file_is_current` hold that. Moving a pack between `lib/` and `local/` (by editing
the catalog and translating again) moves its files and keeps the index notes.

## Fidelity tiers

| tier | means |
|---|---|
| `fit` | worst-view IoU ≥ 0.85, mean ≥ 0.90, ΔE ≤ 6, ≤ 6,000 chars; `editable` when ≤ 30% of its area is mesh |
| `draft` | IoU ≥ 0.75, ΔE ≤ 10, or too long for `fit` (a well of 96 pieces: 11k chars): written, worth a rewrite |
| `exact` | the whole model as one mesh part (`fit=mesh`) at IoU ≥ 0.98 |
| `fail` | not written; the ledger row says so |

A model whose material names a base texture that is missing, unreadable or not decoded (a
JPEG, WebP or broken PNG) is not translated: the error names the material and the texture
(`material wood: texture crate_wood.png: …`), since it would draw white and ΔE would not see it.
A survey still lists it (flag `undecoded`, or a warning).

A pack goes to `lib/` after a pilot: ≥ 80% `fit`, median IoU ≥ 0.90, median ≤ 2,500 chars,
≥ 40% of fit assets editable, no panics, identical bytes on a rebuild (plan §10.3, G5). A pack
that misses one is kept `local` with the reason in its catalog.

## Check one asset

```
asset_import check=assets/props/lib/kenney_mini_dungeon/chair.json compare=true
```

measures it against its source again (the `source` field names the pack and model, `source.norm`
any placement other than the pin) and returns the image: the source's six views over the
translation's (front, back, left, right, top, iso). Look at it before trusting a number: IoU
misses colour patches and thin detail. `from=<model file> compare=true out=<folder>` translates
one model with the image; `fit=prims` (no mesh part) or `fit=mesh` (all mesh) force a mode.

## Characters (figures)

A skinned model whose skeleton a bone-name table recognises (`import/skeleton.rs`: VRM, Unity,
Mixamo, Unreal 4/5, Rigify, CMU, 100STYLE, ported from my-3danimations' `tables.ts`, MIT; plus
KayKit's own names) is translated into a **figure** (`pav_core::figure`): rigid per-bone
segments that play every clip and move the puppets play. Nothing is skinned; each segment
moves whole (S3b; skinning waits for the user's call on the review page).

```
asset_import from=kaykit/adventurers              # Knight, Barbarian, Mage, Rogue, Rogue_Hooded
asset_edit action=check name=LIB/kaykit_adventurers/knight      # prop and figure findings
animsheet def={"figure":"LIB/kaykit_adventurers/knight"} clip=CMU/Wave_Hello tilt=56
```

- **Bones to segments.** The engine's figure plan has 16 segments: `hips spine chest head`,
  `upper_arm lower_arm hand`, `upper_leg lower_leg foot` (`_l`, `_r`: the rig's own sides). The
  table maps the rig's bones to VRM 1.0 bones and each VRM bone folds into a segment (shoulders,
  upper chest and neck into the chest; fingers into the hand; toes into the foot). Each triangle
  goes to the segment its vertices' joint weights favour; a bone the table does not name (IK
  helpers, KayKit's `hand.l`) gives its triangles to its nearest mapped ancestor. Rigid meshes
  ride with their bone (helmets, hats, capes); those held in a hand slot (weapons, shields,
  books) are left out: a look's weapon is drawn by the engine. The 15 VRM-required bones must
  map, or the model is refused naming what it lacks; a model facing −z is refused (`yaw=180`).
- **Files** (all text, in the pack's `lib/` folder like props):
  - `<name>.json`: the body, a format-2 prop of category `character`: one mesh part per segment,
    named after it, placed in a standing display pose (what the Catalogue and previews show);
  - `meshes/<name>_<segment>.json`: each segment's mesh in the segment's own frame (origin at its
    joint; the torso and head in the character's axes; a limb's +y along the bone and +z where
    its middle joint bends out, elbows back, knees forward; a foot's +y forward, +z up);
  - `rigs/<name>.json`: format 1, `unit: cm`: `table`, `look` (the `PuppetDef` proportions read
    from the bones, metres: leg and arm length, torso length, hip and shoulder half-widths,
    head, torso and limb radii), and per segment the model's `bones`, its rest `joint`, `tail`
    and `tris`; `folded` (unnamed bones and kept meshes, by the segment they went to) and
    `left_out`.
- **Drawing.** A look naming `figure` (`PuppetDef.figure`) draws the figure in place of the
  puppet's parts: the puppet's skeleton is posed at the figure's proportions, the torso and
  head turn with the pelvis, chest and head, each limb's root follows its parent, its end the
  puppet's hand or ankle, and its middle joint is solved again at the figure's own unequal
  bone lengths; the body then stands with its lowest point where the puppet's is. Limbs follow
  the puppet's side they sit on (+x is the puppet's index 1, the clips' right): figures perform
  clips as the puppets do. Game code calls `pav_view::assets::emit_figure` (the same part
  drawing as `emit_asset`). Desktop builds only (the rig is read from disk).
- **Check.** `asset_edit action=check` on a figure's body adds the figure's findings: a required
  segment missing, fewer than 12 segments drawn, a segment's mesh far from its bone, a joint
  outside the box of the mesh it hangs from (the body would come apart), proportions out of a
  biped's range, a body off the floor when it stands. The prop lint's `mirror` does not apply
  (imported costumes are asymmetric), and a body may draw 16 meshes.
- **Store.** Index rows of category `character` are kind `character` (`STORE.tsv`, the licence
  gate in `store/licences.tsv`); `assets find=knight` finds them. They are 100% mesh, so they
  stay at `disk` (§4's mesh cap) and the Catalogue room shows them.
- **Size.** A figure over 8,000 triangles is cut by vertex clustering (`character::BUDGET`);
  KayKit's adventurers (4,000-4,800) are written whole: 86-103 KB of mesh text, 2 KB of body and
  2 KB of rig each. The pack's catalog scale (0.8) stands them 1.75-2.17 m tall, hats and helmets included.
- **Limits.** Rigid joints show at elbows and knees on smooth bodies (blocky low-poly packs hide
  them); no wrists, fingers, toes or separate neck; the puppet's squash and stretch and its
  camera cheats do not reach figures; weapons are not drawn in a figure's hand yet; the sim's
  capsule is the look's, not the figure's.

## Known limits

- Facing is per pack (`yaw`), not per model: Kenney chairs face +z, KayKit's −x.
- One connected mesh painted in several colours (castle walls, most food) becomes one mesh part:
  exact to look at, not editable.
- Sizes are the pack's own unless the catalog scales it (Food Kit 0.3, Quaternius 3).
- JPEG textures and FBX-only packs are not read (plan I9): a JPEG-textured model fails to
  translate.
