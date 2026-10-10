import type { Registry } from '../registry.ts';
import type { Issue } from './issues.ts';
import { cloneJson, ID_LISTS, isRecord, mergeBlueprint } from './merge.ts';
import { toCurrentFormat } from './migrate.ts';
import { didYouMean } from './suggest.ts';
import { validateBlueprint } from './validate.ts';

type Json = Record<string, unknown>;

/**
 * One edit. Paths are written like error paths: `body.tail.length`,
 * `limbs[id=hindleg].attach.at`, `parts[id=horns].params.curve`, `skin.layers[1].size`.
 * Items in `limbs` and `parts` are found by id, including ones inherited from the preset (the
 * edit then overrides just that field, as merging by id does). Layers, gaits and actions can also
 * be found by type: `skin.layers[type=mottle].strength`, `motion.gaits[type=walk].stride`.
 */
export type PatchOp =
  /** Sets a value, creating objects and id items along the way. */
  | { readonly op: 'set'; readonly path: string; readonly value: unknown }
  /** Appends an item to a list (`limbs`, `parts`, `skin.layers`, `motion.actions`, …). */
  | { readonly op: 'add'; readonly path: string; readonly value: unknown }
  /** Removes a key (back to the preset or default) or a list item; inherited ids are removed with `"remove": true`. */
  | { readonly op: 'remove'; readonly path: string }
  /** Makes a limb or part a mirrored pair (`side` "both"), or puts it on one side. */
  | {
      readonly op: 'mirror';
      readonly path: string;
      readonly side?: 'both' | 'left' | 'right' | 'center';
    }
  /** Multiplies a number, or every number in a profile, by `by`. Path "" scales the whole creature. */
  | { readonly op: 'scale'; readonly path: string; readonly by: number };

export interface PatchChange {
  readonly path: string;
  readonly from?: unknown;
  readonly to?: unknown;
}

export interface PatchResult {
  readonly ok: boolean;
  /** The patched blueprint (also when there are errors, so they can be inspected). */
  readonly blueprint: Json;
  /** What changed, leaf by leaf, with id-based paths. */
  readonly diff: readonly PatchChange[];
  /** Problems applying the operations, then validation errors and warnings. */
  readonly errors: readonly Issue[];
  readonly warnings: readonly Issue[];
}

export type Step = { key: string } | { id: string } | { index: number } | { type: string };

/** A path problem with a suggested fix. */
class PathError extends Error {
  readonly fix: string | undefined;
  constructor(message: string, fix?: string) {
    super(message);
    this.fix = fix;
  }
}

const PATH_FIX =
  'address list items as [id=x] (limbs, parts), [type=x] (layers, gaits, actions) or [0]';

/** Multiplies and drops floating-point noise (0.6 × 1.5 is 0.9, not 0.8999999999999999). */
const times = (v: number, by: number) => Number((v * by).toPrecision(10));

export function parsePath(path: string): Step[] {
  const steps: Step[] = [];
  const re = /([A-Za-z_][\w]*)|\[id=([^\]]+)\]|\[(\d+)\]|\[type=([^\]]+)\]|\./g;
  const unreadable = (at: number) =>
    new PathError(`cannot read "${path}" at "${path.slice(at)}"`, PATH_FIX);
  let m: RegExpExecArray | null = re.exec(path);
  let consumed = 0;
  while (m) {
    if (m.index !== consumed) throw unreadable(consumed);
    if (m[1] !== undefined) steps.push({ key: m[1] });
    else if (m[2] !== undefined) steps.push({ id: m[2] });
    else if (m[3] !== undefined) steps.push({ index: Number(m[3]) });
    else if (m[4] !== undefined) steps.push({ type: m[4] });
    consumed = re.lastIndex;
    m = re.exec(path);
  }
  if (consumed !== path.length) throw unreadable(consumed);
  return steps;
}

