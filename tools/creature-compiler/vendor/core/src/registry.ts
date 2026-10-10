import { z } from 'zod';
import type { StatsHooks } from './analysis/stats.ts';
import type { LimbRole } from './blueprint/creature.ts';
import type { PartHooks } from './compile/parts.ts';
import type { ActionHooks } from './motion/actions.ts';
import type { PatternHooks } from './shading/kit.ts';
import type { ThemeBias } from './variation/generate.ts';

/** The kinds of module the core can run. See docs/plan.md, "Modularity and variation". */
export const MODULE_KINDS = [
  'bodyPlan',
  'part',
  'pattern',
  'gait',
  'action',
  'theme',
  'stats',
] as const;
export type ModuleKind = (typeof MODULE_KINDS)[number];

/** Lowercase words joined by dots or dashes, e.g. `horn.curved`, `spikes.row`, `quadruped`. */
const MODULE_ID = /^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/;

/** What a module's `normalize` hook knows about where the item sits. */
export interface NormalizeContext {
  /** For parts: the anchor as written, with the module's default anchor filling the gaps. */
  readonly attach?: { readonly on: string; readonly at?: number; readonly angle?: number };
}

/** Fields every module declares. */
export interface ModuleBase<K extends ModuleKind, P extends z.ZodType = z.ZodType> {
  readonly kind: K;
  /** Unique within its kind. */
  readonly id: string;
  /** One sentence for the catalogue, written for an LLM reader. */
  readonly summary: string;
  readonly tags: readonly string[];
  /** Strict schema for the module's parameters, each with a default, range and description. */
  readonly params: P;
  /**
   * The plan milestone that builds this module, e.g. `"9.3"`. Until then the module is a stub:
   * blueprints may use it and it validates, but compile skips it and warns `not_built`, and it
   * never joins the default gaits or actions.
   */
  readonly planned?: string;
  /**
   * Capabilities the module gives a body that uses it, for the actions and gaits that need them:
   * `pincer` from a pincer hand, `display` from a frill, `hover` from insect wings. The core reads
   * them without knowing the modules behind them.
   */
  readonly provides?: readonly string[];
  /**
   * Turns friendly forms of the module's parameters into canonical ones before validation, as
   * blueprint normalization does for the core's fields: a horn's `aim` into `lean` and `turn`.
   * Gets the parameters as written (possibly invalid) and returns them rewritten; anything it
   * does not recognize it leaves for validation to report.
   */
  readonly normalize?: (
    params: Readonly<Record<string, unknown>>,
    context: NormalizeContext,
  ) => Record<string, unknown>;
}

/** Where a part sits. Each slot has its own placement rules (docs/blueprint.md). */
export type PartSlot =
  /** One point on a section, limb or part: `at` and `angle`. */
  | 'surface'
  /** A run of copies between `from` and `to`. */
  | 'row'
  /** The end of a limb, set through the limb's `foot` field. */
  | 'foot'
  /** Along the mouth line of a head with a jaw. */
  | 'mouth'
  /** The surface of a wing or fin, set through the limb's `membrane` field. */
  | 'membrane'
  /** Scattered over or fitted to an area of skin: `area`, and optionally `from` and `to`. */
  | 'area';

/** Base materials hard parts can use. */
export type PartMaterial = 'bone' | 'horn' | 'chitin' | 'enamel' | 'eye' | 'skin';

export interface PartModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'part', P> {
  readonly slot: PartSlot;
  readonly material: PartMaterial;
  /** Default anchor when a blueprint leaves `attach` fields out. */
  readonly attach: {
    readonly on: string;
    readonly at?: number;
    readonly angle?: number;
    readonly from?: number;
    readonly to?: number;
    /** For area-slot parts. */
    readonly area?: 'back' | 'belly' | 'sides' | 'all';
  };
  /** For foot-slot parts: the stance a leg takes when its blueprint leaves `stance` out. */
  readonly stance?: 'plantigrade' | 'digitigrade' | 'unguligrade';
  /** A complete `parts[]` entry (or `foot` object for foot parts) showing typical use. */
  readonly example: Record<string, unknown>;
  /** Geometry (and, for feet, toe bones). */
  readonly hooks?: PartHooks;
  /**
   * A short phrase for the creature's description, e.g. "coiled horns". `count` is how many
   * there are (2 for a mirrored pair), so a single horn can say "a curved horn"; `on` is what it
   * sits on (`head`, `jaw`, `tail`, a limb's id…), so a horn on the jaw can say "tusks".
   */
  readonly describe?: (
    params: Readonly<Record<string, unknown>>,
    info: { readonly count: number; readonly on?: string },
  ) => string;
}

