import { Vector3 } from 'three';
import { z } from 'zod';
import { formatPath, fromZodIssues, type Issue } from '../blueprint/issues.ts';
import type { CompiledCreature } from '../compile/compile.ts';
import type { ActionModule, Registry } from '../registry.ts';
import { type Ground, MotionController, type MotionEvent, type Water } from './controller.ts';
import { openSea, slope, testCourse, withLake } from './terrain.ts';

/**
 * Scenarios script a creature's motion for `render` and `analyze`: the ground, named targets,
 * where it starts and timed calls (walk to a point, follow a course, start an action, look
 * at something), so an agent can check motion with only the CLI. Positions are metres in the
 * world (glTF: Y up, +Z ahead of a creature with heading 0); `analyze` gives the creature's
 * size to place them by.
 */

const range = (min: number, max: number) => z.number().min(min).max(max);
const time = range(0, 120).describe('Seconds from the start');
const point2 = z
  .tuple([range(-1000, 1000), range(-1000, 1000)])
  .describe('A point on the ground in metres, [x, z]');
const point3 = z
  .tuple([range(-1000, 1000), range(-100, 100), range(-1000, 1000)])
  .describe('A point in metres, [x, y, z]');
const NAME = /^[a-z][a-z0-9]*([.-][a-z0-9]+)*$/;
const name = z
  .string()
  .regex(NAME, 'names are lowercase words joined by dots or dashes')
  .describe("The name of one of the scenario's targets");
const where2 = z.union([point2, name]);
const where3 = z.union([point3, name]);
/** Where `moveTo` and `follow` go: on the ground, or at a height (a diver or a flyer). */
const whereAny = z.union([point2, point3, name]);
const speed = range(0, 60).describe(
  'Metres a second (default: its pace on land, in water or in the air)',
);

/** Where a blow comes from: a side of the creature, or degrees from its facing. */
const from = z
  .union([z.enum(['left', 'right', 'front', 'back']), range(-360, 360)])
  .describe('Where the blow comes from: a side, or degrees from its facing (0 ahead, 90 its left)');

const call = z.discriminatedUnion('do', [
  z
    .strictObject({ at: time, do: z.literal('moveTo'), to: whereAny, speed: speed.optional() })
    .describe(
      'Go to a point and stop there: [x, z] on the ground, or [x, y, z] (or a target) at a height a swimmer dives to or a flyer flies to',
    ),
  z
    .strictObject({
      at: time,
      do: z.literal('follow'),
      path: z.array(whereAny).min(1).max(32),
      speed: speed.optional(),
    })
    .describe('Go through the points in order, stopping at the last: a course, on foot or flying'),
  z
    .strictObject({
      at: time,
      do: z.literal('fly'),
      height: range(0.5, 200).optional().describe('Metres above the ground (default: its own)'),
      speed: speed.optional(),
    })
    .describe('Take off (a creature with wings) and fly, circling or hovering until told where'),
  z
    .strictObject({
      at: time,
      do: z.literal('land'),
      to: where2.optional().describe('Where to touch down (default: the first clear ground ahead)'),
    })
    .describe('Land: approach, flare and touch down'),
  z
    .strictObject({
      at: time,
      do: z.literal('drive'),
      speed: range(0, 30).describe('Metres a second'),
      heading: range(-360, 360).optional().describe('Degrees: 0 is +Z, 90 is +X'),
    })
    .describe('Keep moving at a speed (and heading) with no destination'),
  z.strictObject({ at: time, do: z.literal('stop') }).describe('Stop moving'),
  z
    .strictObject({
      at: time,
      do: z.literal('hit'),
      from: from.default('left'),
      strength: range(0, 1)
        .default(0.5)
        .describe('0 a tap, 1 a heavy blow that staggers nearly anything'),
      bone: z
        .string()
        .optional()
        .describe("The bone it lands on, as hit capsules name them (default the torso's middle)"),
    })
    .describe('A blow: it flinches, and staggers if the blow would knock it over'),
  z
    .strictObject({ at: time, do: z.literal('die'), from: from.default('right') })
    .describe('It dies and collapses, falling away from the blow'),
  z
    .strictObject({
      at: time,
      do: z.literal('act'),
      action: z.string().describe("One of the creature's actions, e.g. bite"),
      target: where3.optional(),
    })
    .describe('Start an action, aimed at a point or target'),
  z
    .strictObject({ at: time, do: z.literal('lookAt'), target: z.union([where3, z.null()]) })
    .describe('Turn the head toward a point or target; null looks ahead again'),
  z
    .strictObject({
      at: time,
      do: z.literal('gait'),
      gait: z.union([z.string(), z.null()]).describe("One of the creature's gaits, or null"),
    })
    .describe('Keep to one gait whatever the speed; null lets speed choose again'),
]);

