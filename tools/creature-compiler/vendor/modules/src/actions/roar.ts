import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  duration: z
    .number()
    .min(0.5)
    .max(6)
    .default(2)
    .describe('Seconds for a creature with 1 m hips; scales with size'),
  intensity: z
    .number()
    .min(0)
    .max(1)
    .default(0.8)
    .describe('How wide the jaw opens and how far the head rises'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'roar',
  summary: 'Raises the head and opens the jaw wide; fires a roar-peak event.',
  tags: ['display', 'mouth'],
  needs: ['jaw'],
  params,
  hooks: {
    duration: (p) => (p as Params).duration,
    events: () => [{ at: 0.45, type: 'roar-peak' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      const hold = envelope(t, 0.2, 0.35, 0.75, 0.95);
      // Gather (head down, crouch), then rear up with the head high and the jaw wide, shaking.
      out.stop = true;
      out.crouch = 0.08 * envelope(t, 0, 0.15, 0.2, 0.35);
      out.raise = p.intensity * (0.75 * hold - 0.2 * envelope(t, 0, 0.12, 0.15, 0.3));
      out.rear = 0.18 * p.intensity * hold;
      out.jaw = p.intensity * ramp(t, 0.18, 0.32) * (1 - ramp(t, 0.78, 0.95));
      out.shake = 0.06 * p.intensity * envelope(t, 0.35, 0.42, 0.65, 0.75);
      out.swish = 0.3 * p.intensity * Math.sin(ctx.elapsed * 9) * hold;
      // Wings, where there are any, flare out with it.
      out.wings = 0.8 * envelope(t, 0.15, 0.35, 0.75, 0.95);
    },
  },
});
