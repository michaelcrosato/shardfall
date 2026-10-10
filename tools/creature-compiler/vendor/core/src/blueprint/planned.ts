import type { ModuleKind, Registry } from '../registry.ts';
import type { CreatureSpec, LimbRole } from './creature.ts';
import type { Issue } from './issues.ts';

/**
 * Vocabulary may be in the format before it is built: a blueprint may use it, it validates, and
 * compile skips what it cannot draw yet. This lists what is skipped, naming the plan milestone
 * that builds each thing: modules marked `planned` and limbs of roles not built. Format 0.2's
 * vocabulary is all built since 10.4; packs may still add planned modules.
 */
export function notBuilt(spec: CreatureSpec, registry: Registry): Issue[] {
  const issues: Issue[] = [];
  const seen = new Set<string>();
  const add = (path: string, what: string, milestone: string) => {
    if (seen.has(path)) return;
    seen.add(path);
    issues.push({
      severity: 'warning',
      path,
      code: 'not_built',
      message: `${what} is in the format but not built yet (plan milestone ${milestone}), so it is left out for now`,
      fix: 'keep it: it validates, and it appears once that milestone lands',
    });
  };
  const planned = (kind: ModuleKind, type: string) => registry.get(kind, type)?.planned;

  // Modules that are stubs.
  for (const part of spec.parts) {
    const milestone = planned('part', part.type);
    if (milestone) add(`parts[id=${part.baseId}].type`, `"${part.type}"`, milestone);
  }
  for (const limb of spec.limbs) {
    for (const key of ['foot', 'membrane'] as const) {
      const slot = limb[key];
      const milestone = slot && planned('part', slot.type);
      if (slot && milestone)
        add(`limbs[id=${limb.baseId}].${key}.type`, `"${slot.type}"`, milestone);
    }
  }
  spec.skin.layers.forEach((layer, i) => {
    const milestone = planned('pattern', layer.type);
    if (milestone) add(`skin.layers[${i}].type`, `"${layer.type}"`, milestone);
  });
  for (const [kind, key] of [
    ['gait', 'gaits'],
    ['action', 'actions'],
  ] as const) {
    for (const ref of spec.motion[key]) {
      const milestone = planned(kind, ref.type);
      if (milestone) add(`motion.${key}[type=${ref.type}]`, `"${ref.type}"`, milestone);
    }
  }

  // Every limb role is built since 9.4, and so is what sits on each.

  return issues;
}

/** Limb roles the pipeline builds: all of format 0.2's since 9.4. */
const BUILT_ROLES: readonly LimbRole[] = ['leg', 'arm', 'wing', 'fin', 'tentacle'];

/**
 * The part of a creature the pipeline can build today: limbs of the roles it knows, and the
 * parts that sit on what exists. Everything left out is reported by `notBuilt`.
 */
export function buildable(spec: CreatureSpec): CreatureSpec {
  const limbs = spec.limbs.filter((l) => BUILT_ROLES.includes(l.role));
  const known = new Set(limbs.map((l) => l.id));
  const dropped = new Set(spec.limbs.filter((l) => !known.has(l.id)).map((l) => l.id));
  const parts = spec.parts.filter((p) => !dropped.has(p.on));
  // Parts on dropped parts go too, however deep.
  for (let changed = true; changed; ) {
    changed = false;
    const ids = new Set(parts.map((p) => p.id));
    for (let i = parts.length - 1; i >= 0; i--) {
      const part = parts[i];
      if (!part) continue;
      const onPart = spec.parts.some((p) => p.id === part.on);
      if (onPart && !ids.has(part.on)) {
        parts.splice(i, 1);
        changed = true;
      }
    }
  }
  if (limbs.length === spec.limbs.length && parts.length === spec.parts.length) return spec;
  return { ...spec, limbs, parts };
}
