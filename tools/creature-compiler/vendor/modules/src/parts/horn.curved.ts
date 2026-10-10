import { arc, colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';

const AIMS = ['forward', 'up', 'out', 'back', 'down'] as const;
type Aim = (typeof AIMS)[number];

const params = z.strictObject({
  length: z.number().min(0.02).max(1).default(0.2).describe('Length in torso lengths'),
  width: z.number().min(0.005).max(0.3).default(0.035).describe('Base radius in torso lengths'),
  curve: z
    .number()
    .min(-540)
    .max(540)
    .default(45)
    .describe(
      'Total bend in degrees; positive sweeps back toward the tail, negative forward; past 360 it coils',
    ),
  twist: z
    .number()
    .min(-720)
    .max(720)
    .default(0)
    .describe('Spiral in degrees along the horn, like a ram'),
  lean: z
    .number()
    .min(-90)
    .max(90)
    .default(0)
    .describe('Degrees the root tilts forward (+) or back (-)'),
  turn: z
    .number()
    .min(-180)
    .max(180)
    .default(0)
    .describe(
      'Degrees the bend turns sideways: 90 curves toward the midline (mandibles), -90 away from it',
    ),
  aim: z
    .enum(AIMS)
    .optional()
    .describe(
      'Which way the horn points, wherever it sits: forward (mandibles, a bull), up, out (away from the body), back (swept back) or down (tusks). It replaces lean and turn',
    ),
  ridges: z.number().int().min(0).max(30).default(0).describe('Rings along the horn'),
  color: colorRef('#d4c6a2').describe('Colour at the root: a palette name or a colour'),
  tipColor: colorRef('#3d3329').describe('Colour at the tip'),
});
type Params = z.output<typeof params>;

/**
 * The lean and turn that point a horn's mass (the mean of its centre line) closest to `aim`,
 * from the socket's own frame, so the answer holds wherever the horn sits.
 */
export function aimHorn(
  aim: Aim,
  socket: { readonly normal: Vector3; readonly forward: Vector3; readonly side: Vector3 },
  mirror: number,
  shape: { readonly curve: number; readonly twist: number },
): { lean: number; turn: number } {
  // "out" is away from the midline; on the midline it is straight out of the skin.
  const out = mirror === 0 ? socket.normal.clone() : new Vector3(mirror, 0, 0).normalize();
  const world: Record<Aim, Vector3> = {
    forward: new Vector3(0, 0, 1),
    back: new Vector3(0, 0, -1),
    up: new Vector3(0, 1, 0),
    down: new Vector3(0, -1, 0),
    out,
  };
  const d = world[aim];
  // Pieces are built for the left side and mirrored on the right.
  const target = new Vector3(
    d.dot(socket.side) * (mirror < 0 ? -1 : 1),
    d.dot(socket.normal),
    d.dot(socket.forward),
  );
  let best = { lean: 0, turn: 0, score: -Infinity };
  const mean = new Vector3();
  for (const lean of range(-90, 90, 5)) {
    for (const turn of range(-180, 165, 15)) {
      const points = arc(1, shape.curve, { lean, heading: -turn, twist: shape.twist });
      mean.set(0, 0, 0);
      for (const p of points) mean.add(p);
      // Ties go to the smallest lean and turn.
      const score =
        mean.normalize().dot(target) - 0.002 * (Math.abs(lean) / 90 + Math.abs(turn) / 180);
      if (score > best.score) best = { lean, turn, score };
    }
  }
  return { lean: best.lean, turn: best.turn };
}

function range(from: number, to: number, step: number): number[] {
  const out: number[] = [];
  for (let v = from; v <= to; v += step) out.push(v);
  return out;
}

export default definePart({
  id: 'horn.curved',
  summary: 'Tapered horn bent along an arc; use side "both" for a pair.',
  tags: ['head', 'weapon', 'bone'],
  slot: 'surface',
  material: 'horn',
  attach: { on: 'head', at: 0.75, angle: 40 },
  params,
  example: {
    id: 'horns',
    type: 'horn.curved',
    attach: { on: 'head', at: 0.75, angle: 40, side: 'both' },
    params: { length: 0.25, curve: 60 },
  },
  // Aim replaces lean and turn, so the canonical form leaves them out.
  normalize(params) {
    if (typeof params.aim !== 'string') return { ...params };
    const { lean: _lean, turn: _turn, ...rest } = params;
    return rest;
  },
  describe(p, { count, on }) {
    const curve = Math.abs(p.curve as number);
    const shape =
      curve > 300 ? 'coiled' : curve > 120 ? 'sweeping' : curve > 25 ? 'curved' : 'straight';
    const size = (p.length as number) > 0.4 ? 'long ' : (p.length as number) < 0.1 ? 'short ' : '';
    // A horn on the jaw is a tusk, and one on the tail a stinger.
    const noun = on === 'jaw' ? 'tusk' : on === 'tail' ? 'stinger' : 'horn';
    return count === 1 ? `a ${size}${shape} ${noun}` : `${size}${shape} ${noun}s`;
  },
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const socket = ctx.socket();
      const length = p.length * ctx.scale;
      const width = Math.min(p.width * ctx.scale, length * 0.45);
      const segments = Math.max(
        8,
        Math.round(Math.abs(p.curve) / 15) + Math.round(Math.abs(p.twist) / 30) + 6,
      );
      const { lean, turn } = p.aim
        ? aimHorn(p.aim, socket, ctx.mirror, p)
        : { lean: p.lean, turn: p.turn };
      const path = ctx.geo.arc(length, p.curve, {
        twist: p.twist,
        lean,
        heading: -turn,
        segments,
      });
      const radius = (t: number) => width * (1 - t) ** 0.85 + width * 0.04;
      const piece = ctx.geo.sweep(path, radius, {
        sides: 12,
        tip: 'point',
        ridges: p.ridges,
        ridgeDepth: 0.14,
      });
      ctx.emit(piece, socket, {
        color: ctx.color(p.color, '#d4c6a2'),
        tipColor: ctx.color(p.tipColor, '#3d3329'),
        sink: width * 0.7,
        path: { points: path, radii: path.map((_, i) => radius(i / (path.length - 1))) },
      });
    },
  },
});
