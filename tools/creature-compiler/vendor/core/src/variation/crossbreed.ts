import type { Issue } from '../blueprint/issues.ts';
import { cloneJson, isRecord } from '../blueprint/merge.ts';
import type { Registry } from '../registry.ts';
import { createRng, type Rng } from '../rng.ts';
import {
  blendColor,
  canonicalPath,
  expand,
  finish,
  gauss,
  genesOf,
  getAt,
  isLocked,
  setAt,
  tidy,
  type VariationResult,
} from './genes.ts';

type Json = Record<string, unknown>;

export interface CrossbreedOptions {
  /** Which child: the same parents and seed always give the same child. */
  readonly seed?: number;
  /** How much comes from the second parent, 0 to 1 (default 0.5). */
  readonly mix?: number;
  /**
   * The parent whose body plan and file the child is built on. By default a, or b with chance
   * `mix` when their body plans differ.
   */
  readonly base?: 'a' | 'b';
  /** Paths that keep the base parent's values, e.g. `body.torso`, `limbs`, `parts[id=tusks]`. */
  readonly locked?: readonly string[];
}

export interface CrossbreedResult extends VariationResult {
  /** The parent whose body plan and file the child is built on; `diff` is against it. */
  readonly base: 'a' | 'b';
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const items = (doc: Json, path: string): Json[] => {
  const list = getAt(doc, path);
  return Array.isArray(list) ? list.filter(isRecord) : [];
};

const on = (part: Json) => (isRecord(part.attach) ? part.attach.on : undefined);

/**
 * Pairs each list item of the base with its counterpart in the donor: limbs by id, else by role
 * and the nearest attachment point (forelegs with forelegs); parts by id, else by type; layers,
 * gaits and actions by type. Returns base item path → donor item path.
 */
function pairItems(base: Json, donor: Json): Map<string, string> {
  const pairs = new Map<string, string>();
  const donorLimbs = items(donor, 'limbs');
  for (const limb of items(base, 'limbs')) {
    const same = donorLimbs.find((d) => d.id === limb.id);
    const at = (x: Json) =>
      isRecord(x.attach) && typeof x.attach.at === 'number' ? x.attach.at : 0.5;
    const match =
      same ??
      donorLimbs
        .filter((d) => d.role === limb.role)
        .sort((x, y) => Math.abs(at(x) - at(limb)) - Math.abs(at(y) - at(limb)))[0];
    if (match) pairs.set(`limbs[id=${limb.id}]`, `limbs[id=${match.id}]`);
  }
  const used = new Set<unknown>();
  const donorParts = items(donor, 'parts');
  for (const part of items(base, 'parts')) {
    const match =
      donorParts.find((d) => d.id === part.id && d.type === part.type) ??
      // A part of the same type pairs only where it sits on the same section: a troll's tusks
      // on the jaw are not a beetle's horn on the head.
      donorParts.find((d) => d.type === part.type && !used.has(d) && on(d) === on(part));
    if (!match) continue;
    used.add(match);
    pairs.set(`parts[id=${part.id}]`, `parts[id=${match.id}]`);
  }
  for (const list of ['skin.layers', 'motion.gaits', 'motion.actions']) {
    const theirs = items(donor, list);
    items(base, list).forEach((item, i) => {
      const j = theirs.findIndex((d) => d.type === item.type);
      if (j >= 0) pairs.set(`${list}[${i}]`, `${list}[${j}]`);
    });
  }
  return pairs;
}

/** The donor's path for a base path, through the item pairs; undefined when unpaired. */
function counterpart(path: string, pairs: Map<string, string>): string | undefined {
  const item =
    /^(limbs\[id=[^\]]+\]|parts\[id=[^\]]+\]|(?:skin\.layers|motion\.gaits|motion\.actions)\[\d+\])/.exec(
      path,
    )?.[1];
  if (!item) return path;
  const other = pairs.get(item);
  return other === undefined ? undefined : other + path.slice(item.length);
}

const section = (doc: Json, name: string): Json | undefined =>
  isRecord(doc.body) && isRecord(doc.body[name]) ? doc.body[name] : undefined;

/**
 * Heads and tails: each section's count comes whole from one parent (the donor's with chance
 * `t`), with that parent's fan and, for tails, fork. A creature never gets two and a half heads.
 */
