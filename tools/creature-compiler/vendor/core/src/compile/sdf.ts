import { Vector3 } from 'three';
import type { BoneDef, ChainDef } from './types.ts';

/**
 * The creature's signed distance field: a rounded cone per skin bone (optionally with an
 * elliptical cross-section), plain union inside a chain, and a smooth minimum once per junction
 * where a chain meets its parent. A chain's masses (muscles, caps, the limb root's sphere) are
 * cones too; each joins the chain's cones with its own smooth minimum, `min(C, smin(C, m, k))`,
 * so masses never stack and their order never matters. Head details (docs/design/8.3-heads.md)
 * are masses too, and carves (nostrils) are subtracted from their chain with a smooth maximum
 * after its masses; an evaluator can leave both out, as the grid does. Primitives live in flat
 * typed arrays so evaluation stays fast.
 */
export interface Sdf {
  /** Number of primitives. */
  readonly count: number;
  /**
   * Per primitive (stride 30): a(3) b(3) ra rb side(3) up(3) dir(3) sx sy h chain bone kind
   * 1/sx 1/sy min(sx,sy) cone-b cone-a degenerate blend. Kind 0 is a bone's cone, 1 a mass,
   * 2 a detail mass and 3 a carve.
   */
  readonly data: Float64Array;
  readonly chainCount: number;
  /** Parent chain per chain (-1 for the root chain). Parents come before children. */
  readonly chainParent: Int32Array;
  /** Smooth-min radius per chain at its junction with its parent (metres). */
  readonly chainBlend: Float64Array;
  /** Bounding sphere per primitive: centre(3) radius. */
  readonly bounds: Float64Array;
  /**
   * Extra reach per primitive for culling: a mass's blend, and for a cone the largest blend of
   * its chain's masses (a mass's smooth minimum needs the cones within its blend).
   */
  readonly reach: Float64Array;
  /** Largest junction blend (masses' blends are in `reach`). */
  readonly maxBlend: number;
  /** Bones left out of the field because they are thinner than the grid can show. */
  readonly thinBones: readonly number[];
}

const STRIDE = 30;
const BIG = 1e9;

