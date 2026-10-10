import { Vector3 } from 'three';

/**
 * Eyelids (docs/design/8.3-heads.md): two shells around an eye's centre, each on a bone of its
 * own that turns about the eye's side axis. Every edge is a great circle through the two side
 * poles, so seen from the front the opening is almond-shaped, and turning a lid keeps it so.
 */
export interface LidFrame {
  readonly centre: Vector3;
  /** Across the eye: the lids turn about this axis. */
  readonly side: Vector3;
  readonly up: Vector3;
  /** Out of the opening. */
  readonly out: Vector3;
  /** The eyeball's radius. */
  readonly radius: number;
}

/** Rest angles of the lids' edges from `out` toward `up` (radians). */
export function lidAngles(squint: number): { upper: number; lower: number; meet: number } {
  const DEG = Math.PI / 180;
  const upper = (38 - 30 * Math.min(1, Math.max(0, squint))) * DEG;
  const lower = -34 * DEG;
  return { upper, lower, meet: lower + 0.25 * (upper - lower) };
}

/** The lid's profile across its edge: angle past the edge (degrees) and radius (eyeballs). */
const PROFILE: readonly (readonly [number, number])[] = [
  [16, 1.01],
  [6, 1.025],
  [0, 1.055],
  [4, 1.08],
  [18, 1.08],
  [36, 1.08],
  [56, 1.08],
  [80, 1.08],
  [105, 1.08],
  [130, 1.08],
];
/** Segments from pole to pole. */
const SEGMENTS = 12;

/** Triangles one eye's two lids add to the skin. */
export const LID_TRIANGLES = 2 * 2 * SEGMENTS * (PROFILE.length - 1);

export interface LidMesh {
  readonly positions: number[];
  readonly normals: number[];
  /** Local to this mesh. */
  readonly indices: number[];
  /** 0 for the upper lid's vertices, 1 for the lower's. */
  readonly which: number[];
}

/** Both lids of an eye at rest, in model space, facing out. */
export function lidMesh(f: LidFrame, squint: number): LidMesh {
  const { upper, lower } = lidAngles(squint);
  const out: LidMesh = { positions: [], normals: [], indices: [], which: [] };
  const DEG = Math.PI / 180;
  for (const [which, edge, dir] of [
    [0, upper, 1],
    [1, lower, -1],
  ] as const) {
    const start = out.which.length;
    // Rows across the edge (the profile), columns from pole to pole.
    for (const [past, scale] of PROFILE) {
      const phi = edge + dir * past * DEG;
      // The rim rolls inward: its rows' normals lean toward the opening.
      const inner = scale < 1.07;
      for (let j = 0; j <= SEGMENTS; j++) {
        const psi = -Math.PI / 2 + (j / SEGMENTS) * Math.PI;
        const local = new Vector3(
          Math.sin(psi),
          Math.cos(psi) * Math.sin(phi),
          Math.cos(psi) * Math.cos(phi),
        );
        const p = f.centre
          .clone()
          .addScaledVector(f.side, local.x * f.radius * scale)
          .addScaledVector(f.up, local.y * f.radius * scale)
          .addScaledVector(f.out, local.z * f.radius * scale);
        const n = new Vector3()
          .addScaledVector(f.side, local.x)
          .addScaledVector(f.up, local.y)
          .addScaledVector(f.out, local.z);
        if (inner) {
          // Toward the edge's tangent, away from the lid: the rounded rim.
          const tangent = new Vector3()
            .addScaledVector(f.up, -Math.cos(phi) * dir)
            .addScaledVector(f.out, Math.sin(phi) * dir);
          n.lerp(tangent, past < 3 ? 0.6 : 0.9).normalize();
        }
        out.positions.push(p.x, p.y, p.z);
        out.normals.push(n.x, n.y, n.z);
        out.which.push(which);
      }
    }
    const cols = SEGMENTS + 1;
    const from = out.indices.length;
    for (let r = 0; r + 1 < PROFILE.length; r++)
      for (let j = 0; j < SEGMENTS; j++) {
        const a = start + r * cols + j;
        const b = a + 1;
        const c = a + cols;
        const d = c + 1;
        out.indices.push(a, c, b, b, c, d);
      }
    // Face out of the eye: judge by a triangle on the outer rows, mid-way between the poles.
    const probe = from + (5 * SEGMENTS + SEGMENTS / 2) * 6;
    const at = (i: number) => {
      const v = out.indices[i] as number;
      return new Vector3(
        out.positions[v * 3] as number,
        out.positions[v * 3 + 1] as number,
        out.positions[v * 3 + 2] as number,
      );
    };
    const a = at(probe);
    const nrm = new Vector3()
      .subVectors(at(probe + 1), a)
      .cross(new Vector3().subVectors(at(probe + 2), a));
    if (nrm.dot(new Vector3().subVectors(a, f.centre)) < 0)
      for (let i = from; i < out.indices.length; i += 3) {
        const x = out.indices[i + 1] as number;
        out.indices[i + 1] = out.indices[i + 2] as number;
        out.indices[i + 2] = x;
      }
  }
  return out;
}
