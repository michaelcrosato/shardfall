import { Vector3 } from 'three';

/**
 * Coupled-joint IK for limbs of any segment count. Every joint keeps its rest-pose bend, scaled
 * by one shared factor, so a 3- or 4-segment leg folds exactly like its rest pose and never flips;
 * for 2 segments this is the analytic two-bone solve. The bend factor is found by bisection on
 * the root-to-tip distance, which falls monotonically as the chain folds.
 */
export interface LimbIkSetup {
  /** Segment lengths, root first. */
  readonly lengths: readonly number[];
  /**
   * Rest relative joint angles in radians, one per joint (lengths.length - 1), measured in the
   * limb plane: positive turns toward the pole side.
   */
  readonly bends: readonly number[];
}

/** 2D chain points for bend factor `c`, with the first segment along +x before re-aiming. */
function chain2d(setup: LimbIkSetup, c: number): { xs: number[]; ys: number[] } {
  const xs = [0];
  const ys = [0];
  let angle = 0;
  setup.lengths.forEach((len, i) => {
    if (i > 0) angle += c * (setup.bends[i - 1] ?? 0);
    xs.push((xs[i] ?? 0) + Math.cos(angle) * len);
    ys.push((ys[i] ?? 0) + Math.sin(angle) * len);
  });
  return { xs, ys };
}

/** Root-to-tip distance for bend factor `c`: `chain2d`'s end, without allocating. */
function reachAt(setup: LimbIkSetup, c: number): number {
  let x = 0;
  let y = 0;
  let angle = 0;
  const { lengths, bends } = setup;
  for (let i = 0; i < lengths.length; i++) {
    if (i > 0) angle += c * (bends[i - 1] ?? 0);
    const len = lengths[i] as number;
    x += Math.cos(angle) * len;
    y += Math.sin(angle) * len;
  }
  return Math.hypot(x, y);
}

/** No joint folds past this many radians (about 160°). */
const JOINT_LIMIT = 2.8;

/** Largest useful bend factor: where the reach stops shrinking, within the joint limit. */
function maxBend(setup: LimbIkSetup): number {
  const largest = Math.max(1e-6, ...setup.bends.map(Math.abs));
  const limit = JOINT_LIMIT / largest;
  let best = 0;
  let bestReach = reachAt(setup, 0);
  for (let c = limit / 40; c <= limit + 1e-9; c += limit / 40) {
    const r = reachAt(setup, c);
    if (r < bestReach - 1e-9) {
      best = c;
      bestReach = r;
    } else {
      break;
    }
  }
  return best;
}

export interface LimbIkResult {
  /** Joint positions from root to tip (lengths.length + 1 points). */
  readonly points: Vector3[];
  /** How far the tip ended from the target: too far to reach, or too close to fold to (0 when reached). */
  readonly miss: number;
}

/**
 * Places the chain so it runs from `root` toward `target`, bending in the plane that contains the
 * pole direction. Unreachable targets get a straight limb pointing at them.
 */
