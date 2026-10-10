import { FORMAT } from '../format.ts';
import type { Issue } from './issues.ts';
import { isRecord } from './merge.ts';
import { MIGRATIONS } from './migrations/index.ts';

/** One format upgrade (see `migrations/`). */
export interface Migration {
  /** The format it reads, e.g. `spawnforge/0.1`. */
  readonly from: string;
  /** The format it writes. */
  readonly to: string;
  /** What changed, for the `migrated` warning. */
  readonly note: string;
  /** Returns the upgraded blueprint; never modifies `doc`. */
  apply(doc: Record<string, unknown>): Record<string, unknown>;
}

export const KNOWN_FORMATS: readonly string[] = [FORMAT, ...MIGRATIONS.map((m) => m.from)];

/** Upgrades an older blueprint to the current format, with a warning per step. */
export function migrate(doc: Record<string, unknown>): {
  doc: Record<string, unknown>;
  issues: Issue[];
} {
  const issues: Issue[] = [];
  let current = doc;
  for (let guard = 0; guard < MIGRATIONS.length + 1; guard++) {
    const format = current.format;
    if (format === FORMAT) return { doc: current, issues };
    if (format === undefined) {
      issues.push({
        severity: 'error',
        path: 'format',
        code: 'missing',
        message: 'is required',
        expected: JSON.stringify(FORMAT),
        fix: `add "format": "${FORMAT}"`,
      });
      return { doc: current, issues };
    }
    const step = MIGRATIONS.find((m) => m.from === format);
    if (!step) {
      issues.push({
        severity: 'error',
        path: 'format',
        code: 'unknown_format',
        message: `${JSON.stringify(format)} is not a format this library reads`,
        expected: KNOWN_FORMATS.map((f) => JSON.stringify(f)).join(', '),
        fix: `use "format": "${FORMAT}"`,
      });
      return { doc: current, issues };
    }
    current = step.apply(current);
    issues.push({
      severity: 'warning',
      path: 'format',
      code: 'migrated',
      message: `migrated from ${step.from} to ${step.to}: ${step.note}`,
      fix: `set "format": "${step.to}"`,
    });
  }
  return { doc: current, issues };
}

/**
 * The blueprint in the current format when the chain can upgrade it, otherwise as it was (for
 * validation to report). Commands that write blueprints use it, so they always write the newest
 * format.
 */
export function toCurrentFormat(doc: Record<string, unknown>): Record<string, unknown> {
  const result = migrate(doc);
  return result.issues.some((i) => i.severity === 'error') ? doc : result.doc;
}

export { isRecord };
