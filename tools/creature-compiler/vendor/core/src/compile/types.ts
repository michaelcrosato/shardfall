import type { Vector3 } from 'three';

/** What a bone belongs to. */
export type BoneSection =
  | 'root'
  | 'torso'
  | 'neck'
  | 'head'
  | 'jaw'
  | 'tail'
  | 'limb'
  | 'toe'
  | 'eye'
  /** Eyelids: skin pieces that turn about the eye, not part of the skin's field. */
  | 'lid'
  | 'helper'
  /** A wing's finger bones (docs/design/9.3-wings-fins.md). */
  | 'digit'
  /** Bones that carry a membrane between two spars, or a group of feathers (9.3). */
  | 'station'
  | 'feather'
  /** A part's own bones, from its `bones` hook: antennae, mandibles, pincer fingers (9.4). */
  | 'part';

/** One bone in rest pose, in model space (metres; Y up, the creature faces +Z). */
export interface BoneDef {
  readonly name: string;
  readonly parent: number;
  readonly section: BoneSection;
  /** The limb, part or section this bone belongs to, e.g. `foreleg.L`, `head`. */
  readonly owner: string;
  readonly head: Vector3;
  readonly tail: Vector3;
  /** Unit vector perpendicular to the bone: dorsal on the main axis, the front face on limbs. */
  readonly up: Vector3;
  /** Radius at the head and tail of the bone (metres). */
  readonly r0: number;
  readonly r1: number;
  /** Cross-section scale across (side) and up the bone. */
  readonly cross: readonly [number, number];
  /** The section's own `at` coordinate at the bone's head and tail. */
  readonly t0: number;
  readonly t1: number;
  /** Part of the skin surface (not root, eye or helper bones). */
  readonly skin: boolean;
  /**
   * Radii at evenly spaced points from head to tail, when the section's profile changes within
   * the bone; the skin follows it with one cone per span instead of a straight taper.
   */
  readonly profile?: readonly number[];
  /** Whether anatomy shaped the profile (8.1); thin bones' tubes then follow it too. */
  readonly shaped?: boolean;
  /** The profile before anatomy shaped it, which decides whether the bone is thin. */
  readonly plainProfile?: readonly number[];
  /** The cross-section before anatomy flattened it (legless bodies), for the same decision. */
  readonly plainCross?: readonly [number, number];
  /** Index of the chain this bone belongs to (-1 for none). */
  readonly chain: number;
  /**
   * Always drawn as a swept tube, never in the field or the grid's extent: wing and fin bones,
   * which would otherwise spread the grid over the wingspan (9.3).
   */
  readonly tube?: boolean;
}

/**
 * A mass on a bone: a rounded cone (a capsule when the radii match, a sphere when `a` is `b`)
 * that joins its chain with a smooth minimum against the chain's cones only, so masses never
 * stack on each other. It moves with its bone.
 */
export interface MassDef {
  readonly bone: number;
  readonly a: Vector3;
  readonly b: Vector3;
  readonly ra: number;
  readonly rb: number;
  /** Frame for the cross-section: `up` and the scales across (side) and up. */
  readonly up: Vector3;
  readonly cross: readonly [number, number];
  /** Smooth-min radius against the chain's cones (metres); 0 is a plain union. */
  readonly blend: number;
  /**
   * `detail`: a head detail (lips, brow, cheekbones) kept however small, left out of the grid
   * and shown by the head's refinement. `carve`: a detail subtracted from the chain (nostrils).
   */
  readonly kind?: 'detail' | 'carve';
  /** A small detail the head's refinement splits finer around (brow, cheekbones, nostrils). */
  readonly fine?: boolean;
}

/** A run of bones that join with a plain union; chains join their parents with a smooth min. */
export interface ChainDef {
  readonly id: string;
  readonly section: BoneSection;
  readonly owner: string;
  readonly bones: readonly number[];
  /** Bone this chain grows from (-1 for the root chain). */
  readonly parentBone: number;
  /** Smooth-min radius where it meets its parent chain (metres). */
  readonly blend: number;
  /** Muscle masses, joint caps, the limb root's sphere: blended into the chain's cones. */
  readonly masses: readonly MassDef[];
}

