import type { Registry } from '../registry.ts';
import { expand, opsBetween } from '../variation/genes.ts';
import type { Issue } from './issues.ts';
import { isRecord } from './merge.ts';
import { applyPatch, type PatchChange, type PatchOp } from './patch.ts';
import { validateBlueprint } from './validate.ts';

type Json = Record<string, unknown>;

export interface BlueprintDiff {
  /** False when either blueprint is invalid (see `errors`). */
  readonly ok: boolean;
  /** Patch operations that turn `a` into `b`, by id-based paths, fewest that do the job. */
  readonly ops: readonly PatchOp[];
  /** What the operations change in `a`, leaf by leaf (`~ path: from → to`, `+`, `-`). */
  readonly changes: readonly PatchChange[];
  /** Whether `a` patched with `ops` resolves to exactly the creature `b` resolves to. */
  readonly exact: boolean;
  /** Errors in `a` (paths prefixed `a:`) or `b` (`b:`). */
  readonly errors: readonly Issue[];
}

function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(',')}]`;
  if (isRecord(value))
    return `{${Object.keys(value)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stable(value[k])}`)
      .join(',')}}`;
  return JSON.stringify(value);
}

/**
 * The edits that turn blueprint `a` into blueprint `b`: compares the creatures they resolve to
 * (presets merged, defaults filled) and writes the difference as `patch` operations on `a`, so
 * `patch a.json <ops>` gives a creature identical to `b`'s.
 */
export function diffBlueprints(a: Json, b: Json, registry: Registry): BlueprintDiff {
  const errors = [
    ...validateBlueprint(a, registry, { minimal: false }).errors.map((e) => ({
      ...e,
      path: `a:${e.path}`,
    })),
    ...validateBlueprint(b, registry, { minimal: false }).errors.map((e) => ({
      ...e,
      path: `b:${e.path}`,
    })),
  ];
  const from = expand(a, registry);
  const to = expand(b, registry);
  if (errors.length > 0 || !from.doc || !to.doc)
    return { ok: false, ops: [], changes: [], exact: false, errors };
  const target = stable(to.doc);
  const reaches = (ops: readonly PatchOp[]) => {
    const result = applyPatch(a, ops, registry);
    if (!result.ok) return false;
    const doc = expand(result.blueprint, registry).doc;
    return doc !== undefined && stable(doc) === target;
  };
  // A new preset can drop items the first round removes, or bring values `a` only inherited, so
  // keep going from the patched blueprint until nothing differs.
  let ops: PatchOp[] = [];
  let current = a;
  for (let round = 0; round < 4; round++) {
    const doc = expand(current, registry).doc;
    if (!doc || stable(doc) === target) break;
    const more = opsBetween(current, doc, to.doc);
    const result = applyPatch(current, more, registry);
    const failed = new Set(
      result.errors
        .filter((e) => e.code === 'bad_patch')
        .map((e) => Number(/^ops\[(\d+)\]/.exec(e.path)?.[1])),
    );
    const kept = more.filter((_, i) => !failed.has(i));
    if (kept.length === 0) break;
    ops = [...ops, ...kept];
    current = applyPatch(a, ops, registry).blueprint;
  }
  const exact = reaches(ops);
  // Drop operations that only restate what the preset or defaults give anyway.
  if (exact)
    for (let i = ops.length - 1; i >= 0; i--) {
      const without = ops.filter((_, j) => j !== i);
      if (reaches(without)) ops = without;
    }
  const changes = ops.length > 0 ? applyPatch(a, ops, registry).diff : [];
  return { ok: true, ops, changes, exact, errors };
}
