import type { Issue } from '../blueprint/issues.ts';
import { cloneJson, isRecord, mergeBlueprint } from '../blueprint/merge.ts';
import { MEDIA } from '../blueprint/schema.ts';
import { bodyMedia, validateBlueprint } from '../blueprint/validate.ts';
import { buildSkeleton } from '../compile/skeleton.ts';
import { FORMAT } from '../format.ts';
import type { Feature, Medium, Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import { resolveSpecies, type Species } from './species.ts';

type Json = Record<string, unknown>;

/** A `{ min, max }` range, as in species. */
export interface Range {
  readonly min: number;
  readonly max: number;
}

/** A colour range: hue in degrees, saturation and lightness 0 to 1. */
export interface ColorRange {
  readonly hue: Range;
  readonly saturation: Range;
  readonly lightness: Range;
}

/**
 * What a theme biases. The core runs one grammar for every theme (body plan, then shape, parts,
 * skin and motion); a theme only weights the choices and narrows the ranges. Fragments are
 * species: blueprint pieces in which any number may be a `{ min, max }` range.
 */
export interface ThemeBias {
  /** Body plans to start from, by weight, each with its own shape (body sections, limbs). */
  readonly plans: Readonly<Record<string, { readonly weight: number; readonly shape?: Species }>>;
  /** Shape for every body plan (scale, body sections), under each plan's own shape. */
  readonly shape?: Species;
  /** Optional parts: a `parts[]` entry each, included with `chance`, on some plans only. */
  readonly parts?: readonly {
    readonly chance: number;
    readonly part: Species;
    readonly plans?: readonly string[];
  }[];
  /**
   * Optional limbs: each entry's `limbs[]` items join together with `chance`, on some plans only.
   * An item whose id the body already has changes that limb (hooves for paws on every leg).
   */
  readonly limbs?: readonly {
    readonly chance: number;
    readonly limbs: readonly Species[];
    readonly plans?: readonly string[];
  }[];
  /** Skin fields beside the palette, material and layers, such as fur. */
  readonly skin?: Species;
  /** Pattern layers: `always` first, then `count` picks from `options` by weight. */
  readonly layers?: {
    readonly always?: readonly Species[];
    readonly count: Range;
    readonly options: readonly { readonly weight: number; readonly layer: Species }[];
  };
  /** Palette colours. Belly and accent default to lighter and darker versions of the base. */
  readonly palette: {
    readonly base: ColorRange;
    readonly belly?: ColorRange;
    readonly accent?: ColorRange;
  };
  /** Skin materials by weight. */
  readonly materials?: Readonly<Record<string, number>>;
  /** Temperaments by weight. */
  readonly temperaments?: Readonly<Record<string, number>>;
  /** Name syllables: a start, then an end. */
  readonly names?: { readonly start: readonly string[]; readonly end: readonly string[] };
}

/** Limits a generated creature must meet. */
export interface GenerateConstraints {
  /** Use this body plan whatever the theme prefers. */
  readonly bodyPlan?: string;
  /** Body height in metres (top of the body, not counting horns or spikes). */
  readonly maxHeight?: number;
  readonly minHeight?: number;
  /** Actions the creature must be able to do, e.g. ["bite", "roar"]. */
  readonly actions?: readonly string[];
  /** Part types it must have, e.g. ["horn.curved"]. */
  readonly parts?: readonly string[];
  /**
   * Media it must move in, e.g. ["air"]: a body with wings for the air; a swimmer, or a walker
   * that swims, for water.
   */
  readonly requires?: readonly Medium[];
}

export interface GenerateOptions {
  /** Theme module id, e.g. "reptile". */
  readonly theme: string;
  readonly seed: number;
  readonly constraints?: GenerateConstraints;
}

export interface GenerateResult {
  readonly ok: boolean;
  /** The minimal blueprint. */
  readonly blueprint: Json;
  readonly errors: readonly Issue[];
  readonly warnings: readonly Issue[];
  /** Body size from the skeleton (metres), as the height constraints see it. */
  readonly measurements?: { readonly bodyHeight: number; readonly length: number };
  /** Tries it took to meet the constraints. */
  readonly attempts: number;
}

const ATTEMPTS = 12;

function weighted<T extends string>(rng: Rng, weights: Readonly<Record<T, number>>): T | undefined {
  const entries = (Object.entries(weights) as [T, number][]).filter(([, w]) => w > 0);
  const total = entries.reduce((s, [, w]) => s + w, 0);
  let x = rng.next() * total;
  for (const [key, w] of entries) {
    x -= w;
    if (x < 0) return key;
  }
  return entries.at(-1)?.[0];
}

function hsl(hue: number, s: number, l: number): string {
  const f = (n: number) => {
    const k = (n + hue / 30) % 12;
    const a = s * Math.min(l, 1 - l);
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return `#${[f(0), f(8), f(4)].map((c) => c.toString(16).padStart(2, '0')).join('')}`;
}

const pickRange = (rng: Rng, r: Range) => rng.float(Math.min(r.min, r.max), Math.max(r.min, r.max));
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Base, belly and accent colours with enough difference in lightness that patterns stay
 * readable: the belly at least 0.15 lighter than the base, the accent at least 0.15 away.
 */
function palette(rng: Rng, bias: ThemeBias['palette']): Record<string, string> {
  const draw = (range: ColorRange) => ({
    h: pickRange(rng, range.hue),
    s: pickRange(rng, range.saturation),
    l: pickRange(rng, range.lightness),
  });
  const base = draw(bias.base);
  const belly = bias.belly
    ? draw(bias.belly)
    : { h: base.h + rng.float(-10, 25), s: base.s * 0.6, l: base.l + rng.float(0.2, 0.35) };
  const accent = bias.accent
    ? draw(bias.accent)
    : { h: base.h + rng.float(-25, 25), s: base.s, l: base.l - rng.float(0.15, 0.3) };
  // The accent stays on the side of the base it was drawn on; when there is no room there, the
  // base moves instead.
  if (Math.abs(accent.l - base.l) < 0.15) {
    const dir = accent.l > base.l ? 1 : -1;
    const target = base.l + dir * 0.15;
    if (target >= 0.04 && target <= 0.95) accent.l = target;
    else base.l = accent.l - dir * 0.15;
  }
  belly.l = Math.max(belly.l, base.l + 0.15);
  const hex = (c: { h: number; s: number; l: number }) =>
    hsl(((c.h % 360) + 360) % 360, clamp(c.s, 0, 1), clamp(c.l, 0.04, 0.95));
  return { base: hex(base), belly: hex(belly), accent: hex(accent) };
}

/** Features each required action needs. */
function neededFeatures(registry: Registry, actions: readonly string[]): Set<Feature> {
  const out = new Set<Feature>();
  // Needs with alternatives (`['tail', 'tentacle']`) are met by any; generate asks for the first.
  for (const id of actions)
    for (const need of registry.get('action', id)?.needs ?? [])
      out.add(typeof need === 'string' ? need : (need[0] ?? ''));
  return out;
}

function planHas(registry: Registry, plan: string, role: 'leg' | 'arm'): boolean {
  const preset = registry.get('bodyPlan', plan)?.preset;
  const limbs = Array.isArray(preset?.limbs) ? preset.limbs : [];
  return limbs.some((l) => isRecord(l) && (l.role ?? 'leg') === role);
}

const limbList = (v: unknown): Json[] => (Array.isArray(v) ? v.filter(isRecord) : []);
const hasWing = (limbs: readonly unknown[]) => limbs.some((l) => isRecord(l) && l.role === 'wing');

/** A plan's limbs with a theme's shape on them (shape limbs change preset limbs by id). */
function planLimbs(registry: Registry, plan: string, bias: ThemeBias): Json[] {
  const preset = registry.get('bodyPlan', plan)?.preset ?? {};
  const shape = mergeBlueprint(
    (bias.shape ?? {}) as Json,
    (bias.plans[plan]?.shape ?? {}) as Json,
  ).merged;
  return limbList(
    mergeBlueprint({ limbs: limbList(preset.limbs) }, { limbs: limbList(shape.limbs) }).merged
      .limbs,
  );
}

/** Whether a theme can give a plan wings: its own, or among the theme's optional limbs. */
function canFly(registry: Registry, plan: string, bias: ThemeBias): boolean {
  return (
    hasWing(planLimbs(registry, plan, bias)) ||
    (bias.limbs ?? []).some((e) => (!e.plans || e.plans.includes(plan)) && hasWing(e.limbs))
  );
}

/** Where a set of limbs moves, as `bodyMedia` reads them. */
function limbMedia(limbs: readonly Json[]): Record<Medium, boolean> {
  return bodyMedia(
    limbs.map((l) => ({
      role: typeof l.role === 'string' ? l.role : 'leg',
      attach: { on: isRecord(l.attach) && typeof l.attach.on === 'string' ? l.attach.on : 'torso' },
    })),
  );
}

/** One draw of the grammar: plan, shape, limbs, parts, skin, motion. */
function draw(
  bias: ThemeBias,
  rng: Rng,
  registry: Registry,
  constraints: GenerateConstraints,
  needs: Set<Feature>,
): Json {
  const requires = constraints.requires ?? [];
  const plans = Object.fromEntries(
    Object.entries(bias.plans)
      .filter(([id]) => registry.get('bodyPlan', id))
      .filter(([id]) => !needs.has('arm') || planHas(registry, id, 'arm'))
      .filter(([id]) => !needs.has('legs') || planHas(registry, id, 'leg'))
      .filter(([id]) => !requires.includes('air') || canFly(registry, id, bias))
      .map(([id, p]) => [id, p.weight]),
  );
  const plan =
    constraints.bodyPlan ??
    weighted(rng.stream('plan'), plans) ??
    registry.defaults().bodyPlan ??
    registry.ids('bodyPlan')[0] ??
    '';
  const shape = mergeBlueprint(
    (bias.shape ?? {}) as Json,
    (bias.plans[plan]?.shape ?? {}) as Json,
  ).merged;
  const resolved = resolveSpecies(shape, rng.stream('shape'));

  // Optional limbs: a set joins with its chance, or because the creature must fly and has no
  // wings of its own yet.
  const limbRng = rng.stream('limbs');
  const extra: Json[] = [];
  let needWing = requires.includes('air') && !hasWing(planLimbs(registry, plan, bias));
  for (const [i, entry] of (bias.limbs ?? []).entries()) {
    if (entry.plans && !entry.plans.includes(plan)) continue;
    const hit = limbRng.stream(`chance:${i}`).chance(entry.chance);
    if (!hit && !(needWing && hasWing(entry.limbs))) continue;
    if (hasWing(entry.limbs)) needWing = false;
    entry.limbs.forEach((limb, j) => {
      extra.push(resolveSpecies(limb, limbRng.stream(`limb:${i}:${j}`)) as Json);
    });
  }
  const limbs =
    extra.length > 0
      ? limbList(mergeBlueprint({ limbs: limbList(resolved.limbs) }, { limbs: extra }).merged.limbs)
      : resolved.limbs;

  const partRng = rng.stream('parts');
  const parts: Json[] = Array.isArray(resolved.parts) ? (resolved.parts as Json[]) : [];
  for (const [i, entry] of (bias.parts ?? []).entries()) {
    if (entry.plans && !entry.plans.includes(plan)) continue;
    const required = constraints.parts?.includes((entry.part as Json).type as string) ?? false;
    if (!partRng.stream(`chance:${i}`).chance(entry.chance) && !required) continue;
    parts.push(resolveSpecies(entry.part, partRng.stream(`part:${i}`)) as Json);
  }
  for (const type of constraints.parts ?? []) {
    if (parts.some((p) => p.type === type)) continue;
    const module = registry.get('part', type);
    if (module && module.slot !== 'foot') parts.push(cloneJson(module.example));
  }

  const skinRng = rng.stream('skin');
  const layers: Json[] = (bias.layers?.always ?? []).map(
    (l, i) => resolveSpecies(l, skinRng.stream(`always:${i}`)) as Json,
  );
  if (bias.layers) {
    const pool = [...bias.layers.options];
    const count = Math.round(pickRange(skinRng.stream('count'), bias.layers.count));
    for (let i = 0; i < count && pool.length > 0; i++) {
      const weights = Object.fromEntries(pool.map((o, j) => [String(j), o.weight]));
      const j = Number(weighted(skinRng.stream(`pick:${i}`), weights) ?? 0);
      const [option] = pool.splice(j, 1);
      if (option) layers.push(resolveSpecies(option.layer, skinRng.stream(`layer:${i}`)) as Json);
    }
  }
  const material = bias.materials
    ? weighted(skinRng.stream('material'), bias.materials)
    : undefined;
  const skinExtra = bias.skin ? (resolveSpecies(bias.skin, skinRng.stream('extra')) as Json) : {};
  const temperament = bias.temperaments
    ? weighted(rng.stream('temperament'), bias.temperaments)
    : undefined;

  const names = bias.names;
  const nameRng = rng.stream('name');
  const name = names
    ? `${nameRng.pick(names.start)}${nameRng.pick(names.end)}`.replace(/^./, (c) => c.toUpperCase())
    : undefined;

  const body = (isRecord(resolved.body) ? resolved.body : {}) as Json;
  if (needs.has('jaw')) body.head = { ...(isRecord(body.head) ? body.head : {}), jaw: true };
  if (needs.has('tail')) {
    const tail = isRecord(body.tail) ? body.tail : {};
    if (typeof tail.length !== 'number' || tail.length <= 0) body.tail = { ...tail, length: 0.6 };
  }

  // Media the body does not give on its own: a walker that must swim swims too. Flying needs
  // wings, which only the plan and the optional limbs give.
  const shapeMotion = isRecord(resolved.motion) ? resolved.motion : {};
  const media: Json = { ...(isRecord(shapeMotion.media) ? shapeMotion.media : {}) };
  const preset = registry.get('bodyPlan', plan)?.preset ?? {};
  const allLimbs = limbList(
    mergeBlueprint({ limbs: limbList(preset.limbs) }, { limbs: limbList(limbs) }).merged.limbs,
  );
  const own = limbMedia(allLimbs);
  for (const medium of requires)
    if (medium !== 'air' && !own[medium] && media[medium] === undefined) media[medium] = true;
  const motion: Json = {
    ...shapeMotion,
    ...(temperament ? { temperament } : {}),
    ...(Object.keys(media).length > 0 ? { media } : {}),
  };
  return {
    format: FORMAT,
    ...(name ? { name } : {}),
    seed: rng.stream('seed').int(0, 2 ** 31 - 1),
    extends: plan,
    ...resolved,
    body,
    ...(limbList(limbs).length > 0 ? { limbs } : {}),
    ...(parts.length > 0 ? { parts } : {}),
    skin: {
      ...skinExtra,
      palette: palette(skinRng.stream('palette'), bias.palette),
      ...(material ? { material } : {}),
      ...(layers.length > 0 ? { layers } : {}),
    },
    ...(Object.keys(motion).length > 0 ? { motion } : {}),
  };
}

/** Drops parts that make a blueprint invalid; returns the validation of what is left. */
function repair(blueprint: Json, registry: Registry) {
  let result = validateBlueprint(blueprint, registry, { minimal: false });
  for (let round = 0; round < 4 && !result.ok; round++) {
    const bad = new Set(
      result.errors
        .map((e) => /^parts\[id=([^\]]+)\]/.exec(e.path)?.[1])
        .filter((id): id is string => id !== undefined),
    );
    if (bad.size === 0 || !Array.isArray(blueprint.parts)) break;
    blueprint.parts = blueprint.parts.filter((p) => !(isRecord(p) && bad.has(p.id as string)));
    result = validateBlueprint(blueprint, registry, { minimal: false });
  }
  return result;
}

/**
 * A new creature from a theme and a seed. The theme weights the body plan, shape, limbs, parts,
 * skin and temperament; constraints then fix the body plan, required parts, actions and media
 * (choosing a body that has what they need), and the height (by rescaling the whole creature).
 */
export function generate(options: GenerateOptions, registry: Registry): GenerateResult {
  const theme = registry.get('theme', options.theme);
  const constraints = options.constraints ?? {};
  const fail = (path: string, message: string, fix?: string): GenerateResult => ({
    ok: false,
    blueprint: {},
    errors: [
      { severity: 'error', path, code: 'cannot_generate', message, ...(fix ? { fix } : {}) },
    ],
    warnings: [],
    attempts: 0,
  });
  if (!theme?.bias)
    return fail(
      'theme',
      `no theme "${options.theme}"`,
      `use one of ${registry.ids('theme').join(', ')}`,
    );
  if (constraints.bodyPlan !== undefined && !registry.get('bodyPlan', constraints.bodyPlan))
    return fail(
      'constraints.bodyPlan',
      `no body plan "${constraints.bodyPlan}"`,
      `use one of ${registry.ids('bodyPlan').join(', ')}`,
    );
  for (const [i, id] of (constraints.actions ?? []).entries())
    if (!registry.get('action', id))
      return fail(
        `constraints.actions[${i}]`,
        `no action "${id}"`,
        `use one of ${registry.ids('action').join(', ')}`,
      );
  for (const [i, id] of (constraints.parts ?? []).entries())
    if (!registry.get('part', id))
      return fail(
        `constraints.parts[${i}]`,
        `no part "${id}"`,
        `use one of ${registry.ids('part').join(', ')}`,
      );
  const { minHeight = 0, maxHeight = Number.POSITIVE_INFINITY } = constraints;
  if (minHeight > maxHeight)
    return fail('constraints.minHeight', `minHeight ${minHeight} is above maxHeight ${maxHeight}`);
  for (const [i, medium] of (constraints.requires ?? []).entries())
    if (!(MEDIA as readonly string[]).includes(medium))
      return fail(`constraints.requires[${i}]`, `no medium "${medium}"`, `use ${MEDIA.join(', ')}`);
  if (constraints.requires?.includes('air')) {
    const flies = (bias: ThemeBias, plans: readonly string[]) =>
      plans.some((plan) => registry.get('bodyPlan', plan) && canFly(registry, plan, bias));
    const plans = constraints.bodyPlan ? [constraints.bodyPlan] : Object.keys(theme.bias.plans);
    if (!flies(theme.bias, plans)) {
      const fliers = registry
        .list('theme')
        .filter((t) => t.bias && flies(t.bias, Object.keys(t.bias.plans)))
        .map((t) => t.id);
      return fail(
        'constraints.requires',
        `no ${constraints.bodyPlan ?? options.theme} body in the ${options.theme} theme has wings to fly with`,
        fliers.length > 0
          ? `use a theme with wings: ${fliers.join(', ')}`
          : 'add wings to a creature with patch',
      );
    }
  }

  const needs = neededFeatures(registry, constraints.actions ?? []);
  const root = createRng(options.seed).stream(`theme:${options.theme}`);
  let last: Issue[] = [];
  for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
    const blueprint = draw(
      theme.bias,
      root.stream(`attempt:${attempt}`),
      registry,
      constraints,
      needs,
    );
    let result = repair(blueprint, registry);
    if (!result.ok || !result.creature) {
      last = [...result.errors];
      continue;
    }
    // Height scales with `scale`, so one rescale meets a height limit exactly.
    let size = measureBody(result.creature, registry);
    const target = clamp(size.height, minHeight, maxHeight);
    if (target !== size.height && size.height > 0) {
      const scale = blueprint.scale as number | undefined;
      const current = typeof scale === 'number' ? scale : result.creature.scale;
      blueprint.scale = Number(clamp((current * target) / size.height, 0.05, 20).toPrecision(4));
      result = repair(blueprint, registry);
      if (!result.ok || !result.creature) {
        last = [...result.errors];
        continue;
      }
      size = measureBody(result.creature, registry);
    }
    const has = new Set(result.creature.motion.actions.map((a) => a.type));
    const missing = (constraints.actions ?? []).filter((a) => !has.has(a));
    const missingParts = (constraints.parts ?? []).filter(
      (t) => !result.creature?.parts.some((p) => p.type === t),
    );
    const media = result.creature.motion.media;
    const unmet = (constraints.requires ?? []).filter((m) => !media[m]);
    if (missing.length > 0 || missingParts.length > 0 || unmet.length > 0) {
      last = [
        ...unmet.map((m) => ({
          severity: 'error' as const,
          path: 'constraints.requires',
          code: 'cannot_generate',
          message: `could not make a ${options.theme} creature that moves ${m === 'air' ? 'through the air' : m === 'water' ? 'in water' : 'on land'}`,
        })),
        ...missing.map((a) => ({
          severity: 'error' as const,
          path: 'constraints.actions',
          code: 'cannot_generate',
          message: `could not make a ${options.theme} creature that can "${a}"`,
        })),
        ...missingParts.map((t) => ({
          severity: 'error' as const,
          path: 'constraints.parts',
          code: 'cannot_generate',
          message: `could not fit a "${t}" on a ${options.theme} creature`,
        })),
      ];
      continue;
    }
    return {
      ok: true,
      blueprint: validateBlueprint(blueprint, registry).blueprint ?? blueprint,
      errors: [],
      warnings: result.warnings,
      measurements: {
        bodyHeight: Number(size.height.toFixed(3)),
        length: Number(size.length.toFixed(3)),
      },
      attempts: attempt + 1,
    };
  }
  return { ok: false, blueprint: {}, errors: last, warnings: [], attempts: ATTEMPTS };
}

/**
 * Height and length of the body (metres) from its skeleton: skin bones and their radii, not horns,
 * spikes, wings or fins. Cheap: no meshing.
 */
export function measureBody(
  spec: NonNullable<ReturnType<typeof validateBlueprint>['creature']>,
  registry: Registry,
) {
  const { bones } = buildSkeleton(spec, registry);
  let top = 0;
  let front = Number.NEGATIVE_INFINITY;
  let back = Number.POSITIVE_INFINITY;
  for (const b of bones) {
    // Wing and fin bones are built spread (the bind pose) and rest folded: not the body.
    if (!b.skin || b.tube) continue;
    top = Math.max(top, b.head.y + b.r0 * b.cross[1], b.tail.y + b.r1 * b.cross[1]);
    front = Math.max(front, b.head.z + b.r0, b.tail.z + b.r1);
    back = Math.min(back, b.head.z - b.r0, b.tail.z - b.r1);
  }
  return { height: top, length: Number.isFinite(front) ? front - back : 0 };
}
