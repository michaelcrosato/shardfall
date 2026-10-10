import { Quaternion, Vector3 } from 'three';
import type { LegRigData } from '../compile/compile.ts';
import type { Pose } from './pose.ts';

/**
 * A planted foot's roll (docs/design/8.2-feet.md): late in the stance the heel lifts while the
 * toes stay on the spots they landed on, bending at their first joint like the ball of a foot;
 * early in the swing they let go. Only legs with a stance roll, and only their toes that lie
 * along the ground (a hoof's or a pad's upright toe does not).
 */
export interface RollingToe {
  /** Toe bones, root to tip. */
  readonly bones: readonly number[];
  /** Tip relative to the rest ankle, in the rest frame (facing +Z). */
  readonly tip: Vector3;
  /** Which side of the root-to-tip line the toe's first joint sits on at rest (unit). */
  readonly bend: Vector3;
  /** Rest direction of each bone (unit), and of the first joint to the tip. */
  readonly restDirs: readonly Vector3[];
  readonly restReach: Vector3;
  /** Length of the first bone, and from the first joint to the tip (metres). */
  readonly first: number;
  readonly rest: number;
}

export interface FootRoll {
  readonly toes: readonly RollingToe[];
  /** How high the heel rises before the foot lifts (metres). */
  readonly lift: number;
  /** Where each rolling toe's tip is planted (world). */
  readonly tips: readonly Vector3[];
  /** The heel's current height above the planted ankle (metres). */
  heel: number;
}

/** Heel lift per stance: plantigrade feet roll the most. */
const ROLL_ANGLE = { plantigrade: 25, digitigrade: 18 } as const;

const boneDir = (pose: Pose, b: number) =>
  new Vector3(0, 1, 0).applyQuaternion(pose.restWorldRot[b] as Quaternion);
const restTail = (pose: Pose, b: number) =>
  (pose.restWorldPos[b] as Vector3).clone().addScaledVector(boneDir(pose, b), pose.lengths[b] ?? 0);

/** The roll of a leg's foot, or none when it keeps plan 1's flat feet. */
export function footRoll(rig: LegRigData, pose: Pose): FootRoll | undefined {
  if (rig.stance !== 'plantigrade' && rig.stance !== 'digitigrade') return undefined;
  const ankle = new Vector3(...rig.restFoot);
  const toes: RollingToe[] = [];
  let reach = 0;
  for (const bones of rig.toes) {
    if (bones.length < 2) continue;
    const b0 = bones[0] as number;
    const root = (pose.restWorldPos[b0] as Vector3).clone();
    const tip = restTail(pose, bones.at(-1) as number);
    const along = tip.clone().sub(root);
    // Toes that stand upright (a hoof, a column foot) do not roll.
    if (along.length() < 1e-6 || Math.abs(along.y) / along.length() > 0.8) continue;
    const knuckle = restTail(pose, b0);
    const axis = along.clone().normalize();
    const off = knuckle.clone().sub(root);
    const bend = off.addScaledVector(axis, -off.dot(axis));
    toes.push({
      bones,
      tip: tip.clone().sub(ankle),
      bend: bend.lengthSq() > 1e-12 ? bend.normalize() : new Vector3(0, 1, 0),
      restDirs: bones.map((b) => boneDir(pose, b)),
      restReach: tip.clone().sub(knuckle).normalize(),
      first: pose.lengths[b0] ?? 0,
      rest: tip.distanceTo(knuckle),
    });
    reach += along.length();
  }
  if (toes.length === 0) return undefined;
  const angle = (ROLL_ANGLE[rig.stance] * Math.PI) / 180;
  return {
    toes,
    lift: (reach / toes.length) * Math.sin(angle),
    tips: toes.map(() => new Vector3()),
    heel: 0,
  };
}

const smoothstep = (a: number, b: number, x: number) => {
  const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

/** Heel height at a point of the stance (`local` from 0 at landing to `duty` at lift-off). */
export function heelAt(roll: FootRoll, local: number, duty: number): number {
  return roll.lift * smoothstep(0.65 * duty, duty, Math.min(local, duty));
}

/**
 * Plants each rolling toe's tip where the foot just landed: the rest tip turned with the heading,
 * at the ground's height plus the tip's rest height.
 */
export function plantToes(
  roll: FootRoll,
  ankle: Vector3,
  footLift: number,
  heading: number,
  groundAt: (x: number, z: number) => number,
): void {
  roll.toes.forEach((toe, i) => {
    const tip = roll.tips[i] as Vector3;
    tip.copy(toe.tip).applyAxisAngle(UP, heading).add(ankle);
    // The tip's rest height above the ground (the rest ankle stands `footLift` up).
    tip.y = groundAt(tip.x, tip.z) + toe.tip.y + footLift;
  });
}

const UP = new Vector3(0, 1, 0);
const scratchA = new Vector3();
const scratchB = new Vector3();
const scratchC = new Vector3();
const scratchQ = new Quaternion();
const turn = new Quaternion();

/**
 * Poses a rolling foot's toes: each one's first two bones reach for its planted tip from where
 * its root is now, bending at the first joint the way it bends at rest; the bones beyond turn
 * with the second. `weight` blends from the rest pose (0) to the planted toes (1).
 */
export function poseToes(roll: FootRoll, pose: Pose, heading: number, weight: number): void {
  turn.setFromAxisAngle(UP, heading);
  roll.toes.forEach((toe, i) => {
    const b0 = toe.bones[0] as number;
    pose.solveBone(b0);
    const root = pose.worldPos[b0] as Vector3;
    const target = roll.tips[i] as Vector3;
    const axis = scratchA.subVectors(target, root);
    const reach = toe.first + toe.rest;
    const d = Math.min(
      Math.max(axis.length(), Math.abs(toe.first - toe.rest) + 1e-6),
      reach - 1e-6,
    );
    axis.normalize();
    // The first joint sits off the line toward the side it bends to at rest.
    const pole = scratchB.copy(toe.bend).applyQuaternion(turn);
    pole.addScaledVector(axis, -pole.dot(axis));
    if (pole.lengthSq() < 1e-12) pole.copy(UP);
    pole.normalize();
    const a = (toe.first * toe.first - toe.rest * toe.rest + d * d) / (2 * d);
    const h = Math.sqrt(Math.max(0, toe.first * toe.first - a * a));
    const firstDir = scratchC.copy(axis).multiplyScalar(a).addScaledVector(pole, h).normalize();
    aimBlend(pose, b0, toe.restDirs[0] as Vector3, firstDir, weight);
    // Beyond the first joint the toe keeps its rest shape, turned to reach the tip.
    const knuckle = scratchB.copy(root).addScaledVector(firstDir, toe.first);
    const reachDir = scratchA.subVectors(target, knuckle).normalize();
    const restReach = scratchC.copy(toe.restReach).applyQuaternion(turn);
    scratchQ.setFromUnitVectors(restReach, reachDir);
    for (let k = 1; k < toe.bones.length; k++) {
      const b = toe.bones[k] as number;
      pose.solveBone(b);
      const dir = scratchC.copy(toe.restDirs[k] as Vector3).applyQuaternion(turn);
      const reached = dir.clone().applyQuaternion(scratchQ);
      aimBlend(pose, b, toe.restDirs[k] as Vector3, reached, weight);
    }
  });
}

/** Aims a bone between its rest direction (turned with the body) and `dir`. */
function aimBlend(pose: Pose, b: number, restDir: Vector3, dir: Vector3, weight: number): void {
  pose.solveBone(b);
  const rest = restDir.clone().applyQuaternion(turn);
  pose.aim(b, rest.lerp(dir, weight).normalize());
}
