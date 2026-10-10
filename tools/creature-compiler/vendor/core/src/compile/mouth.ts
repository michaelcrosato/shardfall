import { Vector3 } from 'three';
import { Grow, splitTriangles } from './refine.ts';
import { WeightTable } from './skin.ts';
import type { BoneDef } from './types.ts';

/** The mouth line in head space, shared by the cut, the inner mouth and the teeth. */
export interface MouthLine {
  /** Skull centre (head bone head). */
  readonly origin: Vector3;
  readonly forward: Vector3;
  readonly up: Vector3;
  readonly side: Vector3;
  /** Distance forward of the origin where the mouth corner and the tip sit. */
  readonly corner: number;
  readonly tip: number;
  /** Height of the line at the corner and the tip (along `up`). */
  readonly cornerY: number;
  readonly tipY: number;
  /** Head half-width across the line at the corner and the tip. */
  readonly cornerHalf: number;
  readonly tipHalf: number;
}

export function mouthLine(head: BoneDef, jaw: BoneDef): MouthLine {
  const forward = new Vector3().subVectors(head.tail, head.head).normalize();
  const up = head.up.clone().addScaledVector(forward, -head.up.dot(forward)).normalize();
  const side = new Vector3().crossVectors(up, forward).normalize();
  const along = (p: Vector3) => new Vector3().subVectors(p, head.head).dot(forward);
  const len = head.head.distanceTo(head.tail);
  const hinge = along(jaw.head);
  const corner = hinge + Math.max(0.15 * len, head.r0 * 0.25);
  const tip = len + head.r1 * 0.85;
  const radiusAt = (z: number) =>
    head.r0 + (head.r1 - head.r0) * Math.min(1, Math.max(0, z / (len || 1)));
  return {
    origin: head.head.clone(),
    forward,
    up,
    side,
    corner,
    tip,
    cornerY: -radiusAt(corner) * head.cross[1] * 0.28,
    tipY: -head.r1 * head.cross[1] * 0.3,
    cornerHalf: radiusAt(corner) * head.cross[0] * 0.8,
    tipHalf: head.r1 * head.cross[0] * 0.55,
  };
}

/** The cut's height (along `up`, from the origin) at `z` along the head. */
export function lineY(m: MouthLine, z: number): number {
  const t = Math.min(1, Math.max(0, (z - m.tip) / (m.corner - m.tip)));
  return m.tipY + (m.cornerY - m.tipY) * t;
}

export interface CutResult {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  readonly table: WeightTable;
  /** The cut's edge, corner to corner: the upper side's vertices and their lower copies. */
  readonly boundary: { readonly upper: readonly number[]; readonly lower: readonly number[] };
}

/**
 * Cuts the closed head mesh exactly along the mouth line (docs/design/8.3-heads.md). In front of
 * the mouth corner, edges that cross the cut get a vertex on it, shared by both triangles, so
 * every triangle lies on one side; the vertices on the cut are duplicated, and the lower side's
 * copies and everything below follow the jaw. Behind the corner, the skin below the line keeps
 * some jaw weight, so the opening tapers into the corner instead of tearing.
 */
