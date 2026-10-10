import { z } from 'zod';
import type {
  ActionModule,
  GaitModule,
  Medium,
  ModuleKind,
  Need,
  PartModule,
  Registry,
} from '../registry.ts';
import { createRng } from '../rng.ts';
import { harmonyPalette, toHex } from './colors.ts';
import type {
  CreatureSpec,
  FootSpec,
  LayerSpec,
  LimbSpec,
  ModuleRefSpec,
  PartSpec,
  SideName,
} from './creature.ts';
import { headSpacing, instanceNames, instanceSuffixes } from './instances.ts';
import { formatPath, fromZodIssues, type Issue, type PathKey } from './issues.ts';
import { cloneJson, isRecord, mergeBlueprint } from './merge.ts';
import { migrate } from './migrate.ts';
import { isColorField, normalizeBlueprint, normalizeModules } from './normalize.ts';
import { notBuilt } from './planned.ts';
import {
  type BlueprintDoc,
  type BlueprintSchema,
  buildBlueprintSchema,
  DEFAULT_PALETTE,
  type LimbRoleName,
  MEDIA,
  REGIONS,
  type STANCES,
} from './schema.ts';
import { didYouMean } from './suggest.ts';

// --- Schema per registry ------------------------------------------------------------------------

const schemaCache = new WeakMap<Registry, BlueprintSchema>();

export function blueprintSchemaFor(registry: Registry): BlueprintSchema {
  let schema = schemaCache.get(registry);
  if (!schema) {
    const parts = registry.list('part');
    schema = buildBlueprintSchema(
      {
        bodyPlan: registry.ids('bodyPlan'),
        part: parts.filter((p) => p.slot !== 'foot' && p.slot !== 'membrane').map((p) => p.id),
        foot: parts.filter((p) => p.slot === 'foot').map((p) => p.id),
        membrane: parts.filter((p) => p.slot === 'membrane').map((p) => p.id),
        pattern: registry.ids('pattern'),
        gait: registry.ids('gait'),
        action: registry.ids('action'),
      },
      registry.defaults(),
    );
    schemaCache.set(registry, schema);
  }
  return schema;
}

// --- Resolved document --------------------------------------------------------------------------

type Params = Record<string, unknown>;

/** A module in a limb's slot (`foot`, `membrane`), with its parameters resolved. */
export interface SlotRef {
  type: string;
  params: Params;
}

/** One limb after defaults: every role's fields, with those its role lacks at their neutral values. */
export interface ResolvedLimb {
  id: string;
  role: LimbRoleName;
  attach: { on: string; at: number; side: 'both' | 'left' | 'right' | 'center'; angle: number };
  length: number;
  segments: number;
  radius: number | number[];
  muscle: number | undefined;
  splay: number;
  lift: number;
  stance: (typeof STANCES)[number] | undefined;
  curl: number;
  curlStart: number;
  foot: SlotRef | null;
  membrane: SlotRef | null;
}

/** A coat of fur, resolved. */
export interface ResolvedFur {
  length: number;
  density: number;
  region: (typeof REGIONS)[number][];
}

/** A blueprint after merging, defaults and module parameters, before mirroring. */
export interface ResolvedDoc extends Omit<BlueprintDoc, 'limbs' | 'parts' | 'skin' | 'motion'> {
  limbs: ResolvedLimb[];
  parts: (BlueprintDoc['parts'][number] & { params: Params })[];
  skin: {
    palette: Record<string, string>;
    material: BlueprintDoc['skin']['material'];
    fur: ResolvedFur | null;
    layers: {
      type: string;
      id: string | undefined;
      region: (typeof REGIONS)[number];
      strength: number;
      params: Params;
    }[];
  };
  motion: {
    temperament: BlueprintDoc['motion']['temperament'];
    gaits: ModuleRefSpec[];
    actions: ModuleRefSpec[];
    media: Record<Medium, boolean>;
  };
}

interface ResolveOutcome {
  /** The resolved document; when there are errors it is a best-effort repair, for further checks. */
  doc: ResolvedDoc | undefined;
  errors: Issue[];
  warnings: Issue[];
  /** Ids of items dropped while repairing (references to them are not reported again). */
  dropped: Set<string>;
  /** The blueprint merged with its preset, before defaults: what was written. */
  written?: Record<string, unknown>;
}

type Path = (string | number)[];

/** Deletes the value at `path` so its default applies; drops whole list items when ids are bad. */
function repairAt(doc: Record<string, unknown>, path: Path, dropped: Set<string>): boolean {
  if (path.length === 0) return false;
  // A bad id or type, or a bad item as a whole, drops the item from its list.
  const listKey = path[0];
  if ((listKey === 'limbs' || listKey === 'parts') && typeof path[1] === 'number') {
    const list = doc[listKey];
    if (Array.isArray(list) && (path.length === 2 || path[2] === 'id' || path[2] === 'type')) {
      const item = list[path[1]];
      if (isRecord(item) && typeof item.id === 'string') dropped.add(item.id);
      list.splice(path[1], 1);
      return true;
    }
  }
  // Inside a profile or other plain list, drop the whole list.
  let parentPath = path.slice(0, -1);
  let key = path[path.length - 1] as string | number;
  if (
    typeof key === 'number' &&
    parentPath.length > 0 &&
    !(parentPath.length === 1 && (parentPath[0] === 'limbs' || parentPath[0] === 'parts'))
  ) {
    const grand = parentPath.slice(0, -1);
    const parentKey = parentPath[parentPath.length - 1];
    const isLayers = parentKey === 'layers' || parentKey === 'gaits' || parentKey === 'actions';
    if (!isLayers) {
      key = parentKey as string | number;
      parentPath = grand;
    }
  }
  let node: unknown = doc;
  for (const k of parentPath)
    node =
      isRecord(node) || Array.isArray(node)
        ? (node as Record<string | number, unknown>)[k]
        : undefined;
  if (Array.isArray(node) && typeof key === 'number') {
    node.splice(key, 1);
    return true;
  }
  if (isRecord(node) && key in node) {
    delete node[key as string];
    return true;
  }
  return false;
}

const GENERIC_LAYER = {
  type: z.string(),
  id: z.string().optional(),
  region: z.enum(REGIONS).default('all'),
  strength: z.number().min(0).max(1).default(1),
};

function withGeneric(params: z.ZodType, generic: z.ZodRawShape): z.ZodType {
  const p = params as unknown as { extend?: (shape: z.ZodRawShape) => z.ZodType };
  return typeof p.extend === 'function' ? p.extend(generic) : params;
}

