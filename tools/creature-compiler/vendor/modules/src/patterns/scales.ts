import { colorRef, definePattern, detail, relief, shingles } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'scales',
  summary:
    'Overlapping scales in staggered rows down the body: each one raised at its free rear edge, with darker gaps.',
  tags: ['texture', 'reptile'],
  params: z.strictObject({
    size: z.number().min(0.005).max(0.2).default(0.02).describe('Scale size in torso lengths'),
    bump: z.number().min(0).max(1).default(0.4).describe('Depth of the scale relief'),
    gapColor: colorRef('accent').describe(
      'Colour in the gaps between scales: a palette name or a colour',
    ),
    gap: z.number().min(0).max(1).default(0.3).describe('How dark and wide the gaps are'),
  }),
  example: { type: 'scales', size: 0.02, bump: 0.4 },
  describe: () => 'scales',
  hooks: {
    shade(k, s, p, seed) {
      const size = p.size as number;
      const gapParam = p.gap as number;
      const sh = shingles(k, s, size, seed);
      // The low front of each scale, tucked under the one ahead, takes the gap colour: darkest
      // in the crease where two scales meet.
      const low = k.sub(k.num(1), sh.height);
      // Gaps fade out where scales shrink below a few pixels, and the relief sooner, since bump
      // mapping needs more pixels to look smooth.
      return {
        mask: k.mul(k.mul(k.mul(low, low), k.num(0.3 + gapParam * 0.6)), detail(k, s, size)),
        color: p.gapColor as string,
        height: k.mul(k.mul(sh.height, k.num((p.bump as number) * size * 0.3)), relief(k, s, size)),
      };
    },
  },
});
