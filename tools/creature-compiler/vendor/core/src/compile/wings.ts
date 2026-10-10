import { Matrix4, Quaternion, Vector3 } from 'three';
import type { LimbSpec } from '../blueprint/creature.ts';
import type { PartModule, Registry } from '../registry.ts';
import type { BoneDef } from './types.ts';

/**
 * Wings and fins (docs/design/9.3-wings-fins.md): how a wing is shaped spread (its bind pose)
 * and how it folds at rest. A membrane module may give its own style through `hooks.wing`;
 * without one a wing is shaped and folds as a bat's, so the core never names a module.
 */
export interface WingStyle {
  /**
   * The bind pose: each arm segment's direction in the wing's plane, in degrees from straight
   * out toward the leading edge (negative sweeps back). Shorter lists repeat their last value.
   */
  readonly bind: readonly number[];
  readonly fold: {
    /** Where the folded humerus points: degrees from outward toward the back. */
    readonly sweep: number;
    /** Degrees the folded humerus points below horizontal (negative rises). */
    readonly droop: number;
    /**
     * The folded wing's plane: 0 tangent to the body at the root (lying on the flank or back,
     * its leading edge facing down, so the forearm folds forward below the humerus), 90 flat over
     * the back (its leading edge outward); in between, a roof. Negative turns it toward the
     * flank instead, so a wing rooted high on the back folds down the side.
     */
    readonly lie: number;
    /** In-plane turns at each joint after the first (elbow, wrist, …), toward the leading edge. */
    readonly joints: readonly number[];
    /** Degrees between neighbouring folded digits. */
    readonly digits: number;
    /** How far each digit's later joints flex, in a Z, so the folded bundle stays short. */
    readonly flex: number;
  };
  /**
   * Layer when several wings fold over each other (insects): lower folds first and lies
   * lowest; the left wing of a pair lies above the right.
   */
  readonly stack: number;
  /** Plate thickness as a share of the torso length, for stacking. */
  readonly thickness: number;
  /**
   * A hard case shaped where it rests (a beetle's elytron): it folds onto the body without
   * lifting clear or stacking, and a pair meets at the midline instead of overlapping.
   */
  readonly shell?: boolean;
  /** How it strokes in flight (docs/design/10.4-flight.md); left out, the bat's. */
  readonly stroke?: WingStroke;
}

/** A wing's stroke in flight, in degrees and shares. */
export interface WingStroke {
  /** Half the stroke's swing about the wing's leading-edge axis. */
  readonly amplitude: number;
  /**
   * How far the outer wing folds in its own plane on the upstroke, as a share of the fold's
   * elbow and wrist turns (birds and bats half-fold; insects 0).
   */
  readonly flex: number;
  /** How far the wing pitches about its own length, leading edge down on the downstroke. */
  readonly twist: number;
  /** Stroke plane, from vertical (0) toward the wing's plane (90, a hover's sweep). */
  readonly plane: number;
  /** A wing that does not beat but is held in flight (a case): lifted and swung forward. */
  readonly hold?: { readonly lift: number; readonly forward: number };
}

/** A bat's wing: a shallow M spread, a Z fold against the flank. */
export const BAT_STYLE: WingStyle = {
  bind: [-10, 15, -5],
  fold: { sweep: 90, droop: 0, lie: 0, joints: [165, -160, 0], digits: 3, flex: 10 },
  stack: 0,
  thickness: 0.005,
  stroke: { amplitude: 50, flex: 0.4, twist: 10, plane: 0 },
};

/** A fin: swept back, flat, never folded. */
export const FIN_STYLE: WingStyle = {
  bind: [-35, -10],
  fold: { sweep: 0, droop: 0, lie: 0, joints: [], digits: 0, flex: 0 },
  stack: 0,
  thickness: 0.01,
};

/** Segment shares of a wing's arm (humerus, forearm, hand, …) by segment count. */
export const WING_SHARES: Readonly<Record<number, readonly number[]>> = {
  2: [0.45, 0.55],
  3: [0.32, 0.45, 0.23],
  4: [0.28, 0.38, 0.2, 0.14],
};