/** One leg for the motion controller. */
export interface LegRig {
  readonly id: string;
  readonly pair: number;
  readonly side: 'left' | 'right';
  /** Bones from hip to ankle. */
  readonly bones: readonly number[];
  readonly lengths: readonly number[];
  /** Rest relative joint angles used by the coupled IK (radians). */
  readonly bends: readonly number[];
  /** Rest-pose ankle position and the direction the knee bulges. */
  readonly restFoot: Vector3;
  readonly pole: Vector3;
  /** Total reach (metres). */
  readonly reach: number;
  /** Toe bones, root to tip per toe. */
  readonly toes: readonly (readonly number[])[];
  /** The stance, which makes a planted foot roll (none keeps plan 1's flat feet). */
  readonly stance?: import('../blueprint/creature.ts').Stance;
}

export interface ArmRig {
  readonly id: string;
  readonly side: 'left' | 'right' | 'center';
  readonly bones: readonly number[];
  readonly lengths: readonly number[];
  readonly bends: readonly number[];
  readonly pole: Vector3;
  readonly reach: number;
  readonly toes: readonly (readonly number[])[];
}

/** One head with its neck, jaw and eyes. */
export interface HeadRig {
  /** `head` for the main (middle) head at every count; the others `head.L1`, `head.R1`, … */
  readonly id: string;
  /** Neck bones from the torso to the head (empty with no neck). */
  readonly neck: readonly number[];
  readonly head: number;
  /** -1 without a jaw. */
  readonly jaw: number;
  /** Eye bones on this head. */
  readonly eyes: readonly number[];
}

/** One tail. */
export interface TailRig {
  /** `tail` for the main tail; the others `tail.L1`, `tail.R1`, … */
  readonly id: string;
  /** Root to tip; a forked tail's branches share the trunk's bones. */
  readonly bones: readonly number[];
  /** Index in `bones` of the first bone after the shared trunk (0 when the tails are separate). */
  readonly branch: number;
}

/**
 * A chain of bones something drives: springs (tails today; tentacles, antennae and ears from
 * phase 9), the blink (eyelids), the jaw (mandibles close with it) or a flare (frills open,
 * quills rise).
 */
export interface DrivenChain {
  readonly owner: string;
  readonly bones: readonly number[];
  readonly drive: 'spring' | 'blink' | 'jaw' | 'grip' | 'flare';
  /** Springs: how hard each point is pulled back toward its rest place per step (0 to 1). */
  readonly stiffness?: number;
  /** Springs: whether the action goals' `swish` swings it (tails). */
  readonly swish?: boolean;
  /** Other drives: relative joint angles per named pose (`rest`, `open`, …). */
  readonly poses?: Readonly<Record<string, readonly number[]>>;
}

/** A limb chain of one of the new roles (wings, fins, tentacles), from phase 9. */
export interface LimbChainRig {
  readonly id: string;
  readonly side: 'left' | 'right' | 'center';
  readonly bones: readonly number[];
}

/**
 * A wing (docs/design/9.3-wings-fins.md): its arm bones, digits and feather groups, built spread
 * (the bind pose) and folded at rest by its `folded` pose.
 */
export interface WingRig extends LimbChainRig {
  /** Digit chains, root to tip; digit 0 continues the arm's last bone. */
  readonly digits: readonly (readonly number[])[];
  /** Feather group bones, which fold with the wing. */
  readonly feathers: readonly number[];
  /** The wing's plane in the bind pose: its unit normal, model space. */
  readonly normal: readonly [number, number, number];
  /** Spread: from the root to the farthest tip (m), and the membrane's area (m²). */
  readonly span: number;
  readonly area: number;
  /**
   * Named poses: a local rotation (x, y, z, w) per bone, over `bones`, then the digits in
   * order, then `feathers`. `folded` is the rest pose; the bind pose is spread.
   */
  readonly poses: Readonly<Record<string, readonly number[]>>;
  /** Folded away inside the body under a wing case until it spreads. */
  readonly covered?: boolean;
  /** Whether it holds the creature up in flight (a wing case does not). */
  readonly lift: boolean;
  /** How it strokes in flight (docs/design/10.4-flight.md). */
  readonly stroke: WingStrokeRig;
}

