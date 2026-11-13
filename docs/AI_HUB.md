# Shardfall on the AI Hub

How Shardfall takes part in the AI Hub (https://github.com/michaelcrosato/ai-hub): its sessions
(AI Hub Session Protocol v1, `lib/hub/README.md`), its wall (AI Hub Catalogue v1,
`lib/hub/catalogue.md`) and its languages (AI Hub Languages v1, `lib/hub/languages.md`; how
the game does that is `docs/LANGUAGES.md`).

## The page
`scripts/build-hub.sh --model NAME [--effort LEVEL]` builds the browser game once and packs one
page per session mode into `target/hub/pages/` (`shardfall-shared-world.html`, `-ghost.html`,
`-parallel.html`). `scripts/pack_single_html.py --hub web/hub.json` writes the hub's head:

- the session client (`web/hub/session.js`, the hub's own file, unmodified) as the first script,
  right after `<meta charset>`;
- the catalogue's tags from `web/hub.json` (name, tagline, description, theme colour, type, genre,
  tags, controls, harness, `hub:webgpu`), `hub:model` and `hub:effort` from the command line
  (the model that wrote the build, as its maker names it: never written into the repository),
  the emblem as the icon (an SVG, made in the packer);
- the session tags (`hub:session` 1, the mode, 1-4 players, goal, how-to, 8 minutes); the
  tags follow the mode: `co-op` and `hub:players` for a shared world, `race` for ghost;
- the `game-lines` block (`pav lines`), then the fonts as `font-NAME` blocks before `</body>`.

`web/index.html` holds the page's bridge to the client (`window.shardfallHub`): it connects as
the page loads, keeps the hub's events for the game, and passes the game's calls on. As a safety
net, a copy whose frame is too slow to answer time-up within half the grace period reports its
last standing from the page. Outside a session (no client, or not in the hub's frame) it does
nothing and the game opens on the Pavilion's plaza, as it does anywhere.

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

### Parallel and ghost: "Descend as deep as you can"
Each copy plays its own round from the same start. Progress about once a second: the score is
depth x 1000 + kills (`round::score`), the label `session.progress` ("Depth 3, 41 kills") in the
player's language. At time-up the result (`score`, and the depth and kills as stats).

Ghost: each copy sends its hero's look once (the puppet definition, weapon included) and ten
times a second where it is and its pose (`PuppetState`, unreliable). The others' heroes are drawn
where they are, on the same depth, with the seat's colour and the player's name over them. They
are view objects added to the frame (`Round::ghost_objects`): this copy's world never sees them.

### Shared world: "Clear the depths together"
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
  depth 4 (`coop::GOAL_DEPTH`, then `won`). Standings by kills, scores the kills.

## Testing
`pav_core/tests/coop.rs` checks the party (each hero by its own input, a member's casts and
respawn, the same inputs giving the same world). The hub's harness plays the hub around the page:

```bash
mkdir -p .cache/harness && cd .cache/harness
for f in harness.html bridge.js round.js; do curl -fsSO https://raw.githubusercontent.com/michaelcrosato/ai-hub/main/lib/hub/$f; done
cp ../../target/hub/pages/*.html . && python3 -m http.server 8765
```

then open `http://localhost:8765/harness.html?game=shardfall-shared-world.html&players=2&auto=1`
(and `players=4&latency=150&jitter=50&loss=5`, `scenario=host-leave`, `players=2&scenario=server`;
the ghost and parallel pages the same way) in Chromium with WebGPU; it passes when
`window.__hubCheck.pass` is true. Headless, Chromium needs `--enable-unsafe-webgpu
--enable-features=Vulkan --use-vulkan=swiftshader --use-angle=swiftshader`.

## The package
`scripts/hub-package.sh --model NAME --effort LEVEL` builds the pages, puts the shared-world one
in `target/hub/package/games/shardfall.html`, takes the three screenshots with
`scripts/hub-covers.js` (`covers/shardfall/0.jpg` the opening view on the plaza, `1.jpg` the Proving Grounds,
`2.jpg` a descent level: 640 x 480 baseline JPEG at quality 58, from a 960 x 720 window at scale
2/3) and runs the hub's checker on the folder. The package is handed over, not committed (it
holds the build and its images); importing it into the hub is the hub's step (`pnpm ingest`).
