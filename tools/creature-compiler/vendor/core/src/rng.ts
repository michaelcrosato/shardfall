/**
 * Seeded random numbers. Everything random in Spawnforge comes from here, so the same blueprint
 * and seed give the same creature every time, in Node and in every browser.
 *
 * A generator forks named child streams with `rng.stream(key)`. A child depends only on its
 * parent's seed and its key, never on how many numbers the parent has drawn, so keying streams
 * by part id means changing the horns never reshuffles the spots.
 */
export interface Rng {
  /** The 32-bit seed this generator started from. */
  readonly seed: number;
  /** Uniform float in [0, 1). */
  next(): number;
  /** Uniform float in [min, max). */
  float(min: number, max: number): number;
  /** Uniform integer in [min, max], both ends included. */
  int(min: number, max: number): number;
  /** True with probability `p`. */
  chance(p: number): boolean;
  /** One element of a non-empty list, chosen uniformly. */
  pick<T>(items: readonly T[]): T;
  /** An independent generator for `key`, such as a part id. */
  stream(key: string): Rng;
}

const TWO_POW_32 = 0x1_0000_0000;

export function createRng(seed: number): Rng {
  if (!Number.isInteger(seed)) {
    throw new RangeError(`seed must be an integer, got ${seed}`);
  }
  const root = seed >>> 0;

  // sfc32, with its 128-bit state expanded from the seed by splitmix32 so nearby seeds diverge.
  let s = root;
  const splitmix32 = (): number => {
    s = (s + 0x9e3779b9) | 0;
    let t = s ^ (s >>> 16);
    t = Math.imul(t, 0x21f0aaad);
    t ^= t >>> 15;
    t = Math.imul(t, 0x735a2d97);
    return (t ^ (t >>> 15)) >>> 0;
  };
  let a = splitmix32();
  let b = splitmix32();
  let c = splitmix32();
  let d = splitmix32();

  const next = (): number => {
    const t = (((a + b) | 0) + d) | 0;
    d = (d + 1) | 0;
    a = b ^ (b >>> 9);
    b = (c + (c << 3)) | 0;
    c = (c << 21) | (c >>> 11);
    c = (c + t) | 0;
    return (t >>> 0) / TWO_POW_32;
  };

  return {
    seed: root,
    next,
    float: (min, max) => min + (max - min) * next(),
    int(min, max) {
      if (!Number.isInteger(min) || !Number.isInteger(max) || max < min) {
        throw new RangeError(`int(min, max) needs integers with min <= max, got ${min}, ${max}`);
      }
      return min + Math.floor(next() * (max - min + 1));
    },
    chance: (p) => next() < p,
    pick(items) {
      if (items.length === 0) throw new RangeError('pick() needs a non-empty list');
      return items[Math.floor(next() * items.length)] as (typeof items)[number];
    },
    stream: (key) => createRng(deriveSeed(root, key)),
  };
}

/** Mixes a seed and a string key into a new 32-bit seed (FNV-1a, then the murmur3 finalizer). */
export function deriveSeed(seed: number, key: string): number {
  let h = (0x811c9dc5 ^ seed) >>> 0;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