export interface PatternModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'pattern', P> {
  /** A complete `skin.layers[]` entry showing typical use. */
  readonly example: Record<string, unknown>;
  /** The shader function, written once for CPU and GPU. */
  readonly hooks?: PatternHooks;
  /**
   * A short phrase for the creature's description, e.g. "dark stripes". The description adds
   * where it shows ("on the wings") when its `region` is not `all`.
   */
  readonly describe?: (
    params: Readonly<Record<string, unknown>>,
    info?: { readonly region: string },
  ) => string;
}

/** One leg, as a gait's `offsets` sees it. */
export interface GaitLeg {
  /** 0 is the hindmost pair. */
  readonly pair: number;
  readonly side: 'left' | 'right';
  /** Leg pairs on the body. */
  readonly pairs: number;
}

export interface GaitModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'gait', P> {
  /** Leg pairs the gait works with. `0` is a legless spine gait; `'any'` is one pair or more. */
  readonly legPairs: 'any' | readonly number[];
  /** Phase offset between successive leg pairs, counted from the back, for a given pair count. */
  readonly wave: (pairs: number) => number;
  /**
   * The phase at which each leg's foot lands, 0 to 1, in place of the wave: a gallop's lead, a
   * bound's pairs landing together. Gets the gait's resolved parameters.
   */
  readonly offsets?: (leg: GaitLeg, params: Readonly<Record<string, unknown>>) => number;
  /**
   * Default share of the cycle each foot is planted: one number, one per leg-pair count, or
   * `[slowest, fastest]` across the gait's Froude range (duty falls as an animal speeds up).
   */
  readonly duty: number | ((pairs: number) => number) | readonly [number, number];
  /** Speeds the gait suits, as Froude numbers v²/(g·h). */
  readonly froude: readonly [number, number];
  /** The Froude number it looks typical at (default the middle of its range, at most 1). */
  readonly natural?: number;
  /** Hip heights (m) the gait suits, such as a bound for small bodies (default any). */
  readonly hip?: readonly [number, number];
  /**
   * Postures the gait suits (default any): a gallop needs legs under the body, not a lizard's
   * sprawl.
   */
  readonly postures?: readonly ('upright' | 'sprawl')[];
  /** How far the spine flexes and extends each stride, 0 to 1 (default 0; a `flex` param wins). */
  readonly flex?: number;
  /** Where the gait moves the creature (default `land`). */
  readonly medium?: Medium;
  /**
   * For water gaits, what drives it (10.3): `body`, a wave down the body into the tail (fish,
   * eels, crocodiles; these dive); `legs`, legs paddling at the surface; `fins`, fin limbs
   * beating like a turtle's flippers (these dive too).
   */
  readonly swim?: 'body' | 'legs' | 'fins';
  /**
   * For air gaits, how it flies (10.4): `flapping`, powered strokes; `gliding`, wings held
   * spread; `hovering`, strokes in a horizontal plane holding it in place. The core picks among
   * them by role, never by id (docs/design/10.4-flight.md).
   */
  readonly air?: 'flapping' | 'gliding' | 'hovering';
  /** Body features or capabilities the gait needs, beside its leg pairs (see `Need`). */
  readonly needs?: readonly Need[];
  readonly hooks?: unknown;
}

/** Where a creature moves. */
export type Medium = 'land' | 'water' | 'air';

/**
 * What a body has, for the actions and gaits that need it: the core's features (`head`, `jaw`,
 * `arm`, `tail`, `legs`, `wing`, `fin`, `tentacle`) and the capabilities its modules provide.
 */
export type Feature =
  | 'head'
  | 'jaw'
  | 'arm'
  | 'tail'
  | 'legs'
  | 'wing'
  | 'fin'
  | 'tentacle'
  | (string & {});

/** One need: a feature, or a list meaning any of them (`['tail', 'tentacle']`). */
export type Need = Feature | readonly Feature[];

export interface ActionModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'action', P> {
  readonly needs: readonly Need[];
  /** Body-relative goals over time (see `ActionHooks`). */
  readonly hooks?: ActionHooks;
}

export interface BodyPlanModule extends ModuleBase<'bodyPlan', z.ZodType> {
  /** The blueprint fragment `extends` starts from. Lists carry ids so blueprints can edit them. */
  readonly preset: Record<string, unknown>;
}