export function solveLimb(
  setup: LimbIkSetup,
  root: Vector3,
  target: Vector3,
  pole: Vector3,
): LimbIkResult {
  const toTarget = new Vector3().subVectors(target, root);
  const distance = toTarget.length();
  const a = distance > 1e-9 ? toTarget.clone().divideScalar(distance) : new Vector3(0, -1, 0);
  // In-plane perpendicular toward the pole.
  const p = pole.clone().addScaledVector(a, -pole.dot(a));
  if (p.lengthSq() < 1e-12) {
    p.set(1, 0, 0).addScaledVector(a, -a.x);
    if (p.lengthSq() < 1e-12) p.set(0, 0, 1).addScaledVector(a, -a.z);
  }
  p.normalize();

  const cMax = maxBend(setup);
  let c: number;
  if (distance >= reachAt(setup, 0)) {
    c = 0;
  } else if (distance <= reachAt(setup, cMax)) {
    c = cMax;
  } else {
    let lo = 0;
    let hi = cMax;
    for (let i = 0; i < 40; i++) {
      const mid = (lo + hi) / 2;
      if (reachAt(setup, mid) > distance) lo = mid;
      else hi = mid;
    }
    c = (lo + hi) / 2;
  }

  const { xs, ys } = chain2d(setup, c);
  // Rotate the 2D chain so its end lies on the +x axis (toward the target).
  const endAngle = Math.atan2(ys.at(-1) ?? 0, xs.at(-1) ?? 0);
  const cos = Math.cos(-endAngle);
  const sin = Math.sin(-endAngle);
  const points = xs.map((x, i) => {
    const y = ys[i] ?? 0;
    const u = x * cos - y * sin;
    const v = x * sin + y * cos;
    return root.clone().addScaledVector(a, u).addScaledVector(p, v);
  });
  const reach = reachAt(setup, c);
  return { points, miss: Math.abs(distance - reach) < 1e-6 ? 0 : Math.abs(distance - reach) };
}

/** A limb set up once for repeated solves (the bend limit is worked out ahead of time). */
export interface PreparedLimb {
  readonly setup: LimbIkSetup;
  readonly maxBend: number;
  readonly straightReach: number;
  readonly foldedReach: number;
}

export function prepareLimb(setup: LimbIkSetup): PreparedLimb {
  const c = maxBend(setup);
  return { setup, maxBend: c, straightReach: reachAt(setup, 0), foldedReach: reachAt(setup, c) };
}

const scratchA = new Vector3();
const scratchP = new Vector3();

/**
 * Like `solveLimb`, for a prepared limb, writing joint positions into `out` (lengths + 1
 * vectors) without allocating.
 */
export function solvePrepared(
  limb: PreparedLimb,
  root: Vector3,
  target: Vector3,
  pole: Vector3,
  out: Vector3[],
): number {
  const setup = limb.setup;
  const a = scratchA.subVectors(target, root);
  const distance = a.length();
  if (distance > 1e-9) a.divideScalar(distance);
  else a.set(0, -1, 0);
  const p = scratchP.copy(pole).addScaledVector(a, -pole.dot(a));
  if (p.lengthSq() < 1e-12) {
    p.set(1, 0, 0).addScaledVector(a, -a.x);
    if (p.lengthSq() < 1e-12) p.set(0, 0, 1).addScaledVector(a, -a.z);
  }
  p.normalize();
  let c: number;
  if (distance >= limb.straightReach) c = 0;
  else if (distance <= limb.foldedReach) c = limb.maxBend;
  else {
    let lo = 0;
    let hi = limb.maxBend;
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (reachAt(setup, mid) > distance) lo = mid;
      else hi = mid;
    }
    c = (lo + hi) / 2;
  }
  // Build the 2D chain, then rotate it so its end lies along `a`.
  let x = 0;
  let y = 0;
  let angle = 0;
  const n = setup.lengths.length;
  const xs = tmpX;
  const ys = tmpY;
  xs[0] = 0;
  ys[0] = 0;
  for (let i = 0; i < n; i++) {
    if (i > 0) angle += c * (setup.bends[i - 1] ?? 0);
    x += Math.cos(angle) * (setup.lengths[i] as number);
    y += Math.sin(angle) * (setup.lengths[i] as number);
    xs[i + 1] = x;
    ys[i + 1] = y;
  }
  const endAngle = Math.atan2(y, x);
  const cos = Math.cos(-endAngle);
  const sin = Math.sin(-endAngle);
  for (let i = 0; i <= n; i++) {
    const px = xs[i] as number;
    const py = ys[i] as number;
    (out[i] as Vector3)
      .copy(root)
      .addScaledVector(a, px * cos - py * sin)
      .addScaledVector(p, px * sin + py * cos);
  }
  return Math.abs(distance - Math.hypot(x, y));
}

const tmpX = new Float64Array(8);
const tmpY = new Float64Array(8);
