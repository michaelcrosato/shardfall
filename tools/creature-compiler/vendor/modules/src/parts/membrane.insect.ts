import { colorRef, definePart } from '@spawnforge/core';
import { z } from 'zod';
import { alongSpar, gridSheet } from './_sheet.ts';

const params = z.strictObject({
  shape: z
    .enum(['narrow', 'broad', 'round'])
    .default('narrow')
    .describe('"narrow" like a dragonfly, "broad" like a moth, "round" like a beetle'),
  width: z.number().min(0.1).max(1.5).default(0.4).describe('Wing width relative to its length'),
  veins: z.number().min(0).max(1).default(0.6).describe('How strongly the veins show'),
  color: colorRef('#d8e0e8').describe('Wing colour: a palette name or a colour'),
  translucency: z.number().min(0).max(1).default(0.7).describe('How much light shows through'),
});
type Params = z.output<typeof params>;

/** Width along the wing (0 root to 1 tip) as a share of its widest, by shape. */
const PROFILE: Record<Params['shape'], (s: number) => number> = {
  // A dragonfly's: widest a third of the way out, narrowing to a rounded tip.
  narrow: (s) => Math.sin(Math.PI * Math.min(1, s * 0.9 + 0.08)) ** 0.7,
  // A moth's: broadening toward the tip, then rounding off.
  broad: (s) => Math.min(1, 0.35 + 0.9 * s) * Math.sqrt(Math.max(0, 1 - s ** 6)),
  // A beetle's hind wing: an oval.
  round: (s) => Math.sqrt(Math.max(0, 1 - (2 * s - 1) ** 2)) * 0.9 + 0.1 * (1 - s),
};

export default definePart({
  id: 'membrane.insect',
  summary: 'A thin, veined insect wing on a hinge, for flies, dragonflies, moths and bees.',
  tags: ['wing', 'membrane', 'insect'],
  slot: 'membrane',
  material: 'chitin',
  attach: { on: 'limb' },
  provides: ['hover', 'glide'],
  params,
  example: { type: 'membrane.insect', shape: 'broad', width: 0.8 },
  describe: (p, { count }) =>
    `${count === 1 ? 'a ' : ''}${p.shape === 'broad' ? 'broad ' : ''}insect wing${count === 1 ? '' : 's'}`,
  hooks: {
    // Straight when spread; folded flat back over the abdomen (docs/design/9.3-wings-fins.md).
    // Broad wings (moths, butterflies) row in a big, near-vertical stroke; narrow and round
    // ones beat flatter and twist more (docs/design/10.4-flight.md).
    wing: (raw) => ({
      bind: [-4, 0, 0],
      fold: { sweep: 96, droop: -6, lie: 90, joints: [0, 0, 0], digits: 0, flex: 0 },
      thickness: 0.004,
      stroke:
        (raw as Params).shape === 'broad'
          ? { amplitude: 65, flex: 0, twist: 25, plane: 20 }
          : { amplitude: 55, flex: 0, twist: 40, plane: 55 },
    }),
    build(ctx, raw) {
      const p = raw as Params;
      const wing = ctx.wing;
      if (!wing) return;
      const length = wing.armLength;
      const widest = p.width * length;
      const profile = PROFILE[p.shape];
      const rows = Math.max(6, Math.round(16 * ctx.detail));
      const cols = Math.max(3, Math.round(6 * ctx.detail));
      const sheet = gridSheet(
        rows,
        cols,
        (s, f) => {
          const { point } = alongSpar(wing.arm, s);
          // The leading edge is the vein along the arm; the wing spreads behind it.
          return point.addScaledVector(wing.lead, -f * widest * profile(s));
        },
        (s) => alongSpar(wing.arm, s).weights,
      );
      ctx.sheet(
        sheet.positions,
        sheet.normals,
        sheet.indices,
        sheet.weights,
        sheet.along,
        sheet.across,
        {
          color: p.color,
          opacity: 1 - 0.8 * p.translucency,
          translucency: p.translucency,
          roughness: 0.3,
          veins: p.veins,
        },
      );
    },
  },
});