/** Builds the field from the skeleton, leaving out bones thinner than `minRadius`. */
export function buildSdf(
  bones: readonly BoneDef[],
  chains: readonly ChainDef[],
  minRadius: number,
): Sdf {
  const prims: number[] = [];
  const bounds: number[] = [];
  const thinBones: number[] = [];
  const chainMassBlend = new Float64Array(chains.length);
  const boneChain = (bone: number) => (bones[bone] as BoneDef).chain;

  const push = (
    a: Vector3,
    b: Vector3,
    ra: number,
    rb: number,
    side: Vector3,
    up: Vector3,
    dir: Vector3,
    sx: number,
    sy: number,
    chain: number,
    bone: number,
    kind: number,
    blend = 0,
  ) => {
    const h = a.distanceTo(b);
    const degenerate = h < 1e-9 || Math.abs(ra - rb) >= h ? 1 : 0;
    const coneB = degenerate ? 0 : (ra - rb) / h;
    const coneA = degenerate ? 1 : Math.sqrt(1 - coneB * coneB);
    prims.push(
      a.x,
      a.y,
      a.z,
      b.x,
      b.y,
      b.z,
      ra,
      rb,
      side.x,
      side.y,
      side.z,
      up.x,
      up.y,
      up.z,
      dir.x,
      dir.y,
      dir.z,
      sx,
      sy,
      h,
      chain,
      bone,
      kind,
      1 / sx,
      1 / sy,
      Math.min(sx, sy),
      coneB,
      coneA,
      degenerate,
      blend,
    );
    const scale = Math.max(sx, sy);
    bounds.push(
      (a.x + b.x) / 2,
      (a.y + b.y) / 2,
      (a.z + b.z) / 2,
      h / 2 + Math.max(ra, rb) * scale,
    );
  };

  // Thin by the radii the blueprint gave, so anatomy never moves a bone between the field and
  // the swept tubes. A jaw goes with its head: inside it, a tube would show when the mouth opens.
  const thinnessOf = (bone: BoneDef): number => {
    if (bone.section === 'jaw' && bone.parent >= 0)
      return thinnessOf(bones[bone.parent] as BoneDef);
    const plain = bone.shaped ? bone.plainProfile : bone.profile;
    const cross = bone.plainCross ?? bone.cross;
    return Math.max(bone.r0, bone.r1, ...(plain ?? [])) * Math.min(cross[0], cross[1]);
  };
  for (const [ci, chain] of chains.entries()) {
    for (const id of chain.bones) {
      const bone = bones[id] as BoneDef;
      if (!bone.skin) continue;
      if (bone.tube || thinnessOf(bone) < minRadius) {
        thinBones.push(id);
        continue;
      }
      const dir = new Vector3().subVectors(bone.tail, bone.head);
      if (dir.lengthSq() < 1e-14) dir.copy(bone.up).cross(new Vector3(1, 0, 0));
      dir.normalize();
      const up = bone.up.clone().addScaledVector(dir, -bone.up.dot(dir)).normalize();
      const side = new Vector3().crossVectors(up, dir).normalize();
      const profile = bone.profile;
      if (profile && profile.length > 2) {
        // One cone per span of the profile, so radius profiles show between joints too.
        const spans = profile.length - 1;
        for (let k = 0; k < spans; k++) {
          const a = new Vector3().lerpVectors(bone.head, bone.tail, k / spans);
          const b = new Vector3().lerpVectors(bone.head, bone.tail, (k + 1) / spans);
          const [sx, sy] = bone.cross;
          push(
            a,
            b,
            profile[k] as number,
            profile[k + 1] as number,
            side,
            up,
            dir,
            sx,
            sy,
            ci,
            id,
            0,
          );
        }
      } else {
        const [sx, sy] = bone.cross;
        push(bone.head, bone.tail, bone.r0, bone.r1, side, up, dir, sx, sy, ci, id, 0);
      }
    }
    for (const mass of chain.masses) {
      // A muscle on a bone too thin for the grid, or itself thinner, would float as an island.
      // (Plain masses, such as the limb root's sphere at muscle 0, stay as they always were.)
      // Head details sit on a head the grid shows, and refinement shows them however small.
      if (mass.blend > 0) {
        if (!bones[mass.bone]?.skin || thinBones.includes(mass.bone)) continue;
        if (
          !mass.kind &&
          Math.min(mass.ra, mass.rb) * Math.min(mass.cross[0], mass.cross[1]) < minRadius
        )
          continue;
      }
      const dir = new Vector3().subVectors(mass.b, mass.a);
      if (dir.lengthSq() < 1e-14) dir.copy(mass.up).cross(new Vector3(1, 0, 0));
      if (dir.lengthSq() < 1e-14) dir.set(0, 0, 1);
      dir.normalize();
      const up = mass.up.clone().addScaledVector(dir, -mass.up.dot(dir));
      if (up.lengthSq() < 1e-12) up.set(0, 1, 0).addScaledVector(dir, -dir.y);
      up.normalize();
      const side = new Vector3().crossVectors(up, dir).normalize();
      push(
        mass.a,
        mass.b,
        mass.ra,
        mass.rb,
        side,
        up,
        dir,
        mass.cross[0],
        mass.cross[1],
        ci,
        mass.bone,
        mass.kind === 'carve' ? 3 : mass.kind === 'detail' ? 2 : 1,
        Math.max(0, mass.blend),
      );
      chainMassBlend[ci] = Math.max(chainMassBlend[ci] as number, mass.blend);
    }
  }

  const count = prims.length / STRIDE;
  const reach = new Float64Array(count);
  for (let i = 0; i < count; i++) {
    const kind = prims[i * STRIDE + 22] as number;
    reach[i] =
      kind >= 1
        ? (prims[i * STRIDE + 29] as number)
        : (chainMassBlend[prims[i * STRIDE + 20] as number] as number);
  }
  const chainParent = new Int32Array(chains.length);
  const chainBlend = new Float64Array(chains.length);
  let maxBlend = 0;
  chains.forEach((c, i) => {
    chainParent[i] = c.parentBone >= 0 ? boneChain(c.parentBone) : -1;
    chainBlend[i] = c.blend;
    maxBlend = Math.max(maxBlend, c.blend);
  });
  return {
    count,
    data: new Float64Array(prims),
    chainCount: chains.length,
    chainParent,
    chainBlend,
    bounds: new Float64Array(bounds),
    reach,
    maxBlend,
    thinBones,
  };
}

