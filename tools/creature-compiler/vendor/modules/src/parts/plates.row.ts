import { colorRef, definePart } from '@spawnforge/core';
import { Vector3 } from 'three';
import { z } from 'zod';

const params = z.strictObject({
  count: z
    .number()
    .int()
    .min(2)
    .max(40)
    .default(12)
    .describe('Plates in all; with alternate they take turns left and right'),
  height: z
    .union([z.number().min(0.01).max(0.8), z.array(z.number().min(0.01).max(0.8)).min(1).max(16)])
    .default([0.08, 0.2, 0.08])
    .describe('Height in torso lengths, as one number or a profile along the row'),
  alternate: z.boolean().default(true).describe('Two staggered rows, left and right'),
  shape: z.enum(['kite', 'round', 'spike']).default('kite').describe('Plate outline'),
  color: colorRef('#c8b090').describe('Plate colour: a palette name or a colour'),
  edgeColor: colorRef('accent').describe('Colour at the plate edges'),
});
type Params = z.output<typeof params>;

function profileAt(values: number | readonly number[], t: number): number {
  if (typeof values === 'number') return values;
  if (values.length === 1) return values[0] as number;
  const x = Math.min(1, Math.max(0, t)) * (values.length - 1);
  const i = Math.min(values.length - 2, Math.floor(x));
  const f = x - i;
  return (
    (values[i] as number) +
    ((values[i + 1] as number) - (values[i] as number)) * (f * f * (3 - 2 * f))
  );
}

/** Rounds a closed outline's corners (Chaikin), keeping the corners listed in `sharp`. */
function soften(points: [number, number][], sharp: ReadonlySet<number>, passes = 2) {
  let pts = points.map((p, i) => ({ p, sharp: sharp.has(i) }));
  for (let k = 0; k < passes; k++) {
    const next: typeof pts = [];
    pts.forEach((a, i) => {
      const b = pts[(i + 1) % pts.length] as (typeof pts)[number];
      if (a.sharp) next.push(a);
      else
        next.push({
          p: [0.75 * a.p[0] + 0.25 * b.p[0], 0.75 * a.p[1] + 0.25 * b.p[1]],
          sharp: false,
        });
      if (!b.sharp)
        next.push({
          p: [0.25 * a.p[0] + 0.75 * b.p[0], 0.25 * a.p[1] + 0.75 * b.p[1]],
          sharp: false,
        });
    });
    pts = next;
  }
  return pts.map((q) => q.p);
}

/**
 * A plate's outline in its own plane, x along the body (forward) and y up from the skin, a
 * fifth of it sunk below; counter-clockwise. A kite is a broad diamond with a point leaning
 * back, a round plate a broad oval, a spike a tall narrow blade.
 */
function outline(shape: Params['shape'], h: number): [number, number][] {
  const sunk = -0.2 * h;
  if (shape === 'spike')
    return [
      [-0.22 * h, sunk],
      [0.18 * h, sunk],
      [0.02 * h, 0.55 * h],
      [-0.1 * h, h],
      [-0.12 * h, 0.5 * h],
    ];
  if (shape === 'kite')
    return soften(
      [
        [-0.14 * h, sunk],
        [0.14 * h, sunk],
        [0.34 * h, 0.42 * h],
        [-0.04 * h, h],
        [-0.34 * h, 0.4 * h],
      ],
      new Set([3]),
    );
  const out: [number, number][] = [];
  const n = 18;
  for (let i = 0; i <= n; i++) {
    const a = Math.PI * (i / n);
    out.push([Math.cos(a) * 0.45 * h, Math.max(sunk, Math.sin(a) * h)]);
  }
  out.push([-0.3 * h, sunk], [0.3 * h, sunk]);
  return out;
}

export default definePart({
  id: 'plates.row',
  summary:
    'Upright bony plates along the back, in one row or two alternating rows like a stegosaur.',
  tags: ['back', 'armor', 'bone'],
  slot: 'row',
  material: 'bone',
  attach: { on: 'spine', from: 0.2, to: 0.8, angle: 0 },
  params,
  example: {
    id: 'plates',
    type: 'plates.row',
    attach: { on: 'spine', from: 0.2, to: 0.85 },
    params: { count: 14, height: [0.1, 0.25, 0.1] },
  },
  describe: (p) => ((p.shape as string) === 'spike' ? 'spiked plates' : 'plates'),
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.color, '#c8b090');
      const edgeColor = ctx.color(p.edgeColor, '#5a4a30');
      let tallest = 0;
      for (let i = 0; i < p.count; i++) {
        const u = p.count === 1 ? 0.5 : (i + 0.5) / p.count;
        const at = ctx.from + (ctx.to - ctx.from) * u;
        const h = profileAt(p.height, u) * ctx.scale;
        tallest = Math.max(tallest, h);
        // Alternating plates stand a little either side of the midline and lean out.
        const side = p.alternate ? (i % 2 === 0 ? 1 : -1) : 0;
        const socket = ctx.surface(at, ctx.angle + side * 14);
        const thickness = Math.max(0.004 * ctx.scale, 0.06 * h);
        let piece = ctx.geo.plate(outline(p.shape, h), thickness);
        // The outline's x runs along the body (+Z in socket space); its thin side faces out.
        piece = ctx.geo.rotate(piece, new Vector3(0, 1, 0), -90);
        if (side !== 0) piece = ctx.geo.rotate(piece, new Vector3(0, 0, 1), -side * 10);
        ctx.emit(piece, socket, { color, tipColor: edgeColor, sink: 0 });
      }
      ctx.measure(tallest, p.count);
    },
  },
});
