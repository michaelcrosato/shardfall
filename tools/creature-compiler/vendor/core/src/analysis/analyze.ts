import { Quaternion, Vector3 } from 'three';
import { colorName } from '../blueprint/colors.ts';
import type { CreatureSpec } from '../blueprint/creature.ts';
import type { Issue } from '../blueprint/issues.ts';
import { strength } from '../compile/anatomy.ts';
import { type CompiledCreature, compileCreature, type Quality } from '../compile/compile.ts';
import { CROSS_SCALE, isUprightFront } from '../compile/skeleton.ts';
import { allEyes, mainHead } from '../compile/types.ts';
import { DENSITY, volumeOf } from '../compile/volume.ts';
import { type Ground, MotionController, type Water } from '../motion/controller.ts';
import { applyRest } from '../motion/face.ts';
import { MAX_LOADING, turnRadius } from '../motion/flight.ts';
import { Pose } from '../motion/pose.ts';
import { openSea, slope, testCourse } from '../motion/terrain.ts';
import type { ActionModule, GaitModule, PartModule, PatternModule, Registry } from '../registry.ts';
import { measureBody } from '../variation/generate.ts';

const G = 9.81;
/** Flesh is about as dense as water. */
/** Steps per second (cycles per second) above which a gait reads as jitter at 30 frames a second. */
const MAX_STEPS = 8;

/** A swimmer's checks: at the surface, and diving toward the bed of shallow water. */
export interface SwimCheck {
  readonly gait: string;
  /** m/s. */
  readonly speed: number;
  /**
   * For a creature that swims at the surface (paddlers, and walkers that swim): how far the top
   * of its head sits above the water on average (m); below 0 it swims with its head under.
   */
  readonly headClearance?: number;
  /** Diving to the bed: the deepest any part of the body went into it (m), and which. */
  readonly bed: { readonly worst: number; readonly part?: string; readonly time?: number };
}

/**
 * A flyer's run (docs/design/10.4-flight.md): it takes off from flat ground, circles at its
 * cruising height, then comes in to land on a 15° slope.
 */
export interface FlightCheck {
  /** The height above the ground it was asked to hold (m), and the speed it circled at (m/s). */
  readonly height: number;
  readonly speed: number;
  /** The worst it strayed from that height over level flight, as a share of it. */
  readonly altitude: number;
  /** Deepest a wing passed into the body, a leg, the other wing on its side or the ground. */
  readonly wings: WingHit;
  /** Landing: the deepest the body, a tail or a foot went into the slope (m), and which. */
  readonly landing: { readonly worst: number; readonly part?: string; readonly time?: number };
  /** Its speeds as it touched down (m/s); absent if it never did. */
  readonly touchdown?: {
    readonly vertical: number;
    readonly horizontal: number;
    readonly time: number;
  };
  /** Seconds simulated. */
  readonly seconds: number;
}

/** Two limbs that met, or a limb and the body: how deep (m), and where. */
export interface LimbHit {
  readonly worst: number;
  readonly between?: readonly [string, string];
  /** Which part of the first limb: its upper, middle or lower segment. */
  readonly segment?: 'upper' | 'middle' | 'lower';
  readonly time?: number;
}

/** One motion run: the worst of each problem, with where and when it happened. */
export interface MotionCheck {
  /** Where it ran: on flat or rough ground, or in open water for a body that only swims. */
  readonly ground: 'flat' | 'rough' | 'water';
  readonly gait: string;
  /** m/s. */
  readonly speed: number;
  /** Seconds simulated after warming up (two gait cycles). */
  readonly seconds: number;
  /** Largest drift of a planted foot (m), and which leg. */
  readonly footSlide: { readonly worst: number; readonly leg?: string; readonly time?: number };
  /** Deepest the body went below the ground (m), and which section. */
  readonly penetration: { readonly worst: number; readonly part?: string; readonly time?: number };
  /** Share of frames a leg was stretched past its reach (its foot out of reach), worst leg. */
  readonly overstretch: { readonly worst: number; readonly leg?: string };
  /**
   * Deepest overlap of two limbs, or of a limb with the body (m); `pairs` has every pair that
   * met, deepest first, so one pass shows them all.
   */
  readonly intersection: LimbHit & { readonly pairs: readonly LimbHit[] };
  /** Deepest overlap of two heads or necks (m), with several heads. */
  readonly heads: {
    readonly worst: number;
    readonly between?: readonly [string, string];
    readonly time?: number;
  };
  /**
   * Deepest a wing or fin (its bones or membrane) passes into the body, a leg, an arm or the
   * ground (m), and what it met (docs/design/9.3-wings-fins.md).
   */
  readonly wings: WingHit;
  /**
   * Deepest a tentacle (past its first quarter) passes into the body, a leg or an arm (m), and
   * what it met (docs/design/9.4-tentacles-parts.md). Tentacles touching each other are fine.
   */
  readonly tentacles: TentacleHit;
}

/** A tentacle passing into something. */
export interface TentacleHit {
  readonly worst: number;
  readonly tentacle?: string;
  readonly against?: string;
  readonly time?: number;
}

/** A wing or fin passing into something. */
export interface WingHit {
  readonly worst: number;
  readonly wing?: string;
  readonly against?: string;
  readonly time?: number;
}

export interface Analysis {
  readonly name: string;
  /** What each part built, by id: size (metres) and count, as compile recorded them. */
  readonly parts: Readonly<Record<string, { readonly size: number; readonly count: number }>>;
  /** Metres and kilograms, from the rest pose. */
  readonly measurements: {
    readonly length: number;
    /** Top of everything, horns and spikes included. */
    readonly height: number;
    /** Top of the body alone, as `generate`'s height limits measure it. */
    readonly bodyHeight: number;
    readonly width: number;
    readonly torsoLength: number;
    readonly hipHeight: number;
    readonly legLength: number;
    readonly neckLength: number;
    readonly tailLength: number;
    readonly mass: number;
    readonly centreOfMass: readonly [number, number, number];
    readonly triangles: number;
    readonly bones: number;
    /** How many heads, tails, legs, arms, wings, fins and tentacles it has (a pair counts 2). */
    readonly counts: {
      readonly heads: number;
      readonly tails: number;
      readonly legs: number;
      readonly arms: number;
      readonly wings: number;
      readonly fins: number;
      readonly tentacles: number;
    };
    /** Tip to tip with the wings spread (m); only with wings. */
    readonly wingspan?: number;
    /** The longest tentacle, root to tip (m); only with tentacles. */
    readonly tentacleReach?: number;
  };
  /**
   * m/s: the temperament's walking pace, and the top speed its gaits allow; for a swimmer, its
   * top speed in the water (a body that only swims has its swimming pace as `walk`).
   */
  readonly speed: {
    readonly walk: number;
    readonly max: number;
    readonly swim?: number;
    /** Flyers: cruising speed from wing loading, and the slowest it flies flapping. */
    readonly fly?: number;
    readonly slow?: number;
    readonly gaits: readonly { readonly id: string; readonly from: number; readonly to: number }[];
  };
  /**
   * How fast each legged gait steps at its typical speed: m/s, metres per stride and steps per
   * second (each foot steps once a cycle).
   */
  readonly cadence: readonly {
    readonly id: string;
    readonly speed: number;
    readonly stride: number;
    readonly steps: number;
  }[];
  /**
   * The main head's bite reach: how far it lunges out from where it holds its head (null without
   * a jaw), and its height, in metres; with several heads, `heads` gives each one's, the main
   * head first, and which side of the body it is on (its left is +X).
   */
  readonly reach: {
    readonly bite: number | null;
    readonly headHeight: number;
    readonly heads?: readonly {
      readonly id: string;
      readonly bite: number | null;
      readonly headHeight: number;
      readonly side: 'left' | 'right' | 'middle';
    }[];
  };
  /** Centre of mass over the feet: `margin` is its distance inside the support area (m). */
  readonly stability: {
    readonly supported: boolean;
    readonly margin: number;
    readonly feet: number;
  };
  readonly motion: readonly MotionCheck[];
  /** For a swimmer (docs/design/10.3-swimming.md): how it holds its head and keeps off the bed. */
  readonly swimming?: SwimCheck;
  /** For a flyer (docs/design/10.4-flight.md): level flight, its wings and its landing. */
  readonly flight?: FlightCheck;
  /** Plausibility warnings, each with an id-based path and a fix. */
  readonly warnings: readonly Issue[];
  /** A paragraph describing the creature. */
  readonly description: string;
}

export interface AnalyzeOptions {
  readonly quality?: Quality;
  /** Seed for the rough test ground. */
  readonly terrainSeed?: number;
}

/**
 * Measures a creature, runs its motion on flat and rough ground, and checks it for problems a
 * model can fix: legs that can't reach, bodies in the ground, sliding feet, limbs passing
 * through each other, buried parts, eyes facing backwards, a centre of mass outside the feet.
 */
