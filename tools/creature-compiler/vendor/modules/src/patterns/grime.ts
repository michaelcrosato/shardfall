import { colorRef, definePattern, fbm } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'grime',
  summary: 'Dirt gathered in creases, on the feet and low on the body; adds roughness.',
  tags: ['weathering'],
  params: z.strictObject({
    color: colorRef('#5c4c3a'),
    amount: z.number().min(0).max(1).default(0.5).describe('Overall strength'),
    creases: z.number().min(0).max(1).default(0.7).describe('Dirt in creases and joints'),
    feet: z.number().min(0).max(1).default(0.6).describe('Dirt rising from the ground up the legs'),
  }),
  example: { type: 'grime', amount: 0.4 },
  describe: () => 'grime',
  hooks: {
    shade(k, s, p, seed) {
      const dirt = fbm(
        k,
        k.mul(s.x, k.num(5)),
        k.mul(s.y, k.num(5)),
        k.add(k.mul(s.z, k.num(5)), k.num(seed)),
        2,
        seed,
      );
      const creases = k.mul(s.crease, k.num(p.creases as number));
      const feet = k.mul(
        k.sub(k.num(1), k.smoothstep(k.num(0), k.num(0.35), s.ground)),
        k.num(p.feet as number),
      );
      const low = k.mul(
        k.sub(k.num(1), k.smoothstep(k.num(-1), k.num(-0.2), s.height)),
        k.num(0.35),
      );
      const amount = k.add(k.add(creases, feet), low);
      const mask = k.mul(
        k.clamp(k.mul(amount, k.add(k.num(0.6), k.mul(dirt, k.num(0.8)))), k.num(0), k.num(1)),
        k.param(p.amount as number),
      );
      return { mask, roughness: k.num(0.92) };
    },
  },
});
