import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'run',
  summary:
    'Two-legged running with a moment in the air each stride, for raptors and runners: feet are down less as it speeds up, and the body leans in.',
  tags: ['legs', 'fast'],
  legPairs: [1],
  wave: () => 0.5,
  // Feet down 45% of the cycle when it starts to run, 30% flat out: each stride has a flight.
  duty: [0.45, 0.3],
  postures: ['upright'],
  froude: [0.5, 8],
  natural: 2,
  params: z.strictObject({
    duty: speedProfile(0.2, 0.6)
      .optional()
      .describe(
        'Share of the cycle each foot is planted: one number, or [slowest, fastest] (default [0.45, 0.3])',
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
    lean: z.number().min(0).max(45).default(15).describe('Degrees the body leans forward at speed'),
  }),
});
