import { Vector3 } from 'three';

/**
 * The geometry kit part modules build from. Pieces are built in a local frame where +Y points
 * out of the skin, +Z is the socket's forward direction and +X its side; the compiler places
 * them on the creature. Every vertex carries `t`, its position along the piece from root (0) to
 * tip (1), which materials use for gradients such as darker horn tips.
 */
export interface MeshPiece {
  positions: number[];
  normals: number[];
  indices: number[];
  /** Root-to-tip coordinate per vertex. */
  t: number[];
}

export function emptyPiece(): MeshPiece {
  return { positions: [], normals: [], indices: [], t: [] };
}

/** Appends `b` to `a` and returns `a`. */
export function merge(a: MeshPiece, b: MeshPiece): MeshPiece {
  const base = a.positions.length / 3;
  a.positions.push(...b.positions);
  a.normals.push(...b.normals);
  a.t.push(...b.t);
  for (const i of b.indices) a.indices.push(i + base);
  return a;
}

export interface TubeOptions {
  /** Sides around the tube (default 10). */
  readonly sides?: number;
  /** How the tip ends: a point, a flat cap, or a hemisphere. */
  readonly tip?: 'point' | 'flat' | 'round';
  /** Close the root with a flat cap (default false: roots sink into the skin). */
  readonly capRoot?: boolean;
  /** Rings of extra thickness along the tube. */
  readonly ridges?: number;
  /** Depth of each ridge as a share of the radius (default 0.12). */
  readonly ridgeDepth?: number;
  /** Cross-section scale [side, up] relative to the path frame. */
  readonly cross?: readonly [number, number];
  /**
   * Where the path frame's first axis points (projected off the first tangent); `cross[0]`
   * scales along it. Without it the frame starts anywhere, which only round tubes can afford.
   */
  readonly up?: Vector3;
}

/**
 * Sweeps a circle along a polyline with parallel-transport frames. `radius(t)` gives the radius
 * at each point along the path (t from 0 to 1 by arc length).
 */
