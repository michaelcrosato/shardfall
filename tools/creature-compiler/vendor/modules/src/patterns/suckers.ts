import { colorRef, definePattern, detail, relief } from '@spawnforge/core';
import { z } from 'zod';

export default definePattern({
  id: 'suckers',
  summary:
    'A row of pale suckers down the underside of tentacles (and any limb), each a raised rim round a dark cup.',
  tags: ['texture', 'sea'],
  params: z.strictObject({
    color: colorRef('belly').describe('Rim colour: a palette name or a colour'),
    centerColor: colorRef('#6a3a44').describe('Colour in the cup: a palette name or a colour'),
    count: z.number().int().min(4).max(64).default(22).describe('Suckers along each limb'),
    size: z
      .number()
      .min(0.2)
      .max(1)
      .default(0.8)
      .describe('Sucker length as a share of the spacing'),
    width: z
      .number()
      .min(0.2)
      .max(1.2)
      .default(0.7)
      .describe('How far round the underside they reach, in radians either side'),
    bump: z.number().min(0).max(1).default(0.5).describe('How raised the rims are'),
  }),
  example: { type: 'suckers', count: 24, region: 'limbs' },
  describe: () => 'suckers',
  hooks: {
    shade(k, s, p) {
      // On limbs only (not wing tubes, whose `limb` is 2 and over), clear of the root.
      const onLimb = k.mul(
        k.mul(s.limbs, k.step(s.limb, k.num(1.5))),
        k.smoothstep(k.num(0.04), k.num(0.1), s.limb),
      );
      // Along: one cell per sucker. Across: the angle from the underside's midline, from
      // `height` (-1 there) as on a round section, cos θ = -height.
      const along = k.mul(
        k.sub(k.fract(k.mul(s.limb, k.param(p.count as number))), k.num(0.5)),
        k.num(2 / (p.size as number)),
      );
      const angle = k.sqrt(k.max(k.mul(k.add(s.height, k.num(1)), k.num(2)), k.num(0)));
      const across = k.div(angle, k.param(p.width as number));
      const d = k.sqrt(k.add(k.mul(along, along), k.mul(across, across)));
      const cup = k.mul(k.sub(k.num(1), k.smoothstep(k.num(0.82), k.num(0.98), d)), onLimb);
      const rim = k.mul(cup, k.smoothstep(k.num(0.4), k.num(0.58), d));
      // Suckers on a tentacle are a few hundredths of a torso length across.
      const seen = detail(k, s, 0.04);
      return {
        mask: k.mul(rim, seen),
        under: { mask: k.mul(cup, seen), color: p.centerColor as string },
        height: k.mul(k.mul(rim, k.num((p.bump as number) * 0.006)), relief(k, s, 0.04)),
        roughness: k.num(0.35),
      };
    },
  },
});
