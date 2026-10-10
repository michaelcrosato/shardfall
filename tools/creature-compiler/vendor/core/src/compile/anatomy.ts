/**
 * Anatomy from rules (docs/design/8.1-anatomy.md): radius multipliers for limbs (muscle swell and
 * narrow joints, or chitin segments), the torso, the neck and tails, which the skeleton puts in
 * its bones' profiles; the masses (a neck muscle, the limb roots) are its own.
 * Everything scales with `s = 2 × muscle` (0 to 2) and vanishes at s = 0, where a creature
 * compiles to exactly the mesh it had before.
 */

/** `s` from a muscle value (0 to 1; default 0.5 gives 1). */
export function strength(muscle: number): number {
  return Math.max(0, Math.min(2, muscle * 2));
}

/** A smooth bump: 1 at `c`, about 0.37 at `c ± w`. */
export function bump(x: number, c: number, w: number): number {
  const d = (x - c) / w;
  return Math.exp(-d * d);
}

const smoothstep = (a: number, b: number, x: number) => {
  const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

/** How a limb is shaped, per segment and joint (1 is full strength). */
export interface LimbShape {
  /** Scale of joint j's narrowing (stocky segments narrow less). */
  readonly joint?: (j: number) => number;
  /** How much segment k swells with muscle, as a share of its radius at s = 1. */
  readonly swell?: (k: number) => number;
}

/**
 * Radius multiplier along a limb, at `T` (0 at the root, 1 at the tip; joints at k/n). Ordinary
 * limbs swell with muscle in the upper part of each segment (`shape.swell`: thighs and upper arms
 * most) and narrow symmetrically at each joint, most at the last one (ankle, wrist) when there
 * are three segments or more (on two, the joint is a knee or elbow). It is all radial, so muscle
 * reads as a limb that is full near the body and tapers into its joints, never as a lump.
 * Chitin limbs swell in the middle of each segment and narrow at the joints. The tip never
 * changes, so feet stay on the ground and claws keep their size.
 */
export function limbFactor(
  T: number,
  n: number,
  s: number,
  chitin: boolean,
  shape: LimbShape = {},
): number {
  if (s <= 0) return 1;
  const keepTip = 1 - smoothstep(0.8, 1, T);
  const k = Math.min(n - 1, Math.floor(T * n));
  const t = T * n - k;
  if (chitin) {
    const swell = Math.sin(Math.PI * t);
    const f = 0.13 * s * swell - 0.06 * s * (1 - swell);
    // The last segment's far end is the tip: keep it.
    return 1 + f * (k === n - 1 ? 1 - smoothstep(0.6, 1, t) : 1);
  }
  const joint = shape.joint ?? (() => 1);
  let f = 0;
  for (let j = 1; j < n; j++)
    f -= (j === n - 1 && n >= 3 ? 0.26 : 0.14) * s * joint(j) * bump(T, j / n, 0.07);
  // The swell peaks 40% down the segment and is gone at both of its joints.
  f += s * (shape.swell?.(k) ?? 0) * Math.sin(Math.PI * t ** 0.75);
  return 1 + f * keepTip;
}

/**
 * A bone's radius profile with a multiplier applied: the base profile's points (or the bone's
 * two ends) with a midpoint added, so the shape shows between joints, or `minSpans` even spans
 * when the shape needs more (a chitin segment's arch). Unchanged when the multiplier is 1
 * everywhere (s = 0).
 */
export function shapedProfile(
  radiusAt: (t: number) => number,
  base: readonly number[] | undefined,
  factorAt: (t: number) => number,
  minSpans = 2,
): number[] | undefined {
  const spans = Math.max(base ? base.length - 1 : 2, minSpans);
  const points = Array.from({ length: spans + 1 }, (_, i) => i / spans);
  if (points.every((t) => Math.abs(factorAt(t) - 1) < 1e-9)) return base ? [...base] : undefined;
  const own = base !== undefined && base.length === spans + 1;
  return points.map((t, i) => (own ? (base[i] as number) : radiusAt(t)) * factorAt(t));
}

/**
 * How much of a segment's bellies show, from how stocky it is (radius over length): all of them
 * on slender limbs, none on columns as thick as about half their length, which muscle only
 * turns into balloons.
 */
export function slenderness(radius: number, length: number): number {
  return 1 - smoothstep(0.3, 0.55, radius / Math.max(1e-9, length));
}

/** Where the torso gets a chest, a pelvis and a waist, from the limbs on it (`at` values). */
export interface TorsoPlan {
  readonly chest: number | undefined;
  readonly pelvis: number | undefined;
  readonly waist: number | undefined;
}

/**
 * `upright` torsos (pitch 45° or more: bipeds standing tall) get no waist: on them it reads as a
 * pinch above a sagging belly.
 */
export function torsoPlan(
  limbs: readonly { readonly role: string; readonly at: number; readonly pair?: number }[],
  upright = false,
): TorsoPlan {
  const arms = limbs.filter((l) => l.role === 'arm');
  const legs = limbs.filter((l) => l.role === 'leg');
  const legAts = [...new Set(legs.map((l) => l.at))];
  const front = Math.min(...limbs.map((l) => l.at));
  const hind = Math.max(...legAts);
  const spread = legAts.length > 0 ? hind - Math.min(...legAts) : 0;
  // Pairs clustered near each other (insects, spiders) carry no chest or pelvis.
  const clustered = legAts.length >= 2 && spread < 0.3;
  const chest =
    limbs.length > 0 && !clustered && (arms.length > 0 || legAts.length >= 2)
      ? front + 0.08
      : undefined;
  const pelvis = legs.length > 0 && !clustered ? hind : undefined;
  const waist =
    !upright && chest !== undefined && pelvis !== undefined && pelvis - chest >= 0.35
      ? (chest + pelvis) / 2
      : undefined;
  return { chest, pelvis, waist };
}

/** The torso's radius multiplier at `t` (0 front, 1 back). */
export function torsoFactor(t: number, plan: TorsoPlan, s: number): number {
  if (s <= 0) return 1;
  let f = 1;
  if (plan.chest !== undefined) f += 0.04 * s * bump(t, plan.chest, 0.14);
  if (plan.pelvis !== undefined) f += 0.04 * s * bump(t, plan.pelvis, 0.12);
  if (plan.waist !== undefined) f -= 0.09 * s * bump(t, plan.waist, 0.12);
  return f;
}

/** A tail's radius multiplier at `t` (0 at the root): a muscular base. */
export function tailFactor(t: number, s: number): number {
  if (s <= 0) return 1;
  return 1 + 0.22 * s * (1 - smoothstep(0, 0.3, t));
}

/**
 * A legless body's neck radius multiplier at `at` (0 at the head, 1 at the body): a throat
 * behind the jaw, so the head reads apart from the body instead of as the end of a tube.
 */
export function throatFactor(at: number, s: number): number {
  if (s <= 0) return 1;
  return 1 - 0.2 * Math.min(1, s) * (1 - smoothstep(0, 0.45, at));
}