/** What a membrane module's `digits` hook gets: the wing's frame at the wrist. */
export interface DigitContext {
  /** The wrist (the end of the forearm) and the hand's tip (the end of the arm). */
  readonly wrist: Vector3;
  readonly tip: Vector3;
  /** The wing's plane: straight out (e1), toward the leading edge (e2), its normal. */
  readonly out: Vector3;
  readonly lead: Vector3;
  readonly normal: Vector3;
  /** The hand's direction, in the plane. */
  readonly hand: Vector3;
  /** The arm's whole length and its radius at the tip (metres). */
  readonly armLength: number;
  readonly tipRadius: number;
  readonly scale: number;
}

/** One digit: joints from its root, with a radius at each. */
export interface DigitChain {
  readonly points: readonly Vector3[];
  readonly radii: readonly number[];
  /** Where it roots: at the wrist (its own metacarpal), or continuing the hand from its tip. */
  readonly root: 'wrist' | 'tip';
}

/** The hooks a membrane module adds for its wing. */
export interface WingHooks {
  /** The wing's style; fields left out keep the bat's. */
  wing?(params: Record<string, unknown>): Partial<WingStyle>;
  /** Digit chains grown from the wrist (bat fingers, a bird's alula). */
  digits?(ctx: DigitContext, params: Record<string, unknown>): DigitChain[];
}

/** The style for a wing or fin limb, from its membrane module. */
export function wingStyleOf(limb: LimbSpec, registry: Registry): WingStyle {
  const base = limb.role === 'fin' ? FIN_STYLE : BAT_STYLE;
  const membrane = limb.membrane;
  const module = membrane
    ? (registry.get('part', membrane.type) as PartModule | undefined)
    : undefined;
  const hook = (module?.hooks as WingHooks | undefined)?.wing;
  if (!membrane || !hook) return base;
  const own = hook(membrane.params as Record<string, unknown>);
  return { ...base, ...own, fold: { ...base.fold, ...own.fold } };
}

/** Everything the fold needs to know about one wing, from the skeleton. */
export interface WingFrame {
  /** Index into the rig's wings. */
  readonly wing: number;
  readonly limb: string;
  readonly mirror: 1 | -1 | 0;
  /** Where along its section it attaches (0 front, 1 back on the torso). */
  readonly at: number;
  /** Where the humerus starts, and the root's radius. */
  readonly root: Vector3;
  readonly rootRadius: number;
  /** The wing's plane in the bind pose: out, toward the leading edge, and their normal. */
  readonly out: Vector3;
  readonly lead: Vector3;
  readonly normal: Vector3;
  /** Horizontal, toward the wing's own side (the body's left for a left wing). */
  readonly side: Vector3;
  /** Out of the skin at the root. */
  readonly rootNormal: Vector3;
  readonly style: WingStyle;
  readonly bones: readonly number[];
  readonly digits: readonly { readonly bones: readonly number[]; readonly root: 'wrist' | 'tip' }[];
  /**
   * Covered by a wing case in front of it on its side (a module that provides `cover`): it
   * folds on edge along the back, doubled back at its last joint, hanging into the body under
   * the case without lifting clear, hidden there as a beetle's hind wing is, and comes out as
   * it spreads.
   */
  readonly covered: boolean;
}

/** A capsule something folded must keep clear of. */
export interface Capsule {
  readonly a: Vector3;
  readonly b: Vector3;
  readonly radius: number;
}

const Y = new Vector3(0, 1, 0);
const Z = new Vector3(0, 0, 1);
const DEG = Math.PI / 180;

/** World rotation of a bone in its bind pose (+Y along it, +Z its `up`), as compile writes it. */
export function bindRotation(bone: BoneDef, out = new Quaternion()): Quaternion {
  const dir = new Vector3().subVectors(bone.tail, bone.head);
  if (bone.section === 'root' || dir.lengthSq() < 1e-14) return out.identity();
  dir.normalize();
  const z = bone.up.clone().addScaledVector(dir, -bone.up.dot(dir));
  if (z.lengthSq() < 1e-10) z.set(0, 0, 1).addScaledVector(dir, -dir.z);
  z.normalize();
  const x = new Vector3().crossVectors(dir, z).normalize();
  return out.setFromRotationMatrix(new Matrix4().makeBasis(x, dir, z));
}