/** Merges, applies defaults and checks module parameters. No cross-reference checks. */
export function resolveDocument(input: unknown, registry: Registry): ResolveOutcome {
  const errors: Issue[] = [];
  const warnings: Issue[] = [];
  if (!isRecord(input)) {
    errors.push({
      severity: 'error',
      path: '',
      code: 'invalid_type',
      message: `a blueprint is a JSON object, got ${Array.isArray(input) ? 'a list' : typeof input}`,
    });
    return { doc: undefined, errors, warnings, dropped: new Set() };
  }

  const migrated = migrate(input);
  for (const issue of migrated.issues) (issue.severity === 'error' ? errors : warnings).push(issue);
  if (errors.length > 0) return { doc: undefined, errors, warnings, dropped: new Set() };
  const user = normalizeBlueprint(migrated.doc);

  let preset: Record<string, unknown> = {};
  if (user.extends !== undefined) {
    const plan =
      typeof user.extends === 'string' ? registry.get('bodyPlan', user.extends) : undefined;
    if (plan) preset = plan.preset;
  }
  const { merged, warnings: mergeWarnings, userIndex } = mergeBlueprint(preset, user);
  warnings.push(...mergeWarnings);
  normalizeModules(merged, registry);
  const pathOf = (path: readonly PathKey[]) =>
    formatPath(path, merged, (item) => (isRecord(item) ? userIndex.get(item) : undefined));

  const schema = blueprintSchemaFor(registry);
  let parsed = schema.safeParse(merged);
  const dropped = new Set<string>();
  // Repair a copy (drop each bad value so its default applies) so cross-reference checks can
  // still run and every problem is reported in one pass.
  let working = merged;
  if (!parsed.success) {
    errors.push(...fromZodIssues(parsed.error.issues, schema, merged, pathOf, registry.hints()));
    working = cloneJson(merged);
    for (let round = 0; round < 4 && !parsed.success; round++) {
      const paths = parsed.error.issues
        .flatMap((issue) => {
          const base = issue.path.map((k) => (typeof k === 'number' ? k : String(k)));
          const keys = (issue as { keys?: string[] }).keys;
          return issue.code === 'unrecognized_keys' && keys
            ? keys.map((k) => [...base, k])
            : [base];
        })
        // Deepest and last first, so list indexes stay valid while deleting.
        .sort((a, b) => b.length - a.length || JSON.stringify(b).localeCompare(JSON.stringify(a)));
      let changed = false;
      for (const path of paths) changed = repairAt(working, path, dropped) || changed;
      if (!changed) break;
      parsed = schema.safeParse(working);
    }
  }

  // Module parameters, checked against each module's own schema (on the raw merged values, so
  // structural and parameter errors are all reported at once).
  const moduleParams = (
    module: { params: z.ZodType } | undefined,
    value: unknown,
    generic: z.ZodRawShape | undefined,
    prefix: PathKey[],
  ): Params | undefined => {
    if (!module || !isRecord(value)) return undefined;
    const s = generic ? withGeneric(module.params, generic) : module.params;
    const result = s.safeParse(value);
    if (!result.success) {
      errors.push(
        ...fromZodIssues(
          result.error.issues,
          s,
          value,
          (p) =>
            formatPath([...prefix, ...p], working, (item) =>
              isRecord(item) ? userIndex.get(item) : undefined,
            ),
          registry.hints(),
        ),
      );
      // Drop just the offending fields so later checks still see the rest.
      const trimmed: Params = { ...(value as Params) };
      for (const issue of result.error.issues) {
        const keys = (issue as { keys?: string[] }).keys;
        for (const k of issue.code === 'unrecognized_keys' && keys ? keys : [issue.path[0]]) {
          if (typeof k === 'string' && k !== 'type') delete trimmed[k];
        }
      }
      const fallback = s.safeParse(trimmed);
      return fallback.success ? (fallback.data as Params) : undefined;
    }
    return result.data as Params;
  };

  const rawLimbs = Array.isArray(working.limbs) ? working.limbs : [];
  /** A limb's foot or membrane: its own, the pack's default for its role, or none. */
  const slotOf = (key: 'foot' | 'membrane') =>
    rawLimbs.map((limb, i): SlotRef | null | undefined => {
      if (!isRecord(limb)) return undefined;
      const role = (typeof limb.role === 'string' ? limb.role : 'leg') as LimbRoleName;
      const fallback = registry.defaults()[key]?.[role];
      const raw = limb[key] === undefined ? (fallback ? {} : null) : limb[key];
      if (raw === null) return null;
      const value = typeof raw === 'string' ? { type: raw } : raw;
      if (!isRecord(value)) return undefined;
      const type = typeof value.type === 'string' ? value.type : fallback;
      if (type === undefined) return undefined;
      const module = registry.get('part', type);
      const out = moduleParams(module, { ...value, type }, { type: z.string() }, ['limbs', i, key]);
      if (!out) return undefined;
      const { type: _t, ...params } = out;
      return { type, params };
    });
  const feet = slotOf('foot');
  const membranes = slotOf('membrane');

  const rawParts = Array.isArray(working.parts) ? working.parts : [];
  const partParams = rawParts.map((part, i) => {
    if (!isRecord(part) || typeof part.type !== 'string') return undefined;
    const module = registry.get('part', part.type);
    return moduleParams(module, part.params ?? {}, undefined, ['parts', i, 'params']);
  });

  const rawSkin = isRecord(working.skin) ? working.skin : {};
  const rawLayers = Array.isArray(rawSkin.layers) ? rawSkin.layers : undefined;
  const layerParams = (rawLayers ?? []).map((layer, i) => {
    if (!isRecord(layer) || typeof layer.type !== 'string') return undefined;
    const module = registry.get('pattern', layer.type);
    return moduleParams(module, layer, GENERIC_LAYER, ['skin', 'layers', i]);
  });

  const rawMotion = isRecord(working.motion) ? working.motion : {};
  const refParams = (kind: 'gait' | 'action', key: 'gaits' | 'actions') => {
    const list = Array.isArray(rawMotion[key]) ? (rawMotion[key] as unknown[]) : undefined;
    return list?.map((item, i): ModuleRefSpec | undefined => {
      const type =
        typeof item === 'string'
          ? item
          : isRecord(item) && typeof item.type === 'string'
            ? item.type
            : undefined;
      if (type === undefined) return undefined;
      const module = registry.get(kind, type);
      const out = moduleParams(
        module,
        typeof item === 'string' ? { type } : item,
        { type: z.string() },
        ['motion', key, i],
      );
      if (!out) return undefined;
      const { type: _t, ...params } = out;
      return { type, params };
    });
  };
  const gaits = refParams('gait', 'gaits');
  const actions = refParams('action', 'actions');

  if (!parsed.success) return { doc: undefined, errors, warnings, dropped };

  const doc = parsed.data;
  const palette: Record<string, string> = { ...DEFAULT_PALETTE };
  for (const [name, value] of Object.entries(doc.skin.palette)) {
    if (name === 'harmony') continue;
    if (typeof value === 'string') palette[name] = toHex(value) ?? value;
  }
  // A harmony fills in the base, belly and accent the blueprint itself leaves out (preset
  // colours give way), from the seed and around the blueprint's own base.
  const harmony = doc.skin.palette.harmony;
  if (harmony) {
    const own = (
      isRecord(user.skin) && isRecord(user.skin.palette) ? user.skin.palette : {}
    ) as Record<string, unknown>;
    const ownHex = (key: string) =>
      typeof own[key] === 'string' ? toHex(own[key] as string) : undefined;
    const rng = createRng(doc.seed).stream('palette');
    const made = harmonyPalette(harmony, () => rng.next(), ownHex('base'));
    for (const key of ['base', 'belly', 'accent'] as const) palette[key] = ownHex(key) ?? made[key];
  }
  // Defaults that came from the schema (not the merged input) still need their module params.
  const defaultLayers = rawLayers === undefined;
  const defaultActions = actions === undefined;
  const resolveDefaults = (kind: 'gait' | 'action', list: readonly (string | { type: string })[]) =>
    list.map((item) => {
      const type = typeof item === 'string' ? item : item.type;
      const module = registry.get(kind, type);
      const out = (module?.params.parse({}) ?? {}) as Params;
      return { type, params: out };
    });

  const limbs: ResolvedLimb[] = doc.limbs.map((limb, i) => {
    const own = limb as Partial<Record<'splay' | 'lift' | 'curl' | 'curlStart', number>> & {
      stance?: ResolvedLimb['stance'];
    };
    return {
      id: limb.id,
      role: limb.role,
      attach: limb.attach,
      length: limb.length,
      segments: limb.segments,
      radius: limb.radius,
      muscle: limb.muscle,
      splay: own.splay ?? 0,
      lift: own.lift ?? 0,
      stance: own.stance,
      curl: own.curl ?? 0,
      curlStart: own.curlStart ?? 0,
      foot: feet[i] ?? null,
      membrane: membranes[i] ?? null,
    };
  });
  const media = { ...bodyMedia(limbs) };
  for (const medium of MEDIA) {
    const set = doc.motion.media?.[medium];
    if (set !== undefined) media[medium] = set;
  }
  const features = bodyFeatures({ body: doc.body, limbs, parts: doc.parts }, registry);
  // A gait list replaces the defaults only for the media its gaits serve.
  const listed = (gaits?.filter(Boolean) ?? []) as ModuleRefSpec[];
  const covered = new Set(listed.map((g) => registry.get('gait', g.type)?.medium ?? 'land'));
  const defaultGaits =
    gaits === undefined
      ? suitableGaits(registry, {
          legs: limbs.filter((l) => l.role === 'leg').length,
          features,
          media,
        })
      : suitableGaits(registry, {
          legs: limbs.filter((l) => l.role === 'leg').length,
          features,
          media: Object.fromEntries(MEDIA.map((m) => [m, media[m] && !covered.has(m)])) as Record<
            Medium,
            boolean
          >,
        });
  const fur = doc.skin.fur
    ? {
        length: doc.skin.fur.length,
        density: doc.skin.fur.density,
        region:
          typeof doc.skin.fur.region === 'string'
            ? [doc.skin.fur.region]
            : [...doc.skin.fur.region],
      }
    : null;

  const resolved: ResolvedDoc = {
    ...doc,
    limbs,
    parts: doc.parts.map((part, i) => ({ ...part, params: partParams[i] ?? {} })),
    skin: {
      palette,
      material: doc.skin.material,
      fur,
      layers: doc.skin.layers.map((layer, i) => {
        const full = defaultLayers
          ? (withGeneric(
              registry.get('pattern', layer.type)?.params ?? z.object({}),
              GENERIC_LAYER,
            ).parse(layer) as Params)
          : (layerParams[i] ?? {});
        const { type, id, region, strength, ...params } = full;
        return {
          type: type as string,
          id: id as string | undefined,
          region: region as (typeof REGIONS)[number],
          strength: strength as number,
          params,
        };
      }),
    },
    motion: {
      temperament: doc.motion.temperament,
      gaits: [...listed, ...resolveDefaults('gait', defaultGaits)],
      actions: defaultActions
        ? resolveDefaults('action', suitableActions(registry, features))
        : (actions.filter(Boolean) as ModuleRefSpec[]),
      media,
    },
  };
  return { doc: resolved, errors, warnings, dropped, written: merged };
}

