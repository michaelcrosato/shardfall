import type { Issue } from '../blueprint/issues.ts';
import { cloneJson, ID_LISTS, isRecord } from '../blueprint/merge.ts';
import { toCurrentFormat } from '../blueprint/migrate.ts';
import {
  applyPatch,
  diffJson,
  type PatchChange,
  type PatchOp,
  parsePath,
  type Step,
} from '../blueprint/patch.ts';
import { roleFieldsOnly } from '../blueprint/schema.ts';
import { blueprintSchemaFor, resolveDocument } from '../blueprint/validate.ts';
import { type ModuleKind, paramsJsonSchema, type Registry } from '../registry.ts';
import type { Rng } from '../rng.ts';

type Json = Record<string, unknown>;

interface Schema {
  type?: string | string[];
  minimum?: number;
  maximum?: number;
  enum?: unknown[];
  const?: unknown;
  default?: unknown;
  properties?: Record<string, Schema>;
  items?: Schema;
  anyOf?: Schema[];
  oneOf?: Schema[];
}

/** One heritable value in an expanded blueprint, with what the schema allows for it. */
export interface Gene {
  /** Id-based path, as in errors and patches: `limbs[id=leg].length`. */
  readonly path: string;
  readonly kind: 'number' | 'profile' | 'enum' | 'color' | 'boolean';
  readonly value: unknown;
  readonly min?: number;
  readonly max?: number;
  readonly int?: boolean;
  readonly options?: readonly unknown[];
}

/** Result of a variation operation: the new blueprint and how it differs from its parent. */
export interface VariationResult {
  readonly ok: boolean;
  readonly blueprint: Json;
  /** Gene by gene, between the expanded parent and the expanded child. */
  readonly diff: readonly PatchChange[];
  readonly errors: readonly Issue[];
  readonly warnings: readonly Issue[];
}

/** Keys that name things or place them rather than shape them; variation leaves them alone. */
const FIXED = new Set([
  'format',
  'name',
  'seed',
  'extends',
  'id',
  'type',
  'role',
  'on',
  'side',
  'region',
  // Media follow the body (wings fly, fins swim); `area` places a covering, as `on` does.
  'media',
  'area',
  // A jaw carries the teeth, the mouth parts and the bite: structure, not a trait to drift.
  'jaw',
]);

/**
 * How many heads and tails a body has. They are not genes: mutation never changes them, and
 * crossbreeding takes each section's count (with its spread and fork) whole from one parent.
 */
export const COUNT_PATHS = ['body.neck.count', 'body.tail.count'] as const;

const options = (node: Schema | undefined): Schema[] =>
  node ? [...(node.anyOf ?? []), ...(node.oneOf ?? [])] : [];

function child(node: Schema | undefined, key: string): Schema | undefined {
  if (!node) return undefined;
  return (
    node.properties?.[key] ?? options(node).find((s) => s.properties?.[key])?.properties?.[key]
  );
}

/** The variant of a union whose `role` (its discriminator) matches the value's. */
function variantOf(node: Schema | undefined, value: Json): Schema | undefined {
  const variants = options(node).filter((s) => s.properties?.role);
  if (variants.length === 0) return node;
  const role = value.role ?? 'leg';
  return (
    variants.find((s) => {
      const r = s.properties?.role;
      return r?.const === role || r?.default === role;
    }) ?? node
  );
}

function itemsOf(node: Schema | undefined): Schema | undefined {
  if (!node) return undefined;
  return node.items ?? options(node).find((s) => s.items)?.items;
}

function numberRange(
  node: Schema | undefined,
): { min: number; max: number; int: boolean } | undefined {
  for (const s of node ? [node, ...options(node)] : [])
    if (typeof s.minimum === 'number' && typeof s.maximum === 'number')
      return { min: s.minimum, max: s.maximum, int: s.type === 'integer' };
  return undefined;
}

function enumOf(node: Schema | undefined): unknown[] | undefined {
  for (const s of node ? [node, ...options(node)] : []) if (s.enum) return s.enum;
  return undefined;
}

/** Which module kind a `{ type, params }` object at this path refers to. */
function moduleKindAt(path: string): ModuleKind | undefined {
  if (/^parts\[[^\]]+\]$/.test(path) || /\.(foot|membrane)$/.test(path)) return 'part';
  if (/^skin\.layers\[[^\]]+\]$/.test(path)) return 'pattern';
  if (/^motion\.gaits\[[^\]]+\]$/.test(path)) return 'gait';
  if (/^motion\.actions\[[^\]]+\]$/.test(path)) return 'action';
  return undefined;
}

const HEX = /^#[0-9a-f]{6}$/i;

