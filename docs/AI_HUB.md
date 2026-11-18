# Shardfall on 52in1

How Shardfall takes part in 52in1 (https://52in1.com; the repository is
`michaelcrosato/52in1`): its sessions (AI Hub Session Protocol v1,
https://52in1.com/lib/hub/README.md), its wall and the player's settings (AI Hub Catalogue v1,
https://52in1.com/lib/hub/catalogue.md) and its languages (AI Hub Languages v1,
https://52in1.com/lib/hub/languages.md; how the game does that is `docs/LANGUAGES.md`).
Players know the hub as 52in1: whatever a player sees says 52in1, never AI Hub (the protocol's
own name).

## The page
`scripts/build-hub.sh --model NAME [--effort LEVEL]` builds the browser game and packs one page,
`target/hub/pages/shardfall.html`, that plays every way the game plays together: the hub picks
the online mode each round from its `hub:multiplayer`. `scripts/pack_single_html.py --hub
web/hub.json` writes the hub's head:

- the session client (`web/hub/session.js`, the hub's own file, unmodified: client 1.4.0) as the
  first script, right after `<meta charset>`;
- the catalogue's tags from `web/hub.json` (name, tagline, description, theme colour, type, genre,
  tags, controls, harness, `hub:webgpu`), `hub:model` and `hub:effort` from the command line
  (the model that wrote the build, as its maker names it: never written into the repository),
  the emblem as the icon (an SVG, made in the packer);
- `hub:players` 1-4, `hub:multiplayer` (`parallel 1-4, ghost 1-4, shared-world co-op 1-4,
  shared-world versus 2-4, moments 1-4, same-screen co-op 2-4`) and `hub:dualsense` (the
  DualSense's extras the game uses);
- the session tags (`hub:session` 1, shared world as the mode when nothing says which, 1-4
  players, goal, how-to, 8 minutes), each online mode's own goal (`hub:session-goal-parallel`,
  `-ghost`, `-co-op`, `-versus`, `-moments`) and one `hub:moment` tag per moment;
- the `game-lines` block (`pav lines`), then the fonts as `font-NAME` blocks before `</body>`.

`web/index.html` holds the page's bridge to the client (`window.shardfallHub`): it connects as
the page loads with every online mode the page names, keeps the hub's events for the game, and
passes the game's calls on. As a safety net, a copy whose frame is too slow to answer time-up
within half the grace period reports its last standing from the page. Outside a session (no
client, or not in the hub's frame) it does nothing and the game opens on the Pavilion's plaza,
as it does anywhere.

## On one screen: same-screen co-op
Outside a session the game itself offers co-op on one screen (`pav_app::couch`): a controller
that is not the player's own shows "✕ Join"; Cross or Options there adds a hero for it (the
shared world's party, at the hero's level: Kestrel, Ysolde, then the Wanderer), and holding
Options takes it out again. The camera frames the party and draws back as it spreads, and a
player who strays too far is brought back beside the hero, so everyone stays in view. Local
players fight, pick up gold and level up; trading and the hero's windows stay the hero's. In a
session the hub's own modes play instead.

## The player's settings
In 52in1 the game finds the player's settings at `window.AIHubSettings` (the hub's companion
script puts it there on every game page, in a session or not); the bridge hands them to the
game as JSON (`hub::settings`), and again whenever the player changes them over the game.
Shardfall has no music, so `effects` multiplies the game's own effects volume and `music` has
nothing to scale; the hub applies the master volume and scales every rumble by `vibration`
itself. Outside 52in1 there are no settings and the game plays as it is.

