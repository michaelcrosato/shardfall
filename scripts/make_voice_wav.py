#!/usr/bin/env python3
"""Writes a made-up voice recording for testing Shout to roar through a browser's real capture.

usage: python3 -I scripts/make_voice_wav.py OUT.wav [--seed N] [--talk RMS] [--shout-ratio K]

About 40 s of 16-bit mono 48 kHz WAV: ordinary talk (a glottal pulse train at ~140 Hz through
three formant resonators, in syllables with their own loudness, some drawn out, the odd "umm",
pauses between phrases) at a moderate level (RMS 0.04 by default), five shouts (the same voice
held on an open vowel, higher and brighter, 0.6-0.9 s, K = 4 times the talk's RMS), and a few
seconds of near silence (room noise at RMS 0.001 under all of it). It prints each shout's start
and end and writes them beside the WAV as OUT.json (the duration, the levels, the shouts and
the quiet spells), which `node scripts/shout_check.mjs --wav=OUT.wav` and
`scripts/hub_live.mjs --voice=OUT.wav` read. The same model as shout_check.mjs's voices, from
Python's seeded generator: the same seed gives the same file. Standard library only.

Chromium plays it as the microphone with --use-fake-device-for-media-stream
--use-file-for-fake-audio-capture=OUT.wav (it loops the file); docs/AI_HUB.md, "Testing".
"""
import argparse
import array
import json
import math
import pathlib
import random
import wave

RATE = 48000
# Vowels' first three formants (Hz).
VOWELS = [(730, 1090, 2440), (530, 1840, 2480), (270, 2290, 3010), (570, 840, 2410), (300, 870, 2240), (660, 1720, 2410)]


def sec(s):
    return int(round(s * RATE))


def rms(x, a=0, b=None):
    b = len(x) if b is None else b
    if b <= a:
        return 0.0
    return math.sqrt(sum(v * v for v in x[a:b]) / (b - a))


def glottal(ph, open_q):
    """The glottal flow (Rosenberg), open for `open_q` of the period."""
    rise, fall = open_q * 0.7, open_q * 0.3
    if ph < rise:
        return 0.5 * (1 - math.cos(math.pi * ph / rise))
    if ph < open_q:
        return math.cos(0.5 * math.pi * (ph - rise) / fall)
    return 0.0


def envelope(i, n, a, b):
    if i < a:
        return 0.5 - 0.5 * math.cos(math.pi * i / a)
    if i > n - b:
        return 0.5 - 0.5 * math.cos(math.pi * (n - i) / b)
    return 1.0


class Voice:
    """A voice's source and vocal tract; parts render into a list of floats, each at its RMS."""

    def __init__(self, rnd):
        self.rnd, self.ph, self.prev, self.f0 = rnd, 0.0, 0.0, 140.0

    def part(self, out, at, n, f0, amp, formants, open_q, attack, release, hiss):
        if amp <= 0 or n <= 0:
            return
        res = []
        for f, bw in zip(formants, (90, 110, 170)):
            r = math.exp(-math.pi * bw / RATE)
            c, d = 2 * r * math.cos(2 * math.pi * f / RATE), -r * r
            res.append([1 - c - d, c, d, 0.0, 0.0])
        (g1, c1, d1, _, _), (g2, c2, d2, _, _), (g3, c3, d3, _, _) = res
        a1 = a2 = b1 = b2 = e1 = e2 = 0.0
        gauss, sin, two_pi = self.rnd.gauss, math.sin, 2 * math.pi
        seg = [0.0] * n
        for i in range(n):
            want = f0[0] + (f0[1] - f0[0]) * i / n
            self.f0 += (want - self.f0) * 0.002
            self.ph += self.f0 * (1 + 0.004 * sin(two_pi * 5.5 * (at + i) / RATE)) / RATE
            if self.ph >= 1:
                self.ph -= 1
            g = glottal(self.ph, open_q)
            x = (g - self.prev) * 40 + hiss * gauss(0, 1)
            self.prev = g
            y = g1 * x + c1 * a1 + d1 * a2
            a2, a1 = a1, y
            y2 = g2 * y + c2 * b1 + d2 * b2
            b2, b1 = b1, y2
            y3 = g3 * y2 + c3 * e1 + d3 * e2
            e2, e1 = e1, y3
            seg[i] = y3 * envelope(i, n, attack, release)
        k = amp / (rms(seg) or 1)
        for i in range(n):
            out[at + i] += seg[i] * k


def talk(seconds, rnd):
    """Parts of talk: words of 1-3 syllables, short gaps, a pause every few words."""
    parts, t, words, phrase = [], 0.0, 0, rnd.randint(4, 8)
    while t < seconds:
        for k in range(rnd.randint(1, 3)):
            kind = rnd.random()
            drawn, um = kind < 0.12, kind > 0.97
            n = sec(0.4 + rnd.random() * 0.3 if um else 0.3 + rnd.random() * 0.15 if drawn else 0.12 + rnd.random() * 0.16)
            stress = drawn or (not um and k == 0 and rnd.random() < 0.5)
            base = 150 - 25 * (words % phrase) / phrase + (15 if stress else 0) + 8 * rnd.gauss(0, 1)
            if rnd.random() < 0.4:
                parts.append(dict(n=sec(0.03 + rnd.random() * 0.05), f0=(base, base), amp=0.12, formants=(2500, 3500, 4500),
                                  open_q=0.6, attack=sec(0.01), release=sec(0.01), hiss=3))
            db = 2.5 * rnd.gauss(0, 1) + (3 if stress else 0) - (6 if um else 0)
            parts.append(dict(n=n, f0=(base, base - 6), amp=10 ** (db / 20), formants=(500, 1000, 2400) if um else rnd.choice(VOWELS),
                              open_q=0.6, attack=sec(0.03), release=sec(0.05), hiss=0.02))
            parts.append(dict(n=sec(0.02 + rnd.random() * 0.05), amp=0))
        parts.append(dict(n=sec(0.06 + rnd.random() * 0.12), amp=0))
        words += 1
        if words % phrase == 0:
            parts.append(dict(n=sec(0.3 + rnd.random() * 0.4), amp=0))
            phrase = rnd.randint(4, 8)
        t = sum(p["n"] for p in parts) / RATE
    return parts


