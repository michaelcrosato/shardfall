# The agent asset store

Foundational assets for games on the engine: objects (props), motion clips and materials, each
in its own text format, with generated rows saying what it is, where it came from and how far it
is trusted. Start from an entry instead of from nothing. Design: docs/AGENT_ASSET_STORE.md.

## Find

- `assets find="wooden table"`: object rows `name cat size_cm tier mesh tok tags desc`, best
  first. Plurals and synonyms match (`store/synonyms.tsv`); filters `category=`, `tags=`
  (`@sit` is a socket), `tier=standard`, `similar=NAME`.
- `assets find=walk kind=clip` and `assets find=planks kind=material` search clips and materials.
- A miss says `none` with the nearest rows and a route (what to make, or where else to look) and
  is logged to `store/misses.tsv`, so the store learns what games look for.
- `store/STORE.tsv` lists every entry of every kind in one table (generated; search, don't read
  it whole).

## The shelf

`assets shelf=prop` and `assets shelf=clip`: the best entries of a kind by category, one call,
about 1,500 tokens. Read the shelf before a scene that needs several things. Props lead with
the foundations (`store/foundations.tsv`: a few per archetype, their knobs, tested examples and
the technique each shows): place a variant, e.g. `asset_spawn name=LIB/core/table:wide`, before
making a new asset.

## Tiers

| Tier | Where | Means |
|---|---|---|
| standard | `LIB/core`, the templates, embedded clip sets, core materials | in every build; rooms and game data may name it |
| checked | packs on disk (`LIB/<pack>`) | no `check` findings, described, 2-6 tags, within its token cap, ≤ 30% mesh |
| disk | packs on disk | translated, but not yet checked |

Only standard entries may be named by game data and rooms. Standard holds CC0 and own work;
CC-BY (100STYLE) and CMU clips stay `checked`.

## Licences

The store admits CC0-1.0, CC-BY-3.0 and 4.0 (credit carried) and, for clips, CMU's terms
(`store/licences.tsv`). Own work is CC0-1.0. Everything else is excluded: no row, no copy.
`assets/props/local` and `anim/local` hold sources that may not be shared; the store never
indexes them, and nothing from them goes into the library.

## Fork and learn

Entries double as examples: inspect one (`asset_edit action=inspect name=LIB/core/chair`), then
fork it rather than starting blank:

    asset_edit action=copy from=LIB/kenney_mini_dungeon/barrel name=cask

The copy records its lineage in the document:
`"lineage":{"method":"edited","origin":"import","parents":["LIB/kenney_mini_dungeon/barrel@prop2-…"],"license":"CC0-1.0","credit":"Kenney (www.kenney.nl)"}`.
`origin` is where the line starts (`import`, `agent`, `human`, `generator`) and every fork
inherits it with the licence and credit. Edits and publish keep the lineage; a patch cannot set
it. The index's `licence` and `method` columns (`authored`, `imported`, `edited`, `generated`)
read it. Patch the fork in one batch, look at a sheet (`asset_preview views=true`), place it with
`asset_spawn`. Workflow: docs/ASSET_STUDIO.md; materials: docs/MATERIALS.md; clips:
docs/ANIMATION_STUDIO.md.

## Publish to standard

`asset_edit action=publish name=x if_revision=R review="ok: …"` puts a workshop asset into
`LIB/core` after you look at its sheet at revision R. The gate: no `check` findings, a
description of 1-120 characters, a category, 2-6 tags, the category's token cap, library
references and materials only, and complete provenance (an admitted licence that is the pack's
CC0, an edit's parents, a CC-BY credit). It writes the file, its index row and a review line;
rebuild to embed it. Good enough is the bar: basic checks and one sheet, no polishing rounds.

## Credits

`store/CREDITS.md` names every clip set and object pack with its licence and credit line. A
CC-BY credit travels with every copy and fork (it is in the lineage).

## Generated files

`pav assets reindex=store` writes `store/CREDITS.md` and `store/STORE.tsv`; `pav assets
reindex=<pack>` (or `=templates`, `=core`) rewrites a pack's `index.tsv`. Tests fail when any
is stale. Never edit them by hand; edit the assets, the `PACK.json` or the clip sets.
