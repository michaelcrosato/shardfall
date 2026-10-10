import { colorRef, definePart } from '@spawnforge/core';
import type { Vector3 } from 'three';
import { z } from 'zod';
import { gridSheet } from './_sheet.ts';

const params = z.strictObject({
  height: z.number().min(0.02).max(1).default(0.25).describe('Height in torso lengths'),
  length: z.number().min(0.02).max(1).default(0.25).describe('Length along the body at its base'),
  sweep: z.number().min(0).max(80).default(35).describe('Degrees the fin leans back'),
  shape: z
    .enum(['triangle', 'sail', 'rounded'])
    .default('triangle')
    .describe('"triangle" like a shark, "sail" tall and straight, "rounded" like a carp'),
  color: colorRef('base').describe('Fin colour: a palette name or a colour'),
});
type Params = z.output<typeof params>;

/**
 * The fin's leading and trailing edges at height v (0 base, 1 top), in metres back from its
 * centre along the body, for a base `length` long, `height` tall, leaning back by `lean`
 * (metres at the top).
 */
function edges(shape: Params['shape'], v: number, length: number, lean: number): [number, number] {
  const half = length / 2;
  if (shape === 'sail') {
    // Straight edges, rounding off over the top seventh.
    const top = v > 0.86 ? Math.sqrt(Math.max(0, 1 - ((v - 0.86) / 0.14) ** 2)) : 1;
    const lead = -half + lean * v;
    const trail = half + 0.8 * lean * v;
    const mid = (lead + trail) / 2;
    return [mid + (lead - mid) * top, mid + (trail - mid) * top];
  }
  if (shape === 'rounded') {
    const chord = length * Math.sqrt(Math.max(0, 1 - v * v));
    const centre = 0.5 * lean * v;
    return [centre - chord / 2, centre + chord / 2];
  }
  // A shark's: the leading edge bowed forward, the trailing edge hollowed (falcate).
  const tip = -half + lean + 0.15 * length;
  return [-half + (tip + half) * v ** 1.4, tip + (half - tip) * (1 - v) ** 1.8];
}

export default definePart({
  id: 'fin.dorsal',
  summary: "One fin standing on the midline of the back (or belly, at angle 180), like a shark's.",
  tags: ['fin', 'aquatic', 'back'],
  slot: 'surface',
  material: 'skin',
  attach: { on: 'torso', at: 0.45, angle: 0 },
  params,
  example: {
    id: 'dorsal',
    type: 'fin.dorsal',
    attach: { on: 'torso', at: 0.4, angle: 0 },
    params: { height: 0.3 },
  },
  describe: () => 'a dorsal fin',
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      // Built row by row up from the skin, each vertex standing on the body where it is, so the
      // fin bends with the spine (docs/design/9.3-wings-fins.md).
      const h = p.height * ctx.scale;
      const length = p.length * ctx.scale;
      const lean = h * Math.tan((p.sweep * Math.PI) / 180);
      const rows = Math.max(4, Math.round(10 * ctx.detail));
      const cols = Math.max(4, Math.round(12 * ctx.detail));
      const place = (u: number, v: number) => {
        const [lead, trail] = edges(p.shape, v, length, lean);
        const x = lead + (trail - lead) * u;
        // Further back along the torso is a larger `at`.
        const s = ctx.socket(Math.min(1, Math.max(0, ctx.at + x / ctx.scale)), ctx.angle);
        return {
          position: s.position.clone().addScaledVector(s.normal, v * h - 0.03 * h),
          weights: s.weights.map(([b, w]) => [b, w] as [number, number]),
        };
      };
      const sheet = gridSheet(
        cols,
        rows,
        (u, v) => place(u, v).position,
        (u, v) => place(u, v).weights,
      );
      const color = ctx.color(p.color, '#5d6b74');
      ctx.sheet(
        sheet.positions,
        sheet.normals,
        sheet.indices,
        sheet.weights,
        sheet.across,
        sheet.along.map((u) => u * 2),
        { color, opacity: 1, translucency: 0.3, roughness: 0.5, veins: 0.25 },
      );
      const top = place(0.5, 1).position as Vector3;
      ctx.measure(top.distanceTo(place(0.5, 0).position), 1);
    },
  },
});