## The DualSense
The hub holds the controller; the game reads what the player allows through
`window.AIHubSettings.dualsense` (the page's bridge, `window.shardfallHub`) and never opens it
itself. Each extra adds to a control that works on any pad and on keys; the page's
`hub:dualsense` names them, each once, in the catalogue's order: `light bar, adaptive triggers,
haptics, motion, touchpad, speaker` (`web/hub.json`; the packer refuses a name twice).

- **Light bar, adaptive triggers, haptics, touchpad** (`pav_app::pad_fx`): the hero's life on
  the light bar, R2 stiff while its skill can't be cast, short rumbles that say something, the
  touchpad's press opens the map and a swipe turns the hero's panels.
- **Motion** (`pav_app::motion`): Shardfall does its own motion aim, so while its Motion aim is
  on it reads `gyro` once a frame (`shardfallHub.gyroY`) and the hub leaves the right stick
  alone. Turning the controller left (`gyro.y` +) turns the pad's aim left on screen, by gyro x
  the frame's seconds x the sensitivity (Low 1, Medium 2, High 3), on top of the stick: a pushed
  stick gets the turn added from the push on (letting go drops it); at rest the aim follows the
  hero's facing turned by the controller's turn, and once the controller has been still for
  0.4 s with no skill held that offset eases back to nothing (a nudge for the next attacks,
  never an aim left behind). Turns under 2 degrees a second (a hand's tremor, a sensor's drift)
  add nothing. Settings shows Motion aim (on by default
  when the player's 52in1 Motion aim is) and its sensitivity while a DualSense is connected;
  without `motion` the row shows off with a line saying 52in1's Settings allow it. With the
  game's Motion aim off it never reads the gyro, so the hub's own Motion aim (if the player has
  it on) turns the right stick again half a second later; a line under the row says so.
- **Speaker** (`pad_fx::speaker_cue`, the page's `shardfallHub.speaker`): with `speaker`
  allowed and the game's Controller speaker setting on (its default), the page makes its own
  AudioContext on the first cue and asks `openSpeaker(ctx)` once; into the node it gives it
  synthesizes three short cues, on top of the game's own sounds, for the player's own hero
  (its potions going down, its level going up, a rare or unique item it was the nearest hero
  to): a potion drunk (three rising gulps), a rare or unique item picked up (a two-note chime, a
  four-note arpeggio with a shimmer) and a level-up (a short fanfare), at the game's effects
  volume x 52in1's. A `null` node plays nothing more (asked again when the player's settings
  change), and a speaker that goes away and comes back is asked for a fresh node. Without `speaker`
  (it needs a DualSense on a cable and the microphone allowed) the row shows off with a line
  saying where 52in1 allows it.
- **Microphone: never.** Shardfall has no voice feature: it never calls `openMicrophone()` or
  `getUserMedia()`, so `mic`, `micLater` and `muted` change nothing and Settings shows no voice
  row (the catalogue asks for its line only for voice features a game has). `pad_check.py
  --dualsense` counts any request and expects none.
- Natively there is no hub: no motion aim, no speaker, no rows.

## A round
`pav_app::round` (the app's side) and `pav_core::arpg::round` (the game's):

- **No title, no menus.** In a session the page opens straight into the round: the title
  screen, the pause menu, quitting and the tools are off. The hub's card is shown over the game
  (`hub:session-goal`, `hub:session-howto`).
- **The world.** When the hub has said who plays (`info`), the round's world is built: depth 1,
  a fresh hero per player (seat 0 Wanderer, 1 Kestrel, 2 Ysolde, 3 Wanderer), every level built
  from the round's seed and its depth alone (`Game::round_seed`), so every copy has the same
  levels. Then `ready()`, with no key press.
- **The start.** The world holds still until the hub's clock reaches 0 (a countdown shows), then
  plays. Before the start a copy only clears its frame: the card covers it, and the hub's test
  loads up to four copies on one machine, which must all be ready within 20 seconds.
- **The end.** On `end` the game stops: no input, no sound, nothing sent.
- **Logic apart from drawing.** The round (the hub's events, the player's input out, the world's
  ticks, reports) runs every frame and, in the browser, every 50 ms between frames too
  (`App::tick_round`), so a copy whose drawing falls behind keeps its round and its traffic on
  time. The browser build waits for animation frames rather than polling once its graphics are
  up: same-origin frames share one thread, and a polling copy starved the others.
- **A software GPU.** On a CPU adapter (SwiftShader, lavapipe: the hub's test machines) a round
  draws at low quality, at a small render scale, with no shadowed lights. Chrome's SwiftShader
  still takes 0.5-1 s a frame (4-5 s for the first, while it compiles), whatever is switched off:
  the logic above is what keeps a round right there.

### Moments: "Clear each moment's goal before its time runs out"
A round of moments is three short moments, each loaded afresh: `info.moment` names one of
`pav_core::arpg::moments::MOMENTS` (the scene `moment/<id>` in the tools). Each is the Proving
Grounds with one goal and a fixed hero, the same on every copy (built from the round's seed and
the moment's name): no portal, no waves of its own, no loot.

| Moment | Verb | Seconds | Goal |
|---|---|---|---|
| `pack` | defeat | 25 | Defeat every monster in the pack |
| `champion` | defeat | 30 | Slay the champion and its guards |
| `swarm` | survive | 20 | Stay alive while the swarm closes in |
| `gold` | collect | 20 | Grab every pile of gold before time runs out |

The game decides the outcome in the simulation (`moments::update`): every monster down or every
pile picked up clears it, a fallen hero fails it (no respawn), and the clock running out clears
`swarm` and fails the rest. The copy reports `result({ won, time })` once, the moment it is
decided, then holds the world still; at `time-up` a moment still going is reported as it stands
(the page reports it itself if the frame is too slow). The HUD shows the goal, the time left as a
bar, how many are left, and the outcome as a banner; in `gold` each pile has a ring, and an arrow
on the screen's edge points to piles out of view. Measured with `autoplay` (seeds 1-5), the bot
clears pack 5 of 5 and champion 3 of 5 and survives the swarm 5 of 5 at 40-100% life;
`pav_core/tests/moments.rs` walks the gold in under 80% of its time on seeds 1-5.

### Parallel and ghost: "Descend as deep as you can"
Each copy plays its own round from the same start. Progress about once a second: the score is
depth x 1000 + kills (`round::score`), the label `session.progress` ("Depth 3, 41 kills") in the
player's language. At time-up the result (`score`, and the depth and kills as stats).

Ghost: each copy sends its hero's look once (the puppet definition, weapon included) and ten
times a second where it is and its pose (`PuppetState`, unreliable). The others' heroes are drawn
where they are, on the same depth, with the seat's colour and the player's name over them. They
are view objects added to the frame (`Round::ghost_objects`): this copy's world never sees them.

### Shared world: co-op "Clear the depths together", versus "Slay more monsters than the others"
All players' heroes in one world, run by the host (`pav_app::coop`, `pav_core::arpg::party`):

- **The game.** Besides the game's own hero, `Game::party` holds the other players' heroes, each
  with its own profile, entity and respawn timer. Their input, casts, pickups, respawns and kill
  rewards run as the hero's by swapping the member into the hero's place (`with_member`), so every
  rule written for one hero holds for all. Monsters go for the nearest living hero, with a flow
  field per hero; the kill counts for whoever made it; every hero gains the experience; drops
  fall once and go to whoever picks them up. Travel takes the whole party down.
- **The network.** Players send the host what they do (`in`: their inputs, a few ticks a
  message). The host runs the world: each tick it takes every seat's input (one that hasn't come
  repeats the last, with no new presses; it never waits), applies it, and sends the tick's
  inputs to everyone in bundles (`b`, four ticks each, 15 a second). Every copy runs the same
  world from the bundles: one browser build, no clock, every random number from the seed, so the
  worlds stay the same; the host adds a hash of its world every two seconds (`h`) and a copy that
  differs logs it. Inputs travel quantized and the host applies its own the same way.
- **Seats.** The first player plays the game's own hero, the others are party members; each copy
  shows its own hero (`Sim::viewer`, outside the shared state). A host with no player of its own
  (the hub's server) runs the world and draws little.
- **Leaving and the end.** The host marks a player's leaving in the bundle (their hero leaves at
  that tick on every copy). The host finishes the round: at time-up, or when the party reaches
  depth 4 (`coop::GOAL_DEPTH`). The hub says how the round plays (`info.play`): in co-op every
  player shares one place in the standings and `won` says whether the party got deep enough; in
  versus the standings go by kills, most first. The kills are the scores either way.

## Testing
`pav_core/tests/coop.rs` checks the party (each hero by its own input, a member's casts and
respawn, the same inputs giving the same world). The hub's harness plays the hub around the page:

```bash
mkdir -p .cache/harness && cd .cache/harness
for f in harness.html bridge.js round.js; do curl -fsSO https://52in1.com/lib/hub/$f; done
cp ../../target/hub/pages/shardfall.html . && python3 -m http.server 8765
```

then open `http://localhost:8765/harness.html?game=shardfall.html&players=2&auto=1` with
`&mode=parallel`, `&mode=ghost`, `&mode=shared-world&how=co-op`, `&mode=shared-world&how=versus`
or `&mode=moments` (`&moment=pack,gold,swarm` picks the moments), and the hub's own cases
(`players=4&latency=150&jitter=50&loss=5`, `scenario=host-leave`, `players=2&scenario=server`)
in Chromium with WebGPU; it passes when `window.__hubCheck.pass` is true. Headless, Chromium
needs `--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader
--use-angle=swiftshader`.

In 52in1's own hub (its Wall, Settings, frame host and companion, as its settings test serves
them) with a stand-in DualSense for WebHID, `scripts/hub_live.mjs` plays Shardfall as a player
would and records what the hub sends the controller: the light bar's colours and the right
trigger's modes as HID reports, the rumbles, the right stick with Motion aim on (the hub turns it
only while the game doesn't read `gyro` itself), the touchpad, and the frame's permissions and
the game's asks for the microphone and the speaker before and after 52in1 allows them. Setup:

