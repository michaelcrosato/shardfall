import { Vector3 } from 'three';
import { WeightTable } from './skin.ts';

/**
 * Refines part of a closed mesh (a head) toward a target edge length: splits long edges, projects
 * the new vertices onto the field, and evens the triangles out (docs/design/8.3-heads.md). Edges
 * are split, not triangles, so neighbours always agree and no crack opens; the order is the
 * index order, so the result is the same every time.
 */
export interface RefineInput {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  readonly table: WeightTable;
  /** 1 for vertices in the region: a triangle is in it when all three of its vertices are. */
  readonly mask: Uint8Array;
  /** Longest edge to leave (metres). */
  readonly target: number;
  /** A shorter target near small details, at a point (metres); `target` elsewhere. */
  limitAt?(x: number, y: number, z: number): number;
  /** Moves `p` onto the surface and writes the normal there. */
  project(p: Vector3, normal: Vector3): void;
  /** Most passes (each halves the long edges). */
  readonly passes?: number;
}

export interface RefineResult {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  readonly table: WeightTable;
  readonly mask: Uint8Array;
  /** Passes run, and edges still over the limit after them. */
  readonly passes: number;
  readonly long: number;
}

/** Splits an edge when it is longer than this many target lengths. */
export const SPLIT = 1.4;

/** A growable typed array. */
export class Grow<T extends Float32Array | Uint32Array | Int32Array | Uint8Array> {
  data: T;
  length: number;
  private readonly make: (n: number) => T;
  constructor(initial: T, make: (n: number) => T) {
    this.make = make;
    this.data = make(Math.max(16, initial.length * 2));
    this.data.set(initial);
    this.length = initial.length;
  }
  reserve(extra: number): void {
    if (this.length + extra <= this.data.length) return;
    const next = this.make(Math.max(this.data.length * 2, this.length + extra));
    next.set(this.data.subarray(0, this.length));
    this.data = next;
  }
  push(...values: number[]): void {
    this.reserve(values.length);
    for (const v of values) this.data[this.length++] = v;
  }
  view(): T {
    return this.data.slice(0, this.length) as T;
  }
}