/** Distance to one primitive. */
export function primDistance(sdf: Sdf, i: number, px: number, py: number, pz: number): number {
  const d = sdf.data;
  const o = i * STRIDE;
  const qx = px - (d[o] as number);
  const qy = py - (d[o + 1] as number);
  const qz = pz - (d[o + 2] as number);
  const lx =
    (qx * (d[o + 8] as number) + qy * (d[o + 9] as number) + qz * (d[o + 10] as number)) *
    (d[o + 23] as number);
  const ly =
    (qx * (d[o + 11] as number) + qy * (d[o + 12] as number) + qz * (d[o + 13] as number)) *
    (d[o + 24] as number);
  const lz = qx * (d[o + 14] as number) + qy * (d[o + 15] as number) + qz * (d[o + 16] as number);
  const r1 = d[o + 6] as number;
  const r2 = d[o + 7] as number;
  const h = d[o + 19] as number;
  const scale = d[o + 25] as number;
  const qr2 = lx * lx + ly * ly;
  if ((d[o + 28] as number) === 1) {
    // Degenerate cone: the larger end sphere contains the other.
    const da = Math.sqrt(qr2 + lz * lz) - r1;
    const db = Math.sqrt(qr2 + (lz - h) * (lz - h)) - r2;
    return (da < db ? da : db) * scale;
  }
  const b = d[o + 26] as number;
  const a = d[o + 27] as number;
  const qr = Math.sqrt(qr2);
  const k = -b * qr + a * lz;
  let dist: number;
  if (k < 0) dist = Math.sqrt(qr2 + lz * lz) - r1;
  else if (k > a * h) dist = Math.sqrt(qr2 + (lz - h) * (lz - h)) - r2;
  else dist = qr * a + lz * b - r1;
  return dist * scale;
}

/** Polynomial smooth minimum with blend radius k. */
export function smin(a: number, b: number, k: number): number {
  if (k <= 0) return a < b ? a : b;
  const h = Math.max(k - Math.abs(a - b), 0) / k;
  return (a < b ? a : b) - h * h * k * 0.25;
}

/** Polynomial smooth maximum with blend radius k. */
export function smax(a: number, b: number, k: number): number {
  return -smin(-a, -b, k);
}

/** Scratch buffers for evaluation (one per thread of use). */
export class SdfEvaluator {
  readonly sdf: Sdf;
  private readonly chainD: Float64Array;
  /** Each chain's cones alone, which its masses blend against. */
  private readonly chainC: Float64Array;
  private readonly touched: Int32Array;
  private readonly massPrim: Int32Array;
  private readonly massDist: Float64Array;
  private stamp = 1;
  /** Plain union of the last evaluation (for crease depth). */
  union = BIG;
  /** Primitive nearest the last evaluated point. */
  nearestPrim = -1;
  /** Whether head details and carves count; the grid leaves them to the head's refinement. */
  details = true;

  constructor(sdf: Sdf) {
    this.sdf = sdf;
    this.chainD = new Float64Array(sdf.chainCount);
    this.chainC = new Float64Array(sdf.chainCount);
    this.touched = new Int32Array(sdf.chainCount);
    this.massPrim = new Int32Array(sdf.count);
    this.massDist = new Float64Array(sdf.count);
  }