/** Where a body moves before its blueprint's own `motion.media` switches. */
export function bodyMedia(
  limbs: readonly { role: string; attach?: { on?: string } }[],
): Record<Medium, boolean> {
  const legs = limbs.some((l) => l.role === 'leg');
  // Fins or tentacles on the torso make a swimmer; on the head they are stalks or whiskers.
  const swims = limbs.some(
    (l) => (l.role === 'fin' || l.role === 'tentacle') && (l.attach?.on ?? 'torso') === 'torso',
  );
  return {
    land: legs || !swims,
    water: !legs && swims,
    air: limbs.some((l) => l.role === 'wing'),
  };
}

/**
 * What a body has, for the actions and gaits that need it: the core's features (`head`, `jaw`,
 * `arm`, `tail`, `legs`, `wing`, `fin`, `tentacle`) and every capability its modules provide.
 */
export function bodyFeatures(
  doc: {
    body: { head: { jaw: boolean }; tail: { length: number } };
    limbs: readonly {
      role: string;
      foot?: { type: string } | null;
      membrane?: { type: string } | null;
    }[];
    parts?: readonly { type: string }[];
    skin?: { layers?: readonly { type: string }[] };
  },
  registry?: Registry,
): Set<string> {
  const has = new Set<string>(['head']);
  if (doc.body.head.jaw) has.add('jaw');
  if (doc.body.tail.length > 0) has.add('tail');
  for (const limb of doc.limbs) has.add(limb.role === 'leg' ? 'legs' : limb.role);
  const provided = (kind: ModuleKind, type: string | undefined) => {
    for (const p of (type && registry?.get(kind, type)?.provides) || []) has.add(p);
  };
  for (const limb of doc.limbs) {
    provided('part', limb.foot?.type);
    provided('part', limb.membrane?.type);
  }
  for (const part of doc.parts ?? []) provided('part', part.type);
  for (const layer of doc.skin?.layers ?? []) provided('pattern', layer.type);
  return has;
}

