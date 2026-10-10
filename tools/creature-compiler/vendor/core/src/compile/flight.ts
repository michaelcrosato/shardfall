import { Vector3 } from 'three';
import type { WingRig, WingStrokeRig } from './types.ts';
import { BAT_STYLE, type WingFrame } from './wings.ts';

const DEG = Math.PI / 180;
const v3 = (v: Vector3): [number, number, number] => [v.x, v.y, v.z];

/**
 * A wing's stroke from its style and its frame in the bind pose (docs/design/10.4-flight.md):
 * the axes it strokes about, which way is up for its side, and the angles in radians.
 */
export function strokeOf(frame: WingFrame | undefined, lag: number): WingStrokeRig {
  const style = frame?.style.stroke ?? BAT_STYLE.stroke;
  const out = frame?.out ?? new Vector3(1, 0, 0);
  const lead = frame?.lead ?? new Vector3(0, 0, 1);
  const normal = frame?.normal ?? new Vector3(0, 1, 0);
  // A positive turn about `lead` raises a wing whose `lead × out` points up: the body's left.
  const up = new Vector3().crossVectors(lead, out).y;
  const sign: 1 | -1 | 0 = frame?.mirror === 0 ? 0 : up >= 0 ? 1 : -1;
  const joints = frame?.style.fold.joints ?? [];
  const flex = (frame?.bones ?? [])
    .slice(1)
    .map((_, k) => (joints[k] ?? 0) * (style?.flex ?? 0) * DEG);
  return {
    out: v3(out),
    lead: v3(lead),
    normal: v3(normal),
    sign,
    amplitude: (style?.amplitude ?? 0) * DEG,
    flex,
    twist: (style?.twist ?? 0) * DEG,
    plane: (style?.plane ?? 0) * DEG,
    lag,
    ...(style?.hold
      ? { hold: { lift: style.hold.lift * DEG, forward: style.hold.forward * DEG } }
      : {}),
  };
}

/**
 * How far behind its side's front wing a wing beats: hind wings follow their forewing by 0.05
 * of a beat per wing in front of them, so a pair rows together and never crosses.
 */
export function lagOf(frames: readonly WingFrame[], index: number): number {
  const frame = frames[index];
  if (!frame || frame.style.shell) return 0;
  const ahead = frames.filter(
    (f) => f !== frame && !f.style.shell && f.mirror === frame.mirror && f.at < frame.at,
  ).length;
  return 0.05 * ahead;
}

/**
 * What flight scales with (docs/design/10.4-flight.md): mass (kg), the span tip to tip with the
 * wings spread (m), and the planform area of the wings that lift (m²): each wing's membrane
 * outline in its own bind plane, as a convex hull, so overlapping feather ranks count once.
 */
export function flightOf(
  wings: readonly WingRig[],
  outlines: ReadonlyMap<string, readonly Vector3[]>,
  mass: number,
  spread: { readonly min: readonly number[]; readonly max: readonly number[] },
): { readonly mass: number; readonly span: number; readonly area: number } {
  let area = 0;
  for (const wing of wings) {
    if (!wing.lift) continue;
    const points = outlines.get(wing.id) ?? [];
    const [ox, oy, oz] = wing.stroke.out;
    const [lx, ly, lz] = wing.stroke.lead;
    area += hullArea(
      points.map((p) => [p.x * ox + p.y * oy + p.z * oz, p.x * lx + p.y * ly + p.z * lz]),
    );
  }
  const span = (spread.max[0] ?? 0) - (spread.min[0] ?? 0);
  return { mass, span, area };
}

/** Area of the convex hull of 2D points (Andrew's monotone chain). */
function hullArea(points: [number, number][]): number {
  if (points.length < 3) return 0;
  const sorted = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower: [number, number][] = [];
  for (const p of sorted) {
    while (
      lower.length >= 2 &&
      cross(lower.at(-2) as [number, number], lower.at(-1) as [number, number], p) <= 0
    )
      lower.pop();
    lower.push(p);
  }
  const upper: [number, number][] = [];
  for (const p of sorted.reverse()) {
    while (
      upper.length >= 2 &&
      cross(upper.at(-2) as [number, number], upper.at(-1) as [number, number], p) <= 0
    )
      upper.pop();
    upper.push(p);
  }
  const hull = [...lower.slice(0, -1), ...upper.slice(0, -1)];
  let twice = 0;
  for (let i = 0; i < hull.length; i++) {
    const a = hull[i] as [number, number];
    const b = hull[(i + 1) % hull.length] as [number, number];
    twice += a[0] * b[1] - b[0] * a[1];
  }
  return Math.abs(twice) / 2;
}