export function sweep(
  path: readonly Vector3[],
  radius: (t: number) => number,
  options: TubeOptions = {},
): MeshPiece {
  const sides = options.sides ?? 10;
  const n = path.length;
  const out = emptyPiece();
  if (n < 2) return out;
  const lengths = [0];
  for (let i = 1; i < n; i++)
    lengths.push(
      (lengths[i - 1] as number) + (path[i] as Vector3).distanceTo(path[i - 1] as Vector3),
    );
  const total = lengths[n - 1] || 1;
  const tangents = path.map((_, i) => {
    const a = path[Math.max(0, i - 1)] as Vector3;
    const b = path[Math.min(n - 1, i + 1)] as Vector3;
    return new Vector3().subVectors(b, a).normalize();
  });
  // Initial normal: `up` if given, else any vector not parallel to the first tangent.
  const t0 = tangents[0] as Vector3;
  let normal =
    options.up && Math.abs(options.up.clone().normalize().dot(t0)) < 0.99
      ? options.up.clone()
      : Math.abs(t0.z) < 0.9
        ? new Vector3(0, 0, 1)
        : new Vector3(1, 0, 0);
  normal.addScaledVector(t0, -normal.dot(t0)).normalize();
  const ridges = options.ridges ?? 0;
  const ridgeDepth = options.ridgeDepth ?? 0.12;
  const [cx, cy] = options.cross ?? [1, 1];
  const ringStart: number[] = [];
  for (let i = 0; i < n; i++) {
    const tangent = tangents[i] as Vector3;
    if (i > 0) {
      // Parallel transport: remove the component along the new tangent.
      normal.addScaledVector(tangent, -normal.dot(tangent)).normalize();
    }
    const binormal = new Vector3().crossVectors(tangent, normal).normalize();
    const u = (lengths[i] as number) / total;
    let r = radius(u);
    if (ridges > 0) r *= 1 + ridgeDepth * Math.max(0, Math.cos(u * ridges * Math.PI * 2)) ** 3;
    ringStart.push(out.positions.length / 3);
    for (let s = 0; s < sides; s++) {
      const a = (s / sides) * Math.PI * 2;
      const nx = Math.cos(a) * cx;
      const ny = Math.sin(a) * cy;
      const dir = normal.clone().multiplyScalar(nx).addScaledVector(binormal, ny);
      const p = (path[i] as Vector3).clone().addScaledVector(dir, r);
      const nrm = normal
        .clone()
        .multiplyScalar(Math.cos(a) / cx)
        .addScaledVector(binormal, Math.sin(a) / cy)
        .normalize();
      out.positions.push(p.x, p.y, p.z);
      out.normals.push(nrm.x, nrm.y, nrm.z);
      out.t.push(u);
    }
    normal = normal.clone();
  }
  for (let i = 0; i + 1 < n; i++) {
    const a0 = ringStart[i] as number;
    const b0 = ringStart[i + 1] as number;
    for (let s = 0; s < sides; s++) {
      const s1 = (s + 1) % sides;
      out.indices.push(a0 + s, a0 + s1, b0 + s1, a0 + s, b0 + s1, b0 + s);
    }
  }
  const end = path[n - 1] as Vector3;
  const endT = tangents[n - 1] as Vector3;
  const last = ringStart[n - 1] as number;
  const tip = options.tip ?? 'point';
  if (tip === 'point' || tip === 'flat') {
    const p = tip === 'point' ? end.clone().addScaledVector(endT, radius(1) * 1.5 + 1e-6) : end;
    const c = out.positions.length / 3;
    out.positions.push(p.x, p.y, p.z);
    out.normals.push(endT.x, endT.y, endT.z);
    out.t.push(1);
    for (let s = 0; s < sides; s++) out.indices.push(last + s, last + ((s + 1) % sides), c);
  } else {
    // A hemisphere of three rings.
    const r = radius(1);
    let prev = last;
    const nrm0 = normal;
    const bin0 = new Vector3().crossVectors(endT, nrm0).normalize();
    for (let ring = 1; ring <= 3; ring++) {
      const phi = (ring / 4) * (Math.PI / 2);
      const start = out.positions.length / 3;
      for (let s = 0; s < sides; s++) {
        const a = (s / sides) * Math.PI * 2;
        const dir = nrm0
          .clone()
          .multiplyScalar(Math.cos(a) * Math.cos(phi) * cx)
          .addScaledVector(bin0, Math.sin(a) * Math.cos(phi) * cy)
          .addScaledVector(endT, Math.sin(phi));
        const p = end.clone().addScaledVector(dir, r);
        const nd = dir.clone().normalize();
        out.positions.push(p.x, p.y, p.z);
        out.normals.push(nd.x, nd.y, nd.z);
        out.t.push(1);
      }
      for (let s = 0; s < sides; s++) {
        const s1 = (s + 1) % sides;
        out.indices.push(prev + s, prev + s1, start + s1, prev + s, start + s1, start + s);
      }
      prev = start;
    }
    const c = out.positions.length / 3;
    const p = end.clone().addScaledVector(endT, r);
    out.positions.push(p.x, p.y, p.z);
    out.normals.push(endT.x, endT.y, endT.z);
    out.t.push(1);
    for (let s = 0; s < sides; s++) out.indices.push(prev + s, prev + ((s + 1) % sides), c);
  }
  if (options.capRoot) {
    const start = path[0] as Vector3;
    const c = out.positions.length / 3;
    out.positions.push(start.x, start.y, start.z);
    out.normals.push(-t0.x, -t0.y, -t0.z);
    out.t.push(0);
    for (let s = 0; s < sides; s++) out.indices.push(c, (s + 1) % sides, s);
  }
  return out;
}

export interface ArcOptions {
  /** Points along the arc (default 12). */
  readonly segments?: number;
  /** Total twist in degrees: the bend plane turns around the arc, making a spiral. */
  readonly twist?: number;
  /** Degrees the start leans toward +Z (forward) from straight up. */
  readonly lean?: number;
  /** Degrees the bend plane is turned around +Y; 0 bends toward -Z (back). */
  readonly heading?: number;
}

/**
 * A path from the origin that starts along +Y and bends by `curve` degrees in total, toward -Z
 * (back) for positive values. With `twist` the bend plane turns as it goes, giving ram-horn
 * spirals.
 */
export function arc(length: number, curve: number, options: ArcOptions = {}): Vector3[] {
  const n = Math.max(2, options.segments ?? 12);
  const step = length / n;
  const dir = new Vector3(0, 1, 0).applyAxisAngle(
    new Vector3(1, 0, 0),
    ((options.lean ?? 0) * Math.PI) / 180,
  );
  // Bending toward -Z means rotating about -X.
  let axis = new Vector3(-1, 0, 0).applyAxisAngle(
    new Vector3(0, 1, 0),
    ((options.heading ?? 0) * Math.PI) / 180,
  );
  axis.addScaledVector(dir, -axis.dot(dir)).normalize();
  const bendStep = (curve * Math.PI) / 180 / n;
  const twistStep = ((options.twist ?? 0) * Math.PI) / 180 / n;
  const points = [new Vector3()];
  let p = new Vector3();
  for (let i = 0; i < n; i++) {
    dir.applyAxisAngle(axis, bendStep).normalize();
    axis.applyAxisAngle(dir, twistStep);
    axis.addScaledVector(dir, -axis.dot(dir)).normalize();
    p = p.clone().addScaledVector(dir, step);
    points.push(p);
  }
  axis = axis.clone();
  return points;
}