export function cutMouth(
  positions: Float32Array,
  normals: Float32Array,
  indices: Uint32Array,
  table: WeightTable,
  head: number,
  jaw: number,
  m: MouthLine,
  headRadius: number,
  /**
   * The jaw's distance less the head's at a point: negative where the jaw's own shape is the
   * skin (a chin that reaches past the snout), which goes with the jaw even above the line.
   */
  jawSide: (x: number, y: number, z: number) => number = () => 1,
): CutResult {
  const SLOTS = table.bones.length / table.count;
  const pos = new Grow(positions, (n) => new Float32Array(n));
  const nrm = new Grow(normals, (n) => new Float32Array(n));
  const wb = new Grow(table.bones, (n) => new Int32Array(n).fill(-1));
  const ww = new Grow(table.weights, (n) => new Float32Array(n));
  const n0 = positions.length / 3;
  const local = (v: number) => {
    const P = pos.data;
    const x = (P[v * 3] as number) - m.origin.x;
    const y = (P[v * 3 + 1] as number) - m.origin.y;
    const z = (P[v * 3 + 2] as number) - m.origin.z;
    return {
      z: x * m.forward.x + y * m.forward.y + z * m.forward.z,
      y: x * m.up.x + y * m.up.y + z * m.up.z,
    };
  };
  const heady = (v: number) => {
    let w = 0;
    for (let s = 0; s < SLOTS; s++) {
      const b = wb.data[v * SLOTS + s] as number;
      if (b === head || b === jaw) w += ww.data[v * SLOTS + s] as number;
    }
    return w > 0.5;
  };
  const eps = 1e-4 * headRadius;
  const inMouth = new Grow(new Uint8Array(n0), (n) => new Uint8Array(n));
  const cut = new Grow(new Float32Array(n0), (n) => new Float32Array(n));
  for (let v = 0; v < n0; v++) {
    const l = local(v);
    if (!(l.z > m.corner && heady(v))) continue;
    inMouth.data[v] = 1;
    const P = pos.data;
    const c = Math.min(
      l.y - lineY(m, l.z),
      jawSide(P[v * 3] as number, P[v * 3 + 1] as number, P[v * 3 + 2] as number),
    );
    cut.data[v] = Math.abs(c) < eps ? 0 : c;
  }
  const inTri = (t: number) =>
    inMouth.data[indices[t] as number] === 1 &&
    inMouth.data[indices[t + 1] as number] === 1 &&
    inMouth.data[indices[t + 2] as number] === 1;

  // 1. A vertex where each edge crosses the cut.
  const mids = new Map<number, number>();
  const key = (a: number, b: number) => (a < b ? a * n0 + b : b * n0 + a);
  for (let t = 0; t < indices.length; t += 3) {
    if (!inTri(t)) continue;
    for (let e = 0; e < 3; e++) {
      const a = indices[t + e] as number;
      const b = indices[t + (e === 2 ? 0 : e + 1)] as number;
      const ca = cut.data[a] as number;
      const cb = cut.data[b] as number;
      if (!(ca * cb < 0) || mids.has(key(a, b))) continue;
      const s = ca / (ca - cb);
      const v = pos.length / 3;
      mids.set(key(a, b), v);
      const P = pos.data;
      const N = nrm.data;
      const p = [0, 1, 2].map(
        (k) => (P[a * 3 + k] as number) * (1 - s) + (P[b * 3 + k] as number) * s,
      );
      const nn = [0, 1, 2].map(
        (k) => (N[a * 3 + k] as number) * (1 - s) + (N[b * 3 + k] as number) * s,
      );
      const len = Math.hypot(nn[0] as number, nn[1] as number, nn[2] as number) || 1;
      pos.push(p[0] as number, p[1] as number, p[2] as number);
      nrm.push((nn[0] as number) / len, (nn[1] as number) / len, (nn[2] as number) / len);
      inMouth.push(1);
      cut.push(0);
      lerpWeights(wb, ww, SLOTS, a, b, s);
    }
  }
  const length2 = (a: number, b: number) => {
    const P = pos.data;
    const dx = (P[a * 3] as number) - (P[b * 3] as number);
    const dy = (P[a * 3 + 1] as number) - (P[b * 3 + 1] as number);
    const dz = (P[a * 3 + 2] as number) - (P[b * 3 + 2] as number);
    return dx * dx + dy * dy + dz * dz;
  };
  const tris =
    mids.size > 0
      ? splitTriangles(indices, (a, b) => mids.get(key(a, b)) ?? -1, length2)
      : indices.slice();
  const n1 = pos.length / 3;

  // 2. Sides: a mouth triangle is lower when it lies below the cut (no triangle straddles it now).
  const triLower = new Uint8Array(tris.length / 3);
  const usedLower = new Uint8Array(n1);
  const usedUpper = new Uint8Array(n1);
  const usedOther = new Uint8Array(n1);
  const edgeSides = new Map<number, number>();
  const key1 = (a: number, b: number) => (a < b ? a * n1 + b : b * n1 + a);
  for (let t = 0; t < tris.length; t += 3) {
    const a = tris[t] as number;
    const b = tris[t + 1] as number;
    const c = tris[t + 2] as number;
    const mouth = inMouth.data[a] && inMouth.data[b] && inMouth.data[c];
    const lower =
      mouth && (cut.data[a] as number) + (cut.data[b] as number) + (cut.data[c] as number) < 0;
    triLower[t / 3] = lower ? 1 : 0;
    if (!mouth) {
      usedOther[a] = 1;
      usedOther[b] = 1;
      usedOther[c] = 1;
      continue;
    }
    for (const v of [a, b, c]) {
      if (lower) usedLower[v] = 1;
      else usedUpper[v] = 1;
    }
    for (const [x, y] of [
      [a, b],
      [b, c],
      [c, a],
    ] as const) {
      if (cut.data[x] !== 0 || cut.data[y] !== 0) continue;
      const k = key1(x, y);
      edgeSides.set(k, (edgeSides.get(k) ?? 0) | (lower ? 1 : 2));
    }
  }

  // 3. Duplicate what both sides of the mouth use (the cut's vertices); lower triangles take
  // the copies. Where skin outside the mouth meets the cut (the corners) nothing splits: the
  // opening ends there, and the skin behind stretches with the jaw.
  const copyOf = new Int32Array(n1).fill(-1);
  const lowerVert = new Grow(new Uint8Array(n1), (n) => new Uint8Array(n));
  for (let v = 0; v < n1; v++) {
    if (usedLower[v] && !usedUpper[v]) lowerVert.data[v] = 1;
    if (!(usedLower[v] && usedUpper[v]) || usedOther[v]) continue;
    copyOf[v] = pos.length / 3;
    const P = pos.data;
    const N = nrm.data;
    pos.push(P[v * 3] as number, P[v * 3 + 1] as number, P[v * 3 + 2] as number);
    nrm.push(N[v * 3] as number, N[v * 3 + 1] as number, N[v * 3 + 2] as number);
    wb.push(...Array.from(wb.data.subarray(v * SLOTS, v * SLOTS + SLOTS)));
    ww.push(...Array.from(ww.data.subarray(v * SLOTS, v * SLOTS + SLOTS)));
    lowerVert.push(1);
  }
  const out = tris.slice();
  for (let t = 0; t < tris.length; t += 3) {
    if (!triLower[t / 3]) continue;
    for (let e = 0; e < 3; e++) {
      const c = copyOf[tris[t + e] as number] as number;
      if (c >= 0) out[t + e] = c;
    }
  }

  // 4. Weights: below the cut on the jaw; above it off the jaw; behind the corner the cheek and
  // throat blend toward the jaw, by how far below the line and how near the corner they are.
  const count = pos.length / 3;
  const next = new WeightTable(count);
  next.bones.set(wb.data.subarray(0, count * SLOTS));
  next.weights.set(ww.data.subarray(0, count * SLOTS));
  const cheek = 0.6 * headRadius;
  const ramp = 0.3 * headRadius;
  const yCorner = lineY(m, m.corner);
  for (let v = 0; v < count; v++) {
    if (lowerVert.data[v]) {
      next.set(v, [[jaw, 1]]);
      continue;
    }
    // Only skin the jaw reaches changes.
    let touched = false;
    for (let k = 0; k < SLOTS; k++) if (next.bones[v * SLOTS + k] === jaw) touched = true;
    if (!touched && !(inMouth.data[v] || heady(v))) continue;
    const entries = next.entries(v);
    const kept = entries.filter(([b]) => b !== jaw);
    // Above the cut in the mouth follows the head, even where the jaw's shape made the skin
    // (a chin that reaches past the snout).
    let base: [number, number][] = kept.length > 0 ? kept : inMouth.data[v] ? [[head, 1]] : entries;
    const total = base.reduce((s, [, w]) => s + w, 0) || 1;
    base = base.map(([b, w]) => [b, w / total]);
    let share = 0;
    if (!inMouth.data[v] && heady(v)) {
      const l = local(v);
      const behind = m.corner - l.z;
      if (behind > 0) {
        share = smooth(0, ramp, yCorner - l.y) * (1 - smooth(0, cheek, behind));
      }
    }
    next.set(
      v,
      share > 0
        ? [...base.map(([b, w]) => [b, w * (1 - share)] as [number, number]), [jaw, share]]
        : base,
    );
  }

  // 5. The cut's edge as one chain, from the corner on the head's left round to the right.
  const adjacency = new Map<number, number[]>();
  for (const [k, sides] of edgeSides) {
    if (sides !== 3) continue;
    const a = Math.floor(k / n1);
    const b = k - a * n1;
    for (const [x, y] of [
      [a, b],
      [b, a],
    ] as const) {
      const list = adjacency.get(x) ?? [];
      list.push(y);
      adjacency.set(x, list);
    }
  }
  const sideOf = (v: number) => {
    const P = pos.data;
    return (
      ((P[v * 3] as number) - m.origin.x) * m.side.x +
      ((P[v * 3 + 1] as number) - m.origin.y) * m.side.y +
      ((P[v * 3 + 2] as number) - m.origin.z) * m.side.z
    );
  };
  // Walk every chain from an end; keep the longest (stray loops, if any, stay plain cracks).
  const visited = new Set<number>();
  let best: number[] = [];
  const ends = [...adjacency.keys()]
    .filter((v) => adjacency.get(v)?.length === 1)
    .sort((a, b) => sideOf(b) - sideOf(a) || a - b);
  for (const start of ends) {
    if (visited.has(start)) continue;
    const chain = [start];
    visited.add(start);
    for (let at = start; ; ) {
      const nextV = (adjacency.get(at) ?? []).find((x) => !visited.has(x));
      if (nextV === undefined) break;
      chain.push(nextV);
      visited.add(nextV);
      at = nextV;
    }
    if (chain.length > best.length) best = chain;
  }
  if (best.length > 1 && sideOf(best[0] as number) < sideOf(best.at(-1) as number)) best.reverse();
  return {
    positions: pos.view(),
    normals: nrm.view(),
    indices: out,
    table: next,
    boundary: {
      upper: best,
      lower: best.map((v) => ((copyOf[v] as number) >= 0 ? (copyOf[v] as number) : v)),
    },
  };
}

