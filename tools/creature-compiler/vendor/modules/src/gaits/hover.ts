import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'hover',
  summary: 'Hovering in place on fast figure-eight wingbeats, like an insect or a hummingbird.',
  tags: ['air', 'wings', 'insect'],
  medium: 'air',
  air: 'hovering',
  needs: ['wing', 'hover'],
  legPairs: 'any',
  wave: () => 0,
  duty: 0,
  froude: [0, 0.5],
  params: z.strictObject({
    rate: z.number().min(0.5).max(2).default(1).describe('Wingbeat rate multiplier'),
  }),
});
