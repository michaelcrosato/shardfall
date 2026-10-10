import { Quaternion, Vector3 } from 'three';
import type { CompiledCreature } from '../compile/compile.ts';
import type { WingRig } from '../compile/types.ts';
import type { Pose } from './pose.ts';

const G = 9.81;
/** Air density at sea level (kg/m³). */
const RHO = 1.225;
/** Wing loading above which a creature could not fly (N/m²); speeds stop growing past it. */
export const MAX_LOADING = 700;
/** The fastest a wingbeat is shown (Hz), whatever size predicts. */
const MAX_BEAT = 10;

/** What a winged creature flies at (docs/design/10.4-flight.md). */
export interface FlightNumbers {
  readonly mass: number;
  /** Tip to tip (m), and the lifting wings' planform area (m²). */
  readonly span: number;
  readonly area: number;
  /** Wing loading (N/m²). */
  readonly loading: number;
  /** Cruising speed from wing loading, and the slowest it flies flapping hard (m/s). */
  readonly cruise: number;
  readonly slow: number;
  /** The slowest it glides (m/s). */
  readonly stall: number;
  /** Wingbeat frequency at cruise (Hz), from Pennycuick's fit for birds. */
  readonly beat: number;
  /** The wing roots' height above the root at rest, and half the span (m). */
  readonly shoulder: number;
  readonly halfSpan: number;
  /** The height above the ground it cruises at when asked for none (m). */
  readonly height: number;
}

/**
 * Flight numbers from the compiled body (decision 4 of the design): cruise
 * `v = √(2 · W/S / (ρ · C_L))` with C_L 1 and the loading capped at `MAX_LOADING`, slow flight
 * half that, and Pennycuick's wingbeat `f = m^(3/8) g^(1/2) b^(-23/24) S^(-1/3) ρ^(-3/8)`.
 */
export function flightNumbers(compiled: CompiledCreature): FlightNumbers | undefined {
  const flight = compiled.motion.flight;
  if (!flight || compiled.rig.wings.length === 0) return undefined;
  const { mass, span } = flight;
  // A wing that lifts nothing (cases only) still flies the numbers of a small area.
  const area = Math.max(flight.area, 1e-4);
  const loading = (mass * G) / area;
  const cruise = Math.sqrt((2 * Math.min(loading, MAX_LOADING)) / RHO);
  const beat = Math.min(
    MAX_BEAT,
    mass ** 0.375 *
      G ** 0.5 *
      Math.max(span, 0.05) ** (-23 / 24) *
      area ** (-1 / 3) *
      RHO ** -0.375,
  );
  // Where the wings leave the body, above the root, at rest.
  const pos = compiled.bones.positions;
  const roots = compiled.rig.wings.map((w) => pos[(w.bones[0] as number) * 3 + 1] as number);
  const shoulder = roots.length > 0 ? Math.max(...roots) : 0;
  const halfSpan = span / 2;
  return {
    mass,
    span,
    area: flight.area,
    loading,
    cruise,
    slow: 0.5 * cruise,
    stall: 0.7 * cruise,
    beat,
    shoulder,
    halfSpan,
    height: Math.max(2, shoulder + halfSpan + 1),
  };
}

/** Where every wing is in its beat, and how it beats now (all blended by the controller). */
export interface StrokeState {
  /** Beat phase 0 to 1; the downstroke runs from 0.25 to 0.75. */
  readonly phase: number;
  /** Multiplies each wing's amplitude (effort, braking; 0 in a glide). */
  readonly amplitude: number;
  /** 0 beating, 1 gliding: the wings settle at a slight dihedral instead. */
  readonly glide: number;
  /** 0 to 1: the stroke plane turns toward the wing's own plane, as in a hover. */
  readonly hover: number;
  /** Most the wings may rise above and drop below their spread pose (radians), for clearance. */
  readonly up: number;
  readonly down: number;
  /** 0 to 1: how far held wings (cases) are lifted. */
  readonly hold: number;
  /** Raised above the stroke (radians), clearance limits aside: wings held up to land. */
  readonly raise?: number;
}

const scratchAxis = new Vector3();
const scratchQ = new Quaternion();
const scratchP = new Quaternion();
const scratchPi = new Quaternion();
const body = new Quaternion();
const bodyInverse = new Quaternion();
const Y = new Vector3(0, 1, 0);
/** The dihedral wings settle at while gliding. */
const DIHEDRAL = (5 * Math.PI) / 180;

/**
 * Beats the spread wings (docs/design/10.4-flight.md, decision 6): each lifting wing's humerus
 * swings about its leading-edge axis (tilted toward its plane's normal by the stroke plane), in
 * the body's frame whatever its parent does; the wing twists about its own length; on the
 * upstroke its outer joints flex in the wing's plane. Held wings (cases) lift and swing forward.
 * Centre wings hold still. Bones are posed locally; the caller solves them.
 */
