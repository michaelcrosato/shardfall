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
 *   node scripts/shout_check.mjs --sweep=N        # N voices at each talk level (--levels=0.05,...;
 *                                                 # --quiet-mic: x0.1; --shout=K: shouts Kx, 4 by default):
 *                                                 # shouts heard in talk and on-and-off talk, shouts missed
 *   --only=TEXT                                   # only the cases whose name holds TEXT
 *   --verbose                                     # each case's times, level and threshold
 *   --trace=TEXT [--from=S --to=S]                # block by block, the cases whose name holds TEXT
 *   --tune=RATIO:1.7,HOLD:0.3                     # the detector's constants overridden
 *   --stats                                       # talk's and shouts' 20 ms levels over their RMS
 *
 * The cases: (a) silence then a shout; (b) talk at 0.02, 0.05 and 0.1 RMS, with pauses of up to
 * 20 s, never counts (straight out of silence, once at most); (c) shouts 4x over each talk level
 * count once each, and shouts closer than REST once; (d) a quiet microphone (all of it x0.1):
 * shouts under the floor (0.05 RMS) aren't heard, the floor's trade-off, a full shout (10x the
 * talk) is, and talk isn't; (e) a steady game sound (a music bed at 0.04-0.2 RMS, from the
 * start, swelling, or under talk) doesn't count, and shouts over it do (one starting out of
 * silence counts once at most); (f) a clap (60 ms) doesn't count, nor do claps teach the ear;
 * (g) nothing counts while muted, nor is learned; (h) slow frames (looks up to the analyser's
 * 0.68 s) hear the same shouts; (i) scripts/pad_check.py's stand-in microphone (a tone burst
 * every 4 s) is a shout a burst; (j) on-and-off talk (bursts of 2-6 s, pauses of 3-20 s, or of
 * 90 s; a short phrase every 6.6 s), on a normal and a quiet microphone: only the first burst,
 * straight out of quiet, may count, once; (k) the game's sounds in a quiet room (bursts of
 * 0.3-0.6 s at 0.01-0.04 RMS, tonal and noisy) don't count; (l) a test shout while the ear warms
 * up doesn't deafen it; (m) nor does a long held "aaah" or a loud music bed that stops; (n) music
 * that starts after the microphone opened (a tune, a tune over a bed, accents over a bed, a
 * louder chorus) counts once at most as it starts (as a sound that loud out of the blue is a
 * shout's start); (o) a voice that gets 1.8-2.5x louder may count once in its first 2 s (its
 * first loud syllables are as far over the talk heard so far as a shout), never later; (p)
 * coughs, laughs and breaths don't count; (q) a long cry that falls under the bar it began at
 * doesn't raise the bar for a shout 10 s later, and a cry that wavers for its first 0.5 s is
 * heard once it holds (twelve voices each).
 * The voices: a glottal pulse train through three formant resonators, in syllables (talk at
 * ~140 Hz, some drawn out and stressed, the odd "umm"), one held vowel (a shout: higher,
 * brighter; a cry that falls or wavers) or "ha"s (a laugh), from a seeded generator; the
 * game's sounds: gliding notes with harmonics, low-passed noise, a chord with a beat, a tune of
 * harmonic notes; coughs and breaths: shaped low-passed noise. The sounds and `listen` are
 * exported for tuning scripts (`listen(x, { tune: { RATIO: 1.7 } })` overrides the detector's
 * constants).
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
const TUNE = args.tune ? Object.fromEntries(String(args.tune).split(',').map((kv) => [kv.split(':')[0], Number(kv.split(':')[1])])) : {};

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
// A long cry: a shout whose level falls to `fall` of its start over its middle fifth (as a
// breath runs out).
function cry(seconds, seed, level, fall) {
  const y = shout(seconds, seed, level), n = y.length;
  return y.map((v, i) => v * (i < 0.4 * n ? 1 : i > 0.6 * n ? fall : 1 - (1 - fall) * (i - 0.4 * n) / (0.2 * n)));
}
// A cry that wavers (its level +-`depth` at 7 Hz) for its first `rough` seconds, then holds.
const wavering = (seconds, seed, level, rough, depth) =>
  shout(seconds, seed, level).map((v, i) => v * (i < sec(rough) ? 1 + depth * Math.sin(2 * Math.PI * 7 * i / RATE) : 1));
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
// A game sound through the speakers, `seconds` long at `level` RMS: tonal (a gliding note with
// a few harmonics, a chime or a horn) or noisy (low-passed noise, a whoosh or a hit's crash).
function gameSound(seconds, level, seed, tonal) {
  const r = rng(seed), n = sec(seconds), out = new Float32Array(n);
  const f0 = 180 + 600 * r(), f1 = f0 * (0.7 + 0.6 * r()), cut = 1 - Math.exp(-2 * Math.PI * (800 + 3000 * r()) / RATE);
  let ph = 0, lp = 0;
  for (let i = 0; i < n; i++) {
    let v;
    if (tonal) {
      ph += (f0 + (f1 - f0) * (i / n)) / RATE;
      v = Math.sin(2 * Math.PI * ph) + 0.5 * Math.sin(4 * Math.PI * ph) + 0.25 * Math.sin(6 * Math.PI * ph);
    } else {
      lp += (gauss(r) - lp) * cut;
      v = lp;
    }
    out[i] = v * envelope(i, n, sec(0.01), sec(0.05));
  }
  return scaled(out, level);
}
// A tune through the speakers: notes of 0.3-0.8 s (a tone at 220-440 Hz with three harmonics,
// 30 ms attack, 50 ms release), 80 or 300 ms apart.
function melody(seconds, level, seed = 13) {
  const r = rng(seed), n = sec(seconds), out = new Float32Array(n), scale = [220, 247, 277, 294, 330, 370, 415, 440];
  for (let at = 0; at < n;) {
    const len = sec(0.3 + 0.5 * r()), f = scale[Math.floor(r() * scale.length)];
    for (let i = 0; i < len && at + i < n; i++) {
      const ph = 2 * Math.PI * f * i / RATE;
      out[at + i] = (Math.sin(ph) + 0.5 * Math.sin(2 * ph) + 0.25 * Math.sin(3 * ph) + 0.12 * Math.sin(4 * ph)) * envelope(i, len, sec(0.03), sec(0.05));
    }
    at += len + sec(r() < 0.5 ? 0.08 : 0.3);
  }
  return scaled(out, level);
}
// Scaled so that its loudest 20 ms (in steps of 10 ms) is at `peak` RMS.
function peaked(x, peak) {
  let best = 0;
  for (let i = 0; i + 960 <= x.length; i += 480) best = Math.max(best, rms(x, i, i + 960));
  const k = peak / (best || 1);
  return x.map((v) => v * k);
}
// Low-passed noise (a cutoff of `cut` Hz) under an envelope env(t).
function shaped(seconds, seed, cut, env) {
  const r = rng(seed), c = 1 - Math.exp(-2 * Math.PI * cut / RATE);
  let lp = 0;
  return Float32Array.from({ length: sec(seconds) }, (_, i) => {
    lp += (gauss(r) - lp) * c;
    return lp * env(i / RATE);
  });
}
// A cough: noise rising in 8 ms and falling away with a time constant of `tau` s; its loudest
// 20 ms at `peak` RMS.
const cough = (peak, tau, seed) => peaked(shaped(Math.min(1.2, 6 * tau), seed, 1500, (t) => (t < 0.008 ? t / 0.008 : Math.exp(-(t - 0.008) / tau))), peak);
// A breath into a headset: 0.85 s of low-passed noise, swelling and fading; its loudest 20 ms
// at `peak` RMS.
const breath = (peak, seed, seconds = 0.85) => peaked(shaped(seconds, seed, 1000, (t) => Math.sin(Math.PI * t / seconds) ** 2), peak);
// A laugh: `count` "ha"s, 4.5 a second, voiced between them `gapDb` under the vowels (-8 dB),
// at `level` RMS.
function laugh(count, level, seed, gapDb = -8) {
  const r = rng(seed), f = 200 + 60 * r(), parts = [];
  for (let k = 0; k < count; k++) {
    parts.push({ n: sec(0.15), f0: [f + 10, f - 10], amp: 1, vowel: [800, 1200, 2600], open: 0.5, attack: sec(0.015), release: sec(0.02), hiss: 0.05 });
    parts.push({ n: sec(0.072), f0: [f, f], amp: Math.pow(10, gapDb / 20), vowel: [800, 1200, 2600], open: 0.5, attack: sec(0.01), release: sec(0.01), hiss: 0.3 });
  }
  return scaled(voice(parts, r), level);
}

