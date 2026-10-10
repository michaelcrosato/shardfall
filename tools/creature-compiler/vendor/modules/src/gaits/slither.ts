import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'slither',
  summary:
    'Legless travelling wave down the spine; each segment follows the path of the one ahead.',
  tags: ['spine', 'snake'],
  legPairs: [0],
  wave: () => 0,
  duty: 1,
  froude: [0, 3],
  params: z.strictObject({
    amplitude: z
      .number()
      .min(0)
      .max(0.6)
      .default(0.18)
      .describe('Sideways swing as a share of body length'),
    waves: z.number().min(0.5).max(4).default(1.5).describe('Wavelengths along the body'),
  }),
});
