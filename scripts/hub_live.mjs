#!/usr/bin/env node
/**
 * hub_live.mjs — Shardfall inside 52in1's own hub page (its Wall, Settings, frame host and
 * companion, served locally from a 52in1 checkout as its settings-browser test serves them),
 * with a stand-in DualSense for WebHID recording every report the hub sends the controller.
 *
 * usage (from a folder where Node's `playwright` 1.63 is installed, `npm i playwright@1.63.0`
 * with PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1; the Python package's browsers serve it):
 *   node /path/to/shardfall/scripts/hub_live.mjs 52IN1_CHECKOUT OUT_DIR [--voice=FILE.wav]
 * The checkout: a sparse clone of michaelcrosato/52in1 (lib/, assets/, cloudflare/,
 * scripts/*.mjs, scripts/lib/, the root's *.html *.json *.js), with data/games.json holding only
 * Shardfall's entry from https://52in1.com/data/games.json, its path pointed at
 * /games/shardfall-v3.html, and games/shardfall-v3.html the page to test
 * (target/hub/pages/shardfall.html). docs/AI_HUB.md, "Testing", has the steps.
 *
 * --voice=FILE.wav: Chromium's fake microphone plays the recording (looped, with
 * --use-file-for-fake-audio-capture), so 52in1's openMicrophone() hands the game that voice
 * through Chrome's real capture and audio processing (echo cancellation, noise suppression,
 * gain control as the hub and the page ask). scripts/make_voice_wav.py makes one (talk with
 * five shouts) and writes FILE.json beside it with the shouts' times. Step 6 then plays on
 * until the stream has run the recording through once and more, and records the shouts the
 * game took (shardfallHub.shout() returning true: `shouts_detected` and their times), the
 * ones the page's ear heard (`shardfallHub.shoutsHeard`, taken or not: frames here are slow,
 * and a shout older than a second when the game looks is dropped), its background and
 * threshold, and how they line up with FILE.json's shouts (`voice.match`). The hero there is
 * the Wanderer, with no War Cry on the bar: what is measured is hearing, not roaring.
 *
 * What it does, as a player would:
 *  1. Settings → Controller: connect the DualSense, Motion aim on (the microphone not yet allowed).
 *  2. Opens Shardfall from the Wall; reads the frame's permissions and the AIHubSettings the
 *     game finds; sends the game into the Proving Grounds (?scene=arena) inside the same frame.
 *  3. Plays on the pad (stick nudges) while waves hit the hero: records the light bar colours
 *     and right-trigger modes the hub sends the DualSense, the rumbles asked of the pad.
 *  4. Turns the DualSense left (gyro) and reads the game's right stick: the hub adds the turn
 *     while the game doesn't read gyro itself, and leaves it alone once it does.
 *  5. Presses the touchpad (button 17): the map.
 *  6. Allows the microphone in Settings → Sound, opens the game again: the frame's permissions,
 *     and whether the game ever asks for the microphone or the controller's speaker (with
 *     --voice, what it heard).
 *  7. Leaves the game: the trigger let go and the light bar given back.
 * Writes OUT_DIR/hub_live.json and screenshots.
 */
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

