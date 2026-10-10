import { Vector3 } from 'three';
import type { BoneDef, StationPanel } from './types.ts';

/**
 * Membranes (docs/design/9.3-wings-fins.md): sheets of skin between spars (an arm, a digit, the
 * body's side, a leg), carried by station bones so the sheet stays exactly on the surface ruled
 * between the spars whatever they do, and rigid sheets (insect wings, feather cards, fins).
 */

/** A polyline riding bones: segment k (points k to k+1) moves with `bones[k]`. */
export interface Spar {
  readonly points: readonly Vector3[];
  readonly bones: readonly number[];
}

/** How a membrane looks; colours are sRGB hex. */
export interface MembraneLook {
  readonly color: string;
  /** Colour toward the far end (s = 1); blends from `color`. */
  readonly tipColor?: string;
  /** 1 opaque; below 1 see-through (insect wings). */
  readonly opacity?: number;
  /** How much light shows through from behind, 0 to 1. */
  readonly translucency?: number;
  readonly roughness?: number;
  /** How strongly veins (or rays, or a feather's shaft) show, 0 to 1. */
  readonly veins?: number;
}

/** The membrane mesh as it is built, model space. */
export class MembraneSink {
  readonly positions: number[] = [];
  readonly normals: number[] = [];
  readonly indices: number[] = [];
  /** sRGB per vertex. */
  readonly color: number[] = [];
  /** Per vertex: opacity, translucency, roughness, vein strength. */
  readonly info: number[] = [];
  /** Per vertex: along (0 root to 1 tip) and across (0 to 1), where veins run. */
  readonly vein: number[] = [];
  readonly weights: [number, number][][] = [];
  readonly stations: StationPanel[] = [];
  /** Area per wing limb (m²), for flight later. */
  readonly area = new Map<string, number>();
  /** Every vertex of each wing's membrane in the bind pose, for its planform outline (10.4). */
  readonly outline = new Map<string, Vector3[]>();
}

/** Arc-length samples of a spar. */
function measure(spar: Spar): { total: number; at: (s: number) => { p: Vector3; bone: number } } {
  const lens = [0];
  for (let i = 1; i < spar.points.length; i++)
    lens.push(
      (lens[i - 1] as number) +
        (spar.points[i] as Vector3).distanceTo(spar.points[i - 1] as Vector3),
    );
  const total = lens.at(-1) || 1e-9;
  return {
    total,
    at(s: number) {
      const d = Math.min(1, Math.max(0, s)) * total;
      let i = 0;
      while (i < lens.length - 2 && (lens[i + 1] as number) < d) i++;
      const seg = (lens[i + 1] as number) - (lens[i] as number) || 1e-9;
      const f = Math.min(1, Math.max(0, (d - (lens[i] as number)) / seg));
      return {
        p: new Vector3().lerpVectors(spar.points[i] as Vector3, spar.points[i + 1] as Vector3, f),
        bone: spar.bones[Math.min(i, spar.bones.length - 1)] as number,
      };
    },
  };
}

/** Joint positions along a spar, as arc-length fractions (its inner points). */
function joints(spar: Spar, total: number): number[] {
  const out: number[] = [];
  let d = 0;
  for (let i = 1; i < spar.points.length - 1; i++) {
    d += (spar.points[i] as Vector3).distanceTo(spar.points[i - 1] as Vector3);
    out.push(d / total);
  }
  return out;
}

export interface PanelOptions {
  /** Prefix for station bone names, e.g. `wing.L.p2`. */
  readonly name: string;
  readonly owner: string;
  /** Metres per torso length. */
  readonly scale: number;
  /** Rows and columns of vertices (rows at least the stations'). */
  readonly rows: number;
  readonly cols: number;
  /** How far the free edge (s = 1) dips toward the root between the spars, 0 to 1. */
  readonly scallop?: number;
  /** Stations at most this far apart (metres) and no more than 12. */
  readonly spacing: number;
}

/**
 * A sheet between spars `a` and `b`, carried by station bones (appended to `bones`): at each
 * station a bone on each spar, aimed at the other (`applyStations` poses them the same way). Each
 * vertex is weighted to the two stations around it, both ends: exact on the ruled surface at
 * the stations, whatever the spars do. Rows run from the spars' starts (s = 0) to their ends.
 */
/**
 * A station's roll, shared by its pair (docs/design/9.3-wings-fins.md): the first takes the
 * spars' direction, and each after it the roll before, turned only as far as its aim demands, so
 * neighbouring stations agree and the sheet between them stays on the ruled surface even where
 * the spars fold back on themselves. `z` carries the previous roll in and the new one out.
 */
