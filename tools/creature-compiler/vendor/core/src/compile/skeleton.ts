import { Vector3 } from 'three';
import type { CreatureSpec, CrossSection, LimbSpec } from '../blueprint/creature.ts';
import { instanceSuffixes } from '../blueprint/instances.ts';
import type { PartModule, Registry } from '../registry.ts';
import {
  limbFactor,
  shapedProfile,
  slenderness,
  strength,
  tailFactor,
  throatFactor,
  torsoFactor,
  torsoPlan,
} from './anatomy.ts';
import { strokeOf } from './flight.ts';
import { headDetails, type MouthShape, mouthShape } from './head.ts';
import { type LimbIkSetup, solveLimb } from './ik.ts';
import type { PartHooks, SpanLimb } from './parts.ts';
import { sampleProfile } from './profile.ts';
import type {
  ArmRig,
  BoneDef,
  BoneSection,
  ChainDef,
  LegRig,
  LimbChainRig,
  MassDef,
  Rig,
  Skeleton,
  ToeChain,
  ToeContext,
  WingRig,
} from './types.ts';
import { WING_SHARES, type WingFrame, type WingHooks, wingStyleOf } from './wings.ts';

const DEG = Math.PI / 180;
const X = new Vector3(1, 0, 0);
const Y = new Vector3(0, 1, 0);
const Z = new Vector3(0, 0, 1);

export const CROSS_SCALE: Record<CrossSection, readonly [number, number]> = {
  round: [1, 1],
  tall: [0.8, 1.18],
  wide: [1.22, 0.82],
};

/** Snout radius as a share of the skull radius, and the head's own cross-section. */
const HEAD_SHAPES = {
  round: { front: 0.8, cross: [1, 1] },
  snout: { front: 0.42, cross: [0.95, 1] },
  flat: { front: 0.72, cross: [1.25, 0.72] },
  wedge: { front: 0.4, cross: [1.15, 0.78] },
} as const;

/** Leg segment lengths as shares of the whole limb, by segment count. */
const SEGMENT_SHARES: Record<number, readonly number[]> = {
  2: [0.5, 0.5],
  3: [0.4, 0.35, 0.25],
  4: [0.32, 0.3, 0.23, 0.15],
};

/** Rest bends (degrees, positive toward the pole) per joint, by limb type and segment count. */
const BENDS = {
  hind: { 2: [-50], 3: [-75, 55], 4: [-60, 50, -30] },
  front: { 2: [-45], 3: [-50, 15], 4: [-45, 25, -15] },
  sprawl: { 2: [-100], 3: [-110, 30], 4: [-100, 35, -20] },
  arm: { 2: [-40], 3: [-40, 20], 4: [-35, 20, -15] },
} as const;

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** A neck carrying arms is an upright front: a centaur's human torso. */
export function isUprightFront(spec: CreatureSpec): boolean {
  return spec.body.neck.length > 0 && spec.limbs.some((l) => l.role === 'arm' && l.on === 'neck');
}

/** A curve reparametrized by arc length (0 to 1), from 64 samples. */
function byLength(curve: (s: number) => Vector3): (s: number) => Vector3 {
  const n = 64;
  const pts = Array.from({ length: n + 1 }, (_, i) => curve(i / n));
  const acc = [0];
  for (let i = 1; i <= n; i++)
    acc.push((acc[i - 1] as number) + (pts[i] as Vector3).distanceTo(pts[i - 1] as Vector3));
  const total = acc[n] as number;
  return (s) => {
    const want = Math.min(1, Math.max(0, s)) * total;
    let i = 1;
    while (i < n && (acc[i] as number) < want) i++;
    const a = acc[i - 1] as number;
    const f = (want - a) / Math.max(1e-12, (acc[i] as number) - a);
    return curve((i - 1 + f) / n);
  };
}

/** Torso lengths beyond which a sprawled leg no longer raises the body: it arches and spreads. */
const ARCH_REACH = 0.7;

/** Radii at a few points along a bone from a section profile, when it varies within the bone. */
function boneProfile(
  values: readonly number[],
  ta: number,
  tb: number,
  scale: number,
): number[] | undefined {
  if (values.length <= 2) return undefined;
  const spans = 4;
  return Array.from(
    { length: spans + 1 },
    (_, k) => sampleProfile(values, ta + ((tb - ta) * k) / spans) * scale,
  );
}
const clamp01 = (t: number) => Math.min(1, Math.max(0, t));
const sameProfile = (a: readonly number[], b: readonly number[] | undefined) =>
  b !== undefined && a.length === b.length && a.every((v, i) => v === b[i]);

/** Dorsal "up" for a bone on the main axis whose forward (snout-ward) direction is `forward`. */
export function dorsalUp(forward: Vector3): Vector3 {
  const up = new Vector3().crossVectors(forward, X);
  if (up.lengthSq() < 1e-10) return Y.clone();
  return up.normalize();
}

/** A section path: bones with the section's `at` value at each end, for sampling. */
export interface PathSegment {
  readonly bone: number;
  readonly t0: number;
  readonly t1: number;
}

export interface SkeletonBuild extends Skeleton {
  /** Section paths by name: torso, neck, head, jaw, tail, spine, and each limb and toe id. */
  readonly paths: ReadonlyMap<string, readonly PathSegment[]>;
  /** Helper bones that take half of a joint's rotation: [helper, upper bone, lower bone]. */
  readonly helpers: readonly (readonly [number, number, number])[];
  /** Per head (as `rig.heads`), its mouth's outline at the cut; none without a jaw. */
  readonly mouths: readonly (MouthShape | undefined)[];
  /** Notes for validation warnings, e.g. legs that cannot reach the ground. */
  readonly notes: readonly {
    readonly path: string;
    readonly code: string;
    readonly message: string;
  }[];
  /** Each wing's frame, for the fold (docs/design/9.3-wings-fins.md). */
  readonly wingFrames: readonly WingFrame[];
  /** Toe chains (a wing's thumb) on wings and fins, by limb id. */
  readonly spanToes: ReadonlyMap<string, readonly (readonly number[])[]>;
  /** Wings and fins as built, by limb id, for their membranes. */
  readonly spans: ReadonlyMap<string, Omit<SpanLimb, 'legBehind'>>;
}

interface Frame {
  point: Vector3;
  /** Toward the section's at = 0 end (snout-ward on body sections). */
  forward: Vector3;
  up: Vector3;
  radius: number;
  cross: readonly [number, number];
  bone: number;
}

/** Samples a section path at `at`, extrapolating past its ends along the end bones. */
export function samplePath(
  bones: readonly BoneDef[],
  path: readonly PathSegment[],
  at: number,
): Frame {
  let seg = path[0] as PathSegment;
  let best = Number.POSITIVE_INFINITY;
  for (const s of path) {
    const lo = Math.min(s.t0, s.t1);
    const hi = Math.max(s.t0, s.t1);
    const d = at < lo ? lo - at : at > hi ? at - hi : 0;
    if (d < best - 1e-12) {
      best = d;
      seg = s;
    }
  }
  const bone = bones[seg.bone] as BoneDef;
  const f = seg.t1 === seg.t0 ? 0 : (at - seg.t0) / (seg.t1 - seg.t0);
  const point = new Vector3().lerpVectors(bone.head, bone.tail, f);
  const dir = new Vector3().subVectors(bone.tail, bone.head).normalize();
  const forward = seg.t1 < seg.t0 ? dir : dir.clone().negate();
  const up = bone.up.clone().addScaledVector(forward, -bone.up.dot(forward)).normalize();
  return {
    point,
    forward,
    up,
    radius: lerp(bone.r0, bone.r1, clamp01(f)),
    cross: bone.cross,
    bone: seg.bone,
  };
}

/** Radius of an elliptical cross-section in the direction `angle` (degrees from the top). */
export function radiusAt(
  radius: number,
  cross: readonly [number, number],
  angleDeg: number,
): number {
  const a = angleDeg * DEG;
  return radius * Math.hypot(cross[0] * Math.sin(a), cross[1] * Math.cos(a));
}

/** Direction around a section: 0 is `up`, 90 is the `mirror` side, 180 is down. */
export function aroundDirection(
  frame: { forward: Vector3; up: Vector3 },
  angleDeg: number,
  mirror: number,
): Vector3 {
  const left = new Vector3().crossVectors(frame.up, frame.forward).normalize();
  const a = angleDeg * DEG;
  return frame.up
    .clone()
    .multiplyScalar(Math.cos(a))
    .addScaledVector(left, mirror * Math.sin(a))
    .normalize();
}

class Builder {
  readonly bones: BoneDef[] = [];
  readonly chains: ChainDef[] = [];
  readonly paths = new Map<string, PathSegment[]>();
  readonly helpers: [number, number, number][] = [];

  bone(def: Omit<BoneDef, 'chain'>, chain = -1): number {
    this.bones.push({ ...def, chain });
    return this.bones.length - 1;
  }

