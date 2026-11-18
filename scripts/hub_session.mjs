#!/usr/bin/env node
/**
 * hub_session.mjs — Shardfall in real 52in1 game nights: two players join the hub's own session
 * page (/session, assets/js/session/app.js) and play one round of Shardfall in each online mode
 * its page declares (hub:multiplayer: parallel, ghost, shared-world co-op, shared-world versus,
 * moments), through the hub's lobby, its Play filter, its card, its round coordinator
 * (lib/hub/round.js), its bridge (lib/hub/bridge.js) and its transports, as 52in1's own
 * scripts/session-browser.mjs plays its reference games:
 *  - two tabs of one browser on the local transport (?transport=local: a BroadcastChannel);
 *  - two browser contexts over the real WebRTC mesh, this script standing in for the night's
 *    relay (presence, control and signalling) through ?transport=relay, as session-browser.mjs's
 *    meshSession does, so the game traffic and the clock really travel peer to peer.
 *
 * usage (from a folder where Node's `playwright` 1.63 is installed, as for scripts/hub_live.mjs):
 *   node /path/to/shardfall/scripts/hub_session.mjs 52IN1_CHECKOUT OUT_DIR \
 *     [--modes=parallel,ghost,co-op,versus,moments] [--relay=co-op,ghost] [--limit=45]
 * The checkout: the sparse 52in1 clone of scripts/hub_live.mjs plus /session/ (the session page),
 * with games/shardfall-v3.html the page to test (copy target/hub/pages/shardfall.html there).
 * data/games.json is served as the hub's scan would write it for that page: the checkout's
 * Shardfall entry with the page's own hub:* metas over it (scripts/lib/parse.mjs parseHead, as
 * scripts/scan.mjs folds them in), so the deal sees every mode the page declares.
 *
 * Each round: a fresh night of two seats; the host sets one round and the Play filter to the
 * mode, presses Start; both press Ready on the hub's card; the hub deals Shardfall, loads both
 * copies, counts 3-2-1 and calls time at --limit seconds (the host's ?limit= knob; a round of
 * moments runs its three moments' own lengths). Records per round: the deal, both copies'
 * hello/ready, the start, what each game saw (window.AIHub after its welcome), the game traffic
 * each hub page's bridge counted and the progress the scoreboards heard, the round's record in
 * the history, the results card and the final standings as the hub showed them, and every page
 * error. Writes OUT_DIR/hub_session.json and screenshots.
 *
 * One stand-in: headless Chromium's WebGPU adapter here is SwiftShader, which reports
 * isFallbackAdapter, and the session page counts a seat with only a fallback adapter as having
 * no WebGPU (assets/js/session/app.js, the `webgpu` check), so it would never deal Shardfall
 * (hub:webgpu required; pick.js `eligible`). The hub page (not the game's frame) is told its
 * adapter is not a fallback one, as a seat with a real GPU would say.
 */
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

// Playwright from the working folder (it is not a dependency of this repository).
const require = createRequire(path.join(process.cwd(), 'noop.js'));
const { chromium } = require('playwright');

const argv = process.argv.slice(2);
const positional = argv.filter((a) => !a.startsWith('--'));
const flag = (name, fallback) => {
  const hit = argv.find((a) => a.startsWith(`--${name}=`));
  return hit ? hit.slice(name.length + 3) : fallback;
};
const ROOT = path.resolve(positional[0] ?? '.');
const OUT = path.resolve(positional[1] ?? 'hub_session_out');
fs.mkdirSync(OUT, { recursive: true });
const LIMIT_S = Number(flag('limit', '45'));
/** The modes, by the Play filter's values (assets/js/session/pick.js, MODES). */
const MODES = {
  parallel: 'parallel',
  ghost: 'ghost',
  'co-op': 'shared-world co-op',
  versus: 'shared-world versus',
  moments: 'moments',
};
const listOf = (text) => text.split(',').map((s) => s.trim()).filter(Boolean);
const LOCAL_MODES = listOf(flag('modes', 'parallel,ghost,co-op,versus,moments'));
const RELAY_MODES = listOf(flag('relay', 'co-op,ghost'));
for (const m of [...LOCAL_MODES, ...RELAY_MODES]) if (!MODES[m]) throw new Error(`unknown mode ${m}: one of ${Object.keys(MODES).join(', ')}`);