const smooth = (e0: number, e1: number, x: number) => {
  const t = Math.min(1, Math.max(0, (x - e0) / (e1 - e0)));
  return t * t * (3 - 2 * t);
};

/** Appends the weights of a point a fraction `s` of the way from vertex a to vertex b. */
function lerpWeights(
  wb: Grow<Int32Array>,
  ww: Grow<Float32Array>,
  SLOTS: number,
  a: number,
  b: number,
  s: number,
): void {
  const bones: number[] = [];
  const weights: number[] = [];
  for (const [v, f] of [
    [a, 1 - s],
    [b, s],
  ] as const)
    for (let k = 0; k < SLOTS; k++) {
      const bone = wb.data[v * SLOTS + k] as number;
      const w = ww.data[v * SLOTS + k] as number;
      if (bone < 0 || w <= 0) continue;
      const i = bones.indexOf(bone);
      if (i >= 0) weights[i] = (weights[i] as number) + w * f;
      else {
        bones.push(bone);
        weights.push(w * f);
      }
    }
  const order = bones
    .map((_, i) => i)
    .sort(
      (x, y) =>
        (weights[y] as number) - (weights[x] as number) ||
        (bones[x] as number) - (bones[y] as number),
    )
    .slice(0, SLOTS);
  const total = order.reduce((sum, i) => sum + (weights[i] as number), 0) || 1;
  for (let k = 0; k < SLOTS; k++) {
    const i = order[k];
    wb.push(i === undefined ? -1 : (bones[i] as number));
    ww.push(i === undefined ? 0 : (weights[i] as number) / total);
  }
}