/** A wing's stroke, compiled from its membrane's style: axes, angles in radians. */
export interface WingStrokeRig {
  /** The wing's axes in the bind pose, model space: straight out, toward the leading edge, and its plane's normal. */
  readonly out: readonly [number, number, number];
  readonly lead: readonly [number, number, number];
  readonly normal: readonly [number, number, number];
  /** +1 when a positive turn about `lead` raises the wing, −1 on the other side; 0 on the midline (it holds still). */
  readonly sign: 1 | -1 | 0;
  /** Half the swing about `lead` (tilted toward `normal` by `plane`). */
  readonly amplitude: number;
  /** The in-plane turn at full flex of each arm bone after the humerus (elbow, wrist, …). */
  readonly flex: readonly number[];
  /** Pitch about the wing's length, leading edge down on the downstroke. */
  readonly twist: number;
  /** Stroke plane, from vertical toward the wing's plane. */
  readonly plane: number;
  /** Share of a beat it trails the wing in front of it on its side (hind wings follow fore). */
  readonly lag: number;
  /** A held wing (a case): lifted and swung forward in flight instead of beating. */
  readonly hold?: { readonly lift: number; readonly forward: number };
}

/**
 * A membrane panel's stations: pairs of bones, one on each spar, aimed at each other, which
 * carry the sheet between the spars exactly whatever the spars do (`applyStations`).
 */
export interface StationPanel {
  readonly a: readonly number[];
  readonly b: readonly number[];
}

/** What the motion controller needs to know about a skeleton. */
export interface Rig {
  readonly root: number;
  /** Torso bones from the back (hips) to the front (chest). */
  readonly spine: readonly number[];
  /** Every head, from the creature's left to its right. */
  readonly heads: readonly HeadRig[];
  /** Index into `heads` of the main head. */
  readonly main: number;
  /** Every tail, from left to right; empty without a tail. */
  readonly tails: readonly TailRig[];
  /** Chains the controller drives (springs, blinks, jaws, flares). */
  readonly chains: readonly DrivenChain[];
  readonly legs: readonly LegRig[];
  readonly arms: readonly ArmRig[];
  readonly wings: readonly WingRig[];
  readonly fins: readonly LimbChainRig[];
  readonly tentacles: readonly LimbChainRig[];
  /** Membrane stations, posed after everything else. */
  readonly stations: readonly StationPanel[];
  /** Hip height above the ground in the rest pose (metres). */
  readonly hipHeight: number;
  /** Whether the creature is a sprawler (insect, lizard) or legless. */
  readonly posture: 'upright' | 'sprawl' | 'legless';
}

/** The main head of a rig. */
export const mainHead = (rig: { readonly heads: readonly HeadRig[]; readonly main: number }) =>
  rig.heads[rig.main] as HeadRig;

/** Every eye bone, on every head. */
export const allEyes = (rig: { readonly heads: readonly HeadRig[] }): number[] =>
  rig.heads.flatMap((h) => h.eyes);

export interface Skeleton {
  readonly bones: BoneDef[];
  readonly chains: ChainDef[];
  readonly rig: Rig;
}

/** Context a foot part gets for its height above the ground, before the leg is posed. */
export interface FootContext {
  readonly role: import('../blueprint/creature.ts').LimbRole;
  readonly stance: import('../blueprint/creature.ts').Stance | undefined;
  /** Limb radius at the tip (metres). */
  readonly tipRadius: number;
  /** Metres per torso length. */
  readonly scale: number;
}

/** Context a foot part gets for growing toes off a limb tip. */
export interface ToeContext {
  readonly ankle: Vector3;
  /** Direction of the last limb segment (unit). */
  readonly limbDir: Vector3;
  /** The creature's forward direction, horizontal (unit). */
  readonly forward: Vector3;
  /** Horizontal unit vector toward the limb's own side. */
  readonly outward: Vector3;
  readonly groundY: number;
  readonly role: import('../blueprint/creature.ts').LimbRole;
  /** Limb radius at the tip (metres). */
  readonly tipRadius: number;
  /** Metres per torso length. */
  readonly scale: number;
  readonly mirror: 1 | -1 | 0;
  readonly splay: number;
  /** The leg's stance (none on arms, or on legs that keep plan 1's pose). */
  readonly stance: import('../blueprint/creature.ts').Stance | undefined;
  /** How far above the ground the ankle is (metres). */
  readonly footHeight: number;
}

/** One toe: joint positions from the ankle outward, with a radius at each point (metres). */
export interface ToeChain {
  readonly points: readonly Vector3[];
  readonly radii: readonly number[];
}
