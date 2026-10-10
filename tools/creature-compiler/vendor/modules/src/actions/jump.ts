import { defineAction, envelope, ramp } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  power: z
    .number()
    .min(0)
    .max(1)
    .default(0.6)
    .describe('How hard it springs, 0 to 1: how far it leaps unaimed, and at most'),
  crouch: z.number().min(0).max(1).default(0.5).describe('How deep it crouches first, 0 to 1'),
  height: z
    .number()
    .min(0)
    .max(5)
    .default(0)
    .describe(
      'Least height in metres it clears above the ground in the middle of the leap (over an obstacle); 0 for a natural arc',
    ),
});
type Params = z.output<typeof params>;

export default defineAction({
  id: 'jump',
  summary:
    'Crouches, leaps to a target point (or ahead) over the ground and any height asked for, and lands on its legs, absorbing it; fires takeoff and land events.',
  tags: ['move'],
  needs: ['legs'],
  params,
  hooks: {
    // The controller sets the real length once it plans the arc (crouch, flight, recovery).
    duration: () => 1.2,
    leap: (raw) => {
      const p = raw as Params;
      return {
        crouch: 0.3 + 0.25 * p.crouch,
        recover: 0.4,
        angle: (35 * Math.PI) / 180,
        reach: 2 + 3 * p.power,
        most: 3 + 6 * p.power,
        height: p.height,
      };
    },
    goals(ctx, out) {
      const p = ctx.params as Params;
      const t = ctx.t;
      const takeoff = ctx.leap?.takeoff ?? 0.3;
      const land = ctx.leap?.land ?? 0.7;
      // Down into the crouch, spring up through takeoff, absorb the landing and stand.
      const down = (0.1 + 0.3 * p.crouch) * ramp(t, 0, takeoff * 0.85);
      const spring = 1 - ramp(t, takeoff * 0.85, takeoff);
      const absorb = 0.25 * envelope(t, land, land + 0.05, land + 0.1, 1);
      out.crouch = t < takeoff ? down * spring - 0.05 * ramp(t, takeoff * 0.85, takeoff) : absorb;
      // The head looks where it is going, ahead rather than down at its feet.
      if (ctx.target) out.look = ctx.target.clone().setY(Math.max(ctx.target.y, ctx.head.y * 0.8));
      out.lookWeight = 0.6 * envelope(t, 0, 0.1, 0.8, 1);
      out.rear = 0.15 * envelope(t, takeoff * 0.8, takeoff, takeoff + 0.05, land);
      out.stop = true;
    },
  },
});