/** The inside of a mouth as extra skin: positions, weights, and depth and kind for shading. */
export interface MouthInside {
  readonly positions: number[];
  readonly normals: number[];
  /** Indices: below `base` they are skin vertices (the cut's edge), from `base` the new ones. */
  readonly indices: number[];
  readonly weights: [number, number][][];
  /** Per new vertex: 0 at the lips to 1 at the throat, and 0 cavity, 1 lips and gums, 2 tongue. */
  readonly depth: number[];
  readonly kind: number[];
}

/** How a mouth's inside is shaped, from the blueprint and the head. */
export interface MouthOptions {
  readonly line: MouthLine;
  /** Middle of the corners at the cut's height. */
  readonly centre: Vector3;
  /** `body.head.lips` (0–1). */
  readonly lips: number;
  readonly tongue: 'none' | 'flat' | 'forked';
  /** The head's radius at a point (by its distance along the head). */
  radius(z: number): number;
  /** Room above (upper) or below (lower) the cut at a point, inside the head (metres). */
  room(p: Vector3, up: number): number;
  readonly head: number;
  readonly jaw: number;
}

/** Inset (as a share of the lip thickness T) and lift (in T) of the strip's rings. */
const STRIP = [
  [0, 0],
  [1, 0.02],
  [1.3, 0.8],
  [1.8, 1.2],
] as const;
/** Rings from the gums to the centre of the dome or bowl. */
const DEEP = 3;
/** How close to the centre the deepest ring goes (share of the distance to the edge). */
const INNER = 0.14;
/** Boundary vertices per column of the mouth's inside past the first ring. */
const COLUMN_STEP = 3;
/** Triangles per boundary vertex of the mouth's inside, on one side. */
export const MOUTH_COLUMN = 1 + (2 * (STRIP.length + DEEP) - 2) / COLUMN_STEP;

