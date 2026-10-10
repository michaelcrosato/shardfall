import { colorRef, definePart, type PartBuildContext } from '@spawnforge/core';
import { z } from 'zod';
import { buildFan, type FanSpine, fanChains } from './_fan.ts';

const params = z.strictObject({
  radius: z
    .number()
    .min(0.05)
    .max(1)
    .default(0.3)
    .describe('Frill radius when open, in torso lengths'),
  spines: z.number().int().min(4).max(30).default(12).describe('Spines in the fan'),
  open: z.number().min(0).max(1).default(0.2).describe('How open it is at rest'),
  color: colorRef('accent').describe('Membrane colour: a palette name or a colour'),
  spineColor: colorRef('base').describe('Spine colour'),
});
type Params = z.output<typeof params>;

/**
 * Spines ringing the neck at `at`, round the top and sides but not the throat, opening into a
 * dish that faces a little forward; at rest they lie back over the neck and shoulders.
 */
function spinesOf(ctx: PartBuildContext, p: Params): FanSpine[] {
  const length = p.radius * ctx.scale;
  const out: FanSpine[] = [];
  for (let k = 0; k < p.spines; k++) {
    const angle = -150 + (300 * k) / (p.spines - 1);
    const socket = ctx.surface(ctx.at, angle);
    const radial = socket.normal
      .clone()
      .addScaledVector(socket.forward, -socket.normal.dot(socket.forward))
      .normalize();
    out.push({
      socket,
      open: radial.addScaledVector(socket.forward, 0.3).normalize(),
      fold: socket.forward.clone().negate(),
      folded: 1 - p.open,
      // Shorter toward the throat.
      length: length * (0.65 + 0.35 * Math.cos((angle * Math.PI) / 360)),
      radius: Math.max(0.002 * ctx.scale, 0.03 * length),
    });
  }
  return out;
}

export default definePart({
  id: 'frill',
  summary:
    'A fan of spines with skin between them around the neck, folded at rest and opened in display.',
  tags: ['neck', 'display', 'reptile'],
  slot: 'surface',
  material: 'skin',
  attach: { on: 'neck', at: 0.1, angle: 0 },
  provides: ['display'],
  params,
  example: { id: 'frill', type: 'frill', params: { radius: 0.35, spines: 14 } },
  describe: () => 'a frill',
  hooks: {
    // Each spine on a flare-driven bone (docs/design/9.5-coverings.md).
    bones: (ctx, raw) => fanChains(spinesOf(ctx, raw as Params)),
    build(ctx, raw) {
      const p = raw as Params;
      const spines = spinesOf(ctx, p);
      buildFan(
        ctx,
        spines,
        spines.slice(1).map((_, k) => [k, k + 1] as const),
        {
          color: ctx.color(p.color, '#c8502a'),
          spineColor: ctx.color(p.spineColor, '#6a5a3a'),
          scallop: 0.18,
          translucency: 0.55,
        },
      );
      ctx.measure(p.radius * ctx.scale, p.spines);
    },
  },
});