export function refineRegion(input: RefineInput): RefineResult {
  const SLOTS = input.table.bones.length / input.table.count;
  const pos = new Grow(input.positions, (n) => new Float32Array(n));
  const nrm = new Grow(input.normals, (n) => new Float32Array(n));
  const mask = new Grow(input.mask, (n) => new Uint8Array(n));
  const wb = new Grow(input.table.bones, (n) => new Int32Array(n).fill(-1));
  const ww = new Grow(input.table.weights, (n) => new Float32Array(n));
  // Only triangles touching the region can gain a split edge (a split edge's ends are both in
  // it): the rest are set aside untouched.
  const fixed: number[] = [];
  const touching: number[] = [];
  for (let t = 0; t < input.indices.length; t += 3) {
    const a = input.indices[t] as number;
    const b = input.indices[t + 1] as number;
    const c = input.indices[t + 2] as number;
    const list = input.mask[a] || input.mask[b] || input.mask[c] ? touching : fixed;
    list.push(a, b, c);
  }
  let idx: Uint32Array = new Uint32Array(touching);
  const limit = SPLIT * input.target;
  const limit2 = limit * limit;
  /** Whether an edge is too long for where it is. */
  const tooLong = (a: number, b: number) => {
    const d2 = length2(a, b);
    if (d2 > limit2) return true;
    if (!input.limitAt) return false;
    const P = pos.data;
    const at = input.limitAt(
      ((P[a * 3] as number) + (P[b * 3] as number)) / 2,
      ((P[a * 3 + 1] as number) + (P[b * 3 + 1] as number)) / 2,
      ((P[a * 3 + 2] as number) + (P[b * 3 + 2] as number)) / 2,
    );
    return d2 > (SPLIT * at) ** 2;
  };
  const p = new Vector3();
  const n = new Vector3();
  const maxPasses = input.passes ?? 4;

  const length2 = (a: number, b: number) => {
    const P = pos.data;
    const dx = (P[a * 3] as number) - (P[b * 3] as number);
    const dy = (P[a * 3 + 1] as number) - (P[b * 3 + 1] as number);
    const dz = (P[a * 3 + 2] as number) - (P[b * 3 + 2] as number);
    return dx * dx + dy * dy + dz * dz;
  };
  const inRegion = (t: number) =>
    mask.data[idx[t] as number] === 1 &&
    mask.data[idx[t + 1] as number] === 1 &&
    mask.data[idx[t + 2] as number] === 1;
  /** The averaged weights of a and b into a new vertex's slots (top weights kept). */
  const addWeights = (a: number, b: number) => {
    const bones: number[] = [];
    const weights: number[] = [];
    for (const v of [a, b])
      for (let s = 0; s < SLOTS; s++) {
        const bone = wb.data[v * SLOTS + s] as number;
        const w = ww.data[v * SLOTS + s] as number;
        if (bone < 0 || w <= 0) continue;
        const i = bones.indexOf(bone);
        if (i >= 0) weights[i] = (weights[i] as number) + w / 2;
        else {
          bones.push(bone);
          weights.push(w / 2);
        }
      }
    const order = bones.map((_, i) => i);
    order.sort(
      (x, y) =>
        (weights[y] as number) - (weights[x] as number) ||
        (bones[x] as number) - (bones[y] as number),
    );
    const kept = order.slice(0, SLOTS);
    const total = kept.reduce((s, i) => s + (weights[i] as number), 0) || 1;
    wb.reserve(SLOTS);
    ww.reserve(SLOTS);
    for (let s = 0; s < SLOTS; s++) {
      const i = kept[s];
      wb.data[wb.length + s] = i === undefined ? -1 : (bones[i] as number);
      ww.data[ww.length + s] = i === undefined ? 0 : (weights[i] as number) / total;
    }
    wb.length += SLOTS;
    ww.length += SLOTS;
  };

  let passes = 0;
  for (; passes < maxPasses; passes++) {
    const count = pos.length / 3;
    const mids = new Map<number, number>();
    const key = (a: number, b: number) => (a < b ? a * count + b : b * count + a);
    for (let t = 0; t < idx.length; t += 3) {
      if (!inRegion(t)) continue;
      for (let e = 0; e < 3; e++) {
        const a = idx[t + e] as number;
        const b = idx[t + (e === 2 ? 0 : e + 1)] as number;
        if (!tooLong(a, b)) continue;
        const k = key(a, b);
        if (mids.has(k)) continue;
        mids.set(k, pos.length / 3);
        const P = pos.data;
        p.set(
          ((P[a * 3] as number) + (P[b * 3] as number)) / 2,
          ((P[a * 3 + 1] as number) + (P[b * 3 + 1] as number)) / 2,
          ((P[a * 3 + 2] as number) + (P[b * 3 + 2] as number)) / 2,
        );
        input.project(p, n);
        pos.push(p.x, p.y, p.z);
        nrm.push(n.x, n.y, n.z);
        mask.push(1);
        addWeights(a, b);
      }
    }
    if (mids.size === 0) break;
    idx = splitTriangles(idx, (a, b) => mids.get(key(a, b)) ?? -1, length2);
  }
  if (passes > 0)
    relaxRegion(pos.data, nrm.data, pos.length / 3, idx, mask.data, input.project, p, n);
  let long = 0;
  for (let t = 0; t < idx.length; t += 3) {
    if (!inRegion(t)) continue;
    for (let e = 0; e < 3; e++)
      if (tooLong(idx[t + e] as number, idx[t + (e === 2 ? 0 : e + 1)] as number)) long++;
  }
  const table = new WeightTable(pos.length / 3);
  table.bones.set(wb.data.subarray(0, wb.length));
  table.weights.set(ww.data.subarray(0, ww.length));
  const indices = new Uint32Array(fixed.length + idx.length);
  indices.set(fixed);
  indices.set(idx, fixed.length);
  return {
    positions: pos.view(),
    normals: nrm.view(),
    indices,
    table,
    mask: mask.view(),
    passes,
    long,
  };
}

/**
 * Re-triangulates every triangle with split edges (`mid(a, b)` is the new vertex on an edge, or
 * -1), keeping each triangle's winding: one split gives two triangles, two give three (the quad
 * cut along its shorter diagonal), three give four.
 */
