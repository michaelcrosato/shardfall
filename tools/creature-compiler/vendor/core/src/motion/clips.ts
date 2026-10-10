import type { Quaternion, Vector3 } from 'three';
import type { CompiledCreature } from '../compile/compile.ts';
import { mainHead } from '../compile/types.ts';
import type { Registry } from '../registry.ts';
import { type Ground, MotionController, type MotionEvent, type Water } from './controller.ts';
import { openSea } from './terrain.ts';

const FLAT: Ground = () => ({ height: 0 });

/**
 * One animation baked from the motion controller: a local rotation and position per bone per
 * frame, as plain arrays. The root stays at the origin facing +Z, so gait clips are one cycle in
 * place and loop; `distance` is how far one cycle carries the creature at `speed`.
 */
export interface BakedClip {
  readonly name: string;
  /** Seconds. */
  readonly duration: number;
  readonly loop: boolean;
  /** Frames, evenly spaced; the last frame is at `duration` (equal to the first for loops). */
  readonly frames: number;
  /** frames × bones × 4: local rotations (x, y, z, w). */
  readonly rotations: Float32Array;
  /** frames × bones × 3: local positions. */
  readonly positions: Float32Array;
  /** Per frame: eyelids, 0 open to 1 shut. */
  readonly blink: Float32Array;
  /** Per frame: chest expansion, 0 to 1. */
  readonly breath: Float32Array;
  /** Speed the clip was baked at (m/s), and the distance one cycle covers (m). */
  readonly speed: number;
  readonly distance: number;
  /** Events in the clip (footsteps, action moments), timed from its start. */
  readonly events: readonly MotionEvent[];
  /**
   * The root moves in the clip (a jump, a pounce): its track carries the creature from where the
   * clip starts, so a game either lets it move the creature or strips it (docs/runtime.md).
   */
  readonly rootMotion?: boolean;
  /**
   * An air cycle (`fly`, `glide`, `hover`): the root sits at the origin in the air, and the body
   * pitch (radians, nose up) it was baked at, so a game can tilt it to the flight path.
   */
  readonly air?: { readonly pitch: number };
}

export interface BakeOptions {
  /** Frames per second (default 30). */
  readonly fps?: number;
  /** Clips to bake: "idle", gait ids and action ids. Default: idle, every gait, every action. */
  readonly clips?: readonly string[];
  /** Seconds of idle to bake (default 4). */
  readonly idleSeconds?: number;
}

const STEP = 1 / 120;

/**
 * What `clips` may name for this creature: idle, its gaits and its actions, `death`, and for a
 * flyer `takeoff` and `land`.
 */
export function clipNames(compiled: CompiledCreature, registry: Registry): string[] {
  const controller = new MotionController(compiled, { registry });
  const actions = controller.actions();
  const flight = controller.canFly
    ? ['takeoff', 'land'].filter((name) => !actions.includes(name))
    : [];
  const death = actions.includes('death') ? [] : ['death'];
  return ['idle', ...compiled.motion.gaits.map((g) => g.id), ...actions, ...flight, ...death];
}

/**
 * Bakes animation clips from the motion controller on flat ground: `idle` (breathing, blinks,
 * glances), one in-place cycle of each gait at its natural speed, and each action aimed at a
 * point in front of the head. Deterministic, like the controller.
 */
export function bakeClips(
  compiled: CompiledCreature,
  registry: Registry,
  options: BakeOptions = {},
): BakedClip[] {
  const fps = options.fps ?? 30;
  const known = clipNames(compiled, registry);
  const wanted = options.clips ?? known;
  for (const name of wanted)
    if (!known.includes(name)) {
      // A module the creature lacks: say where it goes.
      const fix = registry.get('action', name)
        ? `; add "${name}" to motion.actions to get it`
        : registry.get('gait', name)
          ? `; add "${name}" to motion.gaits to get it`
          : '';
      throw new Error(`no clip "${name}" for this creature; it has ${known.join(', ')}${fix}`);
    }
  const gaits = new Map(compiled.motion.gaits.map((g) => [g.id, g]));
  const actions = new MotionController(compiled, { registry }).actions();
  return wanted.map((name) => {
    if (name === 'idle') return bakeIdle(compiled, registry, fps, options.idleSeconds ?? 4);
    if (gaits.get(name)?.medium === 'air') return bakeAir(compiled, registry, fps, name);
    if (gaits.has(name)) return bakeGait(compiled, registry, fps, name);
    if (name === 'takeoff' && !actions.includes(name)) return bakeTakeoff(compiled, registry, fps);
    if (name === 'land' && !actions.includes(name)) return bakeLand(compiled, registry, fps);
    if (name === 'death' && !actions.includes(name)) return bakeDeath(compiled, registry, fps);
    return bakeAction(compiled, registry, fps, name);
  });
}

