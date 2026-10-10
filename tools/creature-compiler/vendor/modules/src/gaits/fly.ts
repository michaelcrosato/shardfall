import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'fly',
  summary: 'Flapping flight: power strokes on the way down, wings folded on the way up.',
  tags: ['air', 'wings'],
  medium: 'air',
  air: 'flapping',
  needs: ['wing'],
  legPairs: 'any',
  wave: () => 0,
  duty: 0,
  froude: [0.5, 8],
  params: z.strictObject({
    stroke: z.number().min(0.2).max(2).default(1).describe('Wingbeat amplitude multiplier'),
    bank: z.number().min(0).max(90).default(40).describe('Most degrees it banks into a turn'),
  }),
});