export const ScenarioSchema = z.strictObject({
  ground: z
    .union([
      z.enum(['flat', 'course']),
      z
        .strictObject({
          slope: range(-40, 40).describe('Degrees the ground rises'),
          toward: range(-360, 360).default(0).describe('Degrees: the way it rises, 0 is +Z'),
          from: range(0, 1000).default(0).describe('Metres that way it stays flat first'),
        })
        .describe('Ground rising at `slope` degrees (through the origin, or flat until `from`)'),
    ])
    .default('flat')
    .describe(
      '"flat"; "course": uneven ground with bumps up to 25 cm, flat within 1.5 m of the origin; or a slope',
    ),
  seed: z.number().int().min(0).max(2147483647).default(1).describe("Seed of the course's bumps"),
  water: z
    .union([
      z.enum(['none', 'sea']),
      z
        .strictObject({
          x: range(-1000, 1000).default(0).describe('Centre, metres'),
          z: range(-1000, 1000).default(6).describe('Centre, metres'),
          radius: range(0.5, 200).default(4).describe('Metres to the shore'),
          depth: range(0.1, 50).default(1.5).describe('Metres of water at the middle'),
        })
        .describe('A round lake carved into the ground, its surface at height 0'),
    ])
    .default('none')
    .describe(
      '"none", "sea" (deep water everywhere: the bed 4 body lengths and 2 m down, the surface at 0), or a lake',
    ),
  duration: range(0.5, 120).default(6).describe('Seconds to run'),
  start: z
    .strictObject({
      x: range(-1000, 1000).default(0).describe('Metres'),
      z: range(-1000, 1000).default(0).describe('Metres'),
      heading: range(-360, 360).default(0).describe('Degrees: 0 faces +Z, 90 faces +X'),
      flying: z
        .boolean()
        .default(false)
        .describe('Start in the air at cruise (a creature with wings)'),
      height: range(0.5, 200).optional().describe('Metres above the ground when it starts flying'),
      y: range(-1000, 1000)
        .optional()
        .describe(
          "Metres: the height it starts at in the world, for a swimmer (the sea's surface is 0) or a flyer; overrides height",
        ),
    })
    .default({ x: 0, z: 0, heading: 0, flying: false }),
  targets: z
    .record(z.string().regex(NAME, 'names are lowercase words joined by dots or dashes'), point3)
    .default({})
    .describe('Named points calls can aim at, e.g. { "prey": [0, 0.4, 2] }; renders mark them'),
  calls: z.array(call).max(64).default([]).describe('What happens when, in any order'),
  frames: z
    .number()
    .int()
    .min(2)
    .max(16)
    .default(8)
    .describe('Frames in the filmstrip, evenly spaced over the duration'),
});

export type Scenario = z.output<typeof ScenarioSchema>;
export type ScenarioCall = Scenario['calls'][number];

/** Parses a scenario; on failure, issues with paths, ranges and fixes as blueprints get. */
export function parseScenario(input: unknown): { scenario?: Scenario; issues: Issue[] } {
  const parsed = ScenarioSchema.safeParse(input);
  if (parsed.success) {
    const issues = checkNames(parsed.data);
    return issues.some((i) => i.severity === 'error')
      ? { issues }
      : { scenario: parsed.data, issues };
  }
  return {
    issues: fromZodIssues(parsed.error.issues, ScenarioSchema, input, (path) =>
      formatPath(path, input),
    ),
  };
}