const GAME_FILE = path.join(ROOT, 'games/shardfall-v3.html');
const { staticHandler } = await import(path.join(ROOT, 'scripts/lib/serve.mjs'));
const { parseHead } = await import(path.join(ROOT, 'scripts/lib/parse.mjs'));
const { modesOf } = await import(path.join(ROOT, 'lib/hub/round.js'));

// The library as the hub's scan would write it for this page: the entry, the page's hub metas over it.
const manifest = JSON.parse(fs.readFileSync(path.join(ROOT, 'data/games.json'), 'utf8'));
const base_entry = manifest.games.find((g) => g.slug === 'shardfall');
if (!base_entry) throw new Error('data/games.json has no shardfall entry');
const head = parseHead(fs.readFileSync(GAME_FILE, 'utf8'));
const entry = { ...base_entry, ...head.hub };
const library = { ...manifest, games: [entry] };

const server = createServer(staticHandler(ROOT, { companion: true }));
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
const t0 = performance.now();
const log = (s) => console.log(`${((performance.now() - t0) / 1000).toFixed(1).padStart(6)}s  ${s}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.launch({
  headless: true,
  args: [
    '--no-sandbox', '--autoplay-policy=no-user-gesture-required',
    '--enable-unsafe-webgpu', '--use-vulkan=swiftshader', '--use-angle=swiftshader',
    // Plain host candidates, so two contexts on one machine find each other (session-browser.mjs).
    '--enable-features=Vulkan', '--disable-features=WebRtcHideLocalIpsWithMdns',
  ],
});

const report = {
  started: new Date().toISOString(),
  hub_commit: null,
  page: { file: path.relative(ROOT, GAME_FILE), bytes: fs.statSync(GAME_FILE).size },
  entry: {
    multiplayer: entry.multiplayer, sessionMode: entry.sessionMode, sessionPlayers: entry.sessionPlayers,
    sessionMinutes: entry.sessionMinutes, webgpu: entry.webgpu, moments: entry.moments,
    modes: modesOf(entry).map((m) => ({ key: m.key, where: m.where, play: m.play, seats: m.seats, seeded: m.seeded })),
  },
  knobs: { limit_s: LIMIT_S, splash_s: 20, grace_s: 150, results_s: 8, rate_s: 3, guess_s: 3, data_s: 3 },
  stand_ins: [
    'the hub page is told its WebGPU adapter is not a fallback one (SwiftShader is): assets/js/session/app.js webgpu check, pick.js eligible',
    'data/games.json served with the page\'s hub:* metas over the checkout\'s entry, as scripts/scan.mjs would write it',
    'relay rounds: this script relays presence, control and signalling as session-browser.mjs meshSession does (?transport=relay)',
  ],
  rounds: [],
};
try {
  const { execSync } = await import('node:child_process');
  report.hub_commit = execSync('git log -1 --format=%h%x20%s', { cwd: ROOT }).toString().trim().slice(0, 120);
} catch {}

/** Every page of a round: the library, the adapter stand-in, errors from the hub page and the game's frame. */
async function prepare(ctx) {
  await ctx.route('**/data/games.json', (route) => route.fulfill({ json: library }));
  await ctx.addInitScript(() => {
    if (window.top !== window || !navigator.gpu) return;
    const real = GPU.prototype.requestAdapter;
    GPU.prototype.requestAdapter = async function (...args) {
      const adapter = await real.apply(this, args);
      if (!adapter) return adapter;
      const info = adapter.info;
      const copy = { vendor: info?.vendor, architecture: info?.architecture, device: info?.device, description: info?.description, isFallbackAdapter: false };
      Object.defineProperty(adapter, 'info', { configurable: true, value: copy });
      return adapter;
    };
  });
}

/**
 * Errors (uncaught, console errors, crashes) from the hub page and the game's frame; and the
 * warnings worth reading: the session client's ([AIHub]), the hub page's ([session]), and the
 * game's own (Shardfall logs a shared world whose copy differs from the host's as a warning).
 */
function watch(page, who, rec) {
  page.on('pageerror', (e) => rec.errors.push(`${who} pageerror: ${String(e.message).slice(0, 300)}`));
  page.on('crash', () => rec.errors.push(`${who}: page crashed`));
  page.on('console', (m) => {
    const where = (m.location()?.url || '').includes('/games/') ? ' (game frame)' : '';
    const text = m.text().slice(0, 300);
    if (m.type() === 'error') rec.errors.push(`${who} console${where}: ${text}`);
    else if (m.type() === 'warning' && (where || /\[AIHub\]|\[session\]/.test(text))) rec.warnings.push(`${who} warning${where}: ${text}`);
  });
}

/** Each message once, with how many times it came. */
const counted = (list) => {
  const counts = new Map();
  for (const e of list) counts.set(e, (counts.get(e) ?? 0) + 1);
  return [...counts].map(([e, n]) => (n > 1 ? `${e} (x${n})` : e));
};

const waitFor = (page, fn, arg, timeout = 30_000) => page.waitForFunction(fn, arg, { timeout, polling: 250 });
const session = (page) => page.evaluate(() => JSON.parse(JSON.stringify({
  key: window.__session?.key, seat: window.__session?.seat, state: window.__session?.state, status: window.__session?.status,
  traffic: window.__session?.traffic, progress: window.__session?.progress, card: window.__session?.card, links: window.__session?.links,
  clock: window.__session?.clock,
})));
const frameOf = async (page) => {
  const handle = await page.$('#player iframe');
  return handle ? handle.contentFrame() : null;
};
/** What the game saw: the session client's fields after its welcome, and Shardfall's own copy of it. */
async function gameSaw(page) {
  const frame = await frameOf(page);
  if (!frame) return null;
  return frame.evaluate(async () => {
    // The frames the game draws in a second and a half (software WebGPU here is slow).
    const fps = await new Promise((resolve) => {
      let n = 0;
      const start = performance.now();
      const tick = () => {
        n++;
        if (performance.now() - start < 1500) requestAnimationFrame(tick);
        else resolve(Math.round((n / ((performance.now() - start) / 1000)) * 10) / 10);
      };
      requestAnimationFrame(tick);
      setTimeout(() => resolve(Math.round((n / 4) * 10) / 10), 4000);
    });
    const a = window.AIHub;
    const s = window.shardfallHub;
    return {
      url: location.pathname + location.search,
      client: a?.version ?? null,
      active: a?.active ?? null,
      mode: a?.mode ?? null,
      play: a?.play ?? null,
      me: a?.me ?? null,
      host: a?.host ?? null,
      isHost: a?.isHost ?? null,
      players: (a?.players ?? []).map((p) => `${p.id}:${p.name}`),
      seed: a?.seed ?? null,
      seeded: a?.seeded ?? null,
      limitMs: a?.limitMs ?? null,
      round: a?.round ?? null,
      moment: a?.moment ?? null,
      time: typeof a?.time === 'function' ? Math.round(a.time()) : null,
      shardfall: s ? { active: s.active, waiting: s.waiting, mode: s.info?.mode ?? null, play: s.info?.play ?? null, reported: s.reported, score: s.score } : null,
      canvas: Boolean(document.querySelector('canvas')),
      fps,
    };
  }).catch((e) => ({ error: String(e.message).slice(0, 200) }));
}

/**
 * A little play in each game, every tab in turn: swings and a skill at whatever is around the
 * hero (the middle of the screen; a click also gives the canvas the keyboard), then a walk.
 */
async function play(pages, ms) {
  const keys = ['w', 'a', 's', 'd'];
  const skills = ['q', 'e', 'r', 'f'];
  const end = Date.now() + ms;
  let i = 0;
  while (Date.now() < end) {
    for (const page of pages) {
      try {
        const frame = page.locator('#player iframe');
        const box = await frame.boundingBox();
        if (box) {
          for (let k = 0; k < 3; k++) {
            const a = Math.random() * Math.PI * 2;
            await page.mouse.click(box.x + box.width * (0.5 + 0.12 * Math.cos(a)), box.y + box.height * (0.5 + 0.15 * Math.sin(a)));
            await sleep(150);
          }
        }
        await page.keyboard.press(skills[i % skills.length]);
        const key = keys[i++ % keys.length];
        await page.keyboard.down(key);
        await sleep(900);
        await page.keyboard.up(key);
      } catch {}
    }
    await sleep(200);
  }
}

async function shot(page, name) {
  const file = path.join(OUT, name);
  await page.screenshot({ path: file }).catch(() => {});
  return path.relative(OUT, file);
}

const textOf = (page, sel) => page.evaluate((s) => document.querySelector(s)?.innerText?.replace(/\n{2,}/g, '\n').trim() ?? null, sel).catch(() => null);

/**
 * One night of two seats playing one round of `label` (a key of MODES) on `transport`.
 */
async function night(label, transport) {
  const filter = MODES[label];
  const rec = { label, filter, transport, ok: false, errors: [], warnings: [], timeline: {}, shots: [] };
  report.rounds.push(rec);
  const tag = `${transport}-${label}`;
  const at = performance.now();
  const mark = (what) => (rec.timeline[what] = Math.round((performance.now() - at) / 100) / 10);
  const contexts = [];
  const pages = {};
  const viewport = { width: 960, height: 600 };
  const knobs = `limit=${LIMIT_S}&splash=20&grace=150&results=8&rate=3&guess=3&data=3`;
  try {
    if (transport === 'local') {
      const ctx = await browser.newContext({ viewport, reducedMotion: 'reduce' });
      contexts.push(ctx);
      await prepare(ctx);
      for (const seat of [1, 2]) {
        const page = await ctx.newPage();
        watch(page, `seat ${seat}`, rec);
        await page.goto(`${base}/session?transport=local&as=${seat}&${knobs}#sf-${label}`);
        pages[seat] = page;
      }
    } else {
      // session-browser.mjs meshSession: presence and envelopes pass through Node, game traffic over WebRTC.
      const presence = new Map();
      const members = new Map();
      const push = (page, msg) => page.evaluate((m) => window.__relayIn?.(m), msg).catch(() => {});
      const everyone = (msg, except) => {
        for (const [k, page] of members) if (k !== except) push(page, msg);
      };
      const list = () => [...presence.entries()].map(([key, meta]) => ({ ...meta, key }));
      rec.relay = { envelopes: 0 };
      for (const seat of [1, 2]) {
        const ctx = await browser.newContext({ viewport, reducedMotion: 'reduce' });
        contexts.push(ctx);
        await prepare(ctx);
        await ctx.exposeBinding('__relayOut', (_source, msg) => {
          if (msg.op === 'track') {
            presence.set(msg.key, msg.meta);
            everyone({ op: 'presence', list: list() });
          } else if (msg.op === 'send') {
            rec.relay.envelopes++;
            everyone({ op: 'event', event: msg.event, payload: msg.payload }, msg.key);
          } else if (msg.op === 'leave') {
            presence.delete(msg.key);
            everyone({ op: 'presence', list: list() });
          }
        });
        await ctx.addInitScript(() => {
          window.__sessionRelay = (key) => {
            const handlers = {};
            let presenceFn = () => {};
            let last = [];
            window.__relayIn = (m) => {
              if (m.op === 'presence') presenceFn((last = m.list));
              else if (m.op === 'event') handlers[m.event]?.(m.payload);
            };
            return {
              track: (meta) => window.__relayOut({ op: 'track', key, meta }),
              onPresence: (fn) => {
                presenceFn = fn;
                fn(last);
              },
              send: (event, payload) => window.__relayOut({ op: 'send', key, event, payload }),
              on: (event, fn) => (handlers[event] = fn),
              close: () => window.__relayOut({ op: 'leave', key }),
            };
          };
        });
        const page = await ctx.newPage();
        watch(page, `seat ${seat}`, rec);
        members.set(`seat${seat}`, page);
        await page.goto(`${base}/session?transport=relay&as=${seat}&${knobs}#sf-${tag}`);
        pages[seat] = page;
      }
    }
    const [host, guest] = [pages[1], pages[2]];
    for (const seat of [1, 2]) await waitFor(pages[seat], (n) => window.__session?.seat === n - 1 && document.body.dataset.phase === 'lobby', seat);
    await waitFor(host, () => document.querySelectorAll('.seat:not(.is-empty)').length === 2);
    if (transport === 'relay') {
      for (const [seat, other] of [[1, 'seat2'], [2, 'seat1']]) await waitFor(pages[seat], (k) => window.__session?.links?.[k] === 'open', other, 30_000);
      rec.links = (await session(host)).links;
      log(`${tag}: two contexts connected peer to peer`);
    }
    mark('lobby');
    // One round, so the night ends on its final standings after it.
    for (let i = 0; i < 5 && (await host.textContent('.rounds-step__count')) !== '1'; i++) await host.click('.rounds-step__btn >> nth=0');
    // The Play filter: this mode only.
    const id = `filter-modes-${filter.replace(/[^a-z0-9]+/gi, '-')}`;
    await waitFor(host, (i) => Boolean(document.getElementById(i)), id, 15_000);
    await host.evaluate((i) => document.getElementById(i).click(), id);
    await waitFor(guest, (f) => window.__session?.state?.filters?.modes?.includes(f) && window.__session.state.rounds === 1, filter, 10_000);
    rec.lobby = {
      filters: (await session(guest)).state.filters,
      format: await textOf(host, '.format-line'),
      play_row: await textOf(host, `label:has(#${id})`),
      start: await textOf(host, '#start'),
    };
    rec.shots.push(await shot(host, `${tag}-0-lobby.png`));
    log(`${tag}: lobby set (${JSON.stringify(rec.lobby.filters)}, ${rec.lobby.format}); host presses Start`);
    await host.click('#start');
    for (const page of [host, guest]) {
      await waitFor(page, () => window.__session?.state?.current?.round === 1 && Boolean(document.querySelector('.splash__ready')) && !document.querySelector('#results').hidden, null, 30_000);
    }
    mark('card');
    const cur = (await session(host)).state.current;
    rec.dealt = { name: cur.name, slug: cur.slug, path: cur.path, mode: cur.mode, play: cur.play, seeded: cur.seeded, limitMs: cur.limitMs, seed: cur.seed, moments: cur.moments?.map((m) => m.id) ?? null, players: cur.players.map((p) => p.key) };
    rec.card = await textOf(guest, '#results');
    log(`${tag}: dealt ${cur.name} as ${cur.mode}${cur.play ? ` ${cur.play}` : ''}${rec.dealt.moments ? ` (${rec.dealt.moments.join(', ')})` : ''}; both press Ready`);
    await sleep(1_000);
    for (const page of [host, guest]) {
      await page.click('.splash__ready');
      await waitFor(page, () => window.__session.state.current?.splash?.ready.includes(window.__session.key) || window.__session.state.current?.splash?.released, null, 5_000);
    }
    await waitFor(host, () => window.__session.state.current?.splash?.released, null, 10_000);
    mark('released');
    // Both copies load (hello, then ready), then the 3-2-1.
    await waitFor(host, () => ['countdown', 'playing'].includes(window.__session?.status?.phase), null, 200_000);
    mark('started');
    let st = (await session(host)).status;
    rec.loaded = st.players.map((p) => ({ key: p.key, hello: p.hello, ready: p.ready, out: p.out, client: p.client }));
    log(`${tag}: both copies loaded (${rec.loaded.map((p) => `${p.key} hello ${p.hello} ready ${p.ready}${p.out ? ` out ${p.out}` : ''}`).join('; ')}); the round starts`);
    await waitFor(host, () => window.__session?.status?.phase === 'playing', null, 15_000).catch(() => {});
    const [saw1, saw2] = await Promise.all([gameSaw(host), gameSaw(guest)]);
    rec.game_saw = { seat1: saw1, seat2: saw2 };
    log(`${tag}: the games saw mode ${rec.game_saw.seat1?.mode}/${rec.game_saw.seat2?.mode} play ${rec.game_saw.seat1?.play}/${rec.game_saw.seat2?.play}, me ${rec.game_saw.seat1?.me}/${rec.game_saw.seat2?.me}, isHost ${rec.game_saw.seat1?.isHost}/${rec.game_saw.seat2?.isHost}, ${rec.game_saw.seat1?.fps}/${rec.game_saw.seat2?.fps} fps`);
    rec.samples = [];
    const sample = async () => {
      const [a, b] = await Promise.all([session(host), session(guest)]);
      rec.samples.push({
        t: Math.round((performance.now() - at) / 1000),
        phase: a.status?.phase ?? a.state?.phase,
        step: a.status?.step ?? null,
        traffic: { seat1: a.traffic, seat2: b.traffic },
        progress: { seat1: a.progress, seat2: b.progress },
        // The guest keeps the host's time (clock.js), over the mesh on the relay transport.
        clock: b.clock,
        results: (a.status?.players ?? []).map((p) => p.result ?? null),
      });
    };
    if (filter === 'moments') {
      // Three moments, each loaded afresh in both tabs, each with its own cue and length.
      rec.moments = [];
      for (let step = 0; step < (rec.dealt.moments?.length ?? 3); step++) {
        try {
          await waitFor(host, (n) => (window.__session?.status?.step === n && window.__session.status.phase === 'playing') || window.__session?.state?.phase !== 'round' || (window.__session?.status?.step ?? 0) > n, step, 200_000);
        } catch {
          break;
        }
        if ((await session(host)).state.phase !== 'round') break;
        const [m1, m2] = await Promise.all([gameSaw(host), gameSaw(guest)]);
        const saw = { step, id: rec.dealt.moments?.[step], seat1: m1, seat2: m2 };
        rec.moments.push(saw);
        log(`${tag}: moment ${step + 1} (${saw.id}) playing; the games saw moment ${saw.seat1?.moment}/${saw.seat2?.moment}, mode ${saw.seat1?.mode}/${saw.seat2?.mode}, seed ${saw.seat1?.seed}/${saw.seat2?.seed}, ${saw.seat1?.fps}/${saw.seat2?.fps} fps`);
        if (step === 0) rec.shots.push(await shot(host, `${tag}-1-moment1-seat1.png`));
        // Play until this moment is over.
        const until = Date.now() + 40_000;
        while (Date.now() < until) {
          const s = await session(host);
          if (s.state?.phase !== 'round' || (s.status?.step ?? 0) > step || s.status?.phase === 'grace') break;
          await play([host, guest], 2_500);
          await sample();
          if (step === 0 && !rec.shots.some((f) => f.includes('moment1-later'))) rec.shots.push(await shot(host, `${tag}-1-moment1-later-seat1.png`));
        }
        await sample();
      }
    } else {
      // Play to the hub's time limit, sampling what moved.
      let shotTaken = false;
      const until = Date.now() + LIMIT_S * 1000 + 30_000;
      while (Date.now() < until) {
        const s = await session(host);
        if (s.state?.phase !== 'round' || ['grace', 'ended'].includes(s.status?.phase)) break;
        await play([host, guest], 4_000);
        await sample();
        if (!shotTaken && performance.now() - at > rec.timeline.started * 1000 + 20_000) {
          shotTaken = true;
          rec.shots.push(await shot(host, `${tag}-1-play-seat1.png`));
          rec.shots.push(await shot(guest, `${tag}-1-play-seat2.png`));
        }
      }
    }
    // Time is called; the round ends with results (or void), and the results card comes up.
    await waitFor(guest, () => (window.__session?.state?.history?.length ?? 0) >= 1, null, 90_000);
    mark('ended');
    await sample();
    const after = await session(guest);
    const entry = after.state.history[0];
    rec.history = entry;
    rec.final_status = (await session(host)).status;
    log(`${tag}: round ${entry.status} (${entry.reason}): ${(entry.rows ?? []).map((r) => `${r.name} place ${r.place} +${r.points}${r.score != null ? ` score ${r.score}` : ''}${r.stats ? ` ${JSON.stringify(r.stats)}` : ''}${r.marks ? ` marks ${r.marks.map((m) => (m ? (m.won ? 'C' : 'x') : '-')).join('')}` : ''} win ${r.win}`).join(', ')}`);
    try {
      // The places land one by one: a moment for the last of them, before the card makes way for the rating.
      await waitFor(guest, () => window.__session?.card === 'results', null, 15_000);
      await sleep(1_800);
      rec.results_card = { seat1: await textOf(host, '#results'), seat2: await textOf(guest, '#results') };
      rec.shots.push(await shot(guest, `${tag}-2-results-seat2.png`));
      rec.shots.push(await shot(host, `${tag}-2-results-seat1.png`));
    } catch (e) {
      rec.results_card = { missed: String(e.message).split('\n')[0] };
    }
    // The rating, the guess and the reveal (short here), then the night's final standings.
    try {
      await waitFor(host, () => document.body.dataset.phase === 'ended', null, 60_000);
      mark('final');
      await sleep(1_000);
      rec.final = { seat1: await textOf(host, '.finale'), seat2: await textOf(guest, '.finale'), totals: (await session(host)).state.totals };
      rec.shots.push(await shot(host, `${tag}-3-final-seat1.png`));
    } catch (e) {
      rec.final = { missed: String(e.message).split('\n')[0] };
    }
    const moved = rec.samples.filter((s) => s.traffic.seat1 && s.traffic.seat2);
    const lastTraffic = moved.at(-1)?.traffic ?? null;
    rec.traffic = lastTraffic;
    rec.clock = rec.samples.filter((s) => s.phase === 'playing').at(-1)?.clock ?? null;
    rec.progress_heard = rec.samples.some((s) => Object.keys(s.progress.seat1 ?? {}).length > 1 || Object.keys(s.progress.seat2 ?? {}).length > 1);
    rec.ok = entry.status === 'played';
  } catch (e) {
    rec.failure = String(e?.stack || e).split('\n').slice(0, 3).join(' | ');
    log(`${tag}: FAILED ${rec.failure}`);
    for (const [seat, page] of Object.entries(pages)) {
      rec.shots.push(await shot(page, `${tag}-failure-seat${seat}.png`));
      rec[`at_failure_seat${seat}`] = await session(page).then((s) => ({ phase: s.state?.phase, status: s.status?.phase, players: s.status?.players?.map((p) => ({ key: p.key, hello: p.hello, ready: p.ready, out: p.out, result: p.result })), notice: s.state?.notice, card: s.card })).catch(() => null);
    }
  } finally {
    for (const ctx of contexts) await ctx.close().catch(() => {});
    rec.errors = counted(rec.errors);
    rec.warnings = counted(rec.warnings).slice(0, 40);
    fs.writeFileSync(path.join(OUT, 'hub_session.json'), JSON.stringify(report, null, 2));
  }
}

