import { colorName, colorRef, definePattern } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'bands',
  summary:
    'Wide, even rings of colour around the body and tail, like a coral snake or a lemur tail.',
  tags: ['warning'],
  params: z.strictObject({
    color: colorRef('accent').describe('Band colour: a palette name or a colour'),
    count: z.number().int().min(1).max(64).default(8).describe('Bands from snout to tail tip'),
    width: z
      .number()
      .min(0.05)
      .max(0.95)
      .default(0.5)
      .describe('Band width as a share of the spacing'),
    sharpness: z.number().min(0).max(1).default(0.8).describe('How crisp the band edges are'),
  }),
  example: { type: 'bands', color: 'accent', count: 12, region: 'tail' },
  describe: (p) => `${colorName(p.color as string)} bands`,
  hooks: {
    shade(k, s, p) {
      // Limbs carry the axis value where they attach, so they take that band.
      const u = k.mul(s.spine, k.param(p.count as number));
      const d = k.abs(k.sub(k.fract(u), k.num(0.5)));
      const half = (p.width as number) * 0.5;
      const soft = (1 - (p.sharpness as number)) * 0.2 + 0.01;
      return { mask: k.sub(k.num(1), k.smoothstep(k.num(half - soft), k.num(half + soft), d)) };
    },
  },
});