export function applyStrokes(
  pose: Pose,
  wings: readonly WingRig[],
  spine0: number,
  state: StrokeState,
): void {
  // The body's turn from its bind pose: stroke axes are stored in the bind's model space.
  body
    .copy(pose.worldRot[spine0] as Quaternion)
    .multiply(bodyInverse.copy(pose.bindWorldRot[spine0] as Quaternion).invert());
  bodyInverse.copy(body).invert();
  for (const wing of wings) {
    const s = wing.stroke;
    if (s.sign === 0) continue;
    const humerus = wing.bones[0] as number;
    const parent = pose.parents[humerus] as number;
    if (parent < 0) continue;
    pose.solveBone(parent);
    scratchP.copy(pose.worldRot[parent] as Quaternion);
    scratchPi.copy(scratchP).invert();
    const rot = pose.rot[humerus] as Quaternion;
    const [lx, ly, lz] = s.lead;
    const [nx, ny, nz] = s.normal;
    if (s.hold) {
      // A case lifts about its leading edge and swings forward about its plane's normal.
      const lift = s.sign * s.hold.lift * state.hold;
      const forward = s.sign * s.hold.forward * state.hold;
      turnInBody(rot, scratchAxis.set(lx, ly, lz), lift);
      turnInBody(rot, scratchAxis.set(nx, ny, nz), forward);
      continue;
    }
    if (!wing.lift) continue;
    const phase = 2 * Math.PI * (state.phase - s.lag);
    // The beat, or a glide's slight dihedral, within the clearance limits.
    const beat = s.amplitude * state.amplitude * Math.sin(phase);
    const angle =
      Math.max(-state.down, Math.min(state.up, beat * (1 - state.glide) + DIHEDRAL * state.glide)) +
      (state.raise ?? 0);
    // The stroke plane: from the leading edge toward the normal (a hover sweeps in-plane).
    const plane = s.plane + (Math.PI / 2 - s.plane) * 0.7 * state.hover;
    scratchAxis
      .set(lx, ly, lz)
      .multiplyScalar(Math.cos(plane))
      .addScaledVector(scratchAxis.clone().set(nx, ny, nz), Math.sin(plane))
      .normalize();
    turnInBody(rot, scratchAxis, s.sign * angle);
    // Twist about the humerus's own length: leading edge down on the downstroke.
    const twist = s.twist * Math.cos(phase) * (1 - state.glide) * Math.min(1, state.amplitude);
    if (twist !== 0) rot.multiply(scratchQ.setFromAxisAngle(Y, -s.sign * twist));
    // Upstroke flex: the outer joints turn in the wing's plane, about its bind normal in each
    // parent bone's bind frame (the joints only ever turn about that normal).
    const up = Math.max(0, Math.cos(phase)) * (1 - state.glide) * Math.min(1, state.amplitude);
    if (up > 0) {
      const flex = (bone: number, full: number) => {
        const p = pose.parents[bone] as number;
        const axis = scratchAxis
          .set(nx, ny, nz)
          .applyQuaternion(scratchQ.copy(pose.bindWorldRot[p] as Quaternion).invert());
        (pose.rot[bone] as Quaternion).premultiply(
          scratchQ.setFromAxisAngle(axis, s.sign * full * up),
        );
      };
      s.flex.forEach((full, k) => {
        const bone = wing.bones[k + 1];
        if (bone !== undefined && full !== 0) flex(bone, full);
      });
      // Digits that start at a joint turn with the bone after it (fingers from the wrist turn
      // with the hand), not with the bone they hang from.
      for (const digit of wing.digits) {
        const first = digit[0];
        if (first === undefined) continue;
        const full = s.flex[wing.bones.indexOf(pose.parents[first] as number)] ?? 0;
        if (full !== 0) flex(first, full);
      }
    }
  }

  /** Turns a humerus's local rotation by `angle` about `axis` given in the body's bind frame. */
  function turnInBody(rot: Quaternion, axis: Vector3, angle: number): void {
    if (angle === 0) return;
    axis.applyQuaternion(body);
    // In the parent's frame: P⁻¹ · R(axis) · P, applied before the local rotation.
    const turn = scratchQ.setFromAxisAngle(axis, angle);
    rot.premultiply(scratchPi.clone().multiply(turn).multiply(scratchP));
  }
}

/** Least radius it turns in (m): slow flight at a 60° bank. */
export function turnRadius(numbers: FlightNumbers): number {
  return (numbers.slow * numbers.slow) / (G * Math.tan(Math.PI / 3));
}