  chain(def: Omit<ChainDef, 'bones'>, boneIds: number[]): number {
    const index = this.chains.length;
    this.chains.push({ ...def, bones: boneIds });
    for (const id of boneIds) {
      const b = this.bones[id] as BoneDef;
      this.bones[id] = { ...b, chain: index };
    }
    return index;
  }

  path(name: string, segments: PathSegment[]): void {
    this.paths.set(name, segments);
  }
}

interface BodyLayout {
  torso: { points: Vector3[]; ts: number[] };
  center: Vector3;
  pitch: number;
}

/** Torso points (back to front order not assumed) for a given pitch and centre. */
function torsoPoints(spec: CreatureSpec, pitchDeg: number, center: Vector3, count: number) {
  const L = spec.scale;
  const p = pitchDeg * DEG;
  const d = new Vector3(0, Math.sin(p), Math.cos(p));
  const u = new Vector3(0, Math.cos(p), -Math.sin(p));
  const arch = spec.body.torso.arch;
  const points: Vector3[] = [];
  const ts: number[] = [];
  for (let i = 0; i <= count; i++) {
    const t = i / count; // 0 = front (neck end), 1 = back (tail end)
    points.push(
      center
        .clone()
        .addScaledVector(d, (0.5 - t) * L)
        .addScaledVector(u, arch * L * 4 * t * (1 - t)),
    );
    ts.push(t);
  }
  return { points, ts };
}

