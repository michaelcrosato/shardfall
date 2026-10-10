import { Vector3 } from 'three';
import { lineY, MOUTH_COLUMN, type MouthLine, mouthLine, stripPoint } from './mouth.ts';
import { refineRegion } from './refine.ts';
import { buildSdf, type Sdf, SdfEvaluator } from './sdf.ts';
import type { WeightTable } from './skin.ts';
import type { PrimCulling } from './surface-nets.ts';
import type { BoneDef, ChainDef, MassDef } from './types.ts';

/**
 * A head's details as field primitives on its chain (docs/design/8.3-heads.md): lips along the
 * mouth, a brow over each eye, cheekbones and nostrils, all at the head's own scale. The head's
 * refinement shows them however small the head is.
 */

/** The outline of the mouth at the cut, from one corner round the tip to the other. */
export interface MouthShape {
  readonly line: MouthLine;
  /** Middle of the corners at the cut's height: rays from here cross the outline once. */
  readonly centre: Vector3;
  /** One side's outline from the tip (index 0) to the corner, at even arc lengths. */
  readonly left: readonly Vector3[];
  readonly right: readonly Vector3[];
  /** Arc length of one side, tip to corner (metres). */
  readonly length: number;
  /** The head's radius along the outline, tip to corner. */
  readonly radius: readonly number[];
  /** The head's radius at a distance `z` along it from the skull's centre. */
  radiusAt(z: number): number;
  /**
   * Room inside the head and jaw alone above (`sign` 1) or below (-1) a point in the mouth:
   * how far it is to their skin (metres).
   */
  room(p: Vector3, sign: number): number;
}

/** Samples per side of the outline. */
const OUTLINE = 24;

/**
 * Marches rays in the cut from the middle of the corners through the head and jaw alone (no
 * neck, no details), so lips, teeth, the cavity and beaks share one outline.
 */
export function mouthShape(bones: readonly BoneDef[], chain: ChainDef, head: number, jaw: number) {
  const m = mouthLine(bones[head] as BoneDef, bones[jaw] as BoneDef);
  const field = buildSdf(bones, [{ ...chain, parentBone: -1, masses: [] }], 0);
  const evaluator = new SdfEvaluator(field);
  const centre = m.origin
    .clone()
    .addScaledVector(m.forward, m.corner)
    .addScaledVector(m.up, m.cornerY);
  const reach = (m.tip - m.corner) * 3 + (bones[head] as BoneDef).r0 * 3;
  const at = (angle: number) => {
    // A ray in the cut: `angle` 0 points at the tip, ±90 to the sides (+ is the head's left).
    const a = (angle * Math.PI) / 180;
    const dz = Math.cos(a);
    const dx = Math.sin(a);
    const point = (s: number) => {
      const z = m.corner + dz * s;
      return m.origin
        .clone()
        .addScaledVector(m.forward, z)
        .addScaledVector(m.side, dx * s)
        .addScaledVector(m.up, lineY(m, z));
    };
    const f = (s: number) => {
      const p = point(s);
      return evaluator.eval(p.x, p.y, p.z);
    };
    let lo = 0;
    let hi = reach;
    const steps = 48;
    for (let i = 1; i <= steps; i++) {
      const s = (reach * i) / steps;
      if (f(s) >= 0) {
        hi = s;
        lo = (reach * (i - 1)) / steps;
        break;
      }
    }
    for (let i = 0; i < 30; i++) {
      const mid = (lo + hi) / 2;
      if (f(mid) >= 0) hi = mid;
      else lo = mid;
    }
    return point(hi);
  };
  const side = (sign: number) => {
    const raw: Vector3[] = [];
    const n = OUTLINE * 4;
    for (let i = 0; i <= n; i++) raw.push(at(sign * 90 * (i / n)));
    return evenly(raw, OUTLINE);
  };
  const left = side(1);
  const right = side(-1);
  let length = 0;
  for (let i = 1; i < left.length; i++)
    length += (left[i] as Vector3).distanceTo(left[i - 1] as Vector3);
  const headBone = bones[head] as BoneDef;
  const len = headBone.head.distanceTo(headBone.tail);
  const radiusAt = (z: number) =>
    headBone.r0 + (headBone.r1 - headBone.r0) * Math.min(1, Math.max(0, z / (len || 1)));
  const radius = left.map((p) => radiusAt(new Vector3().subVectors(p, m.origin).dot(m.forward)));
  const room = (p: Vector3, sign: number) => {
    const limit = headBone.r0 * 2;
    const f = (s: number) =>
      evaluator.eval(p.x + m.up.x * sign * s, p.y + m.up.y * sign * s, p.z + m.up.z * sign * s);
    if (f(0) >= 0) return 0;
    let lo = 0;
    let hi = limit;
    for (let i = 1; i <= 24; i++) {
      const s = (limit * i) / 24;
      if (f(s) >= 0) {
        hi = s;
        lo = (limit * (i - 1)) / 24;
        break;
      }
    }
    for (let i = 0; i < 16; i++) {
      const mid = (lo + hi) / 2;
      if (f(mid) >= 0) hi = mid;
      else lo = mid;
    }
    return lo;
  };
  return { line: m, centre, left, right, length, radius, radiusAt, room } satisfies MouthShape;
}

