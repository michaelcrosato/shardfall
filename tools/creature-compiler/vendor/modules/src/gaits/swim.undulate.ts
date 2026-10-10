import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'swim.undulate',
  summary:
    'Swims with a body wave that grows toward the tail, like a fish, an eel, a sea serpent or a crocodile; it dives and steers in three dimensions.',
  tags: ['water', 'body'],
  medium: 'water',
  swim: 'body',
  legPairs: 'any',
  wave: () => 0,
  duty: 1,
  froude: [0.05, 1.2],
  natural: 0.5,
  params: z.strictObject({
    amplitude: z
      .number()
      .min(0.02)
      .max(1)
      .default(0.2)
      .describe('Tail-beat width as a share of body length; the beat comes faster as it narrows'),
    waves: z.number().min(0.5).max(3).default(1).describe('Body waves along the length'),
  }),
});