/** Records `frames` poses, advancing the controller by `dt` between them. */
function record(
  controller: MotionController,
  name: string,
  frames: number,
  dt: number,
  extra: { loop: boolean; speed: number; rootMotion?: boolean },
  input: { ground?: Ground; water?: Water } = {},
): BakedClip {
  const pose = controller.pose;
  const n = pose.count;
  const rotations = new Float32Array(frames * n * 4);
  const positions = new Float32Array(frames * n * 3);
  const blink = new Float32Array(frames);
  const breath = new Float32Array(frames);
  const events: MotionEvent[] = [];
  const start = controller.time;
  const startX = controller.position.x;
  const startZ = controller.position.z;
  for (let f = 0; f < frames; f++) {
    if (f > 0)
      for (const event of controller.update(dt, input))
        events.push({ ...event, time: event.time - start });
    // The root stays at the origin facing +Z: a game places the creature, the clip moves it.
    // With root motion it starts there and travels, as the action carries it.
    const x = extra.rootMotion ? startX : controller.position.x;
    const z = extra.rootMotion ? startZ : controller.position.z;
    for (let b = 0; b < n; b++) {
      const q = pose.rot[b];
      const p = pose.pos[b];
      if (!q || !p) continue;
      rotations.set([q.x, q.y, q.z, q.w], (f * n + b) * 4);
      const root = pose.parents[b] === -1;
      positions.set([p.x - (root ? x : 0), p.y, p.z - (root ? z : 0)], (f * n + b) * 3);
    }
    blink[f] = pose.blink;
    breath[f] = pose.breath;
  }
  if (extra.loop) closeLoop(rotations, positions, frames, n);
  const duration = (frames - 1) * dt;
  return {
    name,
    duration,
    loop: extra.loop,
    frames,
    rotations,
    positions,
    blink,
    breath,
    speed: extra.speed,
    distance: Math.hypot(controller.position.x - startX, controller.position.z - startZ),
    events,
    ...(extra.rootMotion ? { rootMotion: true } : {}),
  };
}

/**
 * Makes a loop seamless: springs and idle glances leave the last frame a little off the first,
 * so the difference is spread over the clip (none at the start, all of it at the end).
 */
function closeLoop(rotations: Float32Array, positions: Float32Array, frames: number, n: number) {
  const last = frames - 1;
  for (let b = 0; b < n; b++) {
    const first = b * 4;
    const end = (last * n + b) * 4;
    // Compare like with like: q and -q are the same rotation.
    let dot = 0;
    for (let j = 0; j < 4; j++)
      dot += (rotations[first + j] as number) * (rotations[end + j] as number);
    const sign = dot < 0 ? -1 : 1;
    const delta = [0, 1, 2, 3].map(
      (j) => (rotations[first + j] as number) * sign - (rotations[end + j] as number),
    );
    const pDelta = [0, 1, 2].map(
      (j) => (positions[b * 3 + j] as number) - (positions[(last * n + b) * 3 + j] as number),
    );
    for (let f = 1; f <= last; f++) {
      const t = f / last;
      const o = (f * n + b) * 4;
      let len = 0;
      for (let j = 0; j < 4; j++) {
        const v = (rotations[o + j] as number) + (delta[j] as number) * t;
        rotations[o + j] = v;
        len += v * v;
      }
      len = Math.sqrt(len) || 1;
      for (let j = 0; j < 4; j++) rotations[o + j] = (rotations[o + j] as number) / len;
      for (let j = 0; j < 3; j++)
        positions[(f * n + b) * 3 + j] =
          (positions[(f * n + b) * 3 + j] as number) + (pDelta[j] as number) * t;
    }
    // The last frame is now the first up to sign; make it identical.
    for (let j = 0; j < 4; j++) rotations[end + j] = (rotations[first + j] as number) * sign;
  }
}

function bakeIdle(
  compiled: CompiledCreature,
  registry: Registry,
  fps: number,
  seconds: number,
): BakedClip {
  const controller = new MotionController(compiled, { registry });
  for (let i = 0; i < 120; i++) controller.update(STEP);
  // With no ambient action (breathing, blinks) the creature just stands: two frames of it.
  const ambient = compiled.motion.actions.some((a) => registry.get('action', a.id)?.hooks?.ambient);
  if (!ambient) return record(controller, 'idle', 2, 1, { loop: true, speed: 0 });
  return record(controller, 'idle', Math.round(seconds * fps) + 1, 1 / fps, {
    loop: true,
    speed: 0,
  });
}

