import { cells, colorName, colorRef, definePattern, detail, relief } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'warts',
  summary: 'Raised bumps and warts, like a toad or a troll.',
  tags: ['detail', 'gross'],
  params: z.strictObject({
    color: colorRef('accent').describe('Wart colour: a palette name or a colour'),
    size: z.number().min(0.005).max(0.2).default(0.025).describe('Wart radius in torso lengths'),
    density: z.number().min(0).max(1).default(0.5).describe('Share of possible warts that appear'),
    height: z.number().min(0).max(1).default(0.6).describe('How far the warts stand out'),
  }),
  example: { type: 'warts', color: 'accent', size: 0.02, density: 0.6, region: 'back' },
  describe: (p) => `${colorName(p.color as string)} warts`,
  hooks: {
    shade(k, s, p, seed) {
      const size = p.size as number;
      const f = k.num(1 / (size * 2.5));
      const c = cells(
        k,
        k.mul(s.x, f),
        k.mul(s.y, f),
        k.add(k.mul(s.z, f), k.num(seed)),
        0.8,
        seed,
      );
      const keep = k.step(c.id, k.param(p.density as number));
      // Radius 0.24 to 0.4 of a cell, varying per wart; a dome inside it.
      const radius = k.add(k.num(0.24), k.mul(k.fract(k.mul(c.id, k.num(7.31))), k.num(0.16)));
      const t = k.min(k.num(1), k.div(c.distance, radius));
      // (1 - t²)² meets the skin with no slope, so the bump has no edge.
      const cap = k.sub(k.num(1), k.mul(t, t));
      const dome = k.mul(k.mul(cap, cap), keep);
      // Mostly relief: the colour shows toward each wart's tip.
      return {
        mask: k.mul(
          k.mul(k.smoothstep(k.num(0.45), k.num(1), dome), k.num(0.55)),
          detail(k, s, size * 2.5),
        ),
        roughness: k.num(0.85),
        height: k.mul(
          k.mul(dome, k.num(size * (p.height as number) * 1.4)),
          relief(k, s, size * 3),
        ),
      };
    },
  },
});