// Playwright from the working folder (it is not a dependency of this repository).
const require = createRequire(path.join(process.cwd(), 'noop.js'));
const { chromium } = require('playwright');
const argv = process.argv.slice(2);
const opts = Object.fromEntries(argv.filter((a) => a.startsWith('--')).map((a) => {
  const [k, ...v] = a.slice(2).split('=');
  return [k, v.length ? v.join('=') : true];
}));
const [rootArg, outArg] = argv.filter((a) => !a.startsWith('--'));
const ROOT = path.resolve(rootArg);
const OUT = path.resolve(outArg ?? 'hub_live_out');
fs.mkdirSync(OUT, { recursive: true });
// --voice=FILE.wav: the fake microphone's recording, its length and (FILE.json) its shouts.
const VOICE = opts.voice ? path.resolve(String(opts.voice)) : null;
const voice = VOICE ? readVoice(VOICE) : null;
function readVoice(file) {
  const b = fs.readFileSync(file);
  if (b.toString('ascii', 0, 4) !== 'RIFF' || b.toString('ascii', 8, 12) !== 'WAVE') throw new Error(`--voice: ${file} is not a WAV file`);
  let at = 12, bytesPerSecond = 0, duration = 0;
  while (at + 8 <= b.length) {
    const id = b.toString('ascii', at, at + 4), n = b.readUInt32LE(at + 4);
    if (id === 'fmt ') bytesPerSecond = b.readUInt32LE(at + 16);
    if (id === 'data' && bytesPerSecond) duration = n / bytesPerSecond;
    at += 8 + n + (n & 1);
  }
  const side = file.replace(/\.wav$/i, '') + '.json';
  const info = fs.existsSync(side) ? JSON.parse(fs.readFileSync(side, 'utf8')) : {};
  return { file, duration: info.duration ?? duration, shouts: info.shouts ?? [] };
}
// The times heard (seconds into the stream) against the recording's shouts, looped: a time
// matches a shout when it falls from 0.1 s after its start to 0.8 s after its end (Chrome's
// processing and the ear's HOLD come between). `offset`: where in the recording the stream began.
function matchShouts(times, listened, offset = 0) {
  const { duration, shouts } = voice, hit = new Set();
  let stray = 0;
  for (const h of times) {
    const t = h + offset, loop = Math.floor(t / duration), p = t - loop * duration;
    const k = shouts.findIndex(([a, b]) => p >= a + 0.1 && p <= b + 0.8);
    if (k < 0) stray++;
    else hit.add(`${loop}:${k}`);
  }
  // The shouts the stream played in full while listening, and those of them not heard.
  let due = 0, missed = 0;
  for (let loop = 0; loop * duration < listened + offset; loop++) {
    shouts.forEach(([a, b], k) => {
      if (loop * duration + a >= offset && loop * duration + b + 0.8 <= listened + offset) {
        due++;
        if (!hit.has(`${loop}:${k}`)) missed++;
      }
    });
  }
  return { offset_s: offset, shouts_due: due, matched: hit.size, missed, stray };
}
const { staticHandler } = await import(path.join(ROOT, 'scripts/lib/serve.mjs'));
const server = createServer(staticHandler(ROOT, { companion: true }));
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
const t0 = performance.now();
const log = (s) => console.log(`${((performance.now() - t0) / 1000).toFixed(1).padStart(6)}s  ${s}`);

