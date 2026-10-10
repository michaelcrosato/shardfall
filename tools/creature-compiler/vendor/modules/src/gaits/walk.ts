import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'walk',
  summary: 'Slow gait for any leg count: feet lift one after another, back to front.',
  tags: ['legs', 'slow'],
  legPairs: 'any',
  wave: (pairs) => (pairs <= 1 ? 0.5 : pairs === 2 ? 0.25 : 1 / pairs),
  // Two legs walk with a longer stance share than four or six.
  duty: (pairs) => (pairs <= 1 ? 0.62 : 0.75),
  froude: [0, 0.5],
  params: z.strictObject({
    duty: speedProfile(0.4, 0.95)
      .optional()
      .describe(
        'Share of the cycle each foot is planted; defaults by leg count: one number, or [slowest, fastest] across its speeds',
      ),
    stepHeight: z
      .number()
      .min(0)
      .max(1)
      .default(0.15)
      .describe('Foot lift as a share of hip height'),
    stride: speedProfile(0.2, 2)
      .default(1)
      .describe('Stride length multiplier: one number, or [slowest, fastest]'),
  }),
});
