import { cells, colorName, colorRef, definePattern, detail } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'spots',
  summary: 'Scattered round spots, from leopard rosettes to sparse blotches.',
  tags: ['camouflage'],
  params: z.strictObject({
    color: colorRef('accent'),
    size: z.number().min(0.005).max(0.5).default(0.04).describe('Spot radius in torso lengths'),
    density: z.number().min(0).max(1).default(0.75).describe('Share of possible spots that appear'),
    jitter: z.number().min(0).max(1).default(0.8).describe('Irregularity of placement and size'),
    ring: z.number().min(0).max(1).default(0).describe('Hollow the spots into rings (rosettes)'),
  }),
  example: { type: 'spots', color: 'accent', size: 0.04, density: 0.6 },
  describe: (p) =>
    (p.ring as number) > 0.4
      ? `${colorName(p.color as string)} rosettes`
      : `${colorName(p.color as string)} spots`,
  hooks: {
    shade(k, s, p, seed) {
      const f = k.num(1 / ((p.size as number) * 2.2));
      const c = cells(
        k,
        k.mul(s.x, f),
        k.mul(s.y, f),
        k.add(k.mul(s.z, f), k.num(seed)),
        p.jitter as number,
        seed,
      );
      const r = k.num(0.36);
      const spot = k.sub(k.num(1), k.smoothstep(k.sub(r, k.num(0.06)), r, c.distance));
      const ring = p.ring as number;
      const hollow =
        ring > 0
          ? k.sub(
              k.num(1),
              k.smoothstep(k.num(0.36 * ring * 0.7 - 0.05), k.num(0.36 * ring * 0.7), c.distance),
            )
          : k.num(0);
      const keep = k.step(c.id, k.param(p.density as number));
      return {
        mask: k.mul(
          k.mul(k.max(k.sub(spot, hollow), k.num(0)), keep),
          detail(k, s, (p.size as number) * 2),
        ),
      };
    },
  },
});
