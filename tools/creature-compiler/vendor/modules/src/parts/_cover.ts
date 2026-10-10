import type { PartBuildContext, Socket } from '@spawnforge/core';
import { Vector3 } from 'three';

/**
 * A grid of the skin over an area part's band (docs/design/9.5-coverings.md): `rows + 1` points
 * along from `from` to `to` by `cols + 1` round from -angle to +angle, row by row.
 */
export interface SkinGrid {
  readonly rows: number;
  readonly cols: number;
  readonly sockets: readonly Socket[];
}

export function skinGrid(
  ctx: PartBuildContext,
  rows: number,
  cols: number,
  range: { readonly from: number; readonly to: number; readonly angle: number },
): SkinGrid {
  // Finding the skin is the costly part: sample every row along the body (where muscle swells
  // and narrows it) but a coarser grid round it, where it curves gently and a chord is well
  // under a millimetre off, and blend between; each lookup starts from its neighbour's depth.
  const cr = Math.min(rows, 16);
  const cc = Math.min(cols, 12);
  const coarse: Socket[] = [];
  for (let r = 0; r <= cr; r++) {
    const at = range.from + ((range.to - range.from) * r) / cr;
    for (let c = 0; c <= cc; c++)
      coarse.push(
        ctx.surface(
          at,
          range.angle * (2 * (c / cc) - 1),
          c > 0 ? coarse.at(-1) : r > 0 ? coarse[(r - 1) * (cc + 1)] : undefined,
        ),
      );
  }
  const sockets: Socket[] = [];
  for (let r = 0; r <= rows; r++)
    for (let c = 0; c <= cols; c++) {
      const x = (r / rows) * cr;
      const y = (c / cols) * cc;
      const r0 = Math.min(cr - 1, Math.floor(x));
      const c0 = Math.min(cc - 1, Math.floor(y));
      const fx = x - r0;
      const fy = y - c0;
      const corners: [Socket, number][] = [
        [coarse[r0 * (cc + 1) + c0] as Socket, (1 - fx) * (1 - fy)],
        [coarse[(r0 + 1) * (cc + 1) + c0] as Socket, fx * (1 - fy)],
        [coarse[r0 * (cc + 1) + c0 + 1] as Socket, (1 - fx) * fy],
        [coarse[(r0 + 1) * (cc + 1) + c0 + 1] as Socket, fx * fy],
      ];
      const mix = (key: 'position' | 'normal' | 'forward' | 'side') => {
        const v = new Vector3();
        for (const [s, w] of corners) v.addScaledVector(s[key], w);
        return v;
      };
      const bones = new Map<number, number>();
      for (const [s, w] of corners)
        for (const [b, k] of s.weights) bones.set(b, (bones.get(b) ?? 0) + k * w);
      const top = [...bones.entries()].sort((a, b) => b[1] - a[1] || a[0] - b[0]).slice(0, 4);
      const sum = top.reduce((a, [, k]) => a + k, 0) || 1;
      sockets.push({
        position: mix('position'),
        normal: mix('normal').normalize(),
        forward: mix('forward').normalize(),
        side: mix('side').normalize(),
        radius: corners.reduce((a, [s, w]) => a + s.radius * w, 0),
        weights: top.map(([b, k]) => [b, k / sum] as const),
      });
    }
  return { rows, cols, sockets };
}

/** A grid of points with a weight list each, row by row. */
export interface PointGrid {
  readonly rows: number;
  readonly cols: number;
  readonly points: Vector3[];
  readonly weights: (readonly (readonly [number, number])[])[];
}

/** Each grid point's surface normal, from its neighbours, facing away from `inside`. */
export function gridNormals(g: PointGrid, outward: (k: number) => Vector3): Vector3[] {
  const at = (r: number, c: number) =>
    g.points[
      Math.min(g.rows, Math.max(0, r)) * (g.cols + 1) + Math.min(g.cols, Math.max(0, c))
    ] as Vector3;
  return g.points.map((_, k) => {
    const r = Math.floor(k / (g.cols + 1));
    const c = k % (g.cols + 1);
    const du = new Vector3().subVectors(at(r + 1, c), at(r - 1, c));
    const dv = new Vector3().subVectors(at(r, c + 1), at(r, c - 1));
    const n = new Vector3().crossVectors(du, dv);
    if (n.lengthSq() < 1e-16) return outward(k).clone();
    n.normalize();
    return n.dot(outward(k)) < 0 ? n.negate() : n;
  });
}

/** A closed mesh between a grid's top and the same grid sunk `thickness` along its normals. */
export interface Slab {
  readonly positions: Vector3[];
  readonly normals: Vector3[];
  readonly indices: number[];
  readonly weights: [number, number][][];
}

/**
 * `down`, when given, is the way each point sinks to make the underside (the skin's normal for a
 * shell, whose top can be steep at its edge); the top's normals otherwise.
 */
