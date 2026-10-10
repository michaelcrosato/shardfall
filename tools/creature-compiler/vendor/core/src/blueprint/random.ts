import { FORMAT } from '../format.ts';
import { paramsJsonSchema, type Registry } from '../registry.ts';
import type { Rng } from '../rng.ts';
import { blueprintSchemaFor } from './validate.ts';

type Json = Record<string, unknown>;
interface Schema {
  type?: string | string[];
  minimum?: number;
  maximum?: number;
  enum?: unknown[];
  const?: unknown;
  properties?: Record<string, Schema>;
  required?: string[];
  items?: Schema;
  minItems?: number;
  maxItems?: number;
  anyOf?: Schema[];
  oneOf?: Schema[];
  default?: unknown;
}

const hex = (rng: Rng) =>
  `#${[0, 1, 2].map(() => rng.int(0, 255).toString(16).padStart(2, '0')).join('')}`;

/** A random value that fits a JSON schema node (ranges, enums, unions, profiles). */
export function sampleSchema(schema: Schema | undefined, rng: Rng): unknown {
  if (!schema) return undefined;
  const options = schema.anyOf ?? schema.oneOf;
  if (options && options.length > 0) return sampleSchema(rng.pick(options), rng);
  if (schema.const !== undefined) return schema.const;
  if (schema.enum && schema.enum.length > 0) return rng.pick(schema.enum);
  const type = Array.isArray(schema.type) ? schema.type[0] : schema.type;
  switch (type) {
    case 'number':
    case 'integer': {
      const min = schema.minimum ?? 0;
      const max = schema.maximum ?? Math.max(1, min + 1);
      // Mostly mid-range, sometimes right at the ends, where bugs live.
      const roll = rng.next();
      const v = roll < 0.08 ? min : roll < 0.16 ? max : rng.float(min, max);
      return type === 'integer' ? Math.round(v) : Number(v.toPrecision(4));
    }
    case 'boolean':
      return rng.chance(0.5);
    case 'string':
      return hex(rng);
    case 'array': {
      const n = rng.int(schema.minItems ?? 1, Math.min(schema.maxItems ?? 4, 6));
      return Array.from({ length: n }, () => sampleSchema(schema.items, rng));
    }
    case 'object': {
      const out: Json = {};
      for (const [key, prop] of Object.entries(schema.properties ?? {})) {
        if (schema.required?.includes(key) || rng.chance(0.5)) out[key] = sampleSchema(prop, rng);
      }
      return out;
    }
    default:
      return schema.default;
  }
}

function at(schema: Schema | undefined, path: readonly string[]): Schema | undefined {
  let node = schema;
  for (const key of path) {
    if (!node) return undefined;
    const props =
      node.properties ??
      [...(node.anyOf ?? []), ...(node.oneOf ?? [])].find((s) => s.properties)?.properties;
    node = key === '[]' ? node.items : props?.[key];
  }
  return node;
}

/** Fields of a schema object, each kept with probability `p`. */
function some(schema: Schema | undefined, keys: readonly string[], rng: Rng, p = 0.5): Json {
  const out: Json = {};
  for (const key of keys) {
    if (rng.chance(p)) {
      const v = sampleSchema(at(schema, [key]), rng);
      if (v !== undefined) out[key] = v;
    }
  }
  return out;
}

/**
 * A random blueprint drawn from the schema and the registry's modules: any body plan (or none),
 * random proportions, extra limbs, parts with random parameters and anchors, random skin. Most
 * come out valid; the fuzz harness compiles the valid ones. Deterministic for a given `rng`.
 */