/** A polyline resampled to `n` + 1 points at even arc lengths. */
function evenly(points: readonly Vector3[], n: number): Vector3[] {
  const lens = [0];
  for (let i = 1; i < points.length; i++)
    lens.push(
      (lens[i - 1] as number) + (points[i] as Vector3).distanceTo(points[i - 1] as Vector3),
    );
  const total = lens.at(-1) || 1;
  const out: Vector3[] = [];
  let j = 0;
  for (let i = 0; i <= n; i++) {
    const d = (total * i) / n;
    while (j < lens.length - 2 && (lens[j + 1] as number) < d) j++;
    const f = (d - (lens[j] as number)) / ((lens[j + 1] as number) - (lens[j] as number) || 1);
    out.push(
      new Vector3().lerpVectors(
        points[j] as Vector3,
        points[j + 1] as Vector3,
        Math.min(1, Math.max(0, f)),
      ),
    );
  }
  return out;
}

/** A point on one side of the outline at `t` (0 tip, 1 corner) by arc length, and the radius. */
export function outlineAt(
  shape: MouthShape,
  t: number,
  side: number,
): { point: Vector3; radius: number } {
  const list = side >= 0 ? shape.left : shape.right;
  const x = Math.min(1, Math.max(0, t)) * (list.length - 1);
  const i = Math.min(list.length - 2, Math.floor(x));
  const f = x - i;
  return {
    point: new Vector3().lerpVectors(list[i] as Vector3, list[i + 1] as Vector3, f),
    radius:
      (shape.radius[i] as number) +
      ((shape.radius[i + 1] as number) - (shape.radius[i] as number)) * f,
  };
}

/** Where an eye sits on a head: its `at` and angle, and the side it is on. */
export interface EyePlace {
  readonly at: number;
  readonly angle: number;
  readonly mirror: number;
}

/** A frame on the head at `at`: the axis point, toward the snout, up, and the radius there. */
export type HeadFrame = (at: number) => {
  readonly point: Vector3;
  readonly forward: Vector3;
  readonly up: Vector3;
  readonly radius: number;
  readonly cross: readonly [number, number];
};

const DEG = Math.PI / 180;

/**
 * The head's detail masses and carves. `lips` and `brow` are the blueprint's (0–1); `eyes` are
 * the eye parts on this head; `frame` samples the head's section path.
 */
