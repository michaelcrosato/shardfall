# The agent asset store

A library of game assets that agents search, use as they are, fork and change, or learn from. It
plays the part Epic's Fab store plays for people, but its customer is an agent. Status: design
only; nothing here is built. Facts come from a read-only survey of Shardfall and its sibling
repositories on 2026-10-14, re-checked where noted; *unverified* marks what was not checked.
Agents using the store read `store/README.md` and the shelf (built in S0-S2), not this file.

## 1. The idea in one page

- **Text only, generated from what exists.** Assets stay in their engines' text formats (format-2
  props, material recipes, key-pose clips); the store adds generated rows, never a second copy.
- **Agents choose from rows:** one row per entry (about 50 tokens), one `find` with a `kind=`
  facet, and a short shelf per kind fetched with one call.
- **A proof of concept first, built to scale:** props, materials and clips, one or two key packs
  each, run end to end, inside Shardfall. Other kinds are designed here and enter when games ask.
- **Only what the library may redistribute and modify:** CC0, CC-BY with its credit, our own
  work. Everything else is excluded entirely: no local tier in the store, no translated copies.
  Shardfall's existing private tiers (`anim/local`, `assets/props/local`) stay outside it.
- **Seeded only to critical mass** (S3), then grown by agent-made assets and by what the user flags
  in test games. Over time creation tools and curation matter more than importers.
- **Open-source imports are a bootstrap,** translated into the formats agents create in, so they
  double as exemplars. Per kind they stop when agent-made assets match or beat them on the user's
  blind review page and test games stop needing them; then their files go (S6, §14).

**Feasibility: yes, and no breakthrough is needed.** Most of it is ordinary engineering in small
stages; the hard parts are measurement and taste. The biggest risk (does a library pay at all?)
is measured in S1, after a few days of work and before anything else is built.

| Ordinary engineering | Genuinely hard |
|---|---|
| Generated rows; byte-identical rebuild tests | **Showing a library helps at all.** Never measured here: the plan's library arm and import tasks IT1-IT6 never ran. riftlight's Gate A failed three times on tokens (R = 0.98, 1.02, then 1.75 with the full library, because its frozen prompt made the agent read the whole 233 KB source instead of searching); that last run won 3 of 4 briefs on quality. |
| Licence gate (exists, default-deny); credits text | **Judging "looks good".** Shardfall's agent judges flipped 11 of 24 tasks between rounds against identical baselines, each reading about 120k tokens to see one image; SpawnForge's single-order reviewers picked side B in 60-75% of pairs. Looks go to the user's review page, never to agent judges. |
| Size and socket lints; planted-defect tests | **Faithful translation of imports.** Textures become one colour per triangle; 31% of the pilot's 264 fits came out editable; photoreal and trim-sheet packs do not translate. |
| Pinned imports | **Characters across rigs.** my-3danimations' retarget gate G4 run 1 failed 5 of 8 steps; its sweep read 89.1% under the original rule and 97.2% under the owner's later reading (its D80; KayKit Knight lowest at 94.1%). Run 2 is pending. |
| Lexical search to about 1,000 rows; two demo rooms | **Search over 10,000+ mixed assets.** Unproven anywhere in the family (§8). |

**Rulings needed from the user:** nine, each with a default an agent applies and logs after 15
minutes without an answer (§14). Nothing has to be created until a second engine needs props.

**Words.** *Row / entry:* a generated index line / its JSON form. *Shelf:* the best entries of
one kind. *Ledger:* a catalog TSV of every candidate in a pack (licence, pick or skip). *IoU:*
silhouette overlap, 1.0 identical. *ΔE:* colour difference, ≤ 6 barely visible. *Mesh share:* an
import's area left as read-only mesh; *editable* means ≤ 30%. *Contact sheet:* four headless views
beside a 1.8 m figure. *Review page:* HTML from `scripts/review_page.py` with one-key verdicts;
*both orders:* each pair shown twice, sides swapped. *find@5:* a fitting asset is in the top 5.
*Yank:* withdraw an entry; its id stays reserved.

## 2. How it follows the doctrine

**The cockpit comes out.** Fab serves a person with a browser: thumbnails, stars, a cart. Here
agents use `find`, rows, lints and contact sheets through tools, and the user steers through flags
on review pages. **The Sherman, not the Tiger:** files in git, lexical search, and the formats,
gates and tiers that already exist; no service, database, embeddings, new geometry format, USD,
Lottie or Spine.

Principles 1-3: short TSV and JSON, every action a tool call, every check headless. 4: text only;
raw GLB, PNG and FBX stay in `.cache/`, keyed by SHA-256. 5: pinned sources, byte-identical
rebuilds. 7 and 9: glTF frame, VRM 1.0 bone names, SPDX ids. 10: key packs only; the rest waits
for triggers. Choices that touch decisions the user already made (plan D5, local licence tiers,
the P4 reading for sprites) are escalations with defaults in §14, not settled here.

## 3. What already exists

**Shardfall** (`/home/user/shardfall`), re-checked 2026-10-14:

| What | Count | Where | Note |
|---|---|---|---|
| Core props | 28 reviewed (36 KB); 4 BUILTIN templates | `assets/props/lib/core/`, `assets/props/templates/` | embedded, web build too; `PACK.json`: CC0-1.0, own work |
| Imported props | 61: Kenney Mini Dungeon 28, Quaternius Medieval Village 33 (288 KB) | `assets/props/lib/<pack>/` | native only; 59 `fit`, 2 `draft` (Cart, Well); 30 have `check` findings; 29 over 30% mesh; no descriptions |
| Import catalogs | 267 model lines, every description empty | `assets/catalogs/*.json`, ledgers `*/*.tsv` | pilot: 99% `fit`, median IoU 0.983, 31% editable |
| Materials | 15 recipes; 13 own with no licence field, `pb/gold`, `pb/iron` CC0 | `assets/materials/` | procedural, no images, no UVs |
| Clips, embedded | 437: CMU 60, Mesh2Motion 177 (CC0), Quaternius 102 (CC0), STYLE100 98 (CC BY 4.0); 2.5 MB; 31 procedural moves | `anim/*.json`, `anim/moves.toml` | AGENTS.md says 423 (stale) |
| Clips, on disk | 100STYLE 780 (2.8 MB); CMU 4,770 (68 MB) plus 2,548 surveyed takes | `anim/100style/`, `anim/cmu/` | `clips load=` |
| Creatures | 8 SpawnForge templates over 81 modules | `tools/creature-compiler/` | upstream private, UNLICENSED |
| Feel | 4 vehicles: cars `grippy`, `drifty`, 2 helicopters; 4 movement models | `rooms/drift.toml:378,384`, `rooms/helicopter.toml:471,477`, `crates/pav_core/src/character.rs:24` | movement models are Rust enum variants; `feel.rs` measures the player only |
| FX | 13 particle presets; lights; distortion | `crates/pav_core/src/props/doc.rs:183`, `crates/pav_view/src/fx.rs:14` | view-only |
| Looks, sound | styles and screen filters as `[view]` settings; a synth `Patch` | `rooms/styles.toml`, `rooms/filters.toml`; `crates/pav_audio/src/lib.rs:21` | events map to sounds in code (`sounds_for`, l.210) |
| Levels | 42 rooms in 7 wings; glyph layers; 8 themes | `rooms/`, `game/themes.toml` | wings hard-coded in `crates/pav_core/src/world.rs` |
| Tools | importer, licence gate, publish gate, `review.tsv`, index search, review pages, asset benchmark | `crates/pav_tools/src/{asset_import,import/licence,asset_library,asset_search}.rs`, `scripts/` | no LICENSE file in the repository |

Not in this checkout, contents *unverified*: `assets/props/local/` (Kenney Castle, Nature, Food,
KayKit Dungeon: CC0, held back because 20-35% of fits were editable; ledgers committed under
`assets/catalogs/`) and `anim/local/` (Bandai Namco, LaFAN1, Mixamo, held back for licences).

**Skinning already exists.** `CreatureAsset::skin` (`crates/pav_core/src/creatures.rs:422`) does
CPU linear-blend skinning, 4 weights per vertex, for compiled SpawnForge creatures, in the studio
preview only (one `RenderFrame.creature` slot; no actors). Imported characters are not wired to it.

**Siblings** (`/home/user/michaelcrosato/`):

| Repo | What it has | Status |
|---|---|---|
| my-3danimations | 1,398 clips in 11 sets (1,199 unique); CC0 pack 366; format 2.0, one clip per file; VRM 1.0 skeleton; search 40/40 written, 8/10 held out | phase 6 of 10; G4 run 2 pending; publishes nothing yet (its O2); acts in other repositories only by proposal (its O9) |
| my-3dge | 4 props; 10 clip sets, about 1,220 files (5.6 MB); 13 texture generators; a 7-shape geometry kit; radians (ADR-0004) | no model importer; reads Shardfall clip sets (`tools/anim/shardfall.ts`) |
| my-3d2dge | everything drawn in code; `E.sprite` rows; 38 billboard props closed to games; a store plan in `docs/HANDOFF-ASSETS.md` | plan not started; 3 benchmark games scored 6/10 on look (flat pines, unshaded foliage, thin shadows, flat walls); excludes Quaternius models (QAL drift) but keeps its CC0 clips |
| riftlight | 151 ModelDefs, 143 TextureDefs (survey, not re-counted); 16-colour palette; GLB importer; `SpriteBatch` | `PACK_SOURCES` empty until its owner approves each pack on #49 |
| spawnforge | 81 modules, 32 blueprints; `spawnforge/0.2` frozen; 40/40 prompts valid first try, 20/20 + 20/20 matched blind | looks "like a toy"; private, UNLICENSED |
| ai-hub | test games as single HTML files; 1 evaluated (Capital Strike, `docs/FEEDBACK.md`) | not in this checkout, *unverified* |

Already shared: format-1 clips (my-3d2dge, my-3dge, Shardfall) and `$library` catalogs.

## 4. Asset kinds and their text formats