const basis = (a: Vector3, b: Vector3, c: Vector3) =>
  new Quaternion().setFromRotationMatrix(new Matrix4().makeBasis(a, b, c));

function segmentDistance(p: Vector3, a: Vector3, b: Vector3): number {
  const ab = new Vector3().subVectors(b, a);
  const t = Math.max(0, Math.min(1, new Vector3().subVectors(p, a).dot(ab) / (ab.lengthSq() || 1)));
  return p.distanceTo(a.clone().addScaledVector(ab, t));
}

export interface FoldResult {
  /** Per wing (in `frames` order): folded local rotations over its bones, then its digits. */
  readonly locals: Quaternion[][];
  /** Per wing: folded world segments (joints and radii), for later wings and checks. */
  readonly capsules: Capsule[][];
  /** Wings that could not be folded clear of the body. */
  readonly blocked: string[];
  /** Each folded bone's world transform at rest (its head and rotation). */
  readonly rest: Map<number, { rotation: Quaternion; position: Vector3 }>;
}

/**
 * Folds every wing in joint space (docs/design/9.3-wings-fins.md): one turn at the shoulder,
 * in-plane turns at the elbow, wrist and digit joints, then lifted off the body, the arms, the
 * ground and the wings folded before it, in 4° steps. Pure: the same bones, the same fold.
 */