/** Applies edit operations to a blueprint, validates the result and lists what changed. */
export function applyPatch(input: Json, ops: readonly PatchOp[], registry: Registry): PatchResult {
  // Edits are written in the current format, so an older blueprint is upgraded first.
  const blueprint = cloneJson(toCurrentFormat(input));
  const errors: Issue[] = [];
  const planPreset = (): Json => {
    const plan =
      typeof blueprint.extends === 'string'
        ? registry.get('bodyPlan', blueprint.extends)
        : undefined;
    return plan && 'preset' in plan ? (plan.preset as Json) : {};
  };
  const preset = (): Json => mergeBlueprint(planPreset(), blueprint).merged;

  /** Finds (or creates) the parent container of the path's last step. */
  const walk = (steps: Step[], create: boolean): { parent: unknown; last: Step } | string => {
    let node: unknown = blueprint;
    const trail: (string | number)[] = [];
    for (const [i, step] of steps.slice(0, -1).entries()) {
      const next = steps[i + 1] as Step;
      if ('key' in step) {
        if (!isRecord(node)) return `${formatSteps(steps.slice(0, i))} is not an object`;
        let child = node[step.key];
        if (child === undefined && create) {
          // Lists that replace their inherited value start from the merged one.
          const inherited = valueAt(preset(), [...trail, step.key]);
          child =
            'key' in next
              ? {}
              : 'index' in next && Array.isArray(inherited)
                ? cloneJson(inherited)
                : [];
          node[step.key] = child;
        }
        node = child;
        trail.push(step.key);
      } else if ('id' in step) {
        if (!Array.isArray(node)) return `${formatSteps(steps.slice(0, i))} is not a list`;
        let item = node.find((x) => isRecord(x) && x.id === step.id);
        if (item === undefined && create) {
          const inherited = valueAt(preset(), trail);
          const known =
            Array.isArray(inherited) && inherited.some((x) => isRecord(x) && x.id === step.id);
          if (!known) {
            const ids = [...node, ...(Array.isArray(inherited) ? inherited : [])]
              .map((x) => (isRecord(x) && typeof x.id === 'string' ? x.id : undefined))
              .filter((x): x is string => x !== undefined);
            const near = didYouMean(step.id, ids);
            throw new PathError(
              `nothing with id "${step.id}" in ${formatSteps(steps.slice(0, i))}`,
              near ? `did you mean "${near}"?` : `ids there: ${ids.join(', ') || 'none'}`,
            );
          }
          item = { id: step.id };
          node.push(item);
        }
        node = item;
        trail.push(-1);
      } else if ('index' in step) {
        if (!Array.isArray(node)) return `${formatSteps(steps.slice(0, i))} is not a list`;
        node = node[step.index];
        trail.push(step.index);
      } else return `${formatSteps(steps.slice(0, i + 1))} was not resolved`;
      if (node === undefined) return `nothing at ${formatSteps(steps.slice(0, i + 1))}`;
    }
    return { parent: node, last: steps.at(-1) as Step };
  };

  /**
   * Turns `[type=x]` steps into indexes of the first item of that type in the list as merged
   * with the preset. A gait or action written as a bare string becomes `{ "type": … }` so its
   * fields can be set.
   */
  const resolveTypes = (steps: Step[]): Step[] => {
    const out: Step[] = [];
    for (const [i, step] of steps.entries()) {
      if (!('type' in step)) {
        out.push(step);
        continue;
      }
      const list = nodeAt(preset(), out);
      const types = Array.isArray(list) ? list.map((x) => (isRecord(x) ? x.type : x)) : [];
      const index = types.indexOf(step.type);
      if (index < 0)
        throw new PathError(
          `nothing of type "${step.type}" in ${formatSteps(out)}`,
          types.length > 0
            ? `types there: ${types.join(', ')}`
            : `${formatSteps(out)} is not set yet; add an item first`,
        );
      const own = nodeAt(blueprint, out);
      if (Array.isArray(own) && typeof own[index] === 'string' && i < steps.length - 1)
        own[index] = { type: own[index] };
      out.push({ index });
    }
    return out;
  };

  const applyOne = (op: PatchOp, i: number): void => {
    const fail = (message: string, fix?: string): void => {
      errors.push({
        severity: 'error',
        path: `ops[${i}]`,
        code: 'bad_patch',
        message,
        ...(fix ? { fix } : {}),
      });
    };
    try {
      if (op.op === 'scale' && op.path === '') {
        const current = typeof blueprint.scale === 'number' ? blueprint.scale : preset().scale;
        blueprint.scale = times(typeof current === 'number' ? current : 1, op.by);
        return;
      }
      const steps = resolveTypes(parsePath(op.path));
      if (steps.length === 0) {
        fail('the path is empty');
        return;
      }
      // Removing an inherited item writes `"remove": true`, so its list may need creating.
      const found = walk(steps, op.op !== 'remove' || 'id' in (steps.at(-1) as Step));
      if (typeof found === 'string') {
        fail(found);
        return;
      }
      const { parent, last } = found;
      const read = (): unknown =>
        'key' in last && isRecord(parent)
          ? parent[last.key]
          : 'index' in last && Array.isArray(parent)
            ? parent[last.index]
            : 'id' in last && Array.isArray(parent)
              ? parent.find((x) => isRecord(x) && x.id === last.id)
              : undefined;
      const write = (value: unknown) => {
        if ('key' in last && isRecord(parent)) parent[last.key] = value;
        else if ('index' in last && Array.isArray(parent)) parent[last.index] = value;
        else throw new Error('cannot write there');
      };
      switch (op.op) {
        case 'set':
          if ('id' in last) {
            fail('set a field of the item, e.g. ".length", not the item');
            return;
          }
          write(cloneJson(op.value));
          return;
        case 'add': {
          let list = read();
          if (list === undefined) {
            const inherited = valueAt(preset(), steps.map(stepKey));
            list =
              'key' in last && !ID_LISTS.has(last.key) && Array.isArray(inherited)
                ? cloneJson(inherited)
                : [];
            write(list);
          }
          if (!Array.isArray(list)) {
            fail(`${op.path} is not a list`);
            return;
          }
          list.push(cloneJson(op.value));
          return;
        }
        case 'remove': {
          if ('key' in last && isRecord(parent)) {
            delete parent[last.key];
            return;
          }
          if (Array.isArray(parent) && 'index' in last) {
            parent.splice(last.index, 1);
            return;
          }
          if (Array.isArray(parent) && 'id' in last) {
            const at = parent.findIndex((x) => isRecord(x) && x.id === last.id);
            // Inherited means from the body plan, not an item the blueprint itself added.
            const inherited = valueAt(planPreset(), steps.slice(0, -1).map(stepKey));
            const fromPreset =
              Array.isArray(inherited) && inherited.some((x) => isRecord(x) && x.id === last.id);
            if (at >= 0) parent.splice(at, 1);
            if (fromPreset) parent.push({ id: last.id, remove: true });
            if (at < 0 && !fromPreset) fail(`nothing with id "${last.id}" to remove`);
            return;
          }
          fail(`nothing at ${op.path}`);
          return;
        }
        case 'mirror': {
          const item = read();
          const target =
            item === undefined && 'id' in last && Array.isArray(parent)
              ? (() => {
                  const created = { id: last.id };
                  parent.push(created);
                  return created;
                })()
              : item;
          if (!isRecord(target)) {
            fail(`${op.path} is not a limb or part`);
            return;
          }
          const attach = isRecord(target.attach) ? target.attach : {};
          attach.side = op.side ?? 'both';
          target.attach = attach;
          return;
        }
        case 'scale': {
          const value = read();
          const scaled = Array.isArray(value)
            ? value.map((v) => (typeof v === 'number' ? times(v, op.by) : v))
            : typeof value === 'number'
              ? times(value, op.by)
              : undefined;
          if (scaled === undefined) {
            const inherited = valueAt(preset(), steps.map(stepKey));
            if (typeof inherited === 'number') write(times(inherited, op.by));
            else if (Array.isArray(inherited))
              write(inherited.map((v) => (typeof v === 'number' ? times(v, op.by) : v)));
            else {
              fail(`${op.path} has no number to scale`, 'set it first');
              return;
            }
            return;
          }
          write(scaled);
          return;
        }
      }
    } catch (error) {
      fail((error as Error).message, error instanceof PathError ? error.fix : undefined);
    }
  };
  for (const [i, op] of ops.entries()) applyOne(op, i);

  const result = validateBlueprint(blueprint, registry, { minimal: false });
  return {
    ok: errors.length === 0 && result.ok,
    blueprint,
    diff: diffJson(input, blueprint),
    errors: [...errors, ...result.errors],
    warnings: result.warnings,
  };
}

