#!/usr/bin/env node
/**
 * shout_check.mjs — Shout to roar's detector (web/index.html, the `shout-detector` script block,
 * `window.shoutDetector`) run in Node over made-up sounds, as the page feeds it: 48 kHz samples
 * in looks every 30 ms (or slow frames' longer ones), 20 ms blocks, the page's analyser size.
 *
 * usage:
 *   node scripts/shout_check.mjs                  # every case; exits 1 if one fails
 *   node scripts/shout_check.mjs --wav=FILE.wav   # the detector over a recording, once and looped:
 *                                                 # its shouts against FILE.json's (make_voice_wav.py)
 *   node scripts/shout_check.mjs --sweep=N        # N voices at each talk level (--levels=0.05,...):
 *                                                 # shouts heard in talk per minute, 4x shouts missed
 *   --verbose                                     # each case's times, background and threshold
 *   --trace=TEXT                                  # block by block, the cases whose name holds TEXT
 *   --stats                                       # talk's and shouts' 20 ms levels over their RMS
 *
 * The cases: (a) silence then a shout; (b) talk at 0.02, 0.05 and 0.1 RMS, with pauses of up to
 * 20 s, never counts (straight out of silence, once at most); (c) shouts 4x over each talk level
 * count once each, and shouts closer than REST once; (d) a quiet microphone (all of it x0.1)
 * still hears them; (e) a steady game sound (a music bed at 0.04-0.2 RMS, from the start,
 * swelling, or under talk) doesn't count, and shouts over it do (one starting out of silence
 * counts once at most); (f) a clap (60 ms) doesn't count; (g) nothing counts while muted, nor is
 * learned; (h) slow frames (looks up to the analyser's 0.68 s) hear the same shouts; (i)
 * scripts/pad_check.py's stand-in microphone (a tone burst every 4 s) is a shout a burst.
 * The voices: a glottal pulse train through three formant resonators, in syllables (talk at
 * ~140 Hz, some drawn out and stressed, the odd "umm") or one held vowel (a shout: higher,
 * brighter), from a seeded generator. The sounds and `listen` are exported for tuning scripts
 * (`listen(x, { tune: { RATIO: 2.4 } })` overrides the detector's constants).
 */
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RATE = 48000;
const ANALYSER = 32768; // the page's AnalyserNode fftSize: what one look can read
const args = Object.fromEntries(process.argv.slice(2).map((a) => {
  const [k, v] = a.replace(/^--/, '').split('=');
  return [k, v ?? true];
}));

// The detector, from the page.
const html = fs.readFileSync(path.join(ROOT, 'web/index.html'), 'utf8');
const block = html.match(/<script id="shout-detector">([\s\S]*?)<\/script>/);
if (!block) {
  console.error('web/index.html has no <script id="shout-detector"> block');
  process.exit(2);
}
const sandbox = { window: {}, Math, Float64Array, Uint8Array };
vm.runInNewContext(block[1], sandbox);
const shoutDetector = sandbox.window.shoutDetector;

// ---- Sounds ----------------------------------------------------------------------------------

function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const gauss = (r) => Math.sqrt(-2 * Math.log(1 - r())) * Math.cos(2 * Math.PI * r());
const rms = (x, a = 0, b = x.length) => {
  let s = 0;
  for (let i = a; i < b; i++) s += x[i] * x[i];
  return Math.sqrt(s / Math.max(1, b - a));
};
const scaled = (x, level) => {
  const k = level / (rms(x) || 1);
  return x.map((v) => v * k);
};

// Vowels' first three formants (Hz).
const VOWELS = [[730, 1090, 2440], [530, 1840, 2480], [270, 2290, 3010], [570, 840, 2410], [300, 870, 2240], [660, 1720, 2410]];

