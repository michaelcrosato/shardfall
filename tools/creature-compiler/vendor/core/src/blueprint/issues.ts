import type { z } from 'zod';
import { COLOR_NAMES } from './colors.ts';
import { didYouMean } from './suggest.ts';

/** Fixes for keys models tend to guess, keyed by `section.key` or just `key`. */
const HINTS: Record<string, string> = {
  'body.torso.length':
    'the torso length is the blueprint `scale` (metres); remove it and set "scale"',
  'body.torso.size': 'the torso length is the blueprint `scale` (metres); widths use "radius"',
  'body.head.size': 'use "length" and "radius"',
  colour: 'spelled "color"',
  wings: 'wings are limbs: add { "id": "wing", "role": "wing" } to the top-level "limbs" list',
  fins: 'fins are limbs ({ "role": "fin" }) for pairs, or fin parts on the midline (list_modules)',
  tentacles: 'tentacles are limbs: add { "id": "tentacle", "role": "tentacle" } to "limbs"',
  legs: 'legs are items in the top-level "limbs" list with "role": "leg"',
  arms: 'arms are items in the top-level "limbs" list with "role": "arm"',
  size: 'sizes are relative: lengths and radii are multiples of the blueprint `scale`',
  'body.heads': 'heads come one per neck: set "body": { "neck": { "count": 3 } }',
  'body.head.count': 'heads come one per neck: set "body": { "neck": { "count": 3 } }',
  'body.tails': 'set "body": { "tail": { "count": 2 } }',
  'body.fur': 'fur is a coat over the skin: set "skin": { "fur": {} }',
  'limbs.hand': 'a hand goes in "foot", whatever the limb (list_modules shows the hand modules)',
  'limbs.count':
    'write one entry per pair ("side": "both" mirrors it), each pair with its own attach.at or angle',
  'limbs.curl': 'only tentacles curl: set "role": "tentacle", or remove it',
  'limbs.curlStart': 'only tentacles curl: set "role": "tentacle", or remove it',
  'limbs.membrane':
    'only wings and fins carry a membrane: set "role": "wing" or "fin", or remove it',
  'limbs.stance': 'only legs have a stance; remove it',
  'limbs.splay': 'only legs and arms take "splay"; remove it',
  'limbs.lift': 'only legs and arms take "lift"; remove it',
  'parts.attach.region':
    'coverings use "area" (back, belly, sides or all); "region" belongs to skin layers',
  'skin.layers.area': 'layers use "region" (all, back, belly, head, torso, limbs, tail, wings)',
  'skin.fur.color': 'fur takes its colours from the palette and the pattern layers',
  'motion.fly': 'say where it moves: "media": { "air": true } (it needs a wing limb)',
  'motion.flying': 'say where it moves: "media": { "air": true } (it needs a wing limb)',
  'motion.flies': 'say where it moves: "media": { "air": true } (it needs a wing limb)',
  'motion.swim': 'say where it moves: "media": { "water": true }',
  'motion.swims': 'say where it moves: "media": { "water": true }',
  'motion.swimming': 'say where it moves: "media": { "water": true }',
  'motion.aquatic': 'say where it moves: "media": { "water": true }',
};

/** Fixes for values models reach for that the format spells another way, keyed `path:value`. */
const VALUE_HINTS: Record<string, string> = {
  'skin.material:fur':
    'fur is a coat over a material: keep "material" and add "skin": { "fur": {} }',
  'skin.material:feathers': 'only wings have feathers, as the membrane of a wing limb',
  'limbs.role:flipper': 'a flipper is a fin without a membrane: "role": "fin", "membrane": null',
  'limbs.role:pincer': 'pincers are hands: an arm whose "foot" is a pincer module',
  'limbs.role:claw': 'claws are feet or hands: a leg or arm whose "foot" has claws',
  'limbs.role:antenna': 'antennae are parts (list_modules with kind "part")',
  'limbs.role:tail': 'tails are body sections: set body.tail (body.tail.count for several)',
  'limbs.role:head': 'heads come one per neck: set body.neck.count',
  'limbs.role:neck': 'necks are body sections: set body.neck (body.neck.count for several)',
};