export function headDetails(options: {
  readonly bones: readonly BoneDef[];
  readonly head: number;
  readonly jaw: number;
  readonly shape: MouthShape | undefined;
  readonly frame: HeadFrame;
  readonly lips: number;
  readonly brow: number;
  readonly eyes: readonly EyePlace[];
}): MassDef[] {
  const { bones, head, jaw, shape, frame } = options;
  const headBone = bones[head] as BoneDef;
  const masses: MassDef[] = [];
  const surface = (at: number, angle: number, mirror: number) => {
    const f = frame(at);
    const a = angle * DEG;
    const left = new Vector3().crossVectors(f.up, f.forward).normalize();
    const dir = f.up
      .clone()
      .multiplyScalar(Math.cos(a))
      .addScaledVector(left, mirror * Math.sin(a))
      .normalize();
    const r = f.radius * Math.hypot(f.cross[0] * Math.sin(a), f.cross[1] * Math.cos(a));
    return { point: f.point.clone().addScaledVector(dir, r), dir, radius: f.radius };
  };
  const cone = (
    a: Vector3,
    b: Vector3,
    ra: number,
    rb: number,
    bone: number,
    up: Vector3,
    kind: 'detail' | 'carve' = 'detail',
    blend = Math.max(ra, rb),
    fine = true,
  ) => {
    masses.push({ bone, a, b, ra, rb, up, cross: [1, 1], blend, kind, fine });
  };

  // Lips: a ridge either side of the cut, upper on the head and lower on the jaw.
  if (shape && options.lips > 0 && jaw >= 0) {
    const m = shape.line;
    const K = 6;
    for (const sign of [1, -1]) {
      for (const [row, bone] of [
        [1, head],
        [-1, jaw],
      ] as const) {
        let prev: { p: Vector3; r: number } | undefined;
        for (let k = 0; k <= K; k++) {
          const t = k / K;
          const o = outlineAt(shape, t, sign);
          const rho = options.lips * 0.18 * o.radius * (1 - 0.5 * t * t);
          const inward = new Vector3().subVectors(shape.centre, o.point);
          inward.addScaledVector(m.up, -inward.dot(m.up)).normalize();
          const p = o.point
            .clone()
            .addScaledVector(inward, 0.6 * rho)
            .addScaledVector(m.up, row * 0.5 * rho);
          // The lips run the length of the mouth: refining all along them would cost more than
          // they show, so only the brow, cheekbones and nostrils are split finer.
          if (prev)
            cone(prev.p, p, prev.r, rho, bone, m.up, 'detail', Math.max(prev.r, rho), false);
          prev = { p, r: rho };
        }
      }
    }
  }

  // The brow: a ridge over each eye, or across the forehead without eyes.
  if (options.brow > 0) {
    const ridge = (at: number, from: number, to: number, mirror: number) => {
      const a = surface(at, from, mirror);
      const b = surface(at, to, mirror);
      const r = options.brow * 0.4 * frame(at).radius;
      cone(
        a.point.clone().addScaledVector(a.dir, -0.5 * r),
        b.point.clone().addScaledVector(b.dir, -0.5 * r),
        r * 0.7,
        r,
        head,
        frame(at).up,
      );
    };
    if (options.eyes.length > 0)
      for (const eye of options.eyes)
        ridge(eye.at + 0.03, eye.angle - 30, eye.angle + 10, eye.mirror || 1);
    else ridge(0.5, -60, 60, 1);
  }

  // Cheekbones: below and behind the eyes toward the jaw's hinge, never longer than the skull
  // is wide (long snouts would otherwise grow ridges along their whole length).
  if (jaw >= 0) {
    const r = 0.1 * headBone.r0;
    const span = Math.min(0.35, (1.5 * headBone.r0) / Math.max(1e-6, headLength(frame)));
    for (const mirror of [1, -1]) {
      const a = surface(0.8 - span, 95, mirror);
      const b = surface(0.8, 100, mirror);
      cone(
        a.point.clone().addScaledVector(a.dir, -0.6 * r),
        b.point.clone().addScaledVector(b.dir, -0.6 * r),
        r,
        r * 0.8,
        head,
        frame(0.6).up,
      );
    }
  }

  // Nostrils: two carves on the top front of the snout.
  {
    const f = frame(0.05);
    const nu = 0.12 * headBone.r1;
    for (const mirror of [1, -1]) {
      const s = surface(0.05, 32, mirror);
      const along = f.forward.clone().multiplyScalar(0.8 * nu);
      cone(
        s.point.clone().sub(along),
        s.point.clone().add(along),
        nu,
        nu,
        head,
        s.dir,
        'carve',
        nu * 0.5,
      );
    }
  }
  return masses;
}

