import { colorName, colorRef, definePattern } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'countershade',
  summary: 'Paler belly blending into a darker back, as on most animals.',
  tags: ['base', 'camouflage'],
  params: z.strictObject({
    color: colorRef('belly').describe('Belly colour: a palette name or a colour'),
    height: z
      .number()
      .min(-1)
      .max(1)
      .default(-0.1)
      .describe('Where the blend sits: -1 belly, 0 flank, 1 back'),
    softness: z.number().min(0.01).max(1).default(0.35).describe('Width of the blend'),
  }),
  example: { type: 'countershade', strength: 0.6 },
  // Over the whole body it is a belly; on one region (the limbs, the head), their undersides.
  describe: (p, info) =>
    `a ${colorName(p.color as string)} ${!info || info.region === 'all' ? 'belly' : 'underside'}`,
  hooks: {
    shade(k, s, p) {
      const height = k.param(p.height as number);
      const soft = k.param(p.softness as number);
      // Pale below `height`, blending over `softness`.
      return {
        mask: k.sub(k.num(1), k.smoothstep(k.sub(height, soft), k.add(height, soft), s.height)),
      };
    },
  },
});