/** Whether a body meets one need (a feature, or any of a list). */
export const meets = (has: ReadonlySet<string>, need: Need): boolean =>
  typeof need === 'string' ? has.has(need) : need.some((n) => has.has(n));

/** Says a need in words: "a jaw", "a tail or a tentacle". */
export const sayNeed = (need: Need): string =>
  (typeof need === 'string' ? [need] : [...need]).map((n) => `a ${n}`).join(' or ');

/** Actions whose needs a body meets, in id order. */
export function suitableActions(registry: Registry, has: ReadonlySet<string>): string[] {
  return registry
    .list('action')
    .filter((a) => !a.planned && a.needs.every((need) => meets(has, need)))
    .map((a) => a.id)
    .sort();
}

/** Whether a gait suits a body: land gaits by leg pairs, every gait by its needs. */
export function gaitSuits(
  g: GaitModule,
  body: { legs: number; features: ReadonlySet<string> },
): boolean {
  const land = (g.medium ?? 'land') === 'land';
  const pairs = !land || (g.legPairs === 'any' ? body.legs >= 1 : g.legPairs.includes(body.legs));
  return pairs && (g.needs ?? []).every((need) => meets(body.features, need));
}

/** Built gaits that suit a body in the media it uses, slowest first. */
export function suitableGaits(
  registry: Registry,
  body: { legs: number; features: ReadonlySet<string>; media: Record<Medium, boolean> },
): string[] {
  return registry
    .list('gait')
    .filter((g) => !g.planned && body.media[g.medium ?? 'land'] && gaitSuits(g, body))
    .sort((a, b) => a.froude[0] - b.froude[0] || a.id.localeCompare(b.id))
    .map((g) => g.id);
}

/** A parameter written beside a part's fields belongs in its `params`. */
function partParamHints(
  issues: Issue[],
  doc: Record<string, unknown>,
  registry: Registry,
): Issue[] {
  const parts = Array.isArray(doc.parts) ? doc.parts : [];
  // Foot parameters written on the limb itself belong in its `foot`.
  const footKeys = new Set(
    registry
      .list('part')
      .filter((p) => p.slot === 'foot')
      .flatMap((p) => Object.keys((p.params as unknown as { shape?: object }).shape ?? {})),
  );
  return issues.map((issue) => {
    const limb = /^limbs\[id=[^\]]+\]\.([A-Za-z0-9_]+)$/.exec(issue.path);
    if (issue.code === 'unknown_key' && limb && footKeys.has(limb[1] ?? ''))
      return {
        ...issue,
        fix: `move "${limb[1]}" into "foot", e.g. "foot": { "type": "${registry.defaults().foot?.leg ?? '…'}", "${limb[1]}": … }`,
      };
    const m = /^parts\[id=([^\]]+)\]\.([A-Za-z0-9_]+)$/.exec(issue.path);
    if (issue.code !== 'unknown_key' || !m) return issue;
    const part = parts.find((p) => isRecord(p) && p.id === m[1]);
    const type = isRecord(part) && typeof part.type === 'string' ? part.type : undefined;
    const module = type ? registry.get('part', type) : undefined;
    const keys = module
      ? Object.keys((module.params as unknown as { shape?: object }).shape ?? {})
      : [];
    return keys.includes(m[2] ?? '') ? { ...issue, fix: `move "${m[2]}" into "params"` } : issue;
  });
}

// --- Cross-reference checks ---------------------------------------------------------------------

const SECTION_NAMES = ['torso', 'neck', 'head', 'tail', 'jaw', 'spine'];

