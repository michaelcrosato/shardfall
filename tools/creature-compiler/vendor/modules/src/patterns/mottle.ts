import { colorName, colorRef, definePattern, fbm } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'mottle',
  summary: 'Soft, cloudy blotches of a second colour that break up the outline.',
  tags: ['camouflage', 'texture'],
  params: z.strictObject({
    color: colorRef('accent'),
    scale: z.number().min(0.01).max(1).default(0.15).describe('Blotch size in torso lengths'),
    contrast: z.number().min(0).max(1).default(0.5).describe('How sharp the blotch edges are'),
    coverage: z.number().min(0).max(1).default(0.5).describe('Share of the skin covered'),
  }),
  example: { type: 'mottle', color: 'accent', scale: 0.2, coverage: 0.4 },
  describe: (p) => `${colorName(p.color as string)} mottling`,
  hooks: {
    shade(k, s, p, seed) {
      const f = k.num(1 / (p.scale as number));
      const n = fbm(k, k.mul(s.x, f), k.mul(s.y, f), k.add(k.mul(s.z, f), k.num(seed)), 3, seed);
      const edge = 1 - (p.coverage as number);
      const soft = 0.02 + (1 - (p.contrast as number)) * 0.18;
      return {
        mask: k.smoothstep(k.num(edge * 0.6 + 0.2 - soft), k.num(edge * 0.6 + 0.2 + soft), n),
      };
    },
  },
});