export function slab(
  g: PointGrid,
  normals: readonly Vector3[],
  thickness: number,
  down?: readonly Vector3[],
): Slab {
  const sink = (k: number) => (down?.[k] ?? normals[k]) as Vector3;
  const out: Slab = { positions: [], normals: [], indices: [], weights: [] };
  const stride = g.cols + 1;
  const add = (p: Vector3, n: Vector3, w: (typeof g.weights)[number]) => {
    out.positions.push(p);
    out.normals.push(n);
    out.weights.push(w.map(([b, k]) => [b, k] as [number, number]));
    return out.positions.length - 1;
  };
  // Top and bottom faces, wound to face along the normals (a middle cell decides).
  const mr = Math.floor(g.rows / 2);
  const mc = Math.floor(g.cols / 2);
  const m = mr * stride + mc;
  const turn = new Vector3().crossVectors(
    new Vector3().subVectors(g.points[m + 1] as Vector3, g.points[m] as Vector3),
    new Vector3().subVectors(g.points[m + stride] as Vector3, g.points[m] as Vector3),
  );
  const flip = turn.dot(normals[m] as Vector3) < 0;
  for (const side of [1, -1]) {
    const base = out.positions.length;
    g.points.forEach((p, k) => {
      const n = normals[k] as Vector3;
      add(
        side > 0 ? p.clone() : p.clone().addScaledVector(sink(k), -thickness),
        n.clone().multiplyScalar(side),
        g.weights[k] ?? [],
      );
    });
    for (let r = 0; r < g.rows; r++)
      for (let c = 0; c < g.cols; c++) {
        const a = base + r * stride + c;
        const b = a + stride;
        if (side > 0 !== flip) out.indices.push(a, a + 1, b, a + 1, b + 1, b);
        else out.indices.push(a, b, a + 1, a + 1, b, b + 1);
      }
  }
  // The edge all round, as a strip of quads facing out.
  const ring: number[] = [];
  for (let c = 0; c < g.cols; c++) ring.push(c);
  for (let r = 0; r < g.rows; r++) ring.push(r * stride + g.cols);
  for (let c = g.cols; c > 0; c--) ring.push(g.rows * stride + c);
  for (let r = g.rows; r > 0; r--) ring.push(r * stride);
  const centre = new Vector3();
  for (const p of g.points) centre.add(p);
  centre.divideScalar(g.points.length);
  for (let i = 0; i < ring.length; i++) {
    const k0 = ring[i] as number;
    const k1 = ring[(i + 1) % ring.length] as number;
    const p0 = g.points[k0] as Vector3;
    const p1 = g.points[k1] as Vector3;
    const n0 = normals[k0] as Vector3;
    const q0 = p0.clone().addScaledVector(sink(k0), -thickness);
    const q1 = p1.clone().addScaledVector(sink(k1), -thickness);
    const along = new Vector3().subVectors(p1, p0);
    const face = new Vector3().crossVectors(along, n0).normalize();
    if (face.dot(new Vector3().subVectors(p0, centre)) < 0) face.negate();
    const a = add(p0, face, g.weights[k0] ?? []);
    const b = add(p1, face, g.weights[k1] ?? []);
    const c = add(q1, face, g.weights[k1] ?? []);
    const d = add(q0, face, g.weights[k0] ?? []);
    // Wind so the face points out.
    const n = new Vector3().crossVectors(
      new Vector3().subVectors(out.positions[b] as Vector3, out.positions[a] as Vector3),
      new Vector3().subVectors(out.positions[d] as Vector3, out.positions[a] as Vector3),
    );
    if (n.dot(face) >= 0) out.indices.push(a, b, d, b, c, d);
    else out.indices.push(a, d, b, b, d, c);
  }
  return out;
}

/** Bilinear lookup into a grid at (u, v) in 0..1, with the nearest point's weights. */
export function sampleGrid(g: PointGrid, normals: readonly Vector3[], u: number, v: number) {
  const x = Math.min(1, Math.max(0, u)) * g.rows;
  const y = Math.min(1, Math.max(0, v)) * g.cols;
  const r = Math.min(g.rows - 1, Math.floor(x));
  const c = Math.min(g.cols - 1, Math.floor(y));
  const fx = x - r;
  const fy = y - c;
  const k = (rr: number, cc: number) => rr * (g.cols + 1) + cc;
  const mix = (list: readonly Vector3[]) =>
    (list[k(r, c)] as Vector3)
      .clone()
      .multiplyScalar((1 - fx) * (1 - fy))
      .addScaledVector(list[k(r + 1, c)] as Vector3, fx * (1 - fy))
      .addScaledVector(list[k(r, c + 1)] as Vector3, (1 - fx) * fy)
      .addScaledVector(list[k(r + 1, c + 1)] as Vector3, fx * fy);
  return {
    point: mix(g.points),
    normal: mix(normals).normalize(),
    weights: g.weights[k(Math.round(x), Math.round(y))] ?? [],
  };
}
