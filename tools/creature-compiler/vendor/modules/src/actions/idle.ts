import { defineAction } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  breath: z.number().min(0).max(1).default(0.5).describe('Breathing depth'),
  fidget: z.number().min(0).max(1).default(0.5).describe('How often it shifts and looks around'),
});
type Params = z.output<typeof params>;

/** Smooth noise from a few sines with seeded phases: wanders in [-1, 1]. */
function wander(time: number, phases: readonly number[]): number {
  let v = 0;
  for (const [i, phase] of phases.entries()) v += Math.sin(time * (0.23 + i * 0.17) + phase);
  return v / phases.length;
}

const state = new WeakMap<object, { phases: number[]; nextBlink: number }>();

export default defineAction({
  id: 'idle',
  summary: 'Breathing, weight shifts, glances, tail swish and blinks while standing.',
  tags: ['ambient'],
  needs: [],
  params,
  hooks: {
    ambient: true,
    duration: () => 0,
    goals(ctx, out) {
      const p = ctx.params as Params;
      let s = state.get(ctx.rng);
      if (!s) {
        s = { phases: [0, 1, 2, 3, 4, 5].map(() => ctx.rng.float(0, Math.PI * 2)), nextBlink: 1 };
        state.set(ctx.rng, s);
      }
      const time = ctx.elapsed / ctx.timeScale;
      // Breathing never stops; it quickens a little while moving.
      const rate = ctx.speed > 0.01 ? 1.6 : 1;
      out.breath = p.breath * (0.5 + 0.5 * Math.sin((time * rate * Math.PI * 2) / 3.2));
      // Blinks at irregular intervals, each about 0.15 s.
      if (ctx.elapsed > s.nextBlink + 0.15 * ctx.timeScale)
        s.nextBlink = ctx.elapsed + ctx.rng.float(1.5, 6) * ctx.timeScale;
      const since = ctx.elapsed - s.nextBlink;
      out.blink = since >= 0 ? Math.sin(Math.min(1, since / (0.15 * ctx.timeScale)) * Math.PI) : 0;
      // Standing still: look around, shift weight, swish the tail.
      const calm = ctx.busy ? 0 : Math.max(0, 1 - ctx.speed * 4);
      const f = p.fidget * calm;
      const phases = s.phases;
      out.glance = {
        yaw: 0.6 * f * wander(time, phases.slice(0, 3)),
        pitch: 0.15 * f * wander(time * 0.7, phases.slice(3)),
      };
      out.shift = 0.03 * f * wander(time * 0.5, phases.slice(1, 4));
      out.swish = 0.25 * f * Math.sin(time * 0.9 + (phases[0] ?? 0));
    },
  },
});
