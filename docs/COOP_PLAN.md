# Co-op: up to four players, one world

The plan for online co-op in Shardfall: up to four players, each free to go anywhere, playing
the same game as single player, with monsters made tougher instead of more numerous. Status:
**planned, not built** (2026-10-16). Facts come from a survey of `main` at `dd5584b`; line
numbers are as of then, and *unverified* marks what was not checked. The agent who builds it
reads this whole file, then works through §11's stages in order. Paths without a crate are under
`crates/pav_core/src/`. Players get their own page, `docs/COOP.md`, in C5.

## 1. What the user asked
| The ask | Where this plan meets it |
|---|---|
| **Up to four players**, online co-op, "essentially the same single player experience" | §2 (1-5), §5.1 (the cap), §5.5, §7 |
| **No extra monsters:** monster **life and damage scale** with the number of players | §6 |
| **Lag-free feel:** whatever it takes on every side (host, client, server) | §2 (2-4, 8-9), §5.7, §9, with measured targets in §10 |
| **No anti-cheat:** every client is trusted | §2 (4), §5.6: claims are never re-checked |
| **Free roaming:** anyone goes anywhere they can reach on their own, never tied to another player, never waiting ("a super mini-MMO") | §2 (1-2), §5.2: one shared instance per place, with travel gated only by the hero's own progress |
| **No voice or text chat.** Emotes with animations, pings and map pings are in | §8 |
| A short `/goal` to build it, the effort level, and whether to `/compact` or `/goal` first | §14 |
| Implied by `AGENTS.md`: the Windows `.exe`, Linux and the browser build play together | §9, §11 (C3) |

## 2. The design in one page
1. **One game, one place, as today.** Each player's game simulates only the place its hero is
   in (town, a level, the arena, the lab). One `Sim` holds one place, and travel rebuilds it.
2. **Place owners.** The first player into a place builds it as single player does and **owns**
   it: their game runs its monsters, bosses and shared mechanics. Players who arrive later are
   **guests** whose games mirror the owner's. One guest is the place's **standby**. When the
   owner leaves, the standby takes over from an exact snapshot, confirmed before the owner goes;
   if the owner drops, the standby promotes from a snapshot at most 5 s old. A player alone in a
   place is playing single player, with zero lag.
3. **Your hero is yours.** Every game fully simulates its own hero: movement, skills, life,
   mana, potions, items, death. Input never waits on the network.
4. **The attacker decides it hit; the victim decides it was hit.** Your hits resolve on your
   screen against what you see and go to the owner as claims. A monster's attack resolves on
   its target's screen against where that hero really is. Nothing is re-checked (§5.6).
5. **Personal rewards, shared world.** Loot, gold, XP, boons, chest payouts, vendors, the stash
   and waypoints are per player and never cross the network: on a kill, each player nearby
   rolls their own drops, as in single player. Monsters, bosses, seals and the level's
   mechanics are shared (§5.5).
6. **Scaling:** life ×(1 + 0.75(n−1)) and damage ×(1 + 0.25(n−1)), where n is the number of
   players in the place (ruling E2; §6).
7. **Joining a place means receiving the owner's exact state.** The owner sends its `SimState`
   snapshot, the CBOR that `Sim::save_state` already writes, minus its own hero. Measured
   deflated with gzip: 41 KB for town, 81 KB for level 3, 109 KB for level 9 and 175 KB for
   level 14. Machines therefore never have to compute identical results.
8. **One replication pattern.** Every character simulated on another machine is a **movement
   stream** (interpolated) plus **action events** (casts, dodges, emotes, deaths), played
   through the engine's own animation and skills.
9. **Transport:** WebRTC data channels through matchbox, on native and in the browser: a full
   mesh with an unreliable channel for streams and reliable channels for control, events and
   bulk transfers (§5.4). Tests and tools use an in-process simulated network. The session host
   keeps only the directory of who owns which place, and hands it on under a new term if it
   leaves (§5.1).
10. **Online rules:** no pause, rewind or speed control. Hit-stop freezes only your hero and
    what you hit, never the world.

**Rejected:**
- *One host simulating every place* (host-authoritative). Every guest would feel latency on
  every monster, even when alone, and the host's game would run up to five places at once.
- *Lockstep or rollback* (for example GGRS). Both need bit-identical simulation on Windows,
  Linux and wasm, which isn't true here: rapier's `enhanced-determinism` is off and the code
  uses `std` trig. Every player would also simulate every place.
- *A WebSocket relay.* On TCP, one lost packet stalls everything behind it, and every message
  takes two hops.