function bakeGait(
  compiled: CompiledCreature,
  registry: Registry,
  fps: number,
  gait: string,
): BakedClip {
  const controller = new MotionController(compiled, { registry });
  // Swimming gaits are baked in open water, the surface at y 0 (10.3).
  const water = compiled.motion.gaits.find((g) => g.id === gait)?.medium === 'water';
  const input: { ground?: Ground; water?: Water } = water ? openSea(compiled.scale) : {};
  if (water) controller.place(0, 0, 0, input.ground, input.water);
  controller.lockGait(gait);
  const speed = controller.gaitSpeed(gait);
  controller.drive(speed, 0);
  // Settle into the gait, then start at the top of a cycle.
  for (let i = 0; i < 480; i++) controller.update(STEP, input);
  let last = controller.phase;
  for (let guard = 0; ; guard++) {
    controller.update(STEP, input);
    if (controller.phase < last) break;
    last = controller.phase;
    if (guard > 4800) throw new Error(`the ${gait} cycle did not advance`);
  }
  // Measure one cycle on a copy of the run: the controller is deterministic, so a second
  // controller in the same state is not needed; time the next cycle instead and bake the one
  // after it at exactly that length.
  const t0 = controller.time;
  last = controller.phase;
  for (let guard = 0; ; guard++) {
    controller.update(STEP, input);
    if (controller.phase < last) break;
    last = controller.phase;
    if (guard > 4800) throw new Error(`the ${gait} cycle did not advance`);
  }
  const cycle = controller.time - t0;
  // At least 12 frames a cycle, so quick little steps still read.
  const frames = Math.max(12, Math.round(cycle * fps)) + 1;
  return record(controller, gait, frames, cycle / (frames - 1), { loop: true, speed }, input);
}

function bakeAction(
  compiled: CompiledCreature,
  registry: Registry,
  fps: number,
  action: string,
): BakedClip {
  const controller = new MotionController(compiled, { registry });
  for (let i = 0; i < 120; i++) controller.update(STEP);
  const head = controller.pose.worldPos[mainHead(compiled.rig).head];
  const target = head
    ? { x: head.x, y: head.y, z: head.z + compiled.scale * 0.6 }
    : { x: 0, y: compiled.scale * 0.5, z: compiled.scale * 2 };
  // A leap goes as far as it leaps unaimed, and plans its arc (so its length) on its first step.
  const leaps = registry.get('action', action)?.hooks?.leap !== undefined;
  controller.act(action, leaps ? {} : { target });
  if (leaps) controller.update(STEP);
  const duration = controller.actionState?.duration ?? 1;
  // A little after the action ends, so it settles back.
  const frames = Math.round((duration + 0.25) * fps) + 1;
  return record(controller, action, frames, 1 / fps, {
    loop: false,
    speed: 0,
    ...(leaps ? { rootMotion: true } : {}),
  });
}

/**
 * An air gait's cycle (docs/design/10.4-flight.md): flown level at cruise (a hover in place) high
 * above flat ground until the strokes settle, then posed frame by frame at exact phases over whole
 * wingbeats, at least 0.5 s of them, the root at the origin. A glide holds still, so half a
 * second of it. Never baked on land.
 */
function bakeAir(
  compiled: CompiledCreature,
  registry: Registry,
  fps: number,
  gait: string,
): BakedClip {
  const controller = new MotionController(compiled, { registry });
  const fl = controller.flightNumbers;
  if (!fl || !controller.canFly) throw new Error(`"${gait}" needs wings to fly with`);
  // High enough that a glide, sinking all the while, never nears the ground.
  const y = 50 + 20 * fl.cruise;
  controller.place(0, 0, 0, FLAT, undefined, { flying: true, y });
  controller.lockGait(gait);
  const speed = controller.gaitSpeed(gait);
  controller.fly({ height: y, speed });
  controller.drive(speed, 0);
  for (let i = 0; i < 4 * 120; i++) controller.update(STEP);
  const beat = controller.wingbeat;
  const beats = beat > 0 ? Math.max(1, Math.ceil(0.5 * beat - 1e-9)) : 0;
  const duration = beats > 0 ? beats / beat : 0.5;
  // At least 8 frames a beat, so a fast stroke still reads.
  const frames = Math.max(8 * Math.max(1, beats), Math.round(duration * fps)) + 1;
  const pose = controller.pose;
  const n = pose.count;
  const rotations = new Float32Array(frames * n * 4);
  const positions = new Float32Array(frames * n * 3);
  const start = controller.phase;
  const at = controller.position.clone();
  for (let f = 0; f < frames; f++) {
    controller.poseBeat(start + (beats * f) / (frames - 1));
    for (let b = 0; b < n; b++) {
      const q = pose.rot[b] as Quaternion;
      const p = pose.pos[b] as Vector3;
      rotations.set([q.x, q.y, q.z, q.w], (f * n + b) * 4);
      const root = pose.parents[b] === -1;
      positions.set(root ? [p.x - at.x, p.y - at.y, p.z - at.z] : [p.x, p.y, p.z], (f * n + b) * 3);
    }
  }
  // Exact phases close the loop; the blink and breath hold.
  const blink = new Float32Array(frames).fill(pose.blink);
  const breath = new Float32Array(frames).fill(pose.breath);
  const events: MotionEvent[] = [];
  // One flap a beat, as the downstroke starts (phase 0.25).
  for (let k = 0; k < beats; k++) {
    const t = (((((0.25 - start) % 1) + 1) % 1) + k) / beat;
    if (t < duration) events.push({ type: 'flap', time: t });
  }
  events.sort((a, b) => a.time - b.time);
  return {
    name: gait,
    duration,
    loop: true,
    frames,
    rotations,
    positions,
    blink,
    breath,
    speed,
    distance: speed * duration,
    events,
    air: { pitch: controller.attitude.pitch },
  };
}

