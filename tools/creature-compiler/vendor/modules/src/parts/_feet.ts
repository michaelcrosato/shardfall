import type { MeshPiece, ToeContext } from '@spawnforge/core';
import { Vector3 } from 'three';

/** Shared by the foot and hand modules (not a module: the pack index skips `_` files). */

export const Y = new Vector3(0, 1, 0);

/** The way a leg's toes point: forward, turned outward on sprawlers. */
export function toeCentre(ctx: ToeContext): Vector3 {
  return ctx.forward
    .clone()
    .addScaledVector(ctx.outward, Math.sin((ctx.splay * Math.PI) / 180) * 0.7)
    .normalize();
}

/** `dir` turned about the vertical by `degrees`. */
export function turned(dir: Vector3, degrees: number): Vector3 {
  return dir.clone().applyAxisAngle(Y, (degrees * Math.PI) / 180);
}

/** A point `distance` along the horizontal `dir` from the ankle, at `height` above the ground. */
export function onGround(ctx: ToeContext, dir: Vector3, distance: number, height: number): Vector3 {
  const p = ctx.ankle.clone().addScaledVector(dir, distance);
  p.y = ctx.groundY + height;
  return p;
}

/** Scales a piece along its own axes. */
export function scaled(piece: MeshPiece, x: number, y: number, z: number): MeshPiece {
  const positions = piece.positions.slice();
  const normals = piece.normals.slice();
  for (let i = 0; i < positions.length; i += 3) {
    positions[i] = (positions[i] as number) * x;
    positions[i + 1] = (positions[i + 1] as number) * y;
    positions[i + 2] = (positions[i + 2] as number) * z;
    // Normals scale inversely, then renormalise.
    const nx = (normals[i] as number) / x;
    const ny = (normals[i + 1] as number) / y;
    const nz = (normals[i + 2] as number) / z;
    const len = Math.hypot(nx, ny, nz) || 1;
    normals[i] = nx / len;
    normals[i + 1] = ny / len;
    normals[i + 2] = nz / len;
  }
  return { ...piece, positions, normals };
}

/** Slides each vertex along Z by `k` times its height (a hoof's sloped front wall). */
export function sheared(piece: MeshPiece, k: number): MeshPiece {
  const positions = piece.positions.slice();
  const normals = piece.normals.slice();
  for (let i = 0; i < positions.length; i += 3) {
    positions[i + 2] = (positions[i + 2] as number) + k * (positions[i + 1] as number);
    // z' = z + k y, so normals transform by the inverse transpose: n_y' = n_y - k n_z.
    const ny = (normals[i + 1] as number) - k * (normals[i + 2] as number);
    const len = Math.hypot(normals[i] as number, ny, normals[i + 2] as number) || 1;
    normals[i] = (normals[i] as number) / len;
    normals[i + 1] = ny / len;
    normals[i + 2] = (normals[i + 2] as number) / len;
  }
  return { ...piece, positions, normals };
}