// A timeline: pieces laid end to end (or `at` a time over what is there), a floor of room
// noise under all of it. Returns the samples, the shouts' [start, end] times and those of the
// pieces marked `maybe` (heard once or not at all: a test shout while the ear warms up).
function scene(pieces, floor = 0.0005) {
  let end = 0;
  for (const p of pieces) end = Math.max(end, (p.at ?? end) + p.x.length / RATE);
  const out = noise(end, floor, 3), shouts = [], maybe = [];
  let t = 0;
  for (const p of pieces) {
    const at = p.at ?? t, i0 = sec(at);
    for (let i = 0; i < p.x.length && i0 + i < out.length; i++) out[i0 + i] += p.x[i] * (p.fade ? Math.min(1, i / sec(p.fade)) : 1);
    if (p.shout) shouts.push([at, at + p.x.length / RATE]);
    if (p.maybe) maybe.push([at, at + p.x.length / RATE]);
    if (p.at === undefined) t = at + p.x.length / RATE;
  }
  return { x: out, shouts, maybe };
}
const gap = (s) => ({ x: new Float32Array(sec(s)) });
const louder = (sc, k) => ({ x: sc.x.map((v) => v * k), shouts: sc.shouts, maybe: sc.maybe, first: sc.first });
// On-and-off talk: `lead` seconds of quiet, then bursts of 2-6 s with the `pauses` between them.
// `first`: the first burst's [start, end] (straight out of quiet, its first stressed syllable
// is a shout's start: it may count once).
function onAndOff(level, seed, pauses = [3, 6, 10, 20], lead = 2) {
  const r = rng(seed), pieces = [gap(lead)];
  let first = null;
  for (let i = 0; i <= pauses.length; i++) {
    const len = 2 + 4 * r();
    if (!first) first = [lead, lead + len];
    pieces.push({ x: talk(len, seed + i, level) });
    if (i < pauses.length) pieces.push(gap(pauses[i]));
  }
  return { ...scene(pieces), first };
}