/** Every name a call uses must be a target; calls past the end never happen. */
function checkNames(scenario: Scenario): Issue[] {
  const issues: Issue[] = [];
  const names = Object.keys(scenario.targets);
  const check = (where: unknown, path: string) => {
    if (typeof where === 'string' && !names.includes(where))
      issues.push({
        severity: 'error',
        path,
        code: 'unknown_target',
        message: `no target named "${where}"`,
        ...(names.length > 0 ? { expected: names.map((n) => `"${n}"`).join(', ') } : {}),
        fix:
          names.length > 0
            ? `use one of the targets, or add "${where}" to "targets"`
            : `add "targets": { "${where}": [x, y, z] }, or give the point itself`,
      });
  };
  scenario.calls.forEach((c, i) => {
    const at = `calls[${i}]`;
    if (c.do === 'moveTo') check(c.to, `${at}.to`);
    if (c.do === 'follow') for (const [k, p] of c.path.entries()) check(p, `${at}.path[${k}]`);
    if ((c.do === 'act' || c.do === 'lookAt') && c.target !== undefined)
      check(c.target, `${at}.target`);
    if (c.at > scenario.duration)
      issues.push({
        severity: 'warning',
        path: `${at}.at`,
        code: 'after_end',
        message: `${c.at} s is after the scenario ends (${scenario.duration} s), so it never happens`,
        fix: `raise "duration" above ${c.at}, or call it earlier`,
      });
  });
  return issues;
}

/**
 * Checks a scenario against a creature: the actions it can start (not those that run by
 * themselves, such as idle) and its gaits.
 */
export function checkScenario(
  scenario: Scenario,
  motion: {
    readonly gaits: readonly { id: string; medium?: string }[];
    readonly actions: readonly { id: string }[];
  },
  registry: Registry,
): Issue[] {
  const issues: Issue[] = [];
  const ambient = (id: string) =>
    (registry.get('action', id) as ActionModule | undefined)?.hooks?.ambient === true;
  const actions = motion.actions.map((a) => a.id).filter((id) => !ambient(id));
  const gaits = motion.gaits.map((g) => g.id);
  scenario.calls.forEach((c, i) => {
    if (c.do === 'act' && !actions.includes(c.action))
      issues.push({
        severity: 'error',
        path: `calls[${i}].action`,
        code: 'unknown_action',
        message: `"${c.action}" is not one the creature can start`,
        expected: actions.map((a) => `"${a}"`).join(', ') || 'none',
        fix:
          actions.length > 0
            ? `use one of its actions, or add "${c.action}" to the blueprint's motion.actions`
            : `add "${c.action}" to the blueprint's motion.actions`,
      });
    if (c.do === 'gait' && c.gait !== null && !gaits.includes(c.gait))
      issues.push({
        severity: 'error',
        path: `calls[${i}].gait`,
        code: 'unknown_gait',
        message: `"${c.gait}" is not one of the creature's gaits`,
        expected: gaits.map((g) => `"${g}"`).join(', ') || 'none',
        fix: 'use one of its gaits, or null to let speed choose',
      });
    if ((c.do === 'fly' || c.do === 'land') && !motion.gaits.some((g) => g.medium === 'air'))
      issues.push({
        severity: 'error',
        path: `calls[${i}].do`,
        code: 'cannot_fly',
        message: `"${c.do}" needs a creature that flies, and this one has no air gait`,
        expected: 'a creature with wings and an air gait',
        fix: 'give it wings, or take the call out',
      });
  });
  return issues;
}

/** What a run of a scenario measured. */
export interface ScenarioResult {
  readonly duration: number;
  /** Where the creature ended: metres, heading in degrees, speed in m/s. */
  readonly end: {
    readonly x: number;
    readonly z: number;
    readonly heading: number;
    readonly speed: number;
  };
  /** Metres walked along the ground. */
  readonly distance: number;
  /** The fastest it went (m/s). */
  readonly topSpeed: number;
  /**
   * The middle of its torso (metres): the lowest and highest it went in the world (below 0 is
   * under a sea's surface) and the most it rose above the ground under it (a flyer's altitude).
   */
  readonly body: {
    readonly lowest: number;
    readonly highest: number;
    readonly aboveGround: number;
  };
  /** Degrees it turned in all, left and right alike. */
  readonly turned: number;
  /** Events other than footsteps, with seconds from the start. */
  readonly events: readonly {
    readonly type: string;
    readonly time: number;
    readonly action?: string;
    readonly gait?: string;
    readonly medium?: string;
    /** The head that acted (with several), or the bone a blow landed on. */
    readonly head?: string;
    readonly bone?: string;
    readonly position?: readonly [number, number, number];
  }[];
  readonly footsteps: number;
  /** Gaits in the order used, with when each began. */
  readonly gaits: readonly { readonly gait: string; readonly from: number }[];
  /** Per target: the closest any snout came (metres) and when. */
  readonly targets: Readonly<Record<string, { readonly closest: number; readonly time: number }>>;
  /** `follow` calls: how many of their points it reached. */
  readonly courses: readonly {
    readonly call: number;
    readonly reached: number;
    readonly of: number;
  }[];
  /** Largest distance a planted foot slid (metres), leaving out staggers and dying. */
  readonly footSlide: number;
  /** Calls the creature refused, with why. */
  readonly failed: readonly { readonly call: number; readonly reason: string }[];
}