/**
 * A takeoff from standing on flat ground (root motion): the crouch, the leap and the climb into
 * flight, ending half a second after powered flight begins, heading +Z.
 */
function bakeTakeoff(compiled: CompiledCreature, registry: Registry, fps: number): BakedClip {
  const controller = new MotionController(compiled, { registry });
  for (let i = 0; i < 120; i++) controller.update(STEP);
  const probe = new MotionController(compiled, { registry });
  for (let i = 0; i < 120; i++) probe.update(STEP);
  probe.fly();
  probe.drive(probe.flightNumbers?.cruise ?? 1, 0);
  // Time it on a twin: the controller is deterministic.
  let t = 0;
  let flying = 0;
  for (let guard = 0; guard < 120 * 30 && flying < 0.5; guard++) {
    probe.update(STEP);
    t += STEP;
    if (probe.flightStage === 'flight') flying += STEP;
  }
  controller.fly();
  controller.drive(controller.flightNumbers?.cruise ?? 1, 0);
  const frames = Math.round(t * fps) + 1;
  return record(controller, 'takeoff', frames, t / (frames - 1), {
    loop: false,
    speed: 0,
    rootMotion: true,
  });
}

/**
 * A landing on flat ground (root motion): from the start of the flare, coming in at slow flight
 * heading +Z (a hoverer's from the start of its descent), to the feet planted half a second
 * after touchdown.
 */
function bakeLand(compiled: CompiledCreature, registry: Registry, fps: number): BakedClip {
  const approach = () => {
    const controller = new MotionController(compiled, { registry });
    const fl = controller.flightNumbers;
    if (!fl) throw new Error('"land" needs wings to fly with');
    controller.place(0, 0, 0, FLAT, undefined, { flying: true });
    controller.land({ x: 0, z: 6 * fl.height + 8 * fl.slow });
    return controller;
  };
  // Time it on a twin: from the flare to half a second after touchdown.
  const probe = approach();
  let before = 0;
  let length = 0;
  let down = -1;
  for (let guard = 0; guard < 120 * 120; guard++) {
    const events = probe.update(STEP);
    const stage = probe.flightStage;
    if (stage !== 'flare' && stage !== 'descend' && probe.flying) before++;
    else length += STEP;
    if (down < 0 && events.some((e) => e.type === 'land')) down = 0;
    else if (down >= 0) down += STEP;
    if (down >= 0.5) break;
  }
  const controller = approach();
  for (let i = 0; i < before; i++) controller.update(STEP);
  const frames = Math.round(length * fps) + 1;
  return record(controller, 'land', frames, length / (frames - 1), {
    loop: false,
    speed: 0,
    rootMotion: true,
  });
}

/**
 * A death on flat ground (docs/design/10.5-hits-death.md): standing, hit from its right, it falls
 * onto its left side; from the blow until half a second after it comes to rest. The root stays
 * in place; the body lies beside where it stood.
 */
function bakeDeath(compiled: CompiledCreature, registry: Registry, fps: number): BakedClip {
  const start = () => {
    const controller = new MotionController(compiled, { registry });
    for (let i = 0; i < 120; i++) controller.update(STEP);
    return controller;
  };
  // Time it on a twin: the controller is deterministic.
  const probe = start();
  probe.die({ direction: { x: 1, z: 0 } });
  let length = 0;
  while (probe.dying && length < 10) {
    probe.update(STEP);
    length += STEP;
  }
  length += 0.5;
  const controller = start();
  controller.die({ direction: { x: 1, z: 0 } });
  const frames = Math.round(length * fps) + 1;
  return record(controller, 'death', frames, length / (frames - 1), { loop: false, speed: 0 });
}