function semanticChecks(doc: ResolvedDoc, registry: Registry): Issue[] {
  const issues: Issue[] = [];
  const error = (path: string, code: string, message: string, extra: Partial<Issue> = {}) =>
    issues.push({ severity: 'error', path, code, message, ...extra });
  const warn = (path: string, code: string, message: string, extra: Partial<Issue> = {}) =>
    issues.push({ severity: 'warning', path, code, message, ...extra });

  const hasNeck = doc.body.neck.length > 0;
  const hasTail = doc.body.tail.length > 0;
  const sections = [
    'torso',
    'head',
    'spine',
    ...(hasNeck ? ['neck'] : []),
    ...(hasTail ? ['tail'] : []),
    ...(doc.body.head.jaw ? ['jaw'] : []),
  ];

  // Ids are unique across limbs and parts, and never shadow a body section.
  const seen = new Map<string, string>();
  const itemPath = (list: 'limbs' | 'parts', id: string) => `${list}[id=${id}]`;
  for (const [list, items] of [
    ['limbs', doc.limbs],
    ['parts', doc.parts],
  ] as const) {
    for (const item of items) {
      if (SECTION_NAMES.includes(item.id)) {
        error(`${itemPath(list, item.id)}.id`, 'reserved_id', `"${item.id}" names a body section`, {
          fix: `rename it, e.g. "${item.id}-${list === 'limbs' ? 'limb' : 'part'}"`,
        });
      }
      const other = seen.get(item.id);
      if (other) {
        error(
          `${itemPath(list, item.id)}.id`,
          'duplicate_id',
          `"${item.id}" is already used in ${other}`,
          {
            fix: 'give each limb and part its own id',
          },
        );
      }
      seen.set(item.id, list);
    }
  }

  // Limbs grow from body sections (any neck or tail; tentacles also from the head); legs come in
  // mirrored pairs.
  const necks = hasNeck ? instanceNames('neck', doc.body.neck.count) : [];
  const tails = hasTail ? instanceNames('tail', doc.body.tail.count) : [];
  let legCount = 0;
  for (const limb of doc.limbs) {
    const p = itemPath('limbs', limb.id);
    const limbSections = [
      'torso',
      ...necks,
      ...tails,
      ...(limb.role === 'tentacle' ? ['head'] : []),
    ];
    if (!limbSections.includes(limb.attach.on)) {
      const guess = didYouMean(limb.attach.on, limbSections);
      error(
        `${p}.attach.on`,
        'unknown_reference',
        `limbs attach to a body section, not "${limb.attach.on}"`,
        {
          expected: `one of ${limbSections.map((s) => `"${s}"`).join(', ')}`,
          fix: guess ? `did you mean "${guess}"?` : 'use "torso"',
        },
      );
    }
    if (limb.role === 'leg') {
      legCount++;
      if (limb.attach.side !== 'both') {
        error(
          `${p}.attach.side`,
          'unpaired_leg',
          `legs come in mirrored pairs, but this one is "${limb.attach.side}"`,
          {
            fix: 'set "side": "both"',
          },
        );
      }
    }
    for (const key of ['foot', 'membrane'] as const) {
      const slot = limb[key];
      const module = slot ? registry.get('part', slot.type) : undefined;
      if (slot && module && module.slot !== key) {
        error(`${p}.${key}.type`, 'wrong_slot', `"${slot.type}" is not a ${key} part`, {
          expected: registry
            .list('part')
            .filter((m) => m.slot === key)
            .map((m) => `"${m.id}"`)
            .join(', '),
          ...(module.slot === 'foot' || module.slot === 'membrane'
            ? { fix: `set it in "${module.slot}" instead` }
            : {}),
        });
      }
    }
  }
  if (legCount > 6) {
    error('limbs', 'too_many_legs', `${legCount} leg pairs; at most 6 are supported`);
  }

  // Several heads need room to sit side by side.
  const chest = (
    typeof doc.body.torso.radius === 'number'
      ? doc.body.torso.radius
      : (doc.body.torso.radius[0] ?? 0.15)
  ) as number;
  const spread = doc.body.neck.spread ?? Math.min(170, NECK_SPREAD * (doc.body.neck.count - 1));
  const spacing = headSpacing({
    neck: { count: doc.body.neck.count, length: doc.body.neck.length, spread },
    head: doc.body.head,
    chest,
  });
  if (spacing.gap < spacing.needed) {
    let wider = spread;
    while (wider < 170) {
      wider += 5;
      const s = headSpacing({
        neck: { count: doc.body.neck.count, length: doc.body.neck.length, spread: wider },
        head: doc.body.head,
        chest,
      });
      if (s.gap >= s.needed) break;
    }
    warn(
      'body.neck',
      'heads_overlap',
      `${doc.body.neck.count} heads ${(spacing.gap * 100).toFixed(0)}% of a torso length apart would overlap`,
      {
        fix:
          wider < 170
            ? `set "spread": ${wider} or more, or lengthen the necks`
            : 'lengthen the necks (body.neck.length) or make the heads smaller',
      },
    );
  }

  // Parts attach to sections (or one instance of several), limbs or other parts.
  const instances = [
    ...necks,
    ...(hasNeck || doc.body.neck.count > 1 ? instanceNames('head', doc.body.neck.count) : []),
    ...(doc.body.head.jaw ? instanceNames('jaw', doc.body.neck.count) : []),
    ...tails,
  ];
  const limbIds = new Set(doc.limbs.map((l) => l.id));
  const partIds = new Set(doc.parts.map((p) => p.id));
  for (const part of doc.parts) {
    const p = itemPath('parts', part.id);
    const module = registry.get('part', part.type);
    if (!module) continue;
    if (module.slot === 'foot') {
      error(
        `${p}.type`,
        'wrong_slot',
        `"${part.type}" is a foot part; set it in a limb's "foot" field instead`,
        {
          fix: `use { "foot": { "type": "${part.type}" } } on a limb`,
        },
      );
      continue;
    }
    if (module.slot === 'membrane') {
      error(
        `${p}.type`,
        'wrong_slot',
        `"${part.type}" is a membrane; set it in a wing or fin limb's "membrane" field instead`,
        { fix: `use { "role": "wing", "membrane": "${part.type}" } in "limbs"` },
      );
      continue;
    }
    if (part.attach.area !== undefined && module.slot !== 'area')
      error(`${p}.attach.area`, 'wrong_slot', `"${part.type}" sits at a point, not over an area`, {
        fix: 'remove "area"; place it with "at" and "angle"',
      });
    const on = part.attach.on ?? module.attach.on;
    const base = on.replace(/\.(L|R)$/, '');
    const known =
      sections.includes(on) ||
      instances.includes(on) ||
      limbIds.has(base) ||
      (partIds.has(base) && base !== part.id);
    if (!known) {
      const candidates = [...sections, ...limbIds, ...[...partIds].filter((id) => id !== part.id)];
      // Names models reach for that are parts of a section rather than sections.
      const snout = 'use "head" with a low "at" (the snout end is at 0)';
      const near: Record<string, string> = {
        snout,
        muzzle: snout,
        nose: snout,
        face: 'use "head"',
        skull: 'use "head"',
        mouth: 'use "jaw"',
        chin: 'use "jaw"',
        back: 'use "spine" (rows) or "torso", with "angle" 0',
        body: 'use "torso"',
        chest: 'use "torso" with a low "at"',
        hips: 'use "torso" with a high "at"',
      };
      const hint = near[on.toLowerCase()];
      const guess = hint ? undefined : didYouMean(on, candidates);
      const missingSection = on === 'neck' || on === 'tail' || on === 'jaw';
      error(
        `${p}.attach.on`,
        'unknown_reference',
        missingSection ? `this creature has no ${on}` : `nothing named "${on}" to attach to`,
        {
          expected: `a body section, limb id or part id: ${candidates.map((c) => `"${c}"`).join(', ')}`,
          fix: missingSection
            ? on === 'jaw'
              ? 'set body.head.jaw to true'
              : `give body.${on} a length above 0`
            : (hint ?? (guess ? `did you mean "${guess}"?` : undefined)),
        },
      );
    }
    if (module.slot === 'mouth') {
      if (!doc.body.head.jaw) {
        error(
          `${p}.type`,
          'needs_jaw',
          `"${part.type}" sits along the mouth, but the head has no jaw`,
          {
            fix: 'set body.head.jaw to true',
          },
        );
      }
    }
    if (module.slot === 'row' && part.attach.at !== undefined) {
      warn(`${p}.attach.at`, 'ignored', 'rows use "from" and "to"; "at" is ignored', {
        fix: 'remove "at"',
      });
    }
    if (module.slot === 'row') {
      const from = part.attach.from ?? module.attach.from ?? 0;
      const to = part.attach.to ?? module.attach.to ?? 1;
      if (from > to)
        warn(
          `${p}.attach`,
          'reversed_row',
          `"from" (${from}) is after "to" (${to}); the row runs backwards`,
        );
    }
    if (
      module.slot === 'surface' &&
      (part.attach.from !== undefined || part.attach.to !== undefined)
    ) {
      warn(
        `${p}.attach`,
        'ignored',
        `"${part.type}" sits at one point; "from" and "to" are ignored`,
        {
          fix: 'use "at" instead',
        },
      );
    }
    const angle = part.attach.angle ?? module.attach.angle ?? 0;
    if (part.attach.side === 'center' && angle !== 0 && angle !== 180) {
      warn(
        `${p}.attach.angle`,
        'off_midline',
        `centre parts sit on the midline, so angle ${angle} is treated as ${angle < 90 ? 0 : 180}`,
      );
    }
  }
  // Part-on-part chains must not loop.
  for (const part of doc.parts) {
    const chain = new Set<string>([part.id]);
    let cur: (typeof doc.parts)[number] | undefined = part;
    while (cur) {
      const on: string = (cur.attach.on ?? registry.get('part', cur.type)?.attach.on ?? '').replace(
        /\.(L|R)$/,
        '',
      );
      if (!partIds.has(on) || SECTION_NAMES.includes(on)) break;
      if (chain.has(on)) {
        error(
          `${itemPath('parts', part.id)}.attach.on`,
          'cycle',
          `parts attach to each other in a loop: ${[...chain, on].join(' → ')}`,
        );
        break;
      }
      chain.add(on);
      cur = doc.parts.find((x) => x.id === on);
    }
  }

  // Colour references in layers and part params name a palette colour or are colours themselves.
  const checkColors = (params: Params, path: string) => {
    for (const [key, value] of Object.entries(params)) {
      if (!isColorField(key) || typeof value !== 'string') continue;
      if (value in doc.skin.palette || toHex(value)) continue;
      const guess = didYouMean(value, Object.keys(doc.skin.palette));
      error(
        `${path}.${key}`,
        'unknown_color',
        `"${value}" is neither a palette name nor a colour`,
        {
          expected: `a palette name (${Object.keys(doc.skin.palette)
            .map((n) => `"${n}"`)
            .join(', ')}) or "#rrggbb"`,
          fix: guess ? `did you mean "${guess}"?` : 'add it to skin.palette',
        },
      );
    }
  };
  doc.skin.layers.forEach((layer, i) => {
    checkColors(layer.params, `skin.layers[${i}]`);
  });
  for (const part of doc.parts) checkColors(part.params, `${itemPath('parts', part.id)}.params`);

  // Gaits suit the body and its media; actions have the body features they need.
  const pairs = legCount;
  const features = bodyFeatures(doc, registry);
  const body = { legs: pairs, features, media: doc.motion.media };
  const compatible = suitableGaits(registry, body);
  doc.motion.gaits.forEach((ref, i) => {
    const gait = registry.get('gait', ref.type);
    if (!gait) return;
    const medium = gait.medium ?? 'land';
    if (!gaitSuits(gait, body)) {
      const land = medium === 'land';
      const missing = (gait.needs ?? []).find((need) => !meets(features, need));
      error(
        `motion.gaits[${i}]`,
        'gait_mismatch',
        land && !missing
          ? `"${ref.type}" does not suit ${pairs} leg pair${pairs === 1 ? '' : 's'}`
          : `"${ref.type}" needs ${sayNeed(missing ?? [])}`,
        {
          expected: compatible.map((c) => `"${c}"`).join(', ') || 'no gait suits this body',
          fix: compatible.length > 0 ? `use "${compatible[0]}"` : undefined,
        },
      );
    } else if (!doc.motion.media[medium]) {
      error(
        `motion.gaits[${i}]`,
        'medium_off',
        `"${ref.type}" moves it through ${medium === 'air' ? 'the air' : medium}, which motion.media turns off`,
        { fix: `set "motion": { "media": { "${medium}": true } }, or remove the gait` },
      );
    }
  });
  // Where it moves: flying needs wings, and it has to move somewhere.
  if (doc.motion.media.air && !features.has('wing'))
    error('motion.media.air', 'missing_feature', 'nothing to fly with', {
      fix: 'add a limb with "role": "wing", or set "air": false',
    });
  if (!MEDIA.some((m) => doc.motion.media[m]))
    error('motion.media', 'no_medium', 'every medium is off, so the creature cannot move', {
      fix: 'turn on at least one of land, water and air',
    });
  // Each gait or action is listed once; a repeat would be ignored.
  for (const key of ['gaits', 'actions'] as const) {
    const seen = new Set<string>();
    doc.motion[key].forEach((ref, i) => {
      if (seen.has(ref.type))
        warn(
          `motion.${key}[${i}]`,
          'duplicate',
          `"${ref.type}" is listed twice; only the first counts`,
          {
            fix: 'remove the repeat and put all its parameters on the first entry',
          },
        );
      seen.add(ref.type);
    });
  }
  if (doc.motion.gaits.length === 0 && compatible.length > 0) {
    warn('motion.gaits', 'no_gait', 'no gaits, so the creature cannot move', {
      fix: `add "${compatible[0]}"`,
    });
  }
  const fixFor: Record<string, string> = {
    jaw: 'set body.head.jaw to true',
    arm: 'add a limb with "role": "arm"',
    tail: 'give body.tail a length above 0',
    legs: 'add a limb with "role": "leg"',
    wing: 'add a limb with "role": "wing"',
    fin: 'add a limb with "role": "fin"',
    tentacle: 'add a limb with "role": "tentacle"',
  };
  doc.motion.actions.forEach((ref, i) => {
    const action = registry.get('action', ref.type) as ActionModule | undefined;
    for (const need of action?.needs ?? []) {
      if (!meets(features, need)) {
        const first = typeof need === 'string' ? need : need[0];
        const providers = registry
          .list()
          .filter((m) => first !== undefined && m.provides?.includes(first))
          .map((m) => `"${m.id}"`);
        error(`motion.actions[${i}]`, 'missing_feature', `"${ref.type}" needs ${sayNeed(need)}`, {
          fix:
            (first && fixFor[first]) ??
            (providers.length > 0
              ? `add a module that provides "${first}": ${providers.join(', ')}; or remove the action`
              : `remove the action: nothing provides "${first}"`),
        });
      }
    }
  });
  return issues;
}

