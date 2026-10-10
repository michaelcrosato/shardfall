import type { CreatureSpec } from '../blueprint/creature.ts';
import type { PartModule, Registry } from '../registry.ts';
import type { Analysis } from './analyze.ts';

/** What a stats module sees: the creature's measured body, not its blueprint. */
export interface StatsInput {
  /** Metres and kilograms (see `Analysis.measurements`). */
  readonly measurements: Analysis['measurements'];
  /**
   * m/s: walking pace and top speed, top speed in the water for a swimmer, and cruising speed in
   * the air for a flyer.
   */
  readonly speed: {
    readonly walk: number;
    readonly max: number;
    readonly swim?: number;
    readonly fly?: number;
  };
  /** The main head's bite reach in metres, or null without a jaw. */
  readonly biteReach: number | null;
  /** Heads (each with its jaw, if the head has one). */
  readonly heads: number;
  readonly legs: number;
  readonly arms: number;
  /** Skin material: "skin", "scales" or "chitin". */
  readonly material: string;
  readonly temperament: string;
  readonly actions: readonly string[];
  /** Every part instance (a mirrored pair counts twice) with its module tags and size. */
  readonly parts: readonly {
    readonly type: string;
    readonly tags: readonly string[];
    /** The part's `length` or `height` parameter in metres, when it has one. */
    readonly size: number;
    /** How many pieces it has (a row's `count`, else 1). */
    readonly count: number;
    /**
     * The head it sits on (`head`, `head.L1`, …), through its neck, jaw or the part it sits on;
     * undefined off the heads. Parts on a head are copied to every head, so each head's weapons
     * appear once per head.
     */
    readonly head?: string;
  }[];
  /** Claws: toes per foot on legs and arms, and claw length in metres. */
  readonly claws: { readonly count: number; readonly length: number };
}

/** A stats module's hooks: one game's mapping from body to numbers. */
export interface StatsHooks {
  compute(input: StatsInput, params: Readonly<Record<string, unknown>>): Record<string, number>;
}

const num = (v: unknown, fallback = 0) => (typeof v === 'number' ? v : fallback);
const largest = (v: unknown) => (Array.isArray(v) ? Math.max(...v.map((x) => num(x))) : num(v));

/** Sections that belong to a head instance: `neck.L1`, `head.L1` and `jaw.L1` are head.L1's. */
const HEAD_SECTION = /^(?:neck|head|jaw)(\.[LR]\d+)?$/;

/** The head instance a part sits on, following parts on parts; undefined off the heads. */
function headOf(
  part: { readonly on: string },
  parts: readonly { readonly id: string; readonly on: string }[],
): string | undefined {
  let on = part.on;
  for (let depth = 0; depth < 8; depth++) {
    const host = parts.find((p) => p.id === on);
    if (!host) break;
    on = host.on;
  }
  const match = HEAD_SECTION.exec(on);
  return match ? `head${match[1] ?? ''}` : undefined;
}

/** The inputs a stats module sees, from a resolved creature and its analysis. */
export function statsInput(spec: CreatureSpec, analysis: Analysis, registry: Registry): StatsInput {
  const L = spec.scale;
  const legs = spec.limbs.filter((l) => l.role === 'leg');
  let clawCount = 0;
  let clawLength = 0;
  for (const limb of spec.limbs) {
    const foot = limb.foot;
    if (!foot) continue;
    // Each foot module says what claws it carries (a hoof none, a paw its toes').
    const hooks = (registry.get('part', foot.type) as PartModule | undefined)?.hooks;
    const claws = hooks?.claws?.(foot.params);
    if (!claws) continue;
    clawCount += claws.count;
    clawLength = Math.max(clawLength, claws.length * L);
  }
  return {
    measurements: analysis.measurements,
    speed: {
      walk: analysis.speed.walk,
      max: analysis.speed.max,
      ...(analysis.speed.swim !== undefined ? { swim: analysis.speed.swim } : {}),
      ...(analysis.speed.fly !== undefined ? { fly: analysis.speed.fly } : {}),
    },
    biteReach: analysis.reach.bite,
    heads: spec.body.neck.count,
    legs: legs.length,
    arms: spec.limbs.filter((l) => l.role === 'arm').length,
    material: spec.skin.material,
    temperament: spec.motion.temperament,
    actions: spec.motion.actions.map((a) => a.type),
    // What the part built, when it says (sizes may follow the head); else its parameters.
    parts: spec.parts.map((part) => {
      const built = analysis.parts[part.baseId];
      const head = headOf(part, spec.parts);
      return {
        type: part.type,
        tags: registry.get('part', part.type)?.tags ?? [],
        size: built
          ? built.size
          : largest(part.params.length ?? part.params.height ?? part.params.size) * L,
        count: built ? built.count : num(part.params.count, 1),
        ...(head ? { head } : {}),
      };
    }),
    claws: { count: clawCount, length: clawLength },
  };
}

/**
 * Game numbers for a creature from a stats module (one per game), computed from its measured
 * body, so form drives function: mass to health, legs to speed, weapons to attack.
 */
export function computeStats(
  spec: CreatureSpec,
  analysis: Analysis,
  registry: Registry,
  id: string,
  params: Readonly<Record<string, unknown>> = {},
): Record<string, number> {
  const module = registry.get('stats', id);
  if (!module)
    throw new Error(
      `no stats module "${id}"; use one of ${registry.ids('stats').join(', ') || 'none'}`,
    );
  const hooks = module.hooks as StatsHooks | undefined;
  if (!hooks) throw new Error(`stats module "${id}" has no compute hook`);
  const parsed = module.params.parse(params) as Record<string, unknown>;
  return hooks.compute(statsInput(spec, analysis, registry), parsed);
}