// A resonator (two poles, unity gain at 0 Hz) per formant, in cascade.
function formants(freqs, bws) {
  return freqs.map((f, k) => {
    const r = Math.exp(-Math.PI * bws[k] / RATE), c = 2 * r * Math.cos(2 * Math.PI * f / RATE), d = -r * r;
    return { c, d, g: 1 - c - d, y1: 0, y2: 0 };
  });
}
function through(fs, x) {
  for (const f of fs) {
    const y = f.g * x + f.c * f.y1 + f.d * f.y2;
    f.y2 = f.y1;
    f.y1 = y;
    x = y;
  }
  return x;
}
// The glottal flow (Rosenberg), open for `open` of the period; its derivative is the source.
const glottal = (ph, open) => {
  const rise = open * 0.7, fall = open * 0.3;
  if (ph < rise) return 0.5 * (1 - Math.cos(Math.PI * ph / rise));
  if (ph < open) return Math.cos(0.5 * Math.PI * (ph - rise) / fall);
  return 0;
};
// A raised-cosine envelope: attack a, release b, over n samples.
const envelope = (i, n, a, b) => (i < a ? 0.5 - 0.5 * Math.cos(Math.PI * i / a) : i > n - b ? 0.5 - 0.5 * Math.cos(Math.PI * (n - i) / b) : 1);

// Voiced sound: `parts` are { n, f0: [from, to], amp, vowel, open, attack, release, hiss }; each
// part's RMS is its `amp` (silence where 0).
function voice(parts, r) {
  const total = parts.reduce((s, p) => s + p.n, 0), out = new Float32Array(total);
  let ph = 0, prev = 0, at = 0, f0s = parts[0]?.f0?.[0] ?? 140;
  for (const p of parts) {
    if (!p.amp) {
      at += p.n;
      continue;
    }
    const fs = formants(p.vowel, [90, 110, 170]);
    for (let i = 0; i < p.n; i++) {
      const want = p.f0[0] + (p.f0[1] - p.f0[0]) * (i / p.n);
      f0s += (want - f0s) * 0.002;
      ph += (f0s * (1 + 0.004 * Math.sin(2 * Math.PI * 5.5 * (at + i) / RATE))) / RATE;
      if (ph >= 1) ph -= 1;
      const g = glottal(ph, p.open);
      const src = (g - prev) * 40 + p.hiss * gauss(r);
      prev = g;
      out[at + i] = through(fs, src) * envelope(i, p.n, p.attack, p.release);
    }
    const k = p.amp / (rms(out, at, at + p.n) || 1);
    for (let i = at; i < at + p.n; i++) out[i] *= k;
    at += p.n;
  }
  return out;
}
const sec = (s) => Math.round(s * RATE);
// Talk: words of 1-3 syllables, short gaps, a pause every few words; ~140 Hz with a fall.
function talk(seconds, seed, level) {
  const r = rng(seed), parts = [];
  let t = 0, words = 0, phrase = 4 + Math.floor(r() * 5);
  while (t < seconds) {
    const syll = 1 + Math.floor(r() * 3);
    for (let k = 0; k < syll; k++) {
      // Most syllables are short; one in eight is a drawn-out, stressed vowel, a few an "umm".
      const kind = r(), drawn = kind < 0.12, um = kind > 0.97;
      const n = sec(um ? 0.4 + r() * 0.3 : drawn ? 0.3 + r() * 0.15 : 0.12 + r() * 0.16);
      const stress = drawn || (!um && k === 0 && r() < 0.5);
      const base = 150 - 25 * (words % phrase) / phrase + (stress ? 15 : 0) + 8 * gauss(r);
      if (r() < 0.4) parts.push({ n: sec(0.03 + r() * 0.05), f0: [base, base], amp: 0.12, vowel: [2500, 3500, 4500], open: 0.6, attack: sec(0.01), release: sec(0.01), hiss: 3 });
      parts.push({ n, f0: [base, base - 6], amp: Math.pow(10, (2.5 * gauss(r) + (stress ? 3 : 0) - (um ? 6 : 0)) / 20), vowel: um ? [500, 1000, 2400] : VOWELS[Math.floor(r() * VOWELS.length)], open: 0.6, attack: sec(0.03), release: sec(0.05), hiss: 0.02 });
      parts.push({ n: sec(0.02 + r() * 0.05), amp: 0 });
    }
    parts.push({ n: sec(0.06 + r() * 0.12), amp: 0 });
    if (++words % phrase === 0) {
      parts.push({ n: sec(0.3 + r() * 0.4), amp: 0 });
      phrase = 4 + Math.floor(r() * 5);
    }
    t = parts.reduce((s, p) => s + p.n, 0) / RATE;
  }
  return fit(scaled(voice(parts, r), level), sec(seconds));
}
// A shout: one held, open vowel, higher and brighter than talk, `seconds` long.
function shout(seconds, seed, level) {
  const r = rng(seed), f = 220 + 40 * r();
  const parts = [
    { n: sec(0.04), f0: [f, f], amp: 0.2, vowel: [2000, 3000, 4000], open: 0.45, attack: sec(0.01), release: sec(0.01), hiss: 3 },
    { n: sec(seconds - 0.04), f0: [f, f + 10], amp: 1, vowel: [850, 1250, 2700], open: 0.45, attack: sec(0.05), release: sec(0.12), hiss: 0.03 },
  ];
  return scaled(voice(parts, r), level);
}
const fit = (x, n) => (x.length === n ? x : Float32Array.from({ length: n }, (_, i) => (i < x.length ? x[i] : 0)));
function noise(seconds, level, seed = 7) {
  const r = rng(seed);
  return Float32Array.from({ length: sec(seconds) }, () => level * gauss(r));
}
// A music bed: a slow chord with a little tremolo and a soft beat every half second.
function music(seconds, level, seed = 11) {
  const r = rng(seed), n = sec(seconds), out = new Float32Array(n), notes = [110, 164.8, 220, 277.2, 329.6];
  for (let i = 0; i < n; i++) {
    const t = i / RATE, beat = t % 0.5;
    let v = 0;
    for (const [k, f] of notes.entries()) v += Math.sin(2 * Math.PI * f * t + k) * (1 + 0.2 * Math.sin(2 * Math.PI * (0.3 + 0.1 * k) * t));
    v += beat < 0.08 ? 2.5 * gauss(r) * (1 - beat / 0.08) : 0;
    out[i] = v;
  }
  return scaled(out, level);
}
// A clap: 60 ms of noise, falling fast.
function clap(level, seed = 5) {
  const r = rng(seed), n = sec(0.06);
  return scaled(Float32Array.from({ length: n }, (_, i) => gauss(r) * Math.exp(-i / (n / 4))), level);
}

