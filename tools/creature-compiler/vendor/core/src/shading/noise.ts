import type { Kit } from './kit.ts';

/** Value noise on the integer lattice, smoothly interpolated, in [0, 1). */
export function valueNoise<F>(k: Kit<F>, x: F, y: F, z: F, salt = 0): F {
  const ix = k.floor(x);
  const iy = k.floor(y);
  const iz = k.floor(z);
  const fx = k.sub(x, ix);
  const fy = k.sub(y, iy);
  const fz = k.sub(z, iz);
  const smooth = (f: F) => k.mul(k.mul(f, f), k.sub(k.num(3), k.mul(k.num(2), f)));
  const ux = smooth(fx);
  const uy = smooth(fy);
  const uz = smooth(fz);
  const one = k.num(1);
  const h = (dx: number, dy: number, dz: number) =>
    k.hash3(dx ? k.add(ix, one) : ix, dy ? k.add(iy, one) : iy, dz ? k.add(iz, one) : iz, salt);
  const x00 = k.mix(h(0, 0, 0), h(1, 0, 0), ux);
  const x10 = k.mix(h(0, 1, 0), h(1, 1, 0), ux);
  const x01 = k.mix(h(0, 0, 1), h(1, 0, 1), ux);
  const x11 = k.mix(h(0, 1, 1), h(1, 1, 1), ux);
  return k.mix(k.mix(x00, x10, uy), k.mix(x01, x11, uy), uz);
}

/** Fractal value noise: `octaves` layers, each twice the frequency and half the weight. */
export function fbm<F>(k: Kit<F>, x: F, y: F, z: F, octaves = 3, salt = 0): F {
  let sum = k.num(0);
  let amp = 0.5;
  let norm = 0;
  let fx = x;
  let fy = y;
  let fz = z;
  for (let o = 0; o < octaves; o++) {
    sum = k.add(sum, k.mul(valueNoise(k, fx, fy, fz, salt + o * 7), k.num(amp)));
    norm += amp;
    amp *= 0.5;
    const two = k.num(2.03);
    fx = k.mul(fx, two);
    fy = k.mul(fy, two);
    fz = k.mul(fz, two);
  }
  return k.div(sum, k.num(norm));
}

export interface Cell<F> {
  /** Distance to the nearest feature point, in cells. */
  readonly distance: F;
  /** Distance to the second nearest, for edges between cells. */
  readonly second: F;
  /** A random value per cell in [0, 1). */
  readonly id: F;
  /** From the point to the nearest feature point, in cells. */
  readonly dx: F;
  readonly dy: F;
  readonly dz: F;
}

/**
 * Cellular (Worley) noise with one feature point per cell, jittered by up to `jitter` (0 to 1)
 * of half a cell around its centre. `reach` 2 checks the 2×2×2 cells nearest the point, which
 * finds the nearest feature but not always the second nearest; use 3 (27 cells) where edges
 * between cells matter, as in scales. With `stagger`, every other row along z is shifted half a
 * cell in x, so the cells pack like scales on a surface facing y; `staggerY` shifts the rows in y
 * too, so surfaces facing x (the flanks) see offset rows as well.
 */
export function cells<F>(
  k: Kit<F>,
  x: F,
  y: F,
  z: F,
  jitter: number,
  salt = 0,
  options: { stagger?: boolean; staggerY?: boolean; reach?: 2 | 3 } = {},
): Cell<F> {
  const half = k.num(0.5);
  const reach = options.reach ?? 2;
  const lo = reach === 3 ? -1 : 0;
  const hi = reach === 3 ? 1 : 1;
  // With reach 3, centre on the cell containing the point; with reach 2, on the nearest corner.
  const centre = (v: F) => (reach === 3 ? k.floor(v) : k.floor(k.sub(v, half)));
  const plainBy = centre(y);
  const bz = centre(z);
  let best = k.num(9);
  let second = k.num(9);
  let id = k.num(0);
  let ox = k.num(0);
  let oy = k.num(0);
  let oz = k.num(0);
  const j = k.num(jitter * 0.5);
  for (let dz = lo; dz <= hi; dz++) {
    const cz = dz === 0 ? bz : k.add(bz, k.num(dz));
    const shift = options.stagger ? k.fract(k.mul(cz, half)) : k.num(0);
    const shiftY = options.stagger && options.staggerY ? shift : undefined;
    const bx = centre(k.sub(x, shift));
    const by = shiftY ? centre(k.sub(y, shiftY)) : plainBy;
    for (let dy = lo; dy <= hi; dy++) {
      const cy = dy === 0 ? by : k.add(by, k.num(dy));
      for (let dx = lo; dx <= hi; dx++) {
        const cx = dx === 0 ? bx : k.add(bx, k.num(dx));
        const jx = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 11), half), j);
        const jy = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 23), half), j);
        const jz = k.mul(k.sub(k.hash3(cx, cy, cz, salt + 37), half), j);
        const ddx = k.sub(k.add(k.add(k.add(cx, half), shift), jx), x);
        const fy = k.add(k.add(cy, half), jy);
        const ddy = k.sub(shiftY ? k.add(fy, shiftY) : fy, y);
        const ddz = k.sub(k.add(k.add(cz, half), jz), z);
        const d = k.sqrt(k.add(k.add(k.mul(ddx, ddx), k.mul(ddy, ddy)), k.mul(ddz, ddz)));
        const closer = k.step(d, best);
        second = k.min(second, k.max(d, best));
        best = k.min(best, d);
        id = k.mix(id, k.hash3(cx, cy, cz, salt + 53), closer);
        ox = k.mix(ox, ddx, closer);
        oy = k.mix(oy, ddy, closer);
        oz = k.mix(oz, ddz, closer);
      }
    }
  }
  return { distance: best, second, id, dx: ox, dy: oy, dz: oz };
}
