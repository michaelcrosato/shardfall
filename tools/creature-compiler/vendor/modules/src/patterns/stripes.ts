import { colorName, colorRef, definePattern, fbm } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'stripes',
  summary: 'Bands running across the spine (or along it), with optional wobble.',
  tags: ['camouflage', 'warning'],
  params: z.strictObject({
    color: colorRef('accent'),
    count: z.number().min(1).max(64).default(10).describe('Stripes from snout to tail tip'),
    width: z
      .number()
      .min(0.05)
      .max(0.95)
      .default(0.35)
      .describe('Stripe width as a share of the spacing'),
    jitter: z.number().min(0).max(1).default(0.3).describe('Wobble and irregularity'),
    direction: z
      .enum(['across', 'along'])
      .default('across')
      .describe('"across" the spine like a tiger, or "along" it'),
    fade: z.number().min(0).max(1).default(0.6).describe('How much stripes fade toward the belly'),
  }),
  example: { type: 'stripes', color: 'accent', count: 12, region: 'back', jitter: 0.4 },
  describe: (p) => `${colorName(p.color as string)} stripes`,
  hooks: {
    shade(k, s, p, seed) {
      const count = k.param(p.count as number);
      const width = p.width as number;
      const jitter = k.param(p.jitter as number);
      const wobble = k.mul(
        k.sub(
          fbm(
            k,
            k.mul(s.x, k.num(3)),
            k.mul(s.y, k.num(3)),
            k.add(k.mul(s.z, k.num(3)), k.num(seed)),
            2,
            seed,
          ),
          k.num(0.5),
        ),
        k.mul(jitter, k.num(1.6)),
      );
      // Across: bands step down the spine (and around the legs). Along: bands follow the spine.
      const u =
        p.direction === 'along'
          ? k.add(k.mul(s.height, k.mul(count, k.num(0.5))), wobble)
          : k.add(k.add(k.mul(s.spine, count), k.mul(s.limb, k.mul(count, k.num(0.35)))), wobble);
      const d = k.abs(k.sub(k.fract(u), k.num(0.5)));
      const half = width * 0.5;
      const band = k.sub(
        k.num(1),
        k.smoothstep(k.num(Math.max(0, half - 0.06)), k.num(half + 0.06), d),
      );
      const fade = k.mix(
        k.num(1),
        k.smoothstep(k.num(-0.7), k.num(0.2), s.height),
        k.param(p.fade as number),
      );
      return { mask: k.mul(band, fade) };
    },
  },
});
