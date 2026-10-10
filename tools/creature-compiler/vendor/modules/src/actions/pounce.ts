import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  power: z
    .number()
    .min(0)
    .max(1)
    .default(0.7)
    .describe('How hard it springs, 0 to 1: how far it can pounce'),
  reach: z
    .number()
    .min(0)
    .max(1)
    .default(0.8)
    .describe('How far the head lunges at the end, as a share of what the neck can reach'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'pounce',
  summary:
    'A low crouch, a leap at the target and a bite as it lands, the head meeting the target; fires takeoff, land and bite-contact events.',
  tags: ['attack', 'move'],
  needs: ['legs', 'jaw'],
  params,
  hooks: {
    duration: () => 1.4,
    leap: (raw) => {
      const p = raw as Params;
      return {
        crouch: 0.5,
        recover: 0.5,
        angle: (25 * Math.PI) / 180,
        reach: 2.5 + 2.5 * p.power,
        most: 3 + 5 * p.power,
        head: true,
      };
    },
    // The jaws close as the feet come down.
    events: (_, leap) => [{ at: leap?.land ?? 0.6, type: 'bite-contact' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      const takeoff = ctx.leap?.takeoff ?? 0.35;
      const land = ctx.leap?.land ?? 0.6;
      // A stalker's low crouch, the spring, then the landing taken low.
      out.crouch =
        t < takeoff
          ? 0.35 * ramp(t, 0, takeoff * 0.8) * (1 - ramp(t, takeoff * 0.85, takeoff))
          : 0.2 * envelope(t, land, land + 0.04, land + 0.1, 1);
      out.look = ctx.target ?? ctx.head.clone().addScaledVector(ctx.forward, 1);
      out.lookWeight = envelope(t, 0, 0.1, 0.85, 1);
      // Jaws open in the air, snap shut at landing; the head lunges into the bite.
      out.jaw = ramp(t, takeoff, (takeoff + land) / 2) * (1 - ramp(t, land - 0.02, land + 0.02));
      out.reach = p.reach * envelope(t, (takeoff + land) / 2, land, land + 0.05, 1);
      out.arms = 0.8 * envelope(t, takeoff, land, land + 0.05, 1);
      out.stop = true;
    },
  },
});