export function analyzeCreature(
  spec: CreatureSpec,
  registry: Registry,
  options: AnalyzeOptions = {},
): Analysis {
  const compiled = compileCreature(spec, registry, { quality: options.quality ?? 'low' });
  const L = compiled.scale;
  const warnings: Issue[] = [...compiled.warnings];
  const warn = (path: string, code: string, message: string, fix: string) =>
    warnings.push({ severity: 'warning', path, code, message, fix });

  // --- Measurements ----------------------------------------------------------------------
  const [x0, , z0] = compiled.bounds.min;
  const [x1, y1, z1] = compiled.bounds.max;
  const { volume, centre } = volumeOf(compiled);
  const legs = spec.limbs.filter((l) => l.role === 'leg');
  const legLength = legs.length > 0 ? Math.max(...legs.map((l) => l.length)) * L : 0;
  const t = compiled.stats.triangles;
  const measurements = {
    length: z1 - z0,
    height: y1,
    // The body alone (no horns or spikes), measured as generate's height limits are.
    bodyHeight: measureBody(spec, registry).height,
    width: x1 - x0,
    torsoLength: L,
    hipHeight: compiled.rig.hipHeight,
    legLength,
    neckLength: spec.body.neck.length * L,
    tailLength: spec.body.tail.length * L,
    mass: volume * DENSITY,
    centreOfMass: [centre.x, centre.y, centre.z] as const,
    triangles: t.skin + t.parts + t.eyes + t.membranes,
    bones: compiled.bones.names.length,
    counts: {
      heads: compiled.rig.heads.length,
      tails: compiled.rig.tails.length,
      legs: spec.limbs.filter((l) => l.role === 'leg').length,
      arms: spec.limbs.filter((l) => l.role === 'arm').length,
      wings: spec.limbs.filter((l) => l.role === 'wing').length,
      fins: spec.limbs.filter((l) => l.role === 'fin').length,
      tentacles: spec.limbs.filter((l) => l.role === 'tentacle').length,
    },
    ...(compiled.rig.wings.length > 0 && compiled.spreadBounds
      ? { wingspan: compiled.spreadBounds.max[0] - compiled.spreadBounds.min[0] }
      : {}),
    ...(compiled.rig.tentacles.length > 0
      ? {
          tentacleReach: Math.max(
            ...compiled.rig.tentacles.map((t) =>
              t.bones.reduce((sum, b) => sum + (compiled.bones.lengths[b] ?? 0), 0),
            ),
          ),
        }
      : {}),
  };

  // --- Speed and reach -------------------------------------------------------------------
  const controller = new MotionController(compiled, { registry });
  const hip = Math.max(0.05 * L, compiled.rig.hipHeight);
  const swim = controller.swimSpeed();
  // Flyers: cruise from wing loading and slow flight (docs/design/10.4-flight.md).
  const fl = controller.canFly ? controller.flightNumbers : undefined;
  const speed = {
    walk: controller.paceSpeed(),
    max: controller.maxSpeed(),
    ...(swim > 0 ? { swim } : {}),
    ...(fl ? { fly: fl.cruise, slow: fl.slow } : {}),
    gaits: compiled.motion.gaits.map((g) => {
      if (g.medium === 'air' && fl)
        return g.air === 'hovering'
          ? { id: g.id, from: 0, to: 0.3 * fl.cruise }
          : { id: g.id, from: g.air === 'gliding' ? fl.stall : fl.slow, to: fl.cruise };
      return {
        id: g.id,
        from: Math.sqrt(g.froude[0] * G * controller.lengthScale),
        // Gaits that keep a foot down top out at Froude 1.5; runs, gallops and swimming go to
        // their top.
        to: Math.sqrt(
          (g.flight || g.medium === 'water' ? g.froude[1] : Math.min(g.froude[1], 1.5)) *
            G *
            controller.lengthScale,
        ),
      };
    }),
  };
  // --- Cadence: fast steps read as jitter --------------------------------------------------
  const cadence = compiled.motion.gaits
    .filter((g) => !g.spine && (g.medium ?? 'land') === 'land')
    .map((g) => ({ id: g.id, ...controller.cadence(g.id) }))
    .map(({ id, speed, stride, steps }) => ({ id, speed, stride, steps }));
  // A skittish creature is meant to scurry, so its fast steps are no mistake.
  const fast = cadence.filter((c) => c.steps > MAX_STEPS);
  const worst = fast.reduce<(typeof fast)[number] | undefined>(
    (w, c) => (w === undefined || c.steps > w.steps ? c : w),
    undefined,
  );
  if (worst && spec.motion.temperament !== 'skittish') {
    // Steps per second fall with the square root of size.
    const scale = roundUp(spec.scale * (worst.steps / MAX_STEPS) ** 2 * 1.05);
    const list = fast.map((c) => `the ${c.id} ${c.steps.toFixed(1)} at ${c.speed.toFixed(2)} m/s`);
    warn(
      'scale',
      'fast_cadence',
      `it steps fast for its size (steps a second: ${list.join(', ')}); above ${MAX_STEPS} a second it reads as jitter at 30 frames a second`,
      `make it bigger ("scale": ${scale} or more), or, if it is meant to be this small, make it skittish ("motion.temperament": "skittish") so it scurries on purpose`,
    );
  }

  // Reach per head, the main head first; its numbers are the creature's.
  const reachOf = (h: (typeof compiled.rig.heads)[number]) => {
    let neckLen = 0;
    for (const b of h.neck) neckLen += compiled.bones.lengths[b] ?? 0;
    const x = compiled.bones.positions[h.head * 3] as number;
    return {
      id: h.id,
      bite: h.jaw >= 0 ? neckLen * 0.6 + 0.12 * L : null,
      headHeight: compiled.bones.positions[h.head * 3 + 1] as number,
      side:
        x > 0.02 * L ? ('left' as const) : x < -0.02 * L ? ('right' as const) : ('middle' as const),
    };
  };
  const main = mainHead(compiled.rig);
  const perHead = [main, ...compiled.rig.heads.filter((h) => h !== main)].map(reachOf);
  const { bite, headHeight } = perHead[0] as (typeof perHead)[number];
  const reach = { bite, headHeight, ...(perHead.length > 1 ? { heads: perHead } : {}) };

  // --- Stability: the centre of mass over the feet's support area -------------------------
  const feet = compiled.rig.legs.map((l) => [l.restFoot[0], l.restFoot[2]] as [number, number]);
  // A foot covers its tip's width and, with toes, as far as they reach from the ankle.
  const footRadius = Math.max(
    0.03 * L,
    ...spec.limbs.filter((l) => l.role === 'leg').map((l) => (l.radius.at(-1) ?? 0.03) * 2 * L),
    ...compiled.rig.legs.map((leg) => toeReach(compiled, leg.restFoot, leg.toes)),
  );
  const stability =
    feet.length === 0
      ? { supported: true, margin: Math.max(0, measurements.width / 2), feet: 0 }
      : supportMargin(feet, footRadius, [centre.x, centre.z]);
  if (feet.length > 0 && !stability.supported) {
    // Which way it tips: ahead of the front feet, behind the hind feet, or out to a side.
    const front = Math.max(...feet.map((f) => f[1]));
    const back = Math.min(...feet.map((f) => f[1]));
    const ahead = centre.z > front;
    const behind = centre.z < back;
    const where = ahead
      ? 'ahead of the front feet'
      : behind
        ? 'behind the hind feet'
        : 'beside the feet';
    const biped = feet.length <= 2;
    warn(
      'limbs',
      'unbalanced',
      `the centre of mass is ${(-stability.margin * 100).toFixed(1)} cm ${where}, so it would tip ${ahead ? 'forward' : behind ? 'backward' : 'over'}`,
      ahead && isUprightFront(spec)
        ? 'the upright front tips it forward: stand it straighter (neck.pitch nearer 90), slim it (neck.radius) or shorten its arms, or move the forelegs forward (a lower attach.at)'
        : ahead
          ? biped
            ? 'the front is too heavy: move the legs forward (a lower attach.at), stand the torso more upright (a higher torso pitch), shorten or slim the head and neck, or lengthen the tail as a counterweight'
            : 'the front is too heavy: move the forelegs forward (a lower attach.at), or shorten or slim the head and neck'
          : behind
            ? biped
              ? 'the back is too heavy: move the legs back (a higher attach.at), lean the torso forward (a lower torso pitch), or shorten or slim the tail'
              : 'the back is too heavy: move the hindlegs back (a higher attach.at), or shorten or slim the tail'
            : 'spread the legs wider (more splay) or slim the body',
    );
  }

  // --- Eyes facing backwards ---------------------------------------------------------------
  const eyeParts = spec.parts.filter((p) => registry.get('part', p.type)?.material === 'eye');
  allEyes(compiled.rig).forEach((bone, i) => {
    const dir = new Vector3(0, 1, 0).applyQuaternion(
      new Quaternion().fromArray(compiled.bones.rotations, bone * 4),
    );
    const part = eyeParts[i];
    if (dir.z < -0.25 && part)
      warn(
        `parts[id=${part.baseId}].attach.at`,
        'eye_backwards',
        'the eye faces backwards',
        'move it toward the snout (lower "at" on the head) or lower its "angle"',
      );
  });

  // --- Motion on flat and rough ground ------------------------------------------------------
  const motion: MotionCheck[] = [];
  /** How many runs, first in `motion`, are at the walking pace (the rest run faster gaits). */
  let paceRuns = 0;
  const walks = compiled.motion.gaits.some((g) => (g.medium ?? 'land') === 'land');
  const swims = compiled.motion.gaits.some((g) => g.medium === 'water');
  if (walks) {
    const rough = testCourse(options.terrainSeed ?? 3, 0.25 * hip, 0);
    const grounds = [
      ['flat', () => ({ height: 0 })],
      ['rough', rough],
    ] as const;
    for (const [name, ground] of grounds)
      motion.push(runMotion(compiled, registry, name, ground, torsoDepth(spec)));
    paceRuns = motion.length;
    // Every other legged gait at its own natural speed, so a gallop's feet are checked too.
    const paced = new Set(motion.map((m) => m.gait));
    for (const gait of compiled.motion.gaits)
      if (!gait.spine && (gait.medium ?? 'land') === 'land' && !paced.has(gait.id))
        for (const [name, ground] of grounds)
          motion.push(runMotion(compiled, registry, name, ground, torsoDepth(spec), gait.id));
  } else if (swims) {
    // A body that only swims is checked in open water.
    const sea = openSea(L);
    motion.push(
      runMotion(compiled, registry, 'water', sea.ground, torsoDepth(spec), undefined, sea.water),
    );
    paceRuns = 1;
  }
  const meeting = new Map<string, { hit: LimbHit; where: string }>();
  for (const [k, check] of motion.entries()) {
    const where = WHERE[check.ground];
    // The faster gaits' runs check the feet and the ground; how limbs, heads, wings and
    // tentacles clear each other is judged at the walking pace, where a creature spends its
    // time (a gallop's gathered legs brush the belly, as real ones do).
    const paced = k < paceRuns;
    if (check.footSlide.worst > 0.02 * L && check.footSlide.leg)
      warn(
        `limbs[id=${check.footSlide.leg.replace(/\.[LR]$/, '')}]`,
        'foot_slide',
        `a planted foot slides ${(check.footSlide.worst * 100).toFixed(1)} cm ${where} (${check.gait})`,
        'lengthen the leg or move it so the foot sits under the hip (attach.at, splay)',
      );
    const path = sectionPath(check.penetration.part ?? '');
    const known = warnings.some((w) => w.path === path && w.code === 'below_ground');
    if (check.penetration.worst > 0.03 * L && check.penetration.part && !known)
      warn(
        path,
        'ground_penetration',
        `the ${check.penetration.part} goes ${(check.penetration.worst * 100).toFixed(1)} cm into the ground ${where}`,
        spec.limbs.some((l) => l.role === 'leg')
          ? 'lengthen the legs, raise the section (pitch, curl) or make it slimmer'
          : 'raise the section (pitch, curl), lower the torso pitch or make it slimmer',
      );
    if (check.overstretch.worst > 0.15 && check.overstretch.leg)
      warn(
        `limbs[id=${check.overstretch.leg.replace(/\.[LR]$/, '')}]`,
        'overstretch',
        `the leg is stretched past its reach ${Math.round(check.overstretch.worst * 100)}% of the time ${where}`,
        'lengthen it, or give the gait a smaller stride',
      );
    if (paced && check.heads.worst > 0.01 * L && check.heads.between) {
      const [a, b] = check.heads.between;
      warn(
        'body.neck',
        'head_intersection',
        `${a} and ${b} pass ${(check.heads.worst * 100).toFixed(1)} cm into each other ${where}`,
        `fan the necks wider (body.neck.spread about ${Math.min(170, Math.round(spec.body.neck.spread + 15))}) or make them longer, or the heads smaller`,
      );
    }
    if (paced)
      for (const pair of check.intersection.pairs) {
        if (pair.worst <= 0.01 * L || !pair.between) continue;
        // Mirror images (foreleg.L and foreleg.R into the torso) share one fix.
        const key = pair.between.map((x) => x.replace(/\.[LR]$/, '')).join('|');
        const seen = meeting.get(key);
        if (!seen || pair.worst > seen.hit.worst) meeting.set(key, { hit: pair, where });
      }
  }
  // Every pair of limbs that met, deepest first, each with its own fix, so one pass shows them
  // all (gate 10's agents chased them one at a time).
  for (const { hit, where } of [...meeting.values()].sort((x, y) => y.hit.worst - x.hit.worst)) {
    const [a, b] = hit.between as readonly [string, string];
    const id = a.replace(/\.[LR]$/, '');
    const limb = spec.limbs.find((l) => l.id === id);
    const depth = hit.worst;
    // Splay that moves the meeting point sideways by the overlap, with some room to spare: it
    // swings about the hip, so a meeting high on the leg needs more than one at the foot.
    const lever = { upper: 0.3, middle: 0.6, lower: 0.9 }[hit.segment ?? 'lower'];
    const splay =
      Math.ceil((Math.atan2(depth * 1.5, lever * (limb?.length ?? 0.5) * L) * 180) / Math.PI / 5) *
      5;
    const section = ['torso', 'neck', 'head', 'jaw', 'tail', 'spine'].includes(b);
    const cm = (depth * 100).toFixed(0);
    // Which remedy works depends on what meets (gate 8's agents found the old list misled):
    // splay clears a leg from the body; two legs of a pair meet under it, where only
    // attaching them higher or thinning them helps; front and hind legs meet in the stride.
    const pair = !section && b.replace(/\.[LR]$/, '') === id;
    const angle = limb?.angle;
    warn(
      `limbs[id=${id}]`,
      'limb_intersection',
      `${a} (its ${hit.segment ?? 'lower'} segment) passes ${(depth * 100).toFixed(1)} cm into ${b} ${where}`,
      section
        ? `move it clear of the ${b} by about ${cm} cm: about ${splay}° more splay works best; or a smaller gait stride or stepHeight, or a thinner ${b === 'torso' ? 'body' : b}. On a broad body a lower attach.angle can make it worse`
        : pair
          ? `the pair meets under the body: attach both higher up the side (a lower attach.angle${angle !== undefined ? `, e.g. ${Math.max(0, Math.round(angle - 15))}` : ''}) or make them thinner (radius); splay barely helps here`
          : `the front and hind legs meet in the stride: a smaller gait stride, attach.at further apart, or thinner legs, by about ${cm} cm`,
    );
  }

  // --- Flight: can it, and how it flies and lands (docs/design/10.4-flight.md) ---------------
  const lifting = compiled.rig.wings.find((w) => w.lift);
  const wingPath = `limbs[id=${(lifting ?? compiled.rig.wings[0])?.id.replace(/\.[LR]$/, '') ?? 'wing'}]`;
  if (fl && (fl.area <= 0 || fl.loading > MAX_LOADING)) {
    // Area grows with the square of the wings' length.
    const grow = fl.area > 0 ? Math.sqrt(fl.loading / MAX_LOADING) : 0;
    warn(
      wingPath,
      'cannot_fly',
      fl.area <= 0
        ? 'its wings have nothing to lift it with (no membrane on a lifting wing), so it could not fly'
        : `its wings carry ${Math.round(fl.loading)} N/m² of its weight; above ${MAX_LOADING} nothing could fly (it would need a span of about ${(fl.span * grow).toFixed(1)} m)`,
      fl.area <= 0
        ? 'give a wing a membrane (leathery, feathered or insect), or keep it on the ground ("motion": { "media": { "air": false } })'
        : `make the wings about ${grow.toFixed(2)} times as long (length, or the membrane's span), or the body smaller or slimmer; a game may still let it fly`,
    );
  }
  const flight = fl ? runFlight(compiled, registry) : undefined;
  if (flight?.touchdown && flight.landing.worst > 0.02 * hip && flight.landing.part) {
    const { vertical, horizontal } = flight.touchdown;
    warn(
      sectionPath(flight.landing.part),
      'hard_landing',
      `landing on a 15° slope, the ${flight.landing.part} goes ${(flight.landing.worst * 100).toFixed(1)} cm into the ground (touching down at ${vertical.toFixed(1)} m/s down, ${horizontal.toFixed(1)} m/s along)`,
      'longer legs reach the slope first (length), or a shorter or higher-held tail and neck (tail.length, tail.curl, neck.pitch) keep clear of it',
    );
  }

  // --- Wings through the body, walking and standing spread (docs/design/9.3-wings-fins.md) ----
  if (compiled.rig.wings.length + compiled.rig.fins.length > 0) {
    const checks: [WingHit, string][] = [
      ...motion.slice(0, paceRuns).map((m) => [m.wings, MOVING[m.ground]] as [WingHit, string]),
      [wingHits(compiled, standing(compiled, 0.5)), 'half spread'],
      [wingHits(compiled, standing(compiled, 1)), 'spread'],
      ...(flight ? [[flight.wings, 'while flying'] as [WingHit, string]] : []),
    ];
    const [hit, when] = checks.reduce((a, b) => (b[0].worst > a[0].worst ? b : a));
    if (hit.worst > Math.max(0.01, 0.01 * L) && hit.wing) {
      const id = hit.wing.replace(/\.[LR]$/, '');
      const limb = spec.limbs.find((l) => l.id === hit.wing || l.id === id);
      // Fixes the membrane's own parameters offer, when it has them.
      const module = limb?.membrane
        ? (registry.get('part', limb.membrane.type) as PartModule | undefined)
        : undefined;
      const keys = Object.keys((module?.params as { shape?: object } | undefined)?.shape ?? {});
      const span = keys.includes('span') ? ", or the membrane's span" : '';
      const trailing =
        keys.includes('trailing') && limb?.membrane?.params.trailing !== 'body'
          ? '; or trail the membrane to the body ("trailing": "body")'
          : '';
      warn(
        `limbs[id=${id}]`,
        'wing_intersection',
        `${hit.wing} passes ${(hit.worst * 100).toFixed(1)} cm into the ${hit.against ?? 'body'} ${when}`,
        `attach it higher (a lower attach.angle) or further forward (attach.at), or make it shorter (length${span})${trailing}`,
      );
    }
  }

  // --- Tentacles through the body while walking (docs/design/9.4-tentacles-parts.md) --------
  if (compiled.rig.tentacles.length > 0) {
    const checks: [TentacleHit, string][] = [
      ...motion
        .slice(0, paceRuns)
        .map((m) => [m.tentacles, MOVING[m.ground]] as [TentacleHit, string]),
      [tentacleHits(compiled, standing(compiled, 0)), 'standing'],
    ];
    const [hit, when] = checks.reduce((a, b) => (b[0].worst > a[0].worst ? b : a));
    if (hit.worst > Math.max(0.01, 0.01 * L) && hit.tentacle) {
      const id = hit.tentacle.replace(/\.[LR]$/, '');
      warn(
        `limbs[id=${id}]`,
        'tentacle_intersection',
        `${hit.tentacle} passes ${(hit.worst * 100).toFixed(1)} cm into the ${hit.against ?? 'body'} ${when}`,
        'attach it further round or further back (attach.angle, attach.at), make it shorter, curl it more (curl), or use fewer',
      );
    }
  }

  // --- Swimming: heads above the surface, bodies off the bed (docs/design/10.3-swimming.md) ---
  const swimming = swims ? runSwim(compiled, registry, torsoDepth(spec)) : undefined;
  if (swimming?.headClearance !== undefined && swimming.headClearance < -0.01 * L)
    warn(
      'body.neck',
      'head_underwater',
      `the head swims ${(-swimming.headClearance * 100).toFixed(1)} cm under the surface (${swimming.gait})`,
      'raise the neck (a higher neck.pitch) or the head (head.pitch), so it holds its head out of the water',
    );
  if (swimming && swimming.bed.worst > 0.03 * L && swimming.bed.part) {
    const path = sectionPath(swimming.bed.part);
    warn(
      path,
      'hits_bed',
      `the ${swimming.bed.part} goes ${(swimming.bed.worst * 100).toFixed(1)} cm into the bed when it dives`,
      'make it slimmer, or raise it (pitch, curl) so it clears what is under the body',
    );
  }

  return {
    name: compiled.name,
    parts: compiled.partSizes,
    measurements,
    speed,
    cadence,
    reach,
    stability,
    motion,
    ...(swimming ? { swimming } : {}),
    ...(flight ? { flight } : {}),
    warnings: dedupe(warnings),
    description: describeCreature(spec, registry, measurements, speed),
  };
}

