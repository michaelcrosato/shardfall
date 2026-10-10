import { Vector3 } from 'three';
import type { BoneDef } from './types.ts';

/** Up to this many (bone, weight) slots per vertex while weights are being worked out. */
const SLOTS = 8;
/** Softness of joints: higher is sharper. */
const SHARPNESS = 6;

/** Distance from a point to a bone's rounded-cone shape (approximate, cheap). */
export function boneDistance(bone: BoneDef, p: Vector3): { distance: number; t: number } {
  const distance = boneDistanceXYZ(bone, p.x, p.y, p.z);
  return { distance, t: lastT };
}

let lastT = 0;

/** Allocation-free bone distance; the bone parameter of the closest point is in `lastBoneT()`. */
export function boneDistanceXYZ(bone: BoneDef, px: number, py: number, pz: number): number {
  const hx = bone.head.x;
  const hy = bone.head.y;
  const hz = bone.head.z;
  const ax = bone.tail.x - hx;
  const ay = bone.tail.y - hy;
  const az = bone.tail.z - hz;
  const len2 = ax * ax + ay * ay + az * az;
  let t = len2 > 1e-12 ? ((px - hx) * ax + (py - hy) * ay + (pz - hz) * az) / len2 : 0;
  t = t < 0 ? 0 : t > 1 ? 1 : t;
  lastT = t;
  const dx = px - (hx + ax * t);
  const dy = py - (hy + ay * t);
  const dz = pz - (hz + az * t);
  const r = bone.r0 + (bone.r1 - bone.r0) * t;
  const d = Math.sqrt(dx * dx + dy * dy + dz * dz) - r;
  return d > 0 ? d : 0;
}

/** Bone parameter (0 head, 1 tail) of the closest point from the last `boneDistanceXYZ` call. */
export function lastBoneT(): number {
  return lastT;
}

/** Sparse per-vertex weights in fixed slots. */
export class WeightTable {
  readonly count: number;
  readonly bones: Int32Array;
  readonly weights: Float32Array;
  constructor(count: number) {
    this.count = count;
    this.bones = new Int32Array(count * SLOTS).fill(-1);
    this.weights = new Float32Array(count * SLOTS);
  }

  add(v: number, bone: number, w: number): void {
    const o = v * SLOTS;
    let empty = -1;
    let smallest = -1;
    for (let s = 0; s < SLOTS; s++) {
      const b = this.bones[o + s] as number;
      if (b === bone) {
        this.weights[o + s] = (this.weights[o + s] as number) + w;
        return;
      }
      if (b < 0 && empty < 0) empty = s;
      if (smallest < 0 || (this.weights[o + s] as number) < (this.weights[o + smallest] as number))
        smallest = s;
    }
    const slot = empty >= 0 ? empty : smallest;
    if (empty < 0 && (this.weights[o + slot] as number) >= w) return;
    this.bones[o + slot] = bone;
    this.weights[o + slot] = w;
  }

  set(v: number, entries: readonly (readonly [number, number])[]): void {
    const o = v * SLOTS;
    this.bones.fill(-1, o, o + SLOTS);
    this.weights.fill(0, o, o + SLOTS);
    for (const [b, w] of entries) this.add(v, b, w);
  }

  entries(v: number): [number, number][] {
    const out: [number, number][] = [];
    const o = v * SLOTS;
    for (let s = 0; s < SLOTS; s++) {
      const b = this.bones[o + s] as number;
      if (b >= 0 && (this.weights[o + s] as number) > 0)
        out.push([b, this.weights[o + s] as number]);
    }
    return out;
  }

  normalize(v: number): void {
    const o = v * SLOTS;
    let sum = 0;
    for (let s = 0; s < SLOTS; s++) sum += this.weights[o + s] as number;
    if (sum <= 0) return;
    for (let s = 0; s < SLOTS; s++) this.weights[o + s] = (this.weights[o + s] as number) / sum;
  }
}

export interface WeightOptions {
  readonly bones: readonly BoneDef[];
  /** Children of each bone. */
  readonly children: readonly (readonly number[])[];
  /** Bones near a point worth testing (e.g. from the SDF block culling). */
  readonly nearbyBones: (p: Vector3) => Iterable<number>;
}

/**
 * Candidate weights at one point: nearest bone, its parent and children, softmax of d/r.
 * With `allowed`, only those bones are considered (a part follows what it is attached to).
 */