// A timeline: pieces laid end to end (or `at` a time over what is there), a floor of room
// noise under all of it. Returns the samples and the shouts' [start, end] times.
function scene(pieces, floor = 0.0005) {
  let end = 0;
  for (const p of pieces) end = Math.max(end, (p.at ?? end) + p.x.length / RATE);
  const out = noise(end, floor, 3), shouts = [];
  let t = 0;
  for (const p of pieces) {
    const at = p.at ?? t, i0 = sec(at);
    for (let i = 0; i < p.x.length && i0 + i < out.length; i++) out[i0 + i] += p.x[i] * (p.fade ? Math.min(1, i / sec(p.fade)) : 1);
    if (p.shout) shouts.push([at, at + p.x.length / RATE]);
    if (p.at === undefined) t = at + p.x.length / RATE;
  }
  return { x: out, shouts };
}
const gap = (s) => ({ x: new Float32Array(sec(s)) });
const louder = (sc, k) => ({ x: sc.x.map((v) => v * k), shouts: sc.shouts });

// ---- Listening as the page does ------------------------------------------------------------------

// Looks every `look()` seconds (30 ms ticks, or slow frames); a look longer than the analyser
// holds reads only its last ANALYSER samples, as the page does. `muted(t)`: the mute button.
function listen(x, { look = () => 0.03, muted = () => false, tune } = {}) {
  const d = shoutDetector(RATE, tune), heard = [];
  let at = 0;
  while (at < x.length) {
    let fresh = Math.min(x.length - at, Math.max(1, Math.round(look() * RATE)));
    at += fresh;
    let from = at - fresh;
    if (fresh > ANALYSER) {
      from = at - ANALYSER;
      d.gap();
    }
    const t = d.feed(x, from, at, at / RATE, muted(at / RATE));
    if (t !== null) heard.push(t);
  }
  return { heard, bg: d.background, thr: d.threshold };
}

