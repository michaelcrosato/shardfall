import type { Migration } from '../migrate.ts';
import bestiary01 from './bestiary-0.1.ts';
import spawnforge01 from './spawnforge-0.1.ts';

/**
 * Every format upgrade, oldest first: one file per step, named after the format it reads. A
 * step turns one format into the next; `migrate` chains them up to the current format.
 */
export const MIGRATIONS: readonly Migration[] = [bestiary01, spawnforge01];