const STEP = 1 / 120;
type Aim = { x: number; y?: number; z: number };
const DEG = Math.PI / 180;

/**
 * Runs a scenario step by step, so a renderer can draw frames between steps. Deterministic:
 * the same creature and scenario give the same result.
 */
export class ScenarioRun {
  readonly controller: MotionController;
  readonly ground: Ground;
  /** The water, when the scenario has some. */
  readonly water: Water | undefined;
  readonly scenario: Scenario;
  time = 0;
  private readonly compiled: CompiledCreature;
  private readonly pending: { call: ScenarioCall; index: number }[];
  private readonly targets: Map<string, Vector3>;
  private course: { index: number; points: Aim[]; next: number; speed?: number } | null = null;
  private readonly courses: { call: number; reached: number; of: number }[] = [];
  private readonly events: ScenarioResult['events'][number][] = [];
  private readonly gaits: { gait: string; from: number }[] = [];
  private readonly closest = new Map<string, { closest: number; time: number }>();
  private readonly anchors = new Map<number, Vector3>();
  private readonly failed: { call: number; reason: string }[] = [];
  private footsteps = 0;
  private footSlide = 0;
  private distance = 0;
  private topSpeed = 0;
  private lowest = Infinity;
  private highest = -Infinity;
  private aboveGround = -Infinity;
  private turned = 0;
  private lastHeading = 0;
  private readonly last = new Vector3();

  constructor(compiled: CompiledCreature, registry: Registry, scenario: Scenario) {
    this.compiled = compiled;
    this.scenario = scenario;
    const land: Ground =
      scenario.ground === 'course'
        ? testCourse(scenario.seed)
        : typeof scenario.ground === 'object'
          ? slope(scenario.ground.slope, scenario.ground.toward, scenario.ground.from)
          : () => ({ height: 0 });
    const water = scenario.water;
    if (water === 'sea') {
      const sea = openSea(compiled.scale);
      this.ground = sea.ground;
      this.water = sea.water;
    } else if (typeof water === 'object') {
      const lake = withLake(land, water);
      this.ground = lake.ground;
      this.water = lake.water;
    } else {
      this.ground = land;
      this.water = undefined;
    }
    this.controller = new MotionController(compiled, { registry });
    const { x, z, heading, flying, height, y } = scenario.start;
    this.controller.place(x, z, heading * DEG, this.ground, this.water, {
      flying,
      ...(y !== undefined
        ? { y }
        : height === undefined
          ? {}
          : { y: this.ground(x, z).height + height }),
    });
    this.last.copy(this.controller.position);
    this.lastHeading = this.controller.heading;
    this.targets = new Map(
      Object.entries(scenario.targets).map(([n, p]) => [n, new Vector3(p[0], p[1], p[2])]),
    );
    // Calls in time order; ties keep the file's order.
    this.pending = scenario.calls
      .map((call, index) => ({ call, index }))
      .sort((a, b) => a.call.at - b.call.at || a.index - b.index);
    this.fire();
    this.measure();
  }

  get done(): boolean {
    return this.time >= this.scenario.duration - 1e-9;
  }

  /** Target positions by name (metres), for drawing markers. */
  targetPoints(): ReadonlyMap<string, Vector3> {
    return this.targets;
  }