| Kind | Payload (text) | Today | Enters |
|---|---|---|---|
| prop | format-2 named primitive parts, cm | 28 core, 61 imports | **proof of concept** |
| material | ≤ 3 procedural pattern layers | 15 core | **proof of concept** |
| clip | key poses, % of hip height (format 1) | 437 embedded, 5,550 on disk | **proof of concept** |
| feel | a `VehicleDef` line plus measured facts (movement models are engine code, not entries) | 4 vehicles | a game builds a vehicle or the user asks; needs a vehicle meter |
| fx, look, level kit | preset plus overrides; a `[view]` table; theme plus glyph legend | 13 presets; styles and filters; 8 themes | a game logs a miss |
| character | `PuppetDef` look plus a clip shelf; imported bodies as per-bone parts | Kestrel, Ysolde | S3b, after critical mass (§14 Q8) |
| creature | SpawnForge blueprint, pinned generator, seed | 8 templates | creatures spawn as actors and §14 Q7 is answered |
| sound | a `Patch` table | none | a data-driven sound bank exists (FEEDBACK) |
| 2D vector | SVG path subset (no filters, text, images, script) | none | a game's HUD or UI asks (paths are text, §14 Q6) |
| sprite | palette-indexed rows plus a legend | none | only through the trial (§11) |