// ---- Listening as the page does ------------------------------------------------------------------

// Looks every `look()` seconds (30 ms ticks, or slow frames); a look longer than the analyser
// holds reads only its last ANALYSER samples, as the page does. `muted(t)`: the mute button.
function listen(x, { look = () => 0.03, muted = () => false, tune = TUNE } = {}) {
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
  return { heard, level: d.level, thr: d.threshold };
}

// ---- The cases --------------------------------------------------------------------------------

const results = [];
// By default the scene's shouts are each heard exactly once and nothing else is, but for its
// `maybe` pieces, heard once or not (`once`: spans heard once each instead, for shouts closer
// than REST); `quiet`: nothing heard; `max`: at most this many heard in all; `first`: at most
// one heard, in the scene's first stretch of talk (out of quiet). A shout is heard from 0.1 s
// after its start to 0.25 s after its end.
function check(name, sc, { look, muted, max, once, first, quiet = false } = {}) {
  if (args.only && !name.includes(args.only)) return;
  if (args.trace && name.includes(args.trace)) {
    const d = shoutDetector(RATE, TUNE);
    for (let i = 0; i + 960 <= sc.x.length; i += 960) {
      const thr = d.threshold, level = d.level, r = rms(sc.x, i, i + 960);
      const t = d.feed(sc.x, i, i + 960, (i + 960) / RATE, false);
      const s0 = (i + 960) / RATE, all = args.from !== undefined && s0 >= Number(args.from) && s0 <= Number(args.to ?? Infinity);
      if (all || r >= thr || t !== null || i % (960 * 25) === 0) console.log(`  ${s0.toFixed(2)} rms ${r.toFixed(4)} level ${level.toFixed(4)} thr ${thr.toFixed(4)}${r >= thr ? ' LOUD' : ''}${t !== null ? ' SHOUT' : ''} (now ${d.levels.now.toFixed(4)}, talk ${d.levels.talk.toFixed(4)})`);
    }
  }
  const { heard, level, thr } = listen(sc.x, { look, muted });
  const spans = once ?? sc.shouts;
  const inSpan = (t, [a, b]) => t >= a + 0.1 && t <= b + 0.25;
  const per = spans.map((s) => heard.filter((t) => inSpan(t, s)).length);
  const maybe = sc.maybe || [], perhaps = maybe.map((s) => heard.filter((t) => inSpan(t, s)).length);
  const stray = heard.filter((t) => !spans.some((s) => inSpan(t, s)) && !maybe.some((s) => inSpan(t, s)));
  let ok;
  let what;
  if (first) {
    const late = heard.filter((t) => t > sc.first[1] + 0.25);
    ok = heard.length <= 1 && !late.length;
    what = `${heard.length - late.length} heard in the first stretch (at most 1), ${late.length} after (none expected)`;
  } else if (max !== undefined) {
    ok = heard.length <= max;
    what = `${heard.length} heard (at most ${max})`;
  } else if (quiet) {
    ok = heard.length === 0;
    what = `${heard.length} heard (none expected)`;
  } else {
    ok = per.every((n) => n === 1) && perhaps.every((n) => n <= 1) && stray.length === 0;
    what = `${per.filter((n) => n === 1).length}/${spans.length} shouts heard once, ${stray.length} stray` +
      (maybe.length ? `, ${perhaps.reduce((a, b) => a + b, 0)} heard where one may be` : '');
  }
  results.push({ name, ok });
  console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}: ${what}`);
  if (args.verbose || !ok) {
    console.log(`        heard at ${heard.map((t) => t.toFixed(2)).join(' ') || '-'}; shouts ${spans.map(([a, b]) => `${a.toFixed(1)}-${b.toFixed(1)}`).join(' ') || '-'}; level ${level.toFixed(4)}, threshold ${thr.toFixed(4)} RMS at the end`);
  }
}
// The same as `check` (each shout heard once, nothing else) over `n` voices, one result.
function voices(name, make, n = 12) {
  if (args.only && !name.includes(args.only)) return;
  const inSpan = (t, [a, b]) => t >= a + 0.1 && t <= b + 0.25, bad = [];
  let once = 0, due = 0, stray = 0;
  for (let v = 0; v < n; v++) {
    const sc = make(v), { heard } = listen(sc.x), per = sc.shouts.map((s) => heard.filter((t) => inSpan(t, s)).length);
    const wrong = heard.filter((t) => !sc.shouts.some((s) => inSpan(t, s))).length;
    once += per.filter((k) => k === 1).length;
    due += per.length;
    stray += wrong;
    if (wrong || per.some((k) => k !== 1)) bad.push(`${v}:${per.join('')}${wrong ? `+${wrong}` : ''}`);
  }
  const ok = once === due && !stray;
  results.push({ name, ok });
  console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}: ${once}/${due} shouts heard once, ${stray} stray${bad.length ? ` (voice:times heard ${bad.join(' ')})` : ''}`);
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
  // (d) A quiet microphone: all of it x0.1 (talk at 0.002-0.01 RMS, -54 to -40 dBFS). A shout
  // must still reach FLOOR (0.05 RMS): one only 4x over such talk (0.02-0.04) isn't heard, the
  // floor's trade-off against the game's sound in a quiet room (k); a full shout, 10x over the
  // talk (+20 dB), is.
  check('(d) quiet mic: silence, then a shout at 0.3 x0.1 (0.03, under the floor): not heard',
    louder(scene([gap(3), { x: shout(0.8, 1, 0.3) }, gap(3)]), 0.1), { quiet: true });
  for (const level of [0.05, 0.1]) {
    check(`(d) quiet mic: talk at ${level} x0.1, shouts 4x (${(0.4 * level).toFixed(2)}, under the floor): not heard`,
      louder(talkWithShouts(level, 4, 60 + level * 1000), 0.1), { quiet: true });
  }
  check('(d) quiet mic: talk at 0.1 x0.1, shouts 10x over it (0.1): heard', louder(talkWithShouts(0.1, 10, 65), 0.1));
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
    // Claps (or a fist on the desk) don't teach the ear how loud the player is: shouts after them
    // in a silent room are heard.
    const then = [gap(1)];
    for (let i = 0; i < 6; i++) then.push({ x: clap(0.5, 90 + i) }, gap(1));
    for (let i = 0; i < 4; i++) then.push({ x: shout(0.7, 97 + i, 0.25), shout: true }, gap(8));
    check('(f) six claps (0.5 RMS) in a silent room, then four shouts at 0.25: each heard', scene(then));
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
  // (j) On-and-off talk: bursts of 2-6 s with pauses of 3, 6, 10 and 20 s between (twice over),
  // after 2 s of quiet. The first burst, straight out of quiet, may count once; no later one
  // does, however long the pause: the level stays where the talk left it.
  for (const level of [0.02, 0.05, 0.1]) {
    for (const k of [1, 0.1]) {
      const sc = louder(onAndOff(level, 120 + level * 1000, [3, 6, 10, 20, 3, 10, 6, 20]), k);
      check(`(j) on-and-off talk at ${level} RMS${k === 1 ? '' : ' x0.1 (quiet mic)'}, pauses of 3-20 s: no burst after the first counts`, sc, { first: true });
    }
  }
  check('(j) talk for 20 s, 90 s of quiet, talk for 20 s at 0.05 RMS', scene([{ x: talk(20, 130, 0.05) }, gap(90), { x: talk(20, 131, 0.05) }], 0.002), { quiet: true });
  // The same over a background that isn't silence: mains hum (50 Hz and its harmonics) or a
  // microphone's DC offset don't teach the ear (a steady sound, not over the quiet of the last
  // 3 s); a radio (a quiet tune, voiced, with gaps) does, as voice, so it fills the talk ring
  // in pauses longer than about 15 s: here they are up to 10 s.
  {
    const oo = onAndOff(0.05, 125, [3, 6, 10, 20, 3, 10, 6, 20]), n = oo.x.length, r = rng(126);
    const hum = Float32Array.from({ length: n }, (_, i) => [1, 2, 3, 4].reduce((a, k) => a + Math.sin(2 * Math.PI * 50 * k * i / RATE) / k, 0));
    for (const [what, bed] of [['mains hum at 0.005 RMS', scaled(hum, 0.005)], ['a DC offset of 0.003', new Float32Array(n).fill(0.003)]]) {
      check(`(j) on-and-off talk at 0.05 RMS over ${what}, pauses of 3-20 s: no burst after the first counts`,
        { ...oo, x: oo.x.map((v, i) => v + bed[i] + 0.001 * gauss(r)) }, { first: true });
    }
    const near = onAndOff(0.05, 128, [3, 6, 10, 3, 10, 6, 10]), radio = scaled(melody(near.x.length / RATE + 1, 1, 127), 0.01);
    check('(j) on-and-off talk at 0.05 RMS over a radio (a tune) at 0.01 RMS, pauses of 3-10 s: no burst after the first counts',
      { ...near, x: near.x.map((v, i) => v + radio[i]) }, { first: true });
  }
  {
    const pieces = [gap(3)];
    for (let i = 0; i < 10; i++) pieces.push({ x: talk(0.6, 140 + i, 0.05) }, gap(6));
    check('(j) a short phrase (0.6 s at 0.05 RMS) every 6.6 s in a quiet room: only the first may count',
      { ...scene(pieces, 0.002), first: [3, 3.6] }, { first: true });
  }
  // (k) The game's sounds through the speakers in a quiet room: bursts of 0.3-0.6 s, tonal and
  // noisy by turns, 3.4 s apart, at 0.01-0.04 RMS (under the floor): none counts.
  for (const level of [0.01, 0.02, 0.03, 0.04]) {
    const r = rng(150 + level * 1000), pieces = [gap(2)];
    for (let i = 0; i < 10; i++) {
      const len = 0.3 + 0.3 * r();
      pieces.push({ x: gameSound(len, level, 160 + i, i % 2 === 0) }, gap(3.4 - len));
    }
    check(`(k) game sounds at ${level} RMS in a quiet room (10 bursts of 0.3-0.6 s, tonal and noisy)`, scene(pieces), { quiet: true });
  }
  // (l) A test shout while the ear warms up (its first 1.5 s; it may count or not) doesn't
  // deafen it: talk and shouts as loud as the test after, or shouts alone.
  for (const at of [0.2, 0.6, 1.0]) {
    const pieces = [gap(at), { x: shout(0.7, 1, 0.3), maybe: true }, gap(1.5)];
    for (let i = 0; i < 12; i++) pieces.push({ x: talk(9.3, 10 + i, 0.05) }, { x: shout(0.7, 30 + i, 0.3), shout: true });
    check(`(l) a test shout (0.3 RMS) at ${at} s, warming up, then talk at 0.05 and twelve shouts as loud: each heard`, scene(pieces));
  }
  {
    const pieces = [gap(0.6), { x: shout(0.7, 2, 0.15), maybe: true }, gap(1.5)];
    for (let i = 0; i < 8; i++) pieces.push({ x: talk(9.3, 50 + i, 0.03) }, { x: shout(0.7, 70 + i, 0.15), shout: true });
    check('(l) a test shout (0.15 RMS) at 0.6 s, then talk at 0.03 and eight shouts as loud: each heard', scene(pieces));
    const solo = [gap(0.8), { x: shout(0.7, 60, 0.25), maybe: true }, gap(4)];
    for (let i = 0; i < 6; i++) solo.push({ x: shout(0.7, 61 + i, 0.25), shout: true }, gap(10));
    check('(l) a test shout (0.25 RMS) at 0.8 s, then six shouts as loud 10 s apart, no talk: each heard', scene(solo));
  }
  // (m) A long held "aaah" counts once and doesn't deafen the ear: shouts as loud after it are
  // heard, alone or between talk; so are shouts after a loud music bed stops.
  for (const len of [3.5, 6, 8]) {
    const pieces = [gap(3), { x: shout(len, 71, 0.3), shout: true }, gap(4)];
    for (let i = 0; i < 4; i++) pieces.push({ x: shout(0.7, 90 + i, 0.3), shout: true }, gap(10));
    check(`(m) a held "aaah" of ${len} s (0.3 RMS) in a quiet room, then four shouts as loud: each heard once`, scene(pieces));
    if (len === 6) check('(m) the same over room noise at 0.005 RMS', scene(pieces, 0.005));
  }
  for (const len of [6, 8]) {
    const pieces = [{ x: talk(15, 70, 0.05) }, { x: shout(len, 71, 0.3), shout: true }, gap(1)];
    for (let i = 0; i < 10; i++) pieces.push({ x: talk(9.3, 72 + i, 0.05) }, { x: shout(0.7, 90 + i, 0.3), shout: true });
    check(`(m) talk at 0.05, a held "aaah" of ${len} s (0.3 RMS), then talk and ten shouts as loud: each heard once`, scene(pieces));
  }
  {
    const pieces = [{ x: talk(15, 75, 0.05) }, { x: music(20, 0.15), maybe: true }];
    for (let i = 0; i < 5; i++) pieces.push({ x: talk(9.3, 76 + i, 0.05) }, { x: shout(0.7, 95 + i, 0.2), shout: true });
    check('(m) talk at 0.05, a music bed at 0.15 for 20 s (its start may count), then talk and five shouts at 0.2: each heard', scene(pieces));
  }
  // (n) Music that starts after the microphone opened: a tune (notes of 0.3-0.8 s, each held
  // as long as a shout), alone, over a bed or after talk, counts once at most, as it starts (a
  // sound that loud out of the blue is a shout's start). So does a bed's first tonal accent
  // 2.5-3x over it, but not the ones that follow every 2.5 s, and a tune's chorus 2.5x louder
  // than its verse, once each; a bed's louder chorus (a chord, not voiced enough) or a tune
  // playing as the microphone opens, never.
  check('(n) 3 s of quiet, then a tune at 0.06 RMS for 30 s: once at most', scene([gap(3), { x: melody(30, 0.06) }]), { max: 1 });
  check('(n) 3 s of quiet, then a tune at 0.06 over a music bed at 0.03 for 60 s: once at most',
    scene([gap(3), { x: music(60, 0.03) }, { at: 3, x: melody(60, 0.06, 14) }]), { max: 1 });
  check('(n) talk at 0.03 for 15 s, then a tune at 0.1 for 30 s: once at most', scene([{ x: talk(15, 3, 0.03) }, { x: melody(30, 0.1, 15) }]), { max: 1 });
  check('(n) talk at 0.05 for 15 s, then a tune at 0.15 over a bed at 0.075 for 40 s: once at most',
    scene([{ x: talk(15, 4, 0.05) }, { x: music(40, 0.075) }, { at: 15, x: melody(40, 0.15, 16) }]), { max: 1 });
  {
    const r = rng(170), pieces = [{ x: music(38, 0.03) }];
    for (let i = 0; i < 14; i++) pieces.push({ at: 2.5 + 2.5 * i, x: gameSound(0.3 + 0.2 * r(), (2.5 + 0.5 * r()) * 0.03, 171 + i, true) });
    check('(n) a music bed at 0.03 with tonal accents (0.3-0.5 s) 2.5-3x over it every 2.5 s: the first at most', scene(pieces), { max: 1 });
    const turns = [];
    for (let i = 0; i < 4; i++) turns.push({ x: music(15, 0.03, 180 + i) }, { x: music(15, 0.075, 190 + i) });
    check('(n) a verse at 0.03 and a chorus at 0.075 RMS by turns, 15 s each, 2 min', scene(turns), { quiet: true });
    check('(n) a tune at 0.1 playing as the microphone opens, 2 min', scene([{ x: melody(120, 0.1, 17) }]), { quiet: true });
    const tune = [];
    for (let i = 0; i < 4; i++) tune.push({ x: melody(15, 0.03, 180 + i) }, { x: melody(15, 0.075, 190 + i), maybe: true });
    check('(n) a tune with a verse at 0.03 and a chorus at 0.075 by turns, 15 s each: each chorus once at most', scene(tune));
  }
  // (o) A raised voice: talk at 0.03, then the same voice 1.8, 2 or 2.5 times as loud (5-8 dB;
  // excited, or speaking up), twelve voices each. Its first drawn-out syllables reach 4-5x the
  // talk heard so far, held and steady: as far over it as a shout, so one may count while the
  // ear hears the new level (the `now` ring, within 2 s); none later.
  for (const up of [1.8, 2, 2.5]) {
    const name = `(o) talk at 0.03 for 15 s, then ${up}x as loud for 20 s, twelve voices: once at most each, in its first 2 s`;
    if (args.only && !name.includes(args.only)) continue;
    const times = [];
    let twice = 0;
    for (let v = 0; v < 12; v++) {
      const { heard } = listen(scene([{ x: talk(15, 2000 + v, 0.03) }, { x: talk(20, 3000 + v, 0.03 * up) }]).x);
      times.push(...heard.map((t) => [v, t]));
      if (heard.length > 1) twice++;
    }
    const late = times.filter(([, t]) => t > 17), ok = !late.length && !twice;
    results.push({ name, ok });
    console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}: ${times.length} heard in ${new Set(times.map(([v]) => v)).size} voices, ` +
      `${late.length} later than 2 s, ${twice} voices more than once${times.length ? ` (voice:s ${times.map(([v, t]) => `${v}:${t.toFixed(1)}`).join(' ')})` : ''}`);
  }
  // (p) Coughs, laughs and breaths into the microphone, in a quiet room or after talk: a cough
  // falls away, a laugh comes in "ha"s, a breath is noise; none is a held vowel.
  for (const [peak, tau] of [[0.25, 0.18], [0.4, 0.12], [0.4, 0.18], [0.8, 0.25]]) {
    const pieces = [gap(2)];
    for (let i = 0; i < 5; i++) pieces.push({ x: cough(peak, tau, 200 + i) }, gap(3));
    check(`(p) coughs (the loudest 20 ms at ${peak} RMS, falling away over ${tau} s) in a quiet room`, scene(pieces), { quiet: true });
  }
  {
    const talked = [{ x: talk(10, 220, 0.03) }];
    for (let i = 0; i < 5; i++) talked.push({ x: cough(0.4, 0.18, 230 + i) }, { x: talk(2.5, 240 + i, 0.03) });
    check('(p) talk at 0.03 with five coughs (0.4 RMS, falling away over 0.18 s)', scene(talked), { quiet: true });
    const laughs = [gap(2)];
    for (let i = 0; i < 4; i++) laughs.push({ x: laugh(5, 0.2, 210 + i) }, gap(3));
    check('(p) laughs (five "ha"s, voiced 8 dB lower between them, 0.2 RMS) in a quiet room', scene(laughs), { quiet: true });
    const breaths = [gap(2)];
    for (let i = 0; i < 5; i++) breaths.push({ x: breath(0.1, 250 + i) }, gap(2.5));
    check('(p) breaths into a headset (0.85 s of noise, the loudest 20 ms at 0.1 RMS) in a quiet room', scene(breaths), { quiet: true });
  }
  // (q) A cry doesn't raise the bar for the next shout, nor its own start the bar over the rest
  // of it. Talk at 0.05, then three times a long cry (3 s at 0.2, falling to 0.7 of that in its
  // middle: under the bar it began at, over the talk) and a shout as loud 10 s after it; or four
  // cries that waver (+-60% at 7 s^-1) for their first 0.5 s, then hold for 1.2 s, between talk.
  // Twelve voices each.
  voices('(q) talk at 0.05, a long cry falling to 0.7 of 0.2 then a shout at 0.2 10 s after it, three times, twelve voices: each heard', (v) => {
    const pieces = [{ x: talk(15, 500 + v, 0.05) }];
    for (let i = 0; i < 3; i++) {
      pieces.push({ x: cry(3, 510 + 10 * v + i, 0.2, 0.7), shout: true }, { x: talk(7, 520 + 10 * v + i, 0.05) });
      pieces.push({ x: shout(0.7, 530 + 10 * v + i, 0.2), shout: true }, { x: talk(9.3, 540 + 10 * v + i, 0.05) });
    }
    return scene(pieces);
  });
  voices('(q) talk at 0.05 and four cries at 0.2 that waver for their first 0.5 s, twelve voices: each heard', (v) => {
    const pieces = [{ x: talk(15, 600 + v, 0.05) }];
    for (let i = 0; i < 4; i++) pieces.push({ x: wavering(1.7, 610 + 10 * v + i, 0.2, 0.5, 0.6), shout: true }, { x: talk(9, 620 + 10 * v + i, 0.05) });
    return scene(pieces);
  });
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

// Many voices (--sweep=N): talk alone (b), talk with shouts K times over it (c; --shout=K, 4 by
// default) and on-and-off talk out of quiet (j; pauses of 3-20 s), seed after seed, at each
// level (--levels=...; --quiet-mic: the levels x0.1). Talk's loudest, drawn-out syllables come
// close to a shout, so this is a rate: it passes at no more than one shout heard in talk per 5
// minutes of it (in a burst of on-and-off talk after the first: none) and 2% of shouts missed.
// Shouts under the floor (the bar in silence) aren't heard by design: their misses are counted
// apart, the floor's trade-off.
function sweep(n) {
  const k = Number(args.shout) || 4, floor = shoutDetector(RATE).threshold;
  const levels = args.levels ? String(args.levels).split(',').map(Number) : [0.02, 0.05, 0.1];
  for (const base of levels) {
    const level = args['quiet-mic'] ? base * 0.1 : base, under = 1.1 * k * level < floor;
    let falseShouts = 0, missed = 0, shouts = 0, seconds = 0, later = 0, first = 0, bursts = 0, onSeconds = 0;
    for (let seed = 1000; seed < 1000 + n; seed++) {
      const alone = listen(scene([{ x: talk(30, seed, level) }]).x).heard;
      const sc = talkWithShouts(level, k, seed * 7);
      const { heard } = listen(sc.x);
      const inSpan = (t, [a, b]) => t >= a + 0.1 && t <= b + 0.25;
      const lost = sc.shouts.filter((s) => !heard.some((t) => inSpan(t, s))), wrong = heard.filter((t) => !sc.shouts.some((s) => inSpan(t, s)));
      shouts += sc.shouts.length;
      missed += lost.length;
      falseShouts += alone.length + wrong.length;
      seconds += 30 + sc.x.length / RATE - sc.shouts.reduce((s, [a, b]) => s + b - a, 0);
      const r = rng(seed * 13), pauses = Array.from({ length: 6 }, () => [3, 6, 10, 20][Math.floor(r() * 4)]);
      const oo = onAndOff(level, seed * 3, pauses), ooHeard = listen(oo.x).heard;
      const late = ooHeard.filter((t) => t > oo.first[1] + 0.25);
      later += late.length;
      first += ooHeard.length > late.length ? 1 : 0;
      bursts += pauses.length + 1;
      onSeconds += oo.x.length / RATE;
      if (args.verbose && (alone.length || lost.length || wrong.length || late.length)) {
        console.log(`        seed ${seed}: talk alone heard at ${alone.map((t) => t.toFixed(2)).join(' ') || '-'}; with shouts missed ${lost.map(([a]) => a.toFixed(1)).join(' ') || '-'}, stray ${wrong.map((t) => t.toFixed(2)).join(' ') || '-'}; on-and-off, after the first burst ${late.map((t) => t.toFixed(2)).join(' ') || '-'}`);
      }
    }
    const minutes = seconds / 60, ok = falseShouts <= minutes / 5 && later === 0 && (under || missed <= 0.02 * shouts);
    const name = `talk at ${+level.toFixed(4)} RMS${args['quiet-mic'] ? ` (${base} x0.1, quiet mic)` : ''}`;
    results.push({ name: `sweep, ${name}`, ok });
    console.log(`${ok ? 'ok  ' : 'FAIL'}  sweep, ${name}, ${n} voices: ${falseShouts} shouts heard in ${minutes.toFixed(1)} min of talk (at most ${Math.floor(minutes / 5)}); ` +
      `on-and-off talk (${(onSeconds / 60).toFixed(1)} min, ${bursts} bursts): ${later} heard after the first burst, ${first}/${n} first bursts out of quiet counted once; ` +
      `${missed}/${shouts} shouts at ${k}x missed${under ? ` (at ${(k * level).toFixed(3)} RMS, under the floor of ${floor.toFixed(2)}: missed by design)` : ''}`);
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
export { RATE, rng, talk, shout, cry, wavering, scene, gap, talkWithShouts, onAndOff, gameSound, music, melody, clap, cough, laugh, breath, louder, listen, sec, shoutDetector };
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (args.stats) stats();
  else if (args.sweep) sweep(Number(args.sweep) || 20);
  else if (args.wav) runWav(String(args.wav));
  else runCases();
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} cases pass${failed.length ? `; failed: ${failed.map((r) => r.name).join('; ')}` : ''}`);
  process.exit(failed.length ? 1 : 0);
}