/**
 * The blueprint with every preset value and default spelled out, in the form blueprints are
 * written in (feet, layers, gaits and actions carry their module parameters inline), or the errors
 * in it.
 */
export function expand(blueprint: unknown, registry: Registry): { doc?: Json; errors: Issue[] } {
  const outcome = resolveDocument(blueprint, registry);
  if (!outcome.doc) return { errors: outcome.errors };
  const doc = cloneJson(outcome.doc) as unknown as Json;
  const inline = (ref: unknown): unknown => {
    if (!isRecord(ref)) return ref;
    const { params, ...rest } = ref;
    const out: Json = { ...rest, ...(isRecord(params) ? params : {}) };
    for (const [k, v] of Object.entries(out)) if (v === undefined) delete out[k];
    return out;
  };
  for (const limb of (doc.limbs as Json[] | undefined) ?? []) {
    roleFieldsOnly(limb);
    if ('foot' in limb) limb.foot = inline(limb.foot);
    if ('membrane' in limb) limb.membrane = inline(limb.membrane);
  }
  const skin = doc.skin as Json | undefined;
  if (skin && Array.isArray(skin.layers)) skin.layers = skin.layers.map(inline);
  const motion = doc.motion as Json | undefined;
  if (motion && Array.isArray(motion.gaits)) motion.gaits = motion.gaits.map(inline);
  if (motion && Array.isArray(motion.actions)) motion.actions = motion.actions.map(inline);
  return { doc, errors: [] };
}

/**
 * Every gene in an expanded blueprint: numbers and profiles with their schema range, enums with
 * their options, colours and switches. Module parameters use their module's schema.
 */
export function genesOf(doc: Json, registry: Registry): Gene[] {
  const genes: Gene[] = [];
  const walk = (value: unknown, node: Schema | undefined, path: string, key: string) => {
    if (FIXED.has(key) || (COUNT_PATHS as readonly string[]).includes(path)) return;
    // A species range stands where a number goes; it reads as a gene at its lower end.
    const ranged =
      isRecord(value) &&
      Object.keys(value).length === 2 &&
      typeof value.min === 'number' &&
      typeof value.max === 'number';
    if (typeof value === 'number' || ranged) {
      const range = numberRange(node);
      const v = ranged ? ((value as Json).min as number) : value;
      if (range) genes.push({ path, kind: 'number', value: v, ...range });
      return;
    }
    if (typeof value === 'boolean') {
      genes.push({ path, kind: 'boolean', value });
      return;
    }
    if (typeof value === 'string') {
      const list = enumOf(node);
      if (list && list.length > 1) genes.push({ path, kind: 'enum', value, options: list });
      else if (HEX.test(value)) genes.push({ path, kind: 'color', value });
      return;
    }
    if (Array.isArray(value)) {
      if (value.length > 0 && value.every((v) => typeof v === 'number')) {
        const range = numberRange(itemsOf(node));
        if (range) genes.push({ path, kind: 'profile', value, ...range });
        return;
      }
      value.forEach((item, i) => {
        const at = isRecord(item) && typeof item.id === 'string' ? `[id=${item.id}]` : `[${i}]`;
        walk(item, itemsOf(node), `${path}${at}`, '');
      });
      return;
    }
    if (!isRecord(value)) return;
    // A union keyed by a field (limbs by `role`): use the variant this value is.
    node = variantOf(node, value);
    const kind = moduleKindAt(path);
    const module =
      kind && typeof value.type === 'string' ? registry.get(kind, value.type) : undefined;
    const own = module ? (paramsJsonSchema(module.params) as Schema) : undefined;
    const nested = /^parts\[[^\]]+\]$/.test(path);
    for (const [k, v] of Object.entries(value)) {
      // Parts nest their module parameters under `params`; feet, layers, gaits and actions
      // carry them inline.
      const sub = nested
        ? k === 'params'
          ? own
          : child(node, k)
        : (own?.properties?.[k] ?? child(node, k));
      walk(v, sub, path ? `${path}.${k}` : k, k);
    }
  };
  walk(doc, paramsJsonSchema(blueprintSchemaFor(registry)) as Schema, '', '');
  return genes;
}

/** Where a list step points: an index, an id or the first item of a type. */
function indexIn(list: unknown[], step: Step): number {
  if ('index' in step) return step.index;
  if ('id' in step) return list.findIndex((x) => isRecord(x) && x.id === step.id);
  if ('type' in step) return list.findIndex((x) => (isRecord(x) ? x.type : x) === step.type);
  return -1;
}

