import type { Issue } from './issues.ts';

/** Lists whose items merge by `id`. Every other list replaces the inherited one. */
export const ID_LISTS = new Set(['limbs', 'parts']);

export const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/** Deep copy of JSON-like data (blueprints hold nothing else). */
export function cloneJson<T>(value: T): T {
  if (Array.isArray(value)) return value.map(cloneJson) as T;
  if (isRecord(value)) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value)) out[k] = cloneJson(v);
    return out as T;
  }
  return value;
}

export interface MergeResult {
  merged: Record<string, unknown>;
  warnings: Issue[];
  /** Index of each user list item that has no id, in the user's own list (for error paths). */
  userIndex: WeakMap<object, number>;
}

/** Whether `item` names another module type (or, in `limbs`, another role) than `inherited`. */
function changesKind(
  inherited: Record<string, unknown>,
  item: Record<string, unknown>,
  list: string,
): boolean {
  if (typeof item.type === 'string' && typeof inherited.type === 'string')
    return item.type !== inherited.type;
  if (list === 'limbs' && typeof item.role === 'string')
    return item.role !== (typeof inherited.role === 'string' ? inherited.role : 'leg');
  return false;
}

/**
 * Applies a blueprint onto a preset. Objects merge key by key, `limbs` and `parts` merge by id
 * (a matching id overrides field by field, a new id adds an item, `"remove": true` deletes an
 * inherited one), and any other list or value replaces what was inherited. An object or item that
 * changes `type` (or a limb that changes `role`) replaces the inherited one. Inputs are not
 * modified.
 */
export function mergeBlueprint(
  base: Record<string, unknown>,
  patch: Record<string, unknown>,
): MergeResult {
  const warnings: Issue[] = [];
  const userIndex = new WeakMap<object, number>();

  function mergeList(key: string, inherited: unknown[], items: unknown[]): unknown[] {
    const result = inherited.map((item) => cloneJson(item));
    // Only inherited items can be overridden; a repeated id within the patch is kept so
    // validation can report it.
    const inheritedIds = new Set(inherited.filter(isRecord).map((r) => r.id));
    items.forEach((item, index) => {
      if (!isRecord(item) || typeof item.id !== 'string') {
        if (isRecord(item)) userIndex.set(item, index);
        result.push(item);
        return;
      }
      const at = inheritedIds.has(item.id)
        ? result.findIndex((r) => isRecord(r) && r.id === item.id)
        : -1;
      if (item.remove === true) {
        if (at >= 0) {
          result.splice(at, 1);
        } else {
          warnings.push({
            severity: 'warning',
            path: `${key}[id=${item.id}]`,
            code: 'nothing_to_remove',
            message: `no inherited ${key === 'limbs' ? 'limb' : 'part'} "${item.id}" to remove`,
            fix: 'delete this entry',
          });
        }
        return;
      }
      if (at >= 0) {
        // A limb with another role, or a part of another type, is a new thing in that place.
        const inherited = result[at] as Record<string, unknown>;
        result[at] = changesKind(inherited, item, key) ? item : mergeValue(inherited, item, []);
      } else {
        result.push(item);
      }
    });
    return result;
  }

  function mergeValue(inherited: unknown, value: unknown, path: string[]): unknown {
    if (value === undefined) return cloneJson(inherited);
    // A module object (a foot, a membrane, a layer) whose `type` changes replaces the old one,
    // so the old module's parameters do not leak into the new one.
    if (isRecord(inherited) && isRecord(value) && changesKind(inherited, value, '')) return value;
    if (isRecord(inherited) && isRecord(value)) {
      const out: Record<string, unknown> = {};
      for (const k of new Set([...Object.keys(inherited), ...Object.keys(value)])) {
        out[k] = mergeValue(inherited[k], value[k], [...path, k]);
      }
      return out;
    }
    const key = path.join('.');
    if (Array.isArray(value) && ID_LISTS.has(key)) {
      return mergeList(key, Array.isArray(inherited) ? inherited : [], value);
    }
    return value;
  }

  const merged = mergeValue(base, patch, []) as Record<string, unknown>;
  return { merged, warnings, userIndex };
}