**The rule for imports, stated once.** An import enters a pack on disk only if it passes the
licence gate and the fit thresholds and is at most 30% mesh (mesh parts can be placed, scaled and
recoloured, never edited, so they teach nothing). It becomes `checked` with zero findings, a
description, tags and its token cap. The 61 committed imports predate the rule. The 15 that are
editable and lint-clean (13 Kenney; Quaternius bell and bench_1) get descriptions and become
`checked` in S0. The other 46 stay on disk as they are: findable, mesh share and findings in the
row, never on the shelf. (`check` runs of 2026-10-14; S0's Kenney rescale may change them.)

### Characters: is it a size thing?

- **Motion: no.** Clips store hips as a percent of hip height and limbs as direction plus bend.
  Kestrel and Ysolde share one biped body plan with different look numbers (scale, torso, limb
  lengths), colours and gear; every clip plays on both. Limits: equal-half limbs, one-segment
  spine, no wrists, fingers or contacts, bipeds only.
- **Bodies and props: yes.** Packs arrive at any scale (Kenney Food Kit is scaled 0.3, Quaternius
  Medieval Village 3). Kenney Mini Dungeon at scale 1 has a 48 cm chair (core: 96 cm) and a table
  32 cm tall (core: 78 cm), and nothing flags either.
- **The standard and the practice.** No single standard: glTF's frame, bone names by family
  (Mixamo, Unreal, Rigify) and VRM 1.0 (55 bones, 15 required; my-3danimations uses it). Engines
  retarget, never stretch (Unity's Avatar, Unreal's IK Retargeter, Godot's `motion_scale`):
  rotations transfer, hip movement scales by hip height, IK pins hands and feet, meshes scale
  uniformly to a height class.
- **Imported rigged characters** are not in the proof of concept; they are the first new kind
  after it (S3b, §14 Q8). Adjust the motion to the body, never the body to the motion: map the
  rig's bones with my-3danimations' tables, read the look's proportions from its bones, and every
  clip plays. Cheapest drawing first: rigid per-bone segments of one CC0 low-poly pack (blocky
  bodies hide the joints); then skinning, reusing the creature path's CPU skinning, if the rigid
  joints read badly on the review page.

## 5. The standard

### Rows and entries are generated, never edited

The sources of truth stay put: payloads (format-2 props already carry description, category,
tags, sockets, source and fit), `PACK.json` (licence, credit), `review.tsv` (reviews bound to
revisions), the index `note` column, clip sets and catalogs. From them the store generates columns
`kind licence method tier` in each `index.tsv`, one `store/STORE.tsv`, and JSON entries for a
second engine. Kinds with nowhere to keep metadata (feel, fx, look) get an entry file holding a
pointer and metadata, never a copy of the payload.

| Field | Meaning |
|---|---|
| `id` | `<kind>/<pack>/<name>`; `<pack>` is the source pack, or `core` for own work; never reused |
| `rev` | the payload's existing revision (`prop2-…`, `mat1-…`; a clip set file's hash) |
| `tier` | `ledger`, `disk`, `checked` or `standard` (§7) |
| `licence`, `credit` | SPDX id (`CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0`) or `LicenseRef-CMU`; the credit line |
| `method` | `authored`, `generated`, `imported` or `edited` |
| `origin` | where the lineage starts: `import`, `agent`, `human`, `generator`; inherited through `parents` (`id@rev`). Phase-out counts it. |
| `size_cm`, `sockets`, `slots` | real-world `w×h×d` in whole cm (plan D1); socket names; colour slots with hex |
| `tok`, `evidence` | payload tokens, `ceil(bytes / 2.8)` as `index.rs` counts; findings, reviews, flags (in rows: `c0 r1 f0`) |
| `desc`, `why` | what it is (≤ 120 characters; rows cut to 48); the technique it shows (≤ 40 tokens, `note` column) |
| `faces`, `source` | per engine, the path of its native payload; imports: URL, archive and licence-file SHA-256 |

A sample row (the core chair, tab-separated, 126 bytes, about 45 tokens): `LIB/core/chair prop
standard 44x96x45 CC0-1.0 authored 375 sit chair seat wood tavern Wooden tavern chair, …`.

**Conventions.**

| Topic | Rule |
|---|---|
| Frame | metres, +Y up, front +Z, right-handed (glTF, my-3dge, riftlight agree). Degrees in rows and room data (riftlight, Shardfall); glTF and my-3dge use radians, so their adapters convert. Payloads declare their unit (Shardfall props: cm). Pivot at bottom centre, on the floor. |
| Size | each pack is normalised once, in its catalog `scale`, so sizes are real-world; a game at another scale scales uniformly against its own metrics. Never per game per pack. |
| Sockets | Shardfall's closed set: sit, grip, grip2, tip, top, hang, wall_mount; +y is the working direction; importers map `SOCKET_*` empties onto it |
| Colour | named `slots` of sRGB hex, bound per engine: Shardfall `$roles`, riftlight's nearest palette entry, my-3d2dge's tone ramps |
| Skeleton, ids | VRM 1.0 names, with my-3danimations' mapping tables (vrm, unity, mixamo, unreal5, unreal4, rigify, cmu, bvh100style). A replaced entry keeps an alias; a withdrawn one is yanked. |

**Adapters.** Each engine reads entries through its own adapter and keeps its richest format;
the store checks outcomes, not parameters. **Clips come first:** the format is shared, my-3dge
already reads Shardfall's sets, and fidelity is fit in mm. Props and feel across engines need
their own plan: my-3dge's kit has 7 shapes (box, frustum, limb, pillar, ramp, ring, roundedBox)
against format 2's lathes, tubes, mesh parts and per-pixel materials; it bakes textures and may
not capture headless (*unverified*); vehicle numbers are Shardfall physics, meaningless to Rapier.

**The clip reader.** `crates/pav_core/src/clips.rs` has no format check (`format` is a `u32`, so
`"format": "2.0"` fails only by accident) and no `deny_unknown_fields`: 2.0 clips flattened by a
shim would silently lose `handL/R`, `fingersL/R`, `spine`, `yaw`, `requires`. S0 refuses them.

## 6. Intake

| Door | What comes in | Weight over time |
|---|---|---|
| Importers | key CC0/CC-BY packs, pinned by URL, archive and licence-file SHA-256; raw files in `.cache/`; one ledger line per candidate (`pick` or `skip: why`) | bootstrap only; frozen per kind at phase-out |
| Generators | material recipes, procedural moves; SpawnForge themes and seeds later | grows |
| Studios (agent-made) | `asset_edit`, `anim_edit`, materials; `publish` into LIB/core | the main source |
| Harvest from test games | files a game wrote under the store's payload paths, lifted from its patch series with `from_game`; anything else becomes a nomination line. Built when 3 games list reusable files in store formats (Capital Strike drew its units in Rust code, which no tool can lift). | the main curation source |

**The gate**, for every door: an allowed licence; zero `check` findings; a description of 1-120
characters; 2-6 tags; the category's token cap (400, 600, 800 or 2,000); complete provenance.
Imports and generated entries also need the fit thresholds and a byte-identical rebuild. Text
fields get a length cap and a short deny-list (`ignore`, `system`, `you must`) until outside
contributors exist. Errors use the Issue format (`crates/pav_core/src/issue.rs`).

### Licence gate

**Admitted:** CC0-1.0; CC-BY-3.0 and 4.0 with the credit carried; our own work (§14 Q1); CMU's
terms for clips (§14 Q4). **Excluded entirely:** Fab Standard, Unity Asset Store, Mixamo, NC, ND,
SA, GPL (copyleft would bind every game), unknown and UNLICENSED; each gets one `$skip` line.

Shardfall's gate (`crates/pav_tools/src/import/licence.rs`) is default-deny for `lib/`, but its
`LOCAL_ONLY` list still routes NC, ND, SA, QAL, Mixamo and Fab to git-ignored `local/<pack>`,
`mocap::local_only()` (`mocap/mod.rs:264`) sends Bandai Namco and LaFAN1 to `anim/local`, and
AGENTS.md (l.33-34, l.191-193) tells agents to do so. Those lanes stay outside the store, which
never indexes them (§14 Q2). And admitted is not embedded: plan
D5 keeps CC-BY out of the embedded build, yet STYLE100's 98 CC BY clips are embedded: §14 Q3.

### Fab's free licence and the 10,000 collected assets

Fab has two licences: CC-BY and Standard; Epic's free drops and the 2024 free Megascans are
Standard. Per a third-party copy of the EULA dated 1 October 2024 (*current text unverified*; not
legal advice), Standard allows private use, reproduction and modification (§3(a)) and shipping
inside a project in object code end users cannot extract (§4(c)); it bars standalone distribution
beyond one project's collaborators (§5(a)) and translating the content or deriving data from it
(§6(b)(i)). So Standard items cannot enter in any form. Fab's CC-BY listings could, each confirmed
at its source; nobody has counted them. Recommendation: leave the haul out (§14 Q9); a logged
miss may justify one pinned import from an original CC-BY source instead.

### Fitting tools

- **Per-pack scale.** Kenney Mini Dungeon's catalog `scale` goes to about 2.2 (table 70 cm, chair
  106 cm). No single scale fits a stylised pack: the same factor makes its barrel 106 cm tall and
  114 cm wide. `size_band` (planned in the plan at l.1001, never built) flags such outliers and a
  fork fixes them. Grounding is already checked (`below_floor`, `not_grounded`).
- **Per-game metrics** (hero, seat, table, door heights) and a fit check that seats and arms a
  reference character come on trigger: a game whose hero is not about 1.8 m.

## 7. Quality

