# CINDERWIRE — Design Document

An original cyberpunk-noir action RPG, and a spiritual successor to the SNES
Shadowrun formula. This document covers the IP, the design pillars, the vertical
slice (`rooms/cinderwire.toml`, "The Helix Ledger"), the full-game systems it
represents, and how the slice was verified.

## 1. Identity: what Cinderwire is

**Logline.** *The corps own the streets. The Veil owns the truth. You run the gap
between them.*

Cinder City, 2149. Forty years after the Ember (the return of wild magic), the
city runs on three currencies: nuyen, secrets, and favors. You are **Wren**, a
courier with a dead drop burned into her memory and a debt to **Mara**, the fixer
who owns Mara's Bar on Rain Street. When a Johnson offers 50,000 nuyen for the
**Helix Ledger** — a list naming every soul Helix Dynamics has bought — Wren takes
the run. The Ledger, of course, names Mara too.

Cinderwire is original IP: no Shadowrun names, places, metatypes, or mechanics are
reused. What it inherits from the SNES classic is the *shape* of the fantasy:

| SNES Shadowrun distinctive | Cinderwire answer |
|---|---|
| Top-down real-time urban runs | Twin-stick infiltration: WASD + cursor aim, stealth cones + gunplay |
| Keyword investigation (talk, collect keywords) | Physical keyword kiosks + dead drops; intel as pickups, not menus |
| Hire shadowrunners (samurai, decker, shaman) | Hire pads: SLAG (rapid fire) vs HEX (long range); crew buffs the run |
| Matrix decking (separate cyberspace grid) | The Veil: jack-in transforms the *same* space (green shift, ICE, datastores) |
| Karma skills, nuyen economy, shops | Score-as-nuyen, loadout choice, skill-gated routes (crouch lanes) |
| Johnson missions: infiltrate, steal, exfil | One complete run: brief → infiltrate → hack → vault → twist |
| Noir story with a personal sting | The Ledger names your own fixer |

## 2. Design pillars

1. **Shoot, sneak, or mix — never stuck.** Guards must be sneaked (cones,
   checkpoints, crouch lanes); turrets/ICE must be shot; the boss is optional if
   you can dodge it. There is no fail state, only lost time.
2. **The run is one continuous place.** No menu diving mid-run: briefing,
   hiring, hacking, and the vault are all *locations* you walk to. Systems are
   geography first, UI second.
3. **Information is loot.** Keywords, dead drops, and datastores pay out as
   physical chips + score. Intel you walk past is intel you lose.
4. **The Veil is a lens, not a level.** Jacking in re-lights the same corridors
   green rather than teleporting to a minigame. (Full game: the Veil gains
   exclusive routes and threats while your body stays vulnerable.)
5. **Every run is scoreable.** Timer + nuyen + accuracy (hits taken) make every
   level a speedrun/farming puzzle, like the genre reference rooms.

## 3. Modernizations over the 1994 formula

- **Checkpoints, not game over.** SPOTTED! returns you to the last checkpoint
  with the clock running. The SNES fail-state loop becomes a flow-state loop.
- **Readable stealth.** Cones painted on the floor (yellow→red), explicit crouch
  rules (60% sight range), alcoves and cover that visibly cut sight lines.
- **Hiring as loadout, not micromanagement.** The SNES party AI was a liability;
  here the crew is a pre-run choice with a clear mechanical identity.
- **Seamless decking.** The SNES Matrix was a separate game that stranded your
  body; the Veil is a mode shift in place, so stealth and hacking interleave.
- **One currency on screen.** Score *is* nuyen, earned in front of you from
  datastores, kills, and the boss — no post-mission accounting.

## 4. The vertical slice: "The Helix Ledger"

One room, one course (`ledger`), ~120 tiles / 3–5 minutes, five acts:

0. **Rain Street (safe hub).** Spawn on START; Mara briefs you from wall/floor
   text; hire SLAG *or* HEX from dead-end nooks (the choice is geometrically
   real); work the JOHNSON and LEDGER kiosks (chip payouts); Wisp the drone
   joins as your follower. Rain, neon marquee, lamps.
1. **The Fence.** A 140° patrol paces the corridor; five alcoves are blind;
   two back-row **dead drops** pay bonus intel off the racing line. CP 1.
2. **Server Hall.** A sweeping sentry + a pacing guard among pillars, plus a
   shootable sentry gun (25). CP 2.
3. **The Veil (jack in).** A forced 2×2 pad flips the full view state green
   (sky, bloom, scanlines, tint, grain). An ICE sentinel patrols; two ICE
   turrets (aimed + ring) guard two datastores (50 each); archive tables + a
   crouch-only duct cross under the watch. Forced jack-out restores the street
   look. CP 3.
4. **Vault Approach + the Helix Spider.** Two sweeping sentry eyes, a bonus
   datastore, then the three-phase Spider (fan → spiral → spiral storm, boss
   bar, 500). The gold seal is gate 1; **the Ledger (finish) stops the clock.**
   Twist on the vault wall: *THE LEDGER NAMES MARA.*

Bypassable by design: the Spider can be dodged, dead drops skipped, the run
shot loud or ghosted — score and time sort out the styles.

## 4b. Mission 2: "The Ash Exchange" (`rooms/ash_exchange.toml`)

The sequel run, on a fresh map with a different pace: tighter and denser than the
Ledger's sprawl (~58 tiles critical path, ~100+ with optionals). Wren sells a
copy of the Ledger through the broker Okonkwo; the deal is a setup; walk Pale
the informant out (Wisp returns as a second follower).

0. **The Exchange (venue hub).** START, three kiosks (BUYER / PRICE / EXIT),
   two rig loadouts (GHOST quiet steps vs RONIN rapid fire), deal tables,
   ember air. Pale + Wisp join.
1. **The Setup.** Patrol + two short-range sleeper guns + alcoves; a forced
   alarm pad flips the night red **one-way** for the back half. CP 1.
2. **The branch.** HIGH road west (tables + crouch duct vs a sentry) or LOW
   road east (the Pit: two guns, crates, pillar) entered through a **timed
   seal door** (kinematic slab, 6 s cycle, recesses into the wall). CP 2.
3. **Side doors (optional).** The Veil closet west (jack-in spur, ring ICE,
   sentry, two datastores; jack-out sits ON the main corridor so skipping is a
   no-op and leaving restores the look) and the Adjudicator's cache east
   (three-phase optional boss whose range *leans on the main corridor* through
   the spur, plus two datastores).
4. **Exfil.** Ordered gates (RUN → DON'T STOP) across the hall, a conveyor
   walkway boost, two sweeping eyes, a gate gun, CP 3, and THE GETAWAY.
   Twist on the north wall: *MARA BOUGHT HER OWN NAME.*

New mechanics vs mission 1: real branch, kinematic timing door, conveyor,
one-way alarm state, ordered gates, escort-flavor follower, optional boss with
main-path pressure. Same promises: checkpoints, no fail state, bypassables.

## 5. Full-game systems (what the slice represents)

The slice is data-only on purpose: it proves the *content pipeline*. The full
game extends it along lines the engine already supports:

- **Campaign of runs.** Each run = one room file (briefing hub → approach →
  twist → vault). Rooms hot-reload; a run a week is a sustainable content cadence.
- **Crew.** SLAG/HEX today are loadout pads; full game adds Shaman-equivalent
  (sight-through-walls pad buffs), per-runner quest chains, and cutscene barks
  via the `say` + label path the slice already uses.
- **The Veil deepens.** Exclusive Veil-only routes (pads that toggle ghost
  bridges), black ICE (emitters with `hazard`), trace timers (course gates with
  teeth). The slice's jack-in/out view swap is the mechanical seed.
- **Economy.** Score-as-nuyen persists via the ARPG hero/inventory path the
  engine ships (`pav_core::arpg`): buy cyberware (stat pads), weapons (blaster
  tuning), and Veil keys between runs. The slice's 665-nuyen table is the
  economy's unit test.
- **Karma/skills.** Crouch lanes today; full game gates routes behind
  skill-check pads (tech locks, spirit doors) fed by a karma point per run.
- **Factions/endings.** Mara vs Helix standing tracked per run; the Ledger
  twist is the template for run-ending stings that recontextualize the hub.

## 6. Tech approach

- **Engine-native, data-only.** The slice is a single `rooms/cinderwire.toml`
  (~450 lines): ASCII layout + legend, 10 NPCs, 24 objects, 20 labels, one
  course. No Rust, no new dependencies, no asset files — exactly the "adding a
  room" path the engine documents. It loads in the pavilion (`world` scene,
  genre wing) and standalone (`--room cinderwire`), and hot-reloads from disk.
