import type { Spar } from '@spawnforge/core';
import { Vector3 } from 'three';

/**
 * A grid sheet for membrane modules (docs/design/9.3-wings-fins.md): `at(s, f)` places the
 * vertex at s along (0 root to 1 tip) and f across (0 to 1); `weights(s, f)` binds it. Rows
 * along, columns across; normals from the grid.
 */
export function gridSheet(
  rows: number,
  cols: number,
  at: (s: number, f: number) => Vector3,
  weights: (s: number, f: number) => [number, number][],
) {
  const positions: Vector3[] = [];
  const along: number[] = [];
  const across: number[] = [];
  const w: [number, number][][] = [];
  for (let j = 0; j < cols; j++) {
    for (let i = 0; i < rows; i++) {
      const s = i / (rows - 1);
      const f = j / (cols - 1);
      positions.push(at(s, f));
      along.push(s);
      across.push(f);
      w.push(weights(s, f));
    }
  }
  const p = (i: number, j: number) => positions[j * rows + i] as Vector3;
  const normals = positions.map((_, v) => {
    const i = v % rows;
    const j = Math.floor(v / rows);
    const di = new Vector3().subVectors(p(Math.min(rows - 1, i + 1), j), p(Math.max(0, i - 1), j));
    const dj = new Vector3().subVectors(p(i, Math.min(cols - 1, j + 1)), p(i, Math.max(0, j - 1)));
    const n = new Vector3().crossVectors(di, dj);
    return n.lengthSq() > 1e-16 ? n.normalize() : new Vector3(0, 1, 0);
  });
  const indices: number[] = [];
  for (let j = 0; j + 1 < cols; j++)
    for (let i = 0; i + 1 < rows; i++) {
      const a = j * rows + i;
      const b = j * rows + i + 1;
      const c = (j + 1) * rows + i + 1;
      const d = (j + 1) * rows + i;
      indices.push(a, b, c, a, c, d);
    }
  return { positions, normals, indices, weights: w, along, across };
}

/** A point `s` of the way along a spar (by length), and the bone it rides, blended at joints. */
export function alongSpar(spar: Spar, s: number): { point: Vector3; weights: [number, number][] } {
  const lens = [0];
  for (let i = 1; i < spar.points.length; i++)
    lens.push(
      (lens[i - 1] as number) +
        (spar.points[i] as Vector3).distanceTo(spar.points[i - 1] as Vector3),
    );
  const total = lens.at(-1) || 1e-9;
  const d = Math.min(1, Math.max(0, s)) * total;
  let i = 0;
  while (i < lens.length - 2 && (lens[i + 1] as number) < d) i++;
  const seg = (lens[i + 1] as number) - (lens[i] as number) || 1e-9;
  const f = Math.min(1, Math.max(0, (d - (lens[i] as number)) / seg));
  const point = new Vector3().lerpVectors(
    spar.points[i] as Vector3,
    spar.points[i + 1] as Vector3,
    f,
  );
  const bone = spar.bones[Math.min(i, spar.bones.length - 1)] as number;
  // Near a joint, share with the neighbour across it, so a bend there stays smooth.
  const blend = 0.2;
  const next = spar.bones[i + 1];
  const prev = spar.bones[i - 1];
  if (f > 1 - blend && next !== undefined) {
    const t = (f - (1 - blend)) / blend / 2;
    return {
      point,
      weights: [
        [bone, 1 - t],
        [next, t],
      ],
    };
  }
  if (f < blend && prev !== undefined) {
    const t = (blend - f) / blend / 2;
    return {
      point,
      weights: [
        [bone, 1 - t],
        [prev, t],
      ],
    };
  }
  return { point, weights: [[bone, 1]] };
}