export function weightsAt(
  p: Vector3,
  options: WeightOptions,
  allowed?: ReadonlySet<number>,
): [number, number][] {
  const { bones, children } = options;
  let nearest = -1;
  let best = Number.POSITIVE_INFINITY;
  for (const id of allowed ?? options.nearbyBones(p)) {
    const bone = bones[id] as BoneDef;
    if (!bone.skin) continue;
    const score = boneDistanceXYZ(bone, p.x, p.y, p.z) / Math.max(1e-6, (bone.r0 + bone.r1) / 2);
    // Near-ties (two bones meeting at a joint) keep the first, so a last-bit difference in the
    // trigonometry between engines never picks another bone.
    if (score < best - 1e-9) {
      best = score;
      nearest = id;
    }
  }
  if (nearest < 0) return [];
  const candidates = [nearest];
  const ok = (id: number) => (bones[id] as BoneDef).skin && (!allowed || allowed.has(id));
  const parent = (bones[nearest] as BoneDef).parent;
  if (parent >= 0 && ok(parent)) candidates.push(parent);
  for (const c of children[nearest] ?? []) if (ok(c)) candidates.push(c);
  const raw: [number, number][] = [];
  let sum = 0;
  for (const id of candidates) {
    const bone = bones[id] as BoneDef;
    const distance = boneDistanceXYZ(bone, p.x, p.y, p.z);
    const w = Math.exp((-SHARPNESS * distance) / Math.max(1e-6, (bone.r0 + bone.r1) / 2));
    raw.push([id, w]);
    sum += w;
  }
  for (const e of raw) e[1] /= sum;
  return raw;
}

/** Skin weights for a mesh: per-vertex candidates, smoothed over the mesh. */
export function computeWeights(
  positions: Float32Array,
  indices: Uint32Array,
  options: WeightOptions,
  smoothing = 2,
): WeightTable {
  const n = positions.length / 3;
  const table = new WeightTable(n);
  const p = new Vector3();
  for (let v = 0; v < n; v++) {
    p.set(
      positions[v * 3] as number,
      positions[v * 3 + 1] as number,
      positions[v * 3 + 2] as number,
    );
    table.set(v, weightsAt(p, options));
  }
  if (smoothing > 0) {
    const neighbours: number[][] = Array.from({ length: n }, () => []);
    for (let t = 0; t < indices.length; t += 3) {
      for (let e = 0; e < 3; e++) {
        const a = indices[t + e] as number;
        const b = indices[t + ((e + 1) % 3)] as number;
        (neighbours[a] as number[]).push(b);
      }
    }
    // Half of vertex `u`'s weights, shared among `share` neighbours, onto vertex `v` of `to`.
    // It reads the slots in place, as `entries` lists them, without building lists.
    const spread = (to: WeightTable, v: number, from: WeightTable, u: number, share: number) => {
      const o = u * SLOTS;
      for (let s = 0; s < SLOTS; s++) {
        const b = from.bones[o + s] as number;
        const w = from.weights[o + s] as number;
        if (b >= 0 && w > 0) to.add(v, b, (w * 0.5) / share);
      }
    };
    for (let it = 0; it < smoothing; it++) {
      const next = new WeightTable(n);
      for (let v = 0; v < n; v++) {
        spread(next, v, table, v, 1);
        const nb = neighbours[v] as number[];
        if (nb.length === 0) {
          spread(next, v, table, v, 1);
          continue;
        }
        for (const u of nb) spread(next, v, table, u, nb.length);
        next.normalize(v);
      }
      table.bones.set(next.bones);
      table.weights.set(next.weights);
    }
  }
  return table;
}

/**
 * Helper bones take half a joint's rotation. Where a vertex is weighted to both bones of a
 * joint, part of that weight moves to the joint's helper, which keeps bent joints from
 * collapsing.
 */
export function applyHelpers(
  table: WeightTable,
  helpers: readonly (readonly [number, number, number])[],
): void {
  if (helpers.length === 0) return;
  // Bones that are the upper bone of some joint: a vertex weighted to none of them is left alone,
  // without listing its weights.
  const uppers = new Set(helpers.map(([, upper]) => upper));
  for (let v = 0; v < table.count; v++) {
    let any = false;
    for (let s = v * SLOTS; s < (v + 1) * SLOTS && !any; s++)
      if ((table.weights[s] as number) > 0 && uppers.has(table.bones[s] as number)) any = true;
    if (!any) continue;
    const entries = table.entries(v);
    let changed = false;
    for (const [helper, upper, lower] of helpers) {
      const wu = entries.find((e) => e[0] === upper);
      const wl = entries.find((e) => e[0] === lower);
      if (!wu || !wl) continue;
      const shared = Math.min(wu[1], wl[1]);
      if (shared < 0.02) continue;
      wu[1] -= shared * 0.5;
      wl[1] -= shared * 0.5;
      entries.push([helper, shared]);
      changed = true;
    }
    if (changed) table.set(v, entries);
  }
}

/** Packs the four largest weights per vertex, renormalized, for skinning. */
export function packTop4(table: WeightTable): { skinIndex: Uint16Array; skinWeight: Float32Array } {
  const skinIndex = new Uint16Array(table.count * 4);
  const skinWeight = new Float32Array(table.count * 4);
  for (let v = 0; v < table.count; v++) {
    const top = table
      .entries(v)
      .sort((a, b) => b[1] - a[1] || a[0] - b[0])
      .slice(0, 4);
    const sum = top.reduce((a, e) => a + e[1], 0) || 1;
    top.forEach(([b, w], i) => {
      skinIndex[v * 4 + i] = b;
      skinWeight[v * 4 + i] = w / sum;
    });
  }
  return { skinIndex, skinWeight };
}
