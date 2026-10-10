import { defineAction, envelope } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  duration: z
    .number()
    .min(0.5)
    .max(8)
    .default(2.5)
    .describe('Seconds for a creature with 1 m hips; scales with size'),
  intensity: z.number().min(0).max(1).default(1).describe('How far everything opens'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'display',
  summary:
    'Threat display: opens frills and hoods, raises quills and sails. Needs a part that provides `display`.',
  tags: ['display'],
  needs: ['display'],
  params,
  hooks: {
    duration: (p) => (p as Params).duration,
    events: () => [{ at: 0.35, type: 'display-peak' }],
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      // Face the threat, rear up a little and hiss, open everything, hold, and fold back
      // (docs/design/9.5-coverings.md).
      out.look = ctx.target ?? ctx.head.clone().addScaledVector(ctx.forward, 1);
      out.lookWeight = envelope(t, 0, 0.1, 0.85, 1);
      const open = envelope(t, 0.12, 0.3, 0.8, 0.95);
      out.flare = p.intensity * open;
      out.wings = p.intensity * open;
      out.rear = 0.15 * p.intensity * open;
      out.raise = 0.15 * p.intensity * open;
      out.jaw = 0.3 * p.intensity * envelope(t, 0.2, 0.3, 0.55, 0.7);
      out.crouch = 0.05 * envelope(t, 0, 0.1, 0.12, 0.25);
      out.stop = true;
    },
  },
});