/**
 * A surface of revolution around +Y from a profile of [radius, height] pairs, bottom to top.
 * Zero radii at the ends close the shape.
 */
export function lathe(profile: readonly (readonly [number, number])[], sides = 16): MeshPiece {
  const out = emptyPiece();
  const n = profile.length;
  const minY = Math.min(...profile.map((p) => p[1]));
  const maxY = Math.max(...profile.map((p) => p[1]));
  for (let i = 0; i < n; i++) {
    const [r, y] = profile[i] as readonly [number, number];
    const prev = profile[Math.max(0, i - 1)] as readonly [number, number];
    const next = profile[Math.min(n - 1, i + 1)] as readonly [number, number];
    // Profile normal: perpendicular to the local slope.
    const dr = next[0] - prev[0];
    const dy = next[1] - prev[1];
    const len = Math.hypot(dr, dy) || 1;
    const pr = dy / len;
    const py = -dr / len;
    for (let s = 0; s < sides; s++) {
      const a = (s / sides) * Math.PI * 2;
      out.positions.push(Math.cos(a) * r, y, Math.sin(a) * r);
      out.normals.push(Math.cos(a) * pr, py, Math.sin(a) * pr);
      out.t.push((y - minY) / (maxY - minY || 1));
    }
  }
  for (let i = 0; i + 1 < n; i++) {
    for (let s = 0; s < sides; s++) {
      const a = i * sides + s;
      const b = i * sides + ((s + 1) % sides);
      const c = (i + 1) * sides + s;
      const d = (i + 1) * sides + ((s + 1) % sides);
      out.indices.push(a, c, d, a, d, b);
    }
  }
  return out;
}

/** A sphere of radius r as a lathe (latitude rings × sides). */
export function sphere(r: number, rings = 8, sides = 14): MeshPiece {
  const profile: [number, number][] = [];
  for (let i = 0; i <= rings; i++) {
    const phi = -Math.PI / 2 + (i / rings) * Math.PI;
    profile.push([Math.cos(phi) * r, Math.sin(phi) * r]);
  }
  const piece = lathe(profile, sides);
  // Exact sphere normals.
  for (let v = 0; v < piece.positions.length; v += 3) {
    const n = new Vector3(
      piece.positions[v],
      piece.positions[v + 1],
      piece.positions[v + 2],
    ).normalize();
    piece.normals[v] = n.x;
    piece.normals[v + 1] = n.y;
    piece.normals[v + 2] = n.z;
  }
  return piece;
}

/**
 * A flat plate from a 2D outline in the XY plane (counter-clockwise), extruded `thickness`
 * along Z, for fins, frills and armour plates.
 */
export function plate(
  outline: readonly (readonly [number, number])[],
  thickness: number,
): MeshPiece {
  const out = emptyPiece();
  const n = outline.length;
  const h = thickness / 2;
  const cx = outline.reduce((a, p) => a + p[0], 0) / n;
  const cy = outline.reduce((a, p) => a + p[1], 0) / n;
  const maxR = Math.max(...outline.map((p) => Math.hypot(p[0] - cx, p[1] - cy))) || 1;
  for (const z of [h, -h]) {
    const c = out.positions.length / 3;
    out.positions.push(cx, cy, z);
    out.normals.push(0, 0, Math.sign(z));
    out.t.push(0);
    for (const [x, y] of outline) {
      out.positions.push(x, y, z);
      out.normals.push(0, 0, Math.sign(z));
      out.t.push(Math.hypot(x - cx, y - cy) / maxR);
    }
    for (let i = 0; i < n; i++) {
      const a = c + 1 + i;
      const b = c + 1 + ((i + 1) % n);
      if (z > 0) out.indices.push(c, a, b);
      else out.indices.push(c, b, a);
    }
  }
  // Side walls.
  for (let i = 0; i < n; i++) {
    const [x0, y0] = outline[i] as readonly [number, number];
    const [x1, y1] = outline[(i + 1) % n] as readonly [number, number];
    const nx = y1 - y0;
    const ny = -(x1 - x0);
    const len = Math.hypot(nx, ny) || 1;
    const s = out.positions.length / 3;
    for (const [x, y, z] of [
      [x0, y0, h],
      [x1, y1, h],
      [x1, y1, -h],
      [x0, y0, -h],
    ] as const) {
      out.positions.push(x, y, z);
      out.normals.push(nx / len, ny / len, 0);
      out.t.push(Math.hypot(x - cx, y - cy) / maxR);
    }
    out.indices.push(s, s + 3, s + 2, s, s + 2, s + 1);
  }
  return out;
}

