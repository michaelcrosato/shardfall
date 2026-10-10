import { defineGait } from '@spawnforge/core';
import { z } from 'zod';

export default defineGait({
  id: 'glide',
  summary: 'Soaring and gliding on spread wings between flaps.',
  tags: ['air', 'wings'],
  medium: 'air',
  air: 'gliding',
  needs: ['wing', 'glide'],
  legPairs: 'any',
  wave: () => 0,
  duty: 0,
  froude: [0.5, 8],
  params: z.strictObject({
    sink: z.number().min(0).max(1).default(0.3).describe('How fast it loses height while gliding'),
  }),
});
