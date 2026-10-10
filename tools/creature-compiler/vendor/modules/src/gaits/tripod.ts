import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'tripod',
  summary: 'Six-legged insect gait: two alternating tripods of feet.',
  tags: ['legs', 'insect', 'fast'],
  legPairs: [3, 4],
  wave: () => 0.5,
  duty: 0.5,
  froude: [0.1, 3],
  params: z.strictObject({
    duty: speedProfile(0.3, 0.8)
      .optional()
      .describe(
        'Share of the cycle each foot is planted: one number, or [slowest, fastest] across its speeds',
      ),
    stepHeight: z
      .number()
      .min(0)
      .max(1)
      .default(0.25)
      .describe('Foot lift as a share of hip height'),
    stride: speedProfile(0.2, 2)
      .default(1)
      .describe('Stride length multiplier: one number, or [slowest, fastest]'),
  }),
});