## 3. What the code assumes today
| Area | Today | Where | Co-op change |
|---|---|---|---|
| The player | `SimState.player`; one `InputFrame` per tick, given only to the player | `sim.rs` `spawn_player` l.412, `step_inner` l.521 (input l.553) | Stays the local hero. Remote heroes use a new remote-drive mode (C0). |
| Shardfall's hero | `Game.hero` (the profile) and `hero_id`; live life and mana are in `Actor` | `arpg/mod.rs` l.123-126, `resume_game` l.324 | `Game` splits into a hero side and a place side (C0). |
| Monster AI | `Game::pre` takes one hero: wake at 12 m, pack at 9 m, casts at the hero's feet; one flow field cached per hero cell | `arpg/mod.rs` `pre` l.629-729, `arpg/brain.rs` l.11-12 and `think` l.56, `arpg/mechanics.rs` `hero_flow` l.248 | Target the nearest living hero (sticky); one flow field per targeted hero. |
| Engine checks | one `player_feet` for every engine brain; `PLAYER_CRUMBLE` tiles react only to the player | `sim.rs` l.545, `ai.rs`, `destruct.rs` l.78 | Any hero (the Crumbling Halls need it). |
| Time | hit-stop slows the whole sim to 0.06 | `Game::time_scale`, `arpg/mod.rs` l.271; `sim.rs` l.524 | Per entity online (§7). |
| Pause, rewind, speed | the app's `TimeControl`; the Esc menu pauses; Backspace rewinds | `pav_app/src/simhost.rs` l.20-38; `app.rs` l.764, l.1000 | Off online. |
| Travel | rebuilds the whole `SimState` within one tick, carrying the `Game`; `resume_game` wipes actors, loot and the level; the layout seed comes from the sim's RNG, so a level is new on every visit | `arpg/mod.rs` `game_travel` l.404; `arpg/world.rs` l.513 | Kept. Co-op decides whether to build fresh (owner) or adopt the owner's snapshot (guest). |
| Damage | `Game::hit` for everything; `targets()` filters by `Team::hostile`, so there is no friendly fire | `arpg/mod.rs` l.1077; `arpg/skills.rs` `fire_cast` l.455, `targets` l.416 | One routing check at the top of `Game::hit` (§5.6). |
| Rewards | `kill()` gives XP to the one `g.hero`; `drop_loot` uses that hero's find stats | `arpg/mod.rs` l.1309; `arpg/loot.rs` l.50 | Each game runs its own reward on a `Killed` event. |
| Scaling hooks | `spawn_actor` sets life (l.598) and damage (l.611); `enemy_damage` is also applied in `roll_damage`, the boss's `death_burst` and powers | `arpg/mod.rs` l.578; `arpg/skills.rs` l.331; `arpg/boss.rs` l.350; `arpg/powers.rs` l.251, l.348 | Life scales on the owner, damage on the victim (§6). |
| Where you can go | `GameCmd::Travel` is gated by the hero's own `max_depth`; `use_exit` refuses while the level is sealed | `arpg/cmd.rs` l.296; `arpg/mechanics.rs` l.911 | Unchanged: "anywhere they can reach on their own". |
| HUD and map | the map takes the hero to be the first `Team::Hero` actor | `pav_app/src/arpg_ui.rs` `level_hud` l.562 (l.641) | Use `hero_id`; draw remote heroes and pings. |
| View | `RenderFrame.player` and `focus` are single; every entity carries its own puppet look | `sim.rs` `frame` l.941; `pav_view/src/build.rs` l.1104 | Unchanged (the local hero); four heroes already render. |
| Saves | one hero and roster per machine or browser | `pav_app/src/save.rs` | Unchanged: no save crosses the network. |
| Bots and tools | `autoplay` and `campaign` drive `g.hero` in one `Session` | `arpg/bot.rs`; `pav_tools/src/agent_tools.rs` l.227 | Per player, through the `coop` tool (§10). |
| App | no title screen (boots into `town`); the pause menu is the only menu | `pav_app/src/settings.rs` l.172; `ui.rs` `MenuAction` l.25, `pause_menu` l.167 | The co-op entries go in the pause menu. |
| Networking | none; no async runtime in use (tokio is only in `Cargo.lock`) | every `Cargo.toml` | A new crate, `pav_net` (§4). |
| Several sims | already work in one process (travel builds a second `Sim`); content caches are process-global | `arpg/mod.rs` l.409 | Enough for the in-process test network. |

## 4. Where the code goes
- **`pav_core::net`** (new; engine API):
  - `PeerId`, `NetId` and `Channel` (unreliable or reliable).
  - A tick's `NetInbox` and `NetOutbox`.
  - `InterpBuffer`: interpolates, extrapolates up to a limit, then holds.
  - A link clock: RTT, jitter and the remote-tick estimate.
  - `SimNet`: an in-process network whose links have latency, jitter, loss and bandwidth drawn
    from a seed.

  It is pure: no IO, no threads, safe on wasm. It is written for any game on the hook; Shardfall
  uses it first.
- **`pav_core::arpg::coop`** (new; the game): messages, the directory, owner and guest logic,
  the hero and place streams, damage routing, scaling, pings and emotes. Its state is
  `Game.net: Option<Box<CoopState>>`, inside `SimState`, so a player's own snapshots and
  replays carry it. It has two parts:
  - the **session part** (directory, peers, host term, link clocks) lasts the whole session;
  - the **place part** (role, place epoch, `NetId` map, interpolation buffers, pending events
    and claims, watermarks) belongs to one place.

  Place snapshots sent to other players carry neither part. Adopting a snapshot keeps the
  recipient's session part and builds a fresh place part from the snapshot; so does any travel
  (§5.2).
- **`pav_net`** (new crate; internals, doctrine 8): the WebRTC transport (matchbox, on native and
  wasm) and the signaling server with rooms (native only; §9), both embedded in the host's game
  and as a small `shardfall-signal` binary. It handles bytes only and knows nothing of the game,
  so `pav_core` stays free of IO.
- **`pav_app`**: flags and settings, the net pump, the Co-op window, party frames, ping and emote
  input and wheels, and pad and touch bindings.
- **`pav_view`**: name plates and ping markers, drawn in the 3D scene so captures show them.
- **`pav_tools`**: one `coop` tool. The MCP tool list is at its 48,000-character cap
  (`pav_tools/tests/asset_errors.rs` l.101).
