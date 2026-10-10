import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  reach: z
    .number()
    .min(0)
    .max(1)
    .default(0.7)
    .describe('How far the neck may stretch, as a share of its reach'),
  speed: z.number().min(0.25).max(4).default(1).describe('Speed multiplier'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'bite',
  summary: 'Lunges the head at a target and snaps the jaw shut; fires a bite-contact event.',
  tags: ['attack', 'mouth'],
  needs: ['jaw'],
  params,
  hooks: {
    // Seconds for a creature with 1 m hips: wind up, strike, recover.
    duration: (p) => 0.75 / (p as Params).speed,
    events: () => [{ at: 0.45, type: 'bite-contact' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      // Without a target, bite at a point in front of the head.
      out.look = ctx.target ?? ctx.head.clone().addScaledVector(ctx.forward, 1);
      out.lookWeight = envelope(t, 0, 0.15, 0.75, 1);
      // Pull back and open (anticipation), lunge (strike), snap shut at contact, recover.
      out.reach =
        p.reach *
        (ramp(t, 0.25, 0.45) - 0.25 * envelope(t, 0, 0.2, 0.2, 0.3)) *
        (1 - ramp(t, 0.55, 1));
      out.jaw =
        ramp(t, 0.05, 0.3) * (1 - ramp(t, 0.4, 0.46)) + 0.15 * envelope(t, 0.46, 0.6, 0.7, 1);
      out.crouch = 0.06 * envelope(t, 0, 0.2, 0.3, 0.45);
      // Arms, where it has them, reach out to seize what it bites.
      out.arms = 0.7 * envelope(t, 0.15, 0.4, 0.55, 0.9);
      // Tentacles, where it has them, reach for it too.
      out.grab = 0.8 * envelope(t, 0.15, 0.4, 0.55, 0.9);
    },
  },
});
