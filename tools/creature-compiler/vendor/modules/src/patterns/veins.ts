import { colorName, colorRef, definePattern, detail, fbm, relief } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'veins',
  summary: 'Branching veins under thin skin, darker or glowing.',
  tags: ['detail', 'gross'],
  params: z.strictObject({
    color: colorRef('#5a1a2a').describe('Vein colour: a palette name or a colour'),
    density: z.number().min(0).max(1).default(0.5).describe('How many branches'),
    width: z.number().min(0.001).max(0.05).default(0.01).describe('Vein width in torso lengths'),
    raised: z.number().min(0).max(1).default(0.3).describe('How far the veins stand out'),
  }),
  example: { type: 'veins', color: '#6a1030', density: 0.6, region: 'head' },
  describe: (p) => `${colorName(p.color as string)} veins`,
  hooks: {
    shade(k, s, p, seed) {
      const width = p.width as number;
      // Ridges of fractal noise, warped so the lines wander and branch.
      const freq = 6 + (p.density as number) * 10;
      const at = (v: (typeof s)['x'], by: number) => k.mul(v, k.num(by));
      const warp = (salt: number) =>
        k.mul(
          k.sub(
            fbm(k, at(s.x, freq * 0.5), at(s.y, freq * 0.5), at(s.z, freq * 0.5), 2, salt),
            k.num(0.5),
          ),
          k.num(1.2),
        );
      const n = fbm(
        k,
        k.add(at(s.x, freq), warp(seed + 3)),
        k.add(at(s.y, freq), warp(seed + 5)),
        k.add(k.add(at(s.z, freq), warp(seed + 7)), k.num(seed)),
        2,
        seed,
      );
      // Lines where the noise crosses its middle; fbm stays near it, so the band is narrow.
      const t = Math.min(0.08, width * freq * 0.11);
      const line = k.sub(
        k.num(1),
        k.smoothstep(k.num(t * 0.4), k.num(t), k.abs(k.sub(n, k.num(0.5)))),
      );
      return {
        mask: k.mul(line, detail(k, s, width * 5)),
        height: k.mul(
          k.mul(line, k.num(width * (p.raised as number) * 0.5)),
          relief(k, s, width * 4),
        ),
      };
    },
  },
});
