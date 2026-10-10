import { defineAction, envelope } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  speed: z.number().min(0.25).max(4).default(1).describe('Speed multiplier'),
  range: z
    .number()
    .min(10)
    .max(180)
    .default(100)
    .describe('Degrees the head may turn from straight ahead'),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'look',
  summary: 'Turns eyes, head and neck toward a target within limits.',
  tags: ['head', 'sense'],
  needs: ['head'],
  params,
  hooks: {
    // A glance: turn, hold, turn back.
    duration: (p) => 2.5 / (p as Params).speed,
    goals(ctx, out) {
      const p = ctx.params as Params;
      const weight = envelope(ctx.t, 0, 0.2, 0.75, 1);
      if (ctx.target) {
        out.look = ctx.target;
        out.lookWeight = weight;
      } else {
        // No target: look over the shoulder, as far as the range allows.
        const yaw = Math.min(p.range, 80) * (Math.PI / 180);
        out.glance = { yaw: yaw * weight, pitch: 0 };
      }
    },
  },
});
