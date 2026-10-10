import type { Migration } from '../migrate.ts';

/**
 * Format 0.2 adds plan 2's vocabulary (several heads and tails, wings, fins and tentacles,
 * coverings, fur, media and more) without changing what any 0.1 field means.
 */
export default {
  from: 'spawnforge/0.1',
  to: 'spawnforge/0.2',
  note: 'format 0.2 only adds fields and modules; nothing in a 0.1 blueprint changes meaning',
  apply: (doc) => ({ ...doc, format: 'spawnforge/0.2' }),
} satisfies Migration;
