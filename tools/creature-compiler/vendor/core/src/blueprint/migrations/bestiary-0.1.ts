import type { Migration } from '../migrate.ts';

/** The project's working name was Bestiary; blueprints from then differ only in `format`. */
export default {
  from: 'bestiary/0.1',
  to: 'spawnforge/0.1',
  note: 'the project was renamed from Bestiary to Spawnforge; the format is otherwise unchanged',
  apply: (doc) => ({ ...doc, format: 'spawnforge/0.1' }),
} satisfies Migration;