export function stationRoll(
  z: Vector3,
  y: Vector3,
  k: number,
  a0: Vector3,
  a1: Vector3,
  b0: Vector3,
  b1: Vector3,
): Vector3 {
  if (k === 0) z.subVectors(a1, a0).add(b1).sub(b0);
  z.addScaledVector(y, -z.dot(y));
  if (z.lengthSq() < 1e-14) {
    z.subVectors(a1, a0).add(b1).sub(b0).addScaledVector(y, -z.dot(y));
    if (z.lengthSq() < 1e-14) z.set(0, 1, 0).addScaledVector(y, -y.y);
    if (z.lengthSq() < 1e-14) z.set(1, 0, 0).addScaledVector(y, -y.x);
  }
  return z.normalize();
}

export function stationPanel(
  a: Spar,
  b: Spar,
  options: PanelOptions,
  bones: BoneDef[],
  sink: MembraneSink,
  look: MembraneLook,
  hexToRgb: (hex: string) => readonly [number, number, number],
): void {
  const A = measure(a);
  const B = measure(b);
  // Stations at the joints of both spars, filled in evenly, from just off the start (where two
  // spars from one joint coincide) to the end.
  const s0 = 0.03;
  const fill = Math.max(2, Math.ceil(Math.max(A.total, B.total) / options.spacing));
  const marks = new Set<number>([s0, 1]);
  for (const s of [...joints(a, A.total), ...joints(b, B.total)]) if (s > s0 + 0.02) marks.add(s);
  for (let k = 1; k < fill; k++) marks.add(s0 + ((1 - s0) * k) / fill);
  let stations = [...marks].sort((x, y) => x - y);
  stations = stations.filter((s, i) => i === 0 || s - (stations[i - 1] as number) > 0.025);
  while (stations.length > 10) {
    // Drop the station with the closest neighbours (never the ends).
    let worst = 1;
    let gap = Infinity;
    for (let i = 1; i < stations.length - 1; i++) {
      const g = (stations[i + 1] as number) - (stations[i - 1] as number);
      if (g < gap) {
        gap = g;
        worst = i;
      }
    }
    stations.splice(worst, 1);
  }
  const K = stations.length;
  const at = stations.map((s) => ({ a: A.at(s), b: B.at(s) }));
  const panel: { a: number[]; b: number[] } = { a: [], b: [] };
  const z = new Vector3();
  for (let k = 0; k < K; k++) {
    const here = at[k] as (typeof at)[number];
    const y = new Vector3().subVectors(here.b.p, here.a.p);
    if (y.lengthSq() < 1e-14) y.set(0, 0, 1);
    y.normalize();
    stationRoll(
      z,
      y,
      k,
      (at[0] as (typeof at)[number]).a.p,
      (at[Math.min(1, K - 1)] as (typeof at)[number]).a.p,
      (at[0] as (typeof at)[number]).b.p,
      (at[Math.min(1, K - 1)] as (typeof at)[number]).b.p,
    );
    const stub = 0.01 * options.scale;
    for (const [end, other, dir, list] of [
      [here.a, here.b, y, panel.a],
      [here.b, here.a, y.clone().negate(), panel.b],
    ] as const) {
      bones.push({
        name: `${options.name}.${k}.${list === panel.a ? 'a' : 'b'}`,
        parent: end.bone,
        section: 'station',
        owner: options.owner,
        head: end.p.clone(),
        tail: end.p.clone().addScaledVector(dir, stub),
        up: z.clone(),
        r0: stub,
        r1: stub,
        cross: [1, 1],
        t0: 0,
        t1: 1,
        skin: false,
        chain: -1,
      });
      void other;
      list.push(bones.length - 1);
    }
  }
  sink.stations.push(panel);

  // Vertices: columns across (f), rows along (s), the free edge dipping between the spars.
  const rows = Math.max(options.rows, K);
  const cols = Math.max(2, options.cols);
  const gap = A.at(1).p.distanceTo(B.at(1).p);
  const span = (A.total + B.total) / 2 || 1;
  const dipOf = (f: number) => (options.scallop ?? 0) * 4 * f * (1 - f) * Math.min(0.5, gap / span);
  const c0 = hexToRgb(look.color);
  const c1 = hexToRgb(look.tipColor ?? look.color);
  const base = sink.positions.length / 3;
  const grid: Vector3[][] = [];
  for (let j = 0; j < cols; j++) {
    const f = j / (cols - 1);
    const end = 1 - dipOf(f);
    const column: Vector3[] = [];
    for (let i = 0; i < rows; i++) {
      const s = s0 + ((end - s0) * i) / (rows - 1);
      const pa = A.at(s).p;
      const pb = B.at(s).p;
      column.push(new Vector3().lerpVectors(pa, pb, f));
      // Weights: the stations around s, both ends.
      let k = 0;
      while (k < K - 2 && (stations[k + 1] as number) < s) k++;
      const sk = stations[k] as number;
      const sk1 = stations[k + 1] as number;
      const u = Math.min(1, Math.max(0, (s - sk) / (sk1 - sk || 1)));
      const w: [number, number][] = [
        [panel.a[k] as number, (1 - u) * (1 - f)],
        [panel.b[k] as number, (1 - u) * f],
        [panel.a[k + 1] as number, u * (1 - f)],
        [panel.b[k + 1] as number, u * f],
      ];
      sink.weights.push(w.filter(([, x]) => x > 1e-6));
      const t = s;
      sink.color.push(
        c0[0] + (c1[0] - c0[0]) * t,
        c0[1] + (c1[1] - c0[1]) * t,
        c0[2] + (c1[2] - c0[2]) * t,
      );
      sink.info.push(
        look.opacity ?? 1,
        look.translucency ?? 0,
        look.roughness ?? 0.6,
        look.veins ?? 0,
      );
      sink.vein.push(s, f);
    }
    grid.push(column);
  }
  // Normals across the grid, and the area.
  let area = 0;
  for (let j = 0; j < cols; j++) {
    for (let i = 0; i < rows; i++) {
      const p = (grid[j] as Vector3[])[i] as Vector3;
      const di = new Vector3().subVectors(
        (grid[j] as Vector3[])[Math.min(rows - 1, i + 1)] as Vector3,
        (grid[j] as Vector3[])[Math.max(0, i - 1)] as Vector3,
      );
      const dj = new Vector3().subVectors(
        (grid[Math.min(cols - 1, j + 1)] as Vector3[])[i] as Vector3,
        (grid[Math.max(0, j - 1)] as Vector3[])[i] as Vector3,
      );
      const n = new Vector3().crossVectors(di, dj);
      if (n.lengthSq() < 1e-16) n.set(0, 1, 0);
      n.normalize();
      sink.positions.push(p.x, p.y, p.z);
      sink.normals.push(n.x, n.y, n.z);
    }
  }
  for (let j = 0; j + 1 < cols; j++) {
    for (let i = 0; i + 1 < rows; i++) {
      const v00 = base + j * rows + i;
      const v01 = base + j * rows + i + 1;
      const v10 = base + (j + 1) * rows + i;
      const v11 = base + (j + 1) * rows + i + 1;
      sink.indices.push(v00, v01, v11, v00, v11, v10);
      const p00 = (grid[j] as Vector3[])[i] as Vector3;
      const p01 = (grid[j] as Vector3[])[i + 1] as Vector3;
      const p10 = (grid[j + 1] as Vector3[])[i] as Vector3;
      const p11 = (grid[j + 1] as Vector3[])[i + 1] as Vector3;
      area +=
        new Vector3().subVectors(p01, p00).cross(new Vector3().subVectors(p11, p00)).length() / 2 +
        new Vector3().subVectors(p11, p00).cross(new Vector3().subVectors(p10, p00)).length() / 2;
    }
  }
  sink.area.set(options.owner, (sink.area.get(options.owner) ?? 0) + area);
  const outline = sink.outline.get(options.owner) ?? [];
  for (const row of grid) for (const p of row) outline.push(p.clone());
  sink.outline.set(options.owner, outline);
}