```bash
git clone -q --filter=blob:none --no-checkout --depth 1 https://github.com/michaelcrosato/52in1.git target/hubtest/52in1
cd target/hubtest/52in1 && git sparse-checkout init --no-cone
printf '/lib/\n/assets/\n/cloudflare/\n/scripts/*.mjs\n/scripts/lib/\n/*.html\n/*.json\n/*.js\n' > .git/info/sparse-checkout
git checkout -q main && mkdir -p data games covers/shardfall
```

then write `data/games.json` with Shardfall's entry from `https://52in1.com/data/games.json`
(`path` and its build's `path` set to `/games/shardfall-v3.html`), copy the page there, install
Node's Playwright 1.63 in a folder of its own, and from that folder run `node
.../scripts/hub_live.mjs target/hubtest/52in1 OUT_DIR` (OUT_DIR/hub_live.json).

## The package
`scripts/hub-package.sh --model NAME --effort LEVEL` builds the page, puts it in
`target/hub/package/games/shardfall.html`, takes the three screenshots with
`scripts/hub_covers.py` (`covers/shardfall/0.jpg` the opening view on the plaza, `1.jpg` the Proving Grounds,
`2.jpg` a descent level: 640 x 480 baseline JPEG at quality 58, from a 960 x 720 window at scale
2/3) and runs the hub's checker on the folder. The package is handed over, not committed (it
holds the build and its images); importing it into the hub is the hub's step (`pnpm ingest`).
The wall holds Shardfall since the first package (2026-11-13 the checker found it there), so a
new package is a new build: `--build v2` (then v3...) names the page `shardfall-v2.html`, which
the checker files under the same slug, `shardfall`.