/** Reads a value at an id-based path. */
export function getAt(doc: unknown, path: string): unknown {
  let node = doc;
  for (const step of parsePath(path)) {
    if ('key' in step) node = isRecord(node) ? node[step.key] : undefined;
    else node = Array.isArray(node) ? node[indexIn(node, step)] : undefined;
    if (node === undefined) return undefined;
  }
  return node;
}

/** Writes a value at an id-based path whose parent already exists. */
export function setAt(doc: unknown, path: string, value: unknown): void {
  let node = doc;
  const steps = parsePath(path);
  for (const [i, step] of steps.entries()) {
    const last = i === steps.length - 1;
    if ('key' in step) {
      if (!isRecord(node)) return;
      if (last) node[step.key] = value;
      else node = node[step.key];
    } else {
      if (!Array.isArray(node)) return;
      const at = indexIn(node, step);
      if (at < 0) return;
      if (last) node[at] = value;
      else node = node[at];
    }
  }
}

/**
 * A path as genes spell it: limbs and parts by id, other list items by index. Accepts the forms
 * patch does (`skin.layers[type=stripes]`, `skin.layers[id=spots]`, `skin.layers[1]`). Undefined
 * when nothing is there.
 */
export function canonicalPath(doc: unknown, path: string): string | undefined {
  if (path === '') return '';
  let node = doc;
  let out = '';
  let steps: Step[];
  try {
    steps = parsePath(path);
  } catch {
    return undefined;
  }
  for (const step of steps) {
    if ('key' in step) {
      if (!isRecord(node) || !(step.key in node)) return undefined;
      node = node[step.key];
      out += out ? `.${step.key}` : step.key;
      continue;
    }
    if (!Array.isArray(node)) return undefined;
    const list = node;
    const at = indexIn(list, step);
    const item = list[at];
    if (item === undefined) return undefined;
    const top = out.split(/[.[]/)[0] ?? '';
    out +=
      ID_LISTS.has(top) && out === top && isRecord(item) && typeof item.id === 'string'
        ? `[id=${item.id}]`
        : `[${at}]`;
    node = item;
  }
  return out;
}

/** True when `path` is `lock` or lies inside it. */
export function isLocked(path: string, locked: readonly string[]): boolean {
  return locked.some(
    (lock) =>
      lock === '' || path === lock || path.startsWith(`${lock}.`) || path.startsWith(`${lock}[`),
  );
}

/** Rounds to three significant figures, so outputs stay readable. */
export const tidy = (v: number, int = false): number =>
  int ? Math.round(v) : Number(v.toPrecision(3));

const stable = (v: unknown) => JSON.stringify(v);

/**
 * Patch operations that turn the expanded `before` into the expanded `after` when applied to
 * `input`, the blueprint `before` was expanded from. Limbs and parts are added, removed and
 * edited by id; other lists are edited item by item when `input` spells them out, else set whole.
 */
export function opsBetween(input: Json, before: Json, after: Json): PatchOp[] {
  const ops: PatchOp[] = [];
  const join = (path: string, k: string) => (path ? `${path}.${k}` : k);
  const visit = (a: unknown, b: unknown, path: string) => {
    if (stable(a) === stable(b)) return;
    if (b === undefined) {
      ops.push({ op: 'remove', path });
      return;
    }
    if (isRecord(a) && isRecord(b)) {
      const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
      if (a.type === b.type) for (const k of keys) visit(a[k], b[k], join(path, k));
      // A new module type: its fields are set whole, so no old parameter leaks into it.
      else
        for (const k of keys)
          if (stable(a[k]) !== stable(b[k]))
            ops.push(
              b[k] === undefined
                ? { op: 'remove', path: join(path, k) }
                : { op: 'set', path: join(path, k), value: cloneJson(b[k]) },
            );
      return;
    }
    if (Array.isArray(a) && Array.isArray(b)) {
      if (ID_LISTS.has(path)) {
        const ids = (list: unknown[]) =>
          new Map(list.filter(isRecord).map((x) => [x.id as string, x] as const));
        const from = ids(a);
        const to = ids(b);
        for (const id of from.keys())
          if (!to.has(id)) ops.push({ op: 'remove', path: `${path}[id=${id}]` });
        for (const [id, item] of to) {
          const old = from.get(id);
          if (old) visit(old, item, `${path}[id=${id}]`);
          else ops.push({ op: 'add', path, value: cloneJson(item) });
        }
        return;
      }
      const written = getAt(input, path);
      const sameShape =
        a.length === b.length &&
        Array.isArray(written) &&
        written.length === a.length &&
        a.every(
          (x, i) =>
            isRecord(x) && isRecord(b[i]) && isRecord(written[i]) && x.type === (b[i] as Json).type,
        );
      if (sameShape) {
        a.forEach((x, i) => {
          visit(x, b[i], `${path}[${i}]`);
        });
        return;
      }
    }
    ops.push({ op: 'set', path, value: cloneJson(b) });
  };
  visit(before, after, '');
  return ops;
}

/**
 * Turns the expanded `child` into edits on `parent` and validates them. Parts, layers, gaits and
 * actions that no longer fit (a part on a section that went away, a bite without a jaw) are
 * dropped rather than failing the whole creature.
 */
export function finish(
  parent: Json,
  parentDoc: Json,
  child: Json,
  registry: Registry,
): VariationResult {
  // Children are written in the current format.
  const base = toCurrentFormat(parent);
  // Limbs the child gained (a wing from the other parent) may go; its own body's limbs may not.
  const own = new Set(
    (Array.isArray(parentDoc.limbs) ? parentDoc.limbs : []).map((l) => (isRecord(l) ? l.id : l)),
  );
  let result = applyPatch(base, opsBetween(base, parentDoc, child), registry);
  for (let round = 0; round < 6 && !result.ok; round++) {
    let dropped = false;
    const drop = new Map<string, Set<number>>();
    for (const e of result.errors) {
      const part = /^parts\[id=([^\]]+)\]/.exec(e.path)?.[1];
      if (part !== undefined && Array.isArray(child.parts)) {
        child.parts = child.parts.filter((p) => !(isRecord(p) && p.id === part));
        dropped = true;
      }
      const limb = /^limbs\[id=([^\]]+)\]/.exec(e.path)?.[1];
      if (limb !== undefined && !own.has(limb) && Array.isArray(child.limbs)) {
        child.limbs = child.limbs.filter((l) => !(isRecord(l) && l.id === limb));
        dropped = true;
      }
      const item = /^(skin\.layers|motion\.gaits|motion\.actions)\[(\d+)\]/.exec(e.path);
      if (item?.[1] && item[2]) {
        const set = drop.get(item[1]) ?? new Set();
        set.add(Number(item[2]));
        drop.set(item[1], set);
      }
    }
    for (const [path, indices] of drop) {
      const list = getAt(child, path);
      if (!Array.isArray(list)) continue;
      setAt(
        child,
        path,
        list.filter((_, i) => !indices.has(i)),
      );
      dropped = true;
    }
    if (!dropped) break;
    result = applyPatch(base, opsBetween(base, parentDoc, child), registry);
  }
  const childDoc = result.ok ? expand(result.blueprint, registry).doc : undefined;
  return {
    ok: result.ok,
    blueprint: result.blueprint,
    diff: childDoc ? diffJson(parentDoc, childDoc) : result.diff,
    errors: result.errors,
    warnings: result.warnings,
  };
}

