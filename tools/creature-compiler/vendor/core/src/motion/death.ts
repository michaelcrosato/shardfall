import { Quaternion, Vector3 } from 'three';
import type { CompiledCreature } from '../compile/compile.ts';
import type { Pose } from './pose.ts';

/**
 * Each bone's support hull (docs/design/10.5-hits-death.md, decision 6): the extreme points, in
 * the bone's own frame, of the skin, part, eye and membrane vertices it carries most, one for
 * each of 64 directions spread over the sphere, duplicates dropped. A body is never much lower
 * than its hulls.
 */
export interface BoneHulls {
  /** Bone of each point. */
  readonly bones: Int32Array;
  /** Points in their bone's bind frame: x, y, z each. */
  readonly points: Float32Array;
  /** First point of each bone, and one past its last (bones × 2). */
  readonly ranges: Int32Array;
}

const DIRECTIONS: readonly (readonly [number, number, number])[] = (() => {
  // 64 directions spread evenly over the sphere (a golden spiral).
  const out: [number, number, number][] = [];
  const n = 64;
  for (let i = 0; i < n; i++) {
    const y = 1 - (2 * (i + 0.5)) / n;
    const r = Math.sqrt(1 - y * y);
    const a = i * Math.PI * (3 - Math.sqrt(5));
    out.push([Math.cos(a) * r, y, Math.sin(a) * r]);
  }
  return out;
})();

export function boneHulls(compiled: CompiledCreature, pose: Pose): BoneHulls {
  const n = pose.count;
  const best = new Float64Array(n * DIRECTIONS.length).fill(-Infinity);
  const at = new Float64Array(n * DIRECTIONS.length * 3);
  const inverse = Array.from({ length: n }, (_, b) =>
    (pose.bindWorldRot[b] as Quaternion).clone().invert(),
  );
  const v = new Vector3();
  for (const mesh of [compiled.skin, compiled.parts, compiled.eyes, compiled.membranes]) {
    const count = mesh.positions.length / 3;
    for (let i = 0; i < count; i++) {
      // The bone that carries the vertex most.
      let bone = -1;
      let weight = 0;
      for (let k = 0; k < 4; k++) {
        const w = mesh.skinWeight[i * 4 + k] as number;
        if (w > weight) {
          weight = w;
          bone = mesh.skinIndex[i * 4 + k] as number;
        }
      }
      if (bone < 0 || bone >= n) continue;
      v.set(
        mesh.positions[i * 3] as number,
        mesh.positions[i * 3 + 1] as number,
        mesh.positions[i * 3 + 2] as number,
      )
        .sub(pose.bindWorldPos[bone] as Vector3)
        .applyQuaternion(inverse[bone] as Quaternion);
      for (let d = 0; d < DIRECTIONS.length; d++) {
        const [dx, dy, dz] = DIRECTIONS[d] as readonly [number, number, number];
        const s = v.x * dx + v.y * dy + v.z * dz;
        const slot = bone * DIRECTIONS.length + d;
        if (s > (best[slot] as number)) {
          best[slot] = s;
          at[slot * 3] = v.x;
          at[slot * 3 + 1] = v.y;
          at[slot * 3 + 2] = v.z;
        }
      }
    }
  }
  const bones: number[] = [];
  const points: number[] = [];
  const ranges = new Int32Array(n * 2);
  for (let b = 0; b < n; b++) {
    ranges[b * 2] = bones.length;
    const seen = new Set<string>();
    for (let d = 0; d < DIRECTIONS.length; d++) {
      const slot = b * DIRECTIONS.length + d;
      if (!Number.isFinite(best[slot] as number)) continue;
      const x = at[slot * 3] as number;
      const y = at[slot * 3 + 1] as number;
      const z = at[slot * 3 + 2] as number;
      const key = `${x.toFixed(5)},${y.toFixed(5)},${z.toFixed(5)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      bones.push(b);
      points.push(x, y, z);
    }
    ranges[b * 2 + 1] = bones.length;
  }
  return { bones: Int32Array.from(bones), points: Float32Array.from(points), ranges };
}

const scratchQ = new Quaternion();
const scratchV = new Vector3();
const scratchU = new Vector3();
const UP = new Vector3(0, 1, 0);

/**
 * How far a bone's hull reaches below the ground (m, positive into it) in the posed skeleton:
 * its lowest point, found in the bone's own frame, against the ground sampled right under it.
 */
export function hullDepth(
  hulls: BoneHulls,
  pose: Pose,
  bone: number,
  ground: (x: number, z: number) => number,
): number {
  const from = hulls.ranges[bone * 2] as number;
  const to = hulls.ranges[bone * 2 + 1] as number;
  if (from === to) return -Infinity;
  const rot = pose.worldRot[bone] as Quaternion;
  // World up in the bone's frame: a point's height is its dot with it.
  const up = scratchU.copy(UP).applyQuaternion(scratchQ.copy(rot).invert());
  let lowest = Infinity;
  let at = from;
  for (let i = from; i < to; i++) {
    const h =
      (hulls.points[i * 3] as number) * up.x +
      (hulls.points[i * 3 + 1] as number) * up.y +
      (hulls.points[i * 3 + 2] as number) * up.z;
    if (h < lowest) {
      lowest = h;
      at = i;
    }
  }
  const p = scratchV
    .set(
      hulls.points[at * 3] as number,
      hulls.points[at * 3 + 1] as number,
      hulls.points[at * 3 + 2] as number,
    )
    .applyQuaternion(rot)
    .add(pose.worldPos[bone] as Vector3);
  return ground(p.x, p.z) - p.y;
}

/**
 * Turns a bone by `angle` about `axis` given in world space, before its own rotation, so it and
 * everything it carries swing about its joint. Its parent must be solved.
 */
export function turnWorld(pose: Pose, bone: number, axis: Vector3, angle: number): void {
  if (angle === 0) return;
  const parent = pose.parents[bone] as number;
  const turn = scratchQ.setFromAxisAngle(axis, angle);
  const rot = pose.rot[bone] as Quaternion;
  if (parent < 0) {
    rot.premultiply(turn);
    return;
  }
  const p = pose.worldRot[parent] as Quaternion;
  // In the parent's frame: P⁻¹ · R · P.
  rot.premultiply(p.clone().invert().multiply(turn).multiply(p));
}