  /** Advances one fixed step (1/120 s); returns the events it fired. */
  step(): MotionEvent[] {
    if (this.done) return [];
    const events = this.controller.update(STEP, {
      ground: this.ground,
      ...(this.water ? { water: this.water } : {}),
    });
    this.time = Math.round((this.time + STEP) * 1e6) / 1e6;
    for (const e of events) {
      if (e.type === 'footstep') {
        this.footsteps++;
        continue;
      }
      if (e.type === 'flap') continue;
      this.events.push({
        type: e.type,
        time: round(this.time),
        ...(e.action ? { action: e.action } : {}),
        ...(e.gait ? { gait: e.gait } : {}),
        ...(e.medium ? { medium: e.medium } : {}),
        ...(e.head ? { head: e.head } : {}),
        ...(e.bone ? { bone: e.bone } : {}),
        ...(e.position ? { position: e.position.map(round) as [number, number, number] } : {}),
      });
      if (e.type === 'arrive' && this.course) this.nextPoint();
    }
    this.fire();
    this.measure();
    return events;
  }

  /** Runs to the end and returns the result. */
  run(): ScenarioResult {
    while (!this.done) this.step();
    return this.result();
  }

  result(): ScenarioResult {
    const c = this.controller;
    return {
      duration: this.scenario.duration,
      end: {
        x: round(c.position.x),
        z: round(c.position.z),
        heading: round(c.heading / DEG),
        speed: round(c.speed),
      },
      distance: round(this.distance),
      topSpeed: round(this.topSpeed),
      body: {
        lowest: round(this.lowest),
        highest: round(this.highest),
        aboveGround: round(this.aboveGround),
      },
      turned: Math.round(this.turned / DEG),
      events: this.events,
      footsteps: this.footsteps,
      gaits: this.gaits,
      targets: Object.fromEntries(
        [...this.closest].map(([n, v]) => [n, { closest: round(v.closest), time: round(v.time) }]),
      ),
      courses: this.courses,
      footSlide: round(this.footSlide),
      failed: this.failed,
    };
  }

  /** Makes every call that is due. */
  private fire(): void {
    while (this.pending.length > 0 && (this.pending[0]?.call.at ?? Infinity) <= this.time + 1e-9) {
      const { call, index } = this.pending.shift() as { call: ScenarioCall; index: number };
      try {
        this.apply(call, index);
      } catch (error) {
        this.failed.push({ call: index, reason: (error as Error).message });
      }
    }
  }

  private apply(call: ScenarioCall, index: number): void {
    const c = this.controller;
    // A new destination ends a course in progress.
    if (call.do === 'moveTo' || call.do === 'drive' || call.do === 'stop') this.course = null;
    switch (call.do) {
      case 'moveTo': {
        c.moveTo(this.aim(call.to), call.speed === undefined ? {} : { speed: call.speed });
        return;
      }
      case 'follow': {
        const points = call.path.map((p) => this.aim(p));
        this.course = {
          index: this.courses.length,
          points,
          next: 0,
          ...(call.speed === undefined ? {} : { speed: call.speed }),
        };
        this.courses.push({ call: index, reached: 0, of: points.length });
        this.nextPoint(true);
        return;
      }
      case 'drive':
        c.drive(call.speed, call.heading === undefined ? undefined : call.heading * DEG);
        return;
      case 'stop':
        c.stop();
        return;
      case 'act':
        c.act(call.action, call.target === undefined ? {} : { target: this.point(call.target) });
        return;
      case 'lookAt':
        c.lookAt(call.target === null ? null : this.point(call.target));
        return;
      case 'gait':
        c.lockGait(call.gait);
        return;
      case 'fly':
        c.fly({
          ...(call.height === undefined ? {} : { height: call.height }),
          ...(call.speed === undefined ? {} : { speed: call.speed }),
        });
        return;
      case 'land': {
        const p = call.to === undefined ? null : this.point(call.to);
        c.land(p ? { x: p.x, z: p.z } : null);
        return;
      }
      case 'hit':
        c.hit({
          direction: this.blow(call.from),
          strength: call.strength,
          ...(call.bone === undefined ? {} : { bone: call.bone }),
        });
        return;
      case 'die':
        c.die({ direction: this.blow(call.from) });
        return;
    }
  }