export interface ThemeModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'theme', P> {
  /** Weights and ranges `generate` draws from (see `ThemeBias`). */
  readonly bias: ThemeBias;
}

export interface StatsModule<P extends z.ZodType = z.ZodType> extends ModuleBase<'stats', P> {
  /** Body to game numbers (see `StatsHooks`). */
  readonly hooks: StatsHooks;
  /** What each number means, by name, for the catalogue. */
  readonly outputs: Readonly<Record<string, string>>;
}

export interface ModuleByKind {
  bodyPlan: BodyPlanModule;
  part: PartModule;
  pattern: PatternModule;
  gait: GaitModule;
  action: ActionModule;
  theme: ThemeModule;
  stats: StatsModule;
}

export type ModuleDefinition = ModuleByKind[ModuleKind];

export function defineModule<const M extends ModuleDefinition>(module: M): M {
  return module;
}

const EMPTY_PARAMS = z.strictObject({});

export function definePart<P extends z.ZodType>(m: Omit<PartModule<P>, 'kind'>): PartModule<P> {
  return { kind: 'part', ...m };
}
export function definePattern<P extends z.ZodType>(
  m: Omit<PatternModule<P>, 'kind'>,
): PatternModule<P> {
  return { kind: 'pattern', ...m };
}
export function defineGait<P extends z.ZodType>(m: Omit<GaitModule<P>, 'kind'>): GaitModule<P> {
  return { kind: 'gait', ...m };
}
export function defineAction<P extends z.ZodType>(
  m: Omit<ActionModule<P>, 'kind'>,
): ActionModule<P> {
  return { kind: 'action', ...m };
}
export function defineBodyPlan(m: Omit<BodyPlanModule, 'kind' | 'params'>): BodyPlanModule {
  return { kind: 'bodyPlan', params: EMPTY_PARAMS, ...m };
}
export function defineTheme<P extends z.ZodType>(m: Omit<ThemeModule<P>, 'kind'>): ThemeModule<P> {
  return { kind: 'theme', ...m };
}
export function defineStats<P extends z.ZodType>(m: Omit<StatsModule<P>, 'kind'>): StatsModule<P> {
  return { kind: 'stats', ...m };
}

/**
 * What a pack offers when a blueprint leaves something out, so the core never names a module.
 * With several packs, the first pack that sets a default wins.
 */
export interface PackDefaults {
  /** Foot part each limb role gets when its blueprint leaves `foot` out. */
  readonly foot?: Readonly<Partial<Record<LimbRole, string>>>;
  /** Membrane each limb role gets when its blueprint leaves `membrane` out (wings and fins). */
  readonly membrane?: Readonly<Partial<Record<LimbRole, string>>>;
  /** Pattern layers a skin gets when its blueprint leaves `skin.layers` out. */
  readonly layers?: readonly Readonly<Record<string, unknown>>[];
  /** Body plan `generate` falls back on when no plan of a theme fits. */
  readonly bodyPlan?: string;
  /**
   * Part tags that mark a lineage, such as `bird` or `insect`: mutation adds or swaps in a part
   * with one only on a creature that already wears a part, foot or membrane with it.
   */
  readonly lineage?: readonly string[];
  /**
   * Part tags that tie a part to a medium, such as `aquatic` for water: mutation adds or swaps in
   * such a part only on a creature that moves in it (or already wears one).
   */
  readonly habitat?: Readonly<Record<string, Medium>>;
}

/** A named set of modules. Games include only the packs they want. */
export interface Pack {
  readonly id: string;
  readonly modules: readonly ModuleDefinition[];
  readonly defaults?: PackDefaults;
  /**
   * Fixes for guesses models make about this pack's vocabulary, which may name its modules:
   * `"limbs.hand"` for an unknown key, `"limbs.role:pincer"` for a value. They join the core's.
   */
  readonly hints?: Readonly<Record<string, string>>;
}

export function definePack<const P extends Pack>(pack: P): P {
  return pack;
}

/** One module as plain data, for `list_modules`, the JSON Schema and docs/catalog.md. */
export interface CatalogEntry {
  readonly kind: ModuleKind;
  readonly id: string;
  readonly pack: string;
  readonly summary: string;
  readonly tags: readonly string[];
  /** JSON Schema of the parameters as written: fields with defaults are optional. */
  readonly params: Record<string, unknown>;
  /** For stubs: the milestone that builds the module. */
  readonly planned?: string;
  /** Capabilities the module gives a body (`display`, `pincer`, …), which some actions need. */
  readonly provides?: readonly string[];
}