/** A standard normal draw (Box–Muller). */
export function gauss(rng: Rng): number {
  const u = 1 - rng.next();
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * rng.next());
}

/** Nudges a colour's brightness and tint by up to about `amount`. */
export function nudgeColor(hex: string, rng: Rng, amount: number): string {
  const [r, g, b] = parseHex(hex);
  const shift = gauss(rng) * 40 * amount;
  const tint = () => gauss(rng) * 25 * amount;
  return toHex([r + shift + tint(), g + shift + tint(), b + shift + tint()]);
}

/** Mixes two colours: 0 is `a`, 1 is `b`. */
export function blendColor(a: string, b: string, t: number): string {
  const x = parseHex(a);
  const y = parseHex(b);
  return toHex(x.map((c, i) => c + ((y[i] ?? c) - c) * t) as [number, number, number]);
}

function parseHex(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.slice(1, 7), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function toHex(rgb: readonly number[]): string {
  return `#${rgb
    .map((c) =>
      Math.round(Math.min(255, Math.max(0, c)))
        .toString(16)
        .padStart(2, '0'),
    )
    .join('')}`;
}

/** HSL lightness of a `#rrggbb` colour, 0 to 1. */
export function lightnessOf(hex: string): number {
  const [r, g, b] = parseHex(hex);
  return (Math.max(r, g, b) + Math.min(r, g, b)) / 510;
}

/** The same hue and saturation at another lightness. */
export function withLightness(hex: string, lightness: number): string {
  const [r, g, b] = parseHex(hex).map((c) => c / 255) as [number, number, number];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  const h =
    d === 0
      ? 0
      : max === r
        ? (60 * ((g - b) / d) + 360) % 360
        : max === g
          ? 60 * ((b - r) / d + 2)
          : 60 * ((r - g) / d + 4);
  const L = Math.min(0.97, Math.max(0.03, lightness));
  const a = s * Math.min(L, 1 - L);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return (L - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))) * 255;
  };
  return toHex([f(0), f(8), f(4)]);
}