  /** The way a blow from `from` pushes, in the world. */
  private blow(from: 'left' | 'right' | 'front' | 'back' | number): { x: number; z: number } {
    const degrees =
      typeof from === 'number' ? from : { front: 0, left: 90, back: 180, right: -90 }[from];
    // The blow's source in the creature's frame (+Z ahead, +X its left); it pushes the other way.
    const a = degrees * DEG;
    const lx = -Math.sin(a);
    const lz = -Math.cos(a);
    const h = this.controller.heading;
    return { x: lx * Math.cos(h) + lz * Math.sin(h), z: -lx * Math.sin(h) + lz * Math.cos(h) };
  }

  /** Heads for the course's next point; `first` starts it. */
  private nextPoint(first = false): void {
    const course = this.course;
    if (!course) return;
    if (!first) {
      course.next++;
      const entry = this.courses[course.index];
      if (entry) this.courses[course.index] = { ...entry, reached: course.next };
    }
    const p = course.points[course.next];
    if (!p) {
      this.course = null;
      return;
    }
    this.controller.moveTo(p, course.speed === undefined ? {} : { speed: course.speed });
  }

  /**
   * Where `moveTo` heads: a point on the ground, or with a height when the call gave one (a
   * named target or [x, y, z]), which a swimmer dives or rises to.
   */
  private aim(where: string | readonly number[]): Aim {
    const p = this.point(where);
    return typeof where === 'string' || where.length === 3
      ? { x: p.x, y: p.y, z: p.z }
      : { x: p.x, z: p.z };
  }

  private point(where: string | readonly number[]): Vector3 {
    if (typeof where === 'string') {
      const p = this.targets.get(where);
      if (!p) throw new Error(`no target named "${where}"`);
      return p.clone();
    }
    const [x = 0, a = 0, b] = where;
    // [x, z] on the ground, or [x, y, z].
    return b === undefined ? new Vector3(x, this.ground(x, a).height, a) : new Vector3(x, a, b);
  }

  /**
   * Distance walked, heights, turning, foot slide, gaits used and how close each head came to
   * each target.
   */
  private measure(): void {
    const c = this.controller;
    this.distance += Math.hypot(c.position.x - this.last.x, c.position.z - this.last.z);
    this.topSpeed = Math.max(this.topSpeed, c.speed);
    this.last.copy(c.position);
    const spine = this.compiled.rig.spine;
    const middle = c.pose.worldPos[spine[Math.floor(spine.length / 2)] as number] as Vector3;
    this.lowest = Math.min(this.lowest, middle.y);
    this.highest = Math.max(this.highest, middle.y);
    this.aboveGround = Math.max(
      this.aboveGround,
      middle.y - this.ground(middle.x, middle.z).height,
    );
    let turn = c.heading - this.lastHeading;
    turn -= Math.round(turn / (2 * Math.PI)) * 2 * Math.PI;
    this.turned += Math.abs(turn);
    this.lastHeading = c.heading;
    const gait = c.gait?.id;
    if (gait && this.gaits.at(-1)?.gait !== gait) this.gaits.push({ gait, from: round(this.time) });
    const feet = c.feet();
    // A stagger's quick steps and a death's buckling legs are not sliding feet.
    const reeling = c.staggering || c.dead;
    this.compiled.rig.legs.forEach((leg, k) => {
      const foot = feet[k];
      if (!foot) return;
      const ankle = c.pose.tail(leg.bones.at(-1) as number);
      const anchor = this.anchors.get(k);
      if (!foot.planted || reeling) this.anchors.delete(k);
      else if (!anchor) this.anchors.set(k, ankle);
      else
        this.footSlide = Math.max(
          this.footSlide,
          Math.hypot(ankle.x - anchor.x, ankle.z - anchor.z),
        );
    });
    for (const [n, p] of this.targets) {
      // From the snout: the far end of each head bone.
      let best = Infinity;
      for (const h of this.compiled.rig.heads)
        best = Math.min(best, c.pose.tail(h.head).distanceTo(p));
      const seen = this.closest.get(n);
      if (!seen || best < seen.closest) this.closest.set(n, { closest: best, time: this.time });
    }
  }
}

const round = (v: number) => Math.round(v * 1000) / 1000;