// ---- The cases --------------------------------------------------------------------------------

const results = [];
// By default the scene's shouts are each heard exactly once and nothing else is (`once`: spans
// heard once each instead, for shouts closer than REST); `quiet`: nothing heard; `max`: at most
// this many heard in all. A shout is heard from 0.1 s after its start to 0.25 s after its end.
function check(name, sc, { look, muted, max, once, quiet = false } = {}) {
  if (args.trace && name.includes(args.trace)) {
    const d = shoutDetector(RATE);
    for (let i = 0; i + 960 <= sc.x.length; i += 960) {
      const thr = d.threshold, bg = d.background, r = rms(sc.x, i, i + 960);
      const t = d.feed(sc.x, i, i + 960, (i + 960) / RATE, false);
      if (r >= thr || t !== null || i % (960 * 25) === 0) console.log(`  ${((i + 960) / RATE).toFixed(2)} rms ${r.toFixed(4)} bg ${bg.toFixed(4)} thr ${thr.toFixed(4)}${r >= thr ? ' LOUD' : ''}${t !== null ? ' SHOUT' : ''}`);
    }
  }
  const { heard, bg, thr } = listen(sc.x, { look, muted });
  const spans = once ?? sc.shouts;
  const inSpan = (t, [a, b]) => t >= a + 0.1 && t <= b + 0.25;
  const per = spans.map((s) => heard.filter((t) => inSpan(t, s)).length);
  const stray = heard.filter((t) => !spans.some((s) => inSpan(t, s)));
  let ok;
  let what;
  if (max !== undefined) {
    ok = heard.length <= max;
    what = `${heard.length} heard (at most ${max})`;
  } else if (quiet) {
    ok = heard.length === 0;
    what = `${heard.length} heard (none expected)`;
  } else {
    ok = per.every((n) => n === 1) && stray.length === 0;
    what = `${per.filter((n) => n === 1).length}/${spans.length} shouts heard once, ${stray.length} stray`;
  }
  results.push({ name, ok });
  console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}: ${what}`);
  if (args.verbose || !ok) {
    console.log(`        heard at ${heard.map((t) => t.toFixed(2)).join(' ') || '-'}; shouts ${spans.map(([a, b]) => `${a.toFixed(1)}-${b.toFixed(1)}`).join(' ') || '-'}; background ${bg.toFixed(4)}, threshold ${thr.toFixed(4)} RMS at the end`);
  }
}
// Talk with shouts `k` times its level every ~7 s.
function talkWithShouts(level, k, seed, n = 5) {
  const pieces = [];
  for (let i = 0; i < n; i++) {
    pieces.push({ x: talk(5 + (i % 3), seed + i, level) });
    pieces.push({ x: shout(0.6 + 0.075 * (i % 5), seed + 100 + i, k * level), shout: true });
  }
  pieces.push({ x: talk(4, seed + 50, level) });
  return scene(pieces);
}

function runCases() {
  // (a) Silence, then a shout.
  check('(a) silence, then a shout (0.3 RMS)', scene([gap(3), { x: shout(0.8, 1, 0.3), shout: true }, gap(3)]));
  check('(a) silence, then a softer shout (0.05 RMS)', scene([gap(3), { x: shout(0.7, 2, 0.05), shout: true }, gap(3)]));
  // (b) Talk never counts.
  for (const level of [0.02, 0.05, 0.1]) {
    check(`(b) talk at ${level} RMS for 40 s`, scene([{ x: talk(40, 10, level) }]), { quiet: true });
  }
  for (const level of [0.02, 0.05, 0.1]) {
    check(`(b) talk at ${level} RMS with pauses of 3, 8 and 20 s`, scene([
      { x: talk(8, 20, level) }, gap(3), { x: talk(8, 21, level) }, gap(8), { x: talk(8, 22, level) }, gap(20), { x: talk(8, 23, level) },
    ]), { quiet: true });
  }
  check('(b) talk at 0.05 RMS after a quiet start counts at most once', scene([gap(3), { x: talk(20, 24, 0.05) }]), { max: 1 });
  // (c) Shouts 4x over talk count once each; closer than REST, once.
  for (const level of [0.02, 0.05, 0.1]) {
    check(`(c) talk at ${level} RMS, five shouts at ${(4 * level).toFixed(2)}`, talkWithShouts(level, 4, 30 + level * 1000));
  }
  {
    const sc = scene([
      { x: talk(6, 40, 0.05) },
      { x: shout(0.6, 41, 0.2), shout: true }, { x: talk(0.4, 42, 0.05) }, { x: shout(0.6, 43, 0.2), shout: true },
      { x: talk(3, 44, 0.05) }, { x: shout(0.6, 45, 0.2), shout: true }, { x: talk(3, 46, 0.05) },
    ]);
    const [a, b, c] = sc.shouts;
    check('(c) two shouts 1 s apart count once, the next 3.6 s later again', sc, { once: [[a[0], b[1]], c] });
  }
  // (d) A quiet microphone: all of it x0.1.
  check('(d) quiet mic: silence, then a shout (0.3 x0.1)', louder(scene([gap(3), { x: shout(0.8, 1, 0.3), shout: true }, gap(3)]), 0.1));
  for (const level of [0.05, 0.1]) {
    check(`(d) quiet mic: talk at ${level} x0.1, shouts 4x`, louder(talkWithShouts(level, 4, 60 + level * 1000), 0.1));
  }
  check('(d) quiet mic: talk at 0.05 x0.1 for 40 s', louder(scene([{ x: talk(40, 10, 0.05) }]), 0.1), { quiet: true });
  // (e) A steady game sound: a music bed.
  for (const level of [0.08, 0.2]) {
    check(`(e) music bed at ${level} RMS from the start, 30 s`, scene([{ x: music(30, level) }]), { quiet: true });
    // Out of silence, a sound's first moments are a shout's: once at most.
    check(`(e) music bed at ${level} RMS faded in over 3 s after 2 s of quiet counts at most once`,
      scene([gap(2), { x: music(30, level), fade: 3 }]), { max: 1 });
  }
  {
    // The bed at 0.04 RMS for 10 s, then twice as loud over 2 s (the swell rides on it).
    const bed = music(30, 0.04), swell = Float32Array.from(bed, (v, i) => v * (i < sec(10) ? 1 : Math.min(2, 1 + (i - sec(10)) / sec(2))));
    check('(e) music bed at 0.04 RMS swelling to 0.08 over 2 s', scene([{ x: swell }]), { quiet: true });
  }
  check('(e) music bed at 0.08 RMS with talk at 0.05 over it', scene([{ x: music(30, 0.08) }, { at: 2, x: talk(26, 70, 0.05) }]), { quiet: true });
  {
    const pieces = [{ x: music(36, 0.08) }];
    for (let i = 0; i < 5; i++) pieces.push({ at: 4 + 7 * i, x: shout(0.7, 80 + i, 0.32), shout: true });
    check('(e) music bed at 0.08 RMS, five shouts at 0.32 over it', scene(pieces));
  }
  check('(e) music bed at 0.2 RMS starting at once after 3 s of quiet counts at most once', scene([gap(3), { x: music(20, 0.2) }]), { max: 1 });
  // (f) Claps.
  {
    const pieces = [gap(1)];
    for (let i = 0; i < 6; i++) pieces.push({ x: clap(0.5, 90 + i) }, gap(2));
    check('(f) claps (60 ms at 0.5 RMS) over silence', scene(pieces), { quiet: true });
    const over = [{ x: talk(20, 95, 0.05) }];
    for (let i = 0; i < 6; i++) over.push({ at: 2 + 3 * i, x: clap(0.5, 96 + i) });
    check('(f) claps over talk at 0.05', scene(over), { quiet: true });
  }
  // (g) Muted: nothing counts, nothing is learned (the shouts while muted; after, talk and a shout).
  {
    const sc = scene([
      { x: talk(5, 100, 0.05) }, { x: shout(0.8, 101, 0.2) }, { x: talk(3, 102, 0.05) }, { x: shout(0.8, 103, 0.2) },
      gap(10), { x: talk(6, 104, 0.05) }, { x: shout(0.8, 105, 0.2), shout: true }, { x: talk(3, 106, 0.05) },
    ]);
    check('(g) muted from 4 s to 24 s: its shouts (and the quiet) not heard, the one after is', sc, { muted: (t) => t >= 4 && t < 24 });
  }
  // (h) Slow frames: looks of 30 ms to 0.65 s (the analyser holds 0.68 s), and of a second.
  {
    const sc = talkWithShouts(0.05, 4, 110);
    const r = rng(111);
    check('(h) slow frames: looks of 0.03-0.65 s hear every shout', sc, { look: () => 0.03 + 0.62 * r() });
    const r2 = rng(112);
    check('(h) frames of a second (0.32 s of each lost): no stray shout', sc, { look: () => (r2() < 0.3 ? 1 : 0.03), max: sc.shouts.length });
  }
  // (i) pad_check.py's stand-in microphone: a 220 Hz tone at 0.8 for 0.5 s every 4 s from 2 s
  // on, silence (zeros) between: each burst a shout.
  {
    const x = new Float32Array(sec(30)), shouts = [];
    for (let at = 2; at + 0.5 < 30; at += 4) {
      for (let i = sec(at); i < sec(at + 0.5); i++) x[i] = 0.8 * Math.sin(2 * Math.PI * 220 * i / RATE);
      shouts.push([at, at + 0.5]);
    }
    check("(i) pad_check.py's stand-in: a tone burst every 4 s, silence between", { x, shouts });
  }
}

// ---- A recording --------------------------------------------------------------------------------

function readWav(file) {
  const b = fs.readFileSync(file);
  if (b.toString('ascii', 0, 4) !== 'RIFF' || b.toString('ascii', 8, 12) !== 'WAVE') throw new Error(`${file} is not a WAV file`);
  let at = 12, fmt = null;
  while (at + 8 <= b.length) {
    const id = b.toString('ascii', at, at + 4), n = b.readUInt32LE(at + 4), body = at + 8;
    if (id === 'fmt ') fmt = { channels: b.readUInt16LE(body + 2), rate: b.readUInt32LE(body + 4), bits: b.readUInt16LE(body + 14) };
    if (id === 'data') {
      if (!fmt || fmt.bits !== 16) throw new Error(`${file}: only 16-bit PCM is read`);
      const frames = Math.floor(n / (2 * fmt.channels)), x = new Float32Array(frames);
      for (let i = 0; i < frames; i++) x[i] = b.readInt16LE(body + 2 * i * fmt.channels) / 32768;
      return { x, rate: fmt.rate };
    }
    at = body + n + (n & 1);
  }
  throw new Error(`${file} has no data chunk`);
}

function runWav(file) {
  const { x, rate } = readWav(file);
  if (rate !== RATE) throw new Error(`${file} is ${rate} Hz; this check listens at ${RATE}`);
  const side = file.replace(/\.wav$/i, '') + '.json';
  const shouts = fs.existsSync(side) ? JSON.parse(fs.readFileSync(side, 'utf8')).shouts : [];
  console.log(`${file}: ${(x.length / RATE).toFixed(1)} s, ${shouts.length ? `${shouts.length} shouts in ${path.basename(side)}` : `no ${path.basename(side)}`}`);
  check(`${path.basename(file)}: every shout heard once, nothing else`, { x, shouts: shouts.map((s) => [s[0], s[1]]) },
    shouts.length ? {} : { max: Infinity });
  // A browser's fake microphone plays it round and round: twice over, end into start.
  const d = x.length / RATE, twice = new Float32Array(2 * x.length);
  twice.set(x);
  twice.set(x, x.length);
  check(`${path.basename(file)} twice over (as a browser loops it)`, { x: twice, shouts: [...shouts, ...shouts.map(([a, b]) => [a + d, b + d])] },
    shouts.length ? {} : { max: Infinity });
  const { heard } = listen(twice);
  console.log(`        heard at ${heard.map((t) => t.toFixed(2)).join(' ') || '-'} (twice over)`);
}

// Many voices (--sweep=N): talk alone (b) and talk with shouts 4x over it (c), seed after seed,
// at each level. Talk's loudest, drawn-out syllables come close to a shout, so this is a rate:
// it passes at no more than one shout heard in talk per 5 minutes of it and 2% of shouts missed.
function sweep(n) {
  for (const level of args.levels ? String(args.levels).split(',').map(Number) : [0.02, 0.05, 0.1]) {
    let falseShouts = 0, missed = 0, shouts = 0, seconds = 0;
    for (let seed = 1000; seed < 1000 + n; seed++) {
      const alone = listen(scene([{ x: talk(30, seed, level) }]).x).heard;
      const sc = talkWithShouts(level, 4, seed * 7);
      const { heard } = listen(sc.x);
      const inSpan = (t, [a, b]) => t >= a + 0.1 && t <= b + 0.25;
      const lost = sc.shouts.filter((s) => !heard.some((t) => inSpan(t, s))), wrong = heard.filter((t) => !sc.shouts.some((s) => inSpan(t, s)));
      shouts += sc.shouts.length;
      missed += lost.length;
      falseShouts += alone.length + wrong.length;
      seconds += 30 + sc.x.length / RATE - sc.shouts.reduce((s, [a, b]) => s + b - a, 0);
      if (args.verbose && (alone.length || lost.length || wrong.length)) {
        console.log(`        seed ${seed}: talk alone heard at ${alone.map((t) => t.toFixed(2)).join(' ') || '-'}; with shouts missed ${lost.map(([a]) => a.toFixed(1)).join(' ') || '-'}, stray ${wrong.map((t) => t.toFixed(2)).join(' ') || '-'}`);
      }
    }
    const minutes = seconds / 60, ok = falseShouts <= minutes / 5 && missed <= 0.02 * shouts;
    results.push({ name: `sweep at ${level}`, ok });
    console.log(`${ok ? 'ok  ' : 'FAIL'}  sweep, talk at ${level} RMS, ${n} voices: ${falseShouts} shouts heard in ${minutes.toFixed(1)} min of talk (at most ${Math.floor(minutes / 5)}); ${missed}/${shouts} shouts at 4x missed`);
  }
}

// The 20 ms levels of talk and of shouts over their own RMS: what RATIO and HOLD tell apart.
function stats() {
  const blocks = (x) => { const out = []; for (let i = 0; i + 960 <= x.length; i += 960) out.push(rms(x, i, i + 960)); return out; };
  const q = (a, p) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
  for (const seed of [10, 20, 30, 77]) {
    const b = blocks(talk(40, seed, 1));
    const run = (k) => { let best = 0, cur = 0; for (const v of b) { cur = v >= k ? cur + 1 : 0; best = Math.max(best, cur); } return best; };
    console.log('talk', seed, [0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 0.95, 0.99, 1].map((p) => p + ':' + q(b, p).toFixed(2)).join(' '));
    console.log('   longest run above 1.5x:', run(1.5), ' 2x:', run(2), ' 2.5x:', run(2.5), ' 3x:', run(3));
  }
  for (const seed of [1, 2, 3]) console.log('shout', seed, blocks(shout(0.7, seed, 1)).map((v) => v.toFixed(2)).join(' '));
}
export { RATE, talk, shout, scene, gap, talkWithShouts, music, clap, louder, listen, sec };
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (args.stats) stats();
  else if (args.sweep) sweep(Number(args.sweep) || 20);
  else if (args.wav) runWav(String(args.wav));
  else runCases();
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} cases pass${failed.length ? `; failed: ${failed.map((r) => r.name).join('; ')}` : ''}`);
  process.exit(failed.length ? 1 : 0);
}
