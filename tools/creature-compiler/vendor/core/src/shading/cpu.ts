import type { Kit } from './kit.ts';

/** PCG hash step on 32-bit unsigned integers. */
function pcg(v: number): number {
  const state = (Math.imul(v >>> 0, 747796405) + 2891336453) >>> 0;
  const word = Math.imul(((state >>> ((state >>> 28) + 4)) ^ state) >>> 0, 277803737) >>> 0;
  return ((word >>> 22) ^ word) >>> 0;
}

/** Same integer hash as the GPU backend: pcg(x + pcg(y + pcg(z + salt))). */
export function hash3u(x: number, y: number, z: number, salt: number): number {
  const ix = ((x | 0) + 32768) >>> 0;
  const iy = ((y | 0) + 32768) >>> 0;
  const iz = ((z | 0) + 32768) >>> 0;
  return pcg((ix + pcg((iy + pcg((iz + salt) >>> 0)) >>> 0)) >>> 0);
}

const f32 = Math.fround;

/** The CPU backend: plain numbers, rounded to float32 where the GPU would be. */
export const cpuKit: Kit<number> = {
  num: (x) => x,
  param: (x) => x,
  add: (a, b) => a + b,
  sub: (a, b) => a - b,
  mul: (a, b) => a * b,
  div: (a, b) => a / b,
  min: Math.min,
  max: Math.max,
  abs: Math.abs,
  floor: Math.floor,
  fract: (a) => a - Math.floor(a),
  sin: Math.sin,
  cos: Math.cos,
  sqrt: (a) => Math.sqrt(Math.max(0, a)),
  pow: (a, b) => Math.max(0, a) ** b,
  mix: (a, b, t) => a + (b - a) * t,
  clamp: (x, lo, hi) => Math.min(hi, Math.max(lo, x)),
  smoothstep: (e0, e1, x) => {
    const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
    return t * t * (3 - 2 * t);
  },
  step: (edge, x) => (x >= edge ? 1 : 0),
  hash3: (x, y, z, salt) => f32(hash3u(x, y, z, salt)) / 4294967296,
};