Four signals, stored apart and never averaged: **correct** (licence, lint, budgets, fit,
determinism), **recognisable** (a contact sheet), **good** (the user's flags), **used** (games
that placed it; searches that ended in a placement or fork). They disagree: SpawnForge matched 20
of 20 prompts blind while its bipeds read as "a sack or sleeping bag with arms".

| Tier | Where it lives | Needs | Decided by |
|---|---|---|---|
| ledger | catalog TSV line | allowed licence, pinned source, survey metrics | automatic; not searched by default |
| disk | `LIB/<pack>`, native builds | translates; fit thresholds; ≤ 30% mesh; no missing or undecoded texture | automatic |
| checked | a column, on disk or core | zero findings, description, 2-6 tags, token cap; clips add no fail in the retarget sweep | automatic; per promoted batch an agent re-checks min(5, n) with contact sheets and sends only what it flags to the user |
| standard | `LIB/core` and the embedded clip sets, in every build | checked, plus the publish loop's review line bound to `rev`; CC0 and own work only (D5) | the publish loop; the user for edge cases |

`disk` is not called `listed`, which already names a catalog flag (`docs/ASSET_IMPORT.md` l.38).
Routine assets need no human review (the user's ruling, AGENTS.md "Testing budget"). The 28 core
props and the embedded CC0 clips start at standard; STYLE100 and CMU wait on §14 Q3 and Q4.

**Checks to add first (S2):** `size_band`; sockets per category (a sword needs `grip`); a missing
texture as a hard import failure (it renders white, and ΔE misses it); five planted defects, all
caught: a 32 cm table, a 48 cm chair, a prop sunk 76 cm, a sword with no grip, a missing texture.

**Flags.** Only the user flags, on a per-game review page; one flag nominates an entry for the
publish loop and is the user's review. `looks`, `feels`: good; `reads` (recognisable at game
scale); `works` (did its job in play): used; `broken`: back to review. Agents nominate, never flag.

**Demotion.** Today `core_assets_are_reviewed` (`crates/pav_tools/tests/asset_library.rs:102`)
fails the build when a core asset's revision lacks a review line, so an edit forces a re-review.
A licence problem yanks an entry. Other demotions wait until a game finds an entry broken.

## 8. Discovery for agents