  /** Field value at a point, using only `prims` (or every primitive when omitted). */
  eval(px: number, py: number, pz: number, prims?: ArrayLike<number>, primCount?: number): number {
    const sdf = this.sdf;
    const chainD = this.chainD;
    const chainC = this.chainC;
    const touched = this.touched;
    const stamp = this.stamp;
    const n = prims ? (primCount ?? prims.length) : sdf.count;
    let nearest = -1;
    let nearestDist = BIG;
    let lowest = sdf.chainCount;
    let highest = -1;
    let masses = 0;
    let carves = 0;
    const details = this.details;
    for (let j = 0; j < n; j++) {
      const i = prims ? (prims[j] as number) : j;
      const kind = sdf.data[i * STRIDE + 22] as number;
      if (kind >= 2 && !details) continue;
      const dist = primDistance(sdf, i, px, py, pz);
      if (dist < nearestDist && kind !== 3) {
        nearestDist = dist;
        nearest = i;
      }
      const c = sdf.data[i * STRIDE + 20] as number;
      if (touched[c] !== stamp) {
        touched[c] = stamp;
        chainC[c] = BIG;
        if (c < lowest) lowest = c;
        if (c > highest) highest = c;
      }
      if (kind === 3) {
        // Carves go at the end of the list, applied after the masses.
        const slot = sdf.count - 1 - carves++;
        this.massPrim[slot] = i;
        this.massDist[slot] = dist;
      } else if (kind >= 1) {
        this.massPrim[masses] = i;
        this.massDist[masses++] = dist;
      } else if (dist < (chainC[c] as number)) {
        chainC[c] = dist;
      }
    }
    // Each chain: its cones, then each mass blended against the cones alone.
    for (let c = lowest; c <= highest; c++)
      if (touched[c] === stamp) chainD[c] = chainC[c] as number;
    for (let m = 0; m < masses; m++) {
      const i = this.massPrim[m] as number;
      const c = sdf.data[i * STRIDE + 20] as number;
      const v = smin(
        chainC[c] as number,
        this.massDist[m] as number,
        sdf.data[i * STRIDE + 29] as number,
      );
      if (v < (chainD[c] as number)) chainD[c] = v;
    }
    for (let k = 0; k < carves; k++) {
      const slot = sdf.count - 1 - k;
      const i = this.massPrim[slot] as number;
      const c = sdf.data[i * STRIDE + 20] as number;
      chainD[c] = smax(
        chainD[c] as number,
        -(this.massDist[slot] as number),
        sdf.data[i * STRIDE + 29] as number,
      );
    }
    // The plain union of the chains, after their masses: only junctions read as creases.
    let union = BIG;
    for (let c = lowest; c <= highest; c++)
      if (touched[c] === stamp && (chainD[c] as number) < union) union = chainD[c] as number;
    // Fold children into parents; parents always have lower indices than their children.
    let d = BIG;
    for (let c = highest; c >= 0 && c >= lowest; c--) {
      if (touched[c] !== stamp) continue;
      const child = chainD[c] as number;
      const parent = sdf.chainParent[c] as number;
      if (parent < 0) {
        if (child < d) d = child;
        continue;
      }
      if (touched[parent] !== stamp) {
        // The parent is out of reach here, so the child passes through unblended.
        touched[parent] = stamp;
        chainD[parent] = child;
        if (parent < lowest) lowest = parent;
        continue;
      }
      chainD[parent] = smin(chainD[parent] as number, child, sdf.chainBlend[c] as number);
    }
    this.stamp = stamp >= 0x3fffffff ? 1 : stamp + 1;
    this.union = union;
    this.nearestPrim = nearest;
    return d;
  }

  /**
   * Field value and normalized gradient from the same four tetrahedron samples: their average is
   * the value at the centre to second order. Half the cost of `eval` plus `gradient`.
   */
  valueAndGradient(
    px: number,
    py: number,
    pz: number,
    eps: number,
    prims: ArrayLike<number> | undefined,
    out: Vector3,
  ): number {
    const a = this.eval(px + eps, py - eps, pz - eps, prims);
    const b = this.eval(px - eps, py - eps, pz + eps, prims);
    const c = this.eval(px - eps, py + eps, pz - eps, prims);
    const d = this.eval(px + eps, py + eps, pz + eps, prims);
    out.set(a - b - c + d, -a - b + c + d, -a + b - c + d);
    const len = out.length();
    if (len > 1e-12) out.divideScalar(len);
    else out.set(0, 1, 0);
    return (a + b + c + d) / 4;
  }

  /** Normalized gradient from four samples on a tetrahedron. */
  gradient(
    px: number,
    py: number,
    pz: number,
    eps: number,
    prims?: ArrayLike<number>,
    primCount?: number,
    out = new Vector3(),
  ): Vector3 {
    const a = this.eval(px + eps, py - eps, pz - eps, prims, primCount);
    const b = this.eval(px - eps, py - eps, pz + eps, prims, primCount);
    const c = this.eval(px - eps, py + eps, pz - eps, prims, primCount);
    const d = this.eval(px + eps, py + eps, pz + eps, prims, primCount);
    out.set(a - b - c + d, -a - b + c + d, -a + b - c + d);
    const len = out.length();
    return len > 1e-12 ? out.divideScalar(len) : out.set(0, 1, 0);
  }
}

export const SDF_STRIDE = STRIDE;
export const SDF_BIG = BIG;

/** The bone a primitive belongs to. */
export function primBone(sdf: Sdf, i: number): number {
  return sdf.data[i * STRIDE + 21] as number;
}
