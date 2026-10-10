import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  speed: z.number().min(0.25).max(3).default(1).describe('Speed multiplier'),
  arc: z.number().min(20).max(180).default(90).describe('Degrees the strike sweeps'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'lash',
  summary: 'Whips the tail or a tentacle at a target; fires a lash-contact event.',
  tags: ['attack'],
  needs: [['tail', 'tentacle']],
  params,
  hooks: {
    // Seconds for a creature with 1 m hips: wind up, strike, recover
    // (docs/design/9.4-tentacles-parts.md).
    duration: (p) => 1.1 / (p as Params).speed,
    events: () => [{ at: 0.5, type: 'lash-contact' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      // Without a target, lash at a point ahead and to the side.
      out.look = ctx.target ?? ctx.head.clone().addScaledVector(ctx.forward, 0.6);
      out.lookWeight = 0.5 * envelope(t, 0, 0.2, 0.7, 1);
      out.lashArc = (p.arc * Math.PI) / 180;
      // Wind up away, strike through to the target at contact, hold, then let it swing back.
      out.lash =
        -0.6 * envelope(t, 0, 0.2, 0.3, 0.38) + ramp(t, 0.32, 0.5) * (1 - ramp(t, 0.62, 0.95));
      out.crouch = 0.05 * envelope(t, 0, 0.25, 0.4, 0.6);
    },
  },
});