export function randomBlueprint(registry: Registry, rng: Rng, name = 'Random'): Json {
  const schema = paramsJsonSchema(blueprintSchemaFor(registry)) as Schema;
  const plan = rng.pick([...registry.ids('bodyPlan'), undefined]);
  const preset = plan ? ((registry.get('bodyPlan', plan) as { preset: Json }).preset ?? {}) : {};
  const body: Json = {};
  const fields: Record<string, string[]> = {
    torso: ['radius', 'arch', 'pitch', 'crossSection', 'segments'],
    neck: ['length', 'radius', 'pitch', 'crossSection', 'segments'],
    head: ['shape', 'length', 'radius', 'jaw', 'pitch', 'crossSection'],
    tail: ['length', 'radius', 'curl', 'curlStart', 'pitch', 'crossSection', 'segments'],
  };
  for (const [section, keys] of Object.entries(fields)) {
    const values = some(at(schema, ['body', section]), keys, rng, 0.35);
    if (Object.keys(values).length > 0) body[section] = values;
  }
  const blueprint: Json = {
    format: FORMAT,
    name,
    seed: rng.int(0, 1_000_000),
    ...(plan ? { extends: plan } : {}),
    scale: Number(rng.float(0.15, 4).toPrecision(3)),
    body,
  };

  // Limbs: tweak inherited ones, sometimes add a pair of legs or arms.
  const limbSchema = at(schema, ['limbs', '[]']);
  const limbs: Json[] = [];
  const presetLimbs = Array.isArray(preset.limbs) ? (preset.limbs as Json[]) : [];
  for (const limb of presetLimbs) {
    if (!rng.chance(0.5)) continue;
    // Each role takes its own fields (wings and fins have no splay).
    const role = typeof limb.role === 'string' ? limb.role : 'leg';
    const own = role === 'leg' || role === 'arm' ? ['splay', 'segments'] : [];
    limbs.push({ id: limb.id, ...some(limbSchema, ['length', 'radius', ...own], rng) });
  }
  const feet = registry.list('part').filter((p) => p.slot === 'foot');
  if (rng.chance(0.3)) {
    const role = rng.pick(['leg', 'arm'] as const);
    const foot = feet.length > 0 && rng.chance(0.7) ? rng.pick(feet) : undefined;
    limbs.push({
      id: `extra${role}`,
      role,
      attach: { on: 'torso', at: rng.float(0, 1), side: 'both' },
      length: rng.float(0.2, 1.5),
      segments: rng.int(2, 4),
      ...(role === 'leg' ? { splay: rng.float(0, 80) } : { lift: rng.float(0, 150) }),
      foot: foot
        ? {
            type: foot.id,
            ...(sampleSchema(paramsJsonSchema(foot.params) as Schema, rng) as Json),
          }
        : null,
    });
  }
  if (limbs.length > 0) blueprint.limbs = limbs;

  // Parts on sections that exist (a neck or tail of length 0 does not).
  const field = (section: string, key: string) =>
    (body[section] as Json | undefined)?.[key] ??
    ((preset.body as Json | undefined)?.[section] as Json | undefined)?.[key];
  const jaw = field('head', 'jaw') ?? false;
  const has = (section: string) => {
    const length = field(section, 'length');
    return typeof length === 'number' ? length > 0 : true;
  };
  const sections = [
    'head',
    'torso',
    ...(has('neck') ? ['neck'] : []),
    ...(has('tail') ? ['tail'] : []),
    ...(jaw ? ['jaw'] : []),
  ];
  const modules = registry
    .list('part')
    .filter((p) => p.slot !== 'foot' && p.slot !== 'membrane' && (p.slot !== 'mouth' || jaw));
  const parts: Json[] = [];
  const count = rng.int(0, 5);
  for (let i = 0; i < count && modules.length > 0; i++) {
    const module = rng.pick(modules);
    const on =
      module.slot === 'row'
        ? rng.pick([...sections.filter((s) => s !== 'jaw'), 'spine'])
        : rng.pick(sections);
    const attach: Json =
      module.slot === 'mouth'
        ? {}
        : module.slot === 'row'
          ? { on, from: rng.float(0, 0.5), to: rng.float(0.5, 1), angle: rng.pick([0, 0, 45, 90]) }
          : {
              on,
              at: rng.float(0, 1),
              angle: rng.float(0, 180),
              side: rng.pick(['both', 'center', 'left']),
            };
    parts.push({
      id: `part${i}`,
      type: module.id,
      ...(module.slot === 'mouth' ? {} : { attach }),
      params: sampleSchema(paramsJsonSchema(module.params) as Schema, rng),
    });
  }
  if (parts.length > 0) blueprint.parts = parts;

  // Skin.
  const patterns = registry.list('pattern');
  const regions = (at(schema, ['skin', 'layers', '[]', 'region'])?.enum as
    | string[]
    | undefined) ?? ['all'];
  blueprint.skin = {
    palette: { base: hex(rng), belly: hex(rng), accent: hex(rng) },
    ...(rng.chance(0.5) ? { material: sampleSchema(at(schema, ['skin', 'material']), rng) } : {}),
    layers: Array.from({ length: rng.int(0, 4) }, () => {
      const module = rng.pick(patterns);
      return {
        type: module.id,
        region: rng.pick(regions),
        strength: Number(rng.float(0, 1).toPrecision(3)),
        ...(sampleSchema(paramsJsonSchema(module.params) as Schema, rng) as Json),
      };
    }),
  };
  // Extra legs change which gaits fit the preset's list, so it becomes the slowest land gait
  // that works with any number of legs.
  const extraLegs = limbs.some((l) => l.role === 'leg');
  const anyLegs = registry
    .list('gait')
    .filter((g) => g.legPairs === 'any' && !g.planned && (g.medium ?? 'land') === 'land')
    .sort((a, b) => a.froude[0] - b.froude[0] || a.id.localeCompare(b.id))[0];
  blueprint.motion = {
    temperament: sampleSchema(at(schema, ['motion', 'temperament']), rng),
    ...(extraLegs && anyLegs ? { gaits: [anyLegs.id] } : {}),
  };
  return blueprint;
}