const WHERE: Record<MotionCheck['ground'], string> = {
  flat: 'on flat ground',
  rough: 'on rough ground',
  water: 'in the water',
};
const MOVING: Record<MotionCheck['ground'], string> = {
  flat: 'walking',
  rough: 'walking on rough ground',
  water: 'swimming',
};

/**
 * A swimmer's checks (docs/design/10.3-swimming.md). In open water at its swimming pace, a
 * creature that swims at the surface should hold its head out. Then, in water a few body
 * depths deep, it dives at the bed ahead, which nothing of its body should go into.
 */
function runSwim(compiled: CompiledCreature, registry: Registry, depth: number): SwimCheck {
  const dt = 1 / 120;
  const rig = compiled.rig;
  const bones = compiled.bones;
  const L = compiled.scale;
  const sections = bones.names
    .map((_, i) => i)
    .filter((i) => ['torso', 'neck', 'head', 'jaw', 'tail'].includes(bones.sections[i] as string));
  const head = mainHead(rig).head;

  // At the surface.
  const sea = openSea(L);
  const c = new MotionController(compiled, { registry });
  c.place(0, 0, 0, sea.ground, sea.water);
  const speed = c.paceSpeed();
  c.drive(speed, 0);
  const atSea = { ground: sea.ground, water: sea.water };
  for (let i = 0; i < 360; i++) c.update(dt, atSea);
  const gait = c.gait;
  const walks = compiled.motion.gaits.some((g) => (g.medium ?? 'land') === 'land');
  const floats = gait?.swim === 'legs' || walks;
  let clearance = 0;
  let frames = 0;
  for (let i = 0; i < 240; i++) {
    c.update(dt, atSea);
    const p = c.pose.worldPos[head] as Vector3;
    clearance += p.y + (bones.radii[head] ?? 0);
    frames++;
  }

  // Diving at the bed of water a few times deeper than it needs to swim.
  let girth = 0.02 * L;
  for (const b of rig.spine) girth = Math.max(girth, bones.radii[b] ?? 0);
  const deep = 6 * girth + (rig.posture === 'legless' ? 0 : 1.7 * rig.hipHeight);
  const shallow: Ground = () => ({ height: -deep });
  const d = new MotionController(compiled, { registry });
  d.place(0, 0, 0, shallow, sea.water);
  d.moveTo({ x: 0, y: -deep, z: 40 * L });
  const bed = { worst: 0 } as { worst: number; part?: string; time?: number };
  const a = new Vector3();
  for (let i = 0; i < 720; i++) {
    d.update(dt, { ground: shallow, water: sea.water });
    if (i % 4 !== 0) continue;
    for (const s of sections) {
      const r = (bones.radii[s] ?? 0) * (bones.sections[s] === 'torso' ? depth : 1);
      for (const p of [d.pose.worldPos[s] as Vector3, d.pose.tail(s, a)]) {
        const into = -deep - (p.y - r);
        if (into > bed.worst)
          Object.assign(bed, { worst: into, part: bones.owners[s] ?? 'body', time: d.time });
      }
    }
  }
  return {
    gait: gait?.id ?? 'none',
    speed,
    ...(floats ? { headClearance: clearance / Math.max(1, frames) } : {}),
    bed,
  };
}

