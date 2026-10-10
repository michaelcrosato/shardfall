import { Euler, Quaternion, Vector3 } from 'three';
import type { Temperament } from '../blueprint/creature.ts';
import type { CompiledCreature, LegRigData } from '../compile/compile.ts';
import { type PreparedLimb, prepareLimb, solvePrepared } from '../compile/ik.ts';
import { type HeadRig, mainHead } from '../compile/types.ts';
import type { ActionModule, Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import {
  type ActionContext,
  type ActionGoals,
  type ActionHooks,
  type LeapTiming,
  ramp,
} from './actions.ts';
import { type BoneHulls, boneHulls, hullDepth, turnWorld } from './death.ts';
import { fabrik } from './fabrik.ts';
import { applyFace } from './face.ts';
import { applyStrokes, type FlightNumbers, flightNumbers, turnRadius } from './flight.ts';
import { Pose } from './pose.ts';
import { type FootRoll, footRoll, heelAt, plantToes, poseToes } from './roll.ts';
import { applyStations, applyWings } from './wings.ts';

const G = 9.81;
/** How far a gait with flight lowers the body, as a share of hip height. */
const LOW = 0.12;
const STEP = 1 / 120;
const UP = new Vector3(0, 1, 0);
const X_AXIS = new Vector3(1, 0, 0);
const Y_AXIS = new Vector3(0, 1, 0);
const Z_AXIS = new Vector3(0, 0, 1);

/** Gait timing resolved for one creature (from the gait modules at compile time). */
export interface GaitInfo {
  readonly id: string;
  /** Phase offset between successive leg pairs, for this creature's pair count. */
  readonly wave: number;
  /**
   * Where each leg's foot lands in the cycle, 0 to 1, by `2 × pair + (right ? 1 : 0)` (pair 0 is
   * the hindmost), for gaits that set it themselves (a gallop's lead); else the wave formula.
   */
  readonly phases?: readonly number[];
  /** Share of the cycle each foot is planted, at the slow end of the gait's Froude range. */
  readonly duty: number;
  /** The same at the fast end, when it changes with speed (10.1). */
  readonly dutyFast?: number;
  /** Speeds the gait suits, as Froude numbers. */
  readonly froude: readonly [number, number];
  /** The Froude number it looks typical at (default the middle of its range, at most 1). */
  readonly natural?: number;
  /** Foot lift as a share of hip height. */
  readonly stepHeight: number;
  readonly stride: number;
  /** Stride multiplier at the fast end, when it changes with speed. */
  readonly strideFast?: number;
  /** How far the spine flexes and extends each stride, 0 to 1. */
  readonly flex?: number;
  /** Degrees the body leans forward at the top of the gait's range. */
  readonly lean?: number;
  /** Every foot leaves the ground at once some time in the cycle. */
  readonly flight?: boolean;
  /** Where the gait moves it, when not on land (10.3, 10.4). */
  readonly medium?: 'water' | 'air';
  /** What drives a water gait: a body wave, paddling legs or beating fins. */
  readonly swim?: 'body' | 'legs' | 'fins';
  /** How an air gait flies, and its settings (10.4). */
  readonly air?: 'flapping' | 'gliding' | 'hovering';
  /** Air gaits: stroke amplitude multiplier, most bank (degrees), glide sink (0–1), beat rate. */
  readonly stroke?: number;
  readonly bank?: number;
  readonly sink?: number;
  readonly rate?: number;
  /** Slither gaits move the spine instead of legs. */
  readonly spine: boolean;
  readonly amplitude: number;
  readonly waves: number;
}

export interface MotionData {
  readonly temperament: Temperament;
  readonly gaits: readonly GaitInfo[];
  /** Actions the creature can perform, with resolved parameters. */
  readonly actions: readonly { readonly id: string; readonly params: Record<string, unknown> }[];
  /**
   * What flight scales with, for winged creatures (docs/design/10.4-flight.md): mass (kg), the
   * span tip to tip spread (m) and the planform area of the wings that lift (m²).
   */
  readonly flight?: { readonly mass: number; readonly span: number; readonly area: number };
}

export interface GroundSample {
  readonly height: number;
  readonly normal?: readonly [number, number, number];
}
/** Height (and optionally normal) of the ground at (x, z). */
export type Ground = (x: number, z: number) => GroundSample;

export interface WaterSample {
  /** Height of the water's surface (m). */
  readonly surface: number;
}
/** Water at (x, z): its surface, or null where there is none (the ground is the bed). */
export type Water = (x: number, z: number) => WaterSample | null;
const FLAT: Ground = () => ({ height: 0 });

/**
 * Something that happened during `update`: a `footstep` (with `leg` and `position`), a `medium`
 * change (into the water or out), a `gait`
 * change, an action's start and end (`action-start`, `action-end`), or an event an action
 * declares, such as `bite-contact` or `roar-peak` (with `action` and the head's `position`).
 */
export interface MotionEvent {
  readonly type: string;
  readonly time: number;
  readonly leg?: string;
  readonly position?: readonly [number, number, number];
  readonly gait?: string;
  readonly action?: string;
  /** Action events: the head that acted (`head`, `head.L1`, …), when there are several. */
  readonly head?: string;
  /** `medium` events: where it moves now (into the water, out onto land). */
  readonly medium?: 'land' | 'water' | 'air';
  /** `hit` events: the bone the blow landed on. */
  readonly bone?: string;
}

/** Pace, posture and attitude per temperament. */
const TEMPERAMENTS: Record<
  Temperament,
  { walk: number; turn: number; crouch: number; headDrop: number }
> = {
  calm: { walk: 0.12, turn: 100, crouch: 0, headDrop: 0 },
  stalking: { walk: 0.06, turn: 70, crouch: 0.1, headDrop: 0.25 },
  skittish: { walk: 0.22, turn: 200, crouch: 0, headDrop: 0 },
  aggressive: { walk: 0.3, turn: 150, crouch: 0.05, headDrop: 0.1 },
  lumbering: { walk: 0.08, turn: 55, crouch: 0, headDrop: 0.05 },
};

/** Flight in progress (docs/design/10.4-flight.md). */
interface AirState {
  /** The takeoff's crouch and leap, flight, and the landing's flare (or a hoverer's descent). */
  stage: 'crouch' | 'launch' | 'flight' | 'flare' | 'descend';
  /** Seconds in the stage. */
  t: number;
  /** Vertical speed (m/s). */
  vy: number;
  /** Bank (radians, positive into a left turn). */
  bank: number;
  /** The air gait's role now. */
  role: 'flapping' | 'gliding' | 'hovering';
  /** A landing asked for: where, its plan once made, and how often it went round. */
  landing: { to: Vector3 | null; plan: LandingPlan | null; goRounds: number } | null;
  /** The flare's start: height and speed. */
  flare?: { y0: number; v0: number };
}

/** A death in progress, or over (docs/design/10.5-hits-death.md). */
interface DeathState {
  /** Seconds since it began, and how long the collapse takes. */
  t: number;
  readonly duration: number;
  /** The fall: about `axis` (in the root's frame) to `angle`, pivoting on `pivot` at its feet. */
  readonly axis: Vector3;
  readonly angle: number;
  readonly pivot: Vector3;
  /** Where the feet were when it died (world), to buckle from. */
  readonly feet: readonly Vector3[];
  /** Lift that keeps it on the ground (m), and how far its necks droop (radians). */
  lift: number;
  droop: number;
  /** Where each head came to rest last frame (its neck's droop), to search from. */
  readonly droops: number[];
  /** Above the ground when it died (a flyer, a swimmer): height above the floor (m) and speed. */
  height: number;
  vy: number;
  readonly sinks: boolean;
  /** How fast it sinks (m/s): to the bed within about 3 s. */
  readonly sink: number;
  /** Its pitch and roll when it died (a flyer banks), eased out as it falls. */
  readonly pitch: number;
  readonly roll: number;
  /** When it reached the floor (s into the death), how long it has lain still, and whether it is at rest. */
  down: number;
  still: number;
  rest: boolean;
}

/** Where a landing touches down and how it comes in. */
interface LandingPlan {
  readonly point: Vector3;
  /** The line it comes in on (unit, horizontal). */
  readonly dir: Vector3;
  readonly flareStart: Vector3;
  /** Height the flare starts at. */
  readonly flareY: number;
}

interface LegState {
  readonly rig: LegRigData;
  readonly limb: PreparedLimb;
  /** Where in the cycle the foot lands; eased toward a new gait's when the gait changes. */
  offset: number;
  /** Rest foot position relative to the root, in body space. */
  readonly neutral: Vector3;
  readonly pole: Vector3;
  readonly footLift: number;
  readonly planted: Vector3;
  readonly liftoff: Vector3;
  readonly target: Vector3;
  swinging: boolean;
  /** Ready to lift: set during stance, cleared at lift-off. */
  armed: boolean;
  readonly points: Vector3[];
  /** The planted foot's roll, for legs with a stance and toes along the ground. */
  readonly roll: FootRoll | undefined;
  /** Swing progress, 0 to 1 (for the toes letting go). */
  swingU: number;
}

interface Spring {
  readonly bones: readonly number[];
  readonly points: Vector3[];
  readonly previous: Vector3[];
  readonly lengths: number[];
  readonly stiffness: number;
  /** Swung by the `swish` goal and the sprawling wave (tails). */
  readonly swish: boolean;
  /** A tail, a tentacle or a part's chain (an antenna). */
  readonly kind: 'tail' | 'tentacle' | 'part';
  /** Per point, how far above the ground it stays (tentacles and parts drape; tails don't). */
  readonly clearance: number[] | undefined;
  /**
   * Per point, where the ground was last sampled under it and its height there (x, z, height):
   * terrain changes over metres, so a point resamples only once it has moved a little.
   */
  readonly floors: Float64Array | undefined;
}

export interface MotionOptions {
  /** Module registry, for the action modules' code. Without it the creature only moves. */
  readonly registry?: Registry;
  /** Gait and action data (default: the compiled creature's own). */
  readonly motion?: MotionData;
}

interface RunningAction {
  readonly id: string;
  readonly hooks: ActionHooks;
  readonly params: Record<string, unknown>;
  readonly target: Vector3 | null;
  readonly start: number;
  /** Seconds; a leap's is set once its arc is planned. */
  duration: number;
  events: readonly { at: number; type: string }[];
  readonly rng: Rng;
  progress: number;
  /** A leaping action's arc, planned on its first step (docs/design/10.2-jumps.md). */
  leap?: Leap;
}

/** A planned leap: a ballistic arc from where it stands to where it lands. */
interface Leap {
  readonly from: Vector3;
  readonly to: Vector3;
  readonly heading: number;
  /** Launch speed upward (m/s) and seconds in the air. */
  readonly rise: number;
  readonly flight: number;
  /** Seconds after the action starts at which it leaves the ground and lands. */
  readonly takeoff: number;
  readonly land: number;
  /** Shares of the action's progress at which it leaves the ground and lands. */
  readonly timing: LeapTiming;
  airborne: boolean;
  landed: boolean;
}

const damp = (current: number, target: number, rate: number, dt: number) =>
  target + (current - target) * Math.exp(-rate * dt);

/**
 * Procedural locomotion. Motion is body-relative goals fitted to whatever skeleton the creature
 * has: legs step in a gait phase pattern, feet plant on the ground and IK fits the legs, the
 * spine bends into turns, the head stays level and tails swing on springs. Deterministic at a
 * fixed 120 Hz step.
 */
export class MotionController {
  readonly pose: Pose;
  /** Root position on the ground and facing (radians; 0 faces +Z). */
  readonly position = new Vector3();
  heading = 0;
  speed = 0;
  time = 0;
  /** Gait phase, 0 to 1. */
  phase = 0;
  gait: GaitInfo | undefined;

  private readonly compiled: CompiledCreature;
  private readonly motion: MotionData;
  private readonly hipHeight: number;
  private readonly legs: LegState[];
  private readonly springs: Spring[];
  /** How far the wings are spread (damped toward the `wings` goal), and the standing goal. */
  private spread = 0;
  private wingGoal = 0;
  /** How far grip-driven parts are shut, damped toward the `grip` goal. */
  /** Grip-driven parts on the creature's left and right, damped toward the `grip` goal. */
  private gripLeft = 0;
  private gripRight = 0;
  /** Frills, hoods, quills and sails, damped toward the `flare` goal (0 at rest, 1 open). */
  private flare = 0;
  private readonly reachPoints: Vector3[] = [];
  /** Per arm, how far the `arms` goal turns it at 1 (radians). */
  private readonly armReach: readonly number[];
  /**
   * How far a point moves before it samples the ground under it again (m): a dead bone after
   * one step, a spring point after three.
   */
  private readonly floorStep: number;
  private wingTree: number[] | undefined;
  private bodyTree: number[] | null | undefined;
  private readonly trail: Vector3[] = [];
  private target: Vector3 | null = null;
  private desiredSpeed = 0;
  private lookTarget: Vector3 | null = null;
  private yawRate = 0;
  private accumulator = 0;
  private pitch = 0;
  private roll = 0;
  private bodyY = 0;
  private bend = 0;
  /** Duty now: the gait's at this speed, eased across a gait change. */
  private dutyNow = 0.7;
  /** Height the body has risen in a flight phase, and its vertical speed (m, m/s). */
  private airY = 0;
  private airV = 0;
  private airborne = false;
  /** Forward lean of a running body (radians), eased. */
  private lean = 0;
  /** Where it is moving: on land (or wading) or swimming (10.3). */
  medium: 'land' | 'water' | 'air' = 'land';
  /** What it flies at, for a winged creature (docs/design/10.4-flight.md). */
  readonly flightNumbers: FlightNumbers | undefined;
  /** Flight in progress, from the takeoff crouch to touchdown. */
  private air: AirState | null = null;
  /** Asked for while an action ran: takes off or lands when it ends. */
  private pendingAir:
    | { readonly kind: 'takeoff'; readonly height?: number; readonly speed?: number }
    | { readonly kind: 'land'; readonly to: Vector3 | null }
    | null = null;
  /** Asked height above the floor, and speed, while flying (undefined: the defaults). */
  private flyHeight: number | undefined;
  private flySpeed: number | undefined;
  /** Where it circles when flying with nowhere to go. */
  private readonly loiter = new Vector3();
  /** How the wings beat now, blended as the air gait changes. */
  private readonly strokes = {
    phase: 0,
    amplitude: 0,
    glide: 0,
    hover: 0,
    up: Math.PI / 2,
    down: Math.PI / 3,
    hold: 0,
    /** Beats per second now. */
    beat: 0,
    /** Wings raised above the stroke (radians): held up as it lands. */
    raise: 0,
  };
  /** False while a distant flyer steps without posing (level of detail); see `update`. */
  private posing = true;
  /** The pose is behind the flight state: pose twice and settle the springs before showing it. */
  private stalePose = false;
  /** Extra lowering of the body (m): a takeoff's crouch, a landing's absorb. */
  private airCrouch = 0;
  /** When it last touched down (s): the landing's absorb dips the body for a moment after. */
  private landedAt: number | undefined;
  /** 1 pitching about the wing roots in flight, 0 about the body's middle as on the ground. */
  private airPivot = 1;
  /**
   * A blow's flinch (docs/design/10.5-hits-death.md): the spine's bend with it, sideways and in
   * pitch (radians), their rates, and how far toward the head it is centred (0 to 1).
   */
  private readonly flinch = { x: 0, y: 0, vx: 0, vy: 0, focus: 0 };
  /** A blow it holds its ground against: the body's sway (m, world) and its rate. */
  private readonly sway = new Vector3();
  private readonly swayV = new Vector3();
  /** A blow that knocks it off balance: the body's slide (m/s, world) and stepping time left. */
  private readonly push = new Vector3();
  private staggerT = 0;
  private steadying = false;
  /** A death in progress or over, and the bones' support hulls it rests on. */
  private death: DeathState | null = null;
  private hulls: BoneHulls | undefined;
  /** The ground of the last update, for posing a death. */
  private groundNow: Ground = FLAT;
  /** Height asked for by `moveTo` (a diver's depth), if any. */
  private targetY: number | undefined;
  /** The spine's height while swimming (m), the tail's swing and the fins' beat now. */
  private swimY = 0;
  private swimSwish = 0;
  private finBeat = 0;
  private events: MotionEvent[] = [];
  private readonly restBodyY: number;
  private readonly halfStride: number;
  /** The same with the body lowered for a gait with flight, which reaches further (10.1). */
  private readonly halfStrideLow: number;
  /** A legless body whose neck rises well above it in the rest pose. */
  private readonly rearing: boolean;
  /** Arms on the neck: an upright front (a centaur's human torso) that stays upright. */
  private readonly uprightFront: boolean;
  /** Per leg (in `compiled.rig.legs` order): how far the foot was out of reach last frame (m). */
  readonly legMiss: number[] = [];
  /** √(hip height / 1 m): actions and springs run slower on big creatures. */
  readonly timeScale: number;
  private readonly actionDefs = new Map<
    string,
    { hooks: ActionHooks; params: Record<string, unknown> }
  >();
  private readonly ambient: {
    id: string;
    hooks: ActionHooks;
    params: Record<string, unknown>;
    rng: Rng;
  }[] = [];
  private current: RunningAction | null = null;
  private actionCount = 0;
  private goals: ActionGoals = {};
  private readonly springRest: Vector3[] = [];
  private readonly slitherJoints: Vector3[] = [];

  constructor(compiled: CompiledCreature, options: MotionOptions = {}) {
    const motion = options.motion ?? compiled.motion;
    this.compiled = compiled;
    this.motion = motion;
    this.pose = new Pose(compiled.bones);
    // Legless bodies have no hips; their pace scales with body length instead.
    this.hipHeight =
      compiled.rig.posture === 'legless'
        ? Math.max(0.05 * compiled.scale, 0.12 * this.spineLength())
        : Math.max(0.05 * compiled.scale, compiled.rig.hipHeight);
    const spine0 = compiled.rig.spine[0] as number;
    this.restBodyY = (this.pose.restPos[spine0] as Vector3).y;
    this.bodyY = this.restBodyY;
    // Start on land: a walking gait, else a slither; a fish (no land gait) starts swimming.
    const land = motion.gaits.filter((g) => (g.medium ?? 'land') === 'land');
    this.gait = land.find((g) => !g.spine) ?? land[0] ?? motion.gaits[0];
    this.legs = compiled.rig.legs.map((rig) => {
      const neutral = new Vector3(...rig.restFoot);
      return {
        rig,
        limb: prepareLimb({ lengths: rig.lengths, bends: rig.bends }),
        offset: offsetFor(rig, this.gait),
        neutral,
        pole: new Vector3(...rig.pole),
        footLift: neutral.y,
        planted: neutral.clone(),
        liftoff: neutral.clone(),
        target: neutral.clone(),
        swinging: false,
        armed: true,
        points: Array.from({ length: rig.lengths.length + 1 }, () => new Vector3()),
        roll: footRoll(rig, this.pose),
        swingU: 0,
      };
    });
    for (const leg of this.legs)
      if (leg.roll) plantToes(leg.roll, leg.neutral, leg.footLift, 0, () => 0);
    this.dutyNow = this.gait?.duty ?? 0.7;
    // How far a foot can travel fore and aft of its neutral spot while planted.
    // How far a foot can travel fore and aft of its neutral spot while planted, standing and
    // with the body lowered by `LOW` of the hip height as a galloper's is.
    const room = (drop: number) =>
      Math.max(
        0.05 * compiled.scale,
        Math.min(
          ...this.legs.map((leg) => {
            // Room fore and aft of the rest foot within the leg's reach, allowing for how far the
            // foot sits out to the side and ahead of or behind the hip.
            const hip = this.pose.restWorldPos[leg.rig.bones[0] as number] as Vector3;
            const dx = leg.neutral.x - hip.x;
            const dy = hip.y - drop - leg.neutral.y;
            // Bipeds stand nearly straight-legged and use more of their reach.
            const most = (this.legs.length <= 2 ? 0.97 : 0.93) * leg.rig.reach;
            const across = Math.sqrt(Math.max(0, most * most - dx * dx - dy * dy));
            return Math.max(0, across - Math.abs(leg.neutral.z - hip.z)) * 0.85;
          }),
          Number.POSITIVE_INFINITY,
        ),
      );
    this.halfStride = room(0);
    this.halfStrideLow = room(LOW * this.hipHeight);
    const main = mainHead(compiled.rig);
    const neckRoot = main.neck[0];
    this.rearing =
      compiled.rig.posture === 'legless' &&
      neckRoot !== undefined &&
      (this.pose.restWorldPos[main.head] as Vector3).y -
        (this.pose.restWorldPos[neckRoot] as Vector3).y >
        3 * (compiled.bones.radii[neckRoot] ?? 0);
    this.uprightFront = compiled.rig.arms.some(
      (arm) =>
        compiled.bones.sections[compiled.bones.parents[arm.bones[0] as number] as number] ===
        'neck',
    );
    // How far each arm turns to reach forward: hanging arms 1.3 rad, arms already held forward
    // (a scorpion's claws) only enough to lift the hand a little above level.
    this.armReach = compiled.rig.arms.map((arm) => {
      const first = arm.bones[0] as number;
      const last = arm.bones.at(-1) as number;
      const hand = scratch1
        .set(0, this.pose.lengths[last] as number, 0)
        .applyQuaternion(this.pose.restWorldRot[last] as Quaternion)
        .add(this.pose.restWorldPos[last] as Vector3)
        .sub(this.pose.restWorldPos[first] as Vector3);
      return Math.max(0.25, Math.min(1.3, 0.62 - Math.atan2(hand.y, hand.z)));
    });
    this.floorStep = 0.05 * compiled.scale;
    this.timeScale = Math.sqrt(this.hipHeight / 1);
    this.flightNumbers = flightNumbers(compiled);
    const rng = createRng(compiled.seed);
    for (const action of motion.actions) {
      const module = options.registry?.get('action', action.id) as ActionModule | undefined;
      const hooks = module?.hooks;
      if (!hooks) continue;
      this.actionDefs.set(action.id, { hooks, params: action.params });
      if (hooks.ambient)
        this.ambient.push({
          id: action.id,
          hooks,
          params: action.params,
          rng: rng.stream(`action.${action.id}`),
        });
    }
    this.springs = compiled.rig.chains
      .filter((c) => c.drive === 'spring' && c.bones.length > 1)
      .map((c) => this.makeSpring(c.bones, c.stiffness ?? 0.35, c.swish ?? false));
    // Each extra head replays the glances a little later, by a delay from its own stream, so
    // heads look about on their own; never mirrored, which would turn neighbours into each
    // other (docs/design/9.1-heads-tails.md).
    this.headGlance = compiled.rig.heads.map((h, i) =>
      i === compiled.rig.main ? 0 : rng.stream(`glance:${h.id}`).float(0.4, 1.6),
    );
    this.pose.solve();
  }

  /** Per head: how long after the shared glance it looks (seconds). */
  private readonly headGlance: readonly number[];
  /** Recent glances, newest last, for heads that follow late. */
  private readonly glances: { time: number; yaw: number; pitch: number }[] = [];

  /** The glance a head makes now: the shared one, its delay ago. */
  private glanceFor(head: number): { yaw: number; pitch: number } | undefined {
    const g = this.goals.glance;
    const delay = this.headGlance[head] ?? 0;
    if (!g || delay === 0) return g;
    const when = this.time - delay;
    let past = this.glances[0];
    for (const entry of this.glances) {
      if (entry.time > when) break;
      past = entry;
    }
    return past ? { yaw: past.yaw, pitch: past.pitch } : undefined;
  }

  /** The main tail's bones, root to tip (empty without a tail). */
  private get tailBones(): readonly number[] {
    const tails = this.compiled.rig.tails;
    return (tails.find((t) => t.id === 'tail') ?? tails[0])?.bones ?? [];
  }

  private makeSpring(bones: readonly number[], stiffness: number, swish: boolean): Spring {
    const points = [
      ...bones.map((b) => (this.pose.worldPos[b] as Vector3).clone()),
      this.pose.tail(bones.at(-1) as number),
    ];
    const first = bones[0] as number;
    const kind = this.compiled.rig.tentacles.some((t) => t.bones[0] === first)
      ? 'tentacle'
      : this.compiled.bones.sections[first] === 'part'
        ? 'part'
        : 'tail';
    const radii = this.compiled.bones.radii;
    return {
      bones,
      points,
      previous: points.map((p) => p.clone()),
      lengths: bones.map((b) => this.pose.lengths[b] as number),
      stiffness,
      swish,
      kind,
      // Tails keep clear of the floor only in the air (10.4), by the thicker bone at each
      // joint; on land they are as before. Tentacles lie on it.
      clearance: points.map((_, i) => {
        const at = radii[bones[Math.min(i, bones.length - 1)] as number] ?? 0;
        const before = radii[bones[Math.max(0, i - 1)] as number] ?? 0;
        return kind === 'tail' ? Math.max(at, before) : 0.8 * at;
      }),
      floors: new Float64Array(points.length * 5).fill(Number.NaN),
    };
  }

  /** Which side of the body a point is on: 1 left, -1 right (0 without a point). */
  private sideOf(point: Vector3 | null | undefined): number {
    if (!point) return 0;
    const x = point.x - this.position.x;
    const z = point.z - this.position.z;
    return x * Math.cos(this.heading) - z * Math.sin(this.heading) >= 0 ? 1 : -1;
  }

  /**
   * Puts the creature at (x, z) facing `heading`, standing: no target, feet planted around it,
   * tail at rest. Use it to spawn or teleport a creature, or to take one back from a baked
   * animation. With `flying`, a creature with wings starts in the air; `y` is the height (its
   * origin's, m) a flyer starts at, or a swimmer put in water.
   */
  place(
    x: number,
    z: number,
    heading = this.heading,
    ground: Ground = FLAT,
    water?: Water,
    options: { flying?: boolean; y?: number } = {},
  ): void {
    this.position.set(x, ground(x, z).height, z);
    this.medium = 'land';
    this.air = null;
    Object.assign(this.strokes, { amplitude: 0, glide: 0, hover: 0, hold: 0, raise: 0 });
    this.airCrouch = 0;
    this.landedAt = undefined;
    this.death = null;
    this.staggerT = 0;
    this.steadying = false;
    this.push.set(0, 0, 0);
    this.sway.set(0, 0, 0);
    this.swayV.set(0, 0, 0);
    Object.assign(this.flinch, { x: 0, y: 0, vx: 0, vy: 0, focus: 0 });
    this.targetY = undefined;
    this.heading = heading;
    this.speed = 0;
    this.yawRate = 0;
    this.target = null;
    this.desiredSpeed = 0;
    this.driveHeading = undefined;
    this.trail.length = 0;
    const turn = new Quaternion().setFromAxisAngle(UP, heading);
    for (const leg of this.legs) {
      const foot = leg.neutral.clone().applyQuaternion(turn).add(this.position);
      foot.y = ground(foot.x, foot.z).height + leg.footLift;
      leg.planted.copy(foot);
      leg.liftoff.copy(foot);
      leg.target.copy(foot);
      leg.swinging = false;
      leg.armed = true;
      if (leg.roll) {
        leg.roll.heel = 0;
        plantToes(leg.roll, foot, leg.footLift, heading, (x, z) => ground(x, z).height);
      }
    }
    // Put in water deep enough to swim, a swimmer starts afloat: at the height `y` asks for,
    // else at the surface if it also walks, else halfway down.
    const depth = this.depthAt(ground, water, x, z);
    if (this.swims && depth > this.swimDepth) {
      this.enterMedium('water', ground);
      this.events.length = 0;
      const bed = ground(x, z).height;
      this.swimY =
        options.y !== undefined && !options.flying
          ? Math.max(bed, Math.min(bed + depth, options.y + this.restBodyY))
          : this.walks
            ? bed + depth
            : bed + depth / 2;
      this.stepSwim(0, bed, bed + depth);
    }
    // Already flying (docs/design/10.4-flight.md): at cruise, at the height asked for or its own.
    if (options.flying && this.canFly) {
      const fl = this.flightNumbers as FlightNumbers;
      const floor = airFloor(ground, water)(x, z).height;
      this.position.y = options.y ?? floor + fl.height;
      this.air = { stage: 'flight', t: 0, vy: 0, bank: 0, role: 'flapping', landing: null };
      this.airPivot = 1;
      this.medium = 'air';
      this.speed = fl.cruise;
      this.spread = 1;
      this.loiter.set(x, 0, z);
      this.setAirGait('flapping');
      this.strokes.amplitude = 1;
      this.strokes.hold = 1;
      this.events.length = 0;
      this.applyPose(ground);
      this.tuckLegs();
    }
    this.applyPose(ground);
    for (const [i, spring] of this.springs.entries())
      this.springs[i] = this.makeSpring(spring.bones, spring.stiffness, spring.swish);
  }

  /**
   * Walk toward a point on the ground. `speed` in m/s (default: the temperament's pace). A `y`
   * is the height a swimmer dives or rises to (10.3); walkers ignore it.
   */
  moveTo(
    target: { x: number; y?: number; z: number } | null,
    options: { speed?: number } = {},
  ): void {
    // The dead go nowhere.
    if (this.death) return;
    this.targetY = target && typeof target.y === 'number' ? target.y : undefined;
    if (!target) {
      this.target = null;
      this.desiredSpeed = 0;
      // In the air, stopping hovers or circles where it is.
      this.loiter.copy(this.position);
      this.driveHeading = undefined;
      return;
    }
    this.target = new Vector3(target.x, 0, target.z);
    this.desiredSpeed = options.speed ?? this.paceSpeed();
    this.atPace = options.speed === undefined;
  }
  /** Whether `moveTo` was given no speed, so it keeps the pace of the medium it is in. */
  private atPace = false;

  /** Keep moving at a speed and heading (radians), with no target. */
  drive(speed: number, heading?: number): void {
    if (this.death) return;
    this.target = null;
    this.atPace = false;
    this.desiredSpeed = Math.max(0, speed);
    if (heading !== undefined) this.driveHeading = heading;
  }
  private driveHeading: number | undefined;

  stop(): void {
    this.moveTo(null);
  }

  /**
   * Starts an action by id (one of the creature's `motion.actions`), aimed at `target` if given.
   * A running action is replaced. Needs the registry passed to the constructor.
   */
  act(id: string, options: { target?: { x: number; y: number; z: number } | null } = {}): void {
    const def = this.actionDefs.get(id);
    if (this.death && def) return;
    if (!def || def.hooks.ambient) {
      const known = this.motion.actions.map((a) => a.id);
      throw new Error(
        !known.includes(id)
          ? `"${id}" is not one of this creature's actions (${known.join(', ') || 'none'})`
          : def
            ? `"${id}" runs by itself and cannot be started`
            : `no code for action "${id}": pass the module registry to the MotionController`,
      );
    }
    if (this.air && def.hooks.leap)
      throw new Error(`"${id}" leaps from the ground; it cannot start while flying`);
    if (this.current)
      this.events.push({ type: 'action-end', time: this.time, action: this.current.id });
    const target = options.target
      ? new Vector3(options.target.x, options.target.y, options.target.z)
      : null;
    this.current = {
      id,
      hooks: def.hooks,
      params: def.params,
      target,
      start: this.time,
      duration: Math.max(0.05, def.hooks.duration(def.params) * this.timeScale),
      // A leap's events wait for its arc (`planLeap`), which needs the ground.
      events: def.hooks.leap
        ? []
        : [...(def.hooks.events?.(def.params) ?? [])].sort((a, b) => a.at - b.at),
      rng: createRng(this.compiled.seed).stream(`act.${id}.${this.actionCount++}`),
      progress: -1,
    };
    this.events.push({ type: 'action-start', time: this.time, action: id });
  }

  /** How tall it stands at rest (m): a `moveTo` higher than 1.5 times that above the ground flies. */
  get standingHeight(): number {
    return Math.max(0.05, this.compiled.bounds.max[1] ?? 0);
  }

  /** Whether it can fly: it has a wing and an air gait. */
  get canFly(): boolean {
    return this.flightNumbers !== undefined && this.gaitsIn('air').length > 0;
  }

  /** In the air: from the takeoff crouch until it touches down (docs/design/10.4-flight.md). */
  get flying(): boolean {
    return this.air !== null;
  }

  /**
   * Where a flight is: `crouch` and `launch` (taking off), `flight`, then `flare` (landing);
   * null on the ground or in the water.
   */
  get flightStage(): 'crouch' | 'launch' | 'flight' | 'flare' | 'descend' | null {
    return this.air?.stage ?? null;
  }

  /** Wingbeats per second now (0 when not flying or gliding with the wings still). */
  get wingbeat(): number {
    return this.air && this.strokes.amplitude > 0.05 ? this.strokes.beat : 0;
  }

  /** The body's pitch (radians, nose up positive) and roll (positive rolls to the right). */
  get attitude(): { readonly pitch: number; readonly roll: number } {
    return { pitch: this.pitch, roll: this.roll };
  }

  /**
   * Poses the wings at a point of their beat (0 to 1) without advancing time, the rest of the
   * body as it is: how air cycles are baked at exact phases. Only while flying.
   */
  poseBeat(phase: number, ground: Ground = FLAT): void {
    if (!this.air) return;
    this.strokes.phase = ((phase % 1) + 1) % 1;
    this.phase = this.strokes.phase;
    this.applyPose(ground);
  }

  /**
   * Takes off (or keeps flying) and holds `height` metres above the ground (default: its span
   * clear of it), at `speed` (default its cruise). With nowhere to go it circles, or hovers if it
   * can. Waits for a running action to end. Throws for a creature that cannot fly.
   */
  fly(options: { height?: number; speed?: number } = {}): void {
    if (!this.canFly)
      throw new Error(
        this.flightNumbers
          ? 'it has no air gait: list one in motion.gaits (or leave them out), with the air on, to fly'
          : 'it cannot fly: it has no wings',
      );
    if (this.death) return;
    this.flyHeight = options.height;
    this.flySpeed = options.speed;
    if (this.air) return;
    if (this.current) {
      this.pendingAir = { kind: 'takeoff', ...options };
      return;
    }
    this.startTakeoff();
  }

  /**
   * Lands at `target` on the ground (or water, for a swimmer), or with none at the first clear
   * ground ahead. Waits for a running action to end.
   */
  land(target: { x: number; z: number } | null = null): void {
    if (this.death) return;
    const to = target ? new Vector3(target.x, 0, target.z) : null;
    if (this.current) {
      this.pendingAir = { kind: 'land', to };
      return;
    }
    if (this.air) this.air.landing = { to, plan: null, goRounds: 0 };
  }

  /**
   * A blow (docs/design/10.5-hits-death.md): `direction` is where it pushes, in world space (only
   * its horizontal part counts); `bone` a bone name or index (default the torso's middle);
   * `strength` 0 to 1 (default 0.5). The spine and neck flinch with it; a blow that would carry
   * the body past its feet makes it stagger, stepping to catch itself. Fires `hit` (and
   * `stagger`). Does nothing to a dead creature.
   */
  hit(options: {
    direction: { x: number; y?: number; z: number };
    bone?: string | number;
    strength?: number;
  }): void {
    if (this.death) return;
    const strength = Math.max(0, Math.min(1, options.strength ?? 0.5));
    const rig = this.compiled.rig;
    const names = this.compiled.bones.names;
    const middle = rig.spine[Math.floor(rig.spine.length / 2)] as number;
    const bone =
      typeof options.bone === 'number'
        ? options.bone
        : options.bone !== undefined
          ? names.indexOf(options.bone)
          : middle;
    if (bone < 0 || bone >= names.length)
      throw new Error(`no bone "${options.bone}"; hit capsules name the bones a blow can land on`);
    const push = new Vector3(options.direction.x, 0, options.direction.z);
    if (push.lengthSq() < 1e-12) push.set(-Math.sin(this.heading), 0, -Math.cos(this.heading));
    push.normalize();
    // In the body's frame: toward its left (+) and forward (+).
    const left = push.x * Math.cos(this.heading) - push.z * Math.sin(this.heading);
    const forward = push.x * Math.sin(this.heading) + push.z * Math.cos(this.heading);
    // The flinch: a kick to a damped spring, peaking near 0.35 rad × strength.
    const omega = (2 * Math.PI * 2.5) / this.timeScale;
    const kick = 0.5 * omega * strength;
    this.flinch.vx += left * kick;
    this.flinch.vy -= forward * kick;
    const section = this.compiled.bones.sections[bone];
    this.flinch.focus = section === 'head' || section === 'jaw' ? 1 : section === 'neck' ? 0.7 : 0;
    const at = this.pose.worldPos[bone] as Vector3;
    this.events.push({
      type: 'hit',
      time: this.time,
      bone: names[bone] as string,
      position: [at.x, at.y, at.z],
    });
    // The push: a velocity that friction stops over `tau`.
    const h = this.hipHeight;
    // Light blows barely push; a full-strength one knocks almost anything off its feet.
    const v0 = (0.4 * strength + 0.6 * strength * strength) * Math.sqrt(G * h);
    const tau = 0.35 * this.timeScale;
    if (this.air || this.medium === 'water' || this.legs.length === 0) {
      // Flyers, swimmers and legless bodies sway with it.
      this.swayV.addScaledVector(push, v0);
      return;
    }
    if (v0 * tau > this.supportMargin(push)) {
      this.push.addScaledVector(push, v0);
      this.staggerT = 2.5 * tau;
      this.steadying = true;
      this.events.push({
        type: 'stagger',
        time: this.time,
        position: [this.position.x, this.position.y, this.position.z],
      });
    } else this.swayV.addScaledVector(push, v0);
  }

  /**
   * How far the body's middle may move along `direction` (world, horizontal unit) before it is
   * past its planted feet: the support polygon, each foot a disc of 0.12 hip heights.
   */
  private supportMargin(direction: Vector3): number {
    const h = this.hipHeight;
    const feet = this.legs.filter((l) => !l.swinging).map((l) => l.planted);
    const centre = this.pose.worldPos[
      this.compiled.rig.spine[Math.floor(this.compiled.rig.spine.length / 2)] as number
    ] as Vector3;
    if (feet.length === 0) return 0;
    const r = 0.12 * h;
    const pts = feet.map((f) => [f.x, f.z] as [number, number]);
    const hull = convexHull2(pts);
    for (let t = 0; t <= 3 * h; t += 0.02 * h) {
      const x = centre.x + direction.x * t;
      const z = centre.z + direction.z * t;
      if (distanceToHull(hull, x, z) > r) return t;
    }
    return 3 * h;
  }

  /**
   * Dies (docs/design/10.5-hits-death.md): it stops what it was doing, fires `death`, and over
   * about a second collapses onto the ground, falling away from `direction` (where the killing
   * blow pushes; default from its right), never into the ground. Then it is `dead` until
   * `place` stands it up again. In the air it falls first; in the water it sinks to the bed.
   */
  die(options: { direction?: { x: number; y?: number; z: number } } = {}): void {
    if (this.death) return;
    if (this.current) {
      this.events.push({ type: 'action-end', time: this.time, action: this.current.id });
      this.current = null;
    }
    this.pendingAir = null;
    this.target = null;
    this.targetY = undefined;
    this.desiredSpeed = 0;
    this.driveHeading = undefined;
    this.lockedGait = undefined;
    this.goals = {};
    this.wingGoal = 0;
    this.hulls ??= boneHulls(this.compiled, this.pose);
    const rig = this.compiled.rig;
    const pose = this.pose;
    const push = new Vector3(options.direction?.x ?? 0, 0, options.direction?.z ?? 0);
    // Default: hit from its right, so pushed toward its left.
    if (push.lengthSq() < 1e-12) push.set(Math.cos(this.heading), 0, -Math.sin(this.heading));
    push.normalize();
    // The push in the root's frame (heading 0 faces +Z, its left is +X).
    const local = new Vector3(
      push.x * Math.cos(this.heading) - push.z * Math.sin(this.heading),
      0,
      push.x * Math.sin(this.heading) + push.z * Math.cos(this.heading),
    );
    // Which way it falls, and how far.
    const spine0 = rig.spine[0] as number;
    const legs = this.legs.length;
    const upright = legs > 0 && legs <= 2 && this.torsoPitch() >= Math.PI / 4;
    let fall: Vector3;
    let angle: number;
    if (legs === 0) {
      // Legless: onto its side if it is taller than wide, else limp where it lies.
      fall = new Vector3(local.x >= 0 ? 1 : -1, 0, 0);
      angle = this.taller() ? Math.PI / 2 : 0;
    } else if (upright) {
      fall = local.clone();
      angle = (80 * Math.PI) / 180;
    } else {
      // Onto the side away from the blow (from straight ahead or behind: onto its right).
      fall = new Vector3(Math.abs(local.x) > 0.2 ? Math.sign(local.x) : -1, 0, 0);
      angle = rig.posture === 'sprawl' ? (20 * Math.PI) / 180 : Math.PI / 2;
    }
    const axis = new Vector3().crossVectors(UP, fall).normalize();
    // It topples over the edge of its feet on that side (or its body's, without legs).
    let edge = 0;
    for (const leg of this.legs)
      edge = Math.max(edge, leg.neutral.x * fall.x + leg.neutral.z * fall.z);
    if (legs === 0) edge = 0.5 * this.halfWidth();
    const restZ = (pose.restPos[spine0] as Vector3).z;
    const pivot = new Vector3(fall.x * edge, 0, restZ + fall.z * edge);
    // Above the floor: a flyer falls, a swimmer sinks.
    const sinks = this.medium === 'water';
    const floorY = this.groundNow(this.position.x, this.position.z).height;
    const height = Math.max(0, this.position.y - floorY);
    this.air = null;
    this.medium = 'land';
    this.death = {
      t: 0,
      duration: 1.1 * this.timeScale,
      axis,
      angle,
      pivot,
      feet: this.legs.map((l) => l.planted.clone()),
      lift: 0,
      droop: 0,
      droops: [],
      height,
      vy: 0,
      sinks,
      sink: height / 3,
      pitch: this.pitch,
      roll: this.roll,
      down: 0,
      still: 0,
      rest: false,
    };
    this.position.y = floorY + height;
    this.events.push({
      type: 'death',
      time: this.time,
      position: [this.position.x, this.position.y, this.position.z],
    });
  }

  /** Dying or dead (from `die` until `place`). */
  get dead(): boolean {
    return this.death !== null;
  }

  /** Dying: from `die` until the body comes to rest. */
  get dying(): boolean {
    return this.death !== null && !this.death.rest;
  }

  /** Knocked off balance by a blow, and stepping until its feet have caught up. */
  get staggering(): boolean {
    return this.staggerT > 0 || this.steadying;
  }

  /** The torso's pitch at rest (radians): an upright biped's is high. */
  private torsoPitch(): number {
    const spine = this.compiled.rig.spine;
    const a = this.pose.restWorldPos[spine[0] as number] as Vector3;
    const b = this.pose.restWorldPos[spine.at(-1) as number] as Vector3;
    return Math.atan2(b.y - a.y, Math.max(1e-6, Math.abs(b.z - a.z)));
  }

  /** The body's half-width and half-height at its middle, at rest, from its hull. */
  private middleExtent(): { width: number; height: number } {
    const hulls = this.hulls as BoneHulls;
    const spine = this.compiled.rig.spine;
    const bone = spine[Math.floor(spine.length / 2)] as number;
    const rot = this.pose.restWorldRot[bone] as Quaternion;
    let width = 0;
    let height = 0;
    const from = hulls.ranges[bone * 2] as number;
    const to = hulls.ranges[bone * 2 + 1] as number;
    for (let i = from; i < to; i++) {
      const p = scratch1
        .set(
          hulls.points[i * 3] as number,
          hulls.points[i * 3 + 1] as number,
          hulls.points[i * 3 + 2] as number,
        )
        .applyQuaternion(rot);
      width = Math.max(width, Math.abs(p.x));
      height = Math.max(height, Math.abs(p.y));
    }
    const r = this.compiled.bones.radii[bone] ?? 0.1;
    return { width: width || r, height: height || r };
  }

  private taller(): boolean {
    const { width, height } = this.middleExtent();
    return height > 1.25 * width;
  }

  private halfWidth(): number {
    return this.middleExtent().width;
  }

  /** True for bodies without legs (they slither). */
  get legless(): boolean {
    return this.compiled.rig.posture === 'legless';
  }

  /** The main action running now, if any. */
  get action(): string | null {
    return this.current?.id ?? null;
  }

  /** The running action's progress (0 to 1) and length in seconds, if one is running. */
  get actionState(): { id: string; progress: number; duration: number } | null {
    const c = this.current;
    return c ? { id: c.id, progress: Math.max(0, c.progress), duration: c.duration } : null;
  }

  /** Actions `act` can start (ambient ones such as idle run by themselves). */
  actions(): string[] {
    return [...this.actionDefs].filter(([, d]) => !d.hooks.ambient).map(([id]) => id);
  }

  /** Use only this gait (by id) whatever the speed, or `null` to choose by speed again. */
  lockGait(id: string | null): void {
    this.lockedGait = id ? this.motion.gaits.find((g) => g.id === id) : undefined;
    if (id && !this.lockedGait)
      throw new Error(
        `no gait "${id}" for this creature; it has ${this.motion.gaits.map((g) => g.id).join(', ') || 'none'}`,
      );
    if (this.lockedGait && this.lockedGait !== this.gait) {
      this.gait = this.lockedGait;
      this.retimeLegs();
      this.gaitEvent(this.gait.id);
    }
  }
  private lockedGait: GaitInfo | undefined;

  /**
   * A `gait` event; one chosen again within the same step (on entering the water, the medium's
   * first gait, then the one for its speed) replaces the first.
   */
  private gaitEvent(gait: string): void {
    const last = this.events.at(-1);
    if (last?.type === 'gait' && last.time === this.time) this.events.pop();
    this.events.push({ type: 'gait', time: this.time, gait });
  }

  /** Speed (m/s) at which a gait looks typical: mid-range of its Froude numbers, capped at 1. */
  gaitSpeed(id: string): number {
    const gait = this.motion.gaits.find((g) => g.id === id);
    if (!gait)
      throw new Error(
        `no gait "${id}" for this creature; it has ${this.motion.gaits.map((g) => g.id).join(', ') || 'none'}`,
      );
    // Air gaits fly at cruise; a hover holds still (docs/design/10.4-flight.md).
    if (gait.medium === 'air' && this.flightNumbers)
      return gait.air === 'hovering' ? 0 : this.flightNumbers.cruise;
    const [lo, hi] = gait.froude;
    const fr = gait.natural ?? Math.max(lo + 0.05, Math.min((lo + hi) / 2, 1, hi));
    return Math.sqrt(fr * G * this.hipHeight);
  }

  /** Each leg's foot: where it is and whether it is planted. */
  feet(): { leg: string; planted: boolean; position: Vector3 }[] {
    return this.legs.map((l) => ({ leg: l.rig.id, planted: !l.swinging, position: l.planted }));
  }

  lookAt(point: { x: number; y: number; z: number } | null): void {
    this.lookTarget = point ? new Vector3(point.x, point.y, point.z) : null;
  }

  /**
   * The length its speeds scale with (m): its hip height, or for a legless body 0.12 of its
   * length. Gait speeds are Froude numbers of it.
   */
  get lengthScale(): number {
    return this.hipHeight;
  }

  /**
   * Walking speed for the temperament, from its Froude number; in the water (or for a body that
   * only swims) the natural speed of its slowest swimming gait.
   */
  paceSpeed(): number {
    if (this.medium === 'air' && this.flightNumbers) return this.flightNumbers.cruise;
    const swim = this.gaitsIn('water')[0];
    if (swim && (this.medium === 'water' || !this.walks)) return this.gaitSpeed(swim.id);
    const fr = TEMPERAMENTS[this.motion.temperament].walk;
    return Math.sqrt(fr * G * this.hipHeight);
  }

  /**
   * Fastest speed any of its gaits allows. Gaits that keep a foot down top out at a Froude number
   * of 1.5; a gait with flight (a run, a gallop) goes to the top of its range.
   */
  maxSpeed(): number {
    const land = this.gaitsIn('land');
    const fr = Math.max(...(land.length > 0 ? land : this.motion.gaits).map(gaitTop), 0.5);
    return Math.sqrt(fr * G * this.hipHeight);
  }

  /** Fastest speed in the water (m/s): the top of its swimming gaits, or 0 if it cannot swim. */
  swimSpeed(): number {
    const fr = Math.max(0, ...this.gaitsIn('water').map((g) => g.froude[1]));
    return Math.sqrt(fr * G * this.hipHeight);
  }

  /**
   * Advances by `dt` seconds and poses the skeleton. Returns events since the last update. With
   * `pose: false` a flyer keeps flying without posing (springs and legs wait), for level of
   * detail: the next posed update catches up in place.
   */
  update(
    dt: number,
    input: { ground?: Ground; water?: Water; pose?: boolean } = {},
  ): MotionEvent[] {
    const ground = input.ground ?? FLAT;
    this.groundNow = ground;
    // Dead and at rest: nothing moves.
    if (this.death?.rest) {
      this.time += Math.min(dt, 0.25);
      const events = this.events;
      this.events = [];
      return events;
    }
    this.posing = input.pose !== false || !this.air;
    this.accumulator += Math.min(dt, 0.25);
    let steps = 0;
    while (this.accumulator >= STEP && steps < 30) {
      this.step(STEP, ground, input.water);
      this.accumulator -= STEP;
      steps++;
    }
    if (!this.posing) this.stalePose = true;
    else {
      if (this.stalePose) this.catchUp(ground);
      this.applyPose(ground);
    }
    this.posing = true;
    const events = this.events;
    this.events = [];
    return events;
  }

  /** After steps without posing: legs tucked and springs at rest where the body is now. */
  private catchUp(ground: Ground): void {
    this.stalePose = false;
    this.applyPose(ground);
    if (this.air) this.tuckLegs();
    this.applyPose(ground);
    for (const [i, spring] of this.springs.entries())
      this.springs[i] = this.makeSpring(spring.bones, spring.stiffness, spring.swish);
  }

  private step(dt: number, ground: Ground, water?: Water): void {
    this.time += dt;
    if (this.death) {
      this.stepDeath(dt, ground);
      return;
    }
    this.stepBlows(dt);
    const temperament = TEMPERAMENTS[this.motion.temperament];
    const leaping = this.current;
    if (leaping?.hooks.leap && !leaping.leap) this.planLeap(leaping, ground);
    this.updateGoals();
    if (leaping?.leap && !leaping.leap.landed) {
      this.stepLeap(leaping.leap, dt, ground);
      this.stepParts(dt, ground);
      return;
    }

    // Flight (docs/design/10.4-flight.md): asked for while an action ran, or a height out of
    // reach on foot; in the air it has a step of its own.
    if (this.pendingAir && !this.current) {
      const pending = this.pendingAir;
      this.pendingAir = null;
      if (pending.kind === 'takeoff') this.fly(pending);
      else this.land(pending.to);
    }
    if (
      !this.air &&
      this.target &&
      this.targetY !== undefined &&
      this.medium !== 'water' &&
      this.canFly &&
      this.targetY - ground(this.target.x, this.target.z).height > 1.5 * this.standingHeight
    )
      this.startTakeoff();
    if (this.air) {
      this.stepAir(dt, ground, water);
      if (this.posing) this.stepParts(dt, this.air ? airFloor(ground, water) : ground);
      return;
    }

    // Steering.
    let wantSpeed = this.goals.stop ? 0 : this.desiredSpeed;
    let wantHeading = this.driveHeading ?? this.heading;
    if (this.target) {
      const to = new Vector3(this.target.x - this.position.x, 0, this.target.z - this.position.z);
      const distance = to.length();
      const arrive = Math.max(0.3 * this.hipHeight, 0.05);
      if (distance < arrive) {
        this.target = null;
        wantSpeed = 0;
        this.desiredSpeed = 0;
        this.events.push({
          type: 'arrive',
          time: this.time,
          position: [this.position.x, this.position.y, this.position.z],
        });
      } else {
        wantHeading = Math.atan2(to.x, to.z);
        wantSpeed = Math.min(wantSpeed, distance * 1.5);
      }
    }
    let turn = wrapAngle(wantHeading - this.heading);
    const maxTurn = ((temperament.turn * Math.PI) / 180) * dt;
    // Slow down for sharp turns.
    if (Math.abs(turn) > 0.6) wantSpeed *= 0.4;
    turn = Math.max(-maxTurn, Math.min(maxTurn, turn));
    this.heading = wrapAngle(this.heading + turn);
    this.yawRate = damp(this.yawRate, turn / dt, 8, dt);
    const accel = 2.5 * Math.sqrt(G * this.hipHeight);
    this.speed += Math.max(-accel * dt, Math.min(accel * dt, wantSpeed - this.speed));
    if (this.speed < 1e-4 && wantSpeed === 0) this.speed = 0;
    const forward = new Vector3(Math.sin(this.heading), 0, Math.cos(this.heading));
    // A swimmer that cannot walk turns back rather than beach itself.
    if (this.medium === 'water' && !this.walks) {
      const x = this.position.x + forward.x * this.speed * dt;
      const z = this.position.z + forward.z * this.speed * dt;
      if (this.depthAt(ground, water, x, z) <= this.swimDepth) this.speed = 0;
    }
    this.position.addScaledVector(forward, this.speed * dt);

    // Water deep enough to swim in, for a creature that swims, takes it off its feet (10.3).
    const depth = this.depthAt(ground, water, this.position.x, this.position.z);
    const swimming = this.swims && depth > this.swimDepth;
    if (swimming !== (this.medium === 'water'))
      this.enterMedium(swimming ? 'water' : 'land', ground);
    if (this.medium === 'water') {
      const bed = ground(this.position.x, this.position.z).height;
      this.stepSwim(dt, bed, bed + depth);
      this.stepParts(dt, ground);
      return;
    }
    this.position.y = ground(this.position.x, this.position.z).height;

    // Gait from speed (Froude number), switching walk to trot near 0.5.
    const froude = (this.speed * this.speed) / (G * this.hipHeight);
    this.chooseGait(
      this.gaitsIn('land').filter((g) => !g.spine),
      froude,
    );

    if (this.compiled.rig.posture === 'legless') {
      this.stepSlither(dt, ground);
    } else {
      this.stepLegs(dt, ground, froude);
    }
    this.stepParts(dt, ground);
  }

  /** Springs, grips, flares and wings, which run whatever the body is doing. */
  private stepParts(dt: number, ground: Ground): void {
    this.stepSprings(dt, ground);
    // Landed: the body dips to absorb it and rises again, and what is left of the wingbeat
    // dies away over about 0.3 s.
    if (!this.air && this.landedAt !== undefined) {
      const t = (this.time - this.landedAt) / (0.4 * this.timeScale);
      this.airCrouch = t < 1 ? 0.15 * this.hipHeight * Math.sin(Math.PI * t) : 0;
      if (t >= 1) this.landedAt = undefined;
    }
    if (!this.air) {
      const st = this.strokes;
      const k = Math.min(1, dt / 0.15);
      st.amplitude -= st.amplitude * k;
      st.raise -= st.raise * k;
      st.hold -= st.hold * k;
      st.down = 0;
      if (st.amplitude + st.raise + st.hold < 1e-3) st.amplitude = st.raise = st.hold = 0;
    }
    // Pincers snap shut fast (about 0.08 s); with `nearest`, only on the target's side.
    const grip = Math.max(-1, Math.min(1, this.goals.grip ?? 0));
    const gripSide = this.goals.nearest ? this.sideOf(this.goals.look) : 0;
    this.gripLeft = damp(this.gripLeft, gripSide < 0 ? 0 : grip, 40, dt);
    this.gripRight = damp(this.gripRight, gripSide > 0 ? 0 : grip, 40, dt);
    // Frills and hoods open in about 0.15 s.
    this.flare = damp(this.flare, Math.max(0, Math.min(1, this.goals.flare ?? 0)), 7, dt);
    // Wings spread and fold over about 0.4 s (longer on big creatures).
    if (this.compiled.rig.wings.length > 0) {
      // In flight the wings stay spread, whatever an action asks (decision 12).
      const want = this.air ? 1 : Math.max(0, Math.min(1, this.goals.wings ?? this.wingGoal));
      this.spread = damp(this.spread, want, 7.5 / this.timeScale, dt);
    }
  }

  /**
   * The gait among `gaits` whose speed range holds this Froude number, preferring the one that
   * starts fastest; the legs ease into it over a stride or two (`easeOffsets`).
   */
  private chooseGait(gaits: readonly GaitInfo[], froude: number): void {
    if (gaits.length < 2 || this.lockedGait) return;
    const best = gaits.reduce((a, g) =>
      froude >= g.froude[0] &&
      froude <= g.froude[1] &&
      (a.froude[0] > froude || a.froude[1] < froude || g.froude[0] > a.froude[0])
        ? g
        : a,
    );
    if (best === this.gait) return;
    this.gait = best;
    this.gaitEvent(best.id);
  }

  /** Its gaits for a medium, slowest first. */
  private gaitsIn(medium: 'land' | 'water' | 'air'): GaitInfo[] {
    return this.motion.gaits.filter((g) => (g.medium ?? 'land') === medium);
  }

  /** Whether it has a gait for water, and one for land. */
  private get swims(): boolean {
    return this.motion.gaits.some((g) => g.medium === 'water');
  }
  private get walks(): boolean {
    return this.motion.gaits.some((g) => (g.medium ?? 'land') === 'land');
  }

  /** Depth of water at (x, z): its surface above the ground there, 0 where there is none. */
  private depthAt(ground: Ground, water: Water | undefined, x: number, z: number): number {
    const w = water?.(x, z);
    return w ? Math.max(0, w.surface - ground(x, z).height) : 0;
  }

  /** The torso's thickest radius (m). */
  private get girth(): number {
    let r = 0;
    for (const b of this.compiled.rig.spine) r = Math.max(r, this.compiled.bones.radii[b] ?? 0);
    return Math.max(r, 0.02 * this.compiled.scale);
  }

  /**
   * Water this deep takes it off its feet: about its hip height for a walker, a little more than
   * its girth for a legless body (docs/design/10.3-swimming.md).
   */
  private get swimDepth(): number {
    return this.compiled.rig.posture === 'legless' ? 1.6 * this.girth : 0.85 * this.hipHeight;
  }

  /** Into the water or out of it: the gait for that medium, feet down again on land. */
  private enterMedium(medium: 'land' | 'water', ground: Ground, fromAir = false): void {
    this.medium = medium;
    this.events.push({ type: 'medium', time: this.time, medium });
    // Heading somewhere at its own pace: the pace of the new medium.
    if (this.atPace && this.target) this.desiredSpeed = this.paceSpeed();
    const gaits = this.gaitsIn(medium);
    const gait = (medium === 'land' ? gaits.find((g) => !g.spine) : undefined) ?? gaits[0];
    if (gait && !this.lockedGait && gait !== this.gait) {
      this.gait = gait;
      this.retimeLegs();
      this.gaitEvent(gait.id);
    }
    this.trail.length = 0;
    if (medium === 'water') {
      this.swimY = this.position.y + this.restBodyY;
      this.airY = 0;
      return;
    }
    // Out onto land: every foot finds the ground under its hip. Coming down from the air, the
    // body eases level instead of snapping.
    this.position.y = ground(this.position.x, this.position.z).height;
    if (!fromAir) this.pitch = 0;
    for (const leg of this.legs) {
      const foot = this.neutralAt(leg, this.heading, leg.planted);
      foot.y = ground(foot.x, foot.z).height + leg.footLift;
      leg.liftoff.copy(foot);
      leg.target.copy(foot);
      leg.swinging = false;
      leg.armed = true;
    }
  }

  /**
   * Swimming (docs/design/10.3-swimming.md). The body holds a height in the water: at the surface
   * for paddlers and for walkers not asked to dive, else where it is or where `moveTo` asks,
   * pitching toward it, never out of the water or into the bed. A wave runs down the body into
   * the tail at a frequency from a Strouhal number of 0.3; legs paddle or trail; fins beat.
   */
  private stepSwim(dt: number, bed: number, surface: number): void {
    this.chooseGait(this.gaitsIn('water'), this.froudeNow());
    const gait = this.gait;
    const style = gait?.swim ?? 'body';
    const h = this.hipHeight;
    const r = this.girth;
    // Paddlers and walkers float with the back awash; divers keep under.
    const floats = style === 'legs' || (this.walks && this.targetY === undefined);
    const top = surface - (floats ? 0.4 : 1.3) * r;
    // Off the bed by the body's girth (and legs), and by how far a pitched snout or tail dips
    // below the middle of the body.
    const [ahead, behind] = this.reaches();
    const dip = Math.max(0, -Math.sin(this.pitch) * ahead, Math.sin(this.pitch) * behind);
    const bottom = bed + 1.3 * r + (this.legs.length > 0 ? 0.6 * h : 0) + dip;
    const asked = this.targetY !== undefined ? this.targetY + this.restBodyY : undefined;
    let want = asked ?? (floats ? top : this.swimY);
    want = bottom > top ? top : Math.max(bottom, Math.min(top, want));
    // Climb or dive at up to about 35° to the travel, easing out over the last stretch.
    const gap = want - this.swimY;
    const climb = Math.min(
      Math.max(0.15 * Math.sqrt(G * h), this.speed * Math.tan(0.6)),
      2 * Math.abs(gap),
    );
    const dy = Math.max(-climb * dt, Math.min(climb * dt, gap));
    this.swimY += dy;
    this.position.y = this.swimY - this.restBodyY;
    this.bodyY = damp(this.bodyY, this.restBodyY, 8, dt);
    this.pitch = damp(
      this.pitch,
      dt > 0 ? Math.atan2(dy / dt, Math.max(this.speed, 0.5)) : 0,
      6,
      dt,
    );
    this.roll = damp(this.roll, Math.max(-0.35, Math.min(0.35, -this.yawRate * 0.15)), 6, dt);
    this.bend = damp(this.bend, Math.max(-0.5, Math.min(0.5, this.yawRate * 0.25)), 6, dt);
    this.lean = damp(this.lean, 0, 4, dt);
    this.airY = 0;

    // Tail beat: Strouhal St = f·A/U ≈ 0.3, with A the tail's sweep; a slow beat at rest.
    const length = this.spineLength();
    const sweep = Math.max(0.05, gait?.amplitude ?? 0.2) * length;
    const frequency = Math.max(0.4 / this.timeScale, (0.3 * this.speed) / sweep);
    this.phase = (this.phase + frequency * dt) % 1;
    const effort = Math.min(1, 0.35 + this.speed / Math.max(1e-3, this.gaitSpeedFor(gait)));
    // The tail swings at its root far enough for its tip to cover the sweep (side to side), a
    // third as far when legs or fins drive.
    const tailLength = this.tailBones.reduce((sum, b) => sum + (this.pose.lengths[b] as number), 0);
    const swing = Math.asin(Math.min(0.7, (0.5 * sweep) / Math.max(1e-3, tailLength)));
    this.swimSwish =
      (style === 'body' ? 1 : 0.35) * swing * effort * Math.sin(this.phase * Math.PI * 2);
    this.finBeat = (style === 'fins' ? 0.7 : 0.12) * effort * Math.sin(this.phase * Math.PI * 2);

    // Legs paddle in circles under the hips, or trail along the body.
    for (const leg of this.legs) {
      const foot = this.neutralAt(leg, this.heading, leg.planted);
      if (style === 'legs') {
        const a = (this.phase - leg.offset) * Math.PI * 2;
        foot.addScaledVector(
          scratch3.set(Math.sin(this.heading), 0, Math.cos(this.heading)),
          Math.cos(a) * 0.3 * h,
        );
        foot.y += (0.25 + 0.15 * Math.sin(a)) * h;
      } else {
        foot.addScaledVector(
          scratch3.set(Math.sin(this.heading), 0, Math.cos(this.heading)),
          -0.35 * h,
        );
        foot.y += 0.45 * h;
      }
      leg.target.copy(foot);
      leg.swinging = true;
      leg.armed = false;
    }
  }

  // --- Hits and death (docs/design/10.5-hits-death.md) -------------------------------------

  /** A blow's flinch springs back, its sway settles, and a stagger slides to a stop. */
  private stepBlows(dt: number): void {
    const f = this.flinch;
    if (f.x !== 0 || f.y !== 0 || f.vx !== 0 || f.vy !== 0) {
      const omega = (2 * Math.PI * 2.5) / this.timeScale;
      f.vx += (-omega * omega * f.x - 2 * 0.45 * omega * f.vx) * dt;
      f.vy += (-omega * omega * f.y - 2 * 0.45 * omega * f.vy) * dt;
      f.x += f.vx * dt;
      f.y += f.vy * dt;
      if (Math.abs(f.x) + Math.abs(f.y) + Math.abs(f.vx) + Math.abs(f.vy) < 1e-5)
        Object.assign(f, { x: 0, y: 0, vx: 0, vy: 0 });
    }
    if (this.sway.lengthSq() + this.swayV.lengthSq() > 0) {
      const omega = (2 * Math.PI * 1.2) / this.timeScale;
      this.swayV
        .addScaledVector(this.sway, -omega * omega * dt)
        .addScaledVector(this.swayV, -2 * 0.6 * omega * dt);
      this.sway.addScaledVector(this.swayV, dt);
      if (this.sway.lengthSq() + this.swayV.lengthSq() < 1e-10) {
        this.sway.set(0, 0, 0);
        this.swayV.set(0, 0, 0);
      }
    }
    if (this.push.lengthSq() > 0) {
      this.position.addScaledVector(this.push, dt);
      this.push.multiplyScalar(Math.exp(-dt / (0.35 * this.timeScale)));
      if (this.push.lengthSq() < 1e-6) this.push.set(0, 0, 0);
    }
    if (this.staggerT > 0) this.staggerT = Math.max(0, this.staggerT - dt);
  }

  /** The collapse: falling (or sinking) to the floor, sliding to a stop, springs going limp. */
  private stepDeath(dt: number, ground: Ground): void {
    const d = this.death as DeathState;
    d.t += dt;
    const u = Math.min(1, d.t / d.duration);
    if (d.height > 0) {
      d.vy = d.sinks ? -Math.max(0.4, d.sink) : d.vy - G * dt;
      d.height = Math.max(0, d.height + d.vy * dt);
    }
    this.speed = damp(this.speed, 0, 4, dt);
    this.position.x += Math.sin(this.heading) * this.speed * dt;
    this.position.z += Math.cos(this.heading) * this.speed * dt;
    if (this.push.lengthSq() > 0) {
      this.position.addScaledVector(this.push, dt);
      this.push.multiplyScalar(Math.exp(-dt / (0.35 * this.timeScale)));
      if (this.push.lengthSq() < 1e-6) this.push.set(0, 0, 0);
    }
    this.position.y = ground(this.position.x, this.position.z).height + d.height;
    // The necks droop until the heads rest on the ground (the pose stops them there).
    d.droop = Math.min(1.4, 1.5 * d.t);
    this.goals = { jaw: 0.25 * u, blink: Math.min(1, 2 * u) };
    this.wingGoal = 0;
    this.yawRate = 0;
    this.stepParts(dt, ground);
    // At rest once the collapse is over, it is on the floor, nothing slides and its springs
    // have stopped (slower than 1% of its size a second for 0.3 s), or 4 s after it began.
    let fastest = this.speed + this.push.length() + Math.abs(this.spread - this.wingGoal);
    for (const spring of this.springs)
      for (let i = 1; i < spring.points.length; i++)
        fastest = Math.max(
          fastest,
          (spring.points[i] as Vector3).distanceTo(spring.previous[i] as Vector3) / dt,
        );
    if (d.height > 0) d.down = d.t;
    const settled = d.t > d.duration + 0.2 && d.height === 0;
    d.still = settled && fastest < 0.01 * this.compiled.scale ? d.still + dt : 0;
    if (d.height === 0 && (d.still >= 0.3 || d.t - d.down > d.duration + 4)) d.rest = true;
  }

  /**
   * Poses a death: posed, its hulls measured against the ground, and lifted by however deep they
   * go, again until it rests on the ground (decision 6); then the necks droop onto it.
   */
  private applyDeathPose(ground: Ground): void {
    const d = this.death as DeathState;
    if (this.deathGround.length !== this.pose.count * 3)
      this.deathGround = new Float64Array(this.pose.count * 3);
    this.deathGround.fill(Number.NaN);
    let lift = d.lift;
    for (let pass = 0; pass < 4; pass++) {
      this.poseDeath(ground, lift);
      const depth = this.deathDepth(ground, false);
      const next = Math.max(0, lift + depth);
      if (Math.abs(next - lift) < 2e-4 * this.compiled.scale) {
        if (next !== lift) {
          lift = next;
          this.poseDeath(ground, lift);
        }
        break;
      }
      lift = next;
      if (pass === 3) this.poseDeath(ground, lift);
    }
    this.settleLimbs(ground);
    // Heads that cannot rest on the ground however their necks droop lift the body.
    let residual = this.droopNecks(ground);
    for (let pass = 0; pass < 2 && residual > 2e-4 * this.compiled.scale; pass++) {
      lift += residual;
      this.poseDeath(ground, lift);
      this.settleLimbs(ground);
      residual = this.droopNecks(ground);
    }
    d.lift = lift;
  }

  /** The dying body at a lift above where its fall puts it (before the necks droop). */
  private poseDeath(ground: Ground, lift: number): void {
    const d = this.death as DeathState;
    const pose = this.pose;
    const rig = this.compiled.rig;
    const u = Math.min(1, d.t / d.duration);
    // The fall accelerates, from a fifth of the way in.
    const v = Math.max(0, Math.min(1, (u - 0.15) / 0.7));
    const angle = d.angle * v * v;
    // The fall, from the attitude it died in (a flyer's bank fades out), onto the ground's slope
    // where it comes to lie.
    const fade = 1 - smooth(d.t / 0.4);
    const fall = scratchQ3
      .setFromEuler(
        scratchEuler.set(
          -this.slopeAlong(ground) * v * v,
          0,
          this.slopeAcross(ground) * v * v,
          'YXZ',
        ),
      )
      .multiply(scratchQ.setFromAxisAngle(d.axis, angle))
      .multiply(scratchQ2.setFromEuler(scratchEuler.set(-d.pitch * fade, 0, d.roll * fade, 'YXZ')));
    pose.reset();
    const root = rig.root;
    (pose.pos[root] as Vector3).copy(this.position);
    (pose.rot[root] as Quaternion).setFromAxisAngle(UP, this.heading);
    if (rig.posture === 'legless') {
      // On its trail; turned over about the ground under its root, and lifted.
      this.applySlither(ground);
      const yaw = scratchQ4.setFromAxisAngle(UP, this.heading);
      const turn = scratchQ5.copy(yaw).multiply(fall).multiply(scratchQ2.copy(yaw).invert());
      (pose.rot[root] as Quaternion).premultiply(turn);
      // It sinks onto the ground, as far as the ground lets it.
      (pose.pos[root] as Vector3).y += lift - this.restBodyY * smooth(u / 0.6);
      pose.solve();
      this.applySprings(this.tailBones);
    } else {
      const spine0 = rig.spine[0] as number;
      // The legs buckle: the body sinks while it topples, as far as the ground lets it.
      const sink = this.restBodyY * (0.35 * smooth(u / 0.4) + 1.2 * v * v);
      const at = scratch1.set(0, this.restBodyY - sink, (pose.restPos[spine0] as Vector3).z);
      at.sub(d.pivot).applyQuaternion(fall).add(d.pivot);
      at.y += lift;
      (pose.pos[spine0] as Vector3).copy(at);
      (pose.rot[spine0] as Quaternion).premultiply(fall);
      this.solvePose();
      // Feet let go of the ground and fall with the body, stopping on the ground.
      const letGo = smooth((u - 0.05) / 0.45);
      const body = scratchQ4
        .copy(pose.worldRot[spine0] as Quaternion)
        .multiply(scratchQ5.copy(pose.restWorldRot[spine0] as Quaternion).invert());
      const down = scratch2.set(0, -1, 0).applyQuaternion(body).multiplyScalar(0.55);
      down.y -= 0.45;
      down.normalize();
      for (const [k, leg] of this.legs.entries()) {
        const first = leg.rig.bones[0] as number;
        const hip = pose.worldPos[first] as Vector3;
        let length = 0;
        for (const b of leg.rig.bones) length += pose.lengths[b] as number;
        const relaxed = scratch3.copy(hip).addScaledVector(down, 0.8 * length);
        const foot = leg.planted.copy(d.feet[k] as Vector3).lerp(relaxed, letGo);
        foot.y = Math.max(foot.y, ground(foot.x, foot.z).height + leg.footLift);
        leg.swinging = false;
      }
      this.poseLegs(true);
      this.applySprings();
    }
    this.applyHelpers();
    this.solvePose();
    this.applyWings();
    this.applyJaw();
  }

  /** Legs and fins that would go into the ground, once the body rests on it, bend up off it. */
  private settleLimbs(ground: Ground): void {
    const pose = this.pose;
    // Legs, then fins and flippers, bend up off the ground bone by bone from the hip or root:
    // each bone that would go into it swings up about its joint until it lies on it.
    // A joint's helper (a knee's) swings the bone above it up.
    const sets = this.deathSets();
    const chains = [...sets.legs, ...sets.fins];
    const upper = new Map(this.compiled.rig.helpers.map(([helper, above]) => [helper, above]));
    for (let pass = 0; pass < 3; pass++) {
      for (const chain of chains)
        for (const b of chain) {
          pose.solveBone(b);
          if (upper.has(b)) continue;
          const depth = this.bonesDepth(ground, [b]);
          if (depth > 0) this.raiseEnd(b, pose.lengths[b] as number, depth);
        }
      this.applyHelpers();
      this.solvePose();
      let moved = false;
      for (const chain of chains)
        for (const b of chain) {
          const above = upper.get(b);
          if (above === undefined) continue;
          const depth = this.bonesDepth(ground, [b]);
          if (depth <= 0) continue;
          this.raiseEnd(above, pose.lengths[above] as number, depth);
          moved = true;
        }
      if (!moved) break;
      this.solvePose();
    }
    this.applyWings();
    this.applyJaw();
  }

  /**
   * How deep the body's hulls go into the ground (m; negative: how far above it the lowest one
   * is). The necks and heads (`heads`) are measured apart, so they can rest on the ground on
   * their own; springs (tails, tentacles) keep their own floors.
   */
  private deathDepth(ground: Ground, heads: boolean): number {
    const sets = this.deathSets();
    return this.bonesDepth(ground, heads ? sets.heads : sets.body);
  }

  /** How deep these bones' hulls go into the ground (m). */
  private bonesDepth(ground: Ground, bones: readonly number[]): number {
    const hulls = this.hulls as BoneHulls;
    const cache = this.deathGround;
    const step = this.floorStep;
    let deepest = -Infinity;
    for (const b of bones) {
      // The ground under each bone, sampled once a frame unless the bone moved far.
      const height = (x: number, z: number) => {
        const k = b * 3;
        if (!(Math.abs(x - (cache[k] as number)) + Math.abs(z - (cache[k + 1] as number)) < step)) {
          cache[k] = x;
          cache[k + 1] = z;
          cache[k + 2] = ground(x, z).height;
        }
        return cache[k + 2] as number;
      };
      deepest = Math.max(deepest, hullDepth(hulls, this.pose, b, height));
    }
    return deepest;
  }
  private deathGround = new Float64Array(0);

  /**
   * Bones the body rests on, bones of the necks and heads, and each leg's bones (with its toes):
   * legs fold out of the way rather than hold the body up. Springs are left out.
   */
  private deathSets(): {
    body: number[];
    heads: number[];
    perHead: number[][];
    legs: number[][];
    fins: number[][];
    subtree: number[][];
  } {
    if (this.deathBones) return this.deathBones;
    const rig = this.compiled.rig;
    const parents = this.pose.parents;
    const sprung = new Set(this.springs.flatMap((s) => s.bones.slice(1)));
    const necks = new Map<number, number>();
    const legless = rig.posture === 'legless';
    rig.heads.forEach((h, i) => {
      // A legless body's neck lies on its trail (unless it reared); only its head is apart.
      const first = legless && !this.rearing ? h.head : (h.neck[0] ?? h.head);
      necks.set(first, i);
    });
    const headOf = (b: number): number => {
      for (let x = b; x >= 0; x = parents[x] ?? -1) {
        const i = necks.get(x);
        if (i !== undefined) return i;
      }
      return -1;
    };
    const isSprung = (b: number): boolean => {
      for (let x = b; x >= 0; x = parents[x] ?? -1) if (sprung.has(x)) return true;
      return false;
    };
    const legOf = new Map<number, number>();
    this.legs.forEach((leg, k) => {
      legOf.set(leg.rig.bones[0] as number, k);
    });
    const whichLeg = (b: number): number => {
      for (let x = b; x >= 0; x = parents[x] ?? -1) {
        const k = legOf.get(x);
        if (k !== undefined) return k;
      }
      return -1;
    };
    const finOf = new Map<number, number>();
    rig.fins.forEach((fin, k) => {
      for (const b of fin.bones) finOf.set(b, k);
    });
    const thighs = new Set(this.legs.map((l) => l.rig.bones[0] as number));
    const sections = this.compiled.bones.sections;
    const body: number[] = [];
    const heads: number[] = [];
    const perHead: number[][] = rig.heads.map(() => []);
    const subtree: number[][] = rig.heads.map(() => []);
    const legs: number[][] = this.legs.map(() => []);
    const fins: number[][] = rig.fins.map(() => []);
    for (let b = 0; b < this.pose.count; b++) {
      const i = headOf(b);
      if (i >= 0) (subtree[i] as number[]).push(b);
      // Membranes ride on their spars; springs keep their own floors.
      if (isSprung(b) || sections[b] === 'station') continue;
      const k = whichLeg(b);
      const f = finOf.get(b);
      // The body rests on its thighs too; the rest of a leg folds out of the way.
      if (thighs.has(b)) body.push(b);
      else if (k >= 0) (legs[k] as number[]).push(b);
      else if (f !== undefined) (fins[f] as number[]).push(b);
      else if (i >= 0) {
        heads.push(b);
        (perHead[i] as number[]).push(b);
      } else body.push(b);
    }
    this.deathBones = { body, heads, perHead, legs, fins, subtree };
    return this.deathBones;
  }
  private deathBones: ReturnType<MotionController['deathSets']> | undefined;

  /**
   * The necks droop toward the ground by the death's droop, as far as the heads stay on it: found
   * by bisection when the full droop would put a head into the ground.
   */
  private droopNecks(ground: Ground): number {
    const d = this.death as DeathState;
    const rig = this.compiled.rig;
    const pose = this.pose;
    // A legless body's neck lies on its trail, unless it reared.
    if (rig.posture === 'legless' && !this.rearing) return 0;
    const sets = this.deathSets().perHead;
    let residual = 0;
    rig.heads.forEach((head, i) => {
      const chain = head.neck.length > 0 ? head.neck : [head.head];
      const saved = chain.map((b) => (pose.rot[b] as Quaternion).clone());
      // Down toward the ground, about the horizontal axis across the neck, as the body lies.
      const from = pose.worldPos[chain[0] as number] as Vector3;
      const to = pose.tail(head.head, scratch3);
      const axis = new Vector3().crossVectors(
        scratch2.subVectors(to, from),
        scratch1.set(0, -1, 0),
      );
      if (axis.lengthSq() < 1e-10) return;
      axis.normalize();
      const bones = sets[i] as number[];
      const below = (this.deathSets().subtree[i] as number[]).filter((b) => !chain.includes(b));
      const sprung = this.springs.filter((sp) => below.includes(sp.bones[0] as number));
      const apply = (droop: number) => {
        chain.forEach((b, k) => {
          (pose.rot[b] as Quaternion).copy(saved[k] as Quaternion);
        });
        pose.solveBone(pose.parents[chain[0] as number] as number);
        for (const b of chain) {
          turnWorld(pose, b, axis, droop / chain.length);
          pose.solveBone(b);
        }
        for (const b of below) pose.solveBone(b);
        // Antennae and other springs on the head follow it down, onto the ground.
        if (sprung.length > 0) {
          this.applySprings(
            this.springs.filter((sp) => !sprung.includes(sp)).map((sp) => sp.bones[0] as number),
          );
          for (const b of below) pose.solveBone(b);
        }
        return this.bonesDepth(ground, bones);
      };
      if (apply(d.droop) <= 0) {
        d.droops[i] = d.droop;
        return;
      }
      // Where it rested last frame, if it still rests there.
      const last = d.droops[i];
      if (
        last !== undefined &&
        last < d.droop &&
        apply(last) <= 0 &&
        apply(Math.min(d.droop, last + 0.01)) > 0
      ) {
        apply(last);
        return;
      }
      // The full droop puts the head into the ground: as far as it rests on it, raising the
      // head if it must; whatever is left, the caller lifts the body by.
      let lo = -0.6;
      const low = apply(lo);
      if (low > 0) {
        residual = Math.max(residual, low);
        return;
      }
      let hi = d.droop;
      for (let k = 0; k < 6; k++) {
        const mid = (lo + hi) / 2;
        if (apply(mid) <= 0) lo = mid;
        else hi = mid;
      }
      d.droops[i] = lo;
      apply(lo);
    });
    this.solvePose();
    this.applyWings();
    this.applyJaw();
    return residual;
  }

  // --- Flight (docs/design/10.4-flight.md) ------------------------------------------------

  /** Starts a takeoff: a crouch with the wings spreading (none for a legless body), then a leap. */
  private startTakeoff(): void {
    this.air = { stage: 'crouch', t: 0, vy: 0, bank: 0, role: 'flapping', landing: null };
    this.airPivot = 1;
    this.loiter.copy(this.position);
    if (this.legless) this.launch();
  }

  /** Every foot leaves the ground: straight up, as fast as reaches 1.5 hip heights. */
  private launch(): void {
    const a = this.air as AirState;
    a.stage = 'launch';
    a.t = 0;
    a.vy = Math.sqrt(2 * G * 1.5 * this.hipHeight);
    this.medium = 'air';
    this.events.push(
      { type: 'takeoff', time: this.time },
      { type: 'medium', time: this.time, medium: 'air' },
    );
    this.setAirGait('flapping');
    for (const leg of this.legs) {
      leg.swinging = true;
      leg.armed = false;
    }
    this.trail.length = 0;
  }

  /** The air gait for a role (`flap`, `glide`, `hover`), falling back to flapping. */
  private setAirGait(role: 'flapping' | 'gliding' | 'hovering'): void {
    const air = this.gaitsIn('air');
    const gait =
      (this.lockedGait?.medium === 'air' ? this.lockedGait : undefined) ??
      air.find((g) => g.air === role) ??
      air.find((g) => g.air === 'flapping') ??
      air[0];
    if (this.air) this.air.role = gait?.air ?? 'flapping';
    if (!gait || gait === this.gait) return;
    this.gait = gait;
    this.gaitEvent(gait.id);
  }

  private canRole(role: 'gliding' | 'hovering'): boolean {
    return this.gaitsIn('air').some((g) => g.air === role);
  }

  /** One step in the air: takeoff, flight, or the landing's flare and touchdown. */
  private stepAir(dt: number, ground: Ground, water: Water | undefined): void {
    const a = this.air as AirState;
    const fl = this.flightNumbers as FlightNumbers;
    const h = this.hipHeight;
    a.t += dt;
    const floor = (x: number, z: number) => {
      const g = ground(x, z).height;
      const w = water?.(x, z);
      return w ? Math.max(g, w.surface) : g;
    };

    if (a.stage === 'crouch') {
      if (this.target) {
        const want = Math.atan2(this.target.x - this.position.x, this.target.z - this.position.z);
        const turn = wrapAngle(want - this.heading) * Math.min(1, 10 * dt);
        this.heading = wrapAngle(this.heading + turn);
        this.yawRate = damp(this.yawRate, turn / dt, 8, dt);
      }
      this.speed = damp(this.speed, 0, 12, dt);
      this.position.y = ground(this.position.x, this.position.z).height;
      this.airCrouch = 0.15 * h * Math.min(1, a.t / (0.2 * this.timeScale));
      this.stepLegs(dt, ground, 0);
      this.strokes.amplitude = 0;
      if (a.t >= 0.25 * this.timeScale && this.spread >= 0.95) this.launch();
      return;
    }
    this.airCrouch = damp(this.airCrouch, 0, 10, dt);

    // Landing: plan it once asked, then fly its approach and flare.
    if (a.landing && !a.landing.plan && a.stage === 'flight') this.planLanding(a, floor);
    if (a.stage === 'flare' || a.stage === 'descend') {
      this.stepTouchdown(a, dt, ground, water, floor);
      return;
    }

    const canHover = this.canRole('hovering');
    const canGlide = this.canRole('gliding');
    const r = turnRadius(fl);
    const forward = scratch3.set(Math.sin(this.heading), 0, Math.cos(this.heading));

    // Where to: the landing's approach line, a target, a heading, or round the loiter point.
    const plan = a.landing?.plan ?? null;
    let goal: Vector3 | null = this.target;
    let wantHeading = this.heading;
    let distance = Number.POSITIVE_INFINITY;
    /** On approach: how far along the line it is from the flare's start (negative before it). */
    let along = 0;
    let across = 0;
    if (plan) {
      // Pure pursuit along the approach line, aiming a turn's length ahead of where it is on it.
      along =
        (this.position.x - plan.flareStart.x) * plan.dir.x +
        (this.position.z - plan.flareStart.z) * plan.dir.z;
      across = Math.abs(
        (this.position.x - plan.flareStart.x) * plan.dir.z -
          (this.position.z - plan.flareStart.z) * plan.dir.x,
      );
      const lookahead = Math.max(2 * r, this.speed * 1.5);
      const at = Math.min(0, along + lookahead);
      goal = scratch4.copy(plan.flareStart).addScaledVector(plan.dir, at);
      wantHeading = Math.atan2(goal.x - this.position.x, goal.z - this.position.z);
      distance = -along;
    } else if (goal) {
      const dx = goal.x - this.position.x;
      const dz = goal.z - this.position.z;
      distance = Math.hypot(dx, dz);
      wantHeading = Math.atan2(dx, dz);
    } else if (this.driveHeading !== undefined && this.desiredSpeed > 0) {
      wantHeading = this.driveHeading;
    } else if (!(canHover && this.flySpeed === undefined)) {
      // Circle the loiter point: aim at a point a little ahead on the circle.
      const rx = this.position.x - this.loiter.x;
      const rz = this.position.z - this.loiter.z;
      // A circle it can follow at the speed it flies and the bank it may use.
      const bankAt = (Math.min(70, this.gait?.bank ?? 40) * Math.PI) / 180;
      const round = Math.max(1.5 * r, (1.2 * this.speed * this.speed) / (G * Math.tan(bankAt)));
      const at = Math.atan2(rx, rz) + 0.6;
      goal = scratch4.set(
        this.loiter.x + Math.sin(at) * round,
        0,
        this.loiter.z + Math.cos(at) * round,
      );
      wantHeading = Math.atan2(goal.x - this.position.x, goal.z - this.position.z);
    }

    // Speed. Landing: slow flight. A target: cruise or as asked, a hoverer slowing to stop at
    // it. A heading: as asked (0 hovers). Nowhere to go: a hoverer hovers, others circle at
    // cruise (or `fly`'s speed). Never under slow flight unless it hovers.
    let want = plan
      ? fl.slow
      : this.target
        ? this.atPace
          ? fl.cruise
          : this.desiredSpeed
        : this.driveHeading !== undefined
          ? this.desiredSpeed
          : canHover && this.flySpeed === undefined
            ? 0
            : (this.flySpeed ?? fl.cruise);
    if (this.target && !plan && Number.isFinite(distance) && canHover)
      want = Math.min(want, Math.sqrt(2 * 0.3 * G * distance));
    // A hoverer slows to a hover over where it lands.
    if (plan && canHover) want = Math.min(want, Math.sqrt(2 * 0.3 * G * Math.max(0, distance)));
    if (!canHover) want = Math.max(want, fl.slow);
    const accel = (want > this.speed ? 0.35 : 0.6) * G;
    this.speed += Math.max(-accel * dt, Math.min(accel * dt, want - this.speed));
    this.speed = Math.max(0, this.speed);

    // Heading: a coordinated turn, at most 1.5 rad/s; the bank follows.
    const bankMax = (Math.min(70, this.gait?.bank ?? 40) * Math.PI) / 180;
    let turn = wrapAngle(wantHeading - this.heading);
    const limit = Math.abs(turn) > 2.4 ? Math.max(bankMax, Math.PI / 3) : bankMax;
    const rate = Math.min(1.5, (G * Math.tan(limit)) / Math.max(this.speed, fl.slow));
    turn = Math.max(-rate * dt, Math.min(rate * dt, turn));
    this.heading = wrapAngle(this.heading + turn);
    this.yawRate = damp(this.yawRate, turn / dt, 6, dt);
    forward.set(Math.sin(this.heading), 0, Math.cos(this.heading));

    // The floor under it, under its wingtips and along where it is turning, 0.5 s and 1.5 s on.
    const side = scratch1.set(forward.z, 0, -forward.x);
    let floorMax = floor(this.position.x, this.position.z);
    for (const sign of [-1, 1])
      floorMax = Math.max(
        floorMax,
        floor(
          this.position.x + side.x * sign * fl.halfSpan,
          this.position.z + side.z * sign * fl.halfSpan,
        ),
      );
    for (const ahead of [0.5, 1.5]) {
      const turnAhead = this.heading + this.yawRate * ahead;
      const run = this.speed * ahead;
      floorMax = Math.max(
        floorMax,
        floor(
          this.position.x + Math.sin((this.heading + turnAhead) / 2) * run,
          this.position.z + Math.cos((this.heading + turnAhead) / 2) * run,
        ),
      );
    }
    const altitude = this.position.y - floorMax;

    // Bank into the turn, as far as the lower wingtip stays clear of the floor.
    const clearance = 0.05 * fl.span;
    const tipRoom = (altitude + fl.shoulder - clearance) / Math.max(1e-3, fl.halfSpan);
    const bankRoom = Math.asin(Math.max(0, Math.min(1, tipRoom)));
    const bank = Math.atan2(this.speed * this.yawRate, G);
    a.bank = damp(a.bank, Math.max(-bankRoom, Math.min(bankRoom, bank)), 6, dt);

    // Height: as asked (a target's, or above the floor ahead), never closer than its span allows.
    const height = this.flyHeight ?? fl.height;
    // Above the floor under and ahead of it; coming in to land, down the glide path from there.
    const cruiseY = floorMax + height;
    let wantY =
      plan !== null
        ? Math.min(cruiseY, plan.flareY + Math.max(0, distance) * Math.tan((12 * Math.PI) / 180))
        : this.targetY !== undefined && this.target
          ? this.targetY
          : cruiseY;
    const needed = Math.max(
      0.6 * h,
      fl.halfSpan * Math.sin(Math.abs(a.bank)) - fl.shoulder + clearance,
    );
    wantY = Math.max(wantY, floorMax + needed);
    const hovering = canHover && this.speed < 0.3 * fl.cruise;
    const climbMax = hovering
      ? 0.5 * Math.sqrt(G * h)
      : Math.max(0.3, this.speed) * Math.sin((15 * Math.PI) / 180);
    const descentMax = hovering
      ? 0.5 * Math.sqrt(G * h)
      : Math.max(0.3, this.speed) * Math.sin((25 * Math.PI) / 180);

    // The air gait: hover when slow, glide when it may lose height, else flap.
    let role = a.role;
    const band = 0.03 * height;
    if (canHover && this.speed < (role === 'hovering' ? 0.45 : 0.3) * fl.cruise) role = 'hovering';
    else if (canGlide && this.speed >= fl.stall * 0.95) {
      if (role === 'gliding') {
        if (this.position.y < wantY - band) role = 'flapping';
      } else if (
        this.position.y >= wantY + 0.3 * band &&
        this.strokes.phase > 0.2 &&
        this.strokes.phase < 0.3
      )
        role = 'gliding';
    } else role = 'flapping';
    if (this.lockedGait?.medium === 'air') role = this.lockedGait.air ?? 'flapping';
    if (role !== a.role) {
      this.setAirGait(role);
      // A locked air gait keeps its role.
      role = a.role;
    }

    // Vertical speed toward the height, within the climb and descent limits.
    let wantVy = Math.max(-descentMax, Math.min(climbMax, (wantY - this.position.y) * 1.2));
    if (a.stage === 'launch') {
      a.vy -= G * dt;
      if (a.vy <= climbMax) a.stage = 'flight';
      wantVy = a.vy;
    } else if (role === 'gliding') {
      // A glider sinks along its glide ratio, at least.
      const ratio = 18 - 14 * (this.gait?.sink ?? 0.3);
      wantVy = Math.min(wantVy, -this.speed / ratio);
    }
    if (a.stage !== 'launch')
      a.vy += Math.max(-0.5 * G * dt, Math.min(0.5 * G * dt, wantVy - a.vy));
    this.position.addScaledVector(forward, this.speed * dt);
    this.position.y += a.vy * dt;
    // Never into the floor, whatever the limits said.
    const under = floor(this.position.x, this.position.z) + 0.3 * h;
    if (this.position.y < under) {
      this.position.y = under;
      a.vy = Math.max(a.vy, 0);
    }

    // Arrival: within reach of a target it stops and hovers, or circles round it.
    if (this.target && !plan && distance < Math.max(fl.span, 0.3 * r)) {
      this.events.push({
        type: 'arrive',
        time: this.time,
        position: [this.position.x, this.position.y, this.position.z],
      });
      this.loiter.copy(this.target);
      this.target = null;
      this.desiredSpeed = 0;
    }
    if (plan && canHover && distance < Math.max(0.5 * this.hipHeight, 0.1)) {
      // Over the point: straight down onto it.
      a.stage = 'descend';
      a.t = 0;
      a.flare = { y0: this.position.y, v0: this.speed };
    } else if (plan && along >= 0) {
      const lined = Math.cos(wrapAngle(Math.atan2(plan.dir.x, plan.dir.z) - this.heading));
      if (lined > 0.9 && across < Math.max(0.1 * fl.span, 2 * this.hipHeight)) {
        a.stage = 'flare';
        a.t = 0;
        a.flare = { y0: this.position.y, v0: this.speed };
      } else {
        // Not lined up: go round and come in again.
        (a.landing as NonNullable<AirState['landing']>).plan = null;
        (a.landing as NonNullable<AirState['landing']>).goRounds++;
      }
    }

    // The body follows the flight path; hovering, it pitches nose up.
    this.pitch = damp(
      this.pitch,
      Math.atan2(a.vy, Math.max(this.speed, 0.5)) + (role === 'hovering' ? 0.6 : 0),
      6,
      dt,
    );
    this.roll = damp(this.roll, -a.bank, 8, dt);
    this.lean = damp(this.lean, 0, 4, dt);
    this.bodyY = damp(this.bodyY, this.restBodyY, 8, dt);
    this.stepStrokes(dt, role, a.vy, climbMax, altitude);
    if (this.posing) this.tuckLegs();
  }

  /**
   * Plans a landing (decision 9): where it touches down (the point asked for, or the first clear
   * ground ahead at its approach distance), the line it comes in on, and where the flare starts.
   */
  private planLanding(a: AirState, floor: (x: number, z: number) => number): void {
    const landing = a.landing as NonNullable<AirState['landing']>;
    const fl = this.flightNumbers as FlightNumbers;
    const canHover = this.canRole('hovering');
    const flareTime = 0.8 * this.timeScale;
    const flareDistance = canHover ? 0 : 0.5 * fl.slow * flareTime;
    const glide =
      Math.max(0, this.position.y - floor(this.position.x, this.position.z)) /
      Math.tan((12 * Math.PI) / 180);
    let point = landing.to?.clone();
    if (!point || landing.goRounds >= 2) {
      // Ahead, past the descent, on ground (or water it could land on).
      const run = flareDistance + glide + turnRadius(fl);
      point = this.position
        .clone()
        .add(new Vector3(Math.sin(this.heading), 0, Math.cos(this.heading)).multiplyScalar(run));
      landing.goRounds = 0;
    }
    point.y = floor(point.x, point.z);
    const dir = new Vector3(point.x - this.position.x, 0, point.z - this.position.z);
    if (dir.lengthSq() < 1e-8) dir.set(Math.sin(this.heading), 0, Math.cos(this.heading));
    dir.normalize();
    const flareStart = point.clone().addScaledVector(dir, -flareDistance);
    flareStart.y = floor(flareStart.x, flareStart.z);
    landing.plan = {
      point,
      dir,
      flareStart,
      // The flare begins a leg's reach and a bit above the higher of the two ends.
      flareY: Math.max(point.y, flareStart.y) + 1.2 * this.hipHeight,
    };
  }

  /**
   * The flare and touchdown (decision 9): the speed falls to walking pace over 0.8 s × the time
   * scale while the body comes down onto the ground under it, never into it; a hoverer comes
   * straight down. The feet reach forward and down; at the ground every foot plants.
   */
  private stepTouchdown(
    a: AirState,
    dt: number,
    ground: Ground,
    water: Water | undefined,
    floor: (x: number, z: number) => number,
  ): void {
    const plan = (a.landing as NonNullable<AirState['landing']>).plan as LandingPlan;
    const h = this.hipHeight;
    if (a.stage === 'descend') {
      // A hoverer comes straight down onto the point, slowing as its feet near the ground,
      // levelling out from its hover as they reach for it.
      this.speed = damp(this.speed, 0, 4, dt);
      const k = Math.min(1, 3 * dt);
      this.position.x += (plan.point.x - this.position.x) * k;
      this.position.z += (plan.point.z - this.position.z) * k;
      const under = floor(this.position.x, this.position.z);
      const rate = 0.3 * Math.sqrt(G * h);
      const left = Math.max(0, this.position.y - under);
      const vy = Math.min(rate, Math.max(0.15 * rate, 2 * left));
      this.position.y = Math.max(under, this.position.y - vy * dt);
      a.vy = -vy;
      const reach = 1 - Math.min(1, left / (1.5 * h));
      this.airPivot = 1 - reach;
      this.pitch = damp(this.pitch, 0.6 * (1 - reach) + this.slopePitch(floor) * reach, 6, dt);
      this.roll = damp(this.roll, 0, 8, dt);
      this.yawRate = damp(this.yawRate, 0, 8, dt);
      this.bodyY = damp(this.bodyY, this.restBodyY, 8, dt);
      this.stepStrokes(dt, 'hovering', -vy, rate, left);
      this.tuckLegs(reach, floor);
      if (this.position.y - under > 0.02 * h) return;
      this.touchdown(ground, water);
      return;
    }
    const flare = a.flare ?? { y0: this.position.y, v0: this.speed };
    const time = this.canRole('hovering')
      ? Math.max(0.8 * this.timeScale, (flare.y0 - plan.point.y) / (0.3 * Math.sqrt(G * h)))
      : 0.8 * this.timeScale;
    const u = Math.min(1, a.t / time);
    const ease = u * u * (3 - 2 * u);
    const walk = TEMPERAMENTS[this.motion.temperament].walk;
    const pace = Math.sqrt(walk * G * h);
    this.speed = flare.v0 + (Math.min(pace, flare.v0) - flare.v0) * ease;
    const forward = scratch3.set(plan.dir.x, 0, plan.dir.z);
    this.heading = Math.atan2(plan.dir.x, plan.dir.z);
    this.yawRate = damp(this.yawRate, 0, 8, dt);
    this.position.addScaledVector(forward, this.speed * dt);
    const under = floor(this.position.x, this.position.z);
    this.position.y = Math.max(under, flare.y0 + (under - flare.y0) * ease);
    a.vy = 0;
    this.airPivot = 1 - ease;
    // Nose up through the flare (at most 20°), then with the slope as the feet touch.
    this.pitch = damp(
      this.pitch,
      0.35 * Math.sin(Math.PI * Math.min(1, u * 1.2)) + this.slopePitch(floor) * ease,
      8,
      dt,
    );
    this.roll = damp(this.roll, 0, 8, dt);
    this.bodyY = damp(this.bodyY, this.restBodyY, 8, dt);
    // Braking strokes, the stroke plane turned forward like a hover's; over the last half the
    // beat dies and the wings rise clear of the ground, as a bird's do as its feet reach down.
    this.strokes.hover = damp(this.strokes.hover, 0.5, 6, dt);
    const settle = Math.max(0, Math.min(1, (u - 0.35) / 0.4));
    const held = settle * settle * (3 - 2 * settle);
    this.stepStrokes(dt, 'flapping', -1, 1, this.position.y - under, 1 - held, 0.5 * held);
    this.tuckLegs(1 - ease, floor);
    if (u < 1 && this.position.y - under > 0.02 * h) return;
    this.touchdown(ground, water);
  }

  /** The ground's rise along the heading over the body's length where it stands (radians). */
  private slopeAlong(ground: Ground): number {
    const half = 0.5 * this.compiled.scale;
    const { x, z } = this.position;
    const fx = Math.sin(this.heading) * half;
    const fz = Math.cos(this.heading) * half;
    return Math.atan2(ground(x + fx, z + fz).height - ground(x - fx, z - fz).height, 2 * half);
  }

  /** The ground's rise toward its left across the body (radians). */
  private slopeAcross(ground: Ground): number {
    const half = 0.25 * this.compiled.scale;
    const { x, z } = this.position;
    const lx = Math.cos(this.heading) * half;
    const lz = -Math.sin(this.heading) * half;
    return Math.atan2(ground(x + lx, z + lz).height - ground(x - lx, z - lz).height, 2 * half);
  }

  /** The ground's pitch along the heading, between the front and hind feet (radians, uphill +). */
  private slopePitch(floor: (x: number, z: number) => number): number {
    const zs = this.legs.map((l) => l.neutral.z);
    const half = zs.length > 1 ? Math.max(0.05, (Math.max(...zs) - Math.min(...zs)) / 2) : 0.4;
    const fx = Math.sin(this.heading) * half;
    const fz = Math.cos(this.heading) * half;
    const { x, z } = this.position;
    return Math.atan2(floor(x + fx, z + fz) - floor(x - fx, z - fz), 2 * half);
  }

  /** On the ground (or water) again: feet planted, the body absorbing it, the wings folding. */
  private touchdown(ground: Ground, water: Water | undefined): void {
    const swims =
      this.swims && this.depthAt(ground, water, this.position.x, this.position.z) > this.swimDepth;
    this.air = null;
    this.events.push({ type: 'land', time: this.time });
    this.position.y = ground(this.position.x, this.position.z).height;
    this.target = null;
    this.desiredSpeed = 0;
    this.landedAt = this.time;
    // The strokes die away as the wings fold (`stepParts`), so nothing jumps.
    this.enterMedium(swims ? 'water' : 'land', ground, true);
  }

  /**
   * The wings' beat for the air gait (decision 6): the phase advances at the creature's wingbeat
   * (faster climbing, slower coming down), and amplitude, glide and hover blend over 0.3 s. One
   * `flap` event per beat, as the downstroke starts. The stroke stays within the elevation
   * limits and, near the floor, clear of it.
   */
  private stepStrokes(
    dt: number,
    role: 'flapping' | 'gliding' | 'hovering',
    vy: number,
    climbMax: number,
    altitude: number,
    braking = 1,
    raise = 0,
  ): void {
    const fl = this.flightNumbers as FlightNumbers;
    const st = this.strokes;
    const climbing = vy > 0.3 * climbMax;
    const effort =
      (this.air?.stage === 'launch' ? 1.3 : climbing ? 1.3 : vy < -0.3 ? 0.8 : 1) * braking;
    const blend = Math.min(1, dt / 0.3);
    st.amplitude +=
      ((role === 'gliding' ? 0 : effort * (this.gait?.stroke ?? 1)) - st.amplitude) * blend;
    st.glide += ((role === 'gliding' ? 1 : 0) - st.glide) * blend;
    st.hover += ((role === 'hovering' ? 1 : 0) - st.hover) * blend;
    st.hold += (1 - st.hold) * blend;
    st.raise += (raise - st.raise) * blend;
    const beat = fl.beat * (this.gait?.rate ?? 1) * Math.min(1.3, effort);
    const before = st.phase;
    st.beat = beat;
    st.phase = (st.phase + beat * dt) % 1;
    // In the air the cycle is the wingbeat (walking motion keyed to it only runs on land).
    this.phase = st.phase;
    if (st.glide < 0.5 && before < 0.25 && st.phase >= 0.25)
      this.events.push({ type: 'flap', time: this.time });
    // Peak elevation (dihedral and stroke) at most 80°; the downstroke clears the floor.
    st.up = (80 * Math.PI) / 180 - this.dihedral;
    const room = (altitude + fl.shoulder - 0.05 * fl.span) / Math.max(1e-3, fl.halfSpan);
    const drop = Math.asin(Math.max(-1, Math.min(1, room))) - Math.abs(this.roll) + this.dihedral;
    st.down = Math.max(0, Math.min(Math.PI / 3, drop));
  }

  /** The highest a wing points above horizontal in its spread pose (radians). */
  private get dihedral(): number {
    let most = 0;
    for (const w of this.compiled.rig.wings)
      most = Math.max(most, Math.asin(Math.max(-1, Math.min(1, w.stroke.out[1]))));
    return most;
  }

  /**
   * Legs in the air, in the body's own frame (decision 17 of the review): forelegs folded back
   * under the chest, hind legs trailing; `reach` 0 to 1 swings them down to land.
   */
  private tuckLegs(reach = 0, floor?: (x: number, z: number) => number): void {
    const pose = this.pose;
    const spine0 = this.compiled.rig.spine[0] as number;
    const body = scratchQ4
      .copy(pose.worldRot[spine0] as Quaternion)
      .multiply(scratchQ5.copy(pose.restWorldRot[spine0] as Quaternion).invert());
    const h = this.hipHeight;
    const middle =
      this.legs.reduce((sum, l) => sum + l.neutral.z, 0) / Math.max(1, this.legs.length);
    for (const leg of this.legs) {
      const hip = pose.worldPos[leg.rig.bones[0] as number] as Vector3;
      const fore = leg.neutral.z > middle + 1e-6 || this.legs.length <= 2;
      const tucked = scratch2.set(0, -0.45 * h, (fore ? -0.35 : -0.6) * h);
      // Reaching to land: below the hip, a little ahead, the leg nearly straight.
      const down = scratch5.set(0, -0.95 * h, 0.1 * h);
      tucked.lerp(down, reach).applyQuaternion(body);
      leg.planted.copy(hip).add(tucked);
      // Reaching for the ground, a foot stops on it (uphill it comes sooner).
      if (floor)
        leg.planted.y = Math.max(leg.planted.y, floor(leg.planted.x, leg.planted.z) + leg.footLift);
      leg.target.copy(leg.planted);
      leg.swinging = true;
      leg.armed = false;
      leg.swingU = 0.5;
    }
  }

  /** A gait's natural speed (m/s). */
  private gaitSpeedFor(gait: GaitInfo | undefined): number {
    return gait ? this.gaitSpeed(gait.id) : this.paceSpeed();
  }

  /**
   * Plans a leaping action's arc on its first step, when the ground is known: where it lands (the
   * target, or where its head meets it for a pounce, within its reach), then the lowest launch
   * angle from the action's that clears the ground and any height it asks for. The action's
   * length becomes the crouch, the flight and the recovery (docs/design/10.2-jumps.md).
   */
  private planLeap(c: RunningAction, ground: Ground): void {
    const plan = c.hooks.leap?.(c.params);
    if (!plan) return;
    const h = this.hipHeight;
    const from = new Vector3(this.position.x, 0, this.position.z);
    from.y = ground(from.x, from.z).height;
    const facing = new Vector3(Math.sin(this.heading), 0, Math.cos(this.heading));
    const dir = c.target ? new Vector3(c.target.x - from.x, 0, c.target.z - from.z) : facing;
    let distance = c.target ? dir.length() : plan.reach * h;
    if (dir.lengthSq() < 1e-12) dir.copy(facing);
    dir.normalize();
    if (plan.head && c.target) {
      // A pounce lands short, so the snout meets the target as the feet come down.
      const head = this.pose.worldPos[mainHead(this.compiled.rig).head] as Vector3;
      const ahead =
        (head.x - this.position.x) * Math.sin(this.heading) +
        (head.z - this.position.z) * Math.cos(this.heading);
      distance -= Math.max(0, ahead) + 0.15 * h;
    }
    distance = Math.max(0.2 * h, Math.min(distance, plan.most * h));
    const to = from.clone().addScaledVector(dir, distance);
    to.y = ground(to.x, to.z).height;
    const drop = to.y - from.y;
    const height = plan.height ?? 0;
    let rise = 0;
    let flight = 0;
    for (let deg = (plan.angle * 180) / Math.PI; deg <= 80; deg += 2.5) {
      const angle = (deg * Math.PI) / 180;
      const cos = Math.cos(angle);
      const denom = 2 * cos * cos * (distance * Math.tan(angle) - drop);
      if (denom <= 1e-9) continue;
      const speed = Math.sqrt((G * distance * distance) / denom);
      rise = speed * Math.sin(angle);
      flight = distance / (speed * cos);
      // The arc must clear the ground all the way, and the height asked for in its middle half.
      let clear = true;
      for (let k = 1; k < 16 && clear; k++) {
        const t = (flight * k) / 16;
        const y = from.y + rise * t - 0.5 * G * t * t;
        const x = from.x + (dir.x * (distance * k)) / 16;
        const z = from.z + (dir.z * (distance * k)) / 16;
        const need = k >= 4 && k <= 12 ? Math.max(height, 0.05 * h) : 0.02 * h;
        if (y < ground(x, z).height + need) clear = false;
      }
      if (clear) break;
    }
    const takeoff = plan.crouch * this.timeScale;
    const land = takeoff + flight;
    c.duration = land + plan.recover * this.timeScale;
    const timing = { takeoff: takeoff / c.duration, land: land / c.duration };
    c.events = [
      ...(c.hooks.events?.(c.params, timing) ?? []),
      { at: timing.takeoff, type: 'takeoff' },
      { at: timing.land, type: 'land' },
    ].sort((a, b) => a.at - b.at);
    c.leap = {
      from,
      to,
      heading: Math.atan2(dir.x, dir.z),
      rise,
      flight,
      takeoff,
      land,
      timing,
      airborne: false,
      landed: false,
    };
  }

  /**
   * A leap's body: it crouches and turns to face where it lands, flies the arc with its legs
   * tucked, then lands with every foot planted on the ground there.
   */
  private stepLeap(leap: Leap, dt: number, ground: Ground): void {
    const elapsed = this.time - (this.current?.start ?? this.time);
    const h = this.hipHeight;
    if (elapsed < leap.takeoff) {
      const turn = wrapAngle(leap.heading - this.heading);
      const step = turn * Math.min(1, 10 * dt);
      this.heading = wrapAngle(this.heading + step);
      this.yawRate = damp(this.yawRate, step / dt, 8, dt);
      this.speed = damp(this.speed, 0, 12, dt);
      this.position.y = ground(this.position.x, this.position.z).height;
      this.stepLegs(dt, ground, this.froudeNow());
      return;
    }
    if (!leap.airborne) {
      leap.airborne = true;
      this.heading = leap.heading;
      this.yawRate = 0;
      for (const leg of this.legs) {
        leg.swinging = true;
        leg.armed = false;
      }
    }
    const t = Math.min(elapsed - leap.takeoff, leap.flight);
    const u = leap.flight > 0 ? t / leap.flight : 1;
    this.position.lerpVectors(leap.from, leap.to, u);
    this.position.y = leap.from.y + leap.rise * t - 0.5 * G * t * t;
    this.speed = leap.from.distanceTo(leap.to) / Math.max(leap.flight, 1e-3);
    // Nose up as it leaves, level at the top, down as it comes in.
    this.pitch = damp(this.pitch, 0.25 * Math.cos(Math.PI * u), 10, dt);
    this.roll = damp(this.roll, 0, 10, dt);
    if (elapsed < leap.land) {
      // Legs tuck under the body in the air and reach down again to land.
      const tuck = Math.sin(Math.PI * u) * 0.25 * h;
      for (const leg of this.legs) {
        this.neutralAt(leg, this.heading, leg.planted);
        leg.planted.y += tuck;
        leg.target.copy(leg.planted);
        leg.swingU = 0.5;
      }
      return;
    }
    // Down: every foot plants where it is, on the ground there.
    leap.landed = true;
    this.position.copy(leap.to);
    this.speed = 0;
    for (const leg of this.legs) {
      const foot = this.neutralAt(leg, this.heading, leg.planted);
      foot.y = ground(foot.x, foot.z).height + leg.footLift;
      leg.liftoff.copy(foot);
      leg.target.copy(foot);
      leg.swinging = false;
      leg.armed = true;
      if (leg.roll) {
        leg.roll.heel = 0;
        plantToes(leg.roll, foot, leg.footLift, this.heading, (x, z) => ground(x, z).height);
      }
    }
  }

  /** Ambient goals, then the main action's on top; fires the action's events as it passes them. */
  private updateGoals(): void {
    const goals: ActionGoals = {};
    const rig = this.compiled.rig;
    // The main head speaks for the creature; an action aimed at a target uses the nearest head.
    const c = this.current;
    const acting = rig.heads[this.nearestHead(c?.target ?? null)] ?? mainHead(rig);
    const head = this.pose.worldPos[acting.head] as Vector3;
    const forward = scratch3.set(Math.sin(this.heading), 0, Math.cos(this.heading));
    const base = {
      timeScale: this.timeScale,
      head,
      forward,
      speed: this.speed,
      busy: this.current !== null,
    };
    for (const a of this.ambient) {
      const ctx: ActionContext = {
        ...base,
        t: 0,
        elapsed: this.time,
        duration: 0,
        params: a.params,
        target: null,
        rng: a.rng,
      };
      a.hooks.goals(ctx, goals);
    }
    if (c) {
      const before = c.progress;
      c.progress = Math.min(1, (this.time - c.start) / c.duration);
      for (const e of c.events) {
        if (e.at > before && e.at <= c.progress)
          this.events.push({
            type: e.type,
            time: this.time,
            action: c.id,
            position: [head.x, head.y, head.z],
            ...(rig.heads.length > 1 ? { head: acting.id } : {}),
          });
      }
      const main: ActionGoals = {};
      c.hooks.goals(
        {
          ...base,
          t: c.progress,
          elapsed: this.time - c.start,
          duration: c.duration,
          params: c.params,
          target: c.target,
          rng: c.rng,
          ...(c.leap ? { leap: c.leap.timing } : {}),
        },
        main,
      );
      Object.assign(goals, main);
      if (c.progress >= 1) {
        this.events.push({ type: 'action-end', time: this.time, action: c.id });
        this.current = null;
      }
    }
    this.goals = goals;
    if (goals.glance && this.compiled.rig.heads.length > 1) {
      this.glances.push({ time: this.time, ...goals.glance });
      while ((this.glances[0]?.time ?? this.time) < this.time - 2) this.glances.shift();
    }
  }

  /** Sets every leg's offset to the gait's at once (a locked gait, before it moves). */
  private retimeLegs(): void {
    for (const leg of this.legs) leg.offset = offsetFor(leg.rig, this.gait);
    this.dutyNow = this.dutyAt(this.gait, this.froudeNow());
  }

  /**
   * Eases each leg's offset toward the gait's, the shorter way round the cycle, by at most a
   * quarter of a cycle per cycle, so a gait change never snaps a foot down or skips a step
   * (docs/design/10.1-gaits.md).
   */
  private easeOffsets(frequency: number, dt: number): void {
    const most = 0.25 * Math.max(frequency, 0.5) * dt;
    for (const leg of this.legs) {
      const goal = offsetFor(leg.rig, this.gait);
      const d = ((((goal - leg.offset) % 1) + 1.5) % 1) - 0.5;
      if (d === 0) continue;
      leg.offset = (((leg.offset + Math.max(-most, Math.min(most, d))) % 1) + 1) % 1;
    }
  }

  /** The gait's duty at a Froude number: its profile across its range, else its one value. */
  private dutyAt(gait: GaitInfo | undefined, froude: number): number {
    if (!gait) return 0.7;
    return profileAt(gait, gait.duty, gait.dutyFast, froude);
  }

  private froudeNow(): number {
    return (this.speed * this.speed) / (G * this.hipHeight);
  }

  /**
   * Stride length (m) of a gait at a Froude number, from dynamic similarity (Alexander):
   * λ/h = 2.3 Fr^0.3, but never more than the legs can reach: past that the creature steps
   * faster instead. `strideScale` stands in for the gait's own `stride` multiplier.
   */
  private strideAt(gait: GaitInfo | undefined, froude: number, strideScale?: number): number {
    const duty = this.dutyAt(gait, froude);
    const h = this.hipHeight;
    const scale = gait ? profileAt(gait, gait.stride, gait.strideFast, froude) : 1;
    const natural =
      Math.max(0.35 * h, 2.3 * h * Math.max(froude, 0.01) ** 0.3) * (strideScale ?? scale);
    // A gait with flight plants each foot briefly, so its stance may use a lower duty here.
    const half = gait?.flight ? this.halfStrideLow : this.halfStride;
    return Math.min(natural, (2 * half) / Math.max(duty, gait?.flight ? 0.15 : 0.3));
  }

  /**
   * How fast a gait steps at its typical speed (or `options.speed`): metres per stride, seconds
   * per cycle and steps per second (each foot steps once a cycle, so cycles per second).
   * `options.stride` tries another stride multiplier.
   */
  cadence(
    id: string,
    options: { speed?: number; stride?: number } = {},
  ): { speed: number; stride: number; cycle: number; steps: number } {
    const speed = options.speed ?? this.gaitSpeed(id);
    const gait = this.motion.gaits.find((g) => g.id === id);
    const stride = this.strideAt(gait, (speed * speed) / (G * this.hipHeight), options.stride);
    const cycle = stride / Math.max(speed, 1e-6);
    return { speed, stride, cycle, steps: 1 / cycle };
  }

  private stepLegs(dt: number, ground: Ground, froude: number): void {
    const gait = this.gait;
    // Duty follows the gait's profile at this speed, and eases across a gait change.
    const wantDuty = this.dutyAt(gait, froude);
    this.dutyNow = this.dutyNow === wantDuty ? wantDuty : damp(this.dutyNow, wantDuty, 4, dt);
    const duty = this.dutyNow;
    const h = this.hipHeight;
    const stride = this.strideAt(gait, froude);
    let frequency = this.speed / stride;
    // Keep stepping while turning on the spot or settling feet after stopping, and finish any
    // swing in progress; once every foot is close to its rest spot the legs stand still.
    const turning = Math.abs(this.yawRate) > 0.15;
    // Staggering, and until every foot has caught up with the body after it.
    if (this.staggerT === 0 && this.legs.every((l) => !l.swinging && this.footError(l) <= 0.04 * h))
      this.steadying = false;
    const staggering = this.staggerT > 0 || this.steadying;
    const restless =
      staggering || turning || this.legs.some((l) => l.swinging || this.footError(l) > 0.12 * h);
    // Staggering, it steps quickly to catch itself.
    const minimum = ((staggering ? 1.2 : 0.25) * Math.sqrt(G / h)) / Math.PI;
    if (restless) frequency = Math.max(frequency, minimum);
    // Step faster when a planted foot would fall out of reach before its lift-off (starting
    // off, speeding up): it has `halfStride` behind its rest spot plus however far ahead it is.
    const unhurried = frequency;
    for (const leg of this.legs) {
      if (leg.swinging) continue;
      const local = (((this.phase - leg.offset) % 1) + 1) % 1;
      if (local >= duty) continue;
      const half = gait?.flight ? this.halfStrideLow : this.halfStride;
      const budget = Math.max(0.05 * half, half + this.footAhead(leg));
      frequency = Math.max(frequency, (this.speed * (duty - local)) / budget);
    }
    frequency = Math.min(frequency, Math.max(unhurried * 3, minimum));
    const previous = this.phase;
    this.phase = (this.phase + frequency * dt) % 1;
    const advanced = frequency > 0;

    for (const leg of this.legs) {
      if (!advanced) break;
      // A leg's cycle starts `offset` after the creature's: φ = (i·w + 0.5·s) mod 1.
      const before = (((previous - leg.offset) % 1) + 1) % 1;
      const now = (((this.phase - leg.offset) % 1) + 1) % 1;
      const crossedLand = now < before;
      // A leg lifts once per cycle, when its phase is in the swing window after a stance.
      if (now < duty) leg.armed = true;
      const needed = this.speed > 0.02 * h || turning || this.footError(leg) > 0.04 * h;
      if (leg.armed && now >= duty && !leg.swinging && needed) {
        leg.armed = false;
        leg.swinging = true;
        leg.liftoff.copy(leg.planted);
      }
      if (leg.swinging) {
        // Swing progress; on the step the phase wraps past 1 the swing is complete.
        const u = crossedLand ? 1 : Math.min(1, Math.max(0, (now - duty) / (1 - duty)));
        // Aim where the hip will be at landing, half a stance ahead.
        const timeToLand = ((1 - u) * (1 - duty)) / Math.max(frequency, 1e-3);
        const stanceTime = duty / Math.max(frequency, 1e-3);
        const futureHeading = this.heading + this.yawRate * timeToLand;
        const ahead = this.speed * (timeToLand + stanceTime * 0.5);
        this.neutralAt(leg, futureHeading, leg.target);
        leg.target.x += Math.sin(this.heading) * ahead;
        leg.target.z += Math.cos(this.heading) * ahead;
        leg.target.y = ground(leg.target.x, leg.target.z).height + leg.footLift;
        leg.swingU = u;
        if (crossedLand || u >= 1) {
          leg.swinging = false;
          leg.planted.copy(leg.target);
          if (leg.roll) {
            leg.roll.heel = 0;
            plantToes(
              leg.roll,
              leg.planted,
              leg.footLift,
              this.heading,
              (x, z) => ground(x, z).height,
            );
          }
          this.events.push({
            type: 'footstep',
            time: this.time,
            leg: leg.rig.id,
            position: [leg.planted.x, leg.planted.y, leg.planted.z],
          });
        } else {
          const e = u * u * (3 - 2 * u);
          leg.planted.lerpVectors(leg.liftoff, leg.target, e);
          leg.planted.y += Math.sin(Math.PI * u) * (gait?.stepHeight ?? 0.15) * h;
        }
      }
    }

    // Flight: with every foot off the ground the body rises and falls on a ballistic arc that
    // lands when the next foot does (docs/design/10.1-gaits.md); on the ground it settles.
    const airborne = advanced && this.speed > 0.02 * h && this.legs.every((l) => l.swinging);
    if (airborne && !this.airborne) {
      let until = 1;
      for (const leg of this.legs)
        until = Math.min(until, (((leg.offset - this.phase) % 1) + 1) % 1);
      this.airV = (G * until) / Math.max(frequency, 1e-3) / 2;
    }
    this.airborne = airborne;
    if (airborne) {
      this.airY = Math.max(0, this.airY + this.airV * dt);
      this.airV -= G * dt;
    } else {
      this.airY = damp(this.airY, 0, 30, dt);
      this.airV = 0;
    }
    this.easeOffsets(frequency, dt);

    // Planted feet roll: late in the stance the heel lifts (straight up, so the foot does not
    // slide) while the toes stay down; standing still, it settles.
    const rolling = advanced && this.speed > 0.02 * h;
    for (const leg of this.legs) {
      if (!leg.roll || leg.swinging) continue;
      const local = (((this.phase - leg.offset) % 1) + 1) % 1;
      leg.roll.heel = rolling ? heelAt(leg.roll, local, duty) : leg.roll.heel * 0.85;
      leg.planted.y = ground(leg.planted.x, leg.planted.z).height + leg.footLift + leg.roll.heel;
    }

    // Body height, pitch and roll follow the feet.
    let front = 0;
    let back = 0;
    let left = 0;
    let right = 0;
    let nf = 0;
    let nb = 0;
    let nl = 0;
    let nr = 0;
    const maxPair = Math.max(0, ...this.legs.map((l) => l.rig.pair));
    // Swinging feet count at their landing height, not mid-arc.
    const footY = (leg: LegState) => (leg.swinging ? leg.target.y : leg.planted.y);
    for (const leg of this.legs) {
      const lift = footY(leg) - leg.footLift - this.position.y;
      if (leg.rig.pair === maxPair && maxPair > 0) {
        front += lift;
        nf++;
      } else if (leg.rig.pair === 0) {
        back += lift;
        nb++;
      }
      if (leg.rig.side === 'left') {
        left += lift;
        nl++;
      } else {
        right += lift;
        nr++;
      }
    }
    const span = Math.max(
      0.2 * h,
      Math.abs(
        (this.legs.find((l) => l.rig.pair === maxPair)?.neutral.z ?? 0) -
          (this.legs.find((l) => l.rig.pair === 0)?.neutral.z ?? 0),
      ),
    );
    const width = Math.max(
      0.1 * h,
      Math.abs((this.legs.find((l) => l.rig.side === 'left')?.neutral.x ?? 0) * 2),
    );
    const wantPitch = nf && nb ? Math.atan2(front / nf - back / nb, span) : 0;
    const wantRoll = nl && nr ? Math.atan2(right / nr - left / nl, width) * 0.6 : 0;
    this.pitch = damp(this.pitch, Math.max(-0.5, Math.min(0.5, wantPitch)), 10, dt);
    this.roll = damp(this.roll, Math.max(-0.35, Math.min(0.35, wantRoll)), 10, dt);
    const mean =
      this.legs.reduce((a, l) => a + (footY(l) - l.footLift), 0) / Math.max(1, this.legs.length);
    // A gait with flight runs low, which gives its legs the room for a longer stance.
    const crouch =
      TEMPERAMENTS[this.motion.temperament].crouch * this.restBodyY +
      (gait?.flight ? LOW * h * Math.min(1, this.speed / Math.max(1e-3, this.paceSpeed())) : 0);
    let wantY = this.restBodyY - crouch + (mean - this.position.y) * 0.8;
    // Sink the body where a planted foot (in a dip) would be out of the leg's reach.
    let drop = 0;
    for (const leg of this.legs) {
      if (leg.swinging) continue;
      const hip = this.pose.restWorldPos[leg.rig.bones[0] as number] as Vector3;
      const c = Math.cos(this.heading);
      const s = Math.sin(this.heading);
      const hx = this.position.x + hip.x * c + hip.z * s;
      const hz = this.position.z - hip.x * s + hip.z * c;
      const hy = this.position.y + hip.y + (wantY - this.restBodyY);
      const across = Math.hypot(leg.planted.x - hx, leg.planted.z - hz);
      const reach = 0.97 * leg.rig.reach;
      if (across >= reach) continue;
      const down = hy - leg.planted.y;
      const fits = Math.sqrt(reach * reach - across * across);
      if (down > fits) drop = Math.max(drop, down - fits);
    }
    wantY -= Math.min(drop, 0.5 * this.restBodyY);
    this.bodyY = damp(this.bodyY, wantY, 12, dt);
    this.bend = damp(this.bend, Math.max(-0.5, Math.min(0.5, this.yawRate * 0.25)), 6, dt);
    // A runner leans into its speed, more toward the top of the gait's range.
    const lean = gait?.lean ? ((gait.lean * Math.PI) / 180) * profileAt(gait, 0, 1, froude) : 0;
    this.lean = damp(this.lean, lean, 4, dt);
  }

  private footError(leg: LegState): number {
    const n = this.neutralAt(leg, this.heading, scratch1);
    return Math.hypot(leg.planted.x - n.x, leg.planted.z - n.z);
  }

  /** How far a planted foot is ahead of its rest spot, along the heading. */
  private footAhead(leg: LegState): number {
    const n = this.neutralAt(leg, this.heading, scratch1);
    return (
      (leg.planted.x - n.x) * Math.sin(this.heading) +
      (leg.planted.z - n.z) * Math.cos(this.heading)
    );
  }

  /** Where a leg's foot rests relative to the body, for a given heading. */
  private neutralAt(leg: LegState, heading: number, out: Vector3): Vector3 {
    const c = Math.cos(heading);
    const s = Math.sin(heading);
    const x = leg.neutral.x;
    const z = leg.neutral.z;
    return out.set(
      this.position.x + x * c + z * s,
      leg.neutral.y + this.position.y,
      this.position.z - x * s + z * c,
    );
  }

  private stepSlither(dt: number, ground: Ground): void {
    const gait = this.gait ?? this.motion.gaits[0];
    const bodyLength = this.spineLength();
    const wavelength = bodyLength / Math.max(0.5, gait?.waves ?? 1.5);
    const amplitude =
      (gait?.amplitude ?? 0.18) *
      bodyLength *
      Math.min(1, this.speed / Math.max(1e-3, this.paceSpeed()) + 0.15);
    this.phase = (this.phase + (this.speed * dt) / wavelength) % 1;
    // The head weaves around the travel line; the body follows its trail.
    const forward = new Vector3(Math.sin(this.heading), 0, Math.cos(this.heading));
    const side = new Vector3(forward.z, 0, -forward.x);
    const head = this.position
      .clone()
      .addScaledVector(side, Math.sin(this.phase * Math.PI * 2) * amplitude);
    head.y = ground(head.x, head.z).height;
    // The newest point follows the head; a new one is laid down each time the head gets a
    // spacing away from the last fixed point.
    const anchor = this.trail[1];
    if (!anchor || anchor.distanceTo(head) > bodyLength / 100) this.trail.unshift(head);
    else (this.trail[0] as Vector3).copy(head);
    // Keep enough trail for the whole body.
    let acc = 0;
    for (let i = 1; i < this.trail.length; i++) {
      acc += (this.trail[i] as Vector3).distanceTo(this.trail[i - 1] as Vector3);
      if (acc > bodyLength * 1.5) {
        this.trail.length = i + 1;
        break;
      }
    }
  }

  /**
   * How far the snout reaches ahead of the body's middle, and the tail tip behind it (m), along
   * the body at rest: what dips furthest when a swimmer pitches.
   */
  private reaches(): readonly [number, number] {
    if (!this.reachCache) {
      const r = this.compiled.rig;
      const sum = (bones: readonly number[]) =>
        bones.reduce((s, b) => s + (this.pose.lengths[b] as number), 0);
      const torso = sum(r.spine) / 2;
      this.reachCache = [
        torso + sum(mainHead(r).neck) + sum([mainHead(r).head]),
        torso + sum(this.tailBones),
      ];
    }
    return this.reachCache;
  }
  private reachCache: readonly [number, number] | undefined;

  private spineLength(): number {
    const r = this.compiled.rig;
    let len = 0;
    for (const b of [...mainHead(r).neck, ...r.spine, ...this.tailBones])
      len += this.pose.lengths[b] as number;
    return Math.max(len, 1e-3);
  }

  /**
   * Where a chain's joints would be in its rest pose, hanging off its posed parent and swung
   * sideways by `swish`: the shape a spring pulls toward.
   */
  private restChain(bones: readonly number[], swish: number): Vector3[] {
    const pose = this.pose;
    const first = bones[0] as number;
    const parent = pose.parents[first] as number;
    const rot = scratchQ3.copy(parent >= 0 ? (pose.worldRot[parent] as Quaternion) : IDENTITY);
    if (swish) rot.premultiply(scratchQ.setFromAxisAngle(UP, swish));
    const at = scratch4.copy(pose.worldPos[first] as Vector3);
    bones.forEach((b, i) => {
      if (i > 0) at.add(scratch1.copy(pose.restPos[b] as Vector3).applyQuaternion(rot));
      rot.multiply(pose.restRot[b] as Quaternion);
      const out = this.springRest[i] ?? new Vector3();
      this.springRest[i] = out;
      out
        .set(0, pose.lengths[b] as number, 0)
        .applyQuaternion(rot)
        .add(at);
    });
    return this.springRest;
  }

  private stepSprings(dt: number, ground: Ground): void {
    if (this.springs.length === 0) return;
    // The roots follow their bones, as the last pose left them (solved throughout); the rest
    // feel inertia, gravity and a pull toward rest.
    // Sprawlers carry the body's side-to-side wave on into the tail.
    const moving = Math.min(1, this.speed / Math.max(1e-3, this.paceSpeed()));
    const wave =
      this.compiled.rig.posture === 'sprawl' && this.medium !== 'water'
        ? Math.sin(this.phase * Math.PI * 2 - 1.2) * 0.15 * moving
        : 0;
    // A lash whips the tail or tentacle nearest the target; a grab reaches the two nearest
    // tentacles for it (docs/design/9.4-tentacles-parts.md).
    const g = this.goals;
    const look = g.look ?? null;
    const lash = look ? Math.max(-1, Math.min(1, g.lash ?? 0)) : 0;
    const grab = look ? Math.max(0, Math.min(1, g.grab ?? 0)) : 0;
    const trailing = this.compiled.rig.posture === 'legless' ? this.tailBones[0] : undefined;
    let lasher = -1;
    let reach1 = -1;
    let reach2 = -1;
    if (look && (lash !== 0 || grab > 0)) {
      let best = Infinity;
      let d1 = Infinity;
      let d2 = Infinity;
      for (const [k, spring] of this.springs.entries()) {
        if (spring.kind === 'part' || spring.bones[0] === trailing) continue;
        const d = (spring.points.at(-1) as Vector3).distanceToSquared(look);
        if (d < best) {
          best = d;
          lasher = k;
        }
        if (spring.kind !== 'tentacle') continue;
        if (d < d1) {
          d2 = d1;
          reach2 = reach1;
          d1 = d;
          reach1 = k;
        } else if (d < d2) {
          d2 = d;
          reach2 = k;
        }
      }
      if (lash === 0) lasher = -1;
    }
    for (const [k, spring] of this.springs.entries()) {
      const pts = spring.points;
      (pts[0] as Vector3).copy(this.pose.worldPos[spring.bones[0] as number] as Vector3);
      const swim = this.medium === 'water' ? this.swimSwish : 0;
      const rest = this.restChain(
        spring.bones,
        spring.swish ? (this.goals.swish ?? 0) + wave + swim : 0,
      );
      if (look && k === lasher) this.lashRest(spring, rest, look, lash, g.lashArc ?? Math.PI / 2);
      if (look && grab > 0 && (k === reach1 || k === reach2))
        this.reachRest(spring, rest, look, grab);
      // A whipping chain follows its strike more tightly; a dead one goes limp.
      const limp = this.death ? Math.min(1, this.death.t / this.death.duration) : 0;
      const stiffness =
        (k === lasher ? Math.max(spring.stiffness, 0.5) : spring.stiffness) * (1 - 0.95 * limp);
      for (let i = 1; i < pts.length; i++) {
        const p = pts[i] as Vector3;
        const prev = spring.previous[i] as Vector3;
        const velocity = scratch1.subVectors(p, prev).multiplyScalar(0.92 - 0.12 * limp);
        prev.copy(p);
        p.add(velocity);
        p.addScaledVector(UP, -G * (0.15 + 0.85 * limp) * dt * dt);
        p.lerp(rest[i - 1] as Vector3, stiffness);
      }
      // One pass from the root leaves every segment its length (a second only rounds).
      for (let i = 1; i < pts.length; i++) {
        const a = pts[i - 1] as Vector3;
        const b = pts[i] as Vector3;
        const len = spring.lengths[i - 1] as number;
        const d = scratch2.subVectors(b, a);
        const l = d.length() || 1;
        b.copy(a).addScaledVector(d, len / l);
      }
      // Tentacles and antennae lie on the ground rather than sink, sliding with some friction:
      // a joint that would go under swings up onto the ground, keeping its segment's length.
      // Tails only in the air and through a landing's absorb (10.4). Between samples, a few
      // floor steps apart, the floor follows the slope of the ground where it was sampled.
      const clearance = spring.clearance;
      const floors = spring.floors;
      const tail = spring.kind === 'tail';
      const step = 3 * this.floorStep;
      if (
        clearance &&
        floors &&
        (!tail || this.air !== null || this.landedAt !== undefined || this.death !== null)
      )
        for (let i = 1; i < pts.length; i++) {
          const a = pts[i - 1] as Vector3;
          const p = pts[i] as Vector3;
          const k = i * 5;
          const moved =
            Math.abs(p.x - (floors[k] as number)) + Math.abs(p.z - (floors[k + 1] as number));
          if (!(moved < step)) {
            const sample = ground(p.x, p.z);
            const n = sample.normal;
            floors[k] = p.x;
            floors[k + 1] = p.z;
            floors[k + 2] = sample.height;
            floors[k + 3] = n ? -n[0] / Math.max(0.2, n[1]) : 0;
            floors[k + 4] = n ? -n[2] / Math.max(0.2, n[1]) : 0;
          }
          const floor =
            (floors[k + 2] as number) +
            (floors[k + 3] as number) * (p.x - (floors[k] as number)) +
            (floors[k + 4] as number) * (p.z - (floors[k + 1] as number)) +
            (clearance[i] as number);
          if (p.y >= floor) continue;
          const len = spring.lengths[i - 1] as number;
          const rise = Math.min(len, floor - a.y);
          const flat = scratch2.set(p.x - a.x, 0, p.z - a.z);
          if (flat.lengthSq() < 1e-12) flat.set(1, 0, 0);
          flat.setLength(Math.sqrt(Math.max(0, len * len - rise * rise)));
          p.set(a.x + flat.x, a.y + rise, a.z + flat.z);
          const prev = spring.previous[i] as Vector3;
          prev.y = p.y;
          prev.x += (p.x - prev.x) * 0.3;
          prev.z += (p.z - prev.z) * 0.3;
        }
    }
  }

  /** Swings a bone up about its head, keeping its length, until its end is `by` higher. */
  private raiseEnd(b: number, length: number, by: number): void {
    const head = this.pose.worldPos[b] as Vector3;
    const end = this.pose.tail(b, scratch2);
    const rise = Math.min(length, end.y + by - head.y);
    const flat = scratch1.set(end.x - head.x, 0, end.z - head.z);
    if (flat.lengthSq() < 1e-12) flat.set(Math.sin(this.heading), 0, Math.cos(this.heading));
    flat.setLength(Math.sqrt(Math.max(0, length * length - rise * rise)));
    this.pose.aim(b, flat.setY(rise));
  }

  /**
   * Swings a chain's rest shape about its root toward `target` (a lash): `v` 1 turns it the whole
   * way (at most `arc` radians), negative winds it up away. Near the end of the strike, a target
   * in reach draws the tip onto it.
   */
  private lashRest(spring: Spring, rest: Vector3[], target: Vector3, v: number, arc: number): void {
    const root = spring.points[0] as Vector3;
    const n = spring.bones.length;
    const toTip = scratch1.subVectors(rest[n - 1] as Vector3, root);
    const toTarget = scratch2.subVectors(target, root);
    const axis = scratch4.crossVectors(toTip, toTarget);
    if (axis.lengthSq() < 1e-12) axis.copy(UP);
    else axis.normalize();
    const angle = v > 0 ? Math.min(arc, toTip.angleTo(toTarget)) * v : 0.5 * arc * v;
    const turn = scratchQ.setFromAxisAngle(axis, angle);
    for (let i = 0; i < n; i++) (rest[i] as Vector3).sub(root).applyQuaternion(turn).add(root);
    const w = ramp(v, 0.6, 1);
    let total = 0;
    for (const l of spring.lengths) total += l;
    if (w > 0 && root.distanceTo(target) < 0.98 * total) this.reachRest(spring, rest, target, w);
  }

  /** Bends a chain's rest shape toward `target` by FABRIK, blended by `w` (a grab). */
  private reachRest(spring: Spring, rest: Vector3[], target: Vector3, w: number): void {
    const n = spring.bones.length;
    const pts = this.reachPoints;
    while (pts.length <= n) pts.push(new Vector3());
    pts.length = n + 1;
    (pts[0] as Vector3).copy(spring.points[0] as Vector3);
    for (let i = 0; i < n; i++) (pts[i + 1] as Vector3).copy(rest[i] as Vector3);
    fabrik(pts, spring.lengths, target);
    for (let i = 0; i < n; i++) (rest[i] as Vector3).lerp(pts[i + 1] as Vector3, w);
  }

  /** Writes the body, legs, head and springs into the pose. */
  private applyPose(ground: Ground): void {
    if (this.death) {
      this.applyDeathPose(ground);
      return;
    }
    const pose = this.pose;
    const rig = this.compiled.rig;
    pose.reset();
    const root = rig.root;
    (pose.pos[root] as Vector3).copy(this.position);
    (pose.rot[root] as Quaternion).setFromAxisAngle(UP, this.heading);

    // On land a legless body follows its trail; swimming, every body is posed as below.
    if (rig.posture === 'legless' && this.medium === 'land') {
      this.applySlither(ground);
      // The main tail follows the trail; extra tails swing on their springs.
      if (this.springs.length > 0) {
        this.applySprings(this.tailBones);
        this.solvePose();
      }
      this.applyWings();
      return;
    }

    // Body: height, bob and sway, pitch and roll, bend into turns, plus the action's crouch,
    // rear and weight shift.
    const g = this.goals;
    const spine0 = rig.spine[0] as number;
    const h = this.hipHeight;
    // In the air nothing walks: no bob, sway or arm swing (the phase is the wingbeat there).
    const moving = this.air ? 0 : Math.min(1, this.speed / Math.max(1e-3, this.paceSpeed()));
    // Gaits with flight rise and fall on their own arc (`airY`) instead of the walking bob;
    // swimmers float.
    const bob =
      this.gait?.flight || this.medium !== 'land'
        ? 0
        : -Math.cos(this.phase * Math.PI * 4) * 0.025 * h * moving;
    const sway =
      (rig.legs.length <= 2 ? Math.sin(this.phase * Math.PI * 2) * 0.02 * h * moving : 0) +
      (g.shift ?? 0) * h;
    // A lunge throws the whole body forward a little, not just the neck.
    const lunge = (g.reach ?? 0) * 0.12 * this.compiled.scale;
    // A blow's sway, from the world into the root's frame.
    const swayX = this.sway.x * Math.cos(this.heading) - this.sway.z * Math.sin(this.heading);
    const swayZ = this.sway.x * Math.sin(this.heading) + this.sway.z * Math.cos(this.heading);
    (pose.pos[spine0] as Vector3).set(
      sway + swayX,
      this.bodyY + bob + this.airY - (g.crouch ?? 0) * h - this.airCrouch,
      (pose.restPos[spine0] as Vector3).z + lunge + swayZ,
    );
    const tilt = scratchQ3.setFromEuler(
      scratchEuler.set(-this.pitch - (g.rear ?? 0) + this.lean, 0, this.roll, 'YXZ'),
    );
    (pose.rot[spine0] as Quaternion).premultiply(tilt);
    // Flying, it pitches and rolls about its wing roots, so the wings stay where they push;
    // reaching for the ground, about its middle again, as it stands.
    if (this.air && rig.wings.length > 0 && this.airPivot > 0) {
      const d = this.wingPivot;
      const turned = scratch1.copy(d).applyQuaternion(tilt).sub(d);
      (pose.pos[spine0] as Vector3).addScaledVector(turned, -this.airPivot);
    }
    const sprawl = rig.posture === 'sprawl' && this.medium === 'land';
    const mainNeck = mainHead(rig).neck;
    // An upright front leans back against most of the body's pitch and rearing, so it stays
    // upright on slopes as a rider would (docs/design/9.2-legs-centaurs.md).
    if (this.uprightFront && mainNeck[0] !== undefined)
      (pose.rot[mainNeck[0]] as Quaternion).premultiply(
        scratchQ.setFromAxisAngle(X_AXIS, -0.8 * (this.pitch + (g.rear ?? 0))),
      );
    const bendBones = [...rig.spine.slice(1), ...mainNeck];
    // A swimmer's body wave runs back from the head, growing toward the hips, into the tail.
    const wave = this.medium === 'water' && this.gait?.swim !== 'legs' ? this.swimSwish * 0.3 : 0;
    // A blow's flinch, spread over the spine and neck, or mostly the neck for a blow to the head.
    const f = this.flinch;
    const torsoBones = rig.spine.length - 1;
    const flinchAt = (k: number) =>
      (1 - f.focus) / Math.max(1, bendBones.length) +
      (k >= torsoBones ? f.focus / Math.max(1, bendBones.length - torsoBones) : 0);
    const bendAt = (k: number) =>
      this.bend / Math.max(1, bendBones.length) +
      f.x * flinchAt(k) +
      (sprawl ? Math.sin(this.phase * Math.PI * 2 - k * 0.6) * 0.06 * moving : 0) +
      (wave !== 0
        ? (wave / Math.max(1, bendBones.length)) *
          (1 - k / Math.max(1, bendBones.length)) *
          Math.cos(this.phase * Math.PI * 2 + k * 0.7)
        : 0);
    bendBones.forEach((b, k) => {
      // Bends turn about the dorsal axis, which on an upright front is its own long axis.
      const axis = this.uprightFront && k >= rig.spine.length - 1 ? Y_AXIS : Z_AXIS;
      (pose.rot[b] as Quaternion).multiply(scratchQ.setFromAxisAngle(axis, -bendAt(k)));
      if (f.y !== 0)
        (pose.rot[b] as Quaternion).multiply(scratchQ.setFromAxisAngle(X_AXIS, -f.y * flinchAt(k)));
    });
    // A gallop or a bound flexes the back as the hind feet land and extends it as the fore feet
    // do, spread over the torso's bones.
    const flex = this.gait?.flex ?? 0;
    if (flex > 0 && moving > 0 && rig.spine.length > 1) {
      const hind = this.legs.find((l) => l.rig.pair === 0)?.offset ?? 0;
      const angle = flex * 0.12 * moving * Math.cos(2 * Math.PI * (this.phase - hind));
      for (const b of rig.spine.slice(1))
        (pose.rot[b] as Quaternion).multiply(
          scratchQ.setFromAxisAngle(X_AXIS, angle / (rig.spine.length - 1)),
        );
    }
    // Other necks bend with the main one, bone for bone, so the heads turn together.
    rig.heads.forEach((h, i) => {
      if (i === rig.main) return;
      h.neck.forEach((b, j) => {
        const k = rig.spine.length - 1 + j;
        (pose.rot[b] as Quaternion).multiply(
          scratchQ.setFromAxisAngle(new Vector3(0, 0, 1), -bendAt(k)),
        );
      });
    });
    this.solvePose();

    // Heads: keep them level, facing the way it walks (or toward a look target); jaws.
    this.applyHeads();
    // A blow to the head snaps it with the flinch, past what keeps it level.
    if (f.focus > 0 && (f.x !== 0 || f.y !== 0))
      for (const head of rig.heads) {
        const q = pose.rot[head.head] as Quaternion;
        q.multiply(scratchQ.setFromAxisAngle(Z_AXIS, -0.6 * f.focus * f.x));
        q.multiply(scratchQ.setFromAxisAngle(X_AXIS, -0.6 * f.focus * f.y));
        pose.solveBone(head.head);
      }
    this.applyJaw();

    // Legs: IK from the posed hips to the planted feet.
    this.poseLegs(this.air !== null);

    // Arms swing against the legs: on bipeds by the gait's phase; above four or more legs (a
    // centaur) with the foreleg on the other side, as a walking person's arms follow their legs.
    // An action's `arms` raises them forward to reach for what it looks at.
    const armSide = g.nearest ? this.sideOf(g.look) : 0;
    for (const [k, arm] of rig.arms.entries()) {
      // With `nearest`, only the arm on the target's side reaches (one claw pinches).
      const reachArms =
        armSide !== 0 && (arm.side === 'left' ? 1 : -1) !== armSide ? 0 : (g.arms ?? 0);
      const first = arm.bones[0] as number;
      let swing: number;
      const fore = this.legs.length > 2 ? this.foreleg(arm.side === 'left' ? 'right' : 'left') : -1;
      if (fore >= 0) {
        const leg = this.legs[fore] as LegState;
        const hip = leg.points[0] as Vector3;
        const foot = leg.points.at(-1) as Vector3;
        const along =
          (foot.x - hip.x) * Math.sin(this.heading) + (foot.z - hip.z) * Math.cos(this.heading);
        const rest = leg.neutral.z - (pose.restWorldPos[leg.rig.bones[0] as number] as Vector3).z;
        swing = Math.max(-1, Math.min(1, (along - rest) / this.halfStride)) * 0.3 * moving;
      } else {
        swing =
          Math.sin(this.phase * Math.PI * 2 + (arm.side === 'left' ? Math.PI : 0)) * 0.25 * moving;
      }
      (pose.rot[first] as Quaternion).premultiply(
        scratchQ.setFromAxisAngle(
          X_AXIS,
          swing * (1 - reachArms) + (this.armReach[k] ?? 1.3) * reachArms,
        ),
      );
    }

    this.applySprings();
    this.applyHelpers();
    this.applyFins();
    this.solvePose();
    this.applyWings();
  }

  /**
   * Leg IK from the posed hips to each leg's `planted` foot. Knees bend toward their poles in the
   * root's frame, or with `bodyFrame` in the body's own (in the air, or falling over).
   */
  private poseLegs(bodyFrame: boolean): void {
    const pose = this.pose;
    const rig = this.compiled.rig;
    for (const [k, leg] of this.legs.entries()) {
      const first = leg.rig.bones[0] as number;
      pose.solveBone(first);
      const hip = pose.worldPos[first] as Vector3;
      // In the air the knees bend in the body's frame, banked and pitched with it.
      const pole = scratch1
        .copy(leg.pole)
        .applyQuaternion(
          bodyFrame
            ? scratchQ4
                .copy(pose.worldRot[rig.spine[0] as number] as Quaternion)
                .multiply(
                  scratchQ5.copy(pose.restWorldRot[rig.spine[0] as number] as Quaternion).invert(),
                )
            : (pose.worldRot[rig.root] as Quaternion),
        );
      this.legMiss[k] = solvePrepared(leg.limb, hip, leg.planted, pole, leg.points);
      leg.rig.bones.forEach((b, k) => {
        pose.aim(b, scratch2.subVectors(leg.points[k + 1] as Vector3, leg.points[k] as Vector3));
      });
      // Toes stay flat, turned with the body; a rolling foot's toes stay on their planted tips,
      // letting go early in the swing.
      for (const toe of leg.rig.toes) {
        for (const b of toe) {
          pose.solveBone(b);
          const restDir = scratch2.set(0, 1, 0).applyQuaternion(pose.restWorldRot[b] as Quaternion);
          restDir.applyAxisAngle(UP, this.heading);
          pose.aim(b, restDir);
        }
      }
      if (leg.roll) {
        const hold = leg.swinging ? 1 - Math.min(1, leg.swingU / 0.25) : 1;
        if (hold > 0) poseToes(leg.roll, pose, this.heading, hold * hold * (3 - 2 * hold));
      }
    }
  }

  /**
   * Fins and flippers beat up and down about the body's long axis while it swims, hard for a
   * flipper stroke, gently for a fish's steering fins (docs/design/10.3-swimming.md).
   */
  private applyFins(): void {
    if (this.finBeat === 0 || this.medium !== 'water') return;
    const pose = this.pose;
    for (const fin of this.compiled.rig.fins) {
      const first = fin.bones[0] as number;
      const parent = pose.parents[first] as number;
      if (parent < 0) continue;
      pose.solveBone(parent);
      // The body's long axis in the parent's frame.
      const axis = scratch1
        .set(Math.sin(this.heading), 0, Math.cos(this.heading))
        .applyQuaternion(scratchQ2.copy(pose.worldRot[parent] as Quaternion).invert())
        .normalize();
      const side = fin.side === 'left' ? 1 : fin.side === 'right' ? -1 : 0;
      if (side === 0) continue;
      (pose.rot[first] as Quaternion).premultiply(
        scratchQ.setFromAxisAngle(axis, side * this.finBeat),
      );
    }
  }

  /**
   * Wings from folded toward spread by the damped `wings` goal, the shoulders lifting a little
   * with each breath, then the membranes' stations (docs/design/9.3-wings-fins.md).
   */
  private applyWings(): void {
    const rig = this.compiled.rig;
    const pose = this.pose;
    if (rig.wings.length === 0) {
      if (rig.stations.length > 0) applyStations(pose, rig.stations);
      return;
    }
    applyWings(pose, rig.wings, this.spread);
    // In flight the spread wings beat (docs/design/10.4-flight.md); their elbow and wrist
    // helpers follow the stroke.
    const st = this.strokes;
    if (this.air || st.amplitude + st.raise + st.hold > 1e-3) {
      applyStrokes(pose, rig.wings, rig.spine[0] as number, st);
      this.applyHelpers(this.wingHelpers);
    }
    const lift = (this.goals.breath ?? 0) * 2 * (Math.PI / 180) * (1 - this.spread);
    if (lift > 0)
      for (const wing of rig.wings) {
        const side = wing.side === 'left' ? 1 : wing.side === 'right' ? -1 : 0;
        const humerus = wing.bones[0] as number;
        (pose.rot[humerus] as Quaternion).premultiply(
          scratchQ.setFromAxisAngle(Y_AXIS, side * lift),
        );
      }
    for (const b of this.wingBones) pose.solveBone(b);
    applyStations(pose, rig.stations);
  }

  /**
   * Forward kinematics for every bone but the membranes' stations, which `applyStations` poses
   * and solves last (docs/design/9.3-wings-fins.md).
   */
  private solvePose(): void {
    if (this.bodyTree === undefined) {
      const sections = this.compiled.bones.sections;
      const tree: number[] = [];
      for (let b = 0; b < sections.length; b++) if (sections[b] !== 'station') tree.push(b);
      this.bodyTree = tree.length === sections.length ? null : tree;
    }
    if (this.bodyTree === null) this.pose.solve();
    else for (const b of this.bodyTree) this.pose.solveBone(b);
  }

  /** Helper bones at the wings' joints, posed again after the strokes. */
  private get wingHelpers(): readonly (readonly [number, number, number])[] {
    if (this.wingHelperList) return this.wingHelperList;
    const wing = new Set(this.wingBones);
    this.wingHelperList = this.compiled.rig.helpers.filter(([, , lower]) => wing.has(lower));
    return this.wingHelperList;
  }
  private wingHelperList: readonly (readonly [number, number, number])[] | undefined;

  /**
   * From the body's first bone to the middle of its wing roots, at rest, in the root's frame:
   * what flight pitches and rolls about.
   */
  private get wingPivot(): Vector3 {
    if (this.pivot) return this.pivot;
    const pose = this.pose;
    const spine0 = this.compiled.rig.spine[0] as number;
    const roots = this.compiled.rig.wings.map(
      (w) => pose.restWorldPos[w.bones[0] as number] as Vector3,
    );
    const mid = roots
      .reduce((sum, p) => sum.add(p), new Vector3())
      .divideScalar(Math.max(1, roots.length));
    this.pivot = mid.sub(pose.restWorldPos[spine0] as Vector3);
    return this.pivot;
  }
  private pivot: Vector3 | undefined;

  /** Every bone hanging from a wing's shoulder except the stations, in solving order. */
  private get wingBones(): readonly number[] {
    if (this.wingTree) return this.wingTree;
    const bones = this.compiled.bones;
    const roots = new Set(this.compiled.rig.wings.map((w) => w.bones[0] as number));
    const tree: number[] = [];
    for (let b = 0; b < bones.names.length; b++) {
      if (bones.sections[b] === 'station') continue;
      for (let x = b, n = 0; x >= 0 && n < 64; x = bones.parents[x] ?? -1, n++)
        if (roots.has(x)) {
          tree.push(b);
          break;
        }
    }
    this.wingTree = tree;
    return tree;
  }

  /**
   * Spreads the wings (1) or folds them (0) when no action asks otherwise; they move there over
   * about 0.4 s.
   */
  setWings(spread: number): void {
    this.wingGoal = Math.max(0, Math.min(1, spread));
  }

  /** How far the wings are spread now, 0 folded to 1. */
  get wingSpread(): number {
    return this.spread;
  }

  /** Index (in `legs`) of the frontmost leg on a side, or -1. */
  private foreleg(side: 'left' | 'right'): number {
    let best = -1;
    this.legs.forEach((leg, i) => {
      if (leg.rig.side !== side) return;
      if (best < 0 || leg.rig.pair > (this.legs[best] as LegState).rig.pair) best = i;
    });
    return best;
  }

  /** Index of the head nearest a point (the main head without one). */
  private nearestHead(point: Vector3 | null): number {
    const rig = this.compiled.rig;
    let best = rig.main;
    if (!point || rig.heads.length < 2) return best;
    let closest = Infinity;
    rig.heads.forEach((h, i) => {
      const d = (this.pose.worldPos[h.head] as Vector3).distanceToSquared(point);
      if (d < closest) {
        closest = d;
        best = i;
      }
    });
    return best;
  }

  private applyHeads(): void {
    // Every head turns to a look; only the nearest one lunges at it.
    const reacher = this.nearestHead(this.goals.look ?? this.lookTarget);
    this.compiled.rig.heads.forEach((h, i) => {
      this.applyHead(h, i === reacher, this.glanceFor(i));
    });
  }

  private applyHead(
    h: HeadRig,
    reaches: boolean,
    glance: { yaw: number; pitch: number } | undefined,
  ): void {
    const pose = this.pose;
    const g = this.goals;
    const head = h.head;
    const restDir = scratch1.set(0, 1, 0).applyQuaternion(pose.restWorldRot[head] as Quaternion);
    const desired = scratch5.copy(restDir).applyAxisAngle(UP, this.heading);
    const drop = TEMPERAMENTS[this.motion.temperament].headDrop;
    if (drop) desired.y -= drop;
    desired.normalize();
    // Glances (look-around), then a look target, then raising and shaking.
    const side = scratch6.crossVectors(desired, UP);
    if (side.lengthSq() < 1e-8) side.set(Math.cos(this.heading), 0, -Math.sin(this.heading));
    side.normalize();
    if (glance) {
      desired.applyAxisAngle(side, glance.pitch).applyAxisAngle(UP, glance.yaw);
    }
    const look = g.look ?? this.lookTarget;
    const weight = g.look ? (g.lookWeight ?? 1) : 1;
    const from = pose.worldPos[head] as Vector3;
    if (look && weight > 0) {
      const to = scratch2.subVectors(look, from).normalize();
      // Limit how far the head turns from the body's facing.
      const facing = scratch4.set(Math.sin(this.heading), 0, Math.cos(this.heading));
      const flat = scratch7.set(to.x, 0, to.z);
      const angle = flat.lengthSq() > 1e-8 ? facing.angleTo(flat) : 0;
      const limit = (100 * Math.PI) / 180;
      if (angle > limit) to.lerp(desired, 1 - limit / angle).normalize();
      desired.lerp(to, weight).normalize();
    }
    if (g.raise) desired.applyAxisAngle(side, g.raise);
    if (g.shake)
      desired.applyAxisAngle(UP, Math.sin((this.time * Math.PI * 14) / this.timeScale) * g.shake);
    desired.normalize();
    // Turn the neck by part of the rotation the head needs, keeping its shape (a raised neck
    // stays raised), then aim the head.
    const neck = h.neck;
    pose.solveBone(head);
    const turn = scratchQ2.setFromUnitVectors(pose.direction(head, scratch1), desired);
    neck.forEach((b, k) => {
      pose.solveBone(b);
      const share = (k + 1) / (neck.length + 1) / neck.length;
      const part = scratchQ.identity().slerp(turn, share);
      pose.aim(b, pose.direction(b, scratch2).applyQuaternion(part));
    });
    pose.solveBone(head);
    pose.aim(head, desired);
    pose.solveSubtree(head);
    if (g.reach && look && reaches) this.lunge(look, g.reach, desired, h);
  }

  /**
   * Stretches the neck (or the front of the spine) so the head moves toward `point` by `amount`
   * of what it can reach, then re-aims the head (cyclic coordinate descent).
   */
  private lunge(point: Vector3, amount: number, facing: Vector3, h: HeadRig): void {
    const pose = this.pose;
    const rig = this.compiled.rig;
    const chain = h.neck.length > 0 ? h.neck : rig.spine.slice(-2);
    let length = 0;
    for (const b of chain) length += pose.lengths[b] as number;
    const start = pose.worldPos[h.head] as Vector3;
    const toward = scratch2.subVectors(point, start);
    const distance = toward.length();
    if (distance < 1e-6) return;
    const goal = scratch7
      .copy(start)
      .addScaledVector(toward, (Math.min(distance * 0.9, length * 0.6) * amount) / distance);
    for (let iteration = 0; iteration < 3; iteration++) {
      for (let k = chain.length - 1; k >= 0; k--) {
        const b = chain[k] as number;
        const base = pose.worldPos[b] as Vector3;
        const a = scratch1.subVectors(pose.worldPos[h.head] as Vector3, base).normalize();
        const c = scratch4.subVectors(goal, base).normalize();
        const turn = scratchQ.setFromUnitVectors(a, c);
        scratchQ2.identity().slerp(turn, 0.6);
        pose.aim(b, pose.direction(b, scratch6).applyQuaternion(scratchQ2));
        pose.solveSubtree(b);
      }
    }
    pose.aim(h.head, facing);
    pose.solveSubtree(h.head);
  }

  /** Opens every jaw by the action's `jaw` goal, about its hinge, and blinks the eyelids. */
  private applyJaw(): void {
    this.pose.breath = this.goals.breath ?? 0;
    this.pose.time = this.time;
    applyFace(
      this.pose,
      this.compiled.rig,
      this.goals.jaw ?? 0,
      this.goals.blink ?? 0,
      [this.gripLeft, this.gripRight],
      this.flare,
    );
  }

  /** Aims each spring chain's bones along its points, except chains starting in `skip`. */
  private applySprings(skip: readonly number[] = []): void {
    // A tail kept off the floor (in the air and through a landing's absorb) is kept off it as
    // posed too: the body may have moved since its spring stepped.
    const floored = this.air !== null || this.landedAt !== undefined || this.death !== null;
    const ground = this.groundNow;
    const height = (x: number, z: number) => ground(x, z).height;
    for (const spring of this.springs) {
      if (skip.includes(spring.bones[0] as number)) continue;
      const floors = floored && spring.kind === 'tail' ? spring.floors : undefined;
      spring.bones.forEach((b, i) => {
        const dir = scratch1.subVectors(
          spring.points[i + 1] as Vector3,
          spring.points[i] as Vector3,
        );
        // `aim` solves the bone itself.
        if (dir.lengthSq() > 1e-12) this.pose.aim(b, dir);
        else this.pose.solveBone(b);
        // Dead, everything a spring bone carries (spikes, a stinger) stays out of the ground.
        if (this.death && this.hulls) {
          const depth = hullDepth(this.hulls, this.pose, b, height);
          if (depth > 0) this.raiseEnd(b, spring.lengths[i] as number, depth);
          return;
        }
        if (!floors || !spring.clearance) return;
        const k = (i + 1) * 5;
        const x0 = floors[k] as number;
        if (Number.isNaN(x0)) return;
        const end = this.pose.tail(b, scratch2);
        const floor =
          (floors[k + 2] as number) +
          (floors[k + 3] as number) * (end.x - x0) +
          (floors[k + 4] as number) * (end.z - (floors[k + 1] as number)) +
          (spring.clearance[i + 1] as number);
        if (end.y >= floor) return;
        // Swing the bone up about its head until its end is on the floor, keeping its length.
        const head = this.pose.worldPos[b] as Vector3;
        const len = spring.lengths[i] as number;
        const rise = Math.min(len, floor - head.y);
        const flat = scratch1.set(end.x - head.x, 0, end.z - head.z);
        if (flat.lengthSq() < 1e-12) flat.set(Math.sin(this.heading), 0, Math.cos(this.heading));
        flat.setLength(Math.sqrt(Math.max(0, len * len - rise * rise)));
        this.pose.aim(b, flat.setY(rise));
      });
    }
  }

  private applyHelpers(helpers = this.compiled.rig.helpers): void {
    const pose = this.pose;
    for (const [helper, , lower] of helpers) {
      const delta = scratchQ
        .copy(pose.rot[lower] as Quaternion)
        .multiply(scratchQ2.copy(pose.restRot[lower] as Quaternion).invert());
      const half = scratchQ2.identity().slerp(delta, 0.5);
      (pose.rot[helper] as Quaternion).multiplyQuaternions(
        half,
        pose.restRot[helper] as Quaternion,
      );
    }
  }

  private applySlither(ground: Ground): void {
    const pose = this.pose;
    const rig = this.compiled.rig;
    // Bones from the head end back: neck (reversed), torso (reversed), tail. A rearing neck (a
    // cobra) keeps its raised pose on the front of the body instead of following the trail.
    const main = mainHead(rig);
    const lead = this.rearing ? [] : [...main.neck].reverse();
    const chain = [...lead, ...[...rig.spine].reverse(), ...this.tailBones];
    if (this.trail.length < 2) {
      this.solvePose();
      return;
    }
    // Place each bone joint along the trail, measured from the head.
    // Joints at increasing distances along the trail, in one walk down it. The trail was laid on
    // the ground, so a joint on it takes its height; one past its end samples the ground.
    const joints = this.slitherJoints;
    while (joints.length < chain.length + 1) joints.push(new Vector3());
    const trail = this.trail;
    const bodyY = (pose.restWorldPos[rig.spine[0] as number] as Vector3).y;
    let segment = 1;
    let acc = 0;
    let distance = 0;
    for (let j = 0; j <= chain.length; j++) {
      if (j > 0) distance += pose.lengths[chain[j - 1] as number] as number;
      const out = joints[j] as Vector3;
      let placed = false;
      while (segment < trail.length) {
        const a = trail[segment - 1] as Vector3;
        const b = trail[segment] as Vector3;
        const length = a.distanceTo(b);
        if (acc + length >= distance) {
          out.lerpVectors(a, b, (distance - acc) / (length || 1));
          placed = true;
          break;
        }
        acc += length;
        segment++;
      }
      if (!placed) {
        // Past the end of the trail: continue straight back.
        const end = trail.at(-1) as Vector3;
        const before = trail.at(-2) as Vector3;
        out
          .subVectors(end, before)
          .setLength(distance - acc)
          .add(end);
        out.y = ground(out.x, out.z).height;
      }
      out.y += bodyY;
    }
    joints.length = chain.length + 1;
    // Torso and neck bones point toward the head; tail bones away from it.
    const spine0 = rig.spine[0] as number;
    const hipIndex = lead.length + rig.spine.length;
    const hip = joints[hipIndex] as Vector3;
    (pose.pos[spine0] as Vector3)
      .copy(hip)
      .sub(this.position)
      .applyQuaternion(scratchQ.copy(pose.rot[rig.root] as Quaternion).invert());
    this.solvePose();
    [...rig.spine].forEach((b, k) => {
      const from = joints[hipIndex - k] as Vector3;
      const to = joints[hipIndex - k - 1] as Vector3;
      pose.solveBone(b);
      pose.aim(b, scratch1.subVectors(to, from));
    });
    lead
      .slice()
      .reverse()
      .forEach((b, k) => {
        const from = joints[lead.length - k] as Vector3;
        const to = joints[lead.length - k - 1] as Vector3;
        pose.solveBone(b);
        pose.aim(b, scratch1.subVectors(to, from));
      });
    pose.solveSubtree(main.head);
    this.applyHeads();
    this.applyJaw();
    this.tailBones.forEach((b, k) => {
      const from = joints[hipIndex + k] as Vector3;
      const to = joints[hipIndex + k + 1] as Vector3;
      pose.solveBone(b);
      pose.aim(b, scratch1.subVectors(to, from));
    });
    this.solvePose();
  }
}

/** Where a leg's foot lands in a gait's cycle: the gait's own phases, else the wave formula. */
function offsetFor(rig: LegRigData, gait: GaitInfo | undefined): number {
  const s = rig.side === 'left' ? 0 : 1;
  const own = gait?.phases?.[2 * rig.pair + s];
  if (own !== undefined) return own;
  const wave = gait?.wave ?? 0.25;
  return (((rig.pair * wave + 0.5 * s) % 1) + 1) % 1;
}

/** A gait setting at a Froude number: `slow` to `fast` across the gait's range (10.1). */
function profileAt(gait: GaitInfo, slow: number, fast: number | undefined, froude: number): number {
  if (fast === undefined || fast === slow) return slow;
  const [lo, hi] = gait.froude;
  const t = hi > lo ? Math.max(0, Math.min(1, (froude - lo) / (hi - lo))) : 0;
  return slow + (fast - slow) * t;
}

/** The highest Froude number a gait is used at: flight gaits their range's top, others 1.5. */
function gaitTop(gait: GaitInfo): number {
  return gait.flight ? gait.froude[1] : Math.min(gait.froude[1], 1.5);
}

/** Smoothstep of a value clamped to 0..1. */
function smooth(x: number): number {
  const t = Math.max(0, Math.min(1, x));
  return t * t * (3 - 2 * t);
}

function wrapAngle(a: number): number {
  return Math.atan2(Math.sin(a), Math.cos(a));
}

/** In the air the floor is the higher of the ground and any water over it. */
/** Convex hull of 2D points (Andrew's monotone chain), counter-clockwise. */
function convexHull2(points: [number, number][]): [number, number][] {
  if (points.length < 3) return points;
  const sorted = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const half = (list: [number, number][]) => {
    const out: [number, number][] = [];
    for (const p of list) {
      while (
        out.length >= 2 &&
        cross(out.at(-2) as [number, number], out.at(-1) as [number, number], p) <= 0
      )
        out.pop();
      out.push(p);
    }
    return out;
  };
  const lower = half(sorted);
  const upper = half([...sorted].reverse());
  return [...lower.slice(0, -1), ...upper.slice(0, -1)];
}

/** Distance from (x, z) to a convex polygon, segment or point (0 inside a polygon). */
function distanceToHull(hull: readonly [number, number][], x: number, z: number): number {
  if (hull.length >= 3) {
    let inside = true;
    for (let i = 0; i < hull.length; i++) {
      const a = hull[i] as [number, number];
      const b = hull[(i + 1) % hull.length] as [number, number];
      if ((b[0] - a[0]) * (z - a[1]) - (b[1] - a[1]) * (x - a[0]) < 0) inside = false;
    }
    if (inside) return 0;
  }
  let best = Infinity;
  for (let i = 0; i < hull.length; i++) {
    const a = hull[i] as [number, number];
    const b = hull[(i + 1) % hull.length] as [number, number];
    const dx = b[0] - a[0];
    const dz = b[1] - a[1];
    const t = Math.max(
      0,
      Math.min(1, ((x - a[0]) * dx + (z - a[1]) * dz) / (dx * dx + dz * dz || 1)),
    );
    best = Math.min(best, Math.hypot(x - a[0] - t * dx, z - a[1] - t * dz));
  }
  return best;
}

function airFloor(ground: Ground, water: Water | undefined): Ground {
  if (!water) return ground;
  return (x, z) => {
    const g = ground(x, z);
    const w = water(x, z);
    return w && w.surface > g.height ? { height: w.surface } : g;
  };
}

const scratch1 = new Vector3();
const scratch2 = new Vector3();
const scratchQ = new Quaternion();
const scratchQ2 = new Quaternion();
const scratchQ3 = new Quaternion();
const scratchQ4 = new Quaternion();
const scratchQ5 = new Quaternion();
const scratch3 = new Vector3();
const scratch4 = new Vector3();
const IDENTITY = new Quaternion();
const scratch5 = new Vector3();
const scratch6 = new Vector3();
const scratch7 = new Vector3();
const scratchEuler = new Euler();
