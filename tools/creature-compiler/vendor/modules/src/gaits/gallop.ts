import { defineGait, speedProfile } from '@spawnforge/core';
import { z } from 'zod';

const params = z.strictObject({
  style: z
    .enum(['transverse', 'rotary'])
    .default('transverse')
    .describe(
      '"transverse" like a horse (the forefeet land in the same order as the hind), "rotary" like a cheetah or a dog (in the opposite order)',
    ),
  lead: z.enum(['left', 'right']).default('right').describe('Which foreleg lands last, leading'),
  duty: speedProfile(0.15, 0.6)
    .optional()
    .describe(
      'Share of the cycle each foot is planted: one number, or [slowest, fastest] (default [0.4, 0.22])',
    ),
  flex: z.number().min(0).max(1).default(0.4).describe('How much the spine flexes each stride'),
  stepHeight: z.number().min(0).max(1).default(0.22).describe('Foot lift as a share of hip height'),
  stride: speedProfile(0.2, 2)
    .default(1)
    .describe('Stride length multiplier: one number, or [slowest, fastest]'),
});
type Params = z.output<typeof params>;

export default defineGait({
  id: 'gallop',
  summary:
    'Fast four-legged gait with a leading foreleg and a moment in the air, like a horse (transverse) or a cheetah (rotary); the back flexes each stride.',
  tags: ['legs', 'fast'],
  legPairs: [2],
  wave: () => 0.45,
  // The hind feet land 0.1 of a cycle apart, then the forefeet, the lead foreleg last. In a
  // transverse gallop the hind foot on the lead's side lands second (a horse leading right:
  // left hind, right hind, left fore, right fore); in a rotary one the other does, so the
  // footfalls go round the body (right hind, left hind, left fore, right fore).
  offsets: ({ pair, side }, raw) => {
    const p = raw as Params;
    const lead = side === p.lead;
    if (pair === 0) return (p.style === 'rotary' ? !lead : lead) ? 0.1 : 0;
    return lead ? 0.55 : 0.45;
  },
  duty: [0.4, 0.22],
  postures: ['upright'],
  froude: [2, 12],
  natural: 4,
  hip: [0.3, 5],
  flex: 0.4,
  params,
});