/**
 * A rigid sheet: positions and normals in model space, triangles, and weights per vertex (a
 * bone, or a blend along a chain). For insect wings, feather cards and fins.
 */
export function rigidSheet(
  positions: readonly Vector3[],
  normals: readonly Vector3[],
  indices: readonly number[],
  weights: readonly (readonly [number, number][])[],
  along: readonly number[],
  across: readonly number[],
  sink: MembraneSink,
  look: MembraneLook,
  hexToRgb: (hex: string) => readonly [number, number, number],
  owner?: string,
): void {
  const base = sink.positions.length / 3;
  const c0 = hexToRgb(look.color);
  const c1 = hexToRgb(look.tipColor ?? look.color);
  positions.forEach((p, v) => {
    const n = normals[v] as Vector3;
    sink.positions.push(p.x, p.y, p.z);
    sink.normals.push(n.x, n.y, n.z);
    const t = along[v] ?? 0;
    const k = t * t;
    sink.color.push(
      c0[0] + (c1[0] - c0[0]) * k,
      c0[1] + (c1[1] - c0[1]) * k,
      c0[2] + (c1[2] - c0[2]) * k,
    );
    sink.info.push(
      look.opacity ?? 1,
      look.translucency ?? 0,
      look.roughness ?? 0.6,
      look.veins ?? 0,
    );
    sink.vein.push(t, across[v] ?? 0);
    sink.weights.push([...(weights[v] ?? [])]);
  });
  let area = 0;
  for (let i = 0; i < indices.length; i += 3) {
    sink.indices.push(
      base + (indices[i] as number),
      base + (indices[i + 1] as number),
      base + (indices[i + 2] as number),
    );
    const p0 = positions[indices[i] as number] as Vector3;
    const p1 = positions[indices[i + 1] as number] as Vector3;
    const p2 = positions[indices[i + 2] as number] as Vector3;
    area += new Vector3().subVectors(p1, p0).cross(new Vector3().subVectors(p2, p0)).length() / 2;
  }
  if (owner) {
    sink.area.set(owner, (sink.area.get(owner) ?? 0) + area);
    const outline = sink.outline.get(owner) ?? [];
    for (const p of positions) outline.push(p.clone());
    sink.outline.set(owner, outline);
  }
}
