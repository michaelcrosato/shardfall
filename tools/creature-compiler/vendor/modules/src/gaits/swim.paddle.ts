import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'swim.paddle',
  summary:
    'Swims at the surface by paddling its legs in circles under the hips, like a dog, a bear or a crocodile going slowly.',
  tags: ['water', 'legs'],
  medium: 'water',
  swim: 'legs',
  needs: ['legs'],
  legPairs: 'any',
  wave: () => 0.5,
  duty: 0.5,
  froude: [0, 0.4],
  natural: 0.2,
  params: z.strictObject({
    stroke: z.number().min(0.2).max(2).default(1).describe('Stroke length multiplier'),
  }),
});