/**
 * A point on a mouth's gums at `t` (0 at the tip, 1 at the corner, by arc length) on one side:
 * where teeth stand (docs/design/8.3-heads.md). The normal points into the opening: down from
 * the upper gums, up from the lower ones; the radius is the skull's.
 */
export function gumPoint(
  shape: MouthShape,
  lips: number,
  t: number,
  row: 'upper' | 'lower',
  side: number,
): { position: Vector3; normal: Vector3; radius: number } {
  const o = outlineAt(shape, t, side);
  const m = shape.line;
  const z = new Vector3().subVectors(o.point, m.origin).dot(m.forward);
  const T = (0.04 + 0.2 * lips) * shape.radiusAt(z);
  const sign = row === 'upper' ? 1 : -1;
  const { point } = stripPoint(shape, o.point, T, 2, sign);
  // The skull's radius, so whatever stands in the mouth is sized to the head, not the snout.
  return { position: point, normal: m.up.clone().multiplyScalar(-sign), radius: shape.radiusAt(0) };
}

/** What `refineHeads` needs from the pipeline. */
export interface RefineHeadsInput {
  readonly positions: Float32Array;
  readonly normals: Float32Array;
  readonly indices: Uint32Array;
  readonly table: WeightTable;
  readonly bones: readonly BoneDef[];
  readonly heads: readonly { readonly head: number; readonly jaw: number }[];
  readonly chains: readonly ChainDef[];
  readonly mouths: readonly (MouthShape | undefined)[];
  readonly sdf: Sdf;
  readonly culling: PrimCulling;
  readonly cell: number;
  /** Edges per skull radius at most (8 low, 12 medium, 16 high). */
  readonly perRadius: number;
  /** Most triangles for the heads, and for the whole skin. */
  readonly allowance: number;
  readonly limit: number;
  /** Skin triangles still to come: swept tubes and eyelids. */
  readonly later: number;
}

/**
 * Refines each head toward its own edge length (docs/design/8.3-heads.md): split, projected
 * onto the field with its details, and evened out; finer within reach of a detail, down to half
 * its radius. The heads share an allowance that keeps the whole skin under its limit, with room
 * for the mouth's inside.
 */
