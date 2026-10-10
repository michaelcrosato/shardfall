import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  speed: z.number().min(0.25).max(3).default(1).describe('Speed multiplier'),
  both: z.boolean().default(false).describe('Snap both pincers together'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'pinch',
  summary: 'Snaps a pincer shut on a target; fires a pinch-contact event.',
  tags: ['attack'],
  needs: ['pincer'],
  params,
  hooks: {
    // Seconds for a creature with 1 m hips: raise and open, snap, hold, let go
    // (docs/design/9.4-tentacles-parts.md).
    duration: (p) => 0.9 / (p as Params).speed,
    events: () => [{ at: 0.5, type: 'pinch-contact' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      out.look = ctx.target ?? ctx.head.clone().addScaledVector(ctx.forward, 0.5);
      out.lookWeight = envelope(t, 0, 0.2, 0.8, 1);
      // The claw rises toward the target (the nearer one unless `both`), opens wide, then
      // snaps shut at contact and holds a moment before letting go.
      out.nearest = !p.both;
      out.arms = 0.75 * envelope(t, 0.05, 0.35, 0.7, 1);
      out.grip =
        -0.8 * envelope(t, 0.1, 0.28, 0.36, 0.4) + ramp(t, 0.38, 0.45) * (1 - ramp(t, 0.75, 0.95));
      out.crouch = 0.04 * envelope(t, 0, 0.25, 0.45, 0.7);
    },
  },
});