// --- Deformers ----------------------------------------------------------------------------------

function mapPoints(piece: MeshPiece, fn: (p: Vector3, t: number) => void): MeshPiece {
  const p = new Vector3();
  for (let v = 0; v < piece.positions.length; v += 3) {
    p.set(
      piece.positions[v] as number,
      piece.positions[v + 1] as number,
      piece.positions[v + 2] as number,
    );
    fn(p, piece.t[v / 3] as number);
    piece.positions[v] = p.x;
    piece.positions[v + 1] = p.y;
    piece.positions[v + 2] = p.z;
  }
  return piece;
}

/**
 * Bends a piece toward -Z: a point at height y on the axis ends up on a circle that turns by
 * `degrees` over `height`. Offsets in z bend with it, so thickness is kept.
 */
export function bend(piece: MeshPiece, degrees: number, height: number): MeshPiece {
  const k = (degrees * Math.PI) / 180 / (height || 1);
  if (Math.abs(k) < 1e-9) return piece;
  const R = 1 / k;
  return mapPoints(piece, (p) => {
    const theta = p.y * k;
    const rz = R + p.z;
    p.y = rz * Math.sin(theta);
    p.z = rz * Math.cos(theta) - R;
  });
}

/** Twists a piece around +Y by `degrees` per unit of t. */
export function twist(piece: MeshPiece, degrees: number): MeshPiece {
  const y = new Vector3(0, 1, 0);
  return mapPoints(piece, (p, t) => {
    p.applyAxisAngle(y, (degrees * Math.PI * t) / 180);
  });
}

/** Scales the cross-section by `1 - amount * t`. */
export function taper(piece: MeshPiece, amount: number): MeshPiece {
  return mapPoints(piece, (p, t) => {
    const s = 1 - amount * t;
    p.x *= s;
    p.z *= s;
  });
}

/** Pushes vertices along their normals by `amount × noise(position)`. */
export function displace(
  piece: MeshPiece,
  amount: number,
  noise: (x: number, y: number, z: number) => number,
): MeshPiece {
  for (let v = 0; v < piece.positions.length; v += 3) {
    const d =
      amount *
      noise(
        piece.positions[v] as number,
        piece.positions[v + 1] as number,
        piece.positions[v + 2] as number,
      );
    for (let a = 0; a < 3; a++)
      piece.positions[v + a] =
        (piece.positions[v + a] as number) + (piece.normals[v + a] as number) * d;
  }
  return piece;
}

/** Mirrors a piece across the YZ plane (x → -x), fixing winding and normals. */
export function mirrorX(piece: MeshPiece): MeshPiece {
  for (let v = 0; v < piece.positions.length; v += 3) {
    piece.positions[v] = -(piece.positions[v] as number);
    piece.normals[v] = -(piece.normals[v] as number);
  }
  for (let i = 0; i < piece.indices.length; i += 3) {
    const a = piece.indices[i + 1] as number;
    piece.indices[i + 1] = piece.indices[i + 2] as number;
    piece.indices[i + 2] = a;
  }
  return piece;
}

/** Moves a piece by an offset. */
export function translate(piece: MeshPiece, x: number, y: number, z: number): MeshPiece {
  return mapPoints(piece, (p) => {
    p.x += x;
    p.y += y;
    p.z += z;
  });
}

/** Rotates a piece around an axis through the origin. */
export function rotate(piece: MeshPiece, axis: Vector3, degrees: number): MeshPiece {
  const a = (degrees * Math.PI) / 180;
  const ax = axis.clone().normalize();
  const n = new Vector3();
  for (let v = 0; v < piece.normals.length; v += 3) {
    n.set(
      piece.normals[v] as number,
      piece.normals[v + 1] as number,
      piece.normals[v + 2] as number,
    ).applyAxisAngle(ax, a);
    piece.normals[v] = n.x;
    piece.normals[v + 1] = n.y;
    piece.normals[v + 2] = n.z;
  }
  return mapPoints(piece, (p) => {
    p.applyAxisAngle(ax, a);
  });
}

/** The kit as one object, handed to part modules as `ctx.geo`. */
export const geometryKit = {
  sweep,
  arc,
  lathe,
  sphere,
  plate,
  merge,
  empty: emptyPiece,
  bend,
  twist,
  taper,
  displace,
  mirrorX,
  translate,
  rotate,
};
export type GeometryKit = typeof geometryKit;