export interface Registry {
  get<K extends ModuleKind>(kind: K, id: string): ModuleByKind[K] | undefined;
  /** Modules sorted by kind, then id, so output is stable. */
  list<K extends ModuleKind>(kind: K): ModuleByKind[K][];
  list(): ModuleDefinition[];
  ids(kind: ModuleKind): string[];
  packOf(kind: ModuleKind, id: string): string | undefined;
  catalog(): CatalogEntry[];
  /** The packs' defaults, merged (the first pack to set one wins). */
  defaults(): PackDefaults;
  /** The packs' hints for validation errors, merged (the first pack wins). */
  hints(): Readonly<Record<string, string>>;
}

export function paramsJsonSchema(params: z.ZodType): Record<string, unknown> {
  const schema = z.toJSONSchema(params, { io: 'input', unrepresentable: 'any' }) as Record<
    string,
    unknown
  >;
  delete schema.$schema;
  return schema;
}

export function createRegistry(packs: readonly Pack[]): Registry {
  const byKey = new Map<string, { module: ModuleDefinition; pack: string }>();
  const key = (kind: ModuleKind, id: string) => `${kind}:${id}`;

  for (const pack of packs) {
    for (const module of pack.modules) {
      if (!MODULE_KINDS.includes(module.kind)) {
        throw new Error(
          `${pack.id}/${module.id}: unknown module kind "${module.kind}"; expected one of ${MODULE_KINDS.join(', ')}`,
        );
      }
      if (!MODULE_ID.test(module.id)) {
        throw new Error(
          `${pack.id}/${module.id}: module ids are lowercase words joined by "." or "-", e.g. "horn.curved"`,
        );
      }
      const existing = byKey.get(key(module.kind, module.id));
      if (existing) {
        throw new Error(
          `${module.kind} "${module.id}" is defined in both pack "${existing.pack}" and pack "${pack.id}"`,
        );
      }
      byKey.set(key(module.kind, module.id), { module, pack: pack.id });
    }
  }

  const defaults: { -readonly [K in keyof PackDefaults]: PackDefaults[K] } = {};
  for (const pack of packs) {
    for (const [name, value] of Object.entries(pack.defaults ?? {}) as [
      keyof PackDefaults,
      never,
    ][]) {
      if (value !== undefined && defaults[name] === undefined) defaults[name] = value;
    }
  }
  const kindOf: Partial<Record<keyof PackDefaults, ModuleKind>> = {
    foot: 'part',
    membrane: 'part',
    layers: 'pattern',
    bodyPlan: 'bodyPlan',
  };
  for (const [name, kind] of Object.entries(kindOf) as [keyof PackDefaults, ModuleKind][]) {
    const value = defaults[name];
    const ids =
      typeof value === 'string'
        ? [value]
        : Array.isArray(value)
          ? value.map((l) => (l as { type?: unknown }).type)
          : Object.values(value ?? {});
    for (const id of ids) {
      if (typeof id !== 'string' || !byKey.has(key(kind, id)))
        throw new Error(`default ${name} names ${kind} "${String(id)}", which no pack defines`);
    }
  }

  const hints: Record<string, string> = {};
  for (const pack of [...packs].reverse()) Object.assign(hints, pack.hints ?? {});

  const sorted = [...byKey.values()].sort(
    (x, y) =>
      MODULE_KINDS.indexOf(x.module.kind) - MODULE_KINDS.indexOf(y.module.kind) ||
      x.module.id.localeCompare(y.module.id),
  );

  function list(kind?: ModuleKind): ModuleDefinition[] {
    return sorted.map((e) => e.module).filter((m) => kind === undefined || m.kind === kind);
  }

  return {
    get: <K extends ModuleKind>(kind: K, id: string) =>
      byKey.get(key(kind, id))?.module as ModuleByKind[K] | undefined,
    list: list as Registry['list'],
    ids: (kind) => list(kind).map((m) => m.id),
    packOf: (kind, id) => byKey.get(key(kind, id))?.pack,
    catalog: () =>
      sorted.map(({ module, pack }) => ({
        kind: module.kind,
        id: module.id,
        pack,
        summary: module.summary,
        tags: module.tags,
        params: paramsJsonSchema(module.params),
        ...(module.planned ? { planned: module.planned } : {}),
        ...(module.provides?.length ? { provides: module.provides } : {}),
      })),
    defaults: () => defaults,
    hints: () => hints,
  };
}