export function refineHeads(input: RefineHeadsInput): {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
  table: WeightTable;
} {
  let { positions, normals, indices, table } = input;
  const { sdf, culling, cell, bones } = input;
  const evaluator = new SdfEvaluator(sdf);
  const g = new Vector3();
  // Only primitives within reach of the point: its block's list is several times longer.
  const near = new Int32Array(sdf.count);
  const margin = sdf.maxBlend + cell;
  const project = (p: Vector3, n: Vector3) => {
    const list = culling.primsOf(culling.blockAt(p.x, p.y, p.z));
    let count = 0;
    for (let j = 0; j < list.length; j++) {
      const i = list[j] as number;
      const o = i * 4;
      const dx = p.x - (sdf.bounds[o] as number);
      const dy = p.y - (sdf.bounds[o + 1] as number);
      const dz = p.z - (sdf.bounds[o + 2] as number);
      const r = (sdf.bounds[o + 3] as number) + (sdf.reach[i] as number) + margin;
      if (dx * dx + dy * dy + dz * dz < r * r) near[count++] = i;
    }
    // One Newton step: a new point starts a small fraction of an edge off the surface.
    const d = evaluator.valueAndGradient(p.x, p.y, p.z, cell * 0.05, near.subarray(0, count), g);
    p.addScaledVector(g, -Math.max(-cell * 0.5, Math.min(cell * 0.5, d)));
    n.copy(g);
  };
  const heads = input.heads.filter((h) => !sdf.thinBones.includes(h.head));
  // Several heads share the allowance: each takes an even share of what the ones before it left.
  let spent = 0;
  for (const [i, h] of heads.entries()) {
    const head = bones[h.head] as BoneDef;
    const ids = new Set([h.head, h.jaw].filter((b) => b >= 0));
    const mask = new Uint8Array(positions.length / 3);
    const SLOTS = table.bones.length / table.count;
    for (let v = 0; v < mask.length; v++) {
      // The head's and jaw's weight, read from the slots in place (as `entries` lists them).
      let w = 0;
      for (let s = v * SLOTS; s < (v + 1) * SLOTS; s++) {
        const b = table.bones[s] as number;
        const x = table.weights[s] as number;
        if (b >= 0 && x > 0 && ids.has(b)) w += x;
      }
      if (w >= 0.5) mask[v] = 1;
    }
    let area = 0;
    let inside = 0;
    const ab = new Vector3();
    const ac = new Vector3();
    for (let t = 0; t < indices.length; t += 3) {
      const a = indices[t] as number;
      const b = indices[t + 1] as number;
      const c = indices[t + 2] as number;
      if (!(mask[a] && mask[b] && mask[c])) continue;
      inside++;
      ab.fromArray(positions, b * 3).sub(g.fromArray(positions, a * 3));
      ac.fromArray(positions, c * 3).sub(g);
      area += ab.cross(ac).length() / 2;
    }
    // The finest edge the allowance and the skin's limit leave room for, with the mouth's
    // inside (a strip of columns along the cut on each side) counted in.
    const finest = Math.max(head.r0 / input.perRadius, cell / 16);
    const shape = h.jaw >= 0 ? input.mouths[input.heads.indexOf(h)] : undefined;
    const rest = indices.length / 3 - inside + input.later;
    const share = Math.max(0, input.allowance - spent) / (heads.length - i);
    let edge = Math.max(finest, Math.sqrt(area / (0.433 * share)));
    // Halving long edges leaves them between 0.7 and 1.4 targets, about 0.8 on average; the cut
    // crosses about 1.4 edges per target length on each side.
    // The mouth's inside comes later, for this head and for each head not yet refined (several
    // heads are alike, so theirs cost the same).
    const mouthCost = (e: number) =>
      shape ? 2 * ((2.8 * shape.length) / e) * MOUTH_COLUMN + 500 : 0;
    const cost = (e: number) => area / (0.433 * (0.8 * e) ** 2) + mouthCost(e) * (heads.length - i);
    for (let k = 0; k < 60 && rest + cost(edge) > 0.97 * input.limit; k++) edge *= 1.08;
    if (area / (0.433 * edge * edge) <= inside) continue;
    // Details: edges within reach of one split down to half its radius.
    const chain = input.chains.find((c) => c.bones.includes(h.head));
    const details = (chain?.masses ?? []).filter((m) => m.fine);
    const limitAt =
      details.length === 0
        ? undefined
        : (x: number, y: number, z: number) => {
            let best = edge;
            for (const d of details) {
              const r = Math.min(d.ra, d.rb);
              // At most one halving finer than the head: details never cost more than that.
              const want = Math.max(finest, 0.5 * r, 0.5 * edge);
              if (want >= best) continue;
              const reach = Math.max(d.ra, d.rb) + d.blend + edge;
              if (segmentDistance2(d.a, d.b, x, y, z) < reach * reach) best = want;
            }
            return best;
          };
    const refined = refineRegion({
      positions,
      normals,
      indices,
      table,
      mask,
      target: edge,
      ...(limitAt ? { limitAt } : {}),
      project,
      passes: 5,
    });
    spent += (refined.indices.length - indices.length) / 3;
    positions = refined.positions;
    normals = refined.normals;
    indices = refined.indices;
    table = refined.table;
  }
  return { positions, normals, indices, table };
}

/** The head's length along its section path, from `at` 0 to 1. */
function headLength(frame: HeadFrame): number {
  return frame(0).point.distanceTo(frame(1).point);
}

/** Squared distance from a point to a segment. */
function segmentDistance2(a: Vector3, b: Vector3, x: number, y: number, z: number): number {
  const abx = b.x - a.x;
  const aby = b.y - a.y;
  const abz = b.z - a.z;
  const apx = x - a.x;
  const apy = y - a.y;
  const apz = z - a.z;
  const len2 = abx * abx + aby * aby + abz * abz;
  const t = len2 > 0 ? Math.min(1, Math.max(0, (apx * abx + apy * aby + apz * abz) / len2)) : 0;
  const dx = apx - abx * t;
  const dy = apy - aby * t;
  const dz = apz - abz * t;
  return dx * dx + dy * dy + dz * dz;
}