/** A problem found in a blueprint, written for a model to read and fix. */
export interface Issue {
  readonly severity: 'error' | 'warning';
  /** Id-based path as written in the file, e.g. `limbs[id=hindleg].attach.at`; "" is the root. */
  readonly path: string;
  /** Stable machine-readable code, e.g. `unknown_key`, `out_of_range`, `unknown_reference`. */
  readonly code: string;
  /** What is wrong, e.g. `1.4 is outside 0–1`. */
  readonly message: string;
  /** The valid range or values, when there are any. */
  readonly expected?: string;
  /** A concrete suggested fix, e.g. `did you mean "length"?`. */
  readonly fix?: string;
}

export type PathKey = string | number;

/** One-line rendering: `path: message (expected …) — fix`. */
export function formatIssue(issue: Issue): string {
  const where = issue.path === '' ? '(root)' : issue.path;
  const expected =
    issue.expected === undefined || issue.message.includes(issue.expected)
      ? ''
      : ` (expected ${issue.expected})`;
  const fix = issue.fix === undefined ? '' : ` — ${issue.fix}`;
  return `${where}: ${issue.message}${expected}${fix}`;
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/**
 * Renders a path against the document it points into, addressing list items by id so paths stay
 * stable when lists merge or mirror: ['limbs', 1, 'attach', 'at'] → `limbs[id=hindleg].attach.at`.
 * Items without an id fall back to `[n]`, or to `indexOf(item)` when given (e.g. the user's own
 * index for an item that merging moved).
 */
export function formatPath(
  path: readonly PathKey[],
  doc: unknown,
  indexOf?: (item: unknown) => number | undefined,
): string {
  let out = '';
  let node: unknown = doc;
  for (const key of path) {
    if (typeof key === 'number') {
      const item = Array.isArray(node) ? node[key] : undefined;
      const id = isRecord(item) && typeof item.id === 'string' ? item.id : undefined;
      out += id === undefined ? `[${indexOf?.(item) ?? key}]` : `[id=${id}]`;
      node = item;
    } else {
      out += out === '' ? key : `.${key}`;
      node = isRecord(node) ? node[key] : undefined;
    }
  }
  return out;
}

// --- Walking Zod schemas (to find allowed keys and bounds for an issue's path) ---------------

interface Def {
  type: string;
  entries?: Record<string, string | number>;
  innerType?: z.ZodType;
  shape?: Record<string, z.ZodType>;
  catchall?: z.ZodType;
  element?: z.ZodType;
  options?: z.ZodType[];
  valueType?: z.ZodType;
  in?: z.ZodType;
}

const defOf = (s: z.ZodType): Def => (s as unknown as { _zod: { def: Def } })._zod.def;

export function unwrap(schema: z.ZodType): z.ZodType {
  let s = schema;
  for (let i = 0; i < 16; i++) {
    const def = defOf(s);
    if (
      def.innerType &&
      ['optional', 'default', 'prefault', 'nullable', 'readonly', 'catch'].includes(def.type)
    ) {
      s = def.innerType;
    } else if (def.type === 'pipe' && def.in) {
      s = def.in;
    } else {
      break;
    }
  }
  return s;
}

/**
 * The option of a union that fits `value`: the first whose shape matches, preferring an object
 * whose literal fields (a discriminator such as `role`) agree with the value's.
 */
function pickOption(options: readonly z.ZodType[], value: unknown): z.ZodType | undefined {
  const shaped = options.filter((o) => matchesShape(o, value));
  if (isRecord(value)) {
    const agrees = shaped.find((o) => {
      const shape = defOf(unwrap(o)).shape ?? {};
      // Literal fields the value sets, or that default (a missing discriminator takes the
      // literal's default); optional literals it leaves out, such as `remove`, say nothing.
      const literals = Object.entries(shape)
        .filter(([, s]) => defOf(unwrap(s)).type === 'literal')
        .map(([key, s]) => ({
          allowed: (defOf(unwrap(s)) as { values?: unknown[] }).values ?? [],
          own: value[key] ?? (defOf(s) as { defaultValue?: unknown }).defaultValue,
        }))
        .filter((l) => l.own !== undefined);
      return literals.length > 0 && literals.every((l) => l.allowed.includes(l.own));
    });
    if (agrees) return agrees;
  }
  return shaped[0];
}

function matchesShape(schema: z.ZodType, value: unknown): boolean {
  const t = defOf(unwrap(schema)).type;
  if (Array.isArray(value)) return t === 'array';
  if (isRecord(value)) return t === 'object' || t === 'record';
  if (value === null) return t === 'null';
  return typeof value === t || (t === 'enum' && typeof value === 'string') || t === 'literal';
}

/** The schema node for `key` inside `schema`, given the value found there. */
function childSchema(schema: z.ZodType, key: PathKey, value: unknown): z.ZodType | undefined {
  let s = unwrap(schema);
  let def = defOf(s);
  if (def.type === 'union' && def.options) {
    s = unwrap(pickOption(def.options, value) ?? def.options[0] ?? s);
    def = defOf(s);
  }
  if (def.type === 'object') return def.shape?.[String(key)] ?? def.catchall;
  if (def.type === 'array') return def.element;
  if (def.type === 'record') return def.valueType;
  return undefined;
}

/** Walks `path` through both the schema and the input, returning the schema node and value there. */
export function locate(
  schema: z.ZodType,
  root: unknown,
  path: readonly PathKey[],
): { schema: z.ZodType | undefined; value: unknown } {
  let s: z.ZodType | undefined = schema;
  let v: unknown = root;
  for (const key of path) {
    const next: unknown =
      Array.isArray(v) || isRecord(v) ? (v as Record<PathKey, unknown>)[key] : undefined;
    s = s === undefined ? undefined : childSchema(s, key, v);
    v = next;
  }
  return { schema: s, value: v };
}

/** Allowed keys of an object schema (undefined for open objects). */
export function objectKeys(schema: z.ZodType, value: unknown): string[] | undefined {
  let s = unwrap(schema);
  let def = defOf(s);
  if (def.type === 'union' && def.options) {
    s = unwrap(pickOption(def.options, value) ?? s);
    def = defOf(s);
  }
  return def.type === 'object' && def.shape ? Object.keys(def.shape) : undefined;
}

/**
 * A sibling field whose options include `value`: `body.head.shape: "wide"` is right for its
 * sibling `crossSection`.
 */
function siblingWith(
  schema: z.ZodType,
  input: unknown,
  path: readonly PathKey[],
  value: string,
): string | undefined {
  const key = path.at(-1);
  if (typeof key !== 'string') return undefined;
  const parent = locate(schema, input, path.slice(0, -1));
  if (!parent.schema) return undefined;
  let s = unwrap(parent.schema);
  let def = defOf(s);
  if (def.type === 'union' && def.options) {
    s = unwrap(def.options.find((o) => matchesShape(o, parent.value)) ?? s);
    def = defOf(s);
  }
  if (def.type !== 'object' || !def.shape) return undefined;
  for (const [name, child] of Object.entries(def.shape)) {
    if (name === key) continue;
    const entries = defOf(unwrap(child)).entries;
    if (entries && Object.values(entries).includes(value)) return name;
  }
  return undefined;
}

function numberBounds(schema: z.ZodType | undefined): { min?: number; max?: number } {
  if (schema === undefined) return {};
  let s = unwrap(schema);
  const def = defOf(s);
  if (def.type === 'union' && def.options) {
    s = unwrap(def.options[0] ?? s);
  }
  if (defOf(s).type === 'array' && defOf(s).element) s = unwrap(defOf(s).element as z.ZodType);
  const n = s as unknown as { minValue?: number | null; maxValue?: number | null };
  return {
    ...(typeof n.minValue === 'number' && Number.isFinite(n.minValue) ? { min: n.minValue } : {}),
    ...(typeof n.maxValue === 'number' && Number.isFinite(n.maxValue) ? { max: n.maxValue } : {}),
  };
}

const describeValue = (v: unknown): string =>
  v === undefined
    ? 'nothing'
    : Array.isArray(v)
      ? 'a list'
      : v === null
        ? 'null'
        : typeof v === 'object'
          ? 'an object'
          : JSON.stringify(v);

const typeName = (v: unknown): string =>
  v === undefined
    ? 'nothing'
    : v === null
      ? 'null'
      : Array.isArray(v)
        ? 'list'
        : typeof v === 'object'
          ? 'object'
          : typeof v;

const quoteList = (values: readonly unknown[]) => values.map((v) => JSON.stringify(v)).join(', ');

interface RawIssue {
  code: string;
  discriminator?: string;
  options?: unknown[];
  path: PropertyKey[];
  message: string;
  keys?: string[];
  values?: unknown[];
  expected?: string;
  minimum?: number | bigint;
  maximum?: number | bigint;
  origin?: string;
  errors?: RawIssue[][];
  format?: string;
  pattern?: string;
}

/**
 * Converts Zod issues into blueprint issues with id-based paths, ranges and suggested fixes.
 * `schema` and `input` are what was parsed; `prefix` places a sub-document (e.g. a part's params)
 * inside the blueprint.
 */
export function fromZodIssues(
  rawIssues: readonly unknown[],
  schema: z.ZodType,
  input: unknown,
  pathString: (path: readonly PathKey[]) => string,
  /** Hints from the packs (`Registry.hints()`), which may name their modules. */
  packHints: Readonly<Record<string, string>> = {},
): Issue[] {
  const out: Issue[] = [];
  for (const raw of rawIssues as RawIssue[]) {
    const path = raw.path.map((k) => (typeof k === 'number' ? k : String(k)));
    const at = locate(schema, input, path);
    const issue = convert(raw, path, at, schema, input, pathString, packHints);
    out.push(...issue);
  }
  return out;
}

function convert(
  raw: RawIssue,
  path: PathKey[],
  at: { schema: z.ZodType | undefined; value: unknown },
  schema: z.ZodType,
  input: unknown,
  pathString: (path: readonly PathKey[]) => string,
  packHints: Readonly<Record<string, string>>,
): Issue[] {
  const p = pathString(path);
  const hints: Record<string, string> = { ...HINTS, ...packHints };
  const valueHints: Record<string, string> = { ...VALUE_HINTS, ...packHints };
  const err = (code: string, message: string, extra: Partial<Issue> = {}): Issue => ({
    severity: 'error',
    path: p,
    code,
    message,
    ...extra,
  });

  switch (raw.code) {
    case 'unrecognized_keys': {
      const known = at.schema ? (objectKeys(at.schema, at.value) ?? []) : [];
      return (raw.keys ?? []).map((key) => {
        const where = path.filter((k) => typeof k === 'string').join('.');
        const scoped = Object.keys(hints)
          .filter((h) => h.startsWith(`${where}.`) && !h.includes(':'))
          .map((h) => h.slice(where.length + 1));
        const hintKey = hints[`${where}.${key}`] !== undefined ? key : didYouMean(key, scoped);
        const hint =
          (hintKey !== undefined ? hints[`${where}.${hintKey}`] : undefined) ?? hints[key];
        const guess = hint === undefined ? didYouMean(key, known) : undefined;
        return {
          severity: 'error' as const,
          path: pathString([...path, key]),
          code: 'unknown_key',
          message: `unknown key "${key}"`,
          expected: known.length > 0 ? `one of ${quoteList(known)}` : undefined,
          fix: hint ?? (guess ? `did you mean "${guess}"?` : 'remove it'),
        };
      });
    }
    case 'too_small':
    case 'too_big': {
      if (raw.origin === 'array' || raw.origin === 'string') {
        const n = raw.code === 'too_small' ? raw.minimum : raw.maximum;
        const unit = raw.origin === 'array' ? 'items' : 'characters';
        return [
          err(
            'bad_length',
            `${raw.code === 'too_small' ? 'needs at least' : 'allows at most'} ${n} ${unit}`,
          ),
        ];
      }
      const bounds = numberBounds(at.schema);
      const min = bounds.min ?? (raw.code === 'too_small' ? Number(raw.minimum) : undefined);
      const max = bounds.max ?? (raw.code === 'too_big' ? Number(raw.maximum) : undefined);
      const value = at.value;
      const expected =
        min !== undefined && max !== undefined
          ? `${min}–${max}`
          : min !== undefined
            ? `≥ ${min}`
            : `≤ ${max}`;
      const clamp = raw.code === 'too_small' ? min : max;
      return [
        err('out_of_range', `${describeValue(value)} is outside ${expected}`, {
          expected,
          ...(clamp !== undefined ? { fix: `use a value in range, e.g. ${clamp}` } : {}),
        }),
      ];
    }
    case 'invalid_value': {
      const values = raw.values ?? [];
      const v = at.value;
      const where = path.filter((k) => typeof k === 'string').join('.');
      const valueHint = typeof v === 'string' ? valueHints[`${where}:${v}`] : undefined;
      if (valueHint)
        return [
          err('invalid_value', `${describeValue(v)} is not allowed`, {
            expected: `one of ${quoteList(values)}`,
            fix: valueHint,
          }),
        ];
      const guess = typeof v === 'string' ? didYouMean(v, values.map(String)) : undefined;
      const sibling = typeof v === 'string' ? siblingWith(schema, input, path, v) : undefined;
      const field = path.at(-1);
      const expected =
        values.length <= 24
          ? `one of ${quoteList(values)}`
          : `one of ${values.length} known values`;
      return [
        err('invalid_value', `${describeValue(v)} is not allowed`, {
          expected,
          fix: sibling
            ? `${JSON.stringify(v)} is a ${sibling}, not a ${String(field)}: set "${sibling}": ${JSON.stringify(v)}${guess ? `, or did you mean "${guess}"?` : ''}`
            : guess
              ? `did you mean "${guess}"?`
              : `pick one of ${quoteList(values.slice(0, 8))}`,
        }),
      ];
    }
    case 'invalid_type': {
      const v = at.value;
      if (v === undefined) {
        return [
          err('missing', 'is required', {
            expected: raw.expected,
            fix: `add ${p.split('.').at(-1)}`,
          }),
        ];
      }
      return [
        err('invalid_type', `expected ${raw.expected}, got ${typeName(v)}`, {
          expected: raw.expected,
        }),
      ];
    }
    case 'invalid_union': {
      // A union keyed by a field (limbs by `role`): the value of that field is not one it knows.
      if (raw.discriminator) {
        const values = (raw.options ?? []).filter((o) => o !== null && o !== undefined);
        return convert(
          { ...raw, code: 'invalid_value', values },
          path,
          locate(schema, input, path),
          schema,
          input,
          pathString,
          packHints,
        );
      }
      // Report the branch that matches the value's shape (a number vs a list, a string vs an object).
      const v = at.value;
      const branches = raw.errors ?? [];
      const unionDef = at.schema ? defOf(unwrap(at.schema)) : undefined;
      const options = unionDef?.options ?? [];
      const index = options.findIndex((o) => matchesShape(o, v));
      const branch = index >= 0 ? branches[index] : undefined;
      if (branch && branch.length > 0) {
        return fromZodIssues(
          branch.map((b) => ({ ...b, path: [...path, ...b.path] })),
          schema,
          input,
          pathString,
          packHints,
        );
      }
      const shapes = options.map((o) => defOf(unwrap(o)).type).join(' or ');
      return [err('invalid_type', `expected ${shapes}, got ${typeName(v)}`, { expected: shapes })];
    }
    case 'invalid_format': {
      if (raw.format === 'regex') {
        return [
          err('invalid_id', `${describeValue(at.value)} is not a valid id`, {
            expected: 'lowercase letters, digits, "-" or "_", starting with a letter',
            fix:
              typeof at.value === 'string'
                ? `use "${
                    at.value
                      .toLowerCase()
                      .replace(/[^a-z0-9_-]+/g, '-')
                      .replace(/^[^a-z]+/, '') || 'part'
                  }"`
                : undefined,
          }),
        ];
      }
      return [err('invalid_format', raw.message)];
    }
    case 'custom': {
      const v = at.value;
      const guess = typeof v === 'string' ? didYouMean(v, COLOR_NAMES) : undefined;
      return [
        err('invalid_value', `${describeValue(v)} is ${raw.message}`, {
          ...(raw.message === 'not a colour'
            ? {
                expected: '"#rrggbb", "#rgb" or a CSS colour name',
                fix: guess ? `did you mean "${guess}"?` : 'use a colour such as "#7a6a50"',
              }
            : {}),
        }),
      ];
    }
    default:
      return [err(raw.code, raw.message)];
  }
}