try {
  log(`library: ${entry.name} ${entry.path}, modes ${report.entry.modes.map((m) => `${m.key} ${m.seats.join('-')}`).join(', ')}`);
  for (const label of LOCAL_MODES) await night(label, 'local');
  for (const label of RELAY_MODES) await night(label, 'relay');
} finally {
  report.finished = new Date().toISOString();
  report.summary = report.rounds.map((r) => ({
    round: `${r.transport} ${r.label}`,
    ok: r.ok,
    dealt: r.dealt ? `${r.dealt.mode}${r.dealt.play ? ` ${r.dealt.play}` : ''}` : null,
    game_saw: r.game_saw ? `${r.game_saw.seat1?.mode}${r.game_saw.seat1?.play ? ` ${r.game_saw.seat1.play}` : ''} / ${r.game_saw.seat2?.mode}${r.game_saw.seat2?.play ? ` ${r.game_saw.seat2.play}` : ''}` : null,
    loaded: r.loaded?.map((p) => p.ready && !p.out),
    traffic: r.traffic,
    status: r.history ? `${r.history.status} (${r.history.reason})` : r.failure ?? null,
    team_won: r.history?.won ?? null,
    rows: r.history?.rows?.map((row) => ({ key: row.key, place: row.place, points: row.points, win: row.win, score: row.score ?? null, stats: row.stats ?? null, ...(row.marks ? { cleared: row.cleared, marks: row.marks.map((m) => (m ? (m.won ? 'C' : 'x') : '-')).join('') } : {}) })) ?? null,
    errors: r.errors.length,
    warnings: r.warnings.length,
  }));
  fs.writeFileSync(path.join(OUT, 'hub_session.json'), JSON.stringify(report, null, 2));
  await browser.close();
  server.close();
}
console.log(JSON.stringify(report.summary, null, 2));