/** Builds the rest-pose skeleton: main axis, limbs (feet on the ground) and toes. */
export function buildSkeleton(spec: CreatureSpec, registry: Registry): SkeletonBuild {
  const L = spec.scale;
  const notes: { path: string; code: string; message: string }[] = [];
  const legs = spec.limbs.filter((l) => l.role === 'leg');
  // Anatomy (docs/design/8.1-anatomy.md): s = 2 × muscle; everything below is unchanged at 0.
  const sBody = strength(spec.body.muscle);
  const chitin = spec.skin.material === 'chitin';
  // Legless bodies rest on a slightly flattened belly.
  const flatten = (c: readonly [number, number]): readonly [number, number] =>
    legs.length === 0 && sBody > 0 ? [c[0] * (1 + 0.06 * sBody), c[1] * (1 - 0.1 * sBody)] : c;
  const torsoCross = flatten(CROSS_SCALE[spec.body.torso.crossSection]);
  const torsoRadius = (t: number) => sampleProfile(spec.body.torso.radius, t) * L;
  // Upright torsos (bipeds standing tall) get no waist, which reads as a sag on them.
  const upright = Math.abs(spec.body.torso.pitch) >= 45;
  // Wings and fins carry their own shoulder (docs/design/9.3-wings-fins.md), not a chest.
  const plan = torsoPlan(
    spec.limbs.filter((l) => l.on === 'torso' && (l.role === 'leg' || l.role === 'arm')),
    upright,
  );
  const torsoSegments = spec.body.torso.segments;

  // --- Posture: torso height and pitch from where the legs need their hips ------------------
  const layoutFor = (pitch: number, center: Vector3): BodyLayout => ({
    torso: torsoPoints(spec, pitch, center, torsoSegments * 4),
    center,
    pitch,
  });
  const torsoSampler = (layout: BodyLayout) => (t: number) => {
    const pts = layout.torso.points;
    const x = clamp01(t) * (pts.length - 1);
    const i = Math.min(pts.length - 2, Math.floor(x));
    const point = new Vector3().lerpVectors(pts[i] as Vector3, pts[i + 1] as Vector3, x - i);
    const forward = new Vector3().subVectors(pts[i] as Vector3, pts[i + 1] as Vector3).normalize();
    return { point, forward, up: dorsalUp(forward), radius: torsoRadius(t), cross: torsoCross };
  };

  const posture =
    legs.length === 0 ? 'legless' : legs.some((l) => l.splay >= 35) ? 'sprawl' : 'upright';
  const lastPair = Math.max(0, ...legs.map((l) => l.pair ?? 0));
  // How high a foot holds the leg's tip: its module's say, or its stance's (no stance keeps plan
  // 1's height; docs/design/8.2-feet.md).
  const footHeightOf = (limb: LimbSpec, tipR: number) => {
    const plain = Math.max(tipR, 0.012 * L);
    const foot = limb.foot;
    const module = foot ? (registry.get('part', foot.type) as PartModule | undefined) : undefined;
    const hook = (module?.hooks as PartHooks | undefined)?.footHeight;
    if (foot && hook)
      return Math.max(
        0.012 * L,
        hook({ role: limb.role, stance: limb.stance, tipRadius: tipR, scale: L }, foot.params),
      );
    if (limb.stance === 'digitigrade') return Math.max(plain, 2.2 * tipR);
    if (limb.stance === 'unguligrade') return Math.max(plain, 3 * tipR);
    return plain;
  };
  const legPlan = (limb: LimbSpec) => {
    const R = limb.length * L;
    const sw = clamp01(limb.splay / 60);
    const tipR = (limb.radius.at(-1) ?? 0.03) * L;
    const footH = footHeightOf(limb, tipR);
    // Upright two-legged walkers stand nearly straight-legged; four-legged ones a little more
    // flexed; sprawlers low.
    const biped = lastPair === 0;
    const upright = biped ? 0.91 : limb.segments === 2 ? 0.9 : 0.87;
    // A sprawled leg (splay 35° and more, fully from 55°) holds its hip no higher than a leg of
    // ARCH_REACH torso lengths would: any length beyond that spreads the foot and raises the knee
    // above the hip, as a spider's legs arch (docs/design/9.2-legs-centaurs.md). Shorter legs,
    // and upright ones, are unchanged.
    const reach = lerp(R, Math.min(R, ARCH_REACH * L), clamp01((limb.splay - 35) / 20));
    const frac = lerp(upright, 0.45, sw);
    let v = frac * reach;
    let h = R * Math.sin(limb.splay * 0.9 * DEG) * 0.85;
    // Sprawled legs fan out along the body, front feet forward and hind feet back (further, to
    // carry the abdomen), as insects and lizards stand.
    const u = lastPair > 0 ? ((limb.pair ?? 0) / lastPair) * 2 - 1 : 0;
    let fore = sw * R * (u > 0 ? 0.25 : 0.4) * u;
    // Sprawlers keep more slack in the leg, so a foot can travel fore and aft while planted.
    const most = lerp(0.96, 0.84, sw) * R;
    const len = Math.hypot(v, h, fore);
    if (len > most) {
      v *= most / len;
      h *= most / len;
      fore *= most / len;
    }
    return { R, sw, footH, v, h, fore, tipR };
  };

  const limbRoot = (
    limb: LimbSpec,
    frame: {
      point: Vector3;
      forward: Vector3;
      up: Vector3;
      radius: number;
      cross: readonly [number, number];
    },
  ) => {
    const dir = aroundDirection(frame, limb.angle, limb.mirror);
    // Arms on an upright front hang from shoulders at the chest's edge (its profile there),
    // clear of the ribs.
    if (limb.role === 'arm' && limb.on === 'neck') {
      const r = sampleProfile(spec.body.neck.radius, limb.at) * L;
      return frame.point.clone().addScaledVector(dir, radiusAt(r, frame.cross, limb.angle) * 0.85);
    }
    return frame.point
      .clone()
      .addScaledVector(dir, radiusAt(frame.radius, frame.cross, limb.angle) * 0.55);
  };

  let pitch = spec.body.torso.pitch;
  let center = new Vector3();
  const torsoLegs = legs.filter((l) => l.on === 'torso' && l.mirror >= 0);
  if (torsoLegs.length > 0) {
    const sample = torsoSampler(layoutFor(pitch, center));
    const rows = torsoLegs.map((limb) => {
      const root = limbRoot(limb, sample(limb.at));
      const plan = legPlan(limb);
      return { y: root.y, z: root.z, want: plan.v + plan.footH };
    });
    const zs = rows.map((r) => r.z);
    const spread = Math.max(...zs) - Math.min(...zs);
    let delta = 0;
    let cy: number;
    if (spread > 0.05 * L && Math.abs(pitch) < 45) {
      // Least squares for height cy and pitch change delta: y + cy + z·delta = want.
      const n = rows.length;
      const mz = zs.reduce((a, b) => a + b, 0) / n;
      const mr = rows.reduce((a, r) => a + (r.want - r.y), 0) / n;
      let num = 0;
      let den = 0;
      for (const r of rows) {
        num += (r.z - mz) * (r.want - r.y - mr);
        den += (r.z - mz) ** 2;
      }
      // Damped toward the blueprint's pitch, strongly for sprawlers whose legs cluster near the
      // front: they keep the body level and let the legs adapt.
      const damping = n * (0.35 * L) ** 2 * (posture === 'sprawl' ? 1 : 0.15);
      delta = Math.max(-0.4, Math.min(0.4, num / (den + damping)));
      cy = mr - mz * delta;
    } else {
      cy = rows.reduce((a, r) => a + (r.want - r.y), 0) / rows.length;
    }
    pitch += delta / DEG;
    center = new Vector3(0, cy, 0);
  } else {
    // Legless: the body rests on the ground.
    let lowest = 0;
    for (let i = 0; i <= 8; i++) {
      const t = i / 8;
      lowest = Math.max(lowest, torsoRadius(t) * torsoCross[1]);
    }
    center = new Vector3(0, lowest * 0.95, 0);
    pitch = legs.length === 0 ? Math.min(pitch, 15) : pitch;
  }
  const layout = layoutFor(pitch, center);
  const sampleTorso = torsoSampler(layout);

  /** A bone's profile with an anatomy multiplier (u: 0 at the bone's head, 1 at its tail). */
  const shaped = (
    radiusAt: (u: number) => number,
    base: readonly number[] | undefined,
    factorAt: (u: number) => number,
    minSpans?: number,
  ) => {
    const profile = shapedProfile(radiusAt, base, factorAt, minSpans);
    // `shaped` marks a profile anatomy changed, which thin bones' tubes then follow too.
    return profile
      ? {
          profile,
          ...(profile !== base && !sameProfile(profile, base)
            ? { shaped: true, ...(base ? { plainProfile: base } : {}) }
            : {}),
        }
      : {};
  };

  // --- Bones ---------------------------------------------------------------------------------
  const b = new Builder();
  const root = b.bone({
    name: 'root',
    parent: -1,
    section: 'root',
    owner: 'root',
    head: new Vector3(0, 0, 0),
    tail: new Vector3(0, 0, 0.1 * L),
    up: Y.clone(),
    r0: 0,
    r1: 0,
    cross: [1, 1],
    t0: 0,
    t1: 0,
    skin: false,
  });

  // Torso: back (t = 1) to front (t = 0).
  const spine: number[] = [];
  for (let k = 0; k < torsoSegments; k++) {
    const ta = 1 - k / torsoSegments;
    const tb = 1 - (k + 1) / torsoSegments;
    const a = sampleTorso(ta);
    const c = sampleTorso(tb);
    const forward = new Vector3().subVectors(c.point, a.point).normalize();
    spine.push(
      b.bone({
        name: `spine.${k}`,
        parent: k === 0 ? root : (spine[k - 1] as number),
        section: 'torso',
        owner: 'torso',
        head: a.point,
        tail: c.point,
        up: dorsalUp(forward),
        r0: a.radius,
        r1: c.radius,
        cross: torsoCross,
        ...(torsoCross !== CROSS_SCALE[spec.body.torso.crossSection]
          ? { plainCross: CROSS_SCALE[spec.body.torso.crossSection] }
          : {}),
        t0: ta,
        t1: tb,
        skin: true,
        ...shaped(
          (u) => torsoRadius(lerp(ta, tb, u)),
          boneProfile(spec.body.torso.radius, ta, tb, L),
          (u) => torsoFactor(lerp(ta, tb, u), plan, sBody),
        ),
      }),
    );
  }
  b.chain(
    {
      id: 'torso',
      section: 'torso',
      owner: 'torso',
      parentBone: -1,
      blend: 0,
      masses: [],
    },
    spine,
  );
  b.path(
    'torso',
    spine.map((id) => ({
      bone: id,
      t0: (b.bones[id] as BoneDef).t0,
      t1: (b.bones[id] as BoneDef).t1,
    })),
  );
  const chest = spine.at(-1) as number;
  const hips = spine[0] as number;

  /**
   * Turns the bones made since `firstBone` (and the masses of chains made since `firstChain`)
   * about the vertical axis through `pivot` by `yaw` radians, then shifts them `shift` along X:
   * how extra heads and tails take their place in the fan.
   */
  const fan = (
    firstBone: number,
    firstChain: number,
    pivot: Vector3,
    yaw: number,
    shift: number,
  ) => {
    if (yaw === 0 && shift === 0) return;
    const move = (p: Vector3) =>
      p
        .clone()
        .sub(pivot)
        .applyAxisAngle(Y, yaw)
        .add(pivot)
        .add(new Vector3(shift, 0, 0));
    const turn = (v: Vector3) => v.clone().applyAxisAngle(Y, yaw);
    for (let i = firstBone; i < b.bones.length; i++) {
      const bone = b.bones[i] as BoneDef;
      b.bones[i] = { ...bone, head: move(bone.head), tail: move(bone.tail), up: turn(bone.up) };
    }
    for (let i = firstChain; i < b.chains.length; i++) {
      const chain = b.chains[i] as ChainDef;
      b.chains[i] = {
        ...chain,
        masses: chain.masses.map((m) => ({ ...m, a: move(m.a), b: move(m.b), up: turn(m.up) })),
      };
    }
  };

  // Neck: a gentle curve from the torso's front, leaving at the torso's angle, to its own pitch.
  // Several heads (docs/design/9.1-heads-tails.md) run the same builder once per instance and
  // turn what it made about the vertical through its root; the main one stays where it is.
  const neckSpec = spec.body.neck;
  const neckLen = neckSpec.length * L;
  const front = sampleTorso(0);
  const uprightFront = isUprightFront(spec);
  const buildHead = (suffix: string, yaw: number, shift: number) => {
    const neckId = `neck${suffix}`;
    const headId = `head${suffix}`;
    const jawId = `jaw${suffix}`;
    const firstBone = b.bones.length;
    const firstChain = b.chains.length;
    const root = front.point.clone().addScaledVector(front.forward, -0.02 * L);
    const neck: number[] = [];
    let headBase = front.point.clone();
    /** The muscle running from the neck into the shoulders, on bodies with limbs on the torso. */
    const neckMuscle = (): MassDef[] => {
      const first = neck[0];
      if (first === undefined || sBody <= 0 || chitin || plan.pelvis === undefined || uprightFront)
        return [];
      const bone = b.bones[first] as BoneDef;
      const r = (bone.r0 + bone.r1) / 2;
      const o = 0.15 * sBody * r;
      const rho = r * (1 + 0.15 * sBody) - o;
      const offset = bone.up.clone().multiplyScalar(o);
      return [
        {
          bone: first,
          a: new Vector3().lerpVectors(bone.head, bone.tail, 0.05).add(offset),
          b: new Vector3().lerpVectors(bone.head, bone.tail, 0.6).add(offset),
          ra: rho,
          rb: rho * 0.85,
          up: bone.up.clone(),
          cross: bone.cross,
          blend: 0.4 * r * Math.min(1, sBody),
        },
      ];
    };
    /**
     * An upright front's chest (docs/design/9.2-legs-centaurs.md): a bar across the shoulders
     * where the arms join, and pectorals below it on the front, both growing with muscle.
     */
    const frontChest = (): MassDef[] => {
      if (!uprightFront || sBody <= 0 || neck.length === 0) return [];
      const path = neck.map((id) => {
        const bone = b.bones[id] as BoneDef;
        return { bone: id, t0: bone.t0, t1: bone.t1 };
      });
      const arms = spec.limbs.filter((l) => l.role === 'arm' && l.on === 'neck');
      const at = Math.min(...arms.map((l) => l.at));
      const k = Math.min(1, sBody);
      // A bar across the front at `t`, sized from the profile there.
      const across = (t: number, width: number, lift: number, radius: number, blend: number) => {
        const frame = samplePath(b.bones, path, t);
        const r = sampleProfile(neckSpec.radius, t) * L;
        const side = new Vector3().crossVectors(frame.up, frame.forward).normalize();
        const half = r * frame.cross[0] * width;
        const centre = frame.point.clone().addScaledVector(frame.up, lift * r * frame.cross[1]);
        return {
          bone: frame.bone,
          a: centre.clone().addScaledVector(side, half),
          b: centre.clone().addScaledVector(side, -half),
          ra: radius * r,
          rb: radius * r,
          up: frame.forward.clone(),
          cross: [1, 1] as const,
          blend: blend * r * k,
        };
      };
      return [
        across(at, 0.85, 0.1, 0.42 + 0.08 * sBody, 0.5),
        across(Math.min(1, at + 0.12), 0.38, -0.42, 0.32 + 0.08 * sBody, 0.45),
      ];
    };
    if (neckLen > 1e-6) {
      const np = neckSpec.pitch * DEG;
      const ndir = new Vector3(0, Math.sin(np), Math.cos(np));
      const p0 = front.point.clone().addScaledVector(front.forward, -0.02 * L);
      // An upright front (arms on the neck: a centaur's human torso) rises nearly straight from
      // the torso's front instead of leaving along it (docs/design/9.2-legs-centaurs.md).
      const p1 = p0.clone().addScaledVector(front.forward, neckLen * (uprightFront ? 0.08 : 0.4));
      const p2 = p0.clone().addScaledVector(ndir, neckLen);
      // `curve` makes an S: today's curve raised to a cubic, its base pushed forward and down and
      // its head end back and up (a swan's neck).
      const curve = neckSpec.curve * DEG;
      const chord = new Vector3().subVectors(p2, p0).normalize();
      const dorsal = new Vector3(0, chord.z, -chord.y);
      const push = Math.sin(curve / 2) * 0.35 * neckLen;
      const c1 = p0
        .clone()
        .lerp(p1, 2 / 3)
        .addScaledVector(dorsal, -push);
      // The base may not dip into the chest.
      c1.y = Math.max(c1.y, p0.y - 0.15 * neckLen);
      const c2 = p2
        .clone()
        .lerp(p1, 2 / 3)
        .addScaledVector(dorsal, push);
      const bez =
        curve === 0
          ? (s: number) =>
              new Vector3()
                .addScaledVector(p0, (1 - s) ** 2)
                .addScaledVector(p1, 2 * s * (1 - s))
                .addScaledVector(p2, s * s)
          : (s: number) =>
              new Vector3()
                .addScaledVector(p0, (1 - s) ** 3)
                .addScaledVector(c1, 3 * s * (1 - s) ** 2)
                .addScaledVector(c2, 3 * s * s * (1 - s))
                .addScaledVector(p2, s ** 3);
      const neckCross = CROSS_SCALE[neckSpec.crossSection];
      // An upright front's bones are spaced evenly along its length, so `at` and the radius
      // profile fall where they say; other necks keep the curve's own spacing.
      const along = uprightFront ? byLength(bez) : (s: number) => bez(s);
      for (let k = 0; k < neckSpec.segments; k++) {
        const sa = k / neckSpec.segments;
        const sb = (k + 1) / neckSpec.segments;
        const a = along(sa);
        const c = along(sb);
        const forward = new Vector3().subVectors(c, a).normalize();
        neck.push(
          b.bone({
            name: `${neckId}.${k}`,
            parent: k === 0 ? chest : (neck[k - 1] as number),
            section: 'neck',
            owner: neckId,
            head: a,
            tail: c,
            up: dorsalUp(forward),
            r0: sampleProfile(neckSpec.radius, 1 - sa) * L,
            r1: sampleProfile(neckSpec.radius, 1 - sb) * L,
            ...shaped(
              (u) => sampleProfile(neckSpec.radius, 1 - (sa + (sb - sa) * u)) * L,
              boneProfile(neckSpec.radius, 1 - sa, 1 - sb, L),
              (u) => (legs.length === 0 ? throatFactor(1 - (sa + (sb - sa) * u), sBody) : 1),
            ),
            cross: neckCross,
            t0: 1 - sa,
            t1: 1 - sb,
            skin: true,
          }),
        );
      }
      headBase = p2;
      b.chain(
        {
          id: neckId,
          section: 'neck',
          owner: neckId,
          parentBone: chest,
          blend: 0.5 * Math.min(sampleProfile(neckSpec.radius, 1) * L, front.radius),
          masses: [...neckMuscle(), ...frontChest()],
        },
        neck,
      );
      b.path(
        neckId,
        neck.map((id) => ({
          bone: id,
          t0: (b.bones[id] as BoneDef).t0,
          t1: (b.bones[id] as BoneDef).t1,
        })),
      );
    }

    // Head: one bone from the skull centre to the snout centre; the jaw hangs below it.
    const headSpec = spec.body.head;
    const shape = HEAD_SHAPES[headSpec.shape];
    const headCross: [number, number] = [
      shape.cross[0] * CROSS_SCALE[headSpec.crossSection][0],
      shape.cross[1] * CROSS_SCALE[headSpec.crossSection][1],
    ];
    const hp = headSpec.pitch * DEG;
    const hd = new Vector3(0, Math.sin(hp), Math.cos(hp));
    const headUp = dorsalUp(hd);
    const r0 = headSpec.radius * L;
    const r1 = r0 * shape.front;
    const headLen = headSpec.length * L;
    const centers = Math.max(1e-4 * L, headLen - r0 - r1);
    const skull = headBase
      .clone()
      .addScaledVector(hd, r0 * 0.3)
      .addScaledVector(headUp, r0 * 0.1);
    const snout = skull.clone().addScaledVector(hd, centers);
    const total = centers + r0 + r1;
    const headParent = neck.length > 0 ? (neck.at(-1) as number) : chest;
    const head = b.bone({
      name: headId,
      parent: headParent,
      section: 'head',
      owner: headId,
      head: skull,
      tail: snout,
      up: headUp,
      r0,
      r1,
      cross: headCross,
      t0: (centers + r1) / total,
      t1: r1 / total,
      skin: true,
    });
    const headBones = [head];
    let jaw = -1;
    if (headSpec.jaw) {
      const hinge = skull
        .clone()
        .addScaledVector(hd, centers * 0.1)
        .addScaledVector(headUp, -r0 * 0.45);
      const tip = snout
        .clone()
        .addScaledVector(hd, r1 * 0.2)
        .addScaledVector(headUp, -r1 * 0.55);
      jaw = b.bone({
        name: jawId,
        parent: head,
        section: 'jaw',
        owner: jawId,
        head: hinge,
        tail: tip,
        up: dorsalUp(new Vector3().subVectors(tip, hinge).normalize()),
        r0: r0 * 0.45,
        r1: r1 * 0.6,
        cross: headCross,
        t0: 1,
        t1: 0,
        skin: true,
      });
      headBones.push(jaw);
      b.path(jawId, [{ bone: jaw, t0: 1, t1: 0 }]);
      const helper = b.bone({
        name: `${jawId}.helper`,
        parent: head,
        section: 'helper',
        owner: jawId,
        head: hinge.clone(),
        tail: hinge.clone().addScaledVector(hd, 0.05 * L),
        up: headUp.clone(),
        r0: r0 * 0.45,
        r1: r0 * 0.45,
        cross: headCross,
        t0: 1,
        t1: 1,
        skin: false,
      });
      b.helpers.push([helper, head, jaw]);
    }
    const headRootRadius = neck.length > 0 ? sampleProfile(neckSpec.radius, 0) * L : front.radius;
    const headChain = b.chain(
      {
        id: headId,
        section: 'head',
        owner: headId,
        parentBone: headParent,
        blend: 0.5 * Math.min(headRootRadius, r0),
        masses: [],
      },
      headBones,
    );
    // Turn this instance into place before anything reads its bones' positions.
    fan(firstBone, firstChain, root, yaw, shift);
    const headPath: PathSegment[] = [
      { bone: head, t0: (b.bones[head] as BoneDef).t0, t1: (b.bones[head] as BoneDef).t1 },
    ];
    b.path(headId, headPath);
    // Head details (docs/design/8.3-heads.md): lips, a brow over each eye, cheekbones, nostrils.
    const mouth =
      jaw >= 0 ? mouthShape(b.bones, b.chains[headChain] as ChainDef, head, jaw) : undefined;
    const eyePlaces = spec.parts
      .filter((p) => p.on === headId && registry.get('part', p.type)?.material === 'eye')
      .map((p) => ({ at: p.at, angle: p.angle, mirror: p.mirror }));
    b.chains[headChain] = {
      ...(b.chains[headChain] as ChainDef),
      masses: headDetails({
        bones: b.bones,
        head,
        jaw,
        shape: mouth,
        frame: (at) => samplePath(b.bones, headPath, at),
        lips: headSpec.lips,
        brow: headSpec.brow,
        eyes: eyePlaces,
      }),
    };
    return { id: headId, neck, head, jaw, mouth };
  };
  const headCount = Math.max(1, neckSpec.count);
  // Roots spread across the chest's front, necks fanned over `spread` (the layout `headSpacing`
  // checks at validation); instance 0 is the creature's left (+X).
  const headStep = headCount > 1 ? (neckSpec.spread * DEG) / (headCount - 1) : 0;
  const headRootStep = headCount > 1 ? (1.2 * front.radius) / (headCount - 1) : 0;
  const builtHeads = instanceSuffixes(headCount).map((suffix, i) => {
    const k = i - (headCount - 1) / 2;
    return buildHead(suffix, -k * headStep, -k * headRootStep);
  });
  const mainHead = builtHeads.findIndex((h) => h.id === 'head');
  const { neck } = builtHeads[mainHead] as (typeof builtHeads)[number];

  // Tail: leaves the torso's back end, then bends by `curl` after `curlStart`. Several tails
  // leave separately, fanned like heads; with `forkAt`, one trunk forks into branches that
  // continue its profile and curl (docs/design/9.1-heads-tails.md).
  const tailSpec = spec.body.tail;
  const tailLen = tailSpec.length * L;
  const tail: number[] = [];
  const builtTails: { id: string; bones: number[]; branch: number }[] = [];
  if (tailLen > 1e-6) {
    const back = sampleTorso(1);
    const tp = tailSpec.pitch * DEG;
    const td = new Vector3(0, Math.sin(tp), -Math.cos(tp));
    const n = tailSpec.segments;
    const seg = tailLen / n;
    const curling = Array.from({ length: n }, (_, k) => (k + 0.5) / n > tailSpec.curlStart);
    const curlCount = Math.max(1, curling.filter(Boolean).length);
    const tailCross = flatten(CROSS_SCALE[tailSpec.crossSection]);
    // Legged bodies' tails start muscular, never wider than the torso's end.
    const tailRadius = (t: number) => sampleProfile(tailSpec.radius, t) * L;
    const tailShape = (t: number) =>
      legs.length === 0
        ? 1
        : Math.min(tailFactor(t, sBody), Math.max(1, back.radius / tailRadius(t)));
    const start = back.point.clone().addScaledVector(back.forward, 0.02 * L);
    /** Segments `k0` to `k1` of one tail, from `pos`, hanging from `parent`. */
    const run = (k0: number, k1: number, from: Vector3, parent: number, name: string): number[] => {
      const made: number[] = [];
      let pos = from.clone();
      let bend = 0;
      for (let k = 0; k < k1; k++) {
        if (curling[k]) bend += (tailSpec.curl * DEG) / curlCount;
        if (k < k0) continue;
        const dir = td
          .clone()
          .applyAxisAngle(
            X,
            k === 0 ? 0 : bend - (curling[k] ? (tailSpec.curl * DEG) / curlCount / 2 : 0),
          );
        if (k === 0) dir.add(back.forward.clone().negate()).normalize();
        const next = pos.clone().addScaledVector(dir, seg);
        made.push(
          b.bone({
            name: `${name}.${k}`,
            parent: made.length === 0 ? parent : (made.at(-1) as number),
            section: 'tail',
            owner: name,
            head: pos,
            tail: next,
            up: dorsalUp(dir.clone().negate()),
            r0: sampleProfile(tailSpec.radius, k / n) * L,
            r1: sampleProfile(tailSpec.radius, (k + 1) / n) * L,
            ...shaped(
              (u) => tailRadius((k + u) / n),
              boneProfile(tailSpec.radius, k / n, (k + 1) / n, L),
              (u) => tailShape((k + u) / n),
            ),
            cross: tailCross,
            ...(tailCross !== CROSS_SCALE[tailSpec.crossSection]
              ? { plainCross: CROSS_SCALE[tailSpec.crossSection] }
              : {}),
            t0: k / n,
            t1: (k + 1) / n,
            skin: true,
          }),
        );
        pos = next;
      }
      return made;
    };
    const count = Math.max(1, tailSpec.count);
    const suffixes = instanceSuffixes(count);
    const step = count > 1 ? (tailSpec.spread * DEG) / (count - 1) : 0;
    // Tails point backward, so turning one toward the creature's left (+X) is a negative yaw.
    const yawOf = (i: number) => (i - (count - 1) / 2) * step;
    const fork =
      count > 1 && tailSpec.forkAt > 0
        ? Math.min(n - 1, Math.max(1, Math.round(tailSpec.forkAt * n)))
        : 0;
    const chainOf = (id: string, bones: number[], parent: number) => {
      b.chain(
        {
          id,
          section: 'tail',
          owner: id,
          parentBone: parent,
          blend:
            parent === hips
              ? 0.5 * Math.min(sampleProfile(tailSpec.radius, 0) * L, back.radius)
              : 0.5 * ((b.bones[parent] as BoneDef).r1 ?? 0),
          masses: [],
        },
        bones,
      );
    };
    const pathOf = (id: string, bones: readonly number[]) =>
      b.path(
        id,
        bones.map((bone) => ({
          bone,
          t0: (b.bones[bone] as BoneDef).t0,
          t1: (b.bones[bone] as BoneDef).t1,
        })),
      );
    if (fork === 0) {
      // Separate tails, roots spread across the rear.
      const rootStep = count > 1 ? (0.8 * back.radius) / (count - 1) : 0;
      suffixes.forEach((suffix, i) => {
        const id = `tail${suffix}`;
        const firstBone = b.bones.length;
        const firstChain = b.chains.length;
        const bones = run(0, n, start, hips, id);
        chainOf(id, bones, hips);
        fan(firstBone, firstChain, start, yawOf(i), -(i - (count - 1) / 2) * rootStep);
        pathOf(id, bones);
        builtTails.push({ id, bones, branch: 0 });
      });
    } else {
      // One trunk, then a branch per tail from the fork; the main branch continues the trunk's
      // chain, so a forked tail still swings as one from its root.
      const trunk = run(0, fork, start, hips, 'tail');
      const forkAt = (b.bones[trunk.at(-1) as number] as BoneDef).tail.clone();
      const branches = suffixes.map((suffix, i) => {
        const id = `tail${suffix}`;
        const firstBone = b.bones.length;
        const bones = run(fork, n, forkAt, trunk.at(-1) as number, id);
        fan(firstBone, b.chains.length, forkAt, yawOf(i), 0);
        return { id, bones };
      });
      for (const { id, bones } of branches) {
        const all = [...trunk, ...bones];
        if (id === 'tail') chainOf(id, all, hips);
        else chainOf(id, bones, trunk.at(-1) as number);
        pathOf(id, all);
        builtTails.push({ id, bones: all, branch: fork });
      }
    }
    const main = builtTails.find((t) => t.id === 'tail');
    if (main) tail.push(...main.bones);
  }

  // The virtual spine path: neck (from the head end) → torso → tail, by arc length.
  {
    const ordered = [...[...neck].reverse(), ...[...spine].reverse(), ...tail];
    const lens = ordered.map((id) =>
      (b.bones[id] as BoneDef).head.distanceTo((b.bones[id] as BoneDef).tail),
    );
    const sum = lens.reduce((a, c) => a + c, 0) || 1;
    let acc = 0;
    const segs: PathSegment[] = [];
    for (const [i, id] of ordered.entries()) {
      const bone = b.bones[id] as BoneDef;
      const len = lens[i] as number;
      // Neck and torso bones point snout-ward, so their head is the far end of the segment.
      const towardTail = bone.section === 'tail';
      const ta = acc / sum;
      const tb = (acc + len) / sum;
      segs.push({ bone: id, t0: towardTail ? ta : tb, t1: towardTail ? tb : ta });
      acc += len;
    }
    b.path('spine', segs);
  }

  // --- Limbs -----------------------------------------------------------------------------------
  const legRigs: LegRig[] = [];
  const armRigs: ArmRig[] = [];
  const wingRigs: WingRig[] = [];
  const finRigs: LimbChainRig[] = [];
  const wingFrames: WingFrame[] = [];
  const spanToes = new Map<string, number[][]>();
  const spans = new Map<string, Omit<SpanLimb, 'legBehind'>>();
  const frontPair = Math.max(-1, ...legs.map((l) => l.pair ?? -1));
  const forwardH = Z.clone();

  /**
   * A wing or fin (docs/design/9.3-wings-fins.md): built spread in its own plane as swept tubes
   * (never in the field), with a shoulder mass on the body, digits from its membrane and, on a
   * wing, a thumb from its foot. The fold comes later, in compile.
   */
  const spanLimb = (limb: LimbSpec, frame: Frame) => {
    const fin = limb.role === 'fin';
    const style = wingStyleOf(limb, registry);
    const mirror = limb.mirror;
    const normalOut = aroundDirection(frame, limb.angle, mirror);
    // The shoulder joint stands just off the skin, the pectoral mass joining it to the body, so
    // a wing folded along the body clears it.
    const rootRadiusL = sampleProfile(limb.radius, 0) * L;
    const rootPos = frame.point
      .clone()
      .addScaledVector(
        normalOut,
        radiusAt(frame.radius, frame.cross, limb.angle) + (fin ? -0.3 : 1.1) * rootRadiusL,
      );
    const sideRaw = new Vector3().crossVectors(frame.up, frame.forward).normalize();
    const side = new Vector3(sideRaw.x, 0, sideRaw.z).multiplyScalar(mirror || 1);
    if (side.lengthSq() < 1e-8) side.set(mirror || 1, 0, 0);
    side.normalize();
    // Straight out: mostly sideways, partly off the skin, so a wing high on the back rises.
    const out =
      mirror === 0
        ? normalOut.clone()
        : side.clone().multiplyScalar(0.6).addScaledVector(normalOut, 0.4).normalize();
    let normal = new Vector3().crossVectors(out, forwardH);
    if (normal.lengthSq() < 1e-6) normal = Y.clone();
    normal.normalize();
    if (normal.y < 0) normal.negate();
    const lead = new Vector3().crossVectors(normal, out).normalize();
    if (lead.dot(forwardH) < 0) lead.negate();
    const R = limb.length * L;
    const shares = WING_SHARES[limb.segments] ?? (WING_SHARES[3] as readonly number[]);
    const n = shares.length;
    const points = [rootPos.clone()];
    for (let k = 0; k < n; k++) {
      const a = ((style.bind[Math.min(k, style.bind.length - 1)] ?? 0) * Math.PI) / 180;
      const dir = out.clone().multiplyScalar(Math.cos(a)).addScaledVector(lead, Math.sin(a));
      points.push((points[k] as Vector3).clone().addScaledVector(dir, (shares[k] as number) * R));
    }
    // A fin is flat across its plane (a flipper alone, or a thin leading ray before its
    // membrane); a wing's arm is round.
    const cross: readonly [number, number] = fin
      ? limb.membrane
        ? [1.3, 0.35]
        : [1.6, 0.3]
      : [1, 1];
    const thin = fin && limb.membrane ? 0.5 : 1;
    const limbBones: number[] = [];
    for (let k = 0; k < n; k++) {
      limbBones.push(
        b.bone({
          name: `${limb.id}.${k}`,
          parent: k === 0 ? frame.bone : (limbBones[k - 1] as number),
          section: 'limb',
          owner: limb.id,
          head: (points[k] as Vector3).clone(),
          tail: (points[k + 1] as Vector3).clone(),
          up: normal.clone(),
          r0: sampleProfile(limb.radius, k / n) * L * thin,
          r1: sampleProfile(limb.radius, (k + 1) / n) * L * thin,
          cross,
          t0: k / n,
          t1: (k + 1) / n,
          // A hard case is its own arm: its bones carry the shell and draw nothing.
          skin: style.shell !== true,
          tube: true,
        }),
      );
    }
    // Elbow and wrist helpers, as arms have, for the skin around a folded joint.
    for (let k = 1; k < n && !fin; k++) {
      const upper = limbBones[k - 1] as number;
      const lower = limbBones[k] as number;
      const joint = (b.bones[lower] as BoneDef).head;
      const helper = b.bone({
        name: `${limb.id}.${k}.helper`,
        parent: upper,
        section: 'helper',
        owner: limb.id,
        head: joint.clone(),
        tail: joint
          .clone()
          .addScaledVector(
            new Vector3().subVectors(joint, (b.bones[upper] as BoneDef).head).normalize(),
            0.02 * L,
          ),
        up: normal.clone(),
        r0: (b.bones[lower] as BoneDef).r0,
        r1: (b.bones[lower] as BoneDef).r0,
        cross: [1, 1],
        t0: k / n,
        t1: k / n,
        skin: false,
      });
      b.helpers.push([helper, upper, lower]);
    }
    b.chain(
      {
        id: limb.id,
        section: 'limb',
        owner: limb.id,
        parentBone: frame.bone,
        blend: 0,
        masses: [],
      },
      limbBones,
    );
    b.path(
      limb.id,
      limbBones.map((id, k) => ({ bone: id, t0: k / n, t1: (k + 1) / n })),
    );
    // The shoulder: a mass on the body where the arm leaves the skin, and with muscle a flight
    // muscle down the chest under it.
    const rootRadius = sampleProfile(limb.radius, 0) * L;
    const parentChain = (b.bones[frame.bone] as BoneDef).chain;
    const host = b.chains[parentChain];
    // Fins grow flush from the skin, with no shoulder.
    if (host && !fin) {
      const sLimb = strength(limb.muscle);
      const masses: MassDef[] = [
        // From inside the skin out to the joint, tapering, blended wide: a shoulder, not a ball.
        {
          bone: frame.bone,
          a: rootPos.clone().addScaledVector(normalOut, -1.6 * rootRadius),
          b: rootPos.clone(),
          ra: rootRadius * 1.2,
          rb: rootRadius * 0.95,
          up: Y.clone(),
          cross: [1, 1],
          blend: rootRadius,
        },
      ];
      if (sLimb > 0) {
        const below = frame.point
          .clone()
          .addScaledVector(normalOut, -0.2 * frame.radius)
          .addScaledVector(Y, -0.5 * frame.radius);
        masses.push({
          bone: frame.bone,
          a: rootPos.clone().addScaledVector(normalOut, -0.3 * rootRadius),
          b: below,
          ra: 0.25 * sLimb * frame.radius,
          rb: 0.18 * sLimb * frame.radius,
          up: Y.clone(),
          cross: [1, 1],
          blend: 0.4 * frame.radius * Math.min(1, sLimb),
        });
      }
      b.chains[parentChain] = { ...host, masses: [...host.masses, ...masses] };
    }

    const wristBone = limbBones[Math.max(0, n - 2)] as number;
    const wrist = n >= 2 ? (points[n - 1] as Vector3).clone() : rootPos.clone();
    const tip = (points[n] as Vector3).clone();
    const handDir = new Vector3().subVectors(tip, points[n - 1] as Vector3).normalize();
    const tipRadius = sampleProfile(limb.radius, 1) * L;
    // Digits from the membrane module.
    const digits: { bones: number[]; root: 'wrist' | 'tip' }[] = [];
    const membrane = limb.membrane;
    const mModule = membrane
      ? (registry.get('part', membrane.type) as PartModule | undefined)
      : undefined;
    const digitHook = (mModule?.hooks as WingHooks | undefined)?.digits;
    if (membrane && digitHook) {
      const chains = digitHook(
        {
          wrist: wrist.clone(),
          tip: tip.clone(),
          out: out.clone(),
          lead: lead.clone(),
          normal: normal.clone(),
          hand: handDir.clone(),
          armLength: R,
          tipRadius,
          scale: L,
        },
        membrane.params as Record<string, unknown>,
      );
      chains.forEach((chain, di) => {
        const ids: number[] = [];
        const m = chain.points.length - 1;
        for (let k = 0; k < m; k++) {
          ids.push(
            b.bone({
              name: `${limb.id}.d${di}.${k}`,
              parent:
                k === 0
                  ? chain.root === 'wrist'
                    ? wristBone
                    : (limbBones.at(-1) as number)
                  : (ids[k - 1] as number),
              section: 'digit',
              owner: `${limb.id}.d${di}`,
              head: (chain.points[k] as Vector3).clone(),
              tail: (chain.points[k + 1] as Vector3).clone(),
              up: normal.clone(),
              r0: chain.radii[k] ?? tipRadius * 0.4,
              r1: chain.radii[k + 1] ?? tipRadius * 0.2,
              cross: [1, 1],
              t0: k / m,
              t1: (k + 1) / m,
              skin: true,
              tube: true,
            }),
          );
        }
        if (ids.length === 0) return;
        digits.push({ bones: ids, root: chain.root });
        b.chain(
          {
            id: `${limb.id}.d${di}`,
            section: 'limb',
            owner: `${limb.id}.d${di}`,
            parentBone: ids[0] === undefined ? wristBone : (b.bones[ids[0]] as BoneDef).parent,
            blend: 0,
            masses: [],
          },
          ids,
        );
        b.path(
          `${limb.id}.d${di}`,
          ids.map((id, k) => ({ bone: id, t0: k / ids.length, t1: (k + 1) / ids.length })),
        );
      });
    }
    // A thumb from the foot, at the wrist.
    const foot = limb.foot;
    const fModule = foot ? (registry.get('part', foot.type) as PartModule | undefined) : undefined;
    const toeHook = (
      fModule?.hooks as
        | { toes?: (ctx: ToeContext, p: Record<string, unknown>) => ToeChain[] }
        | undefined
    )?.toes;
    const toes: number[][] = [];
    if (foot && toeHook && !fin) {
      toeHook(
        {
          ankle: wrist.clone(),
          limbDir: lead.clone(),
          forward: forwardH.clone(),
          outward: side.clone(),
          groundY: 0,
          role: limb.role,
          tipRadius,
          scale: L,
          mirror,
          splay: 0,
          stance: undefined,
          footHeight: 0,
        },
        foot.params,
      ).forEach((toe, ti) => {
        const ids: number[] = [];
        for (let k = 0; k + 1 < toe.points.length; k++) {
          ids.push(
            b.bone({
              name: `${limb.id}.toe${ti}.${k}`,
              parent: k === 0 ? wristBone : (ids[k - 1] as number),
              section: 'toe',
              owner: `${limb.id}.toe${ti}`,
              head: (toe.points[k] as Vector3).clone(),
              tail: (toe.points[k + 1] as Vector3).clone(),
              up: normal.clone(),
              r0: toe.radii[k] ?? tipRadius * 0.4,
              r1: toe.radii[k + 1] ?? tipRadius * 0.3,
              cross: [1, 0.8],
              t0: k / (toe.points.length - 1),
              t1: (k + 1) / (toe.points.length - 1),
              skin: true,
              tube: true,
            }),
          );
        }
        if (ids.length === 0) return;
        toes.push(ids);
        b.chain(
          {
            id: `${limb.id}.toe${ti}`,
            section: 'toe',
            owner: `${limb.id}.toe${ti}`,
            parentBone: wristBone,
            blend: 0,
            masses: [],
          },
          ids,
        );
        b.path(
          `${limb.id}.toe${ti}`,
          ids.map((id, k) => ({ bone: id, t0: k / ids.length, t1: (k + 1) / ids.length })),
        );
      });
    }
    spanToes.set(limb.id, toes);
    spans.set(limb.id, {
      role: fin ? 'fin' : 'wing',
      on: limb.on,
      at: limb.at,
      angle: limb.angle,
      mirror,
      out: out.clone(),
      lead: lead.clone(),
      normal: normal.clone(),
      bones: limbBones,
      digits,
      armLength: R,
      tipRadius,
    });
    const sideName = limb.side === 'center' ? 'center' : limb.side;
    if (fin) {
      finRigs.push({ id: limb.id, side: sideName, bones: limbBones });
      return;
    }
    let span = 0;
    for (const id of [...limbBones, ...digits.flatMap((d) => d.bones)])
      span = Math.max(span, rootPos.distanceTo((b.bones[id] as BoneDef).tail));
    const covered = spec.limbs.some(
      (other) =>
        other.role === 'wing' &&
        other.on === limb.on &&
        other.mirror === mirror &&
        other.at < limb.at &&
        other.membrane !== null &&
        (registry.get('part', other.membrane.type) as PartModule | undefined)?.provides?.includes(
          'cover',
        ) === true,
    );
    wingFrames.push({
      wing: wingRigs.length,
      limb: limb.id,
      mirror,
      at: limb.at,
      root: rootPos.clone(),
      rootRadius,
      out: out.clone(),
      lead: lead.clone(),
      normal: normal.clone(),
      side: mirror === 0 ? normalOut.clone().setY(0).normalize() : side.clone(),
      rootNormal: normalOut.clone(),
      style,
      bones: limbBones,
      digits,
      covered,
    });
    wingRigs.push({
      id: limb.id,
      side: sideName,
      bones: limbBones,
      digits: digits.map((d) => d.bones),
      feathers: [],
      normal: [normal.x, normal.y, normal.z],
      span,
      area: 0,
      poses: {},
      ...(covered ? { covered } : {}),
      // Compile fills in how it lifts and strokes, once the membranes are built.
      lift: !style.shell,
      stroke: strokeOf(undefined, 0),
    });
  };

  /**
   * A tentacle (docs/design/9.4-tentacles-parts.md): a chain of even bones from just inside the
   * skin, leaving along the surface and on toward the section's far end (behind on the torso,
   * forward on the head), straight for `curlStart` and then turning by `curl` toward the belly.
   * Thin bones become swept tubes; the chain hangs on a spring. A tentacle that would reach into
   * the ground is turned up at its root until it rests on it.
   */
  const tentacleRigs: LimbChainRig[] = [];
  const tentacleLimb = (limb: LimbSpec, frame: ReturnType<typeof samplePath>) => {
    const out = aroundDirection(frame, limb.angle, limb.mirror);
    // On toward the section's far end: back along the torso, neck and tail, forward round the
    // mouth on the head.
    const along = limb.on === 'head' ? frame.forward.clone() : frame.forward.clone().negate();
    const dir0 = out.clone().multiplyScalar(0.3).addScaledVector(along, 0.7).normalize();
    const belly = frame.up.clone().negate();
    const axis = new Vector3().crossVectors(dir0, belly);
    if (axis.lengthSq() < 1e-8) axis.crossVectors(dir0, frame.forward);
    axis.normalize();
    const n = limb.segments;
    const R = limb.length * L;
    const straight = Math.min(n - 1, Math.round(limb.curlStart * n));
    const turn = (limb.curl * DEG) / Math.max(1, n - straight);
    const root = frame.point
      .clone()
      .addScaledVector(out, radiusAt(frame.radius, frame.cross, limb.angle) * 0.7);
    // Curling toward the belly until it meets the ground, then lying on it and curling on
    // across it, away from the midline.
    const points = [root.clone()];
    const ups: Vector3[] = [];
    const dir = dir0.clone();
    const ax = axis.clone();
    const sideways = new Vector3(0, Math.sign(out.x || dir0.x || 1), 0);
    let grounded = false;
    for (let k = 0; k < n; k++) {
      const r = sampleProfile(limb.radius, (k + 1) / n) * L;
      if (k >= straight) dir.applyAxisAngle(ax, turn);
      const prev = points[k] as Vector3;
      let next = prev.clone().addScaledVector(dir, R / n);
      if (!grounded && next.y < r) {
        grounded = true;
        ax.copy(sideways).multiplyScalar(Math.sign(limb.curl) || 1);
      }
      if (grounded) {
        dir.y = 0;
        if (dir.lengthSq() < 1e-8) dir.copy(along).setY(0);
        dir.normalize();
        next = prev.clone().addScaledVector(dir, R / n);
        next.y = Math.max(r, Math.min(prev.y, next.y));
      }
      points.push(next);
      ups.push(new Vector3().crossVectors(dir, ax).normalize());
    }
    const chain = { points, ups };
    const bones: number[] = [];
    for (let k = 0; k < n; k++)
      bones.push(
        b.bone({
          name: `${limb.id}.${k}`,
          parent: k === 0 ? frame.bone : (bones[k - 1] as number),
          section: 'limb',
          owner: limb.id,
          head: (chain.points[k] as Vector3).clone(),
          tail: (chain.points[k + 1] as Vector3).clone(),
          up: (chain.ups[k] as Vector3).clone(),
          r0: sampleProfile(limb.radius, k / n) * L,
          r1: sampleProfile(limb.radius, (k + 1) / n) * L,
          cross: [1, 1],
          t0: k / n,
          t1: (k + 1) / n,
          skin: true,
        }),
      );
    b.chain(
      {
        id: limb.id,
        section: 'limb',
        owner: limb.id,
        parentBone: frame.bone,
        blend: 0.6 * sampleProfile(limb.radius, 0) * L,
        masses: [],
      },
      bones,
    );
    b.path(
      limb.id,
      bones.map((id, k) => ({ bone: id, t0: k / n, t1: (k + 1) / n })),
    );
    tentacleRigs.push({
      id: limb.id,
      side: limb.mirror === 1 ? 'left' : limb.mirror === -1 ? 'right' : 'center',
      bones,
    });
  };

  for (const limb of spec.limbs) {
    const path = b.paths.get(limb.on);
    if (!path) continue;
    const frame = samplePath(b.bones, path, limb.at);
    if (limb.role === 'wing' || limb.role === 'fin') {
      spanLimb(limb, frame);
      continue;
    }
    if (limb.role === 'tentacle') {
      tentacleLimb(limb, frame);
      continue;
    }
    const rootPos = limbRoot(limb, frame);
    const R = limb.length * L;
    const shares = SEGMENT_SHARES[limb.segments] ?? (SEGMENT_SHARES[3] as readonly number[]);
    const lengths = shares.map((s) => s * R);
    const side = new Vector3()
      .crossVectors(frame.up, frame.forward)
      .normalize()
      .multiplyScalar(limb.mirror || 1);
    const outward = new Vector3(side.x, 0, side.z);
    if (outward.lengthSq() < 1e-8) outward.set(limb.mirror || 1, 0, 0);
    outward.normalize();
    const segKey = String(limb.segments) as '2' | '3' | '4';
    const isFront =
      limb.role === 'leg' && legs.length > 2 && limb.pair === frontPair && Math.abs(pitch) < 45;
    const sLimb = strength(limb.muscle);

    let target: Vector3;
    let pole: Vector3;
    let bendsDeg: readonly number[];
    let plan: ReturnType<typeof legPlan> | undefined;
    if (limb.role === 'leg') {
      plan = legPlan(limb);
      target = new Vector3(rootPos.x, plan.footH, rootPos.z)
        .addScaledVector(outward, plan.h)
        .addScaledVector(forwardH, plan.fore);
      const knee = isFront ? forwardH.clone().negate() : forwardH.clone();
      pole = knee
        .multiplyScalar(1 - plan.sw)
        .addScaledVector(Y, plan.sw)
        .addScaledVector(outward, plan.sw * 0.5)
        .normalize();
      const base = (isFront ? BENDS.front : BENDS.hind)[segKey];
      const sprawl = BENDS.sprawl[segKey];
      bendsDeg = base.map((v, i) => lerp(v, sprawl[i] as number, plan?.sw ?? 0));
    } else {
      // Hanging by default; `lift` swings the arm forward and up.
      const lift = (limb.lift * Math.PI) / 180;
      const down = Y.clone()
        .multiplyScalar(-Math.cos(lift))
        .addScaledVector(forwardH, Math.sin(lift));
      target = rootPos
        .clone()
        .addScaledVector(down, R * 0.8)
        .addScaledVector(forwardH, R * 0.18 * Math.cos(lift))
        .addScaledVector(outward, R * 0.12);
      pole = forwardH
        .clone()
        .multiplyScalar(-Math.cos(lift))
        .addScaledVector(Y, -Math.sin(lift))
        .addScaledVector(outward, 0.3)
        .normalize();
      bendsDeg = BENDS.arm[segKey];
    }
    const setup: LimbIkSetup = { lengths, bends: bendsDeg.map((v) => v * DEG) };
    const solved = solveLimb(setup, rootPos, target, pole);
    if (limb.role === 'leg' && solved.miss > 0.02 * L && target.distanceTo(rootPos) > R) {
      notes.push({
        path: `limbs[id=${limb.baseId}]`,
        code: 'leg_too_short',
        message: `the leg is ${(solved.miss / L).toFixed(2)} torso lengths too short to reach the ground`,
      });
    }

    const limbBones: number[] = [];
    const n = lengths.length;
    // How slender each segment is: stocky ones get fewer bellies, and their joints narrow less
    // and carry smaller caps, which on a column read as knobs.
    const slender = Array.from({ length: n }, (_, k) =>
      slenderness(
        sampleProfile(limb.radius, (k + 0.5) / n) * L,
        (solved.points[k] as Vector3).distanceTo(solved.points[k + 1] as Vector3),
      ),
    );
    const jointScale = (j: number) =>
      Math.min(slender[j - 1] as number, slender[Math.min(j, n - 1)] as number);
    // Thighs and upper arms swell most, more when the limb is thin for the body it joins (they
    // are much thicker than the limb below them); calves and forearms less; feet and hands not.
    const rootRadiusL = sampleProfile(limb.radius, 0.5 / n) * L;
    const bulk = (limb.role === 'arm' ? 0.32 : 0.42) * frame.radius;
    const thin = Math.max(0, Math.min(1, (bulk - rootRadiusL) / rootRadiusL));
    const swell = (k: number) =>
      (k === 0 ? 0.28 + 0.22 * thin : k === 1 ? 0.14 : 0) * (slender[k] as number);
    const shape = { joint: jointScale, swell };
    for (let k = 0; k < n; k++) {
      const a = solved.points[k] as Vector3;
      const c = solved.points[k + 1] as Vector3;
      const dir = new Vector3().subVectors(c, a).normalize();
      const faceRef = forwardH.clone().addScaledVector(dir, -forwardH.dot(dir));
      const face = faceRef.lengthSq() > 1e-8 ? faceRef.normalize() : Y.clone();
      limbBones.push(
        b.bone({
          name: `${limb.id}.${k}`,
          parent: k === 0 ? frame.bone : (limbBones[k - 1] as number),
          section: 'limb',
          owner: limb.id,
          head: a.clone(),
          tail: c.clone(),
          up: face,
          r0: sampleProfile(limb.radius, k / n) * L,
          r1: sampleProfile(limb.radius, (k + 1) / n) * L,
          ...shaped(
            (u) => sampleProfile(limb.radius, (k + u) / n) * L,
            boneProfile(limb.radius, k / n, (k + 1) / n, L),
            (u) => limbFactor((k + u) / n, n, sLimb, chitin, shape),
            // Four spans, so a segment's swell and its joints' narrowing come out smooth.
            4,
          ),
          cross: [1, 1],
          t0: k / n,
          t1: (k + 1) / n,
          skin: true,
        }),
      );
    }
    // Knee, hock and elbow helpers.
    for (let k = 1; k < n; k++) {
      const upper = limbBones[k - 1] as number;
      const lower = limbBones[k] as number;
      const joint = (b.bones[lower] as BoneDef).head;
      const helper = b.bone({
        name: `${limb.id}.${k}.helper`,
        parent: upper,
        section: 'helper',
        owner: limb.id,
        head: joint.clone(),
        tail: joint
          .clone()
          .addScaledVector(
            new Vector3().subVectors(joint, (b.bones[upper] as BoneDef).head).normalize(),
            0.02 * L,
          ),
        up: (b.bones[upper] as BoneDef).up.clone(),
        r0: (b.bones[lower] as BoneDef).r0,
        r1: (b.bones[lower] as BoneDef).r0,
        cross: [1, 1],
        t0: k / n,
        t1: k / n,
        skin: false,
      });
      b.helpers.push([helper, upper, lower]);
    }
    const rootRadius = sampleProfile(limb.radius, 0) * L;
    b.chain(
      {
        id: limb.id,
        section: 'limb',
        owner: limb.id,
        parentBone: frame.bone,
        blend: 0.5 * Math.min(rootRadius, frame.radius),
        masses: [
          {
            bone: limbBones[0] as number,
            a: rootPos.clone(),
            b: rootPos.clone(),
            // The shoulder or hip melds into the body and the limb with muscle (a plain union at
            // 0); it does not grow, which turns short legs into balls.
            ra: rootRadius * 1.12,
            rb: rootRadius * 1.12,
            up: Y.clone(),
            cross: [1, 1],
            blend: 0.4 * rootRadius * Math.min(1, sLimb),
          },
        ],
      },
      limbBones,
    );
    b.path(
      limb.id,
      limbBones.map((id, k) => ({ bone: id, t0: k / n, t1: (k + 1) / n })),
    );

    // Toes from the foot part.
    const toes: number[][] = [];
    const foot = limb.foot;
    const module = foot ? (registry.get('part', foot.type) as PartModule | undefined) : undefined;
    const hooks = module?.hooks as
      | { toes?: (ctx: ToeContext, p: Record<string, unknown>) => ToeChain[] }
      | undefined;
    if (foot && hooks?.toes) {
      const last = b.bones[limbBones.at(-1) as number] as BoneDef;
      const ctx: ToeContext = {
        ankle: last.tail.clone(),
        limbDir: new Vector3().subVectors(last.tail, last.head).normalize(),
        forward: forwardH.clone(),
        outward: outward.clone(),
        groundY: 0,
        role: limb.role,
        tipRadius: last.r1,
        scale: L,
        mirror: limb.mirror,
        splay: limb.splay,
        stance: limb.role === 'leg' ? limb.stance : undefined,
        footHeight: limb.role === 'leg' ? last.tail.y : 0,
      };
      hooks.toes(ctx, foot.params).forEach((toe, ti) => {
        const ids: number[] = [];
        for (let k = 0; k + 1 < toe.points.length; k++) {
          const a = toe.points[k] as Vector3;
          const c = toe.points[k + 1] as Vector3;
          const dir = new Vector3().subVectors(c, a).normalize();
          const upRef = Y.clone().addScaledVector(dir, -dir.y);
          ids.push(
            b.bone({
              name: `${limb.id}.toe${ti}.${k}`,
              parent: k === 0 ? (limbBones.at(-1) as number) : (ids[k - 1] as number),
              section: 'toe',
              owner: `${limb.id}.toe${ti}`,
              head: a.clone(),
              tail: c.clone(),
              up: upRef.lengthSq() > 1e-8 ? upRef.normalize() : forwardH.clone(),
              r0: toe.radii[k] ?? last.r1 * 0.4,
              r1: toe.radii[k + 1] ?? last.r1 * 0.3,
              cross: [1, 0.8],
              t0: k / (toe.points.length - 1),
              t1: (k + 1) / (toe.points.length - 1),
              skin: true,
            }),
          );
        }
        if (ids.length === 0) return;
        toes.push(ids);
        b.chain(
          {
            id: `${limb.id}.toe${ti}`,
            section: 'toe',
            owner: `${limb.id}.toe${ti}`,
            parentBone: limbBones.at(-1) as number,
            blend: 0.5 * Math.min(toe.radii[0] ?? last.r1, last.r1),
            masses: [],
          },
          ids,
        );
        b.path(
          `${limb.id}.toe${ti}`,
          ids.map((id, k) => ({ bone: id, t0: k / ids.length, t1: (k + 1) / ids.length })),
        );
      });
    }

    const sideName = limb.side === 'center' ? 'center' : limb.side;
    if (limb.role === 'leg' && limb.pair !== undefined && sideName !== 'center') {
      legRigs.push({
        id: limb.id,
        pair: limb.pair,
        side: sideName,
        bones: limbBones,
        lengths,
        bends: setup.bends,
        restFoot: (solved.points.at(-1) as Vector3).clone(),
        pole,
        reach: R,
        toes,
        ...(limb.stance ? { stance: limb.stance } : {}),
      });
    } else {
      armRigs.push({
        id: limb.id,
        side: sideName,
        bones: limbBones,
        lengths,
        bends: setup.bends,
        pole,
        reach: R,
        toes,
      });
    }
  }

  const legRoots = legRigs.map((l) => (b.bones[l.bones[0] as number] as BoneDef).head.y);
  const rig: Rig = {
    root,
    spine,
    // Eyes join after the parts.
    heads: builtHeads.map((h) => ({ id: h.id, neck: h.neck, head: h.head, jaw: h.jaw, eyes: [] })),
    main: mainHead,
    tails: builtTails,
    // Each tail swings on its own spring; a forked tail's branches hang from its trunk, which
    // swings with the main branch, so the trunk springs first.
    chains: builtTails
      .map((t) => ({
        owner: t.id,
        bones: t.id === 'tail' ? t.bones : t.bones.slice(t.branch),
        drive: 'spring' as const,
        stiffness: 0.35,
        swish: true,
      }))
      .filter((c) => c.bones.length > 1)
      .sort((a, b) => (a.owner === 'tail' ? -1 : b.owner === 'tail' ? 1 : 0))
      // Tentacles hang on softer springs (docs/design/9.4-tentacles-parts.md).
      .concat(
        tentacleRigs.map((t) => ({
          owner: t.id,
          bones: [...t.bones],
          drive: 'spring' as const,
          stiffness: 0.18,
          swish: false as boolean,
        })),
      ),
    legs: legRigs,
    arms: armRigs,
    wings: wingRigs,
    fins: finRigs,
    tentacles: tentacleRigs,
    stations: [],
    hipHeight:
      legRoots.length > 0 ? legRoots.reduce((a, c) => a + c, 0) / legRoots.length : center.y,
    posture,
  };
  return {
    bones: b.bones,
    chains: b.chains,
    rig,
    paths: b.paths,
    helpers: b.helpers,
    notes,
    wingFrames,
    spanToes,
    spans,
    mouths: builtHeads.map((h) => h.mouth),
  };
}

export type { BoneSection };