/**
 * Ring `k` of the strip inside the lips at the cut's edge point `p` (lip thickness T): moved
 * toward the midline at the same distance along the head, and toward the throat near the tip,
 * never more than 45% of the way (so short and narrow snouts keep their rings apart), then lifted
 * into the head (upper, `sign` 1) or jaw (-1) by at most 0.6 of the room there. Ring 2 is the
 * gums, where teeth stand. `flat` is the point before lifting.
 */
export function stripPoint(
  o: Pick<MouthOptions, 'line' | 'centre' | 'room'>,
  p: Vector3,
  T: number,
  k: number,
  sign: number,
): { point: Vector3; flat: Vector3 } {
  const m = o.line;
  const across = m.side.clone().multiplyScalar(-new Vector3().subVectors(p, m.origin).dot(m.side));
  const back = new Vector3().subVectors(o.centre, p);
  back.addScaledVector(m.up, -back.dot(m.up));
  const toward = across.addScaledVector(back, 0.35);
  const reach = 0.45 * toward.length();
  const [inset, lift] = STRIP[k] as readonly [number, number];
  const flat = p.clone().addScaledVector(toward.normalize(), Math.min(reach, inset * T));
  const h = Math.min(lift * T, 0.6 * o.room(onCut(m, flat, 0, 1), sign));
  return { point: onCut(m, flat, h, sign), flat };
}

/** A point moved onto the cut's height at its distance along the head, then lifted. */
function onCut(m: MouthLine, p: Vector3, lift: number, sign: number): Vector3 {
  const d = new Vector3().subVectors(p, m.origin);
  const z = d.dot(m.forward);
  return p.clone().addScaledVector(m.up, lineY(m, z) - d.dot(m.up) + sign * lift);
}

/** The lip thickness at a point of the cut. */
export function lipThickness(o: MouthOptions, z: number): number {
  return (0.04 + 0.2 * o.lips) * o.radius(z);
}

/**
 * The mouth's inside, joined to the cut's edge: a strip on each side (the lip's inner face and
 * the gums), then a dome over the upper side and a bowl under the lower one that close toward
 * the throat, walls at the corners that stretch as the jaw opens, and a tongue on the floor.
 */
