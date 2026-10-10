import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'swim.flap',
  summary:
    'Swims by beating long fins or flippers like wings, like a turtle or a penguin; it dives.',
  tags: ['water', 'fins'],
  medium: 'water',
  swim: 'fins',
  needs: ['fin'],
  legPairs: 'any',
  wave: () => 0,
  duty: 0.5,
  froude: [0, 1.2],
  natural: 0.5,
  params: z.strictObject({
    stroke: z.number().min(0.2).max(2).default(1).describe('Stroke size multiplier'),
  }),
});
