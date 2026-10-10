import { Vector3 } from 'three';

const root = new Vector3();
const d = new Vector3();

/**
 * FABRIK (forward and backward reaching IK) on a chain of joints, in place: the root stays put,
 * every segment keeps its length, and the tip goes to `target`, or as near as the chain reaches
 * (straightened toward it). Suits long chains such as tentacles (docs/design/9.4-tentacles-parts.md).
 */
export function fabrik(
  points: readonly Vector3[],
  lengths: readonly number[],
  target: Vector3,
  iterations = 10,
  tolerance = 1e-4,
): void {
  const n = points.length;
  if (n < 2) return;
  root.copy(points[0] as Vector3);
  let total = 0;
  for (let i = 0; i < n - 1; i++) total += lengths[i] as number;
  if (root.distanceTo(target) >= total) {
    d.subVectors(target, root).normalize();
    for (let i = 1; i < n; i++)
      (points[i] as Vector3)
        .copy(points[i - 1] as Vector3)
        .addScaledVector(d, lengths[i - 1] as number);
    return;
  }
  for (let it = 0; it < iterations; it++) {
    (points[n - 1] as Vector3).copy(target);
    for (let i = n - 2; i >= 0; i--) {
      const p = points[i] as Vector3;
      const next = points[i + 1] as Vector3;
      d.subVectors(p, next);
      const l = d.length() || 1;
      p.copy(next).addScaledVector(d, (lengths[i] as number) / l);
    }
    (points[0] as Vector3).copy(root);
    for (let i = 1; i < n; i++) {
      const p = points[i] as Vector3;
      const prev = points[i - 1] as Vector3;
      d.subVectors(p, prev);
      const l = d.length() || 1;
      p.copy(prev).addScaledVector(d, (lengths[i - 1] as number) / l);
    }
    if ((points[n - 1] as Vector3).distanceToSquared(target) < tolerance * tolerance) break;
  }
}