function stepKey(step: Step): string | number {
  return 'key' in step ? step.key : 'index' in step ? step.index : -1;
}

function formatSteps(steps: readonly Step[]): string {
  let out = '';
  for (const s of steps) {
    if ('key' in s) out += out ? `.${s.key}` : s.key;
    else if ('id' in s) out += `[id=${s.id}]`;
    else if ('type' in s) out += `[type=${s.type}]`;
    else out += `[${s.index}]`;
  }
  return out || '(root)';
}

/** The value at a path of key, id and index steps, or undefined. */
function nodeAt(doc: unknown, steps: readonly Step[]): unknown {
  let node = doc;
  for (const step of steps) {
    if ('key' in step) node = isRecord(node) ? node[step.key] : undefined;
    else if ('id' in step)
      node = Array.isArray(node) ? node.find((x) => isRecord(x) && x.id === step.id) : undefined;
    else if ('index' in step) node = Array.isArray(node) ? node[step.index] : undefined;
    else return undefined;
    if (node === undefined) return undefined;
  }
  return node;
}

/** Reads a value by keys; -1 stands for "an id item" and stops there. */
function valueAt(doc: unknown, keys: readonly (string | number)[]): unknown {
  let node = doc;
  for (const key of keys) {
    if (key === -1) return undefined;
    if (typeof key === 'number') node = Array.isArray(node) ? node[key] : undefined;
    else node = isRecord(node) ? node[key] : undefined;
    if (node === undefined) return undefined;
  }
  return node;
}