- **Built on proven geometry.** Challenge sections reuse the sightline-tuned
  stealth map (guard hall, junction, camera hall, vault); the street hub,
  Matrix re-skin, turrets, datastores, boss, and all story dressing are new.
- **Why not the ARPG path?** The engine's hack-and-slash systems (items, tree,
  loot) are deep but fantasy-tuned; re-skinning them to cyberpunk would have
  produced a worse Shadowrun *and* a worse Shardfall. The room framework's
  stealth guards, vision cones, emitters, shootables, pads/signals, and course
  timers turned out to be the better chassis for this fantasy.

## 7. Verification

The build sandbox in this session could not execute the Rust toolchain (child
`exec` of compiled binaries is denied below the top shell; documented in the
final report), so runtime verification was replaced with the strongest static
verification available:

- **`/tmp/cw_check.py`** mirrors `RoomDef::validate` (legend coverage,
  entrance bounds, label positions, zone heights) and extends it: TOML schema
  for every NPC/object/label/behavior/fx table, strict `#rrggbb` colors,
  PuppetDef look keys, walkability + BFS connectivity from spawn to all 12
  objectives, guard patrol-point walkability, course integrity (START/FINISH/
  gates/checkpoints), signal send/listen matching, forced-path tiles, and the
  jack-in/out pad mirror. Result: **PASS, 0 errors, 0 warnings.**
- **Controls:** the reference `stealth.toml` passes every generic check (only
  the cinderwire-specific tile assertions fail, by design); a corrupted copy
  with 4 injected defects fails with exactly those 4 errors.
- **`/tmp/cw_previz.py`** renders the map (ASCII + `docs/cinderwire_map.png`)
  and objective distances: hires/kiosks 2–6 tiles, dead drops 15/31, CPs at
  38/67/85, the Ledger at 120.
- **First runtime gate (on any machine with cargo):**
  `cargo run -q -p pav_tools --bin pav -- room_check path=rooms/cinderwire.toml`
  then `pav capture scene=cinderwire ticks=60 width=1280 height=720
  out=out/cinderwire.png`. Expected: room check clean, night street render.
  Same for mission 2 (`room_check path=rooms/ash_exchange.toml`,
  `pav capture scene=ash_exchange ...`), which additionally passed
  `/tmp/ax_check.py` (adds course-coherence and jack-out-mirror checks):
  **PASS, 0 errors, 0 warnings**, with mission 1 still passing (regression).

## 8. Content manifest

- `rooms/cinderwire.toml` — mission 1, "The Helix Ledger".
- `rooms/ash_exchange.toml` — mission 2, "The Ash Exchange".
- `docs/cinderwire_map.png`, `docs/ash_exchange_map.png` — top-down previz.
- `docs/CINDERWIRE.md` — this file.
- `/tmp/cw_check.py` + `/tmp/ax_check.py`, `/tmp/cw_previz.py` +
  `/tmp/ax_previz.py` — session validation tools (kept out of the repo; the
  repo's own gate is `pav room_check`).