export function foldWings(
  bones: readonly BoneDef[],
  frames: readonly WingFrame[],
  field: (p: Vector3) => number,
  obstacles: readonly Capsule[],
  scale: number,
): FoldResult {
  const locals: Quaternion[][] = frames.map(() => []);
  const rest = new Map<number, { rotation: Quaternion; position: Vector3 }>();
  const capsules: Capsule[][] = frames.map(() => []);
  const blocked: string[] = [];
  const placed: Capsule[] = [];
  let flat = 0;
  const margin = 0.01 * scale;
  // Lower layers first: by layer, then wings further back (a hind wing lies under a forewing),
  // then of a pair the right before the left (which lies above it).
  const order = frames
    .map((f, i) => ({ f, i }))
    .sort(
      (a, b) =>
        a.f.style.stack - b.f.style.stack ||
        b.f.at - a.f.at ||
        a.f.mirror - b.f.mirror ||
        (a.f.limb < b.f.limb ? -1 : a.f.limb > b.f.limb ? 1 : 0),
    );
  for (const { f, i } of order) {
    const style = f.style;
    const axis = new Vector3().crossVectors(f.out, f.lead).normalize();
    const all = [...f.bones, ...f.digits.flatMap((d) => d.bones)];
    const bindWorld = new Map<number, Quaternion>(
      all.map((b) => [b, bindRotation(bones[b] as BoneDef)]),
    );
    const dirOf = (b: number) => {
      const bone = bones[b] as BoneDef;
      return new Vector3().subVectors(bone.tail, bone.head).normalize();
    };
    const angleOf = (d: Vector3) => Math.atan2(d.dot(f.lead), d.dot(f.out));
    // Folded in-plane angles: the arm turns at each joint, digits fan from the hand and flex.
    const folded = new Map<number, number>();
    let previous = angleOf(dirOf(f.bones[0] as number));
    // A covered wing goes straight, then doubles back at its last joint like a page turned
    // over (a half turn about the crease across the arm), so it fits under the case and its
    // membrane still hangs on the same side.
    const page = new Map<number, Quaternion>();
    f.bones.forEach((b, k) => {
      const last = f.covered && k > 0 && k === f.bones.length - 1;
      if (k > 0 && !last) previous += (style.fold.joints[k - 1] ?? 0) * DEG;
      folded.set(b, previous);
      if (last) {
        const along = f.out
          .clone()
          .multiplyScalar(Math.cos(previous))
          .addScaledVector(f.lead, Math.sin(previous));
        const crease = new Vector3().crossVectors(axis, along).normalize();
        page.set(b, new Quaternion().setFromAxisAngle(crease, 0.97 * Math.PI));
      }
    });
    const hand = folded.get(f.bones.at(-1) as number) as number;
    let fan = 0;
    for (const digit of f.digits) {
      let a = hand;
      digit.bones.forEach((b, j) => {
        // Later joints flex in a Z toward the leading side, away from the back.
        if (digit.root === 'wrist') {
          if (j === 0) a = hand - fan * style.fold.digits * DEG;
          else a += (j % 2 === 1 ? 1 : -1) * style.fold.flex * DEG;
        } else {
          a += (j % 2 === 0 ? 1 : -1) * style.fold.flex * DEG;
        }
        folded.set(b, a);
      });
      if (digit.root === 'wrist') fan++;
    }
    const swingOf = (b: number) => {
      const swing = new Quaternion().setFromAxisAngle(
        axis,
        (folded.get(b) as number) - angleOf(dirOf(b)),
      );
      const turned = page.get(b);
      return turned ? turned.clone().multiply(swing) : swing;
    };

    // The shoulder: the bind frame of the humerus onto the folded one.
    const h0 = dirOf(f.bones[0] as number);
    const lead0 = new Vector3().crossVectors(axis, h0).normalize();
    const S = f.side.clone();
    const U = Y.clone();
    // The folded wing lies in a plane: tangent to the body at its root (lie 0), turning toward
    // flat over the back (lie 90). The humerus points back and down within it; the leading edge
    // faces forward (so a forearm folds forward along the body), or outward lying flat.
    // A covered wing stands on edge along the back instead, its leading edge up and the rest
    // hanging down inside the body under the case.
    const lie = style.fold.lie * DEG;
    const plane = f.covered
      ? S.clone()
      : f.rootNormal
          .clone()
          .multiplyScalar(Math.cos(lie))
          .addScaledVector(lie < 0 ? S : U, Math.abs(Math.sin(lie)))
          .normalize();
    const sweep = f.covered ? 90 * DEG : style.fold.sweep * DEG;
    const droop = f.covered ? 8 * DEG : style.fold.droop * DEG;
    const hf = S.clone()
      .multiplyScalar(Math.cos(sweep))
      .addScaledVector(Z, -Math.sin(sweep))
      .multiplyScalar(Math.cos(droop))
      .addScaledVector(U, -Math.sin(droop));
    hf.addScaledVector(plane, -hf.dot(plane));
    if (hf.lengthSq() < 1e-8) hf.copy(Z).negate().addScaledVector(plane, plane.z);
    hf.normalize();
    // The leading edge faces down the flank (the forearm and hand fold below the humerus), or
    // outward when the wing lies over the back, or up when it is covered.
    const leadF = new Vector3().crossVectors(plane, hf).normalize();
    const facing = f.covered ? leadF.y : style.fold.lie < 45 ? -leadF.y : leadF.dot(S);
    if (facing < 0) leadF.negate();
    const shoulder = basis(hf, leadF, new Vector3().crossVectors(hf, leadF)).multiply(
      basis(h0, lead0, axis).invert(),
    );
    // Insects stack: each layer up rises a plate's thickness and a gap at the tip.
    const length = f.bones.reduce(
      (s, b) => s + (bones[b] as BoneDef).head.distanceTo((bones[b] as BoneDef).tail),
      0,
    );
    // Wings folded flat lie in layers: each one above every flat wing folded before it.
    const layered = style.fold.lie >= 45 && !style.shell && !f.covered;
    const level = layered ? flat : 0;
    if (layered) flat++;
    if (level > 0) {
      const rise = level * Math.atan(((style.thickness + 0.004) * scale) / Math.max(1e-6, length));
      const tilt = new Vector3().crossVectors(hf, U);
      if (tilt.lengthSq() > 1e-8)
        shoulder.premultiply(new Quaternion().setFromAxisAngle(tilt.normalize(), rise));
    }
    // Lifting off the body: the smallest turn about the shoulder, in 4° steps, that clears —
    // rolling out about the body's length, swinging out about the vertical, pitching up, or
    // rolling and swinging together.
    const m = f.mirror || 1;
    const roll = Z.clone().multiplyScalar(m);
    const yaw = Y.clone().multiplyScalar(-m);
    const pitch = new Vector3(1, 0, 0);
    const turn = (axis: Vector3, step: number, deg: number) =>
      new Quaternion().setFromAxisAngle(axis, step * deg * DEG);
    // A wing against the side rolls or swings out; one lying flat pitches its tip up.
    const lifts: ((step: number) => Quaternion)[] =
      style.fold.lie < 45
        ? [
            (k) => turn(roll, k, 4),
            (k) => turn(yaw, k, 4),
            (k) => turn(roll, k, 3).multiply(turn(yaw, k, 3)),
            (k) => turn(pitch, k, 4),
          ]
        : [(k) => turn(pitch, k, 3), (k) => turn(pitch, k, 3).multiply(turn(yaw, k, 2))];
    const pose = (lift: Quaternion) => {
      const turn = lift.clone().multiply(shoulder);
      const world = new Map<number, Quaternion>();
      for (const b of all)
        world.set(
          b,
          turn
            .clone()
            .multiply(swingOf(b))
            .multiply(bindWorld.get(b) as Quaternion),
        );
      // Joints, root first: the arm from the shoulder, digits from the wrist or the tip.
      const joints = new Map<number, [Vector3, Vector3]>();
      let at = f.root.clone();
      for (const b of f.bones) {
        const bone = bones[b] as BoneDef;
        const end = at
          .clone()
          .addScaledVector(
            Y.clone().applyQuaternion(world.get(b) as Quaternion),
            bone.head.distanceTo(bone.tail),
          );
        joints.set(b, [at, end]);
        at = end;
      }
      const wrist = (joints.get(f.bones.at(-2) ?? (f.bones[0] as number)) as [Vector3, Vector3])[1];
      const tip = (joints.get(f.bones.at(-1) as number) as [Vector3, Vector3])[1];
      for (const digit of f.digits) {
        let p = (digit.root === 'wrist' ? wrist : tip).clone();
        for (const b of digit.bones) {
          const bone = bones[b] as BoneDef;
          const end = p
            .clone()
            .addScaledVector(
              Y.clone().applyQuaternion(world.get(b) as Quaternion),
              bone.head.distanceTo(bone.tail),
            );
          joints.set(b, [p, end]);
          p = end;
        }
      }
      return { world, joints };
    };
    const clear = (joints: Map<number, [Vector3, Vector3]>) => {
      for (const [b, [a, e]] of joints) {
        const bone = bones[b] as BoneDef;
        for (const t of [0.5, 1]) {
          const p = a.clone().lerp(e, t);
          const r = t === 1 ? bone.r1 : (bone.r0 + bone.r1) / 2;
          // The shoulder sits in the pectoral mass by design.
          if (b === f.bones[0] && p.distanceTo(f.root) < 2.5 * f.rootRadius) continue;
          if (p.y < r || field(p) < r + margin) return false;
          for (const c of [...obstacles, ...placed])
            if (segmentDistance(p, c.a, c.b) < r + c.radius + 0.004 * scale) return false;
        }
      }
      return true;
    };
    let result = pose(new Quaternion());
    let ok = f.covered || style.shell === true || clear(result.joints);
    for (let step = 1; step <= 12 && !ok; step++) {
      for (const lift of lifts) {
        result = pose(lift(step));
        ok = clear(result.joints);
        if (ok) break;
      }
    }
    if (!ok) blocked.push(f.limb);
    // Local rotations: against the parent's folded world rotation, or its bind one.
    for (const b of all) {
      const parent = (bones[b] as BoneDef).parent;
      const parentWorld = result.world.get(parent) ?? bindRotation(bones[parent] as BoneDef);
      (locals[i] as Quaternion[]).push(
        parentWorld
          .clone()
          .invert()
          .multiply(result.world.get(b) as Quaternion),
      );
    }
    for (const b of all) {
      const [a, e] = result.joints.get(b) as [Vector3, Vector3];
      rest.set(b, { rotation: (result.world.get(b) as Quaternion).clone(), position: a.clone() });
      const bone = bones[b] as BoneDef;
      const c = { a, b: e, radius: Math.max(bone.r0, bone.r1) };
      (capsules[i] as Capsule[]).push(c);
      placed.push(c);
    }
  }
  return { locals, capsules, blocked, rest };
}