/**
 * A flyer's run (docs/design/10.4-flight.md), 30 s at most: it takes off from flat ground and
 * circles its takeoff point at slow-flight speed and its cruising height, then lands uphill on a
 * 15° slope well beyond the circle. Level flight is measured against the height it was asked
 * for; wings are checked against everything they could meet as they beat, and the landing for
 * anything going into the slope.
 */
function runFlight(compiled: CompiledCreature, registry: Registry): FlightCheck {
  const dt = 1 / 120;
  const c = new MotionController(compiled, { registry });
  const fl = c.flightNumbers as NonNullable<MotionController['flightNumbers']>;
  const rig = compiled.rig;
  const bones = compiled.bones;
  // The circle it flies (as the controller's loiter), and the slope far enough beyond it.
  const bank =
    ((compiled.motion.gaits.find((g) => g.air === 'flapping')?.bank ?? 40) * Math.PI) / 180;
  const round = Math.max(
    1.5 * turnRadius(fl),
    (1.2 * fl.slow * fl.slow) / (G * Math.tan(Math.min(bank, (70 * Math.PI) / 180))),
  );
  const from = round + 1.5 * fl.cruise + 2 * fl.span + 5;
  const ground = slope(15, 0, from);
  const input = { ground };
  const height = fl.height;
  c.fly({ speed: fl.slow });
  const sections = bones.names
    .map((_, i) => i)
    .filter((i) => ['torso', 'neck', 'head', 'jaw', 'tail'].includes(bones.sections[i] as string));
  let wings: WingHit = { worst: 0 };
  const landing = { worst: 0 } as { worst: number; part?: string; time?: number };
  let touchdown: FlightCheck['touchdown'];
  let altitude = 0;
  let steps = 0;
  const limit = 30 / dt;
  const a = new Vector3();
  const checkWings = () => {
    const w = wingHits(compiled, c.pose, ground);
    if (w.worst > wings.worst) wings = { ...w, time: c.time };
    const clash = wingClash(compiled, c.pose);
    if (clash.worst > wings.worst) wings = { ...clash, time: c.time };
  };
  // Up to its height (10 s at most).
  while (steps < 10 / dt) {
    c.update(dt, input);
    steps++;
    if (c.flightStage === 'flight' && Math.abs(c.position.y - height) < 0.05 * height) break;
  }
  // Once round the circle, at most 12 s: level flight.
  const circle = Math.min(12, (2 * Math.PI * round) / fl.slow);
  for (let i = 0; i < circle / dt; i++, steps++) {
    c.update(dt, input);
    altitude = Math.max(
      altitude,
      Math.abs(c.position.y - ground(c.position.x, c.position.z).height - height) / height,
    );
    if (i % 12 === 0) checkWings();
  }
  // In to land uphill, beyond the foot of the slope.
  c.land({ x: 0, z: from + Math.max(2, 3 * rig.hipHeight) });
  let lastY = c.position.y;
  let after = -1;
  for (; steps < limit && after < 1; steps++) {
    const speed = c.speed;
    const events = c.update(dt, input);
    const vertical = (lastY - c.position.y) / dt;
    lastY = c.position.y;
    if (!touchdown && events.some((e) => e.type === 'land'))
      touchdown = { vertical: Math.max(0, vertical), horizontal: speed, time: c.time };
    if (touchdown) after += dt;
    const stage = c.flightStage;
    if (stage === 'flight' && steps % 12 === 0) checkWings();
    if ((stage === 'flare' || stage === 'descend') && steps % 2 === 0) checkWings();
    if (stage !== 'flare' && stage !== 'descend' && !touchdown) continue;
    const pose = c.pose;
    const time = c.time;
    for (const i of sections) {
      const r = bones.radii[i] ?? 0;
      for (const p of [pose.worldPos[i] as Vector3, pose.tail(i, a)]) {
        const into = ground(p.x, p.z).height - (p.y - r);
        if (into > landing.worst)
          Object.assign(landing, { worst: into, part: bones.owners[i] ?? 'body', time });
      }
    }
    for (const leg of rig.legs) {
      const ankle = pose.tail(leg.bones.at(-1) as number, a);
      const into = ground(ankle.x, ankle.z).height + (leg.restFoot[1] as number) - ankle.y;
      if (into > landing.worst) Object.assign(landing, { worst: into, part: leg.id, time });
    }
  }
  return {
    height,
    speed: fl.slow,
    altitude,
    wings,
    landing,
    ...(touchdown ? { touchdown } : {}),
    seconds: steps * dt,
  };
}

