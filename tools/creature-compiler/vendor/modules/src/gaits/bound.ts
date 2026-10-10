import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'bound',
  summary:
    'Small quadrupeds leaping with both hind legs, then both front legs, like a rabbit or a weasel; the back flexes and stretches with each leap.',
  tags: ['legs', 'fast', 'small'],
  legPairs: [2],
  wave: () => 0.5,
  // Each pair lands almost together, half a cycle after the other.
  offsets: ({ pair, side }) => (pair === 0 ? 0 : 0.5) + (side === 'right' ? 0.04 : 0),
  duty: [0.35, 0.22],
  postures: ['upright'],
  froude: [1.5, 10],
  natural: 3,
  hip: [0, 0.35],
  flex: 0.6,
  params: z.strictObject({
    duty: speedProfile(0.15, 0.6)
      .optional()
      .describe(
        'Share of the cycle each foot is planted: one number, or [slowest, fastest] (default [0.35, 0.22])',
      ),
    flex: z.number().min(0).max(1).default(0.6).describe('How much the spine flexes each bound'),
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