function inheritCounts(child: Json, donor: Json, t: number, locked: readonly string[], rng: Rng) {
  for (const [name, keys] of [
    ['neck', ['spread']],
    ['tail', ['spread', 'forkAt']],
  ] as const) {
    const mine = section(child, name);
    const theirs = section(donor, name);
    // One draw per section, made whatever happens, so locking one never moves the other.
    const take = rng.stream(name).chance(t);
    if (!mine || !theirs || !take || mine.count === theirs.count) continue;
    if (isLocked(`body.${name}.count`, locked)) continue;
    // A count of tails means nothing on a body without one.
    if (name === 'tail' && (mine.length === 0 || theirs.length === 0)) continue;
    mine.count = theirs.count;
    for (const key of keys)
      if (theirs[key] === undefined) delete mine[key];
      else mine[key] = cloneJson(theirs[key]);
  }
}

/** Roles a child may gain or lose by breeding; legs and arms are its body plan's. */
const BRED_ROLES = ['wing', 'fin', 'tentacle'] as const;

/**
 * Wings, fins and tentacles: pairs of one role match one to one (by id, then from front to back),
 * and how many the child has comes from one parent, the donor's with chance `t`. Taking the
 * donor's brings over its unmatched limbs of that role (a wolf gains a griffin's wings) or drops
 * the base's (a griffin loses them); matched limbs blend gene by gene as before.
 */
function inheritLimbs(child: Json, donor: Json, t: number, locked: readonly string[], rng: Rng) {
  const roleOf = (l: Json) => (typeof l.role === 'string' ? l.role : 'leg');
  const at = (l: Json) => (isRecord(l.attach) && typeof l.attach.at === 'number' ? l.attach.at : 0);
  let limbs = items(child, 'limbs');
  const ids = new Set([...limbs, ...items(child, 'parts')].map((x) => x.id));
  for (const role of BRED_ROLES) {
    const take = rng.stream(role).chance(t);
    if (!take || isLocked('limbs', locked)) continue;
    const mine = limbs.filter((l) => roleOf(l) === role);
    const theirs = items(donor, 'limbs').filter((l) => roleOf(l) === role);
    if (mine.length === theirs.length) continue;
    const matched = new Set<Json>();
    const unmatched = theirs.filter((d) => {
      const same = mine.find((l) => l.id === d.id && !matched.has(l));
      if (same) matched.add(same);
      return !same;
    });
    const rest = mine.filter((l) => !matched.has(l)).sort((x, y) => at(x) - at(y));
    unmatched.sort((x, y) => at(x) - at(y));
    // Front to back, as many as both have.
    const paired = Math.min(rest.length, unmatched.length);
    for (const limb of rest.slice(0, paired)) matched.add(limb);
    unmatched.splice(0, paired);
    limbs = limbs.filter(
      (l) => roleOf(l) !== role || matched.has(l) || isLocked(`limbs[id=${l.id}]`, locked),
    );
    for (const limb of unmatched) {
      let id = limb.id as string;
      for (let n = 2; ids.has(id); n++) id = `${limb.id}${n}`;
      ids.add(id);
      limbs.push({ ...cloneJson(limb), id });
    }
  }
  child.limbs = limbs;
}

/** Resamples a profile to `n` values. */
function resample(profile: readonly number[], n: number): number[] {
  if (profile.length === n) return [...profile];
  return Array.from({ length: n }, (_, i) => {
    const x = n === 1 ? 0 : (i / (n - 1)) * (profile.length - 1);
    const k = Math.min(profile.length - 2, Math.floor(x));
    if (k < 0) return profile[0] ?? 0;
    const a = profile[k] ?? 0;
    const b = profile[k + 1] ?? a;
    return a + (b - a) * (x - k);
  });
}

/**
 * A child of two parents. It keeps one parent's body plan (the second parent's with chance
 * `mix` when they differ), blends numbers and colours between matched genes, picks enums and
 * switches from either parent, and inherits each unmatched part, layer and action from the parent
 * that has it, by chance.
 */