/**
 * Wings on one side against each other (a hind wing under its forewing): their bones past the
 * shoulder, as capsules. Wings on opposite sides may meet over the back, as insects' do.
 */
function wingClash(compiled: CompiledCreature, pose: Pose): WingHit {
  const rig = compiled.rig;
  const bones = compiled.bones;
  let worst: WingHit = { worst: 0 };
  const side = (w: (typeof rig.wings)[number]) =>
    Math.sign(bones.positions[(w.bones[0] as number) * 3] as number);
  const chain = (w: (typeof rig.wings)[number]) => [...w.bones.slice(1), ...w.digits.flat()];
  const a = new Vector3();
  const b = new Vector3();
  const e = new Vector3();
  const f = new Vector3();
  const lifting = rig.wings.filter((w) => w.lift);
  for (let i = 0; i < lifting.length; i++)
    for (let j = i + 1; j < lifting.length; j++) {
      const wi = lifting[i] as (typeof rig.wings)[number];
      const wj = lifting[j] as (typeof rig.wings)[number];
      if (side(wi) !== side(wj) || side(wi) === 0) continue;
      for (const bi of chain(wi))
        for (const bj of chain(wj)) {
          a.copy(pose.worldPos[bi] as Vector3);
          pose.tail(bi, b);
          e.copy(pose.worldPos[bj] as Vector3);
          pose.tail(bj, f);
          const overlap =
            ((bones.radii[bi] ?? 0) + (bones.radii[bj] ?? 0)) * 0.8 - segmentDistance(a, b, e, f);
          if (overlap > worst.worst) worst = { worst: overlap, wing: wi.id, against: wj.id };
        }
    }
  return worst;
}

/** Rounds up to two significant figures. */
function roundUp(v: number): number {
  const unit = 10 ** (Math.floor(Math.log10(v)) - 1);
  return Number((Math.ceil(v / unit - 1e-9) * unit).toPrecision(2));
}

const sectionPath = (part: string) =>
  ['head', 'jaw', 'neck', 'torso', 'tail'].includes(part)
    ? `body.${part === 'jaw' ? 'head' : part}`
    : `limbs[id=${part.replace(/\.[LR]$/, '')}]`;

