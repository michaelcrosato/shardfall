import { Matrix4, Quaternion, Vector3 } from 'three';
import { stationRoll } from '../compile/membranes.ts';
import type { StationPanel, WingRig } from '../compile/types.ts';
import type { Pose } from './pose.ts';

/**
 * Wings (docs/design/9.3-wings-fins.md) rest folded and spread toward their bind pose:
 * `spread` 0 is folded, 1 spread. Each wing, digit and feather bone turns from its rest
 * rotation toward its bind rotation; the caller solves the pose afterwards.
 */
export function applyWings(pose: Pose, wings: readonly WingRig[], spread: number): void {
  if (spread <= 0) return;
  const t = Math.min(1, spread);
  for (const wing of wings) {
    for (const bone of [...wing.bones, ...wing.digits.flat(), ...wing.feathers]) {
      (pose.rot[bone] as Quaternion)
        .copy(pose.restRot[bone] as Quaternion)
        .slerp(pose.bindRot[bone] as Quaternion, t);
    }
  }
}

const a = new Vector3();
const x = new Vector3();
const y = new Vector3();
const z = new Vector3();
const sx = new Vector3();
const sy = new Vector3();
const m = new Matrix4();
const world = new Quaternion();
const inverse = new Quaternion();

/**
 * Aims every membrane station at its partner across the panel, with a roll they share (the
 * spars' mean direction), so a sheet weighted between two stations on each side lies exactly on
 * the ruled surface between the spars, whatever they do. Station bones are leaves: they are
 * posed last, from their parents' current world transforms, and solved here.
 */
export function applyStations(pose: Pose, stations: readonly StationPanel[]): void {
  const worldPos = pose.worldPos;
  for (const panel of stations) {
    const n = panel.a.length;
    // Their heads follow their parents.
    for (let k = 0; k < n; k++) {
      pose.solveBone(panel.a[k] as number);
      pose.solveBone(panel.b[k] as number);
    }
    const second = n > 1 ? 1 : 0;
    for (let k = 0; k < n; k++) {
      const ia = panel.a[k] as number;
      const ib = panel.b[k] as number;
      a.copy(worldPos[ia] as Vector3);
      y.subVectors(worldPos[ib] as Vector3, a);
      if (y.lengthSq() < 1e-14) y.set(0, 0, 1);
      y.normalize();
      // The roll carried on from the station before, as compile set them up.
      stationRoll(
        z,
        y,
        k,
        worldPos[panel.a[0] as number] as Vector3,
        worldPos[panel.a[second] as number] as Vector3,
        worldPos[panel.b[0] as number] as Vector3,
        worldPos[panel.b[second] as number] as Vector3,
      );
      x.crossVectors(y, z).normalize();
      for (let side = 0; side < 2; side++) {
        const bone = side === 0 ? ia : ib;
        const sign = side === 0 ? 1 : -1;
        sx.copy(x).multiplyScalar(sign);
        sy.copy(y).multiplyScalar(sign);
        world.setFromRotationMatrix(m.makeBasis(sx, sy, z));
        const parent = pose.parents[bone] as number;
        inverse.copy(pose.worldRot[parent] as Quaternion).invert();
        (pose.rot[bone] as Quaternion).multiplyQuaternions(inverse, world);
        (pose.worldRot[bone] as Quaternion).copy(world);
      }
    }
  }
}