export function crossbreed(
  a: Json,
  b: Json,
  options: CrossbreedOptions,
  registry: Registry,
): CrossbreedResult {
  const left = expand(a, registry);
  const right = expand(b, registry);
  if (!left.doc || !right.doc) {
    const errors: Issue[] = [
      ...left.errors.map((e) => ({ ...e, message: `parent a: ${e.message}` })),
      ...right.errors.map((e) => ({ ...e, message: `parent b: ${e.message}` })),
    ];
    return { ok: false, blueprint: a, diff: [], errors, warnings: [], base: 'a' };
  }
  const mix = clamp(options.mix ?? 0.5, 0, 1);
  const root = createRng(options.seed ?? 1).stream(
    `parents:${String(left.doc.seed ?? 0)}:${String(right.doc.seed ?? 0)}`,
  );
  const fromB =
    options.base !== undefined
      ? options.base === 'b'
      : left.doc.extends !== right.doc.extends && root.stream('plan').chance(mix);
  const [baseInput, baseDoc, donor] = fromB ? [b, right.doc, left.doc] : [a, left.doc, right.doc];
  // Share of each gene that comes from the donor.
  const t = fromB ? 1 - mix : mix;
  const child = cloneJson(baseDoc);
  const pairs = pairItems(baseDoc, donor);
  const locks = (options.locked ?? []).map((lock) => ({
    lock,
    path: canonicalPath(baseDoc, lock),
  }));
  const locked = locks.flatMap((l) => (l.path === undefined ? [] : [l.path]));

  for (const gene of genesOf(baseDoc, registry)) {
    if (isLocked(gene.path, locked)) continue;
    const where = counterpart(gene.path, pairs);
    const theirs = where === undefined ? undefined : getAt(donor, where);
    if (theirs === undefined) continue;
    const rng = root.stream(`gene:${gene.path}`);
    // Each gene's share wobbles around t, but not at the ends: mix 0 or 1 copies a parent.
    const share = clamp(t + gauss(rng) * 0.6 * t * (1 - t), 0, 1);
    const { min = Number.NEGATIVE_INFINITY, max = Number.POSITIVE_INFINITY } = gene;
    let value: unknown;
    if (gene.kind === 'number' && typeof theirs === 'number') {
      const v = gene.value as number;
      value = clamp(tidy(v + (theirs - v) * share, gene.int), min, max);
    } else if (gene.kind === 'profile' && (Array.isArray(theirs) || typeof theirs === 'number')) {
      const mine = gene.value as number[];
      const other = resample(Array.isArray(theirs) ? (theirs as number[]) : [theirs], mine.length);
      value = mine.map((v, i) => clamp(tidy(v + ((other[i] ?? v) - v) * share), min, max));
    } else if (gene.kind === 'color' && typeof theirs === 'string' && theirs.startsWith('#')) {
      value = blendColor(gene.value as string, theirs, share);
    } else if (
      (gene.kind === 'enum' || gene.kind === 'boolean') &&
      typeof theirs === typeof gene.value
    ) {
      value = rng.chance(t) ? theirs : gene.value;
    }
    if (value !== undefined) setAt(child, gene.path, value);
  }

  inheritCounts(child, donor, t, locked, root.stream('counts'));
  inheritLimbs(child, donor, t, locked, root.stream('limbs'));

  // Unmatched parts: the base's stay with chance 1 - t (sense organs always stay), the donor's
  // come over with chance t.
  const pairedDonor = new Set(pairs.values());
  const inherit = root.stream('inherit');
  const partsLocked = isLocked('parts', locked);
  const parts = items(child, 'parts').filter(
    (p) =>
      partsLocked ||
      isLocked(`parts[id=${p.id}]`, locked) ||
      pairs.has(`parts[id=${p.id}]`) ||
      registry.get('part', p.type as string)?.tags.includes('sense') ||
      inherit.chance(1 - t),
  );
  const ids = new Set(parts.map((p) => p.id));
  for (const part of partsLocked ? [] : items(donor, 'parts')) {
    if (pairedDonor.has(`parts[id=${part.id}]`) || !inherit.chance(t)) continue;
    if (registry.get('part', part.type as string)?.tags.includes('sense')) continue;
    let id = part.id as string;
    for (let n = 2; ids.has(id); n++) id = `${part.id}${n}`;
    ids.add(id);
    parts.push({ ...cloneJson(part), id });
  }
  child.parts = parts;
  // Layers and actions the donor has and the base lacks come over with chance t.
  for (const list of ['skin.layers', 'motion.actions']) {
    if (isLocked(list, locked)) continue;
    const mine = items(child, list);
    const kept = mine.filter((_, i) => pairs.has(`${list}[${i}]`) || inherit.chance(1 - t));
    const types = new Set(kept.map((x) => x.type));
    for (const item of items(donor, list))
      if (!types.has(item.type) && inherit.chance(t)) kept.push(cloneJson(item));
    if (kept.length > 0) setAt(child, list, kept);
  }

  child.seed = root.stream('seed').int(0, 2 ** 31 - 1);
  const names = [left.doc.name, right.doc.name].filter((n) => typeof n === 'string');
  if (names.length === 2) child.name = `${names[0]} × ${names[1]}`;
  const result = finish(baseInput, baseDoc, child, registry);
  const unused: Issue[] = locks
    .filter((l) => l.path === undefined)
    .map((l) => ({
      severity: 'warning' as const,
      path: l.lock,
      code: 'unknown_lock',
      message: 'nothing at this path in the base parent, so it locks nothing',
      fix: 'use a path like "body.torso", "limbs" or "parts[id=tusks]"',
    }));
  return { ...result, warnings: [...unused, ...result.warnings], base: fromB ? 'b' : 'a' };
}