function dedupe(issues: Issue[]): Issue[] {
  const seen = new Set<string>();
  return issues.filter((i) => {
    const key = `${i.path}|${i.code}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/**
 * Signed distance of `point` inside the support area: the convex hull of the feet, each grown by
 * the foot's radius. Two feet make a strip between them.
 */
function supportMargin(
  feet: readonly [number, number][],
  radius: number,
  point: [number, number],
): { supported: boolean; margin: number; feet: number } {
  const hull = convexHull(feet);
  let margin: number;
  if (hull.length === 1) {
    const [x, z] = hull[0] as [number, number];
    margin = radius - Math.hypot(point[0] - x, point[1] - z);
  } else if (hull.length === 2) {
    margin =
      radius - distanceToSegment(point, hull[0] as [number, number], hull[1] as [number, number]);
  } else {
    // Inside: the smallest distance to an edge; outside: minus the distance to the hull.
    let inside = true;
    let nearest = Infinity;
    for (let i = 0; i < hull.length; i++) {
      const a = hull[i] as [number, number];
      const b = hull[(i + 1) % hull.length] as [number, number];
      const cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
      if (cross < 0) inside = false;
      nearest = Math.min(nearest, distanceToSegment(point, a, b));
    }
    margin = (inside ? nearest : -nearest) + radius;
  }
  return { supported: margin >= 0, margin, feet: feet.length };
}

function convexHull(points: readonly [number, number][]): [number, number][] {
  const pts = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  if (pts.length <= 2) return pts;
  const cross = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower: [number, number][] = [];
  for (const p of pts) {
    while (lower.length >= 2 && cross(lower.at(-2) as never, lower.at(-1) as never, p) <= 0)
      lower.pop();
    lower.push(p);
  }
  const upper: [number, number][] = [];
  for (const p of [...pts].reverse()) {
    while (upper.length >= 2 && cross(upper.at(-2) as never, upper.at(-1) as never, p) <= 0)
      upper.pop();
    upper.push(p);
  }
  return [...lower.slice(0, -1), ...upper.slice(0, -1)];
}

function distanceToSegment(p: [number, number], a: [number, number], b: [number, number]): number {
  const dx = b[0] - a[0];
  const dz = b[1] - a[1];
  const len = dx * dx + dz * dz;
  const t = len > 0 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len)) : 0;
  return Math.hypot(p[0] - (a[0] + dx * t), p[1] - (a[1] + dz * t));
}

/** Closest distance between segments ab and cd (3D). */
function segmentDistance(a: Vector3, b: Vector3, c: Vector3, d: Vector3): number {
  const u = b.clone().sub(a);
  const v = d.clone().sub(c);
  const w = a.clone().sub(c);
  const aa = u.dot(u);
  const bb = u.dot(v);
  const cc = v.dot(v);
  const dd = u.dot(w);
  const ee = v.dot(w);
  const den = aa * cc - bb * bb;
  let s = den > 1e-12 ? (bb * ee - cc * dd) / den : 0;
  s = Math.max(0, Math.min(1, s));
  let t = cc > 1e-12 ? (bb * s + ee) / cc : 0;
  t = Math.max(0, Math.min(1, t));
  s = aa > 1e-12 ? Math.max(0, Math.min(1, (bb * t - dd) / aa)) : 0;
  return a.clone().addScaledVector(u, s).distanceTo(c.clone().addScaledVector(v, t));
}

/**
 * The torso's half-height as a share of its radius: its cross-section, flattened further on a
 * legless body resting on its belly, as the skeleton builds it.
 */
function torsoDepth(spec: CreatureSpec): number {
  const legless = !spec.limbs.some((l) => l.role === 'leg');
  const s = strength(spec.body.muscle);
  return CROSS_SCALE[spec.body.torso.crossSection][1] * (legless && s > 0 ? 1 - 0.1 * s : 1);
}

/**
 * Walks the creature at its pace (or runs one gait at its natural speed) for two gait cycles
 * after a warm-up, watching for problems.
 */
function runMotion(
  compiled: CompiledCreature,
  registry: Registry,
  ground: MotionCheck['ground'],
  height: Ground,
  /** The torso's half-height as a share of its radius (`torsoDepth`). */
  depth = 1,
  gait?: string,
  water?: Water,
): MotionCheck {
  const c = new MotionController(compiled, { registry });
  const input = water ? { ground: height, water } : { ground: height };
  if (water) c.place(0, 0, 0, height, water);
  if (gait) c.lockGait(gait);
  const speed = gait ? c.gaitSpeed(gait) : c.paceSpeed();
  c.drive(speed, 0.4);
  const dt = 1 / 120;
  for (let i = 0; i < 240; i++) c.update(dt, input);
  const rig = compiled.rig;
  const bones = compiled.bones;
  const legBones = rig.legs.map((l) => l.bones);
  const sections = bones.names
    .map((_, i) => i)
    .filter((i) => ['torso', 'neck', 'head', 'jaw', 'tail'].includes(bones.sections[i] as string));
  const anchors = new Map<string, Vector3>();
  const slide = { worst: 0 } as { worst: number; leg?: string; time?: number };
  const pen = { worst: 0 } as { worst: number; part?: string; time?: number };
  const stretched = rig.legs.map(() => 0);
  const hit = { worst: 0 } as {
    worst: number;
    between?: [string, string];
    segment?: 'upper' | 'middle' | 'lower';
    time?: number;
  };
  // Every pair that meets, at its deepest.
  const pairs = new Map<string, LimbHit>();
  const meet = (
    overlap: number,
    between: [string, string],
    segment: 'upper' | 'middle' | 'lower',
    time: number,
  ) => {
    if (overlap > hit.worst) Object.assign(hit, { worst: overlap, between, segment, time });
    const key = between.join('|');
    if (overlap > 0 && overlap > (pairs.get(key)?.worst ?? 0))
      pairs.set(key, { worst: overlap, between, segment, time });
  };
  const segmentOf = (k: number, n: number) =>
    k === 0 ? 'upper' : k === n - 1 ? 'lower' : 'middle';
  // Several heads: each one's neck past its first quarter, head and jaw, against the others'.
  const headBones = rig.heads.map((h) => [
    ...h.neck.slice(Math.ceil(h.neck.length / 4)),
    h.head,
    ...(h.jaw >= 0 ? [h.jaw] : []),
  ]);
  const crowd = { worst: 0 } as { worst: number; between?: [string, string]; time?: number };
  let wingHit: WingHit = { worst: 0 };
  let tentacleHit: TentacleHit = { worst: 0 };
  let frames = 0;
  let cycles = 0;
  let last = c.phase;
  const start = c.time;
  const a = new Vector3();
  const b = new Vector3();
  const e = new Vector3();
  const f = new Vector3();
  while (cycles < 2 && c.time - start < 8) {
    c.update(dt, input);
    if (c.phase < last) cycles++;
    last = c.phase;
    frames++;
    const time = c.time - start;
    const pose = c.pose;
    for (const [k, foot] of c.feet().entries()) {
      const leg = rig.legs[k];
      if (!leg) continue;
      const ankle = pose.tail(leg.bones.at(-1) as number);
      if (!foot.planted) anchors.delete(foot.leg);
      else if (!anchors.has(foot.leg)) anchors.set(foot.leg, ankle);
      else {
        const at = anchors.get(foot.leg) as Vector3;
        const d = Math.hypot(ankle.x - at.x, ankle.z - at.z);
        if (d > slide.worst) Object.assign(slide, { worst: d, leg: foot.leg, time });
      }
      if ((c.legMiss[k] ?? 0) > 0.02 * compiled.scale) stretched[k] = (stretched[k] ?? 0) + 1;
    }
    for (const i of sections) {
      const r = (bones.radii[i] ?? 0) * (bones.sections[i] === 'torso' ? depth : 1);
      const head = pose.worldPos[i] as Vector3;
      const tail = pose.tail(i, a);
      for (const p of [head, tail]) {
        const depth = height(p.x, p.z).height - (p.y - r);
        if (depth > pen.worst)
          Object.assign(pen, { worst: depth, part: bones.owners[i] ?? 'body', time });
      }
    }
    if (frames % 4 !== 0) continue;
    if (rig.wings.length + rig.fins.length > 0) {
      const w = wingHits(compiled, pose, height);
      if (w.worst > wingHit.worst) wingHit = { ...w, time };
    }
    if (rig.tentacles.length > 0) {
      const t = tentacleHits(compiled, pose);
      if (t.worst > tentacleHit.worst) tentacleHit = { ...t, time };
    }
    for (let i = 0; i < headBones.length; i++)
      for (let j = i + 1; j < headBones.length; j++)
        for (const bi of headBones[i] as number[])
          for (const bj of headBones[j] as number[]) {
            a.copy(pose.worldPos[bi] as Vector3);
            pose.tail(bi, b);
            e.copy(pose.worldPos[bj] as Vector3);
            pose.tail(bj, f);
            const overlap =
              ((bones.radii[bi] ?? 0) + (bones.radii[bj] ?? 0)) * 0.8 - segmentDistance(a, b, e, f);
            if (overlap > crowd.worst)
              Object.assign(crowd, {
                worst: overlap,
                between: [rig.heads[i]?.id ?? '', rig.heads[j]?.id ?? ''],
                time,
              });
          }
    // Limbs through limbs, and lower limb bones through the body.
    for (let i = 0; i < legBones.length; i++) {
      for (let j = i + 1; j < legBones.length; j++) {
        for (const [k, bi] of (legBones[i] as number[]).entries()) {
          for (const bj of legBones[j] as number[]) {
            a.copy(pose.worldPos[bi] as Vector3);
            pose.tail(bi, b);
            e.copy(pose.worldPos[bj] as Vector3);
            pose.tail(bj, f);
            const overlap =
              ((bones.radii[bi] ?? 0) + (bones.radii[bj] ?? 0)) * 0.8 - segmentDistance(a, b, e, f);
            meet(
              overlap,
              [rig.legs[i]?.id ?? '', rig.legs[j]?.id ?? ''],
              segmentOf(k, (legBones[i] as number[]).length),
              time,
            );
          }
        }
      }
      for (const [k, bi] of (legBones[i] as number[]).entries()) {
        if (k === 0) continue;
        for (const s of sections) {
          if (bones.sections[s] !== 'torso' && bones.sections[s] !== 'tail') continue;
          a.copy(pose.worldPos[bi] as Vector3);
          pose.tail(bi, b);
          e.copy(pose.worldPos[s] as Vector3);
          pose.tail(s, f);
          const overlap =
            ((bones.radii[bi] ?? 0) + (bones.radii[s] ?? 0)) * 0.7 - segmentDistance(a, b, e, f);
          meet(
            overlap,
            [rig.legs[i]?.id ?? '', bones.owners[s] ?? 'body'],
            segmentOf(k, (legBones[i] as number[]).length),
            time,
          );
        }
      }
    }
  }
  const worstLeg = stretched.reduce((best, n, k) => (n > (stretched[best] ?? 0) ? k : best), 0);
  return {
    ground,
    gait: c.gait?.id ?? 'none',
    speed,
    seconds: c.time - start,
    footSlide: slide,
    penetration: pen,
    overstretch: {
      worst: frames > 0 ? (stretched[worstLeg] ?? 0) / frames : 0,
      ...(rig.legs[worstLeg] ? { leg: rig.legs[worstLeg].id } : {}),
    },
    intersection: { ...hit, pairs: [...pairs.values()].sort((x, y) => y.worst - x.worst) },
    heads: crowd,
    wings: wingHit,
    tentacles: tentacleHit,
  };
}

/**
 * How deep any tentacle passes into the body, the legs or the arms in a pose: its bones past the
 * first quarter (which leaves the skin), as capsules.
 */
function tentacleHits(compiled: CompiledCreature, pose: Pose): TentacleHit {
  const rig = compiled.rig;
  const bones = compiled.bones;
  const sections = new Set(['torso', 'neck', 'head', 'jaw', 'tail']);
  const others = [
    ...bones.names.map((_, i) => i).filter((i) => sections.has(bones.sections[i] as string)),
    ...rig.legs.flatMap((l) => l.bones.slice(1)),
    ...rig.arms.flatMap((a) => a.bones),
  ];
  let worst: TentacleHit = { worst: 0 };
  const a = new Vector3();
  const b = new Vector3();
  const e = new Vector3();
  const f = new Vector3();
  for (const t of rig.tentacles)
    for (const id of t.bones.slice(Math.ceil(t.bones.length / 4))) {
      a.copy(pose.worldPos[id] as Vector3);
      pose.tail(id, b);
      const r = bones.radii[id] ?? 0;
      for (const s of others) {
        e.copy(pose.worldPos[s] as Vector3);
        pose.tail(s, f);
        const depth = (bones.radii[s] ?? 0) * 0.8 + r * 0.8 - segmentDistance(a, b, e, f);
        if (depth > worst.worst)
          worst = { worst: depth, tentacle: t.id, against: bones.owners[s] ?? 'body' };
      }
    }
  return worst;
}

/** The rest pose with the wings spread by `spread`, standing still. */
function standing(compiled: CompiledCreature, spread: number): Pose {
  const pose = new Pose(compiled.bones);
  applyRest(pose, compiled.rig, { spread });
  return pose;
}

/**
 * How deep any wing or fin passes into the body, the legs, the arms or the ground in a pose:
 * its bones past the shoulder (as capsules), and every 8th membrane vertex skinned by the pose,
 * except where a membrane meets the body or a leg (within a tenth of its panel's width).
 */
function wingHits(
  compiled: CompiledCreature,
  pose: Pose,
  ground: Ground = () => ({ height: 0 }),
): WingHit {
  const rig = compiled.rig;
  const bones = compiled.bones;
  const owner = (b: number) => bones.owners[b] ?? '';
  const sections = new Set(['torso', 'neck', 'head', 'jaw', 'tail']);
  const body = bones.names
    .map((_, i) => i)
    .filter((i) => sections.has(bones.sections[i] as string));
  const legs = [...rig.legs.flatMap((l) => l.bones.slice(1)), ...rig.arms.flatMap((a) => a.bones)];
  // A wing covered by a case rests inside the body by design, until it is fully spread.
  const spread = (w: (typeof rig.wings)[number]) =>
    (pose.rot[w.bones[0] as number] as Quaternion).angleTo(
      pose.bindRot[w.bones[0] as number] as Quaternion,
    ) < 0.05;
  const shown = rig.wings.filter((w) => !w.covered || spread(w));
  const hidden = new Set(rig.wings.filter((w) => !shown.includes(w)).map((w) => w.id));
  const chains = [
    ...shown.map((w) => ({ id: w.id, bones: [...w.bones.slice(1), ...w.digits.flat()] })),
    ...rig.fins.map((f) => ({ id: f.id, bones: f.bones.slice(1) })),
  ];
  const wingish = new Set<number>();
  for (const w of rig.wings)
    for (const b of [...w.bones, ...w.digits.flat(), ...w.feathers]) wingish.add(b);
  for (const f of rig.fins) for (const b of f.bones) wingish.add(b);
  let worst: WingHit = { worst: 0 };
  const a = new Vector3();
  const b = new Vector3();
  const e = new Vector3();
  const f = new Vector3();
  const consider = (depth: number, wing: string, against: string) => {
    if (depth > worst.worst) worst = { worst: depth, wing, against };
  };
  const into = (p: Vector3, r: number, wing: string) => {
    for (const s of body) {
      e.copy(pose.worldPos[s] as Vector3);
      pose.tail(s, f);
      consider((bones.radii[s] ?? 0) * 0.8 + r - pointSegment(p, e, f), wing, owner(s));
    }
    for (const l of legs) {
      e.copy(pose.worldPos[l] as Vector3);
      pose.tail(l, f);
      consider((bones.radii[l] ?? 0) * 0.8 + r - pointSegment(p, e, f), wing, owner(l));
    }
    consider(ground(p.x, p.z).height - (p.y - r), wing, 'ground');
  };
  for (const chain of chains)
    for (const id of chain.bones) {
      a.copy(pose.worldPos[id] as Vector3);
      pose.tail(id, b);
      const r = bones.radii[id] ?? 0;
      into(a.clone().lerp(b, 0.5), r, chain.id);
      into(b, r, chain.id);
    }
  // Membrane vertices, skinned.
  const m = compiled.membranes;
  const count = m.positions.length / 3;
  if (count === 0) return worst;
  const skin = bones.names.map((_, i) => {
    const bindRot = (pose.bindWorldRot[i] as Quaternion).clone().invert();
    return { bindRot, bindPos: pose.bindWorldPos[i] as Vector3 };
  });
  const wingOf = (bone: number): string | undefined => {
    for (let x = bone, n = 0; x >= 0 && n < 64; x = pose.parents[x] ?? -1, n++) {
      if (wingish.has(x)) {
        const name = owner(x);
        return name;
      }
    }
    return undefined;
  };
  const p = new Vector3();
  const v = new Vector3();
  const sum = new Vector3();
  // Each wing's and fin's first bone sits in the shoulder (or, for a fin, the body).
  const roots = new Set([...rig.wings, ...rig.fins].map((w) => w.bones[0] as number));
  for (let i = 0; i < count; i += 8) {
    let attached = 0;
    let root = 0;
    let wing: string | undefined;
    sum.set(0, 0, 0);
    for (let k = 0; k < 4; k++) {
      const w = m.skinWeight[i * 4 + k] as number;
      if (w <= 0) continue;
      const bone = m.skinIndex[i * 4 + k] as number;
      const parent = pose.parents[bone] ?? -1;
      // A station on the body's or a leg's spar: the membrane's attached edge; or the root.
      const station = bones.sections[bone] === 'station';
      if (station && !wingish.has(parent)) attached += w;
      else if (roots.has(station ? parent : bone)) root += w;
      else wing ??= wingOf(bone);
      const sk = skin[bone] as { bindRot: Quaternion; bindPos: Vector3 };
      v.set(
        m.positions[i * 3] as number,
        m.positions[i * 3 + 1] as number,
        m.positions[i * 3 + 2] as number,
      )
        .sub(sk.bindPos)
        .applyQuaternion(sk.bindRot)
        .applyQuaternion(pose.worldRot[bone] as Quaternion)
        .add(pose.worldPos[bone] as Vector3);
      sum.addScaledVector(v, w);
    }
    if (attached > 0.9 || root > 0.5 || !wing || hidden.has(wing)) continue;
    p.copy(sum);
    into(p, 0, wing);
  }
  return worst;
}

function pointSegment(p: Vector3, a: Vector3, b: Vector3): number {
  const ab = new Vector3().subVectors(b, a);
  const t = Math.min(1, Math.max(0, p.clone().sub(a).dot(ab) / Math.max(1e-12, ab.lengthSq())));
  return p.distanceTo(a.clone().addScaledVector(ab, t));
}

/** A paragraph a model (or person) can read instead of looking. */
export function describeCreature(
  spec: CreatureSpec,
  registry: Registry,
  m: Analysis['measurements'],
  speed: Analysis['speed'],
): string {
  const metres = (v: number) => (v >= 1 ? `${v.toFixed(1)} m` : `${Math.round(v * 100)} cm`);
  const kg = (v: number) =>
    v >= 1000
      ? `${(v / 1000).toFixed(1)} t`
      : v >= 10
        ? `${Math.round(v)} kg`
        : v >= 0.1
          ? `${v.toFixed(1)} kg`
          : `${Math.max(1, Math.round(v * 1000))} g`;
  const legs = spec.limbs.filter((l) => l.role === 'leg').length;
  const arms = spec.limbs.filter((l) => l.role === 'arm').length;
  const front = isUprightFront(spec);
  const finned = spec.limbs.some((l) => l.role === 'fin');
  const tentacles = spec.limbs.filter((l) => l.role === 'tentacle').length;
  const plan =
    legs === 0
      ? finned
        ? 'finned swimmer'
        : tentacles > 0
          ? 'tentacled creature'
          : 'legless serpent'
      : legs === 2
        ? 'biped'
        : legs === 4
          ? 'quadruped'
          : `${legs}-legged creature`;
  const upright = m.height > m.length;
  const size = upright
    ? `a ${metres(m.height)} tall ${plan}`
    : `a ${metres(m.length)} long, ${metres(m.height)} tall ${plan}`;
  const body: string[] = [];
  // Proportions, in the words prompts use.
  const torsoRadius = Math.max(...spec.body.torso.radius);
  if (spec.body.torso.crossSection === 'wide') body.push('a broad, flat body');
  else if (spec.body.torso.crossSection === 'tall') body.push('a deep, narrow body');
  if (legs === 2 && spec.body.torso.pitch < 35) body.push('a horizontal body');
  const legList = spec.limbs.filter((l) => l.role === 'leg');
  if (legList.length > 0) {
    const legLength = legList.reduce((s, l) => s + l.length, 0) / legList.length;
    const splay = legList.reduce((s, l) => s + l.splay, 0) / legList.length;
    const [short, long] = legs === 2 ? [1, 1.5] : [0.45, 0.8];
    const sprawl = splay > 25 ? 'sprawling ' : '';
    if (legLength < short) body.push(`short ${sprawl}legs`);
    else if (legLength > long) body.push(`long ${sprawl}legs`);
    else if (sprawl) body.push('sprawling legs');
  }
  // An upright front is a torso, not a neck (docs/design/9.2-legs-centaurs.md).
  if (front) body.push('an upright torso');
  if (arms > 0) body.push(`${arms === 2 ? 'two' : arms} arms`);
  // Tentacles (docs/design/9.4-tentacles-parts.md).
  const counts = [
    '',
    'one',
    'two',
    'three',
    'four',
    'five',
    'six',
    'seven',
    'eight',
    'nine',
    'ten',
  ];
  if (tentacles > 0)
    body.push(tentacles === 1 ? 'a tentacle' : `${counts[tentacles] ?? tentacles} tentacles`);
  // Wings and fins, as their membranes describe them (docs/design/9.3-wings-fins.md).
  const spans = new Map<string, CreatureSpec['limbs'][number][]>();
  for (const l of spec.limbs)
    if (l.role === 'wing' || l.role === 'fin') {
      const id = l.id.replace(/\.[LR]$/, '');
      spans.set(id, [...(spans.get(id) ?? []), l]);
    }
  for (const group of spans.values()) {
    const first = group[0] as CreatureSpec['limbs'][number];
    const count = group.length;
    const module = first.membrane
      ? (registry.get('part', first.membrane.type) as PartModule | undefined)
      : undefined;
    body.push(
      module?.describe?.(first.membrane?.params ?? {}, { count }) ??
        (first.role === 'fin'
          ? count === 1
            ? 'a flipper'
            : 'flippers'
          : count === 1
            ? 'a wing'
            : 'wings'),
    );
  }
  const head = spec.body.head;
  const words = ['', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine'];
  const heads = spec.body.neck.count;
  const neck = spec.body.neck.length;
  const neckWord = front
    ? ''
    : neck >= 1
      ? 'very long neck'
      : neck >= 0.6
        ? 'long neck'
        : neck > 0 && neck < 0.2
          ? 'short neck'
          : '';
  if (heads > 1) {
    const size =
      head.length >= 0.4 || head.radius >= 0.9 * torsoRadius
        ? 'big '
        : head.length <= 0.18 && head.radius <= 0.5 * torsoRadius
          ? 'small '
          : '';
    body.push(
      `${words[heads] ?? heads} ${size}heads${neck > 0 ? ` on ${neckWord ? `${neckWord}s` : 'necks'}` : ''}`,
    );
  } else {
    if (head.length >= 0.4 || head.radius >= 0.9 * torsoRadius) body.push('a big head');
    else if (head.length <= 0.18 && head.radius <= 0.5 * torsoRadius) body.push('a small head');
    if (neckWord) body.push(`a ${neckWord}`);
  }
  const tail = spec.body.tail.length;
  if (tail > 0) {
    const kind = Math.abs(spec.body.tail.curl) > 120 ? ' curled' : '';
    const word =
      tail >= 2
        ? `very long${kind}`
        : tail >= 1
          ? `long${kind}`
          : tail < 0.4
            ? `short${kind}`
            : kind.trim();
    const tails = spec.body.tail.count;
    if (tails > 1)
      body.push(
        `${words[tails] ?? tails} ${word ? `${word} ` : ''}tails${spec.body.tail.forkAt > 0 ? ' forking from one' : ''}`,
      );
    else body.push(word ? `a ${word} tail` : 'a tail');
  }
  const where: Record<string, string> = {
    head: 'on its head',
    jaw: 'on its jaw',
    spine: 'along its back',
    torso: 'on its body',
    neck: 'on its neck',
    tail: 'on its tail',
  };
  const seen = new Set<string>();
  for (const part of spec.parts) {
    if (seen.has(part.baseId)) continue;
    seen.add(part.baseId);
    const module = registry.get('part', part.type) as PartModule | undefined;
    const count = spec.parts.filter((p) => p.baseId === part.baseId).length;
    const phrase = module?.describe?.(part.params, { count, on: part.on }) ?? part.baseId;
    // Area parts say which area of the body they cover (docs/design/9.5-coverings.md).
    const area =
      module?.slot === 'area' && part.area && (part.on === 'torso' || part.on === 'spine')
        ? part.area === 'all'
          ? 'all over it'
          : `on its ${part.area === 'sides' ? 'sides' : part.area}`
        : undefined;
    const place =
      module?.slot === 'mouth' || module?.material === 'eye'
        ? ''
        : (area ??
          (part.on === 'torso' && Math.abs(part.angle) < 30 ? 'along its back' : where[part.on]));
    body.push(place ? `${phrase} ${place}` : phrase);
  }
  const foot = spec.limbs.find((l) => l.role === 'leg' && l.foot)?.foot;
  if (foot) {
    const module = registry.get('part', foot.type) as PartModule | undefined;
    if (module?.describe) body.push(module.describe(foot.params, { count: legs }));
  }
  // Hands (a scorpion's pincers) as their module says.
  const hand = spec.limbs.find((l) => l.role === 'arm' && l.foot)?.foot;
  if (hand) {
    const module = registry.get('part', hand.type) as PartModule | undefined;
    if (module?.describe) body.push(module.describe(hand.params, { count: arms }));
  }
  const skin = [
    `${colorName(spec.skin.palette.base ?? '#808080')} ${spec.skin.material}`,
    ...spec.skin.layers.map((layer) => {
      const module = registry.get('pattern', layer.type) as PatternModule | undefined;
      const phrase = module?.describe?.(layer.params, { region: layer.region }) ?? layer.type;
      return layer.region === 'all' ? phrase : `${phrase} on the ${layer.region}`;
    }),
  ];
  // Walking gaits only: swimming and flying are said apart.
  const offLand = new Set(
    spec.motion.gaits
      .filter(
        (g) =>
          ((registry.get('gait', g.type) as GaitModule | undefined)?.medium ?? 'land') !== 'land',
      )
      .map((g) => g.type),
  );
  const gaits = speed.gaits.map((g) => g.id).filter((g) => !offLand.has(g));
  const swims =
    (speed.swim !== undefined ? ` and swims up to ${speed.swim.toFixed(1)} m/s` : '') +
    (speed.fly !== undefined ? `, and flies at about ${speed.fly.toFixed(0)} m/s` : '');
  const moves =
    gaits.length === 0 && speed.swim !== undefined
      ? `It swims at about ${speed.walk.toFixed(1)} m/s, up to ${speed.swim.toFixed(1)} m/s`
      : legs === 0
        ? `It slithers at about ${speed.walk.toFixed(1)} m/s${swims}`
        : `It walks at about ${speed.walk.toFixed(1)} m/s${gaits.length > 1 ? ` and ${gaits.at(-1)}s up to ${speed.max.toFixed(1)} m/s` : ''}${swims}`;
  // Ambient actions (breathing, blinks) run all the time; the description lists what it can do.
  const actions = spec.motion.actions
    .map((a) => a.type)
    .filter((a) => !(registry.get('action', a) as ActionModule | undefined)?.hooks?.ambient);
  // Two layers that read the same are said once; two parts that do are counted.
  const unique = (items: readonly string[]) => [...new Set(items)];
  const counted = (items: readonly string[]) =>
    unique(items).map((item) => {
      const n = items.filter((x) => x === item).length;
      return n > 1 ? `${item} (×${n})` : item;
    });
  const list = (items: readonly string[]) =>
    items.length <= 1 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`;
  return [
    `${spec.name}: ${size} of about ${kg(m.mass)}${body.length ? `, with ${list(counted(body))}` : ''}.`,
    `Skin: ${list(unique(skin))}.`,
    `${moves}; ${spec.motion.temperament} temperament${actions.length ? `; it can ${list(actions)}` : ''}.`,
  ]
    .join(' ')
    .replace(/\ba (?=[aeiou])/g, 'an ');
}

/** How far a foot's toes reach from its ankle, across the ground (metres). */
function toeReach(
  compiled: CompiledCreature,
  ankle: readonly [number, number, number],
  toes: readonly (readonly number[])[],
): number {
  const { positions, rotations, lengths } = compiled.bones;
  let reach = 0;
  for (const toe of toes)
    for (const b of toe) {
      const tail = new Vector3(0, 1, 0)
        .applyQuaternion(new Quaternion().fromArray(rotations, b * 4))
        .multiplyScalar(lengths[b] as number)
        .add(new Vector3().fromArray(positions, b * 3));
      reach = Math.max(reach, Math.hypot(tail.x - ankle[0], tail.z - ankle[2]));
    }
  return reach;
}