def shout(seconds, rnd):
    """Parts of a shout: one held, open vowel, higher and brighter than talk."""
    f = 220 + 40 * rnd.random()
    return [
        dict(n=sec(0.04), f0=(f, f), amp=0.2, formants=(2000, 3000, 4000), open_q=0.45, attack=sec(0.01), release=sec(0.01), hiss=3),
        dict(n=sec(seconds - 0.04), f0=(f, f + 10), amp=1, formants=(850, 1250, 2700), open_q=0.45, attack=sec(0.05), release=sec(0.12), hiss=0.03),
    ]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("out", help="the WAV file to write (OUT.json goes beside it)")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--talk", type=float, default=0.04, help="the talk's RMS, of 1 (default 0.04, -28 dBFS)")
    ap.add_argument("--shout-ratio", type=float, default=4.0, help="the shouts' RMS over the talk's (default 4)")
    a = ap.parse_args()
    rnd = random.Random(a.seed)

    # The timeline: (what, seconds). It opens on talk (the page's ear learns the voice while it
    # warms up; talk straight out of silence can count once, docs/AI_HUB.md), and the shouts
    # sit between stretches of talk, at least 3 s apart. Looped, the quiet at the end runs into
    # the talk at the start: by then the ear knows the voice.
    plan = [("talk", 7.5), ("shout", 0.7), ("talk", 4), ("shout", 0.8), ("quiet", 3), ("talk", 5),
            ("shout", 0.6), ("talk", 3), ("quiet", 2), ("talk", 4), ("shout", 0.9), ("talk", 3), ("shout", 0.75),
            ("talk", 2.5), ("quiet", 3)]
    total = sec(sum(s for _, s in plan))
    out = [0.0] * total
    voice = Voice(rnd)
    talk_spans, shouts, quiet = [], [], []
    at = 0
    for what, seconds in plan:
        n = sec(seconds)
        if what == "quiet":
            quiet.append((at / RATE, (at + n) / RATE))
        else:
            parts = talk(seconds, rnd) if what == "talk" else shout(seconds, rnd)
            p_at = at
            for p in parts:
                if p_at + p["n"] > at + n:
                    break
                voice.part(out, p_at, p["n"], p.get("f0", (140, 140)), p["amp"], p.get("formants", VOWELS[0]), p.get("open_q", 0.6),
                           p.get("attack", 1), p.get("release", 1), p.get("hiss", 0))
                p_at += p["n"]
            (talk_spans if what == "talk" else shouts).append((at, at + n))
        at += n

    # Levels: the talk at --talk RMS (over all its stretches), each shout K times that.
    talk_now = math.sqrt(sum(sum(v * v for v in out[a0:b0]) for a0, b0 in talk_spans) / sum(b0 - a0 for a0, b0 in talk_spans))
    k_talk = a.talk / talk_now
    for a0, b0 in talk_spans:
        for i in range(a0, b0):
            out[i] *= k_talk
    for a0, b0 in shouts:
        k = a.shout_ratio * a.talk / (rms(out, a0, b0) or 1)
        for i in range(a0, b0):
            out[i] *= k
    # Room noise under all of it.
    for i in range(total):
        out[i] += 0.001 * rnd.gauss(0, 1)
    peak = max(abs(v) for v in out)
    if peak > 0.99:
        raise SystemExit(f"the loudest sample is {peak:.2f}: lower --talk or --shout-ratio")

    path = pathlib.Path(a.out)
    path.parent.mkdir(parents=True, exist_ok=True)
    pcm = array.array("h", (max(-32767, min(32767, int(round(v * 32767)))) for v in out))
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    spans = [(round(a0 / RATE, 3), round(b0 / RATE, 3)) for a0, b0 in shouts]
    side = path.with_suffix(".json")
    side.write_text(json.dumps({
        "rate": RATE, "duration": round(total / RATE, 3), "seed": a.seed, "talk_rms": a.talk,
        "shout_rms": round(a.shout_ratio * a.talk, 4), "peak": round(peak, 3),
        "shouts": spans, "quiet": [(round(x, 3), round(y, 3)) for x, y in quiet],
    }, indent=1) + "\n")
    print(f"{path}: {total / RATE:.1f} s at {RATE} Hz, talk RMS {a.talk}, shouts RMS {a.shout_ratio * a.talk:.3f}, peak {peak:.2f}")
    for i, (s0, s1) in enumerate(spans, 1):
        print(f"  shout {i}: {s0:6.2f} - {s1:6.2f} s")
    print(f"  quiet: {', '.join(f'{x:.1f}-{y:.1f} s' for x, y in quiet)}")
    print(f"  (the shouts and the quiet spells: {side})")


if __name__ == "__main__":
    main()