const browser = await chromium.launch({
  headless: true,
  args: [
    '--no-sandbox', '--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required',
    '--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-angle=swiftshader',
    ...(VOICE ? [`--use-file-for-fake-audio-capture=${VOICE}`] : []),
  ],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
// The stand-in DualSense for WebHID (as 52in1's settings-browser.mjs has it): reports sent, input reports to send.
await context.addInitScript(() => {
  window.__reports = [];
  const device = {
    vendorId: 0x054c, productId: 0x0ce6, productName: 'DualSense Wireless Controller', opened: false,
    collections: [{ outputReports: [{ reportId: 0x02 }] }], listeners: [],
    async open() { this.opened = true; }, async close() { this.opened = false; }, async forget() {},
    async sendReport(id, data) { window.__reports.push([id, [...data], performance.now()]); },
    async receiveFeatureReport() { return new DataView(new ArrayBuffer(1)); },
    addEventListener(type, fn) { if (type === 'inputreport') this.listeners.push(fn); },
  };
  window.__input = ({ gyro = [0, 0, 0], accel = [0, 8192, 0], fingers = [], mute = false } = {}) => {
    const data = new DataView(new Uint8Array(63).buffer);
    gyro.forEach((n, k) => data.setInt16(15 + 2 * k, n, true));
    accel.forEach((n, k) => data.setInt16(21 + 2 * k, n, true));
    for (const [k, at] of [32, 36].entries()) {
      const f = fingers[k];
      if (!f) { data.setUint8(at, 0x80); continue; }
      const [id, x, y] = f;
      data.setUint8(at, id); data.setUint8(at + 1, x & 0xff); data.setUint8(at + 2, ((x >> 8) & 0x0f) | ((y & 0x0f) << 4)); data.setUint8(at + 3, y >> 4);
    }
    data.setUint8(9, mute ? 0x04 : 0);
    data.setUint8(52, 0x08);
    for (const fn of device.listeners) fn({ reportId: 0x01, data });
  };
  if (window.top !== window) return;
  Object.defineProperty(Navigator.prototype, 'hid', {
    configurable: true,
    get: () => ({
      addEventListener() {},
      async requestDevice() { return [device]; },
      async getDevices() { return localStorage.getItem('ai-hub:dualsense')?.includes('"on":true') ? [device] : []; },
    }),
  });
});
// The game's frame: a DualSense in the standard mapping it can press (18 buttons: the touchpad is 17),
// with sticks, a rumble spy, and spies on the microphone.
await context.addInitScript(() => {
  if (window.top === window) return;
  const buttons = Array.from({ length: 18 }, () => ({ pressed: false, touched: false, value: 0 }));
  const axes = [0, 0, 0, 0];
  window.__rumbles = [];
  window.__micAsks = 0;
  const pad = {
    id: 'DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)', index: 0, connected: true, mapping: 'standard',
    get axes() { return axes.slice(); }, buttons, timestamp: 0,
    vibrationActuator: { type: 'dual-rumble', playEffect: (kind, p) => { window.__rumbles.push(p); return Promise.resolve('complete'); } },
  };
  let plugged = false;
  window.__pad = {
    set(down) {
      const fresh = !plugged; plugged = true;
      for (const [i, v] of Object.entries(down)) Object.assign(buttons[i], { pressed: !!v, touched: !!v, value: v ? 1 : 0 });
      pad.timestamp = performance.now();
      if (fresh) dispatchEvent(new Event('gamepadconnected'));
    },
    stick(i, v) { axes[i] = v; pad.timestamp = performance.now(); },
  };
  Object.defineProperty(Navigator.prototype, 'getGamepads', { configurable: true, value: () => [plugged ? pad : null, null, null, null] });
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const gum = md.getUserMedia.bind(md);
    md.getUserMedia = (c) => { window.__micAsks++; return gum(c); };
  }
  // The game's asks through 52in1 (the companion's own getUserMedia runs under openMicrophone,
  // so __micAsks counts it too), and when the stream came (__micStreamAt, performance.now()):
  // wrapped as soon as the companion has made AIHubSettings (a frame without it, a minute on,
  // stops looking).
  window.__openMic = 0; window.__speakerAsks = 0; window.__micStreamAt = null;
  const since = Date.now();
  const wrap = setInterval(() => {
    const h = window.AIHubSettings;
    if (!h) {
      if (Date.now() - since > 60_000) clearInterval(wrap);
      return;
    }
    clearInterval(wrap);
    if (h.openMicrophone) {
      const f = h.openMicrophone.bind(h);
      h.openMicrophone = () => {
        window.__openMic++;
        const p = f();
        Promise.resolve(p).then((st) => { if (st && window.__micStreamAt === null) window.__micStreamAt = performance.now(); }, () => {});
        return p;
      };
    }
    const d = h.dualsense;
    if (d && d.openSpeaker) { const f = d.openSpeaker.bind(d); d.openSpeaker = (c) => { window.__speakerAsks++; return f(c); }; }
  }, 2);
  // The shouts the game took: shardfallHub.shout() returning true, wrapped once the page made it.
  window.__shoutsTaken = [];
  const wrapShout = setInterval(() => {
    const b = window.shardfallHub;
    if (!b || !b.shout) {
      if (Date.now() - since > 60_000) clearInterval(wrapShout);
      return;
    }
    clearInterval(wrapShout);
    const f = b.shout;
    b.shout = (on) => {
      const heard = f(on);
      if (heard) window.__shoutsTaken.push(performance.now());
      return heard;
    };
  }, 2);
});

const result = { errors: [] };
const page = await context.newPage();
page.on('pageerror', (e) => result.errors.push(`hub: ${e.message}`));
page.on('console', (m) => { if (m.type() === 'error') result.errors.push(`console: ${m.text().slice(0, 200)}`); });

async function gameFrame() {
  const handle = await page.waitForSelector('.player__frame', { timeout: 60_000 });
  const frame = await handle.contentFrame();
  await frame.waitForLoadState('load');
  await frame.waitForFunction(() => window.AIHubSettings && window.AIHubSettings.music !== undefined, null, { timeout: 60_000 });
  return frame;
}
const allowed = () => page.getAttribute('.player__frame', 'allow').then((a) => (a || '').split('; ').filter(Boolean));
const hubSees = (g) => g.evaluate(() => {
  const h = window.AIHubSettings; const d = h.dualsense;
  return { music: h.music, effects: h.effects, vibration: h.vibration, mic: h.mic, micLater: h.micLater, connected: d.connected, lightbar: d.lightbar, triggers: d.triggers, motion: d.motion, touchpad: d.touchpad, speaker: d.speaker, aim: d.aim };
});
// Reports: the light bar (flag d[1] & 0x04, RGB at 44..46) and the right trigger (flag d[0] & 0x04, mode at 10).
const reportsSince = (n) => page.evaluate((k) => window.__reports.slice(k).map(([, d]) => d), n);
const hex = (d) => '#' + [d[44], d[45], d[46]].map((x) => x.toString(16).padStart(2, '0')).join('');

try {
  await page.goto(base + '/#/');
  await page.waitForSelector('.settings-button', { timeout: 60_000 });
  // 1. Settings → Controller: the DualSense, Motion aim on.
  await page.click('.settings-button');
  await page.click('.settings__row[data-page="controller"]');
  await page.getByRole('button', { name: /Connect a DualSense/ }).click();
  await page.waitForSelector('.settings__status', { timeout: 10_000 });
  await page.getByRole('button', { name: /^Motion aim/ }).click();
  result.controllerPage = (await page.textContent('.settings__body')).replace(/\s+/g, ' ').slice(0, 400);
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  log('settings: DualSense connected, Motion aim on');

  // 2. Shardfall from the Wall.
  await page.evaluate(() => (location.hash = '#/play/shardfall'));
  let game = await gameFrame();
  result.allow_before_mic = await allowed();
  result.settings_in_game = await hubSees(game);
  log(`game: frame allows [${result.allow_before_mic.join(', ')}]; AIHubSettings ${JSON.stringify(result.settings_in_game)}`);
  await page.waitForTimeout(8000);
  await page.screenshot({ path: path.join(OUT, '1-wall-game.png') });
  // Into the Proving Grounds, in the same frame (the companion comes with the page again).
  await game.evaluate(() => { location.search = '?scene=arena&seed=3'; });
  await page.waitForTimeout(3000);
  game = await gameFrame();
  log('game: the Proving Grounds');
  await game.evaluate(() => window.__pad.set({}));

  // 3. Play on the pad while the waves come: light bar, trigger, rumbles.
  const before = (await page.evaluate(() => window.__reports.length));
  for (let i = 0; i < 9; i++) {
    await game.evaluate(() => window.__pad.stick(0, 0.5));
    await page.waitForTimeout(1200);
    await game.evaluate(() => window.__pad.stick(0, 0));
    await page.waitForTimeout(6000);
  }
  await page.screenshot({ path: path.join(OUT, '2-fight.png') });
  const play = await reportsSince(before);
  result.lightbar_colours = [...new Set(play.filter((d) => d[1] & 0x04).map(hex))];
  result.right_trigger_modes = [...new Set(play.filter((d) => d[0] & 0x04).map((d) => '0x' + d[10].toString(16)))];
  result.rumbles = await game.evaluate(() => window.__rumbles.length);
  log(`play: light bar ${result.lightbar_colours.join(' ')}; right trigger modes ${result.right_trigger_modes.join(' ')}; rumbles ${result.rumbles}`);

  // 4. Motion: turn the DualSense left at 60°/s and read the game's right stick.
  await page.evaluate(() => (window.__turning = setInterval(() => window.__input({ gyro: [0, 984, 0] }), 8)));
  // The stick as the game would read it, polled for 10 s (frames are slow here): the furthest left it went.
  result.right_stick_while_turning = await game.evaluate(async () => {
    let min = 0, last = null;
    for (let i = 0; i < 100; i++) {
      const a = navigator.getGamepads()[0]?.axes ?? [];
      min = Math.min(min, a[2] ?? 0); last = [a[2], a[3]];
      await new Promise((r) => setTimeout(r, 100));
    }
    return { min_x: min, last };
  });
  result.game_reads_gyro = await game.evaluate(() => {
    const d = window.AIHubSettings.dualsense; return d.gyro ? JSON.parse(JSON.stringify(d.gyro)) : null;
  });
  await page.evaluate(() => clearInterval(window.__turning));
  for (let i = 0; i < 8; i++) { await page.evaluate(() => window.__input()); await page.waitForTimeout(20); }
  await page.screenshot({ path: path.join(OUT, '3-turned.png') });
  log(`motion: right stick while turning ${JSON.stringify(result.right_stick_while_turning)} (the hub adds the turn unless the game reads gyro itself)`);

  // 5. The touchpad's press: the map.
  await game.evaluate(() => window.__pad.set({ 17: 1 }));
  await page.waitForTimeout(2500);
  await game.evaluate(() => window.__pad.set({ 17: 0 }));
  await page.waitForTimeout(2500);
  await page.screenshot({ path: path.join(OUT, '4-touchpad-map.png') });
  result.mic_asks_before_allowed = await game.evaluate(() => ({ getUserMedia: window.__micAsks, openMicrophone: window.__openMic }));

  // 7a. Leaving: trigger let go, light given back.
  const kept = await page.evaluate(() => window.__reports.length);
  await page.evaluate(() => (location.hash = '#/'));
  await page.waitForSelector('.player__frame', { state: 'detached', timeout: 20_000 });
  await page.waitForTimeout(1500);
  const after = await reportsSince(kept);
  result.on_leave = {
    right_trigger_modes: [...new Set(after.filter((d) => d[0] & 0x04).map((d) => '0x' + d[10].toString(16)))],
    lightbar: [...new Set(after.filter((d) => d[1] & 0x04).map(hex))],
  };
  log(`left: trigger modes ${result.on_leave.right_trigger_modes.join(' ')}, light bar ${result.on_leave.lightbar.join(' ')}`);

  // 6. The microphone allowed in Settings → Sound; the game again.
  await context.grantPermissions(['microphone'], { origin: base });
  await page.click('.settings-button');
  await page.click('.settings__row[data-page="sound"]');
  await page.getByRole('button', { name: /^Games can hear the microphone/ }).click();
  await page.waitForFunction(() => localStorage.getItem('ai-hub:mic') === 'true', null, { timeout: 5_000 });
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.evaluate(() => (location.hash = '#/play/shardfall'));
  game = await gameFrame();
  result.allow_after_mic = await allowed();
  result.settings_after_mic = await hubSees(game);
  await game.evaluate(() => { location.search = '?scene=arena&seed=3'; });
  await page.waitForTimeout(3000);
  game = await gameFrame();
  await game.evaluate(() => window.__pad.set({}));
  const started = Date.now();
  for (let i = 0; ; i++) {
    if (i >= 6) {
      // With --voice, on until the stream has played the recording through and 4 s more (at most 3 min).
      if (!voice) break;
      const heardFor = await game.evaluate(() => (window.__micStreamAt === null ? -1 : (performance.now() - window.__micStreamAt) / 1000));
      if (heardFor >= voice.duration + 4 || Date.now() - started > 180_000) break;
    }
    await game.evaluate(() => window.__pad.stick(0, -0.5));
    await page.waitForTimeout(1000);
    await game.evaluate(() => { window.__pad.stick(0, 0); window.__pad.set({ 12: 1 }); });
    await page.waitForTimeout(1500);
    await game.evaluate(() => window.__pad.set({ 12: 0 }));
    await page.waitForTimeout(4000);
  }
  result.after_mic_allowed = await game.evaluate(() => ({ micAsks: window.__micAsks, openMicrophone: window.__openMic, openSpeaker: window.__speakerAsks }));
  await page.screenshot({ path: path.join(OUT, '5-mic-allowed.png') });
  log(`mic allowed: frame allows [${result.allow_after_mic.join(', ')}]; the game's asks ${JSON.stringify(result.after_mic_allowed)}`);
  if (voice) {
    // What the game took and what the page's ear heard, in seconds since the stream came (the
    // ear's own times count from when it began listening, a moment later).
    const v = await game.evaluate(() => ({
      streamAt: window.__micStreamAt, now: performance.now(), taken: window.__shoutsTaken.slice(),
      heard: Array.isArray(window.shardfallHub.shoutsHeard) ? window.shardfallHub.shoutsHeard.slice() : null,
      hearing: typeof window.shardfallHub.hearing === 'function' ? window.shardfallHub.hearing() : null,
    }));
    const since = (t) => Math.round(t - v.streamAt) / 1000;
    const listened = v.streamAt === null ? 0 : since(v.now);
    const heard = v.heard ?? v.taken.map(since);
    // Where in the recording the stream began: 0 if the fake microphone started it afresh;
    // the best of every 50 ms otherwise, for comparison.
    let match = null, best = null;
    if (voice.shouts.length) {
      match = best = matchShouts(heard, listened, 0);
      for (let o = 0.05; o < voice.duration; o += 0.05) {
        const m = matchShouts(heard, listened, Math.round(o * 100) / 100);
        if (m.matched - m.stray > best.matched - best.stray) best = m;
      }
    }
    result.voice = {
      file: voice.file, duration_s: voice.duration, shouts_in_file: voice.shouts, listened_s: listened,
      shouts_detected: v.taken.length, shout_times_s: v.taken.map(since),
      page_heard_s: v.heard, hearing: v.hearing, match, match_best_offset: best,
    };
    log(`voice: ${listened.toFixed(1)} s heard; the game took ${v.taken.length} shouts at ${result.voice.shout_times_s.join(' ')} s; `
      + `the page heard ${heard.length} at ${heard.join(' ')} s; match ${JSON.stringify(result.voice.match)}`);
  }
} catch (e) {
  result.failure = String(e && e.stack || e);
  log('FAILED: ' + result.failure.split('\n')[0]);
  await page.screenshot({ path: path.join(OUT, 'failure.png') }).catch(() => {});
} finally {
  fs.writeFileSync(path.join(OUT, 'hub_live.json'), JSON.stringify(result, null, 2));
  await browser.close();
  server.close();
}
console.log(JSON.stringify(result, null, 2));
