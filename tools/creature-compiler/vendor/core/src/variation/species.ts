import type { Issue } from '../blueprint/issues.ts';
import { cloneJson, isRecord } from '../blueprint/merge.ts';
import { toCurrentFormat } from '../blueprint/migrate.ts';
import { validateBlueprint } from '../blueprint/validate.ts';
import type { Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import { blendColor, genesOf } from './genes.ts';

/**
 * A species is a blueprint with ranges in it: anywhere a number goes, `{ "min": 0.5, "max": 0.7 }`
 * stands for "somewhere in between", and anywhere a colour goes, `{ "min": "#5a5a5a", "max":
 * "#9a9a9a" }` for a colour between the two. `instantiate` resolves every range for one seed.
 * Each range draws from its own stream, keyed by its id-based path, so changing one range never
 * reshuffles the others.
 */
export type Species = Record<string, unknown>;

const HEX = /^#[0-9a-f]{6}$/i;

/** A range: exactly the keys "min" and "max", both numbers or both `#rrggbb` colours. */
export function isRange(
  value: unknown,
): value is { min: number; max: number } | { min: string; max: string } {
  if (!isRecord(value) || Object.keys(value).length !== 2) return false;
  const { min, max } = value;
  return (
    (typeof min === 'number' && typeof max === 'number') ||
    (typeof min === 'string' && typeof max === 'string' && HEX.test(min) && HEX.test(max))
  );
}

/** True when the blueprint has any range in it. */
export function isSpecies(blueprint: unknown): boolean {
  if (isRange(blueprint)) return true;
  if (Array.isArray(blueprint)) return blueprint.some(isSpecies);
  if (isRecord(blueprint)) return Object.values(blueprint).some(isSpecies);
  return false;
}

const itemKey = (item: unknown, i: number) =>
  isRecord(item) && typeof item.id === 'string' ? `[id=${item.id}]` : `[${i}]`;

/** Replaces every range using `pick(range, path)`. */
function resolveRanges(
  value: unknown,
  path: string,
  pick: (range: { min: unknown; max: unknown }, path: string) => unknown,
): unknown {
  if (isRange(value)) return pick(value, path);
  if (Array.isArray(value))
    return value.map((item, i) => resolveRanges(item, `${path}${itemKey(item, i)}`, pick));
  if (isRecord(value)) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value))
      out[k] = resolveRanges(v, path ? `${path}.${k}` : k, pick);
    return out;
  }
  return value;
}

/**
 * Paths of ranges whose field is an integer (true) or a plain number (false), from the blueprint
 * and module schemas. Fields the schemas can't place are left out.
 */
export function rangeKinds(species: Species, registry: Registry): Map<string, boolean> {
  const kinds = new Map<string, boolean>();
  for (const gene of genesOf(species, registry))
    if (gene.kind === 'number') kinds.set(gene.path, gene.int === true);
  return kinds;
}

/**
 * Resolves every range in a species fragment, each from `rng`'s stream for its path. A number
 * range gives an integer where the field is one (`integer(path)`), or, for fields it doesn't
 * know, where both ends are integers.
 */
export function resolveSpecies<T>(
  species: T,
  rng: Rng,
  integer: (path: string) => boolean | undefined = () => undefined,
): T {
  return resolveRanges(cloneJson(species), '', ({ min, max }, path) => {
    const stream = rng.stream(path);
    if (typeof min === 'string' && typeof max === 'string')
      return blendColor(min, max, stream.next());
    const lo = Math.min(min as number, max as number);
    const hi = Math.max(min as number, max as number);
    if (integer(path) ?? (Number.isInteger(lo) && Number.isInteger(hi)))
      return stream.int(Math.ceil(lo), Math.floor(hi));
    return Number(stream.float(lo, hi).toPrecision(3));
  }) as T;
}

/**
 * One individual of a species: every range resolved for `seed`. With the registry, integer fields
 * get integers and number fields any value in between, whatever the ends look like.
 */
export function instantiate(
  species: Species,
  seed: number,
  registry?: Registry,
): Record<string, unknown> {
  const root = createRng(seed);
  const kinds = registry ? rangeKinds(species, registry) : undefined;
  const individual = resolveSpecies(species, root, (path) => kinds?.get(path));
  // Each individual gets its own seed too, so its patterns differ in their details.
  individual.seed = root.stream('individual').int(0, 4_294_967_295);
  if (typeof species.name === 'string') individual.name = `${species.name} #${seed}`;
  return toCurrentFormat(individual);
}

/**
 * Problems in a species: malformed ranges, and anything that makes an individual invalid. Every
 * range is checked at both ends (all minimums, then all maximums) and at a few seeds.
 */
export function validateSpecies(
  species: Species,
  registry: Registry,
  samples = 4,
): { ok: boolean; errors: Issue[]; warnings: Issue[]; checked: string[] } {
  const errors: Issue[] = [];
  const walk = (value: unknown, path: string) => {
    if (isRecord(value) && ('min' in value || 'max' in value) && !isRange(value)) {
      errors.push({
        severity: 'error',
        path,
        code: 'bad_range',
        message:
          'a range has exactly two keys, "min" and "max": two numbers, or two colours like "#5a5a5a"',
        fix: '{ "min": 0.5, "max": 0.7 } or { "min": "#5a5a5a", "max": "#9a9a9a" }',
      });
      return;
    }
    if (isRange(value) && typeof value.min === 'number' && value.min > (value.max as number))
      errors.push({
        severity: 'error',
        path,
        code: 'bad_range',
        message: `min ${value.min} is above max ${value.max}`,
        fix: `{ "min": ${value.max}, "max": ${value.min} }`,
      });
    if (Array.isArray(value))
      for (const [i, v] of value.entries()) walk(v, `${path}${itemKey(v, i)}`);
    else if (isRecord(value))
      for (const [k, v] of Object.entries(value)) walk(v, path ? `${path}.${k}` : k);
  };
  walk(species, '');
  if (errors.length > 0) return { ok: false, errors, warnings: [], checked: [] };

  const found = new Map<string, Issue>();
  const warnings = new Map<string, Issue>();
  const checked: string[] = [];
  const check = (individual: unknown, label: string) => {
    checked.push(label);
    const result = validateBlueprint(individual, registry, { minimal: false });
    for (const e of result.errors)
      if (!found.has(e.path + e.code))
        found.set(e.path + e.code, { ...e, message: `${e.message} (${label})` });
    for (const w of result.warnings)
      if (!warnings.has(w.path + w.code)) warnings.set(w.path + w.code, w);
  };
  check(
    resolveRanges(species, '', (r) => r.min),
    'with every range at its min',
  );
  check(
    resolveRanges(species, '', (r) => r.max),
    'with every range at its max',
  );
  for (let s = 1; s <= samples; s++)
    check(instantiate(species, s, registry), `individual seed ${s}`);
  return {
    ok: found.size === 0,
    errors: [...found.values()],
    warnings: [...warnings.values()],
    checked,
  };
}