- **Encoding:**
  - Messages use `postcard` 1.1.3: compact serde, a new dependency.
  - Snapshots use the existing CBOR (`Sim::save_state`, `ciborium`), deflated with
    `miniz_oxide` 0.8, already a workspace dependency.
  - A malformed message is dropped and logged, never a panic.

## 5. How it works
### 5.1 Session and directory
- A session is a matchbox full-mesh room at `<signal>/<code>`. The code is six characters of
  Crockford base32. Every player links to every other: at most six links. Rooms are kept apart
  by our signaling server (§9), and `Hello` carries the code as a second check.
- **Handshake:** a joining player sends `Hello{protocol, build, code, name, profile}`. The host
  answers `Welcome{slot, directory, settings}` or `Reject{reason}`:
  - The reason is a version mismatch (the `.exe` and the Vercel build can differ; bump
    `PROTOCOL` whenever a message changes), a wrong code, or a full session (a fifth player).
  - Failures show on screen in plain words: host not found, version mismatch, session full.
- **The host** is the room's creator. It keeps the **directory**:
  - peers: slot 1-4, colour, name, and a hero profile (character, level, look);
  - places: `PlaceKey` → owner, standby, members, epoch (the standby is the member who takes
    over if the owner drops; §5.2);
  - the session settings: the host's difficulty sliders and the `coop.*` params.

  It broadcasts every change, and every player keeps a copy.
