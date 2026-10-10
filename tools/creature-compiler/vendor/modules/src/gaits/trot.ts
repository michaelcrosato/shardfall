import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'trot',
  summary: 'Faster four-legged gait: diagonal feet move together.',
  tags: ['legs', 'fast'],
  legPairs: [2],
  wave: () => 0.5,
  duty: 0.5,
  froude: [0.4, 2.5],
  params: z.strictObject({
    duty: speedProfile(0.3, 0.7)
      .optional()
      .describe(
        'Share of the cycle each foot is planted: one number, or [slowest, fastest] across its speeds',
      ),
    stepHeight: z
      .number()
      .min(0)
      .max(1)
      .default(0.2)
      .describe('Foot lift as a share of hip height'),
    stride: speedProfile(0.2, 2)
      .default(1)
      .describe('Stride length multiplier: one number, or [slowest, fastest]'),
  }),
});
