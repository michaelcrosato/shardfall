import { colorRef, definePattern, valueNoise } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'slime',
  summary: 'A wet, glossy coat of slime with drips and a faint tint.',
  tags: ['wet', 'gross'],
  params: z.strictObject({
    color: colorRef('#a8c890').describe('Slime tint: a palette name or a colour'),
    wetness: z
      .number()
      .min(0)
      .max(1)
      .default(0.8)
      .describe('How glossy the skin gets (1 is mirror-wet)'),
    drips: z.number().min(0).max(1).default(0.4).describe('How many drips run down the flanks'),
    tint: z.number().min(0).max(1).default(0.2).describe('How much the slime colours the skin'),
  }),
  example: { type: 'slime', wetness: 0.9, drips: 0.5 },
  describe: () => 'a coat of slime',
  hooks: {
    shade(k, s, p, seed) {
      // Drips: noise stretched eight times taller than wide, where it runs high, low on the
      // flanks and below.
      const f = 30;
      const n = valueNoise(
        k,
        k.mul(s.x, k.num(f)),
        k.mul(s.y, k.num(f / 8)),
        k.add(k.mul(s.z, k.num(f)), k.num(seed)),
        seed,
      );
      const edge = 1 - (p.drips as number) * 0.45;
      const drip = k.mul(
        k.smoothstep(k.num(edge - 0.04), k.num(edge + 0.04), n),
        k.sub(k.num(1), k.smoothstep(k.num(-0.1), k.num(0.7), s.height)),
      );
      const tint = k.param(p.tint as number);
      return {
        mask: k.clamp(k.add(tint, k.mul(drip, k.num(0.5))), k.num(0), k.num(1)),
        coat: k.num(1),
        roughness: k.num(0.5 - 0.46 * (p.wetness as number)),
        height: k.mul(drip, k.num(0.0008)),
      };
    },
  },
});
