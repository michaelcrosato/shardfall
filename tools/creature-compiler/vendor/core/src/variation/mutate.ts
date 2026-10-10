import type { Issue } from '../blueprint/issues.ts';
import { cloneJson, isRecord } from '../blueprint/merge.ts';
import { MEDIA } from '../blueprint/schema.ts';
import { bodyMedia } from '../blueprint/validate.ts';
import type { Medium, PartModule, Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import {
  canonicalPath,
  expand,
  finish,
  type Gene,
  gauss,
  genesOf,
  getAt,
  isLocked,
  lightnessOf,
  nudgeColor,
  setAt,
  tidy,
  type VariationResult,
  withLightness,
} from './genes.ts';

type Json = Record<string, unknown>;

export interface MutateOptions {
  /** Which mutation: the same seed and parent always give the same child. */
  readonly seed?: number;
  /** How far to drift, 0 to 1: the share of genes that change and how much (default 0.3). */
  readonly amount?: number;
  /** Paths that never change, e.g. `scale`, `body.head`, `parts[id=horns]`, `skin`. */
  readonly locked?: readonly string[];
  /** Whether parts may be added, removed or swapped (default true). */
  readonly structure?: boolean;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** A new value for one gene, or undefined to leave it. */
function mutateGene(gene: Gene, rng: Rng, amount: number): unknown {
  const { min = Number.NEGATIVE_INFINITY, max = Number.POSITIVE_INFINITY } = gene;
  switch (gene.kind) {
    // Switches turn features on and off (a row's upper teeth, eyelids, cloven hooves): mutation
    // drifts what a creature has and never switches it off or on. Crossbreeding still takes
    // them from either parent.
    case 'boolean':
      return undefined;
    case 'enum': {
      if (!rng.chance(0.3)) return undefined;
      const others = (gene.options ?? []).filter((o) => o !== gene.value);
      return others.length > 0 ? rng.pick(others) : undefined;
    }
    case 'color':
      return nudgeColor(gene.value as string, rng, amount);
    case 'profile': {
      const common = Math.exp(gauss(rng) * 0.35 * amount);
      return (gene.value as number[]).map((v) =>
        tidy(clamp(v * common * Math.exp(gauss(rng) * 0.1 * amount), min, max)),
      );
    }
    case 'number': {
      const v = gene.value as number;
      // Zero usually means "none" (no tail, no ridges, no twist): nothing grows from nothing.
      if (v === 0) return undefined;
      if (gene.int) {
        const step = Math.max(1, Math.round(Math.abs(gauss(rng)) * amount * (max - min) * 0.1));
        return clamp(v + (rng.chance(0.5) ? step : -step), min, max);
      }
      const next =
        min >= 0
          ? v * Math.exp(gauss(rng) * 0.35 * amount)
          : v + gauss(rng) * 0.1 * amount * (max - min);
      return tidy(clamp(next, min, max));
    }
  }
}

/** Part modules a creature can gain or swap to in `parts` (feet and membranes belong to limbs). */
function placeable(registry: Registry): PartModule[] {
  return registry.list('part').filter((m) => m.slot !== 'foot' && m.slot !== 'membrane');
}

/** Sense organs are never added, removed, swapped out or swapped in. */
const isSense = (module: PartModule | undefined) => module?.tags.includes('sense') ?? false;

/** Where an expanded creature moves: its body's media, then its own `motion.media` switches. */
function mediaOf(doc: Json): Record<Medium, boolean> {
  const limbs = ((doc.limbs as unknown[] | undefined) ?? []).filter(isRecord).map((l) => ({
    role: typeof l.role === 'string' ? l.role : 'leg',
    attach: { on: isRecord(l.attach) && typeof l.attach.on === 'string' ? l.attach.on : 'torso' },
  }));
  const media = bodyMedia(limbs);
  const set = isRecord(doc.motion) && isRecord(doc.motion.media) ? doc.motion.media : {};
  for (const m of MEDIA) if (typeof set[m] === 'boolean') media[m] = set[m];
  return media;
}

/**
 * Foot modules a limb can wear instead of `current`: a shared tag, and for legs a foot that
 * stands them (one with a stance, or the pack's default leg foot), for arms one that does not.
 * Wings, fins and tentacles keep theirs.
 */
function footSwaps(registry: Registry, role: string, current: PartModule): PartModule[] {
  if (role !== 'leg' && role !== 'arm') return [];
  const standing = registry.defaults().foot?.leg;
  return registry
    .list('part')
    .filter(
      (m) =>
        m.slot === 'foot' &&
        m.id !== current.id &&
        m.tags.some((t) => current.tags.includes(t)) &&
        (role === 'leg' ? m.stance !== undefined || m.id === standing : m.stance === undefined),
    );
}

/** A module's example as a limb's inline `foot` object. */
function footOf(module: PartModule): Json {
  const { params, ...rest } = cloneJson(module.example);
  return { ...rest, ...(isRecord(params) ? params : {}), type: module.id };
}

/**
 * One structural change: adds a part whose tags match the creature's existing parts (a beak
 * only on a bird, fins only on a swimmer), removes one, swaps one for another module with the same slot and a shared tag,
 * or changes the feet of every limb of one role that wears one foot type. Limbs, heads and tails
 * are never added: those come from themes, edits and crossbreeding.
 */
function changeStructure(child: Json, rng: Rng, registry: Registry, locked: readonly string[]) {
  const parts = (child.parts as Json[] | undefined) ?? [];
  child.parts = parts;
  const moduleOf = (p: Json) => registry.get('part', p.type as string);
  // A part of a lineage (a beak, mandibles) joins only a creature of that lineage, one that
  // already wears such a part, foot or membrane; a part of a medium (fins) only one that moves
  // in it.
  const { lineage = [], habitat = {} } = registry.defaults();
  const media = mediaOf(child);
  const worn = new Set(
    [
      ...parts.map((p) => p.type),
      ...((child.limbs as unknown[] | undefined) ?? [])
        .filter(isRecord)
        .flatMap((l) => [l.foot, l.membrane].map((m) => (isRecord(m) ? m.type : undefined))),
    ].flatMap((type) => (typeof type === 'string' ? (registry.get('part', type)?.tags ?? []) : [])),
  );
  const fits = (m: PartModule) =>
    m.tags.every((t) => {
      const medium = habitat[t];
      if (medium !== undefined && media[medium]) return true;
      return !(lineage.includes(t) || medium !== undefined) || worn.has(t);
    });
  const free = parts.filter((p) => !isLocked(`parts[id=${p.id}]`, locked) && !isSense(moduleOf(p)));
  const tags = new Set(parts.flatMap((p) => moduleOf(p)?.tags ?? []));
  const present = new Set(parts.map((p) => p.type));
  const addable = placeable(registry).filter(
    (m) => !present.has(m.id) && !isSense(m) && fits(m) && m.tags.some((t) => tags.has(t)),
  );
  const swaps = (p: Json) => {
    const old = moduleOf(p);
    return placeable(registry).filter(
      (m) =>
        old &&
        m.id !== old.id &&
        m.slot === old.slot &&
        !isSense(m) &&
        fits(m) &&
        m.tags.some((t) => old.tags.includes(t)),
    );
  };
  const swappable = free.filter((p) => swaps(p).length > 0);
  // Feet change by group: every limb of one role wearing one foot type, so a wolf never ends up
  // with hooves in front and paws behind.
  const groups = new Map<string, { limbs: Json[]; swaps: PartModule[] }>();
  for (const limb of ((child.limbs as unknown[] | undefined) ?? []).filter(isRecord)) {
    const foot = isRecord(limb.foot) ? limb.foot : undefined;
    const module = foot ? registry.get('part', foot.type as string) : undefined;
    if (!module || isLocked(`limbs[id=${limb.id}].foot`, locked)) continue;
    const role = typeof limb.role === 'string' ? limb.role : 'leg';
    const key = `${role}:${module.id}`;
    const group = groups.get(key) ?? { limbs: [], swaps: footSwaps(registry, role, module) };
    group.limbs.push(limb);
    groups.set(key, group);
  }
  const feet = [...groups.values()].filter((g) => g.swaps.length > 0);
  const choices = [
    ...(addable.length > 0 ? ['add'] : []),
    ...(free.length > 0 ? ['remove'] : []),
    ...(swappable.length > 0 ? ['swap'] : []),
    ...(feet.length > 0 ? ['feet'] : []),
  ];
  if (choices.length === 0) return;
  const choice = rng.pick(choices);
  const ids = new Set(parts.map((p) => p.id));
  const freshId = (base: string) => {
    let id = base;
    for (let n = 2; ids.has(id); n++) id = `${base}${n}`;
    return id;
  };
  if (choice === 'add') {
    const module = rng.pick(addable);
    const item = cloneJson(module.example);
    item.id = freshId(typeof item.id === 'string' ? item.id : module.id.replace(/\W/g, '-'));
    parts.push(item);
  } else if (choice === 'remove') {
    const victim = rng.pick(free);
    child.parts = parts.filter((p) => p !== victim);
  } else if (choice === 'swap') {
    const target = rng.pick(swappable);
    const old = moduleOf(target);
    const module = rng.pick(swaps(target));
    const example = module.example;
    target.type = module.id;
    target.params = cloneJson(isRecord(example.params) ? example.params : {});
    // A part that sits elsewhere by default (a tail fin for a dorsal one) moves there.
    if (old?.attach.on !== module.attach.on) {
      if (isRecord(example.attach)) target.attach = cloneJson(example.attach);
      else delete target.attach;
    }
  } else {
    const group = rng.pick(feet);
    const module = rng.pick(group.swaps);
    for (const limb of group.limbs) limb.foot = footOf(module);
  }
}

/**
 * Patterns stay readable: each palette colour keeps at least the lightness difference from the
 * base it had in the parent (up to 0.12), on the same side, so a stripe never fades into the skin.
 */
function keepContrast(parent: Json, child: Json, locked: readonly string[]) {
  const before = (parent.skin as Json | undefined)?.palette as Record<string, string> | undefined;
  const after = (child.skin as Json | undefined)?.palette as Record<string, string> | undefined;
  if (!before?.base || !after?.base) return;
  const [lo, hi] = [0.04, 0.96];
  // Twice: moving the base for one colour can matter to another.
  for (let pass = 0; pass < 2; pass++)
    for (const name of Object.keys(after)) {
      const old = before[name];
      const color = after[name] as string;
      if (name === 'base' || !old || isLocked(`skin.palette.${name}`, locked)) continue;
      const was = lightnessOf(old) - lightnessOf(before.base);
      const side = Math.sign(was) || 1;
      const need = Math.min(Math.abs(was), 0.12);
      const base = lightnessOf(after.base);
      const now = lightnessOf(color) - base;
      if (Math.sign(now) === side && Math.abs(now) >= need - 0.005) continue;
      const target = base + side * need;
      if (target >= lo && target <= hi) after[name] = withLightness(color, target);
      else {
        // No room on that side of the base: put the colour at the edge and move the base.
        const edge = side > 0 ? hi : lo;
        after[name] = withLightness(color, edge);
        if (!isLocked('skin.palette.base', locked))
          after.base = withLightness(after.base, edge - side * need);
      }
    }
}

/**
 * A child of one parent: numbers drift within their ranges, colours shift, and now and then an
 * enum flips or a part is added, removed or swapped (parts with matching tags). Locked paths
 * never change. Each gene draws from its own seed stream, so locking one never reshuffles the
 * others.
 */
export function mutate(
  blueprint: Json,
  options: MutateOptions,
  registry: Registry,
): VariationResult {
  const { doc, errors } = expand(blueprint, registry);
  if (!doc) return { ok: false, blueprint, diff: [], errors, warnings: [] };
  const amount = clamp(options.amount ?? 0.3, 0, 1);
  // Locks in any form patch accepts, spelled the way gene paths are.
  const locks = (options.locked ?? []).map((lock) => ({ lock, path: canonicalPath(doc, lock) }));
  const locked = locks.flatMap((l) => (l.path === undefined ? [] : [l.path]));
  // Mixed with the parent's own seed, so one mutation seed sends different parents different ways.
  const root = createRng(options.seed ?? 1).stream(`parent:${String(doc.seed ?? 0)}`);
  const child = cloneJson(doc);
  // Gaits and actions the parent leaves to its body's defaults stay that way.
  const fixed = [...locked];
  for (const list of ['motion.gaits', 'motion.actions'])
    if (getAt(blueprint, list) === undefined) fixed.push(list);
  for (const gene of genesOf(doc, registry)) {
    // The skin's material is what the creature is made of: scales never drift into chitin.
    if (isLocked(gene.path, fixed) || gene.path === 'skin.material') continue;
    const rng = root.stream(`gene:${gene.path}`);
    if (!rng.chance(amount)) continue;
    const value = mutateGene(gene, rng, amount);
    if (value !== undefined) setAt(child, gene.path, value);
  }
  keepContrast(doc, child, locked);
  if (options.structure !== false && !isLocked('parts', locked)) {
    const rng = root.stream('structure');
    if (rng.chance(amount * 0.6)) changeStructure(child, rng, registry, locked);
  }
  const result = finish(blueprint, doc, child, registry);
  const unused: Issue[] = locks
    .filter((l) => l.path === undefined)
    .map((l) => ({
      severity: 'warning' as const,
      path: l.lock,
      code: 'unknown_lock',
      message: 'nothing at this locked path, so it locks nothing',
      fix: 'use a path like "body.head", "parts[id=horns]" or "skin.layers[type=stripes]"',
    }));
  return { ...result, warnings: [...unused, ...result.warnings] };
}