// --- Expansion into a creature spec -------------------------------------------------------------

/** Degrees between neighbouring necks, and tails, when `spread` is left out. */
const NECK_SPREAD = 25;
const TAIL_SPREAD = 20;

const asProfile = (v: number | readonly number[]): number[] =>
  typeof v === 'number' ? [v] : [...v];

export function expandCreature(doc: ResolvedDoc, registry: Registry): CreatureSpec {
  const sides = (
    side: string,
    angle: number,
  ): { name: SideName; suffix: string; mirror: 1 | -1 | 0 }[] => {
    const resolved = side === 'both' && (angle === 0 || angle === 180) ? 'center' : side;
    if (resolved === 'both') {
      return [
        { name: 'left', suffix: '.L', mirror: 1 },
        { name: 'right', suffix: '.R', mirror: -1 },
      ];
    }
    if (resolved === 'left') return [{ name: 'left', suffix: '', mirror: 1 }];
    if (resolved === 'right') return [{ name: 'right', suffix: '', mirror: -1 }];
    return [{ name: 'center', suffix: '', mirror: 0 }];
  };

  // Legs are ordered back to front: the hindmost pair is pair 0.
  const legOrder = doc.limbs
    .filter((l) => l.role === 'leg')
    .map((l) => l.id)
    .sort((a, b) => {
      const la = doc.limbs.find((l) => l.id === a);
      const lb = doc.limbs.find((l) => l.id === b);
      return (lb?.attach.at ?? 0) - (la?.attach.at ?? 0) || a.localeCompare(b);
    });

  const limbs: LimbSpec[] = [];
  const limbInstances = new Map<string, LimbSpec[]>();
  for (const limb of doc.limbs) {
    const instances = sides(limb.attach.side, limb.attach.angle).map((s): LimbSpec => {
      const id = `${limb.id}${s.suffix}`;
      const foot: FootSpec | null = limb.foot
        ? { id: `${id}.foot`, type: limb.foot.type, params: limb.foot.params }
        : null;
      const membrane: FootSpec | null = limb.membrane
        ? { id: `${id}.membrane`, type: limb.membrane.type, params: limb.membrane.params }
        : null;
      const footModule = limb.foot ? registry.get('part', limb.foot.type) : undefined;
      return {
        id,
        baseId: limb.id,
        side: s.name,
        mirror: s.mirror,
        role: limb.role,
        on: limb.attach.on as LimbSpec['on'],
        at: limb.attach.at,
        angle: limb.attach.angle,
        length: limb.length,
        segments: limb.segments,
        radius: asProfile(limb.radius),
        splay: limb.splay,
        lift: limb.lift,
        muscle: limb.muscle ?? doc.body.muscle,
        stance: limb.role === 'leg' ? (limb.stance ?? footModule?.stance) : undefined,
        curl: limb.curl,
        curlStart: limb.curlStart,
        foot,
        membrane,
        pair: limb.role === 'leg' ? legOrder.indexOf(limb.id) : undefined,
      };
    });
    limbInstances.set(limb.id, instances);
    limbs.push(...instances);
  }

  const parts: PartSpec[] = [];
  const partInstances = new Map<string, PartSpec[]>();
  // Parts may attach to parts, so expand in dependency order.
  const pending = [...doc.parts];
  for (let guard = 0; pending.length > 0 && guard < 100; guard++) {
    for (let i = 0; i < pending.length; i++) {
      const part = pending[i];
      if (!part) continue;
      const module = registry.get('part', part.type) as PartModule;
      const on = part.attach.on ?? module.attach.on;
      const base = on.replace(/\.(L|R)$/, '');
      const explicitSide = on !== base;
      const targetIsPart = doc.parts.some((p) => p.id === base);
      if (targetIsPart && !partInstances.has(base)) continue;
      pending.splice(i--, 1);

      const angle = part.attach.angle ?? module.attach.angle ?? 0;
      const at = part.attach.at ?? module.attach.at ?? 0.5;
      const from = part.attach.from ?? module.attach.from ?? 0;
      const to = part.attach.to ?? module.attach.to ?? 1;
      const targets = limbInstances.get(base) ?? partInstances.get(base);
      let made: PartSpec[];
      if (targets && !explicitSide && targets.length > 1) {
        // On a mirrored limb or part: one copy per side, following the target.
        made = targets.map((t) => ({
          id: `${part.id}.${t.side === 'left' ? 'L' : 'R'}`,
          baseId: part.id,
          type: part.type,
          side: t.side,
          mirror: t.mirror,
          on: t.id,
          at,
          from,
          to,
          angle,
          area: part.attach.area ?? module.attach.area,
          params: part.params,
        }));
      } else {
        const side = part.attach.side ?? (angle === 0 || angle === 180 ? 'center' : 'both');
        const target = targets?.find((t) => t.id === on) ?? targets?.[0];
        made = sides(side, angle).map((s) => ({
          id: `${part.id}${s.suffix}`,
          baseId: part.id,
          type: part.type,
          side: s.name,
          mirror: s.mirror,
          on: target ? target.id : on,
          at,
          from,
          to,
          angle: s.name === 'center' ? (angle < 90 ? 0 : 180) : angle,
          area: part.attach.area ?? module.attach.area,
          params: part.params,
        }));
      }
      partInstances.set(part.id, made);
      parts.push(...made);
    }
  }

  // Several heads and tails (docs/design/9.1-heads-tails.md): a part on the head, jaw or neck, or
  // on such a part, gets a copy on every extra head; a part on the tail one on every extra tail,
  // except one wholly on a forked tail's trunk, placed once. A copy's id is the part's id plus
  // the instance, then its side (`horns.L1.L`), and it keeps the part's `baseId`, so every head
  // draws the same.
  const sectionOf = (part: PartSpec, depth = 0): string => {
    const parent = parts.find((p) => p.id === part.on);
    return parent && depth < 32 ? sectionOf(parent, depth + 1) : part.on;
  };
  const copyId = (part: PartSpec, suffix: string) =>
    `${part.baseId}${suffix}${part.id.slice(part.baseId.length)}`;
  const forkAt = doc.body.tail.count > 1 ? doc.body.tail.forkAt : 0;
  const onTrunk = (part: PartSpec, depth = 0): boolean => {
    const parent = parts.find((p) => p.id === part.on);
    if (parent && depth < 32) return onTrunk(parent, depth + 1);
    return (
      forkAt > 0 && (part.to <= forkAt || (part.from === 0 && part.to === 1 && part.at <= forkAt))
    );
  };
  const copies: PartSpec[] = [];
  for (const [sections, count] of [
    [['head', 'jaw', 'neck'], doc.body.neck.count],
    [['tail'], doc.body.tail.count],
  ] as const) {
    const extra = instanceSuffixes(count).filter((suffix) => suffix !== '');
    if (extra.length === 0) continue;
    const copied = parts.filter(
      (p) =>
        (sections as readonly string[]).includes(sectionOf(p)) &&
        !(sections[0] === 'tail' && onTrunk(p)),
    );
    const ids = new Set(copied.map((p) => p.id));
    for (const suffix of extra)
      for (const part of copied)
        copies.push({
          ...part,
          id: copyId(part, suffix),
          on: ids.has(part.on)
            ? copyId(copied.find((p) => p.id === part.on) as PartSpec, suffix)
            : `${part.on}${suffix}`,
        });
  }
  parts.push(...copies);

  const palette = doc.skin.palette;
  const resolveColors = (params: Params): Params => {
    const out: Params = {};
    for (const [k, v] of Object.entries(params)) {
      out[k] = isColorField(k) && typeof v === 'string' ? (palette[v] ?? toHex(v) ?? v) : v;
    }
    return out;
  };
  const layerIds = new Map<string, number>();
  const layers: LayerSpec[] = doc.skin.layers.map((layer) => {
    let id = layer.id;
    if (id === undefined) {
      const n = (layerIds.get(layer.type) ?? 0) + 1;
      layerIds.set(layer.type, n);
      id = n === 1 ? layer.type : `${layer.type}-${n}`;
    }
    return {
      id,
      type: layer.type,
      region: layer.region,
      strength: layer.strength,
      params: resolveColors(layer.params),
    };
  });

  return {
    format: doc.format,
    name: doc.name,
    seed: doc.seed,
    extends: doc.extends,
    scale: doc.scale,
    body: {
      muscle: doc.body.muscle,
      torso: { ...doc.body.torso, radius: asProfile(doc.body.torso.radius) },
      neck: {
        ...doc.body.neck,
        radius: asProfile(doc.body.neck.radius),
        spread: doc.body.neck.spread ?? Math.min(170, NECK_SPREAD * (doc.body.neck.count - 1)),
      },
      head: doc.body.head,
      tail: {
        ...doc.body.tail,
        radius: asProfile(doc.body.tail.radius),
        spread: doc.body.tail.spread ?? Math.min(170, TAIL_SPREAD * (doc.body.tail.count - 1)),
      },
    },
    limbs,
    parts: parts.map((p) => ({ ...p, params: resolveColors(p.params) })),
    skin: { palette, material: doc.skin.material, fur: doc.skin.fur, layers },
    motion: doc.motion,
  };
}

