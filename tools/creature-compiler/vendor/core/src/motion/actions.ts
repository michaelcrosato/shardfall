import type { Vector3 } from 'three';
import type { Rng } from '../rng.ts';

/**
 * What an action sees each step. Actions work in body-relative goals, so one module runs on any
 * creature with the features it needs.
 */
export interface ActionContext {
  /** Progress through the action, 0 to 1 (always 0 for ambient actions). */
  readonly t: number;
  /** Seconds since the action started (since spawn for ambient actions). */
  readonly elapsed: number;
  /** The action's length in seconds for this creature, after scaling by size. */
  readonly duration: number;
  /**
   * √(hip height / 1 m): multiply natural periods by it, so big creatures move slowly and small
   * ones quickly (dynamic similarity).
   */
  readonly timeScale: number;
  /** The action's resolved parameters. */
  readonly params: Readonly<Record<string, unknown>>;
  /** Where the action is aimed, in world space, if anywhere. */
  readonly target: Vector3 | null;
  /** Head position and the body's facing (unit, horizontal), in world space. */
  readonly head: Vector3;
  readonly forward: Vector3;
  /** Ground speed in m/s; ambient actions calm down while walking. */
  readonly speed: number;
  /** Whether a main action is running (for ambient actions to step back). */
  readonly busy: boolean;
  /** Seeded random stream for this creature and action. */
  readonly rng: Rng;
  /**
   * For actions that leap (10.2): the progress at which the feet leave the ground and land
   * again, once the controller has planned the arc.
   */
  readonly leap?: LeapTiming;
}

/** When a leap leaves the ground and lands, as shares of the action's progress (0 to 1). */
export interface LeapTiming {
  readonly takeoff: number;
  readonly land: number;
}

/**
 * How an action carries the body through the air (docs/design/10.2-jumps.md): the controller
 * plans a ballistic arc from these and runs it on the game's ground.
 */
export interface LeapPlan {
  /** Seconds (for a creature with 1 m hips) crouching before takeoff and absorbing the landing. */
  readonly crouch: number;
  readonly recover: number;
  /** Launch angle above horizontal, radians; raised when the ground or `height` needs it. */
  readonly angle: number;
  /** How far it leaps with no target, and at most, in hip heights. */
  readonly reach: number;
  readonly most: number;
  /** Least height (m) the arc clears above the ground between takeoff and landing. */
  readonly height?: number;
  /** Lands so the head meets the target (a pounce) rather than the feet. */
  readonly head?: boolean;
}

/**
 * Body-relative goals an action sets. Each is optional; a main action's goals override the
 * ambient ones they share. Angles are radians.
 */
export interface ActionGoals {
  /** A point the head turns to face, and how strongly (0 to 1, default 1). */
  look?: Vector3 | null;
  lookWeight?: number;
  /** Yaw and pitch offsets for the head from straight ahead (glances, look-around). */
  glance?: { yaw: number; pitch: number };
  /** How far the head lunges toward `look`, as a share of what the neck can reach (0 to 1). */
  reach?: number;
  /** Lifts the head (positive) or drops it, beyond what `look` asks. */
  raise?: number;
  /** Quick side-to-side head shake amplitude. */
  shake?: number;
  /** Jaw opening, 0 closed to 1 wide. */
  jaw?: number;
  /** Lowers the body by this share of hip height (anticipation, crouching). */
  crouch?: number;
  /** Pitches the body up at the front (rearing). */
  rear?: number;
  /** Shifts the body's weight sideways, as a share of hip height. */
  shift?: number;
  /** Chest expansion, 0 to 1. */
  breath?: number;
  /** Eyelids, 0 open to 1 shut. */
  blink?: number;
  /** Tail swing sideways. */
  swish?: number;
  /** Raises the arms forward to reach for what the head looks at, 0 to 1 (arms only). */
  arms?: number;
  /** Spreads the wings: 0 folded at rest, 1 spread wide (docs/design/9.3-wings-fins.md). */
  wings?: number;
  /** Closes grip-driven parts (a pincer's finger): 0 at rest, 1 shut, negative opens wide (9.4). */
  grip?: number;
  /** `arms` and `grip` move only the side nearest `look`: one claw pinches (9.4). */
  nearest?: boolean;
  /** Reaches the tentacles nearest the look target toward it, 0 to 1 (9.4). */
  grab?: number;
  /**
   * Whips the tail or tentacle nearest the look target: 0 to 1 along the strike, negative
   * winding up away from it (9.4).
   */
  lash?: number;
  /** The most a lash sweeps toward its target, in radians (default π/2). */
  lashArc?: number;
  /**
   * Opens frills and hoods and raises quills and sails, 0 at rest to 1 fully open
   * (docs/design/9.5-coverings.md).
   */
  flare?: number;
  /** Stand still while the action runs. */
  stop?: boolean;
}

export interface ActionHooks {
  /**
   * Ambient actions (idle) run all the time underneath main actions; main actions run one at a
   * time when asked.
   */
  readonly ambient?: boolean;
  /** Seconds for a creature with 1 m hips (scaled by `timeScale`); unused for ambient actions. */
  duration(params: Readonly<Record<string, unknown>>): number;
  /**
   * Events fired as progress passes each `at` (0 to 1), e.g. `bite-contact` at the snap. Leaping
   * actions get their takeoff and landing, so an event can fall on either.
   */
  events?(
    params: Readonly<Record<string, unknown>>,
    leap?: LeapTiming,
  ): readonly { at: number; type: string }[];
  /** Actions that leave the ground: how to leap (10.2). `takeoff` and `land` events come free. */
  leap?(params: Readonly<Record<string, unknown>>): LeapPlan;
  /** Writes the goals for this moment into `out`. */
  goals(ctx: ActionContext, out: ActionGoals): void;
}

/** Smooth 0→1 between `a` and `b`. */
export function ramp(t: number, a: number, b: number): number {
  const x = Math.min(1, Math.max(0, (t - a) / (b - a)));
  return x * x * (3 - 2 * x);
}

/** Rises 0→1 over [a, b], holds, falls 1→0 over [c, d]. */
export function envelope(t: number, a: number, b: number, c: number, d: number): number {
  return ramp(t, a, b) * (1 - ramp(t, c, d));
}