/** Leaf-by-leaf differences, matching list items by id where they have one. */
export function diffJson(before: unknown, after: unknown, path = ''): PatchChange[] {
  if (Object.is(before, after)) return [];
  const join = (key: string) => (path ? `${path}.${key}` : key);
  // A new or removed object or list shows as its contents, item by item (a whole list item
  // shows as one line).
  const item = path.endsWith(']');
  if (before === undefined && (isRecord(after) || Array.isArray(after)) && path !== '' && !item)
    if (!Array.isArray(after) || after.every((x) => isRecord(x) && typeof x.id === 'string'))
      return diffJson(Array.isArray(after) ? [] : {}, after, path);
  if (after === undefined && (isRecord(before) || Array.isArray(before)) && path !== '' && !item)
    if (!Array.isArray(before) || before.every((x) => isRecord(x) && typeof x.id === 'string'))
      return diffJson(before, Array.isArray(before) ? [] : {}, path);
  if (isRecord(before) && isRecord(after)) {
    const keys = [...new Set([...Object.keys(before), ...Object.keys(after)])];
    return keys.flatMap((k) => diffJson(before[k], after[k], join(k)));
  }
  if (Array.isArray(before) && Array.isArray(after)) {
    const byId = (list: unknown[]) =>
      list.every((x) => isRecord(x) && typeof x.id === 'string')
        ? new Map(list.map((x) => [(x as Json).id as string, x]))
        : undefined;
    const a = byId(before);
    const b = byId(after);
    if (a && b && (a.size > 0 || b.size > 0)) {
      const ids = [...new Set([...a.keys(), ...b.keys()])];
      return ids.flatMap((id) => diffJson(a.get(id), b.get(id), `${path}[id=${id}]`));
    }
    if (before.every((x) => !isRecord(x)) && after.every((x) => !isRecord(x)))
      return JSON.stringify(before) === JSON.stringify(after)
        ? []
        : [{ path, from: before, to: after }];
    const n = Math.max(before.length, after.length);
    return Array.from({ length: n }, (_, i) =>
      diffJson(before[i], after[i], `${path}[${i}]`),
    ).flat();
  }
  return [
    {
      path,
      ...(before === undefined ? {} : { from: before }),
      ...(after === undefined ? {} : { to: after }),
    },
  ];
}

/** One line per change: `~ path: from → to`, `+ path: value`, `- path`. */
export function formatDiff(diff: readonly PatchChange[]): string[] {
  const show = (v: unknown) => JSON.stringify(v);
  return diff.map((d) =>
    d.from === undefined
      ? `+ ${d.path}: ${show(d.to)}`
      : d.to === undefined
        ? `- ${d.path}`
        : `~ ${d.path}: ${show(d.from)} → ${show(d.to)}`,
  );
}
