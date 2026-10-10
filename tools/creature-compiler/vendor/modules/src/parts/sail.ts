import { colorRef, definePart, type PartBuildContext } from '@spawnforge/core';
import { z } from 'zod';
import { buildFan, type FanSpine, fanChains } from './_fan.ts';

const params = z.strictObject({
  height: z
    .union([z.number().min(0.02).max(1), z.array(z.number().min(0.02).max(1)).min(1).max(16)])
    .default([0.1, 0.4, 0.1])
    .describe('Height in torso lengths, as one number or a profile along the sail'),
  spines: z.number().int().min(3).max(40).default(12).describe('Spines holding the sail'),
  color: colorRef('accent').describe('Membrane colour: a palette name or a colour'),
  spineColor: colorRef('base').describe('Spine colour'),
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

/** Spines standing along the row, leaning back a little at rest. */
function spinesOf(ctx: PartBuildContext, p: Params): FanSpine[] {
  const out: FanSpine[] = [];
  for (let k = 0; k < p.spines; k++) {
    const u = p.spines === 1 ? 0.5 : k / (p.spines - 1);
    const socket = ctx.socket(ctx.from + (ctx.to - ctx.from) * u, ctx.angle);
    const length = profileAt(p.height, u) * ctx.scale;
    out.push({
      socket,
      open: socket.normal
        .clone()
        .addScaledVector(socket.forward, -socket.normal.dot(socket.forward))
        .normalize(),
      fold: socket.forward.clone().negate(),
      folded: 1 / 6,
      length,
      radius: Math.max(0.003 * ctx.scale, 0.012 * ctx.scale + 0.02 * length),
    });
  }
  return out;
}

export default definePart({
  id: 'sail',
  summary: 'A tall sail of skin stretched over long spines along the back, like a dimetrodon.',
  tags: ['back', 'display', 'reptile'],
  slot: 'row',
  material: 'skin',
  attach: { on: 'spine', from: 0.25, to: 0.6, angle: 0 },
  provides: ['display'],
  params,
  example: {
    id: 'sail',
    type: 'sail',
    attach: { on: 'spine', from: 0.25, to: 0.6 },
    params: { height: [0.1, 0.45, 0.1] },
  },
  describe: () => 'a sail',
  hooks: {
    // Each spine on a flare-driven bone, standing up straight in display
    // (docs/design/9.5-coverings.md).
    bones: (ctx, raw) => fanChains(spinesOf(ctx, raw as Params)),
    build(ctx, raw) {
      const p = raw as Params;
      const spines = spinesOf(ctx, p);
      buildFan(
        ctx,
        spines,
        spines.slice(1).map((_, k) => [k, k + 1] as const),
        {
          color: ctx.color(p.color, '#a04a2a'),
          spineColor: ctx.color(p.spineColor, '#5a4a3a'),
          scallop: 0.1,
          translucency: 0.6,
        },
      );
      ctx.measure(Math.max(...spines.map((s) => s.length)), p.spines);
    },
  },
});