**What works now.** `assets find=` reads index rows (embedded, then each pack's `index.tsv`) and
replies `name cat size_cm parts tok tags desc` (tags cut to 4, sockets as `@sit`, descriptions
cut short). `kind=` takes prop, creature or material and lists valid values on an error; `tier=`,
`category=`, `tags=`, `similar=` exist; clips have `clips find=`. Every word must match and nothing
is stemmed: `wooden table`, `tables`, `tree` and `character` return 0 rows.

**Fixes (S0):** real descriptions; a synonym table; plural stripping; an OR fallback; `kind=clip`
handing the query to the clips ranker (a facet, not a merged ranker); never a bare 0 but `none`,
the 3 nearest rows and a route ("make it: copy LIB/core/table"); tier and mesh share in each row,
so choosing needs no second call. Each miss appends a line to `store/misses.tsv`.

**The shelf.** `assets shelf=prop` (and `=clip`) returns one kind's best entries, at most about
1,500 tokens each (a token test). It stays out of the fixed prompt; AGENTS.md and the MCP
instructions name it in one line. Without it discovery fails: an outside agent built a climbing
game without a store's climb and hang clips, which sat outside the file it was handed.

**The eval.** 10 queries for present assets (a fresh agent writes them from contact sheets; ids
labelled first) must reach find@5 ≥ 8/10; 5 for absent assets must get `none` plus a route.
my-3danimations went from 4/10 to 8/10 on a fresh held-out round (7/10 on CC0 alone) after
rebuilding vocabulary, idioms and scoring. Here vocabulary work stops after one rebuild.

**The known future problem: scale.** At about 1,400 rows (159 KB for my-3danimations' clips) an
agent can no longer read a catalog whole; 10,000 rows is roughly 350k tokens. Responses, each on a
trigger: shard by kind; a taxonomy of at most 3 levels (Poly Haven's pattern); default to checked
or better; dedupe by fingerprint; a semantic lane only when held-out recall falls below 8/10.

## 9. Use

- **By reference.** Game data and rooms may name only embedded entries (test
  `game_asset_refs_are_embedded`; embedded budget 3 MB), which also keeps each build to what it
  uses; a per-game lock file waits until a build needs one. In the asset benchmark each author
  call re-read a fixed prompt of about 55k tokens, so cost tracks calls, not bytes.
- **Fork and learn.** `asset_edit action=copy` exists; from S2 it records `parents`, `method:
  edited`, the inherited `origin`, licence and credit. Shelf entries carry `why`: exemplars.
- **Credits.** STYLE100 (CC BY 4.0) is embedded in every build but credited only in its set file
  and the Animation Studio's "Source credits" panel (`crates/pav_app/src/animation_ui.rs:140`). S0
  generates a credits text from clip set `sources` and every `PACK.json`, with a unit test.

| Command | Tool | Status |
|---|---|---|
| `assets find="wooden table" kind=prop` | assets | exists; fixes in S0 |
| `assets find=walk kind=clip`; `assets shelf=prop` | assets | S0 (today `clips find=walk`) |
| `asset_edit action=inspect`, `check`, `copy`, `publish`; `asset_spawn` | asset_edit, asset_spawn | exist; `copy` records lineage from S2 |
| `asset_spawn find=…`, lineage report, lock-file check | none | triggers (§13) |

## 10. The library in the demo world

Wings are hard-coded in `crates/pav_core/src/world.rs`, so an eighth wing is engine work for no
gain. Three rooms go into an existing wing (Workshop or Styles & Filters); existing rooms that show
library content (`motion_library`, `mix_match`, `styles`, `filters`, `drift`, `feel_lab`,
`particles`) get a sign naming their entry ids. Rooms may name only embedded assets, so a third
room shows the rest without naming any. A test asserts every id resolves and every limit has a label.

- **Catalogue** (S2, native builds). The room names no assets: a pad pages through every entry in
  the store's rows, 20 at a time, beside the 1.8 m figure, each signed with its row (tier, mesh
  share, findings). In the web build the pad shows a sign that the catalogue needs the desktop build.
  This is how "every model we have" is on show without breaking the embedded-only rule.

- **Hall.** Standard props on plinths beside the 1.8 m figure, each signed with its row, the list
  generated from the index. Kestrel sits on a chair through `sit`, Ysolde holds a staff through
  `grip`, and one clip plays on both bodies.
- **Limits room.** Each exhibit is signed with its number and the check that catches it, or "not
  caught": a core chair scaled to 48 cm (`size_band` from S2; not caught today); a core prop sunk
  76 cm (`below_floor`); a prop with a missing material reference (`unknown_material`, magenta);
  STYLE100 `WildLegs_Walk` (546 mm worst-point error); a two-hand clip whose hands miss on a short
  body. A sign gives the limits only imports show: 29 of 61 imports over 30% mesh; textures
  flattened to face colours (not caught: the compare image uses the same flattened source). Those
  live in the native studio (`asset_preview`); textured originals appear only on a review page
  rendered into `.cache/`, never committed. Creature exhibits wait until creatures are actors;
  their sign will list what the native path does not draw (`docs/CREATURE_STUDIO.md`).

## 11. 2D

**Today 2D is drawn by code:** `Look::Cutout` (puppets on a camera-facing card), the `pixel_art`
filter (1-16 px blocks, `crates/pav_app/src/look_ui.rs:461`), my-3d2dge's pixel-art puppets. Code
and SVG drawing improve with coding ability (136 of 139 blind SVG samples structurally perfect on
one public benchmark). Freehand pixel grids do not: a sprite row tokenizes into pieces of 1 to 4
pixels, so columns never line up (survey, *not re-checked*). riftlight redrew sprites that misread
("a quiver that read as a carrot"); my-3dge dropped sprite cards for a pixel filter. **Most of the
"odd look" comes from mixing**: pixel densities side by side, non-integer scale, bilinear
filtering, clashing palettes. And the plan says bitmap textures "will not be reached" (P4), so
whether sprite rows count as text is §14 Q6.

**The trial** is pencilled in as S5, after the proof of concept (S2), because the user asked to
see whether sprites can work without looking odd. It must prove it is worth the slippery slope:
- **Engine: riftlight** (`SpriteBatch`, pixel-row TextureDefs); Shardfall has no sprite drawer and
  gets none for a trial. **Pack:** Kenney 1-Bit Pack (CC0, 1,078 files, 16×16 tiles, per
  kenney.nl, *not re-checked*), once riftlight's owner approves it on its #49.
- **Format:** palette-indexed rows, a legend pointing at tone slots; per game one pixels-per-metre
  value, one palette, nearest filtering, integer scale; pivot at the feet. A synthetic 32×32
  sprite measured about 424-451 tokens this way, against 3,126 as integer arrays and 8,533 as SVG
  rectangles (o200k; real dithered art costs more; base64 PNG was 305 but cannot be edited).
- **Arms, on the same 3 briefs:** imported sprites; code-drawn art; 3D props through `pixel_art`
  or `Look::Cutout` (Shardfall's arm, no new renderer path). Agents edit sprites only through
  operations (recolour, mirror, outline, shift, paste, remap palette, integer scale), each checked
  by a render diff. **Cost:** about 19 runs × 400k ≈ 7-8M tokens plus a review page.

**Success (all four):** sprites win at least 2 of 3 briefs against each other arm, in both orders,
on the user's review page; they need no more model calls than the cheaper other arm; the import
check catches 5 of 5 planted mixing faults; agent edits pass the render diff in at least 8 of 10
tasks. **Kill:** any one fails. Sprites stay out; the trial reruns only when the benchmark agent
changes. Authored 2D stays code and vector, the line that rises as models improve.

## 12. Where it lives

**Inside Shardfall,** in a new top-level `store/` folder: generated `STORE.tsv`, `README.md`
(≤ 1,500 tokens), `misses.tsv`, later entry files for feel, fx and look. Payloads stay where they
are; clips stay in Shardfall's format-1 sets (`anim/*.json`). Nothing for the user to create.

**A separate repository is a trigger:** a second engine needs props or materials. Then the user
creates a private GitHub repository (suggested `agent-assets`) with access for the agents'
sessions. It would take LIB/core (36 KB), the lib packs (288 KB), the embedded clips (2.5 MB),
`anim/100style` (2.8 MB) and, if §14 Q4 admits CMU, `anim/cmu` (68 MB). Engines pin a commit with
a SHA-256 per file, as Shardfall pins SpawnForge (`tools/creature-compiler/pin.json`); an engine
that changes re-checks its own faces in its own tests. my-3danimations' clips come in by hash
only once it passes G4 run 2 and publishes, and Shardfall reads format 2.0. ai-hub stays the
archive of test games; SpawnForge becomes the creature generator after §14 Q7.

## 13. Roadmap

Smallest first; each stage pays off alone and has an exit test. Every test bigger than a few runs
states its token cost before it runs and reports the actual cost after.

| Stage | Work | Exit test |
|---|---|---|
| S0 Smallest step (days) | Kenney Mini Dungeon rescaled (about 2.2) and re-checked. Descriptions and synonym-rich tags for the 15 editable, lint-clean imports. Find fixes and the `tier` column (§8). `assets shelf=` for props and clips. Credits text with a unit test. Clip reader refuses unknown formats and fields. Licence fields for own work (§14 Q1). A row test refuses any entry whose licence is not admitted, so the local tiers stay outside the store (§14 Q2). One line in AGENTS.md. | The eval (§8): find@5 ≥ 8/10, absent queries 5/5 `none` plus a route; `wooden table`, `tables`, `tree`, `character` each give a hit or `none` with a route; every CC-BY entry in the build is in the credits text; a format-2.0 clip file is refused with an Issue; shelf token test passes. |
| S1 Does it pay? | Arm A: `assets find=` and `clips find=` return only BUILTIN templates, no shelf. Arm L: S0's find and shelf. 6 briefs that existing tools pass or fail: a tavern corner (table, 2 chairs, barrel, candles; sizes ±15%); a market stall forked from a library prop, with an awning material; a corridor in 3 materials; a weapon rack of 3 weapons; a heavy walk and a taunt from library clips; a campfire with seats. Scored from transcripts: model calls, price-weighted tokens (as FEEDBACK counts them, shelf included), `check` findings, every placed id resolves. 1 run per arm and brief: 12 runs, about 4.8M tokens at about 400k a run. Room-scale briefs may cost more (*unverified*), so the largest runs first and the estimate is restated. | Arm L lower in calls and in price-weighted tokens on at least 5 of 6 briefs, with no more findings (the approved lean rule, plan §10.3; one fix round). If not: fix discovery once and rerun; if it fails again, the store stays S0's find and shelf and nothing later is built. |
| S2 Proof of concept, end to end | Columns `kind licence method tier`, `store/STORE.tsv`, `store/README.md` with a token test. `size_band`, socket lints, texture hard fail, five planted defects. `copy` records lineage. Kenney Mini Dungeon through every step: ledger, gate, rows, find, the Hall, a fork published to standard. CC0 clip sets and 15 core materials as rows. Hall, Limits and Catalogue rooms. | Planted defects 5/5 caught; the rooms pass `room_check` and the ids test; a prop forked from an import reaches standard through the publish loop; the user flips through one review page of the rooms. |
| S3 Critical mass | The test-game prompt (`docs/FEEDBACK.md`) gains three lines: "assets you looked for: query, found id or none"; "up to 5 things you built that another game could reuse: file, one line"; a pointer to the shelf. A per-game review page for the user's flags. 10 fresh briefs with need lists, by an agent that never saw the catalog (about 4M tokens). | **Critical mass:** a need is one line in a brief's need list or a game's answer; it is met when the agent placed or forked an entry from the top 5 instead of authoring from scratch (read from transcripts and `store/misses.tsv`). Reached when met/needs ≥ 0.8 for each proof-of-concept kind over the 10 briefs plus any games that arrived. Below that: studio-made entries for the most-logged misses first, one import only for misses a studio could not fill, re-measure; at most 3 rounds, then record the shortfall. Seeding stops either way. |
| S3b Characters | One CC0 low-poly rigged pack (KayKit or Kenney): bones mapped with my-3danimations' tables, look proportions read from the bones, rigid per-bone parts. Skinning through the creature path's CPU skinning only if rigid joints read badly. | Every embedded clip plays on 3 imported characters with no `check` findings; they stand in the Hall; the user flips one review page of them performing, rigid and (if built) skinned side by side. |
| S4 Second engine, clips only | my-3dge reads store clip rows through its existing Shardfall reader. | 10 clip entries load in both engines; each clip's fit in mm matches within a tolerance fixed when the stage starts. |
| S5 Sprite trial | §11, in riftlight, after S2 (pencilled in at the user's request). | §11's criteria. |
| S6 Phase-out, per kind | At each change of the benchmark agent, for each kind still importing: `scripts/asset_bench` runs 10 authoring briefs with that kind's imports hidden (its pinned author runner and token counts; no judge step; about 4M tokens). The user compares agent-made with imported on a review page: 10 pairs, each in both orders, verdicts left, right or same (about 10 minutes). An ablation reruns 5 fresh briefs with the imports hidden from find (about 2M tokens). | A pair counts for agent-made only if both verdicts call it better or the same; inconsistent pairs count against it. Pass: ≥ 5 of 10 pairs; every agent-made entry passes `check` within its token cap; ablation briefs pass their checks 5 of 5, with calls higher on at most 1. Two passes in a row freeze the kind's importer catalogs; a third retires imports (§1). `STORE.tsv`'s header reports the imported share per kind, by `origin`. |

**Triggers, not stages**, each with its own small plan when it fires: the harvester and the kinds
not in the proof of concept (§4, §6); several voters (a second voter); lineage reports (3 forks
change one field); a lock file (a build needs one); the semantic lane (held-out recall below
8/10); props and feel across engines (my-3dge gains a model importer and headless capture); clip
federation and a separate repository (§12); a JSON Schema (a TypeScript engine reads entries;
until then the validator is a serde struct with `deny_unknown_fields` returning Issues).

## 14. Risks, open questions and not doing

**Risks.** The core claim is unmeasured, and S1's one run per brief can pass 5 of 6 by chance
about 1 time in 9. Test games are rare (FEEDBACK's tally: 1 game, which used no props, clips or
studios), so S3 measures on fresh briefs. Imports translate poorly (31% editable), so the seed may
be thin. A bad licence cannot be recalled once a game ships; Quaternius' licence drift already
splits the engines (Shardfall imports its pinned CC0 December 2020 props; my-3d2dge excludes its
models). Descriptions are a prompt-injection channel, and agents copy examples whose names match
the brief, which inflates name-based evals. Process weight: my-3danimations runs 56 work packages
and 11 gates; this store should stay rows, lints, a shelf and a review page until a trigger fires.
**A deviation, for review:** "phased out entirely" keeps a yank line and the credit lines forks
inherit, because CC-BY attribution travels with every fork. Shipped games are unaffected.

**Open questions for the user** (not for agents). Each is asked in chat and logged in
`docs/ESCALATIONS.md`; after 15 minutes without an answer an agent applies the default and logs it.
1. **Own work's licence.** LIB/core is already CC0-1.0. Extend CC0 to the 13 own materials,
   VehicleDefs, particle presets, themes and agent-made assets (there is no LICENSE file)?
   Default: yes, CC0. Needed before S0's rows.
2. **Local licence lanes.** Does "excluded entirely" also close the lanes that translate
   non-redistributable libraries into git-ignored tiers (`LOCAL_ONLY`, `local_only()`, `clips
   load=local`, AGENTS.md l.33-34 and l.191-193, D5's last sentence)? Default: no. They stay as
   they are, outside the store: the store never indexes, finds, shelves or exports them, and a test
   checks that no row has a non-admitted licence. Closing them is a separate call for the user.
3. **D5 and CC-BY in the embedded build.** Default: keep D5: standard holds CC0 and own work, and
   CC-BY tops out at `checked`, on disk. STYLE100's 98 embedded clips (about 30 references in
   rooms and game data) and CMU's 60 are existing exceptions, logged, kept until the user rules,
   since moving them breaks rooms. The credits text ships either way; if the user also lifts the
   plan's "no in-game credits screen", an egui screen only displays that text.
4. **CMU mocap.** Its terms allow redistribution and modification but not resale of the data
   "even in converted form". Default: admit it for clips, that term on every row, taken from a
   reviewed licence row (Shardfall's `anim/cmu.json` omits the no-resale clause).
5. **Quaternius models.** Default: admit the pinned CC0 December 2020 release, checked against its
   licence-file hash, rather than excluding its models everywhere. Its clips stay CC0.
6. **Sprites and SVG under P4 and D3.** Do sprite rows and third-party SVG icons count as text
   data? Default: SVG paths do (vector drawing, the same family as code-drawn 2D, which the user
   allows) and enter when a game asks. Palette-indexed sprite rows count as text for the S5 trial
   only and stay out of the store unless the trial passes. The plan's "bitmap textures will not be
   reached" is read as covering image files and textures on 3D models, not this trial.
7. **SpawnForge** is private and UNLICENSED. Default: no creatures until the user licenses
   blueprints and compiled outputs.
8. **Rigged characters.** The plan rules out skinned import, but the user wants imported
   characters that play our clips. Default: not in the proof of concept; S3b after critical mass,
   rigid segments first, skinning only if the review page shows the joints read badly.
9. **The 10,000 Fab assets.** Default: leave them out. First confirm the haul sits in no
   git-tracked or shared path; there, they would be binaries committed without approval (P4) and
   possibly outside Fab §5(a) (*unverified*). Also ask the collecting agent how it chose what to
   take: the answer shows what selection tools the store needs.

**Not doing:** any asset whose licence bars redistribution or modification, in any form; a
service, database, gallery or marketplace; binary payloads; a bulk import (Kenney's 60,000+, Poly
Haven, the Fab haul) before a game asks; embeddings before the eval asks; stars, rewards or agent
votes; USD, Lottie or Spine; a new egui panel; freehand agent sprites; skinned characters before
rigid segments have been tried.

**Rejected:** a separate repository or service from day one (no second engine consumes props);
hand-written card files beside payloads (two sources of truth); one neutral payload for every
engine (it pulls all of them to the weakest; a format alone did not beat code on tokens in
riftlight, 0.98 and 1.02); agent judges (noisy, side-biased, about 120k tokens each, ruled out by
the user on 2026-10-12); critical mass as a count (my-3d2dge's plan: 300+ shapes, 300+ sprites),
which says nothing about use; a fixed date to end imports (quality differs by kind).