export function mouthInside(
  o: MouthOptions,
  boundary: CutResult['boundary'],
  skin: Float32Array,
  base: number,
): MouthInside {
  const m = o.line;
  const out: MouthInside = {
    positions: [],
    normals: [],
    indices: [],
    weights: [],
    depth: [],
    kind: [],
  };
  const n = boundary.upper.length;
  if (n < 2) return out;
  const add = (p: Vector3, bone: number, depth: number, kind: number) => {
    out.positions.push(p.x, p.y, p.z);
    out.normals.push(0, 0, 0);
    out.weights.push([[bone, 1]]);
    out.depth.push(depth);
    out.kind.push(kind);
    return base + out.depth.length - 1;
  };
  const at = (v: number) => new Vector3(skin[v * 3], skin[v * 3 + 1], skin[v * 3 + 2]);
  const zOf = (p: Vector3) => new Vector3().subVectors(p, m.origin).dot(m.forward);
  const rings = STRIP.length + DEEP;
  // Every boundary vertex joins the strip; deeper in, every third column is enough.
  const picks: number[] = [];
  for (let i = 0; i < n - 1; i += COLUMN_STEP) picks.push(i);
  picks.push(n - 1);
  if (picks.length >= 3 && (picks.at(-1) as number) - (picks.at(-2) as number) < 2)
    picks.splice(-2, 1);
  const columns: { upper: number[][]; lower: number[][] } = { upper: [], lower: [] };
  for (const [sign, list, bone, key] of [
    [1, boundary.upper, o.head, 'upper'],
    [-1, boundary.lower, o.jaw, 'lower'],
  ] as const) {
    for (const i of picks) {
      const v = list[i] as number;
      const p = at(v);
      const T = lipThickness(o, zOf(p));
      const column = [v];
      let flat = p;
      for (let k = 1; k < STRIP.length; k++) {
        const ring = stripPoint(o, p, T, k, sign);
        flat = ring.flat;
        column.push(add(ring.point, bone, 0.08 * k, 1));
      }
      // Deeper rings: from the gums to near the throat, rising to the palate's (or floor's)
      // height, at most 0.6 of the room inside the head and jaw there.
      const gum = flat;
      const height0 = Math.min(
        (STRIP.at(-1) as readonly [number, number])[1] * T,
        0.6 * o.room(onCut(m, gum, 0, 1), sign),
      );
      for (let k = 1; k <= DEEP; k++) {
        const f = k / DEEP;
        const q = gum.clone().lerp(o.centre, f * (1 - INNER));
        const room = o.room(onCut(m, q, 0, 1), sign);
        const top = Math.max(height0, 0.6 * room);
        const height = Math.min(
          0.6 * room,
          height0 + (top - height0) * Math.sin((f * Math.PI) / 2),
        );
        column.push(add(onCut(m, q, height, sign), bone, 0.3 + 0.7 * f, 0));
      }
      columns[key].push(column);
    }
  }
  // Each side's centre closes the palate or floor at the throat.
  const centres = {
    upper: add(onCut(m, o.centre, 0.6 * o.room(o.centre, 1), 1), o.head, 1, 0),
    lower: add(onCut(m, o.centre, 0.6 * o.room(o.centre, -1), -1), o.jaw, 1, 0),
  };
  const tri = (a: number, b: number, c: number) => {
    out.indices.push(a, b, c);
  };
  for (const [key, list] of [
    ['upper', boundary.upper],
    ['lower', boundary.lower],
  ] as const) {
    const cols = columns[key];
    const from = out.indices.length;
    let probe = from;
    for (let j = 0; j + 1 < picks.length; j++) {
      const a = cols[j] as number[];
      const b = cols[j + 1] as number[];
      // The cut's edge zips onto the first ring: the run's first half fans to this column's
      // first ring vertex, the second half to the next column's.
      const s0 = picks[j] as number;
      const s1 = picks[j + 1] as number;
      const mid = Math.floor((s0 + s1) / 2);
      if (j === Math.floor((picks.length - 1) / 2)) probe = out.indices.length;
      for (let i = s0; i < s1; i++)
        tri(list[i] as number, list[i + 1] as number, (i < mid ? a[1] : b[1]) as number);
      tri(list[mid] as number, b[1] as number, a[1] as number);
      for (let k = 1; k + 1 < rings; k++) {
        tri(a[k] as number, b[k] as number, b[k + 1] as number);
        tri(a[k] as number, b[k + 1] as number, a[k + 1] as number);
      }
      tri(a[rings - 1] as number, b[rings - 1] as number, centres[key]);
    }
    // The upper side faces down into the mouth and the lower up: judge by the lip's flat strip.
    orient(out, skin, base, from, probe, m.up.clone().multiplyScalar(key === 'upper' ? -1 : 1));
  }
  // Walls at the corners join the upper and lower columns at each end.
  for (const j of [0, picks.length - 1]) {
    const u = columns.upper[j] as number[];
    const l = columns.lower[j] as number[];
    const from = out.indices.length;
    for (let k = 0; k + 1 < rings; k++) {
      // The corner's own vertex is shared by both sides: no triangle between it and itself.
      if (u[k] !== l[k]) tri(u[k] as number, l[k] as number, l[k + 1] as number);
      tri(u[k] as number, l[k + 1] as number, u[k + 1] as number);
    }
    tri(u[rings - 1] as number, l[rings - 1] as number, centres.lower);
    tri(u[rings - 1] as number, centres.lower, centres.upper);
    // A wall stands across the corner, so its inside faces forward, toward the tip. At rest the
    // first rings of the two columns touch; judge by the gums', which do not.
    orient(out, skin, base, from, out.indices.length - 4 * 6 - 6, m.forward);
  }
  if (o.tongue !== 'none') tongue(o, out, base);
  smoothNormals(out, skin, base);
  return out;
}

