import type { MeshPiece, PartBuildContext } from '@spawnforge/core';
import { Vector3 } from 'three';

/**
 * Weights for a point `t` of the way along a chain of bones: the bone it is on, shared with the
 * next across the last fifth before each joint, so a bend there stays smooth.
 */
export function alongChain(bones: readonly number[], t: number): [number, number][] {
  const n = bones.length;
  const x = Math.min(n - 1e-6, Math.max(0, t * n));
  const k = Math.floor(x);
  const f = x - k;
  const bone = bones[k] as number;
  const next = bones[k + 1];
  if (next !== undefined && f > 0.8) {
    const w = (f - 0.8) / 0.2 / 2;
    return [
      [bone, 1 - w],
      [next, w],
    ];
  }
  return [[bone, 1]];
}

/**
 * A piece built in model space into the hard parts, on a part's own bones: along `bones` by its
 * root-to-tip coordinate, or rigid on `bone` (docs/design/9.4-tentacles-parts.md).
 */
export function solidOn(
  ctx: PartBuildContext,
  piece: MeshPiece,
  look: { readonly color: string; readonly roughness: number },
  on: { readonly bones?: readonly number[]; readonly bone?: number },
): void {
  const positions: Vector3[] = [];
  const normals: Vector3[] = [];
  const weights: [number, number][][] = [];
  for (let v = 0; v < piece.positions.length / 3; v++) {
    positions.push(new Vector3().fromArray(piece.positions, v * 3));
    normals.push(new Vector3().fromArray(piece.normals, v * 3));
    weights.push(
      on.bone !== undefined ? [[on.bone, 1]] : alongChain(on.bones ?? [], piece.t[v] ?? 0),
    );
  }
  ctx.solid(positions, normals, piece.indices, weights, look);
}
