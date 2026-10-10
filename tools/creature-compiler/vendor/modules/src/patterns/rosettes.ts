import { cells, colorName, colorRef, definePattern, detail, valueNoise } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'rosettes',
  summary: 'Leopard-like rosettes: broken rings of dark marks around a tinted centre.',
  tags: ['camouflage', 'cat'],
  params: z.strictObject({
    color: colorRef('accent').describe('Ring colour: a palette name or a colour'),
    centerColor: colorRef('base').describe('Colour inside each ring'),
    size: z.number().min(0.01).max(0.4).default(0.06).describe('Rosette radius in torso lengths'),
    density: z
      .number()
      .min(0)
      .max(1)
      .default(0.7)
      .describe('Share of possible rosettes that appear'),
    broken: z
      .number()
      .min(0)
      .max(1)
      .default(0.6)
      .describe('How broken each ring is into separate marks'),
  }),
  example: { type: 'rosettes', color: 'accent', size: 0.05, region: 'back' },
  describe: (p) => `${colorName(p.color as string)} rosettes`,
  hooks: {
    shade(k, s, p, seed) {
      const size = p.size as number;
      const f = k.num(1 / (size * 2.4));
      const c = cells(
        k,
        k.mul(s.x, f),
        k.mul(s.y, f),
        k.add(k.mul(s.z, f), k.num(seed)),
        0.75,
        seed,
      );
      const keep = k.step(c.id, k.param(p.density as number));
      // The ring, between about 0.55 and 0.9 of the radius (0.4 cells).
      const ring = k.mul(
        k.smoothstep(k.num(0.2), k.num(0.24), c.distance),
        k.sub(k.num(1), k.smoothstep(k.num(0.34), k.num(0.38), c.distance)),
      );
      // Broken into marks by noise around it, different in each rosette.
      const around = valueNoise(
        k,
        k.add(k.mul(c.dx, k.num(9)), k.mul(c.id, k.num(97))),
        k.mul(c.dy, k.num(9)),
        k.mul(c.dz, k.num(9)),
        seed + 1,
      );
      const cut = (p.broken as number) * 0.6;
      const marks = k.mul(ring, k.smoothstep(k.num(cut - 0.06), k.num(cut + 0.06), around));
      const inside = k.sub(k.num(1), k.smoothstep(k.num(0.2), k.num(0.24), c.distance));
      const seen = detail(k, s, size * 2);
      return {
        mask: k.mul(k.mul(marks, keep), seen),
        under: { mask: k.mul(k.mul(inside, keep), seen), color: p.centerColor as string },
      };
    },
  },
});