/** A vertex's position: a skin vertex below `base`, else a new one. */
function vertex(out: MouthInside, skin: Float32Array, base: number, v: number): Vector3 {
  return v < base
    ? new Vector3(skin[v * 3], skin[v * 3 + 1], skin[v * 3 + 2])
    : new Vector3(
        out.positions[(v - base) * 3],
        out.positions[(v - base) * 3 + 1],
        out.positions[(v - base) * 3 + 2],
      );
}

/**
 * Winds the triangles from index `from` on (built with one consistent winding) so that the one
 * at `probe` faces along `want`.
 */
function orient(
  out: MouthInside,
  skin: Float32Array,
  base: number,
  from: number,
  probe: number,
  want: Vector3,
): void {
  const a = vertex(out, skin, base, out.indices[probe] as number);
  const b = vertex(out, skin, base, out.indices[probe + 1] as number);
  const c = vertex(out, skin, base, out.indices[probe + 2] as number);
  const nrm = new Vector3().subVectors(b, a).cross(new Vector3().subVectors(c, a));
  if (nrm.dot(want) >= 0) return;
  for (let t = from; t < out.indices.length; t += 3) {
    const x = out.indices[t + 1] as number;
    out.indices[t + 1] = out.indices[t + 2] as number;
    out.indices[t + 2] = x;
  }
}

