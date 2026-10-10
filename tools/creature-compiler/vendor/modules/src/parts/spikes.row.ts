import { colorRef, definePart } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  count: z
    .number()
    .int()
    .min(1)
    .max(60)
    .default(7)
    .describe('Spikes in the row (in each row, with side "both")'),
  height: z
    .union([z.number().min(0.01).max(0.5), z.array(z.number().min(0.01).max(0.5)).min(1).max(16)])
    .default(0.08)
    .describe('Height in torso lengths, as one number or a profile along the row'),
  width: z.number().min(0.005).max(0.2).default(0.025).describe('Base radius in torso lengths'),
  curve: z
    .number()
    .min(-90)
    .max(90)
    .default(20)
    .describe('Degrees each spike sweeps back toward the tail'),
  jitter: z
    .number()
    .min(0)
    .max(1)
    .default(0)
    .describe('Irregular heights and angles, for jagged rows'),
  color: colorRef('#ddd1b4').describe('Colour at the root: a palette name or a colour'),
  tipColor: colorRef('#5e5040').describe('Colour at the tip'),
});
type Params = z.output<typeof params>;

function profileAt(values: number | readonly number[], t: number): number {
  if (typeof values === 'number') return values;
  if (values.length === 1) return values[0] as number;
  const x = Math.min(1, Math.max(0, t)) * (values.length - 1);
  const i = Math.min(values.length - 2, Math.floor(x));
  const a = values[i] as number;
  const b = values[i + 1] as number;
  const f = x - i;
  return a + (b - a) * (f * f * (3 - 2 * f));
}

export default definePart({
  id: 'spikes.row',
  summary: 'A row of conical spikes between two points, e.g. down the spine at angle 0.',
  tags: ['back', 'tail', 'defence', 'bone'],
  slot: 'row',
  material: 'bone',
  attach: { on: 'torso', angle: 0 },
  params,
  example: {
    id: 'dorsal',
    type: 'spikes.row',
    attach: { on: 'torso', from: 0.05, to: 0.95, angle: 0 },
    params: { count: 9, height: [0.08, 0.15, 0.06] },
  },
  describe: (p) => `a row of ${p.count as number} spikes`,
  hooks: {
    build(ctx, raw) {
      const p = raw as Params;
      const color = ctx.color(p.color, '#ddd1b4');
      const tipColor = ctx.color(p.tipColor, '#5e5040');
      for (let i = 0; i < p.count; i++) {
        const u = p.count === 1 ? 0.5 : (i + 0.5) / p.count;
        const at = ctx.from + (ctx.to - ctx.from) * u;
        const wobble = () => (ctx.rng.next() - 0.5) * p.jitter;
        const height = profileAt(p.height, u) * ctx.scale * (1 + wobble() * 0.8);
        const socket = ctx.socket(at, Math.min(180, Math.max(0, ctx.angle + wobble() * 30)));
        const width = Math.min(p.width * ctx.scale, height * 0.6);
        const path = ctx.geo.arc(height, p.curve * (1 + wobble()), { segments: 5 });
        const piece = ctx.geo.sweep(path, (t) => width * (1 - t), { sides: 8, tip: 'point' });
        ctx.emit(piece, socket, { color, tipColor, sink: width * 0.5 });
      }
    },
  },
});