- **If the host leaves or drops,** copies of the directory may differ (a broadcast can stop
  partway), so the new host rebuilds it rather than trusting its copy:
  - The earliest-joined connected player (ties by peer id) becomes host under a new **host
    term**: the pair (number, host's peer id).
    - Every control message carries the term, and messages from older terms are dropped.
    - If a split link ever leaves two hosts, the higher pair wins; the other host steps down
      and reports.
  - The new host asks every player for `Report{place, role, place epoch, name, profile,
    settings version}` and rebuilds the directory from the answers:
    - if two players claim to own one place, the higher place epoch wins and the other resyncs
      as a guest;
    - the settings with the highest version win.
  - It publishes the rebuilt directory before accepting any change.

  With the embedded signaling server (E1's default), nobody new can join after the host goes,
  but the session goes on.
- **`PlaceKey`** is one of `Town`, `Arena`, `Lab` or `Level(depth)`. There is one instance per
  key per session: whoever goes to depth 7 joins whoever is already there.

### 5.2 Entering, leaving and owning a place
**Entering.** Travel starts as today: the portal window, `T`, or the exit, through
`GameCmd::Travel`, gated by the hero's own `max_depth`. Then:
1. The game sends `Enter(place)` to the host and keeps its loading veil up.
2. **Empty place:** the host answers `Assign{owner: you, epoch}`. The game builds the place as
   single player does, and owns it.
3. **Occupied place:** the host answers `Assign{owner: O}`. The game asks O to `Join`.
   - O sends `PlaceSnapshot`: its `SimState` minus its hero side and its `Game.net`, deflated,
     in 16 KB chunks on the bulk channel (about 0.1-0.5 s).
   - The game **adopts** it: it keeps its own hero side and co-op state, turns O and the other
     members into remote heroes, and spawns its hero at the place's spawn. It is now a guest.
4. O adds the newcomer's hero, and every member gets `MemberJoined`.

**Leaving** is travelling away or disconnecting:
- **A guest leaves:** the owner removes its hero, and members get `MemberLeft`.
- **Every place with guests has a standby:** its earliest-joined guest, named in the directory;
  if the standby leaves, the next guest takes the role. The owner keeps the standby promotable:
  - it sends the standby the place stream without the interest limit (far actors at 2 Hz);
  - it sends a full snapshot every 5 s, or every 10 s once snapshots pass 100 KB;
  - this owner-to-standby link has its own budget of 40 KB/s, on top of §5.4's per-player
    budget.
- **The owner leaves, others remain:** the standby takes over in three steps, so no change is
  lost:
  1. The old owner takes a final snapshot and sends it as `Handoff`. From that tick it stops
     advancing the place: monsters hold, and claims and uses queue unapplied. So nothing
     happens after the snapshot that the snapshot lacks.
  2. The standby adopts it and answers `Ready`.
  3. The old owner tells the host, which broadcasts `OwnerChanged{epoch+1}` to re-point
     everyone's streams. The old owner forwards its queued claims and uses to the new owner,
     then travels behind its loading veil, and forwards any claim that still reaches it.

  Monsters pause for a fraction of a second at a handoff.

  If `Ready` hasn't come within 3 s, the old owner travels anyway and the host treats it as a
  drop.
- **The owner drops:** after `owner_timeout` (2 s) with no packets, the host makes the standby
  the owner. The standby **promotes**: it adopts its latest standby snapshot, applies its fresher
  stream data on top, and switches the mirrors to full AI. Brains restart, and up to 5 s of
  far-off changes can be lost: acceptable for a crash.
- **The last player leaves:** the place is gone. The next visitor builds a new one, as in single
  player.

**Epochs** drop stale messages from a former owner. If two players enter an empty place
together, the host answers in order and the second waits for the first's snapshot.

**Resync** heals any desync. A guest asks for a fresh snapshot and adopts it, keeping its hero,
when any of these happens:
- a `NetId` named by a stream or an event still has no `Spawned` 2 s after it first appeared.
  A stream, or a `Cast` on the unordered event channel, can beat its spawn; until then it is
  buffered, not treated as a desync;
- a `PlaceState` hash doesn't match;
- an epoch has a gap.

One adopt function serves joining, handoff, promotion and resync.

**IDs:** the owner assigns each actor's `NetId`, and each game maps `NetId`s to its own
`EntityId`s, because games allocate entities independently after they adopt a snapshot. Remote
heroes are `NetId`s by slot.

### 5.3 One tick
1. The app receives packets and stamps each with the next local tick, giving the `NetInbox`.
2. `Sim::step_net(&InputFrame, &NetInbox)` runs three steps:
   - the co-op pre-step applies the inbox: directory, snapshots, events and claims, and
     streams into the interpolation buffers;
   - the normal step;
   - the co-op post-step writes the `NetOutbox`: streams and events.
3. The app sends the outbox.

Replays record the inbox per tick in a new `net` track in `Replay`. So `record_save` and
`replay` still reproduce one player's game exactly on the machine that recorded it (doctrine 5;
`AGENTS.md`'s platform note). Single player calls `step_net` with an empty inbox and behaves
exactly as today.

### 5.4 Streams and events
Four channels, so a lost or large message never holds up an unrelated one:
- **0 streams:** unreliable and unordered, each message at most 1,200 bytes.
- **1 control:** reliable and ordered: the handshake, the directory, owner changes and the
  handoff's steps.
- **2 events:** reliable and unordered. Each event carries a per-sender sequence number, and
  order is restored only where it matters:
  - a `NetId`'s spawn, death and despawn are kept as tombstones, so a late spawn never revives
    a dead actor;
  - each player's claims apply in id order (§5.6).
- **3 bulk:** reliable and ordered: snapshots and `PlaceState`.

| Message | Ch. | When | From → to | Carries |
|---|---|---|---|---|
| Hero stream | 0 | 30 Hz | each player → others in its place | tick, position, velocity, facing, grounded and posture, life and mana fractions, ailment bits, current action and phase |
| Place stream | 0 | 20 Hz | owner → each guest | owner tick and, per player, the last claim applied (the claim watermark); then per actor in the guest's interest: `NetId`, position, velocity, yaw, life fraction, state bits (casting, stunned, frozen, burning, dead), target |
| Standby stream, standby snapshot | 0, 3 | far actors 2 Hz; snapshot every 5-10 s | owner → the standby | the place stream without the interest limit; a full snapshot (§5.2; on its own 40 KB/s budget) |
| `Cast`, `Dodge` | 2 | on event | whoever runs the character → its place | actor, skill, aim, target, start tick, seed |
| `Spawned`, `Despawned` | 2 | on event | owner → guests | `NetId`, and the new entity and its `Actor` as in the owner's state (CBOR), so summons, waves and brood arrive complete. Every reference inside them to another entity (`Actor.master`, `last_hit`, hit lists, owners, joints) travels as a `NetId` and is remapped on receipt; an unknown one becomes `None` |
| `Killed` | 2 | on event | owner → guests | `NetId`, owner tick, killer, position, level, rarity, rank, boss |
| `HitClaim` | 2 | per hit | guest → owner | claim id (rising, per claimant), target, and the attacker's side as its game resolved it: damage by type before the target's mitigation, crit, ailments with strength and duration, knockback, execute, power effects |
| `UseClaim` | 2 | per use | guest → owner | a world-changing use: a chest, a well, a keg, a Menagerie creature, an arena wave |
| `HeroHit`, `HeroDied`, `Respawned` | 2 | on event | a player → its place | others' screens: flash, number, fall |
| `PlaceState` | 3 | on change, ≤ 5 Hz | owner → guests | the shared mechanics state, deflated and sequence-numbered (latest wins) |
| `PartyStatus` | 2 | 1 Hz | a player → all | life, level, place, dead (for party frames) |
| `ProfileChanged` | 2 | on change | a player → all | character, level, look |
| `Emote`, `Ping` | 2 | on use | a player → its place | §8 |
| `Enter`, `Assign`, `Join`, `Handoff`, `Ready`, `OwnerChanged`, `Report` | 1 (`Handoff` on 3) | on change | the players concerned | the host term and place epoch, plus each step's data (§5.1, §5.2) |
| `Probe`, `Echo` | 0 | 1 Hz per link | both ways | the link clock |

The place stream covers actors in the guest's interest: within 45 m of its hero, every boss,
and anything targeting it. It sends the nearest first and goes round-robin past the size cap.

Budget: at most 40 KB/s per player with four players in one level, not counting the standby's
link, which has its own budget (§5.2). Positions start as f32; quantize them only if a
measurement goes over budget.

### 5.5 Who simulates what
| Thing | Simulated by | Other games |
|---|---|---|
| Each hero | its player's game, fully | remote-drive from the hero stream; casts, dodges and emotes from events; its damage ignored |
| Monsters, bosses, summons, chest keepers, arena waves, Menagerie creatures | the place's owner (AI, life, ailments, deaths) | mirrors: remote-drive from the place stream; casts from events |
| Projectiles and ground effects | every game that plays the cast | damage only as §5.6 routes it |
| Shared mechanics: kegs, rift gates, wells, totems, the crumbling floor, the seal and exit, chests and their waves | the owner | `PlaceState` and events (a keg's blast: each game applies it to its own hero) |
| Static hazards: lava, spike beats, wind, ice, darkness, time bubbles | every game, on its own hero (the owner also on monsters) | not sent |
| Personal: loot, gold, XP, potions, shrine boons, chest payouts, the passive point for a first boss clear, vendor and buyback, the stash, waypoints (`max_depth`), fog of war (`LevelState.seen`), the kill streak | each player's game only | never sent |
| Cosmetic: villagers, Biscuit, the Hall of Heroes performers, floaters, gibs, particles, shake | each game locally | never sent |

Shrines are personal: each player can use each shrine once. A guest's world-changing use plays
at once in its own game and goes to the owner as a `UseClaim`.

*Unverified:* how time bubbles and wind act on characters. In C2, check each mechanic in
`arpg/mechanics.rs` against this table, and record any changes in the stage's `docs/HISTORY.md`
entry.

### 5.6 Damage routing
One check at the top of `Game::hit` returns `Apply`, `Claim` or `Ignore`:

| Target ↓ \ attacker → | your hero | a monster or hazard | a remote hero |
|---|---|---|---|
| your hero | — (no friendly fire) | **Apply**: you decide you were hit | — |
| a monster or object, in a place you own | **Apply** | **Apply** | **Ignore**: its player claims |
| a monster or object, in a place where you're a guest | **Claim**: play it now, send `HitClaim` | **Ignore** | **Ignore** |
| a remote hero | — | **Ignore**: its player decides | — |

The owner applies a claim through `Game::hit`:
- **The attacker's side is used as sent:** no re-roll, and no reading of the attacker's powers
  or build, which the owner doesn't have.
- **The owner applies its own current defences for the target:** armour, resistances, wards
  and buffs. The guest predicted with its mirror's defences; the watermark corrects any
  difference.
- **Order:** it applies each player's claims in id order, holding any that arrive early. The
  watermark it advertises is the last claim applied, so it never passes a gap.
- It credits the claimer.

Effects on the attacker (life and mana on hit, power procs, hit-stop) happen in the attacker's
game when the hit lands there. Single player is the "a place you own" row.

### 5.7 Lag hiding
1. **Your own hero has no latency** (§2, point 3).
2. **Other players' characters show at the owner's time minus an interpolation delay:** 1.5
   times the stream interval plus the p95 jitter, clamped to 50-200 ms. On loss they
   extrapolate for up to 150 ms, then hold. Corrections blend over 100 ms; teleports (rift
   gates, respawn) snap.
3. **Hits are instant on your screen.** The flash, number and sound, your hit-stop and your
   life on hit all happen at once. The monster's bar shows predicted life: the owner's latest
   figure minus your claims above the watermark that came with it, so no claim is subtracted
   twice. A predicted kill plays the death and drops your loot at once, since loot is personal.
   If the watermark hasn't passed the killing claim within 1 s, the mirror comes back (rare).
4. **You dodge what you see.** Monster attacks resolve on your machine against your hero, with
   the full windup. It is never shortened, which is fair to the victim.
5. **Interactions** (rift gate, well, keg, chest) play at once in your game; the owner confirms.
6. **The link clock** sends a probe on each link every second and keeps RTT, jitter (EWMA) and
   each place's owner-tick estimate.
7. **Heroes pass through each other**; monsters still collide with heroes.
8. **The owner is whoever entered first.** If phones or slow browsers owning a place make
   guests' monsters stutter, hand ownership to a native player in the place. Build this only if
   it shows up.

## 6. Scaling
- **n** is the number of players in the place (E2's default). Setting `coop.scale_by = "session"`
  counts everyone in the session instead.
- **Life** is multiplied by 1 + `coop.life_per_player` × (n−1). The default 0.75 gives ×1,
  ×1.75, ×2.5 and ×3.25.
  - The owner applies it in `spawn_actor` (`arpg/mod.rs` l.598).
  - When n changes, every game rescales the actors it holds in the same way, keeping each
    life fraction (`Actor::recompute`, `arpg/combat.rs` l.295): the owner its monsters, guests
    their mirrors. So predictions, execute thresholds and damage numbers use the owner's
    maximum.
- **Damage** is multiplied by 1 + `coop.damage_per_player` × (n−1). The default 0.25 gives ×1,
  ×1.25, ×1.5 and ×1.75.
  - It is applied where monster damage is rolled in the victim's game, using that game's n,
    next to `enemy_damage`.
  - The places are `roll_damage` (`arpg/skills.rs` l.331), `death_burst` (`arpg/boss.rs`
    l.350) and powers (`arpg/powers.rs` l.251, l.348).
- **Rewards** are per player, as single player gives them. On `Killed`, every player within
  `coop.share_radius` (40 m) of the kill, or who hit the monster, gets the full XP and rolls
  their own drops. Distance is judged at the kill's owner tick: each game keeps its hero's
  positions for the last 2 s, and judges an older kill from its oldest kept position.
- Ward Totems and kegs keep their own life, as today.
- The session host's difficulty sliders apply in every game.
- All four params are a `Tunable` (`coop.*`), so `params`, `set`, presets and the tuning panel
  get them.
- **Check (C2):** compare four bots in a place against one bot alone. If kill time or deaths
  differ by more than 25%, adjust the defaults once and record it. Feel is the user's call.

## 7. Online changes to single-player features
- **Pause, speed, single step and rewind** (the Esc menu, F6, Backspace, the pad's Select): off in
  a session. The Esc menu opens over the running game.
- **Hit-stop:** `Game::time_scale` returns 1 in a session. Your hero freezes for the hit-stop's
  length, and the struck monster's pose holds on your screen. The boss entrance's 0.35 s freeze
  becomes a camera beat.
- **Studios, the F10 editor and the engine demo scenes** (world, rooms, starter): leave the
  session first. Co-op covers Shardfall's places only.
- **Character switching** in the Hall of Heroes: allowed in town; it sends `ProfileChanged`.
- **Death:** as single player: 3 s, then respawn at the place's spawn with full life. Other
  players see you fall and reappear.
- **Saves:** each game saves its own hero, as now. Rewind history isn't recorded online.

## 8. Pings, emotes and the co-op UI
**Pings.** The keys below are free in the game today (`pav_app/src/input.rs` l.111-129,
`app.rs` l.835-944).
- **World pings:** `V` (or mouse Back) pings the ground under the cursor, and the context picks
  the kind:
  - a monster → **Danger**;
  - an item, feature or spot → **Look**;
  - your own hero → **Help**;
  - anything else → **Go**.

  Hold `V` for a wheel of the four. On a pad, R3 pings at the aim point (hold it for the wheel).
  On touch, a ping button sits in the top-right row (`touch.rs` l.410-442).
- **Map pings:** with the big map open (`M`), click while holding `V` (or `Alt`) to ping that
  spot.
- **What the others see** (players in the same place only):
  - a ring and beam in the pinger's colour, with the kind's icon, for 4 s, drawn by `pav_view`
    so captures show them;
  - an arrow at the screen edge with the distance, when the ping is off screen;
  - a blip on the minimap and the big map;
  - a short synth cue per kind (`pav_audio`).
- **Limit:** at most 3 pings per 3 s per player; extras are dropped.

**Emotes.**
- **Eight**, defined as data in `game/emotes.toml` (key, name, clip, loop, icon): wave, cheer,
  bow, dance, laugh, point, sit and taunt.
  - Clips come from the store (`assets shelf=clip`, `assets find=<word> kind=clip`), keeping
    each source's credit and licence: open-licence tiers only, never `anim/local`.
  - A missing clip is made through the Animation Studio's loop.
- **Use:** hold `B` for the emote wheel (pad: L3; touch: an emote button). The emote plays on
  your hero at once and stops when you move, attack or dodge. Other players' games play it on
  your hero too.
- *Unverified:* that every clip plays on all three heroes' puppets. Check with `animsheet` in
  C4.

**UI** (egui in `pav_app`; the `coop` tool reports the same state as text, because captures
don't show egui):
- **Pause menu → Co-op:**
  - **Host:** the code, a copy button, and "players n/4".
  - **Join:** a code, or `CODE@ip:port` for a host's embedded server.
  - **Leave.**
- **Flags and settings:**
  - flags: `--host`, `--join CODE[@addr]` and `--name NAME`;
  - `shardfall.toml`: `name`, `signal = "wss://..."`, `signal_port = 3536`, `turn`,
    `turn_user` and `turn_pass`;
  - the browser: `?join=CODE`.

  The game still boots into town; hosting turns the running game into a session.
- **Party frames**, up to three: slot colour, name, character, level, a life bar, the place
  ("Emberwatch", "Depth 7, The Molten Floor"), a dead mark, and a link-quality dot.
- **Name plates** over remote heroes (`pav_view`): name and level in the slot colour, with a thin
  life bar.
- **Map:** remote heroes as arrows in their colours, and pings.
- **Toasts** for joins, leaves and lost connections. The text is fixed; there is no chat.
- **Slot colours:** 1 amber `#f2a33a`, 2 cyan `#3ac8f2`, 3 lime `#8fe04a`, 4 magenta `#e04ad0`.

## 9. Transport and connection
- **The libraries:** **matchbox_socket 0.12.0** (2025-05-21) and **matchbox_signaling 0.12.0**.
  - They are the newest versions at least 12 months old on 2026-10-16 (doctrine 9; 0.14 is
    current). Licence: MIT or Apache-2.0.
  - The same API runs on native and wasm: full-mesh WebRTC data channels with
    `ChannelConfig::unreliable()`, `reliable()` and custom configs (reliable and unordered is
    `ordered: false` with unlimited retransmits), ICE servers (`RtcIceServerConfig`, Google
    STUN by default), and a pluggable `SignallerBuilder`.
  - Native-to-browser play is a stated matchbox feature, *unverified* here until C3.
- **Rooms:** the signaling library's built-in full mesh (`full_mesh_builder`) puts every
  connected peer in one mesh. The standalone `matchbox_server` keeps rooms apart by URL path,
  using its own topology on the signaling library; its newest qualifying release is 0.11.0, and
  it has no 0.12. `pav_net` reuses that pattern on `matchbox_signaling` 0.12, so the host's
  embedded server and the public `shardfall-signal` binary run the same code.
- **Native:**
  - It uses webrtc-rs 0.13, with `ring` 0.17 and `rustls` and no OpenSSL; matchbox's lockfile
    lists about 300 crates.
  - One `net` thread runs a tokio runtime. On it run matchbox's message loop (which reaches
    tokio through `async-compat`) and, on the host, the embedded signaling server (axum on
    tokio).
  - The sim thread drains and fills the socket's channels each tick.
  - Check the Windows cross-build (`scripts/build-windows.sh`) on the first day of C3.
- **wasm** uses the browser's own WebRTC. The message loop runs under
  `wasm_bindgen_futures::spawn_local`, and the socket is polled in `pump()`
  (`pav_app/src/app.rs` l.990). No threads and no blocking calls.
- **Signaling (E1):**
  - By default, the host's native game runs `pav_net`'s signaling server on port 3536.
    Joiners use `CODE@host:3536` on a LAN, through a port forward, or over a VPN such as
    Tailscale.
  - A browser on the Vercel page is served over https, so it can only reach a `wss://` server.
    Internet play from the browser waits on E1.
  - Recommended: `shardfall-signal` in a small container with TLS, its URL in
    `shardfall.toml` and the web build.
- **NAT:** STUN only by default. TURN comes from settings if a real session fails to connect;
  most home networks connect without it.
- **Sizes:** unreliable messages are at most 1,200 bytes; snapshots go in 16 KB chunks on the
  bulk channel.

## 10. Tools, tests and targets
- **`SimNet`:** N players in one process, each with a full `Sim`, over links with seeded latency,
  jitter, loss and bandwidth. One tick steps every game and the network. Runs are deterministic,
  so tests and tools repeat exactly.
- **The `coop` tool** (one tool, help at most 50 tokens):
  - `coop players=4 latency=80 jitter=15 loss=0.02` starts an in-process session; player 1 is
    the current session.
  - `coop peer=2` points the other tools (`input`, `player`, `capture`, `see`, `game_cmd`,
    `level`) at player 2's game; `step` advances every game.
  - `coop` with no arguments reports, for each player: place, role, life, RTT, KB/s,
    interpolation delay, mirror error, unconfirmed claims and resyncs.
  - `coop bots=true seconds=60` lets bots drive every player, and reports kills, deaths, XP and
    clear times with the network numbers.
  - Through `pav live` it reports the real session.
- **Targets.** These are objective stand-ins for "feels lag-free", measured at 100 ms RTT, ±20 ms
  jitter and 2% loss, with four players in one level:

  | Measure | Target |
  |---|---|
  | Own-hero input latency | 0 ticks (true by construction; asserted) |
  | Mirror error: a guest's mirror vs the owner's actor at the same owner tick | median ≤ 0.15 m, p99 ≤ 0.5 m |
  | Pops: corrections over 0.5 m in one frame | ≤ 1 per minute per player |
  | Bandwidth | ≤ 40 KB/s per player |
  | Hit confirmation (the watermark passing a claim) | median ≤ RTT + 2 ticks, p99 ≤ 2 × RTT |
  | A guest's tick cost | ≤ the owner's |
- **Suites** in `crates/pav_core/tests/coop.rs` (new; cargo, no GPU):
  - the handshake: version mismatch, a full session;
  - entering, joining, leaving, handoff, owner drop and host drop;
  - a graceful handoff loses no change; an owner drop with players far apart promotes the
    standby without stale actors; a host drop in mid-broadcast converges on one directory;
  - a place stream that beats its `Spawned` is buffered, not resynced;
  - the claim watermark: predicted life never subtracts a claim twice, and claims that arrive
    out of order never move the watermark past a gap;
  - a handoff in mid-fight loses no claim;
  - a spawned brood's `master` points at the right actor in every game;
  - travelling between two occupied places leaves no stale `NetId`;
  - a guest's kill confirmed by the owner, with the guest's own loot;
  - victim-side hits: a dodge on the guest's screen counts;
  - scaling, and the live rescale;
  - ping and emote delivery;
  - one player's replay reproducing its hash;
  - every existing suite, unchanged.
- **`pav_net` tests:** two native peers and the embedded signaling server on 127.0.0.1 exchange
  messages on all four channels, and two rooms on one server stay apart.
- **`scripts/coop-local.sh`** (new), all on localhost, with screenshots from each window into
  `out/coop/`:
  - a native host under Xvfb and lavapipe, with `--bridge 127.0.0.1:7878`;
  - a native joiner on `:7879`;
  - a browser tab in headless Chromium with `?join=` (the flags are in `docs/REFERENCE.md`,
    "Browser and phones").
- **Windows:** the `.exe` hosting under Wine (`scripts/smoke-windows.sh`) with a Linux joiner.
- **Cost:** these are tool checks with no model calls, so they may run as often as needed
  (`AGENTS.md`, "Testing budget"). No agent judges; looks and feel go to the user.

## 11. Stages
Each stage is one PR, merged when its checks pass (`AGENTS.md`, "Per PR"), with a dated
`docs/HISTORY.md` entry and `docs/PROGRESS.md` brought up to date. Single player must behave
exactly as before after every stage.

**C0: several heroes in one place (no network).**
- **Remote-drive mode** for characters (`Character.remote`): a kinematic target, with the
  animation fed from velocity, posture and grounded. The `riding` early-out (`character.rs`
  l.494) is the template, but it skips animation, which this mode keeps.
- **Remote heroes** as `Team::Hero` actors with their own looks. Generalize `refresh_hero`
  (`arpg/mod.rs` l.441) to any hero profile.
- **AI for several heroes:** monsters target the nearest living hero, sticky with hysteresis;
  `hero_flow` is kept per targeted hero; aggro checks every hero. `PLAYER_CRUMBLE` and the
  other player-only engine checks take any hero.
- **Split `Game`:**
  - The hero side is `hero`, `hero_id` and the hero's `Actor`, `loot`, `gold`, `vendor`,
    `buyback`, `respawn`, `travel`, `floaters`, `message`, `hitstop`, `shake`, `streak`,
    `echoes`, `power_clock`, `level_flash`, the loot settings, the bags cache and `roster`.
  - The rest is the place side.
  - Add `adopt_place(snapshot, hero_side)`, and have travel use it too.
- **The damage-routing check** (§5.6) with the single-player role, and the `coop.*` params with
  the scaling functions.
- **Exit:**
  - every suite passes unchanged;
  - `campaign from=1 to=3` matches `main`;
  - a new test adds a second, remote-driven hero, and monsters go for the nearer one.

**C1: a session over the simulated network.**
- `pav_core::net`: `SimNet`, `InterpBuffer` and the link clock.
- `arpg::coop`:
  - the handshake (with the code), and the directory with host terms and reports;
  - entering, joining and leaving, and snapshot adoption (keeping the recipient's co-op
    state);
  - the standby and its stream, the three-step handoff, promotion, and resync (buffering
    early streams);
  - the hero and place streams, and spawn and despawn with complete actors.
- `Sim::step_net`, the replay's `net` track, and the `coop` tool.
- **Exit:** the session suites in `coop.rs` pass, and a four-player `coop` run meets §10's
  mirror-error, pop and bandwidth targets.

**C2: combat and rewards online.**
- Hit claims with ids, resolved results and the watermark; prediction; victim-side monster
  hits; `Killed` with its tick, and personal rewards.
- `PlaceState` and the mechanics, checking §5.5's table mechanic by mechanic.
- Scaling with the live rescale in every game, and the online rules (§7).
- **Exit:**
  - `coop bots=true` with four players clears levels 1-3 and kills the Hollow King on level 4;
  - the scaling check (§6) is done;
  - each class of mechanic has a suite.

**C3: the real network.**
- `pav_net`: matchbox on native and wasm, and the signaling server with rooms (embedded, and the
  `shardfall-signal` binary).
- App wiring: the net thread, the wasm pump, flags and settings, the Co-op window, and handshake
  errors on screen.
- `scripts/coop-local.sh`.
- **Exit:**
  - two native games and a browser tab play together over localhost WebRTC, with screenshots in
    the PR;
  - the Windows build compiles and hosts under Wine with a Linux joiner.

**C4: presence.**
- Party frames, name plates, the map, pings (world and map), emotes (data, wheel, clips), pad and
  touch bindings, and sound cues.
- **Exit:** captures show name plates and pings, smoke screenshots show the egui parts, and
  suites cover delivery.

**C5: hardening and docs.**
- Hardening:
  - rejoining within 60 s keeps your slot;
  - host and owner drops are handled under load;
  - bandwidth caps;
  - net lines in `shardfall.log`: connect, RTT, owner changes, resyncs.
- Docs:
  - `docs/COOP.md` (new) for players: hosting, joining, a port forward or Tailscale;
  - `docs/REFERENCE.md`: the tool and the tests;
  - a pointer in `AGENTS.md`, which must stay under 32 KiB;
  - a section in `docs/GAME.md`;
  - `docs/PROGRESS.md`.
- **Exit:** a 30-minute soak with four bots, one owner drop and one host drop: no stuck place,
  no panic, and fewer than one resync per minute.

**C6: internet play (after E1).** The public signaling server and optional TURN. The web build
points at the server, the user plays a real session, and their notes become follow-up work.

## 12. Rulings
These are asked in chat with this plan (2026-10-16; `docs/ESCALATIONS.md`). Each has a default,
which the run applies if no answer has come 15 minutes after it reaches the question.

| # | Question | Default |
|---|---|---|
| E1 | Where does a public signaling server run? The account, and any small cost, are the user's. | None yet. Hosts run the embedded server, internet play needs a port forward or a VPN, and browser players wait for the answer (C6). |
| E2 | Scale by the players in the place, or by everyone in the session (the request's literal "in the game")? | In the place, so a player fighting alone doesn't face four players' monsters. |
| E3 | Personal loot, or one shared drop to fight over? | Personal: each player gets single player's drops. |

## 13. Risks
- **webrtc-rs on Windows (mingw):** `ring` builds C and assembly. Verify this first in C3; if it
  won't build, escalate rather than drop the browser.
- **Strict NATs:** some pairs of players won't connect without TURN (C6).
- **Snapshot size grows with content:** measured at 41-175 KB; budget 300 KB, and leave caches
  out with `#[serde(skip)]`.
- **Big snapshots on the browser's main thread:** decoding about 1.5 MB of CBOR may hitch a
  frame. Measure in C3, and spread the work over frames if needed.
- **Promotion after a drop is approximate:** brains restart, and up to 5 s of far-off changes
  can be lost. That's acceptable for a crash; graceful leaves hand over exactly.
- **The MCP tool list is full:** keep to one `coop` tool with short help, and trim elsewhere if
  `asset_errors.rs` fails.
- **The bot's known stall** on level 4's seal (`docs/PROGRESS.md`, "Open issues") may block C2's
  exit; fix it there if it does.

## 14. Running it
The expected size is about 8,000-12,000 lines over six PRs (C0-C5); C6 waits on E1.
1. **Before you start:** answer E1-E3 (§12) if you can, so the run doesn't stop on them.
2. **Start fresh, don't compact.** Open a new session, or `/clear` this one, then give the
   `/goal` below. This file carries everything the run needs, and a `/compact` summary of
   earlier work would add tokens that every later call re-reads. A new cloud session builds
   `pav` in its start hook.
3. **Effort: extra (xhigh).** The design decisions are made here; what remains is a long,
   careful engineering run with tool-checked exits. That suits xhigh. Max would mostly add cost,
   and medium or high would risk cutting corners on ownership and resync edge cases.
4. **The goal line:**

```
/goal Build co-op as docs/COOP_PLAN.md specifies: stages C0-C5 in order, one PR per stage merged when its checks pass, single player unchanged. Done when §10's targets hold in a four-player bot session and two native games and a browser tab play together over localhost WebRTC.
```