/** A tongue on the floor of the lower side, from the throat toward the tip, on the jaw. */
function tongue(o: MouthOptions, out: MouthInside, base: number): void {
  const m = o.line;
  const forked = o.tongue === 'forked';
  const front = m.tip - (m.tip - m.corner) * (forked ? 0.12 : 0.22);
  const back = m.corner - (m.tip - m.corner) * 0.05;
  const width = (forked ? 0.32 : 0.55) * m.cornerHalf;
  const thick = 0.35 * width * (forked ? 0.8 : 1);
  const floorAt = (z: number) => {
    const flat = m.origin.clone().addScaledVector(m.forward, z);
    flat.addScaledVector(m.up, lineY(m, z));
    const room = o.room(flat, -1);
    // Rest it a little above the bowl's lowest point.
    return lineY(m, z) - Math.min(0.45 * room, 0.6 * room - 0.5 * thick);
  };
  const steps = 10;
  const sides = 8;
  const prongs = forked ? [-1, 1] : [0];
  for (const prong of prongs) {
    const start = out.depth.length;
    for (let i = 0; i <= steps; i++) {
      const u = i / steps;
      const z = back + (front - back) * u;
      // Rounded at both ends; a forked tongue splits over its last fifth.
      const taper = Math.sqrt(Math.max(0, Math.sin(Math.PI * Math.min(1, 0.08 + u * 0.92))));
      const split = forked ? smooth(0.78, 1, u) : 0;
      const w = width * (forked ? 0.5 + 0.5 * (1 - split) : 1) * Math.max(0.15, taper);
      const t = thick * Math.max(0.2, taper);
      const offset = prong * split * width * 0.45;
      const centre = m.origin
        .clone()
        .addScaledVector(m.forward, z)
        .addScaledVector(m.side, offset)
        .addScaledVector(m.up, floorAt(z) + t * 0.5);
      for (let s = 0; s < sides; s++) {
        const a = (s / sides) * Math.PI * 2;
        const p = centre
          .clone()
          .addScaledVector(m.side, Math.cos(a) * w * (forked && prong !== 0 ? 0.5 : 0.5))
          .addScaledVector(m.up, Math.sin(a) * t * 0.5);
        out.positions.push(p.x, p.y, p.z);
        out.normals.push(0, 0, 0);
        out.weights.push([[o.jaw, 1]]);
        out.depth.push(0.2 + 0.8 * (1 - u));
        out.kind.push(2);
      }
    }
    const v = (i: number, s: number) => base + start + i * sides + (s % sides);
    const from = out.indices.length;
    for (let i = 0; i < steps; i++)
      for (let s = 0; s < sides; s++) {
        out.indices.push(v(i, s), v(i + 1, s), v(i + 1, s + 1));
        out.indices.push(v(i, s), v(i + 1, s + 1), v(i, s + 1));
      }
    // Caps at both ends.
    for (const [i, flip] of [
      [0, true],
      [steps, false],
    ] as const) {
      const c = out.depth.length;
      let cx = 0;
      let cy = 0;
      let cz = 0;
      for (let s = 0; s < sides; s++) {
        const j = (start + i * sides + s) * 3;
        cx += out.positions[j] as number;
        cy += out.positions[j + 1] as number;
        cz += out.positions[j + 2] as number;
      }
      out.positions.push(cx / sides, cy / sides, cz / sides);
      out.normals.push(0, 0, 0);
      out.weights.push([[o.jaw, 1]]);
      out.depth.push(i === 0 ? 1 : 0.2);
      out.kind.push(2);
      for (let s = 0; s < sides; s++)
        if (flip) out.indices.push(base + c, v(i, s + 1), v(i, s));
        else out.indices.push(base + c, v(i, s), v(i, s + 1));
    }
    // Outward: the first quad at angle 0 faces the side it sits on.
    const middle = Math.floor(steps / 2) * sides * 6;
    const at = vertex(out, new Float32Array(0), base, v(Math.floor(steps / 2), 0));
    const centre = vertex(out, new Float32Array(0), base, v(Math.floor(steps / 2), sides / 2));
    orient(out, new Float32Array(0), base, from, from + middle, at.sub(centre));
  }
}

/** Area-weighted normals for the new vertices (the cut's edge keeps the skin's). */
function smoothNormals(out: MouthInside, skin: Float32Array, base: number): void {
  const p = (v: number) =>
    v < base
      ? new Vector3(skin[v * 3], skin[v * 3 + 1], skin[v * 3 + 2])
      : new Vector3(
          out.positions[(v - base) * 3],
          out.positions[(v - base) * 3 + 1],
          out.positions[(v - base) * 3 + 2],
        );
  for (let t = 0; t < out.indices.length; t += 3) {
    const a = out.indices[t] as number;
    const b = out.indices[t + 1] as number;
    const c = out.indices[t + 2] as number;
    const pa = p(a);
    const nrm = new Vector3().subVectors(p(b), pa).cross(new Vector3().subVectors(p(c), pa));
    for (const v of [a, b, c]) {
      if (v < base) continue;
      const j = (v - base) * 3;
      out.normals[j] = (out.normals[j] as number) + nrm.x;
      out.normals[j + 1] = (out.normals[j + 1] as number) + nrm.y;
      out.normals[j + 2] = (out.normals[j + 2] as number) + nrm.z;
    }
  }
  for (let j = 0; j < out.normals.length; j += 3) {
    const len =
      Math.hypot(
        out.normals[j] as number,
        out.normals[j + 1] as number,
        out.normals[j + 2] as number,
      ) || 1;
    out.normals[j] = (out.normals[j] as number) / len;
    out.normals[j + 1] = (out.normals[j + 1] as number) / len;
    out.normals[j + 2] = (out.normals[j + 2] as number) / len;
  }
}
