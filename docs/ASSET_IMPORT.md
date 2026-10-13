# Asset Import

`asset_import` turns open model libraries (glTF, OBJ) into format-2 props: primitives where
they fit, plus at most one read-only vertex-coloured mesh part for the rest. Most of the time
you do not import: `assets find=words` already lists the imported packs.

## Use what is imported

- `assets find=barrel` ranks every pack's assets with the templates and the workshop.
  `tier=disk` keeps committed packs (`LIB/kenney_mini_dungeon/...`, `LIB/quaternius_medieval_village/...`),
  `tier=local` those kept on this machine.
- A library asset is read-only: `asset_edit action=copy from=LIB/<pack>/<name> name=x`, then edit
  the copy. Never edit a `{"mesh":...}` part's file; recolour it (`recolor`) or replace it with
  primitives.
- Game code draws one with `emit_asset(scene, "LIB/<pack>/<name>", ...)`; rooms and game data may
  name only embedded assets (BUILTIN, LIB/core).

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
  town, dungeon, signage); the tags are what `assets find=` matches besides the name. `$skip`:
  `"<pack>/<id>": "why"` (rigged characters, effects; rigged models are skipped anyway). A source
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

## Known limits

- Facing is per pack (`yaw`), not per model: Kenney chairs face +z, KayKit's −x.
- One connected mesh painted in several colours (castle walls, most food) becomes one mesh part:
  exact to look at, not editable.
- Sizes are the pack's own unless the catalog scales it (Food Kit 0.3, Quaternius 3).
- JPEG textures and FBX-only packs are not read (plan I9).