// --- Minimal blueprint --------------------------------------------------------------------------

function stableStringify(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`;
  if (isRecord(value)) {
    return `{${Object.keys(value)
      .filter((k) => value[k] !== undefined)
      .sort()
      .map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

const KEEP = new Set(['format', 'extends', 'name', 'seed']);

/**
 * The shortest blueprint that resolves to the same creature: drops every value that equals what
 * the preset or defaults would give anyway.
 */
export function minimalBlueprint(
  user: Record<string, unknown>,
  registry: Registry,
): Record<string, unknown> {
  const fingerprint = (doc: Record<string, unknown>) => {
    const r = resolveDocument(doc, registry);
    return r.doc ? stableStringify(r.doc) : undefined;
  };
  const target = fingerprint(user);
  if (target === undefined) return user;
  const current = normalizeBlueprint(migrate(user).doc);

  const tryRemove = (
    container: Record<string, unknown> | unknown[],
    key: string | number,
  ): boolean => {
    if (Array.isArray(container)) {
      const [removed] = container.splice(key as number, 1);
      if (fingerprint(current) === target) return true;
      container.splice(key as number, 0, removed);
      return false;
    }
    const saved = container[key as string];
    delete container[key as string];
    if (fingerprint(current) === target) return true;
    container[key as string] = saved;
    return false;
  };

  const visit = (node: Record<string, unknown>, path: string[]) => {
    for (const key of Object.keys(node)) {
      if (path.length === 0 && KEEP.has(key)) continue;
      if (key === 'id' || key === 'remove') continue;
      if (tryRemove(node, key)) continue;
      const value = node[key];
      const childPath = [...path, key];
      if (isRecord(value)) {
        visit(value, childPath);
        if (Object.keys(value).length === 0) tryRemove(node, key);
      } else if (
        Array.isArray(value) &&
        (childPath.join('.') === 'limbs' || childPath.join('.') === 'parts')
      ) {
        for (let i = value.length - 1; i >= 0; i--) {
          if (tryRemove(value, i)) continue;
          const item = value[i];
          if (isRecord(item)) visit(item, childPath);
        }
        if (value.length === 0) tryRemove(node, key);
      }
    }
  };
  visit(current, []);
  return current;
}

// --- Entry point --------------------------------------------------------------------------------

export interface ValidationResult {
  readonly ok: boolean;
  readonly errors: readonly Issue[];
  readonly warnings: readonly Issue[];
  /** The minimal blueprint: non-default values only, friendly forms normalized. */
  readonly blueprint?: Record<string, unknown>;
  /** The creature spec the compiler builds from (when there are no errors). */
  readonly creature?: CreatureSpec;
  /**
   * Things the blueprint uses that the format has but the pipeline does not build yet, each with
   * the plan milestone that builds it. Not errors or warnings: keep them.
   */
  readonly notBuilt?: readonly Issue[];
}

export interface ValidateOptions {
  /** Compute the minimal blueprint (default true). */
  readonly minimal?: boolean;
}

export function validateBlueprint(
  input: unknown,
  registry: Registry,
  options: ValidateOptions = {},
): ValidationResult {
  const outcome = resolveDocument(input, registry);
  const errors = isRecord(input)
    ? partParamHints(outcome.errors, mergedForHints(input, registry), registry)
    : [...outcome.errors];
  const warnings = [...outcome.warnings];
  if (!outcome.doc) return { ok: false, errors, warnings };
  const reported = new Set(errors.map((e) => e.path));
  for (const issue of semanticChecks(outcome.doc, registry)) {
    if (
      issue.code === 'unknown_reference' &&
      [...outcome.dropped].some((id) => issue.message.includes(`"${id}"`))
    )
      continue;
    if (issue.severity === 'error' && reported.has(issue.path)) continue;
    (issue.severity === 'error' ? errors : warnings).push(issue);
  }
  if (errors.length > 0) return { ok: false, errors, warnings };
  const creature = expandCreature(outcome.doc, registry);
  const planned = notBuilt(creature, registry);
  const blueprint =
    options.minimal === false
      ? undefined
      : minimalBlueprint(input as Record<string, unknown>, registry);
  return {
    ok: true,
    errors,
    warnings,
    ...(planned.length > 0 ? { notBuilt: planned } : {}),
    ...(blueprint ? { blueprint } : {}),
    creature,
  };
}

function mergedForHints(
  input: Record<string, unknown>,
  registry: Registry,
): Record<string, unknown> {
  const user = normalizeBlueprint(migrate(input).doc);
  const plan =
    typeof user.extends === 'string' ? registry.get('bodyPlan', user.extends) : undefined;
  return mergeBlueprint(plan?.preset ?? {}, user).merged;
}

/** Resolves a blueprint into a creature spec, throwing with every error if it is invalid. */
export function resolveBlueprint(input: unknown, registry: Registry): CreatureSpec {
  const result = validateBlueprint(input, registry, { minimal: false });
  if (!result.ok || !result.creature) {
    const lines = result.errors.map(
      (e) => `  ${e.path || '(root)'}: ${e.message}${e.fix ? ` — ${e.fix}` : ''}`,
    );
    throw new Error(`invalid blueprint:\n${lines.join('\n')}`);
  }
  return result.creature;
}