export function splitTriangles(
  idx: Uint32Array,
  mid: (a: number, b: number) => number,
  length2: (a: number, b: number) => number,
): Uint32Array {
  const next = new Grow(new Uint32Array(0), (size) => new Uint32Array(size));
  next.reserve(idx.length * 2);
  const m = [-1, -1, -1];
  const v = [0, 0, 0];
  for (let t = 0; t < idx.length; t += 3) {
    let split = 0;
    for (let e = 0; e < 3; e++) v[e] = idx[t + e] as number;
    for (let e = 0; e < 3; e++) {
      m[e] = mid(v[e] as number, v[e === 2 ? 0 : e + 1] as number);
      if ((m[e] as number) >= 0) split++;
    }
    if (split === 0) {
      next.push(v[0] as number, v[1] as number, v[2] as number);
      continue;
    }
    if (split === 3) {
      const [a, b, c] = v as [number, number, number];
      const [ab, bc, ca] = m as [number, number, number];
      next.push(a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca);
      continue;
    }
    // Rotate so that edge (a, b) is split (and, with two, also (b, c)).
    let r = (m[0] as number) >= 0 ? 0 : (m[1] as number) >= 0 ? 1 : 2;
    if (split === 2 && (m[(r + 2) % 3] as number) >= 0) r = (r + 2) % 3;
    const a = v[r] as number;
    const b = v[(r + 1) % 3] as number;
    const c = v[(r + 2) % 3] as number;
    const ab = m[r] as number;
    if (split === 1) {
      next.push(a, ab, c, ab, b, c);
      continue;
    }
    // A corner triangle at b, and the quad (a, ab, bc, c) cut along its shorter diagonal.
    const bc = m[(r + 1) % 3] as number;
    next.push(ab, b, bc);
    if (length2(a, bc) <= length2(ab, c)) next.push(a, ab, bc, a, bc, c);
    else next.push(a, ab, c, ab, bc, c);
  }
  return next.view();
}

/**
 * Moves each vertex whose triangles are all in the region halfway to its neighbours' centroid
 * within its tangent plane, then back onto the surface: projection bunches vertices where the
 * surface curves, and this spreads them out again.
 */
function relaxRegion(
  pos: Float32Array,
  nrm: Float32Array,
  count: number,
  idx: Uint32Array,
  mask: Uint8Array,
  project: (p: Vector3, n: Vector3) => void,
  p: Vector3,
  n: Vector3,
): void {
  const inside = mask.slice(0, count);
  const sum = new Float64Array(count * 3);
  const deg = new Uint32Array(count);
  for (let t = 0; t < idx.length; t += 3) {
    const a = idx[t] as number;
    const b = idx[t + 1] as number;
    const c = idx[t + 2] as number;
    if (!(mask[a] && mask[b] && mask[c])) {
      inside[a] = 0;
      inside[b] = 0;
      inside[c] = 0;
    }
    for (const [x, y] of [
      [a, b],
      [b, c],
      [c, a],
    ] as const) {
      for (let k = 0; k < 3; k++) {
        sum[x * 3 + k] = (sum[x * 3 + k] as number) + (pos[y * 3 + k] as number);
        sum[y * 3 + k] = (sum[y * 3 + k] as number) + (pos[x * 3 + k] as number);
      }
      deg[x] = (deg[x] as number) + 1;
      deg[y] = (deg[y] as number) + 1;
    }
  }
  const d = new Vector3();
  for (let v = 0; v < count; v++) {
    const k = deg[v] as number;
    if (!inside[v] || k === 0) continue;
    p.set(pos[v * 3] as number, pos[v * 3 + 1] as number, pos[v * 3 + 2] as number);
    n.set(nrm[v * 3] as number, nrm[v * 3 + 1] as number, nrm[v * 3 + 2] as number);
    d.set(
      (sum[v * 3] as number) / k - p.x,
      (sum[v * 3 + 1] as number) / k - p.y,
      (sum[v * 3 + 2] as number) / k - p.z,
    );
    d.addScaledVector(n, -d.dot(n));
    p.addScaledVector(d, 0.5);
    project(p, n);
    pos[v * 3] = p.x;
    pos[v * 3 + 1] = p.y;
    pos[v * 3 + 2] = p.z;
    nrm[v * 3] = n.x;
    nrm[v * 3 + 1] = n.y;
    nrm[v * 3 + 2] = n.z;
  }
}
